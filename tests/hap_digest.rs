mod support;

use hap_resigner::hap::digest::compute_content_digest;
use hap_resigner::hap::format::{TYPE_PROFILE, TYPE_PROPERTY, TYPE_SIGNER, parse_hap};
use hap_resigner::hap::signing_block::{BlockValue, preserved_optional_blocks};

#[test]
fn matches_reference_chunked_sha256_vector() {
    let digest = compute_content_digest(
        &[b"abc".as_slice(), b"def".as_slice(), b"ghi".as_slice()],
        &[b"property".as_slice(), b"profile".as_slice()],
    );

    assert_eq!(
        digest,
        [
            0xa7, 0x10, 0x8c, 0xa7, 0x41, 0x39, 0xca, 0x02, 0xd7, 0xed, 0x6c, 0xcb, 0x76, 0x7a,
            0x69, 0xcc, 0xc2, 0x09, 0xb8, 0x95, 0x83, 0x06, 0xf2, 0xe9, 0xbd, 0xd5, 0xdc, 0xd4,
            0x88, 0xf9, 0x90, 0xa7,
        ]
    );
}

#[test]
fn preserves_only_property_from_existing_signature() {
    let property = b"code-sign property";
    let unsigned = support::empty_zip();
    let block = support::signing_block(&[
        (TYPE_PROPERTY, property),
        (TYPE_PROFILE, b"old profile"),
        (TYPE_SIGNER, b"old signer"),
    ]);
    let signed = support::insert_before_central_directory(&unsigned, &block);
    let layout = parse_hap(&signed).expect("valid HAP");

    let preserved = preserved_optional_blocks(&signed, layout.signing_block.as_ref())
        .expect("valid property block");

    assert_eq!(
        preserved,
        vec![BlockValue {
            block_type: TYPE_PROPERTY,
            value: property.to_vec(),
        }]
    );
}
