#[macro_use]
extern crate lazy_static;

pub use hdc::config;

mod auth;
mod client;
mod host_app;
mod logger;
mod parser;
mod server;
mod task;
mod translate;
mod tty_utility;

pub use client::run_client_mode;
pub use parser::{extract_global_params, split_opt_and_cmd, ParsedCommand};

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::LazyLock;
use std::thread;
use std::time::Duration;

const SYSTEM_HDC_ADDRESS: &str = "127.0.0.1:8710";

#[derive(Debug, Clone, PartialEq, Eq)]
struct HdcServer {
    address: String,
}

static HDC_SERVER: LazyLock<Result<HdcServer, String>> = LazyLock::new(start_hdc_server);

pub fn run_command(arguments: &[&str]) -> io::Result<String> {
    let selected_server = match &*HDC_SERVER {
        Ok(server) => server,
        Err(error) => return Err(io::Error::other(error.clone())),
    };
    let parsed = parser::split_opt_and_cmd(
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
    );
    let mut command = parser::extract_global_params(parsed.options)?;
    command.server_addr = selected_server.address.clone();
    command.command = parsed.command;
    command.parameters = parsed.parameters;
    command.launch_server = false;
    if command.command.is_none() {
        return Err(io::Error::other("unknown HDC command"));
    }
    if !hdc::utils::begin_output_capture() {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "another inline HDC command is running",
        ));
    }
    let result = ylong_runtime_static::block_on(client::run_client_mode(command));
    let output = hdc::utils::finish_output_capture();
    if let Err(error) = result {
        if output.is_empty() {
            return Err(error);
        }
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}

fn start_hdc_server() -> Result<HdcServer, String> {
    let inline_address = format!("127.0.0.1:{}", hdc::config::SERVER_DEFAULT_PORT);
    select_hdc_server(SYSTEM_HDC_ADDRESS, &inline_address)
}

fn select_hdc_server(external_address: &str, inline_address: &str) -> Result<HdcServer, String> {
    match ylong_runtime_static::builder::RuntimeBuilder::new_multi_thread()
        .worker_num(4)
        .worker_stack_size(4 * 1024 * 1024)
        .build_global()
    {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
    if server_is_listening(external_address)? {
        return Ok(HdcServer {
            address: external_address.to_owned(),
        });
    }
    if server_is_listening(inline_address)? {
        return Ok(HdcServer {
            address: inline_address.to_owned(),
        });
    }
    let server_address = inline_address.to_owned();
    ylong_runtime_static::spawn(async move {
        if let Err(error) = server::run_server_mode(server_address).await {
            eprintln!("INLINE_HDC_SERVER_FAILED: {error}");
        }
    });
    thread::sleep(Duration::from_millis(500));
    if !server_is_listening(inline_address)? {
        return Err(format!(
            "inline HDC server did not start on {inline_address}"
        ));
    }
    Ok(HdcServer {
        address: inline_address.to_owned(),
    })
}

fn server_is_listening(address: &str) -> Result<bool, String> {
    let socket = address
        .parse::<SocketAddr>()
        .map_err(|error| format!("invalid HDC server address {address}: {error}"))?;
    Ok(TcpStream::connect_timeout(&socket, Duration::from_millis(250)).is_ok())
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::select_hdc_server;

    #[test]
    fn prefers_an_existing_external_hdc_server_on_a_different_port() {
        let external_listener = TcpListener::bind("127.0.0.1:0").expect("external listener");
        let external_address = external_listener.local_addr().unwrap().to_string();
        let inline_address = unused_local_address();
        let selected = select_hdc_server(&external_address, &inline_address).expect("server");
        assert_eq!(selected.address, external_address);
    }

    fn unused_local_address() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("unused listener");
        listener.local_addr().unwrap().to_string()
    }
}
