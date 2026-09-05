use std::fs;

#[cfg(feature = "agc")]
use std::cell::{Cell, RefCell};

#[cfg(feature = "agc")]
use hap_resigner::agc::client::{CertificateRecord, DeviceRecord, ProvisionRecord};
#[cfg(feature = "agc")]
use hap_resigner::materials::generate::GeneratedKeyMaterial;
use hap_resigner::materials::manager::find_local_materials;
#[cfg(feature = "agc")]
use hap_resigner::materials::manager::{
    ManagerError, MaterialApi, PrepareRequest, prepare_materials_with_api,
};
use hap_resigner::materials::store::MaterialStore;

const NOW: i64 = 1_767_225_600;
const UDID: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn scans_all_pairs_and_selects_profile_matching_private_key() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    store
        .save_key_pair("broken-first", b"not a p12", b"not a certificate")
        .unwrap();
    store
        .save_key_pair(
            "matching-second",
            &fs::read("tests/fixtures/placeholder.p12").unwrap(),
            &fs::read("tests/fixtures/formal-chain.pem").unwrap(),
        )
        .unwrap();
    store
        .save_profile(
            "com.example.test",
            &fs::read("tests/fixtures/profile.p7b").unwrap(),
        )
        .unwrap();

    let prepared = find_local_materials(&store, "com.example.test", UDID, NOW, |_| {
        vec!["123456".to_owned()]
    })
    .expect("material scan")
    .expect("matching materials");

    assert!(prepared.pair.p12.ends_with("matching-second.p12"));
    assert_eq!(prepared.password, "123456");
    assert_eq!(prepared.identity.certificates.len(), 2);
}

#[cfg(feature = "agc")]
#[test]
fn creates_and_persists_materials_when_local_pair_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    let api = FakeApi {
        certificate: fs::read("tests/fixtures/formal-chain.pem").unwrap(),
        profile: fs::read("tests/fixtures/profile.p7b").unwrap(),
    };
    let generated = GeneratedKeyMaterial {
        identifier: "generated-key".to_owned(),
        alias: "debugKey".to_owned(),
        p12: fs::read("tests/fixtures/placeholder.p12").unwrap(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----".to_owned(),
    };
    let saved_password = RefCell::new(None);
    let request = PrepareRequest {
        bundle_name: "com.example.test",
        udid: UDID,
        now_unix: NOW,
        team_id: "team-1",
        new_password: "123456",
        device_name: "test-device",
        device_type: "4",
        certificate_name: "test-certificate",
        provision_name: "test-provision",
    };

    let prepared = prepare_materials_with_api(
        &store,
        &api,
        &request,
        |_| Vec::new(),
        |_| Ok(Vec::new()),
        |_, password| {
            saved_password.replace(Some(password.to_owned()));
            Ok(())
        },
        |_, _| Ok(generated.clone()),
    )
    .expect("created materials");

    assert_eq!(prepared.password, "123456");
    assert_eq!(saved_password.into_inner().as_deref(), Some("123456"));
    assert!(prepared.pair.p12.is_file());
    assert!(prepared.pair.certificate.is_file());
    assert!(prepared.profile_path.is_file());
}

