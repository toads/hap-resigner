use std::error::Error as _;
use std::time::Duration;

use reqwest::blocking::{Client, ClientBuilder};

pub(crate) fn http_client_builder(timeout: Duration, user_agent: &'static str) -> ClientBuilder {
    Client::builder().timeout(timeout).user_agent(user_agent)
}

pub(crate) fn safe_transport_error(error: &reqwest::Error) -> String {
    let category = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_decode() {
        "decode"
    } else if error.is_body() {
        "body"
    } else {
        "request"
    };
    let target = error
        .url()
        .and_then(safe_origin)
        .unwrap_or_else(|| "<unknown>".to_owned());
    let sensitive_url = error.url().map(ToString::to_string);
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        let mut message = redact_embedded_urls(&cause.to_string());
        if let Some(sensitive_url) = &sensitive_url {
            message = message.replace(sensitive_url, "<redacted-url>");
        }
        let message = message.chars().take(256).collect::<String>();
        if causes.last() != Some(&message) {
            causes.push(message);
        }
        if causes.len() == 6 {
            break;
        }
        source = cause.source();
    }
    if causes.is_empty() {
        causes.push("unavailable".to_owned());
    }
    format!(
        "{category} failure; target={target}; causes: {}",
        causes.join(" -> ")
    )
}

pub(crate) fn redact_embedded_urls(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_start) = find_url_start(&input[cursor..]) {
        let start = cursor + relative_start;
        output.push_str(&input[cursor..start]);
        let candidate = &input[start..];
        let end = candidate
            .char_indices()
            .find_map(|(index, character)| {
                (index > 0 && is_url_delimiter(character)).then_some(index)
            })
            .unwrap_or(candidate.len());
        let raw_url = &candidate[..end];
        if let Ok(url) = reqwest::Url::parse(raw_url) {
            if let Some(origin) = safe_origin(&url) {
                output.push_str(&origin);
                if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
                    output.push_str("/<redacted>");
                }
            } else {
                output.push_str("<redacted-url>");
            }
        } else {
            output.push_str("<redacted-url>");
        }
        cursor = start + end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn find_url_start(input: &str) -> Option<usize> {
    [input.find("https://"), input.find("http://")]
        .into_iter()
        .flatten()
        .min()
}

fn is_url_delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '\"' | '\'' | '<' | '>' | '\\' | '(' | ')' | '[' | ']' | '{' | '}'
        )
}

fn safe_origin(url: &reqwest::Url) -> Option<String> {
    let host = url.host_str()?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use super::{redact_embedded_urls, safe_transport_error};

    #[test]
    fn transport_error_keeps_cause_but_removes_signed_url_details() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("connection");
            drop(stream);
        });
        let error = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .get(format!(
                "http://{address}/private/profile.p7b?AccessKeyId=secret&Signature=private"
            ))
            .send()
            .expect_err("closed connection must fail");
        let diagnostic = safe_transport_error(&error);
        assert!(diagnostic.contains("127.0.0.1"), "{diagnostic}");
        assert!(diagnostic.contains("cause"), "{diagnostic}");
        for secret in [
            "profile.p7b",
            "AccessKeyId",
            "Signature",
            "secret",
            "private",
        ] {
            assert!(
                !diagnostic.contains(secret),
                "leaked {secret:?}: {diagnostic}"
            );
        }
        server.join().unwrap();
    }

    #[test]
    fn embedded_obs_signed_url_is_reduced_to_its_origin() {
        let redacted = redact_embedded_urls(
            r#"response={"newUrl":"https://bucket.obs.example.com/private/cert.cer?AccessKeyId=secret&Signature=private"}"#,
        );
        assert!(redacted.contains("https://bucket.obs.example.com/<redacted>"));
        for secret in [
            "private/cert.cer",
            "AccessKeyId",
            "Signature",
            "secret",
            "private\"",
        ] {
            assert!(!redacted.contains(secret), "leaked {secret:?}: {redacted}");
        }
    }
}
