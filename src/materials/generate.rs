use base64::Engine;
use p12_keystore::{
    Certificate as P12Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain,
};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("key or CSR generation failed: {0}")]
    Rcgen(String),
    #[error("PKCS#12 generation failed: {0}")]
    Pkcs12(String),
}

#[derive(Debug, Clone)]
pub struct GeneratedKeyMaterial {
    pub identifier: String,
    pub alias: String,
    pub p12: Vec<u8>,
    pub csr_pem: String,
}

pub fn generate_key_material(
    team_id: &str,
    password: &str,
) -> Result<GeneratedKeyMaterial, GenerateError> {
    let key_pair = KeyPair::generate().map_err(|error| GenerateError::Rcgen(error.to_string()))?;
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CountryName, "CN");
    distinguished_name.push(DnType::OrganizationName, "HarmonyOS");
    distinguished_name.push(DnType::OrganizationalUnitName, team_id);
    distinguished_name.push(DnType::CommonName, "DebugKey");
    let mut params = CertificateParams::default();
    params.distinguished_name = distinguished_name;
    let placeholder = params
        .self_signed(&key_pair)
        .map_err(|error| GenerateError::Rcgen(error.to_string()))?;
    let csr = params
        .serialize_request(&key_pair)
        .and_then(|request| request.pem())
        .map_err(|error| GenerateError::Rcgen(error.to_string()))?;

    let local_id = Sha256::digest(key_pair.public_key_raw());
    let key_chain = PrivateKeyChain::new(
        &local_id[..20],
        PrivateKey::from_der(key_pair.serialized_der())
            .map_err(|error| GenerateError::Pkcs12(error.to_string()))?,
        [P12Certificate::from_der(placeholder.der().as_ref())
            .map_err(|error| GenerateError::Pkcs12(error.to_string()))?],
    );
    let alias = "debugKey".to_owned();
    let mut key_store = KeyStore::new();
    key_store.add_entry(&alias, KeyStoreEntry::PrivateKeyChain(key_chain));
    let p12 = key_store
        .writer(password)
        .write()
        .map_err(|error| GenerateError::Pkcs12(error.to_string()))?;

    let name_hash = Sha256::digest(format!("signing_key_{team_id}").as_bytes());
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(name_hash);
    Ok(GeneratedKeyMaterial {
        identifier: format!("auto_{}", &encoded[..20]),
        alias,
        p12,
        csr_pem: csr,
    })
}
