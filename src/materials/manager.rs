use std::fs;
use std::path::{Path, PathBuf};

#[cfg(feature = "agc")]
use super::generate::GeneratedKeyMaterial;
#[cfg(feature = "agc")]
use crate::agc::client::{
    AgcClient, CERTIFICATE_MANAGEMENT_URL, CertificateRecord, DeviceRecord, ProvisionRecord,
};
use thiserror::Error;

use super::profile::{ProvisionProfile, validate_profile};
use super::store::{KeyPairFiles, MaterialStore, StoreError};
use super::{SigningIdentity, load_signing_identity, validate_p12_private_key};

#[cfg(feature = "agc")]
const AGC_DEBUG_CERTIFICATE_LIMIT: usize = 3;

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("material I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote material operation failed: {0}")]
    Remote(String),
    #[error("generated material is invalid: {0}")]
    InvalidMaterial(String),
    #[error(
        "CERTIFICATE_LIMIT_REACHED: AGC 当前已有 {count} 张调试证书，创建前请先前往 AGC 管理并废除不再使用的证书：{management_url}"
    )]
    CertificateLimit {
        count: usize,
        management_url: &'static str,
    },
}

pub struct PreparedMaterials {
    pub pair: KeyPairFiles,
    pub profile_path: PathBuf,
    pub profile: Vec<u8>,
    pub password: String,
    pub identity: SigningIdentity,
    pub provision: ProvisionProfile,
}

pub fn find_local_materials<F>(
    store: &MaterialStore,
    bundle_name: &str,
    udid: &str,
    now_unix: i64,
    mut password_candidates: F,
) -> Result<Option<PreparedMaterials>, ManagerError>
where
    F: FnMut(&Path) -> Vec<String>,
{
    let profile_path = store.profile_path(bundle_name)?;
    if !profile_path.is_file() {
        return Ok(None);
    }
    let profile = fs::read(&profile_path)?;

    for pair in store.list_key_pairs()? {
        let Ok(p12) = fs::read(&pair.p12) else {
            continue;
        };
        let Ok(certificate) = fs::read(&pair.certificate) else {
            continue;
        };
        for password in password_candidates(&pair.p12) {
            let Ok(identity) = load_signing_identity(&p12, &password, &certificate) else {
                continue;
            };
            let Some(signer_certificate) = identity.certificates.first() else {
                continue;
            };
            let Ok(provision) =
                validate_profile(&profile, bundle_name, udid, signer_certificate, now_unix)
            else {
                continue;
            };
            return Ok(Some(PreparedMaterials {
                pair,
                profile_path,
                profile,
                password,
                identity,
                provision,
            }));
        }
    }
    Ok(None)
}

#[cfg(feature = "agc")]
pub struct PrepareRequest<'a> {
    pub bundle_name: &'a str,
    pub udid: &'a str,
    pub now_unix: i64,
    pub team_id: &'a str,
    pub new_password: &'a str,
    pub device_name: &'a str,
    pub device_type: &'a str,
    pub certificate_name: &'a str,
    pub provision_name: &'a str,
}

#[cfg(feature = "agc")]
pub trait MaterialApi {
    fn list_certificates(&self) -> Result<Vec<CertificateRecord>, ManagerError>;
    fn upload_csr(
        &self,
        csr_pem: &str,
        certificate_name: &str,
    ) -> Result<CertificateRecord, ManagerError>;
    fn register_device(
        &self,
        udid: &str,
        device_type: &str,
        device_name: &str,
    ) -> Result<DeviceRecord, ManagerError>;
    fn create_test_provision(
        &self,
        certificate_ids: &[String],
        device_ids: &[String],
        bundle_name: &str,
        provision_name: &str,
    ) -> Result<ProvisionRecord, ManagerError>;
    fn download_object(&self, source_url: &str) -> Result<Vec<u8>, ManagerError>;
}

#[cfg(feature = "agc")]
impl MaterialApi for AgcClient {
    fn list_certificates(&self) -> Result<Vec<CertificateRecord>, ManagerError> {
        AgcClient::list_certificates(self).map_err(|error| ManagerError::Remote(error.to_string()))
    }

