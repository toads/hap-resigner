use std::{env, fs};

use hap_resigner::hap::sign::sign_hap;
use hap_resigner::materials::load_signing_identity;
use hap_resigner::materials::profile::validate_profile;

#[test]
#[ignore = "requires REAL_HAP_* environment variables"]
fn signs_real_hap_from_environment() {
    let input_path = env::var("REAL_HAP_INPUT").expect("REAL_HAP_INPUT");
    let output_path = env::var("REAL_HAP_OUTPUT").expect("REAL_HAP_OUTPUT");
    let p12_path = env::var("REAL_HAP_P12").expect("REAL_HAP_P12");
    let cert_path = env::var("REAL_HAP_CERT").expect("REAL_HAP_CERT");
    let profile_path = env::var("REAL_HAP_PROFILE").expect("REAL_HAP_PROFILE");
    let password = env::var("REAL_HAP_PASSWORD").unwrap_or_else(|_| "123456".to_owned());

    let input = fs::read(input_path).expect("input HAP");
    let p12 = fs::read(p12_path).expect("P12");
    let cert = fs::read(cert_path).expect("certificate chain");
    let profile = fs::read(profile_path).expect("profile");
    let identity = load_signing_identity(&p12, &password, &cert).expect("signing identity");
    if let (Ok(bundle), Ok(udid)) = (env::var("REAL_HAP_BUNDLE"), env::var("REAL_HAP_UDID")) {
        validate_profile(
            &profile,
            &bundle,
            &udid,
            &identity.certificates[0],
            time::OffsetDateTime::now_utc().unix_timestamp(),
        )
        .expect("real provisioning profile");
    }
    let output = sign_hap(&input, &identity, &profile).expect("Rust signed HAP");

    fs::write(output_path, output).expect("write signed HAP");
}
