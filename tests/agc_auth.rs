#![cfg(feature = "agc")]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use hap_resigner::agc::auth::{AuthClient, TokenData, parse_temp_token};

#[test]
fn parses_temp_token_and_cn_site_suffix() {
    let parsed = parse_temp_token("actual.jwt.token&site=1").expect("temp token");

    assert_eq!(parsed.actual_token, "actual.jwt.token");
    assert_eq!(parsed.site_code, "CN");
}

#[test]
fn refreshes_access_token_with_stored_jwt() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request");
        let mut request = [0_u8; 4096];
        let size = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..size]).to_ascii_lowercase();
        assert!(request.contains("/authrouter/auth/api/jwtoken/check"));
        assert!(request.contains("jwttoken: old.jwt.token"));
        assert!(request.contains("refresh: true"));
        let body = r#"{"status":true,"userInfo":{"accessToken":"new-access","refreshToken":"new-refresh","userId":"user-1"}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write response");
    });

    let mut token = TokenData {
        access_token: "old-access".to_owned(),
        refresh_token: "old-refresh".to_owned(),
        user_id: "user-1".to_owned(),
        jwt_token: "old.jwt.token".to_owned(),
        team_id: "team-1".to_owned(),
        teams: BTreeMap::new(),
    };
    let base = format!("http://{address}");
    let client = AuthClient::with_bases(&base, &base).expect("client");

    client.refresh_token(&mut token).expect("refresh");
    server.join().unwrap();

    assert_eq!(token.access_token, "new-access");
    assert_eq!(token.refresh_token, "new-refresh");
}
