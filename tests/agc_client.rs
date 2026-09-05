#![cfg(feature = "agc")]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

#[cfg(all(feature = "app", target_os = "macos"))]
use hap_resigner::agc::auth::AuthClient;
use hap_resigner::agc::auth::TokenData;
use hap_resigner::agc::client::AgcClient;
#[cfg(all(feature = "app", target_os = "macos"))]
use hap_resigner::materials::store::SystemSecretStore;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

#[test]
fn calls_device_certificate_provision_and_download_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().unwrap();
    let base = format!("http://{address}");
    let download_url = format!("{base}/blob");
    let server = thread::spawn(move || {
        for step in 0..7 {
            let (mut stream, _) = listener.accept().expect("request");
            let request = read_request(&mut stream);
            if step < 6 {
                assert!(request.to_ascii_lowercase().contains("oauth2token: access"));
            }
            let (expected_path, body) = match step {
                0 => ("/api/cps/device-manage/v1/device/list", r#"{"list":[],"totalCount":0}"#.to_owned()),
                1 => {
                    assert!(request.contains(r#""udid":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA""#));
                    ("/api/cps/device-manage/v1/device/add", r#"{"ret":{"code":0},"id":"device-1","udid":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","deviceName":"test-device"}"#.to_owned())
                }
                2 => ("/api/cps/harmony-cert-manage/v1/cert/list", r#"{"certList":[{"id":"cert-1","certName":"debug","certType":"1","certUrl":"source://cert"}]}"#.to_owned()),
                3 => {
                    assert!(request.contains(r#""csr":"-----BEGIN CERTIFICATE REQUEST-----""#));
                    ("/api/cps/harmony-cert-manage/v1/cert/add", r#"{"ret":{"code":0},"id":"cert-2","certName":"new","certType":"1","certUrl":"source://new"}"#.to_owned())
                }
                4 => {
                    assert!(request.contains(r#""packageName":"com.example.test""#));
                    ("/api/cps/provision-manage/v1/ide/test/provision/add", r#"{"code":0,"provisionFileUrl":"source://profile","id":"profile-1"}"#.to_owned())
                }
                5 => ("/api/amis/app-manage/v1/objects/url/reapply", format!(r#"{{"urlsInfo":[{{"newUrl":"{download_url}"}}]}}"#)),
                6 => ("/blob", "profile-bytes".to_owned()),
                _ => unreachable!(),
            };
            assert!(request.contains(expected_path), "{request}");
            respond(&mut stream, &body, step == 6);
        }
    });

    let token = TokenData {
        access_token: "access".to_owned(),
        refresh_token: String::new(),
        user_id: "user".to_owned(),
        jwt_token: "a.b.c".to_owned(),
        team_id: "team".to_owned(),
        teams: BTreeMap::new(),
    };
    let client = AgcClient::with_base(&base, token).expect("client");
    let device = client
        .register_device(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "4",
            "test-device",
        )
        .expect("device");
    assert_eq!(device.id.as_deref(), Some("device-1"));
    assert_eq!(
        client.list_certificates().unwrap()[0].id.as_deref(),
        Some("cert-1")
    );
    assert_eq!(
        client
            .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "new")
            .unwrap()
            .id
            .as_deref(),
        Some("cert-2")
    );
    let provision = client
        .create_test_provision(
            &["cert-1".to_owned()],
            &["device-1".to_owned()],
            "com.example.test",
            "profile-name",
        )
        .expect("provision");
    assert_eq!(provision.id.as_deref(), Some("profile-1"));
    assert_eq!(
        client.download_object(&provision.url).expect("download"),
        b"profile-bytes"
    );
    server.join().unwrap();
}

#[test]
fn device_add_without_metadata_reloads_the_registered_udid() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let udid = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let server = thread::spawn(move || {
        let (mut first_list, _) = listener.accept().expect("initial device list");
        assert!(read_request(&mut first_list).contains("/device/list"));
        respond(&mut first_list, r#"{"list":[]}"#, false);

        let (mut add, _) = listener.accept().expect("device add");
        assert!(read_request(&mut add).contains("/device/add"));
        respond(&mut add, r#"{"ret":{"code":0}}"#, false);

        let (mut second_list, _) = listener.accept().expect("reloaded device list");
        assert!(read_request(&mut second_list).contains("/device/list"));
        respond(
            &mut second_list,
            &format!(
                r#"{{"list":[{{"id":"device-created","udid":"{udid}","deviceName":"test-device"}}]}}"#
            ),
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let device = client
        .register_device(udid, "4", "test-device")
        .expect("device recovered after add");
    assert_eq!(device.id.as_deref(), Some("device-created"));
    server.join().unwrap();
}

#[test]
fn incomplete_existing_device_is_reloaded_without_duplicate_add() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let udid = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let server = thread::spawn(move || {
        let (mut first_list, _) = listener.accept().expect("initial device list");
        assert!(read_request(&mut first_list).contains("/device/list"));
        respond(
            &mut first_list,
            &format!(r#"{{"list":[{{"udid":"{udid}","deviceName":"test-device"}}]}}"#),
            false,
        );

        let (mut second_list, _) = listener.accept().expect("reloaded device list");
        let request = read_request(&mut second_list);
        assert!(
            request.contains("/device/list"),
            "unexpected duplicate write: {request}"
        );
        respond(
            &mut second_list,
            &format!(
                r#"{{"list":[{{"id":"device-existing","udid":"{udid}","deviceName":"test-device"}}]}}"#
            ),
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let device = client
        .register_device(udid, "4", "test-device")
        .expect("incomplete device recovered");
    assert_eq!(device.id.as_deref(), Some("device-existing"));
    server.join().unwrap();
}

#[test]
fn duplicate_device_add_response_reloads_the_existing_udid() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let udid = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let server = thread::spawn(move || {
        let (mut first_list, _) = listener.accept().expect("initial device list");
        assert!(read_request(&mut first_list).contains("/device/list"));
        respond(&mut first_list, r#"{"list":[]}"#, false);

        let (mut add, _) = listener.accept().expect("device add");
        assert!(read_request(&mut add).contains("/device/add"));
        respond(
            &mut add,
            r#"{"ret":{"code":409,"msg":"Device already exists"}}"#,
            false,
        );

        let (mut second_list, _) = listener.accept().expect("reloaded device list");
        assert!(read_request(&mut second_list).contains("/device/list"));
        respond(
            &mut second_list,
            &format!(
                r#"{{"list":[{{"id":"device-existing","udid":"{udid}","deviceName":"test-device"}}]}}"#
            ),
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let device = client
        .register_device(udid, "4", "test-device")
        .expect("duplicate device recovered");
    assert_eq!(device.id.as_deref(), Some("device-existing"));
    server.join().unwrap();
}

#[test]
fn device_lookup_refuses_multiple_complete_records_for_one_udid() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let udid = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let server = thread::spawn(move || {
        let (mut list, _) = listener.accept().expect("device list");
        assert!(read_request(&mut list).contains("/device/list"));
        respond(
            &mut list,
            &format!(
                r#"{{"list":[{{"id":"device-one","udid":"{udid}"}},{{"id":"device-two","udid":"{udid}"}}]}}"#
            ),
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let error = client
        .register_device(udid, "4", "test-device")
        .expect_err("ambiguous device records")
        .to_string();
    assert!(error.contains("2 complete device records"), "{error}");
    server.join().unwrap();
}

#[test]
fn incomplete_device_add_is_bounded_and_does_not_leak_the_udid() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let udid = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let server = thread::spawn(move || {
        let (mut first_list, _) = listener.accept().expect("initial device list");
        assert!(read_request(&mut first_list).contains("/device/list"));
        respond(&mut first_list, r#"{"list":[]}"#, false);

        let (mut add, _) = listener.accept().expect("device add");
        assert!(read_request(&mut add).contains("/device/add"));
        respond(
            &mut add,
            &format!(r#"{{"ret":{{"code":0,"msg":"accepted for {udid}"}},"udid":"{udid}"}}"#),
            false,
        );

        for _ in 0..5 {
            let (mut list, _) = listener.accept().expect("device reload");
            assert!(read_request(&mut list).contains("/device/list"));
            respond(&mut list, r#"{"list":[]}"#, false);
        }
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let error = client
        .register_device(udid, "4", "test-device")
        .expect_err("device remains invisible")
        .to_string();
    assert!(error.contains("5 list attempts"), "{error}");
    assert!(
        error.contains("device add response did not contain a complete id"),
        "{error}"
    );
    assert!(!error.contains(udid), "UDID leaked: {error}");
    server.join().unwrap();
}

#[test]
fn empty_certificate_response_means_no_existing_certificates() {
    let (client, server) = client_with_certificate_response("application/json", "");
    assert_eq!(
        client.list_certificates().expect("empty certificate list"),
        vec![]
    );
    server.join().unwrap();
}

#[test]
fn numeric_certificate_type_is_normalized() {
    let (client, server) = client_with_certificate_response(
        "application/json",
        r#"{"certList":[{"id":"1978397109577119104","certName":"auto_debug","certObjectId":"CN/example.cer","certType":1,"status":1}]}"#,
    );
    let certificate = client
        .list_certificates()
        .expect("numeric certType")
        .remove(0);
    assert_eq!(certificate.cert_type, "1");
    assert_eq!(certificate.cert_url, "CN/example.cer");
    server.join().unwrap();
}

#[test]
fn invalid_certificate_response_preserves_safe_diagnostics() {
    let (client, server) = client_with_certificate_response(
        "text/html; charset=utf-8",
        "proxy-prefix {\"oauth2Token\":\"unknown-token\",\"csr\":\"bare-csr-secret\",\"certObjectId\":\"CN/private-object\",\"newUrl\":\"https://bucket.obs.example.com/private/profile.p7b?AccessKeyId=object-secret&Signature=private\"}",
    );
    let error = client
        .list_certificates()
        .expect_err("non-JSON certificate response must fail")
        .to_string();
    for evidence in [
        "/api/cps/harmony-cert-manage/v1/cert/list",
        "HTTP 200",
        "text/html; charset=utf-8",
        "non-JSON response body",
        "<redacted>",
    ] {
        assert!(
            error.contains(evidence),
            "missing {evidence:?} in {error:?}"
        );
    }
    for secret in [
        "proxy-prefix",
        "unknown-token",
        "bare-csr-secret",
        "CN/private-object",
        "private/profile.p7b",
        "AccessKeyId",
        "object-secret",
    ] {
        assert!(!error.contains(secret), "secret leaked in {error:?}");
    }
    server.join().unwrap();
}

#[test]
fn invalid_certificate_field_type_does_not_leak_the_rejected_value_via_serde() {
    let (client, server) = client_with_certificate_response(
        "application/json",
        r#"{"certList":[{"id":"cert-1","certName":"debug","certType":{"oauth2Token":"source-leak"},"certObjectId":"CN/private-object"}]}"#,
    );
    let error = client
        .list_certificates()
        .expect_err("object certType must fail")
        .to_string();
    assert!(error.contains("unexpected JSON shape"), "{error}");
    for secret in ["source-leak", "CN/private-object"] {
        assert!(!error.contains(secret), "serde source leaked: {error}");
    }
    server.join().unwrap();
}

#[test]
fn certificate_upload_without_metadata_reloads_the_named_certificate() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut add_stream, _) = listener.accept().expect("cert add");
        let add_request = read_request(&mut add_stream);
        assert!(add_request.contains("/api/cps/harmony-cert-manage/v1/cert/add"));
        assert!(add_request.contains(r#""certName":"new-certificate""#));
        respond(&mut add_stream, r#"{"ret":{"code":0}}"#, false);

        let (mut list_stream, _) = listener.accept().expect("cert list");
        let list_request = read_request(&mut list_stream);
        assert!(list_request.contains("/api/cps/harmony-cert-manage/v1/cert/list"));
        respond(
            &mut list_stream,
            r#"{"certList":[{"id":"cert-created","certName":"new-certificate","certType":1,"certObjectId":"CN/new.cer"}]}"#,
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let certificate = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "new-certificate")
        .expect("certificate recovered from list");
    assert_eq!(certificate.id.as_deref(), Some("cert-created"));
    assert_eq!(certificate.cert_url, "CN/new.cer");
    server.join().unwrap();
}

#[test]
fn certificate_lookup_recovers_from_a_transient_transport_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut add_stream, _) = listener.accept().expect("cert add");
        assert!(read_request(&mut add_stream).contains("/cert/add"));
        respond(&mut add_stream, r#"{"ret":{"code":0}}"#, false);

        let (mut failed_list, _) = listener.accept().expect("first cert list");
        assert!(read_request(&mut failed_list).contains("/cert/list"));
        drop(failed_list);

        let (mut recovered_list, _) = listener.accept().expect("second cert list");
        assert!(read_request(&mut recovered_list).contains("/cert/list"));
        respond(
            &mut recovered_list,
            r#"{"certList":[{"id":"cert-created","certName":"new-certificate","certType":1,"certObjectId":"CN/new.cer"}]}"#,
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let certificate = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "new-certificate")
        .expect("certificate recovered after transient list failure");
    assert_eq!(certificate.id.as_deref(), Some("cert-created"));
    server.join().unwrap();
}

#[test]
fn duplicate_certificate_name_reloads_the_existing_complete_record() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut add_stream, _) = listener.accept().expect("cert add");
        assert!(read_request(&mut add_stream).contains("/cert/add"));
        respond(
            &mut add_stream,
            r#"{"ret":{"code":409,"msg":"Certificate already exists"}}"#,
            false,
        );
        let (mut list_stream, _) = listener.accept().expect("cert list");
        assert!(read_request(&mut list_stream).contains("/cert/list"));
        respond(
            &mut list_stream,
            r#"{"certList":[{"id":"cert-existing","certName":"stable-certificate","certType":"1","certObjectId":"source://existing"}]}"#,
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let certificate = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "stable-certificate")
        .expect("duplicate name recovered");
    assert_eq!(certificate.id.as_deref(), Some("cert-existing"));
    server.join().unwrap();
}

#[test]
fn certificate_lookup_refuses_multiple_complete_records_with_the_same_name() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut add_stream, _) = listener.accept().expect("cert add");
        assert!(read_request(&mut add_stream).contains("/cert/add"));
        respond(&mut add_stream, r#"{"ret":{"code":0}}"#, false);
        let (mut list_stream, _) = listener.accept().expect("cert list");
        assert!(read_request(&mut list_stream).contains("/cert/list"));
        respond(
            &mut list_stream,
            r#"{"certList":[{"id":"cert-one","certName":"same-name","certType":"1","certObjectId":"source://one"},{"id":"cert-two","certName":"same-name","certType":"1","certObjectId":"source://two"}]}"#,
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let error = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "same-name")
        .expect_err("ambiguous certificate name")
        .to_string();
    assert!(
        error.contains("returned 2 complete certificates"),
        "{error}"
    );
    server.join().unwrap();
}

#[test]
fn incomplete_certificate_create_response_is_preserved_after_bounded_lookup() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut add_stream, _) = listener.accept().expect("cert add");
        assert!(read_request(&mut add_stream).contains("/cert/add"));
        respond(
            &mut add_stream,
            r#"{"ret":{"code":0,"msg":"accepted"},"csr":"-----BEGIN CERTIFICATE REQUEST-----private-----END CERTIFICATE REQUEST-----","certObjectId":"CN/private-object.cer","newUrl":"https://bucket.obs.example.com/private/cert.cer?AccessKeyId=secret&Signature=private","oauth2Token":"access"}"#,
            false,
        );
        for _ in 0..5 {
            let (mut list_stream, _) = listener.accept().expect("cert list");
            assert!(read_request(&mut list_stream).contains("/cert/list"));
            respond(&mut list_stream, r#"{"certList":[]}"#, false);
        }
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let error = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "new-certificate")
        .expect_err("certificate remains invisible")
        .to_string();
    for evidence in ["new-certificate", "5 list attempts", "accepted"] {
        assert!(error.contains(evidence), "missing {evidence:?}: {error}");
    }
    for secret in [
        "CERTIFICATE REQUEST",
        "CN/private-object.cer",
        "AccessKeyId",
        "\"access\"",
    ] {
        assert!(
            !error.contains(secret),
            "sensitive response leaked: {error}"
        );
    }
    server.join().unwrap();
}

#[test]
fn certificate_limit_error_contains_the_management_entry_without_retrying_upload() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("cert add");
        let request = read_request(&mut stream);
        assert!(request.contains("/api/cps/harmony-cert-manage/v1/cert/add"));
        respond(
            &mut stream,
            r#"{"ret":{"code":12345,"msg":"The number of certificates exceeds limit"}}"#,
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let error = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "new-certificate")
        .expect_err("certificate limit")
        .to_string();
    assert!(error.contains("CERTIFICATE_LIMIT_REACHED"), "{error}");
    assert!(
        error.contains("https://developer.huawei.com/consumer/cn/service/josp/agc/index.html"),
        "{error}"
    );
    server.join().unwrap();
}

#[test]
fn unknown_certificate_rejection_keeps_only_endpoint_and_safe_error_code() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("cert add");
        assert!(read_request(&mut stream).contains("/cert/add"));
        respond(
            &mut stream,
            r#"{"ret":{"code":9568322,"msg":"CSR raw-csr-secret rejected for https://obs.example/private?token=unknown-token"}}"#,
            false,
        );
    });
    let client = AgcClient::with_base(&base, test_token()).expect("client");
    let error = client
        .upload_csr("-----BEGIN CERTIFICATE REQUEST-----", "new-certificate")
        .expect_err("unknown certificate rejection")
        .to_string();
    for evidence in ["/cert/add", "9568322", "<redacted>"] {
        assert!(error.contains(evidence), "missing {evidence:?}: {error}");
    }
    for secret in ["raw-csr-secret", "unknown-token", "obs.example"] {
        assert!(!error.contains(secret), "secret leaked: {error}");
    }
    server.join().unwrap();
}

#[test]
#[cfg(all(feature = "app", target_os = "macos"))]
#[ignore = "requires a live AGC token in macOS Keychain"]
fn live_agc_strict_tls_downloads_certificate_from_obs() {
    let secrets = SystemSecretStore;
    eprintln!("LIVE_AGC_STAGE: keychain_read");
    let token_bytes = secrets.get("agc-token").expect("AGC token");
    let mut token: TokenData = serde_json::from_slice(&token_bytes).expect("token JSON");
    eprintln!("LIVE_AGC_STAGE: token_refresh");
    AuthClient::new()
        .expect("auth client")
        .refresh_token(&mut token)
        .expect("token refresh");
    secrets
        .set(
            "agc-token",
            &serde_json::to_vec(&token).expect("refreshed token JSON"),
        )
        .expect("save refreshed token");
    eprintln!("LIVE_AGC_STAGE: certificate_list");
    let client = AgcClient::new(token).expect("AGC client");
    let certificate = client
        .list_certificates()
        .expect("certificate list")
        .into_iter()
        .find(|certificate| !certificate.cert_url.is_empty())
        .expect("downloadable certificate");
    eprintln!("LIVE_AGC_STAGE: obs_download");
    let certificate_bytes = client
        .download_object(&certificate.cert_url)
        .expect("OBS certificate download");
    eprintln!("LIVE_AGC_STAGE: complete");
    assert!(!certificate_bytes.is_empty());
    assert!(
        pem::parse_many(&certificate_bytes)
            .expect("certificate PEM")
            .iter()
            .any(|block| block.tag() == "CERTIFICATE")
    );
}

#[test]
fn strict_client_rejects_a_self_signed_server() {
    let (base, server) = self_signed_certificate_server();
    let client = AgcClient::with_base(&base, test_token()).expect("strict client");
    assert!(client.list_certificates().is_err());
    server.join().unwrap();
}

fn self_signed_certificate_server() -> (String, thread::JoinHandle<()>) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(["127.0.0.1".to_owned()]).expect("certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key)
        .expect("TLS server config");
    let listener = TcpListener::bind("127.0.0.1:0").expect("TLS listener");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let connection = ServerConnection::new(Arc::new(config)).expect("TLS connection");
        let mut stream = StreamOwned::new(connection, stream);
        let mut request = [0_u8; 4096];
        if stream.read(&mut request).is_err() {
            return;
        }
        let body = r#"{"certList":[]}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.flush();
    });
    (format!("https://{address}"), server)
}

fn test_token() -> TokenData {
    TokenData {
        access_token: "access".to_owned(),
        refresh_token: String::new(),
        user_id: "user".to_owned(),
        jwt_token: "a.b.c".to_owned(),
        team_id: "team".to_owned(),
        teams: BTreeMap::new(),
    }
}

fn client_with_certificate_response(
    content_type: &'static str,
    body: &'static str,
) -> (AgcClient, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request");
        let request = read_request(&mut stream);
        assert!(request.contains("/api/cps/harmony-cert-manage/v1/cert/list"));
        respond_with(&mut stream, 200, "OK", content_type, body);
    });
    let token = TokenData {
        access_token: "access".to_owned(),
        refresh_token: String::new(),
        user_id: "user".to_owned(),
        jwt_token: "a.b.c".to_owned(),
        team_id: "team".to_owned(),
        teams: BTreeMap::new(),
    };
    (AgcClient::with_base(&base, token).expect("client"), server)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let size = stream.read(&mut buffer).expect("read");
        if size == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..size]);
        let Some(header_end) = data.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&data[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| line.split_once(':'))
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if data.len() >= header_end + content_length {
            break;
        }
    }
    String::from_utf8(data).expect("UTF-8 request")
}

fn respond(stream: &mut TcpStream, body: &str, binary: bool) {
    let content_type = if binary {
        "application/octet-stream"
    } else {
        "application/json"
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("response");
}

fn respond_with(stream: &mut TcpStream, status: u16, reason: &str, content_type: &str, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("response");
}
