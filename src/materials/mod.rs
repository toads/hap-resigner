pub mod generate;
pub mod manager;
pub mod profile;
pub mod store;

use der::{Decode, Encode};
use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
use p256::ecdsa::SigningKey;
use p256::pkcs8::{DecodePrivateKey, EncodePublicKey};
use thiserror::Error;
use x509_cert::Certificate;

#[derive(Debug, Error)]
pub enum MaterialError {
    #[error("failed to read PKCS#12: {0}")]
    Pkcs12(String),
    #[error("PKCS#12 contains no private key")]
    MissingPrivateKey,
    #[error("failed to decode P-256 private key: {0}")]
    PrivateKey(String),
    #[error("failed to parse certificate chain: {0}")]
    Certificate(String),
    #[error("certificate chain has no leaf matching the private key")]
    MatchingCertificateNotFound,
}

pub struct SigningIdentity {
    pub alias: String,
    pub signing_key: SigningKey,
    pub certificates: Vec<Vec<u8>>,
}

pub fn load_signing_identity(
    p12_data: &[u8],
    password: &str,
    certificate_chain_pem: &[u8],
) -> Result<SigningIdentity, MaterialError> {
    let (alias, signing_key) = load_private_signing_key(p12_data, password)?;
    let signer_spki = signing_key
        .verifying_key()
        .to_public_key_der()
        .map_err(|error| MaterialError::PrivateKey(error.to_string()))?;

    let pem_blocks = pem::parse_many(certificate_chain_pem)
        .map_err(|error| MaterialError::Certificate(error.to_string()))?;
    let mut certificates = Vec::new();
    for block in pem_blocks {
        if block.tag() != "CERTIFICATE" {
            continue;
        }
        let certificate = Certificate::from_der(block.contents())
            .map_err(|error| MaterialError::Certificate(error.to_string()))?;
        let spki = certificate
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(|error| MaterialError::Certificate(error.to_string()))?;
        certificates.push((spki == signer_spki.as_bytes(), block.contents().to_vec()));
    }

    let signer_index = certificates
        .iter()
        .position(|(matches, _)| *matches)
        .ok_or(MaterialError::MatchingCertificateNotFound)?;
    let (_, signer_certificate) = certificates.remove(signer_index);
    let mut ordered = Vec::with_capacity(certificates.len() + 1);
    ordered.push(signer_certificate);
    ordered.extend(certificates.into_iter().map(|(_, certificate)| certificate));

    Ok(SigningIdentity {
        alias,
        signing_key,
        certificates: ordered,
    })
}

pub fn validate_p12_private_key(p12_data: &[u8], password: &str) -> Result<(), MaterialError> {
    load_private_signing_key(p12_data, password).map(|_| ())
}

fn load_private_signing_key(
    p12_data: &[u8],
    password: &str,
) -> Result<(String, SigningKey), MaterialError> {
    let key_store = KeyStore::from_pkcs12(p12_data, password, Pkcs12ImportPolicy::Relaxed)
        .map_err(|error| MaterialError::Pkcs12(error.to_string()))?;
    let (alias, key_chain) = key_store
        .private_key_chain()
        .ok_or(MaterialError::MissingPrivateKey)?;
    let signing_key = SigningKey::from_pkcs8_der(key_chain.key().as_der())
        .map_err(|error| MaterialError::PrivateKey(error.to_string()))?;
    Ok((alias.to_owned(), signing_key))
}
