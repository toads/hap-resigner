use std::fs;

use hap_resigner::materials::profile::{ProfileError, validate_profile};

const NOW: i64 = 1_767_225_600;
const UDID: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn validates_profile_bundle_device_and_signing_certificate() {
    let p7b = fs::read("tests/fixtures/profile.p7b").expect("profile fixture");
    let leaf = fs::read("tests/fixtures/formal-leaf.der").expect("leaf fixture");

    let profile =
        validate_profile(&p7b, "com.example.test", UDID, &leaf, NOW).expect("valid profile");

    assert_eq!(profile.bundle_info.bundle_name, "com.example.test");
    assert_eq!(profile.profile_type, "debug");
}

#[test]
fn rejects_profile_for_wrong_bundle_or_device() {
    let p7b = fs::read("tests/fixtures/profile.p7b").expect("profile fixture");
    let leaf = fs::read("tests/fixtures/formal-leaf.der").expect("leaf fixture");

    assert!(matches!(
        validate_profile(&p7b, "com.wrong", UDID, &leaf, NOW),
        Err(ProfileError::BundleMismatch)
    ));
    assert!(matches!(
        validate_profile(&p7b, "com.example.test", "B", &leaf, NOW),
        Err(ProfileError::DeviceMismatch)
    ));
}

#[test]
fn rejects_tampered_profile_content() {
    let mut p7b = fs::read("tests/fixtures/profile.p7b").expect("profile fixture");
    let marker = b"com.example.test";
    let offset = p7b
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("bundle marker");
    p7b[offset] ^= 1;
    let leaf = fs::read("tests/fixtures/formal-leaf.der").expect("leaf fixture");

    assert!(matches!(
        validate_profile(&p7b, "com.example.test", UDID, &leaf, NOW),
        Err(ProfileError::CmsIntegrity)
    ));
}
