use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};

use super::generate::GeneratedKeyMaterial;
use thiserror::Error;

#[cfg(feature = "app")]
const SECRET_SERVICE: &str = "ohos-hap-resigner";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("invalid material identifier")]
    InvalidIdentifier,
    #[error("material I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("system credential store failed: {0}")]
    Credential(String),
    #[error("platform data directory is unavailable")]
    DataDirectoryUnavailable,
    #[error("invalid pending material metadata: {0}")]
    InvalidPending(String),
    #[error("pending material metadata failed: {0}")]
    Metadata(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPairFiles {
    pub p12: PathBuf,
    pub certificate: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingKeyMaterial {
    pub identifier: String,
    pub alias: String,
    pub team_id: String,
    pub certificate_name: String,
    pub csr_pem: String,
    pub p12: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PendingKeyRecord {
    identifier: String,
    alias: String,
    team_id: String,
    certificate_name: String,
    csr_pem: String,
}

#[derive(Debug, Clone)]
pub struct MaterialStore {
    root: PathBuf,
}

impl MaterialStore {
    pub fn at(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    #[cfg(feature = "app")]
    pub fn system() -> Result<Self, StoreError> {
        let dirs = directories::ProjectDirs::from("com", "OhCodesec", "HAP Resigner")
            .ok_or(StoreError::DataDirectoryUnavailable)?;
        Ok(Self::at(dirs.data_local_dir()))
    }

    pub fn materials_dir(&self) -> PathBuf {
        self.root.join("materials")
    }

    pub fn p12_path(&self, identifier: &str) -> Result<PathBuf, StoreError> {
        validate_identifier(identifier)?;
        Ok(self.materials_dir().join(format!("{identifier}.p12")))
    }

    pub fn profile_path(&self, bundle_name: &str) -> Result<PathBuf, StoreError> {
        validate_identifier(bundle_name)?;
        Ok(self
            .materials_dir()
            .join(format!("profile_{bundle_name}.p7b")))
    }

    pub fn save_profile(&self, bundle_name: &str, profile: &[u8]) -> Result<PathBuf, StoreError> {
        let path = self.profile_path(bundle_name)?;
        atomic_write(&path, profile)?;
        Ok(path)
    }

    pub fn save_key_pair(
        &self,
        identifier: &str,
        p12: &[u8],
        certificate: &[u8],
    ) -> Result<KeyPairFiles, StoreError> {
        let materials = self.materials_dir();
        let pair = KeyPairFiles {
            p12: self.p12_path(identifier)?,
            certificate: materials.join(format!("{identifier}.cer")),
        };
        atomic_write(&pair.p12, p12)?;
        atomic_write(&pair.certificate, certificate)?;
        Ok(pair)
    }

    pub fn save_pending_key(
        &self,
        generated: &GeneratedKeyMaterial,
        team_id: &str,
        certificate_name: &str,
    ) -> Result<PendingKeyMaterial, StoreError> {
        let p12 = self.p12_path(&generated.identifier)?;
        let record = PendingKeyRecord {
            identifier: generated.identifier.clone(),
            alias: generated.alias.clone(),
            team_id: team_id.to_owned(),
            certificate_name: certificate_name.to_owned(),
            csr_pem: generated.csr_pem.clone(),
        };
        atomic_write(&p12, &generated.p12)?;
        atomic_write(
            &self.pending_path(&generated.identifier)?,
            &serde_json::to_vec(&record)?,
        )?;
        Ok(PendingKeyMaterial {
            identifier: record.identifier,
            alias: record.alias,
            team_id: record.team_id,
            certificate_name: record.certificate_name,
            csr_pem: record.csr_pem,
            p12,
        })
    }

    pub fn list_pending_keys(&self) -> Result<Vec<PendingKeyMaterial>, StoreError> {
        let materials = self.materials_dir();
        if !materials.exists() {
            return Ok(Vec::new());
        }
        let mut pending = Vec::new();
        for entry in fs::read_dir(&materials)? {
            let path = entry?.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(identifier) = file_name.strip_suffix(".pending.json") else {
                continue;
            };
            validate_identifier(identifier)?;
            let record: PendingKeyRecord = serde_json::from_slice(&fs::read(&path)?)?;
            if record.identifier != identifier {
                return Err(StoreError::InvalidPending(format!(
                    "identifier mismatch in {}",
                    path.display()
                )));
            }
            let p12 = materials.join(format!("{identifier}.p12"));
            if !p12.is_file() {
                return Err(StoreError::InvalidPending(format!(
                    "missing P12 for {identifier}"
                )));
            }
            pending.push(PendingKeyMaterial {
                identifier: record.identifier,
                alias: record.alias,
                team_id: record.team_id,
                certificate_name: record.certificate_name,
                csr_pem: record.csr_pem,
                p12,
            });
        }
        pending.sort_by(|left, right| left.p12.cmp(&right.p12));
        Ok(pending)
    }

    pub fn clear_pending_key(&self, identifier: &str) -> Result<(), StoreError> {
        let path = self.pending_path(identifier)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    fn pending_path(&self, identifier: &str) -> Result<PathBuf, StoreError> {
        validate_identifier(identifier)?;
        Ok(self
            .materials_dir()
            .join(format!("{identifier}.pending.json")))
    }

    pub fn list_key_pairs(&self) -> Result<Vec<KeyPairFiles>, StoreError> {
        let materials = self.materials_dir();
        if !materials.exists() {
            return Ok(Vec::new());
        }
        let mut pairs = Vec::new();
        for entry in fs::read_dir(materials)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("p12") {
                continue;
            }
            let certificate = path.with_extension("cer");
            if certificate.is_file() {
                pairs.push(KeyPairFiles {
                    p12: path,
                    certificate,
                });
            }
        }
        pairs.sort_by(|left, right| left.p12.cmp(&right.p12));
        Ok(pairs)
    }
}

#[cfg(feature = "app")]
#[derive(Debug, Clone, Default)]
pub struct SystemSecretStore;

#[cfg(feature = "app")]
impl SystemSecretStore {
    pub fn set(&self, name: &str, secret: &[u8]) -> Result<(), StoreError> {
        keyring::Entry::new(SECRET_SERVICE, name)
            .and_then(|entry| entry.set_secret(secret))
            .map_err(|error| StoreError::Credential(error.to_string()))
    }

    pub fn get(&self, name: &str) -> Result<Vec<u8>, StoreError> {
        keyring::Entry::new(SECRET_SERVICE, name)
            .and_then(|entry| entry.get_secret())
            .map_err(|error| StoreError::Credential(error.to_string()))
    }

    pub fn get_optional(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let entry = keyring::Entry::new(SECRET_SERVICE, name)
            .map_err(|error| StoreError::Credential(error.to_string()))?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(StoreError::Credential(error.to_string())),
        }
    }

    pub fn delete(&self, name: &str) -> Result<(), StoreError> {
        keyring::Entry::new(SECRET_SERVICE, name)
            .and_then(|entry| entry.delete_credential())
            .map_err(|error| StoreError::Credential(error.to_string()))
    }
}

fn validate_identifier(identifier: &str) -> Result<(), StoreError> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(StoreError::InvalidIdentifier);
    }
    Ok(())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::InvalidIdentifier)?;
    fs::create_dir_all(parent)?;
    let mut file = AtomicWriteFile::options().open(path)?;
    file.write_all(contents)?;
    file.commit()?;
    Ok(())
}
