mod support;

use std::fs;

use hap_resigner::cli::{SignOptions, sign_file};
use hap_resigner::hap::format::{TYPE_PROFILE, TYPE_SIGNER, parse_hap};

#[test]
fn sign_command_writes_a_parseable_hap() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.hap");
    let output = temp.path().join("output.hap");
    let profile = temp.path().join("profile.p7b");
    fs::write(&input, support::empty_zip()).unwrap();
    fs::write(&profile, b"profile").unwrap();

    sign_file(&SignOptions {
        input,
        output: output.clone(),
        p12: "tests/fixtures/placeholder.p12".into(),
        certificate: "tests/fixtures/formal-chain.pem".into(),
        profile,
        password: "123456".to_owned(),
    })
    .expect("CLI sign");

    let data = fs::read(output).unwrap();
    let layout = parse_hap(&data).unwrap();
    let block = layout.signing_block.unwrap();
    assert_eq!(
        block.block_value(&data, TYPE_PROFILE),
        Some(b"profile".as_slice())
    );
    assert!(block.block_value(&data, TYPE_SIGNER).is_some());
}
