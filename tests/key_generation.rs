use hap_resigner::materials::generate::generate_key_material;
use hap_resigner::materials::validate_p12_private_key;
use p12_keystore::{KeyStore, Pkcs12ImportPolicy};

#[test]
fn generates_deterministic_named_p256_p12_and_csr() {
    let generated = generate_key_material("team-123", "123456").expect("key material");

    assert_eq!(generated.alias, "debugKey");
    assert!(generated.identifier.starts_with("auto_"));
    assert!(
        generated
            .csr_pem
            .starts_with("-----BEGIN CERTIFICATE REQUEST-----")
    );
    let key_store = KeyStore::from_pkcs12(&generated.p12, "123456", Pkcs12ImportPolicy::Strict)
        .expect("generated P12 is readable");
    assert_eq!(
        key_store.private_key_chain().map(|(alias, _)| alias),
        Some("debugKey")
    );
    validate_p12_private_key(&generated.p12, "123456").expect("P12 private key is readable");
    assert!(validate_p12_private_key(&generated.p12, "wrong-password").is_err());

    let second = generate_key_material("team-123", "another-password").expect("second key");
    assert_eq!(second.identifier, generated.identifier);
}
