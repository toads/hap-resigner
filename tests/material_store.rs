use std::fs;

use hap_resigner::materials::generate::GeneratedKeyMaterial;
use hap_resigner::materials::store::{MaterialStore, StoreError};

#[test]
fn stores_and_discovers_material_pairs_atomically() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());

    let pair = store
        .save_key_pair("keyA", b"p12 bytes", b"certificate bytes")
        .expect("save key pair");
    let profile = store
        .save_profile("com.example.test", b"profile bytes")
        .expect("save profile");

    assert_eq!(fs::read(&pair.p12).unwrap(), b"p12 bytes");
    assert_eq!(fs::read(&pair.certificate).unwrap(), b"certificate bytes");
    assert_eq!(fs::read(&profile).unwrap(), b"profile bytes");
    assert_eq!(store.list_key_pairs().unwrap(), vec![pair]);
}

#[test]
fn pending_key_survives_until_a_verified_pair_is_completed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    let generated = GeneratedKeyMaterial {
        identifier: "pending-key".to_owned(),
        alias: "debugKey".to_owned(),
        p12: b"pending p12".to_vec(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----pending".to_owned(),
    };

    let pending = store
        .save_pending_key(&generated, "team-1", "unique-certificate")
        .expect("save pending key");
    assert_eq!(fs::read(&pending.p12).unwrap(), b"pending p12");
    assert_eq!(pending.team_id, "team-1");
    assert_eq!(pending.certificate_name, "unique-certificate");
    assert_eq!(store.list_pending_keys().unwrap(), vec![pending.clone()]);

    let pair = store
        .save_key_pair("pending-key", b"pending p12", b"verified certificate")
        .unwrap();
    store.clear_pending_key("pending-key").unwrap();
    assert!(store.list_pending_keys().unwrap().is_empty());
    assert_eq!(fs::read(pair.certificate).unwrap(), b"verified certificate");
}

#[test]
fn rejects_identifiers_that_can_escape_the_material_directory() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());

    assert!(matches!(
        store.profile_path("../escape"),
        Err(StoreError::InvalidIdentifier)
    ));
}
