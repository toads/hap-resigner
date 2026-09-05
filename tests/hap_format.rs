mod support;

use hap_resigner::hap::format::{TYPE_PROFILE, TYPE_PROPERTY, TYPE_SIGNER, parse_hap};

#[test]
fn parses_existing_v3_signing_block_and_values() {
    let property = b"code-sign property";
    let profile = b"profile bytes";
    let signer = b"cms signer";
    let unsigned = support::empty_zip();
    let block = support::signing_block(&[
        (TYPE_PROPERTY, property),
        (TYPE_PROFILE, profile),
        (TYPE_SIGNER, signer),
    ]);
    let signed = support::insert_before_central_directory(&unsigned, &block);

    let layout = parse_hap(&signed).expect("valid signed HAP");

    let signing = layout.signing_block.expect("signing block");
    assert_eq!(signing.start, 0);
    assert_eq!(signing.size, block.len());
    assert_eq!(signing.version, 3);
    assert_eq!(
        signing.block_value(&signed, TYPE_PROPERTY),
        Some(property.as_slice())
    );
    assert_eq!(
        signing.block_value(&signed, TYPE_PROFILE),
        Some(profile.as_slice())
    );
    assert_eq!(
        signing.block_value(&signed, TYPE_SIGNER),
        Some(signer.as_slice())
    );
    assert_eq!(layout.central_directory_offset, block.len());
    assert_eq!(layout.central_directory_size, 0);
}

#[test]
fn parses_unsigned_zip_without_signing_block() {
    let unsigned = support::empty_zip();

    let layout = parse_hap(&unsigned).expect("valid unsigned ZIP");

    assert!(layout.signing_block.is_none());
    assert_eq!(layout.central_directory_offset, 0);
    assert_eq!(layout.central_directory_size, 0);
}
