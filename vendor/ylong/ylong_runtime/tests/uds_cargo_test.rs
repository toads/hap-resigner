// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This test can only run in cargo.

#![cfg(all(target_family = "unix", feature = "net"))]

use std::os::fd::{AsFd, AsRawFd};

use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};
use ylong_runtime::net::{UnixDatagram, UnixListener, UnixStream};

/// Uds UnixListener test case.
///
/// # Brief
/// 1. Create a std UnixListener with `bind()`.
/// 2. Convert std UnixListener to Ylong_runtime UnixListener.
/// 3. Check result is correct.
#[test]
fn sdv_uds_listener_baisc_test() {
    const PATH: &str = "/tmp/uds_listener_path1";
    let _ = std::fs::remove_file(PATH);
    let listener = std::os::unix::net::UnixListener::bind(PATH).unwrap();
    let handle = ylong_runtime::spawn(async {
        let res = UnixListener::from_std(listener);
        assert!(res.is_ok());
        let listener = res.unwrap();
        assert!(listener.as_fd().as_raw_fd() >= 0);
        assert!(listener.as_raw_fd() >= 0);
        assert!(listener.take_error().is_ok());
    });
    ylong_runtime::block_on(handle).unwrap();
    let _ = std::fs::remove_file(PATH);
}

/// Uds UnixListener test case.
///
/// # Brief
/// 1. Create a server with `bind()` and `accept()`.
/// 2. Create a client with `connect()`.
/// 3. Server Sends message and client recv it.
#[test]
fn sdv_uds_listener_read_write_test() {
    const PATH: &str = "/tmp/uds_listener_path2";
    let _ = std::fs::remove_file(PATH);
    let client_msg = "hello client";
    let server_msg = "hello server";

    ylong_runtime::block_on(async {
        let mut read_buf = [0_u8; 12];
        let listener = UnixListener::bind(PATH).unwrap();

        let handle = ylong_runtime::spawn(async {
            let mut stream = UnixStream::connect(PATH).await;
            while stream.is_err() {
                stream = UnixStream::connect(PATH).await;
            }
            let mut stream = stream.unwrap();
            let mut read_buf = [0_u8; 12];
            stream.read_exact(&mut read_buf).await.unwrap();
            assert_eq!(
                std::str::from_utf8(&read_buf).unwrap(),
                client_msg.to_string()
            );
            stream.write_all(server_msg.as_bytes()).await.unwrap();
        });

        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(client_msg.as_bytes()).await.unwrap();

        stream.read_exact(&mut read_buf).await.unwrap();
        assert_eq!(
            std::str::from_utf8(&read_buf).unwrap(),
            server_msg.to_string()
        );

        handle.await.unwrap();
    });

    let _ = std::fs::remove_file(PATH);
}

/// uds UnixListener/UnixStream test case.
///
/// # Title
/// sdv_uds_stream_test
///
/// # Brief
/// 1. Creates a server and a client.
/// 2. Sends and writes message to each other.
///
/// # Note
/// Each execution will leave a file under PATH which must be deleted,
/// otherwise the next bind operation will fail.
#[test]
fn sdv_uds_stream_test() {
    const PATH: &str = "/tmp/uds_path1";
    let _ = std::fs::remove_file(PATH);

    async fn server() {
        let mut read_buf = [0_u8; 12];
        let listener = UnixListener::bind(PATH).unwrap();
        let (mut stream, _) = listener.accept().await.unwrap();
        let n = stream.write(b"hello client").await.unwrap();
        assert_eq!(n, "hello client".len());
        let n = stream.read(&mut read_buf).await.unwrap();
        assert_eq!(n, "hello server".len());
        assert_eq!(
            std::str::from_utf8(&read_buf).unwrap(),
            "hello server".to_string()
        );
    }

    async fn client() {
        let mut read_buf = [0_u8; 12];
        loop {
            if let Ok(mut stream) = UnixStream::connect(PATH).await {
                let n = stream.read(&mut read_buf).await.unwrap();
                assert_eq!(n, "hello server".len());
                assert_eq!(
                    std::str::from_utf8(&read_buf).unwrap(),
                    "hello client".to_string()
                );

                let n = stream.write(b"hello server").await.unwrap();
                assert_eq!(n, "hello client".len());
                break;
            }
        }
    }

    let handle = ylong_runtime::spawn(client());
    ylong_runtime::block_on(server());
    ylong_runtime::block_on(handle).unwrap();

    std::fs::remove_file(PATH).unwrap();
}

/// uds UnixDatagram test case.
///
/// # Title
/// sdv_uds_datagram_test
///
/// # Brief
/// 1. Creates a server and a client.
/// 2. Client Sends message and server recv it.
///
/// # Note
/// Each execution will leave a file under PATH which must be deleted,
/// otherwise the next bind operation will fail.
#[test]
fn sdv_uds_datagram_test() {
    const PATH: &str = "/tmp/uds_path2";
    let _ = std::fs::remove_file(PATH);

    async fn server() {
        let socket = UnixDatagram::bind(PATH).unwrap();

        let mut buf = vec![0; 11];
        socket.recv(buf.as_mut_slice()).await.expect("recv failed");
        assert_eq!(
            std::str::from_utf8(&buf).unwrap(),
            "hello world".to_string()
        );
    }

    async fn client() {
        let socket = UnixDatagram::unbound().unwrap();
        loop {
            if socket.connect(PATH).is_ok() {
                socket.send(b"hello world").await.expect("send failed");
                break;
            };
        }
    }

    let handle = ylong_runtime::spawn(client());
    ylong_runtime::block_on(server());
    ylong_runtime::block_on(handle).unwrap();

    std::fs::remove_file(PATH).unwrap();
}