    fn upload_csr(
        &self,
        csr_pem: &str,
        certificate_name: &str,
    ) -> Result<CertificateRecord, ManagerError> {
        AgcClient::upload_csr(self, csr_pem, certificate_name)
            .map_err(|error| ManagerError::Remote(error.to_string()))
    }

    fn register_device(
        &self,
        udid: &str,
        device_type: &str,
        device_name: &str,
    ) -> Result<DeviceRecord, ManagerError> {
        AgcClient::register_device(self, udid, device_type, device_name)
            .map_err(|error| ManagerError::Remote(error.to_string()))
    }

    fn create_test_provision(
        &self,
        certificate_ids: &[String],
        device_ids: &[String],
        bundle_name: &str,
        provision_name: &str,
    ) -> Result<ProvisionRecord, ManagerError> {
        AgcClient::create_test_provision(
            self,
            certificate_ids,
            device_ids,
            bundle_name,
            provision_name,
        )
        .map_err(|error| ManagerError::Remote(error.to_string()))
    }

    fn download_object(&self, source_url: &str) -> Result<Vec<u8>, ManagerError> {
        AgcClient::download_object(self, source_url)
            .map_err(|error| ManagerError::Remote(error.to_string()))
    }
}

#[cfg(feature = "agc")]
pub fn prepare_materials_with_api<A, Passwords, PendingPasswords, SavePassword, Generate>(
    store: &MaterialStore,
    api: &A,
    request: &PrepareRequest<'_>,
    mut password_candidates: Passwords,
    mut pending_password_candidates: PendingPasswords,
    mut save_password: SavePassword,
    mut generate: Generate,
) -> Result<PreparedMaterials, ManagerError>
where
    A: MaterialApi,
    Passwords: FnMut(&Path) -> Vec<String>,
    PendingPasswords: FnMut(&Path) -> Result<Vec<String>, ManagerError>,
    SavePassword: FnMut(&Path, &str) -> Result<(), ManagerError>,
    Generate: FnMut(&str, &str) -> Result<GeneratedKeyMaterial, ManagerError>,
{
    if let Some(materials) = find_local_materials(
        store,
        request.bundle_name,
        request.udid,
        request.now_unix,
        |path| password_candidates(path),
    )? {
        return Ok(materials);
    }

    let pairs = store.list_key_pairs()?;
    let pending_keys = store
        .list_pending_keys()?
        .into_iter()
        .filter(|pending| pending.team_id == request.team_id)
        .collect::<Vec<_>>();
    if pending_keys.len() > 1 {
        return Err(ManagerError::InvalidMaterial(format!(
            "multiple pending keys exist for team {}",
            request.team_id
        )));
    }

    let validated_pending = if let Some(pending) = pending_keys.first() {
        let p12 = fs::read(&pending.p12)?;
        let password = pending_password_candidates(&pending.p12)?
            .into_iter()
            .find(|password| validate_p12_private_key(&p12, password).is_ok())
            .ok_or_else(|| {
                ManagerError::InvalidMaterial(
                    "pending P12 password is unavailable or cannot unlock its private key"
                        .to_owned(),
                )
            })?;
        Some((pending.clone(), p12, password))
    } else {
        None
    };

    let mut candidates = Vec::with_capacity(pairs.len() + usize::from(validated_pending.is_some()));
    for pair in pairs {
        let Some(identifier) = pair
            .p12
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let Ok(p12) = fs::read(&pair.p12) else {
            continue;
        };
        let passwords = password_candidates(&pair.p12);
        candidates.push((identifier, pair.p12, p12, passwords));
    }
    if let Some((pending, p12, password)) = &validated_pending {
        if let Some((_, _, _, passwords)) = candidates
            .iter_mut()
            .find(|(_, path, _, _)| path == &pending.p12)
        {
            *passwords = vec![password.clone()];
        } else {
            candidates.push((
                pending.identifier.clone(),
                pending.p12.clone(),
                p12.clone(),
                vec![password.clone()],
            ));
        }
    }

    let certificates = api.list_certificates()?;
    let debug_certificate_count = certificates
        .iter()
        .filter(|certificate| certificate.cert_type == "1")
        .count();
    let mut selected = None;
    'certificates: for certificate in certificates {
        if (!certificate.cert_type.is_empty() && certificate.cert_type != "1")
            || certificate.cert_url.is_empty()
            || certificate.id.is_none()
        {
            continue;
        }
        if candidates.is_empty() {
            continue;
        }
        let chain = api.download_object(&certificate.cert_url)?;
        for (identifier, _p12_path, p12, passwords) in &candidates {
            for password in passwords {
                if let Ok(identity) = load_signing_identity(p12, password, &chain) {
                    selected = Some((
                        identifier.clone(),
                        p12.clone(),
                        chain,
                        password.clone(),
                        identity,
                        certificate,
                    ));
                    break 'certificates;
                }
            }
        }
    }
    if selected.is_none() && debug_certificate_count >= AGC_DEBUG_CERTIFICATE_LIMIT {
        return Err(ManagerError::CertificateLimit {
            count: debug_certificate_count,
            management_url: CERTIFICATE_MANAGEMENT_URL,
        });
    }

    let (pair, password, identity, certificate) =
        if let Some((identifier, p12, chain, password, identity, certificate)) = selected {
            let pair = store.save_key_pair(&identifier, &p12, &chain)?;
            store.clear_pending_key(&identifier)?;
            (pair, password, identity, certificate)
        } else {
            let (pending, p12, passwords) =
                if let Some((pending, p12, password)) = validated_pending {
                    (pending, p12, vec![password])
                } else {
                    let generated = generate(request.team_id, request.new_password)?;
                    let p12_path = store.p12_path(&generated.identifier)?;
                    save_password(&p12_path, request.new_password)?;
                    let pending = store.save_pending_key(
                        &generated,
                        request.team_id,
                        request.certificate_name,
                    )?;
                    let p12 = generated.p12;
                    (pending, p12, vec![request.new_password.to_owned()])
                };

            let certificate = api.upload_csr(&pending.csr_pem, &pending.certificate_name)?;
            let certificate_url = certificate.cert_url.as_str();
            if certificate.id.is_none() || certificate_url.is_empty() {
                return Err(ManagerError::InvalidMaterial(
                    "certificate response has no id or URL".to_owned(),
                ));
            }
            let chain = api.download_object(certificate_url)?;
            let (password, identity) = passwords
                .into_iter()
                .find_map(|password| {
                    load_signing_identity(&p12, &password, &chain)
                        .ok()
                        .map(|identity| (password, identity))
                })
                .ok_or_else(|| {
                    ManagerError::InvalidMaterial(
                        "downloaded certificate does not match the pending private key".to_owned(),
                    )
                })?;
            let pair = store.save_key_pair(&pending.identifier, &p12, &chain)?;
            store.clear_pending_key(&pending.identifier)?;
            (pair, password, identity, certificate)
        };

    let certificate_id = certificate
        .id
        .ok_or_else(|| ManagerError::InvalidMaterial("certificate id is missing".to_owned()))?;
    let device = api.register_device(request.udid, request.device_type, request.device_name)?;
    let device_id = device
        .id
        .ok_or_else(|| ManagerError::InvalidMaterial("device id is missing".to_owned()))?;
    let provision = api.create_test_provision(
        &[certificate_id],
        &[device_id],
        request.bundle_name,
        request.provision_name,
    )?;
    let profile = api.download_object(&provision.url)?;
    let profile_path = store.save_profile(request.bundle_name, &profile)?;
    let signer_certificate = identity
        .certificates
        .first()
        .ok_or_else(|| ManagerError::InvalidMaterial("signer certificate is missing".to_owned()))?;
    let provision = validate_profile(
        &profile,
        request.bundle_name,
        request.udid,
        signer_certificate,
        request.now_unix,
    )
    .map_err(|error| ManagerError::InvalidMaterial(error.to_string()))?;

    Ok(PreparedMaterials {
        pair,
        profile_path,
        profile,
        password,
        identity,
        provision,
    })
}