#[cfg(feature = "agc")]
#[test]
fn rejects_a_mismatched_downloaded_chain_before_persisting_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    let api = FakeApi {
        certificate: b"not the generated key certificate".to_vec(),
        profile: fs::read("tests/fixtures/profile.p7b").unwrap(),
    };
    let generated = GeneratedKeyMaterial {
        identifier: "mismatched-key".to_owned(),
        alias: "debugKey".to_owned(),
        p12: fs::read("tests/fixtures/placeholder.p12").unwrap(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----".to_owned(),
    };
    let request = PrepareRequest {
        bundle_name: "com.example.test",
        udid: UDID,
        now_unix: NOW,
        team_id: "team-1",
        new_password: "123456",
        device_name: "test-device",
        device_type: "4",
        certificate_name: "test-certificate",
        provision_name: "test-provision",
    };

    let error = match prepare_materials_with_api(
        &store,
        &api,
        &request,
        |_| Vec::new(),
        |_| Ok(Vec::new()),
        |_, _| Ok(()),
        |_, _| Ok(generated.clone()),
    ) {
        Ok(_) => panic!("mismatched chain accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("does not match"), "{error}");
    let pending = store.list_pending_keys().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(!pending[0].p12.with_extension("cer").exists());
}

#[cfg(feature = "agc")]
#[test]
fn missing_pending_password_fails_before_any_remote_request() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    let generated = GeneratedKeyMaterial {
        identifier: "pending-without-password".to_owned(),
        alias: "debugKey".to_owned(),
        p12: fs::read("tests/fixtures/placeholder.p12").unwrap(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----stable".to_owned(),
    };
    store
        .save_pending_key(&generated, "team-1", "stable-certificate-name")
        .unwrap();
    let api = RetryApi {
        certificate: fs::read("tests/fixtures/formal-chain.pem").unwrap(),
        profile: fs::read("tests/fixtures/profile.p7b").unwrap(),
        uploads: RefCell::new(Vec::new()),
        list_calls: Cell::new(0),
    };
    let request = PrepareRequest {
        bundle_name: "com.example.test",
        udid: UDID,
        now_unix: NOW,
        team_id: "team-1",
        new_password: "unused",
        device_name: "test-device",
        device_type: "4",
        certificate_name: "unused-certificate",
        provision_name: "test-provision",
    };

    let error = match prepare_materials_with_api(
        &store,
        &api,
        &request,
        |_| vec!["123456".to_owned()],
        |_| {
            Err(ManagerError::InvalidMaterial(
                "credential store unavailable".to_owned(),
            ))
        },
        |_, _| panic!("must not replace a pending password"),
        |_, _| panic!("must not generate a replacement key"),
    ) {
        Ok(_) => panic!("missing pending password accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("credential store unavailable"));
    assert_eq!(api.list_calls.get(), 0);
    assert!(api.uploads.borrow().is_empty());
}

#[cfg(feature = "agc")]
#[test]
fn upload_failure_persists_and_reuses_the_same_key_and_certificate_name() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    let api = RetryApi {
        certificate: fs::read("tests/fixtures/formal-chain.pem").unwrap(),
        profile: fs::read("tests/fixtures/profile.p7b").unwrap(),
        uploads: RefCell::new(Vec::new()),
        list_calls: Cell::new(0),
    };
    let generated = GeneratedKeyMaterial {
        identifier: "pending-generated-key".to_owned(),
        alias: "debugKey".to_owned(),
        p12: fs::read("tests/fixtures/placeholder.p12").unwrap(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----stable".to_owned(),
    };
    let request = PrepareRequest {
        bundle_name: "com.example.test",
        udid: UDID,
        now_unix: NOW,
        team_id: "team-1",
        new_password: "123456",
        device_name: "test-device",
        device_type: "4",
        certificate_name: "stable-certificate-name",
        provision_name: "test-provision",
    };
    let saved_password = RefCell::new(None::<String>);
    let generate_calls = Cell::new(0);

    let first = prepare_materials_with_api(
        &store,
        &api,
        &request,
        |_| Vec::new(),
        |_| panic!("no pending key exists on the first attempt"),
        |_, password| {
            assert!(
                store.list_pending_keys().unwrap().is_empty(),
                "pending marker became visible before its password was durable"
            );
            saved_password.replace(Some(password.to_owned()));
            Ok(())
        },
        |_, _| {
            generate_calls.set(generate_calls.get() + 1);
            Ok(generated.clone())
        },
    );
    assert!(first.is_err());
    let pending = store.list_pending_keys().expect("pending key persisted");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].certificate_name, "stable-certificate-name");
    assert!(pending[0].p12.is_file());

    let prepared = prepare_materials_with_api(
        &store,
        &api,
        &request,
        |_| {
            saved_password
                .borrow()
                .clone()
                .into_iter()
                .collect::<Vec<_>>()
        },
        |_| {
            Ok(saved_password
                .borrow()
                .clone()
                .into_iter()
                .collect::<Vec<_>>())
        },
        |_, _| panic!("pending password must not be replaced"),
        |_, _| {
            generate_calls.set(generate_calls.get() + 1);
            Ok(generated.clone())
        },
    )
    .expect("retry with pending key");

    assert!(prepared.pair.certificate.is_file());
    assert!(store.list_pending_keys().unwrap().is_empty());
    assert_eq!(generate_calls.get(), 1);
    let uploads = api.uploads.borrow();
    assert_eq!(uploads.len(), 2);
    assert_eq!(uploads[0], uploads[1]);
}

#[cfg(feature = "agc")]
#[test]
fn full_debug_certificate_capacity_stops_before_key_generation_and_cert_add() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MaterialStore::at(temp.path());
    let api = AtCapacityApi;
    let request = PrepareRequest {
        bundle_name: "com.example.test",
        udid: UDID,
        now_unix: NOW,
        team_id: "team-1",
        new_password: "unused",
        device_name: "test-device",
        device_type: "4",
        certificate_name: "must-not-be-created",
        provision_name: "test-provision",
    };

    let error = match prepare_materials_with_api(
        &store,
        &api,
        &request,
        |_| Vec::new(),
        |_| panic!("no pending key exists"),
        |_, _| panic!("must not save a generated password"),
        |_, _| panic!("must not generate a key at certificate capacity"),
    ) {
        Ok(_) => panic!("certificate capacity was ignored"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("CERTIFICATE_LIMIT_REACHED"), "{error}");
    assert!(error.contains("3"), "{error}");
    assert!(error.contains("https://developer.huawei.com/"), "{error}");
}

#[cfg(feature = "agc")]
struct AtCapacityApi;

#[cfg(feature = "agc")]
impl MaterialApi for AtCapacityApi {
    fn list_certificates(&self) -> Result<Vec<CertificateRecord>, ManagerError> {
        Ok((0..3)
            .map(|index| CertificateRecord {
                id: Some(format!("cert-{index}")),
                cert_name: format!("existing-{index}"),
                cert_type: "1".to_owned(),
                cert_url: format!("source://certificate-{index}"),
            })
            .collect())
    }

    fn upload_csr(
        &self,
        _csr_pem: &str,
        _certificate_name: &str,
    ) -> Result<CertificateRecord, ManagerError> {
        panic!("cert/add must not be called at capacity")
    }

    fn register_device(
        &self,
        _udid: &str,
        _device_type: &str,
        _device_name: &str,
    ) -> Result<DeviceRecord, ManagerError> {
        panic!("device registration must not be reached")
    }

    fn create_test_provision(
        &self,
        _certificate_ids: &[String],
        _device_ids: &[String],
        _bundle_name: &str,
        _provision_name: &str,
    ) -> Result<ProvisionRecord, ManagerError> {
        panic!("profile creation must not be reached")
    }

    fn download_object(&self, _source_url: &str) -> Result<Vec<u8>, ManagerError> {
        panic!("certificate download is unnecessary without a local key")
    }
}

#[cfg(feature = "agc")]
struct RetryApi {
    certificate: Vec<u8>,
    profile: Vec<u8>,
    uploads: RefCell<Vec<(String, String)>>,
    list_calls: Cell<usize>,
}

#[cfg(feature = "agc")]
impl MaterialApi for RetryApi {
    fn list_certificates(&self) -> Result<Vec<CertificateRecord>, ManagerError> {
        self.list_calls.set(self.list_calls.get() + 1);
        Ok(Vec::new())
    }

    fn upload_csr(
        &self,
        csr_pem: &str,
        certificate_name: &str,
    ) -> Result<CertificateRecord, ManagerError> {
        let mut uploads = self.uploads.borrow_mut();
        uploads.push((csr_pem.to_owned(), certificate_name.to_owned()));
        if uploads.len() == 1 {
            return Err(ManagerError::Remote(
                "connection reset after POST".to_owned(),
            ));
        }
        Ok(CertificateRecord {
            id: Some("cert-1".to_owned()),
            cert_url: "source://certificate".to_owned(),
            ..Default::default()
        })
    }

    fn register_device(
        &self,
        udid: &str,
        _device_type: &str,
        device_name: &str,
    ) -> Result<DeviceRecord, ManagerError> {
        Ok(DeviceRecord {
            id: Some("device-1".to_owned()),
            udid: udid.to_owned(),
            device_name: device_name.to_owned(),
        })
    }

    fn create_test_provision(
        &self,
        _certificate_ids: &[String],
        _device_ids: &[String],
        _bundle_name: &str,
        provision_name: &str,
    ) -> Result<ProvisionRecord, ManagerError> {
        Ok(ProvisionRecord {
            id: Some("provision-1".to_owned()),
            url: "source://profile".to_owned(),
            name: provision_name.to_owned(),
        })
    }

    fn download_object(&self, source_url: &str) -> Result<Vec<u8>, ManagerError> {
        match source_url {
            "source://certificate" => Ok(self.certificate.clone()),
            "source://profile" => Ok(self.profile.clone()),
            _ => Err(ManagerError::Remote("unexpected URL".to_owned())),
        }
    }
}

#[cfg(feature = "agc")]
struct FakeApi {
    certificate: Vec<u8>,
    profile: Vec<u8>,
}

#[cfg(feature = "agc")]
impl MaterialApi for FakeApi {
    fn list_certificates(&self) -> Result<Vec<CertificateRecord>, ManagerError> {
        Ok(Vec::new())
    }

    fn upload_csr(
        &self,
        _csr_pem: &str,
        _certificate_name: &str,
    ) -> Result<CertificateRecord, ManagerError> {
        Ok(CertificateRecord {
            id: Some("cert-1".to_owned()),
            cert_url: "source://certificate".to_owned(),
            ..Default::default()
        })
    }

    fn register_device(
        &self,
        udid: &str,
        _device_type: &str,
        device_name: &str,
    ) -> Result<DeviceRecord, ManagerError> {
        Ok(DeviceRecord {
            id: Some("device-1".to_owned()),
            udid: udid.to_owned(),
            device_name: device_name.to_owned(),
        })
    }

    fn create_test_provision(
        &self,
        _certificate_ids: &[String],
        _device_ids: &[String],
        _bundle_name: &str,
        provision_name: &str,
    ) -> Result<ProvisionRecord, ManagerError> {
        Ok(ProvisionRecord {
            id: Some("provision-1".to_owned()),
            url: "source://profile".to_owned(),
            name: provision_name.to_owned(),
        })
    }

    fn download_object(&self, source_url: &str) -> Result<Vec<u8>, ManagerError> {
        match source_url {
            "source://certificate" => Ok(self.certificate.clone()),
            "source://profile" => Ok(self.profile.clone()),
            _ => Err(ManagerError::Remote("unexpected URL".to_owned())),
        }
    }
}
