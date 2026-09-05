use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerIdentifier};
use der::{Decode, Encode};
use p256::ecdsa::{DerSignature as P256Signature, VerifyingKey as P256VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use p384::ecdsa::{DerSignature as P384Signature, VerifyingKey as P384VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use signature::Verifier;
use thiserror::Error;
use x509_cert::Certificate;

const ECDSA_SHA256: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("profile CMS is malformed: {0}")]
    Malformed(String),
    #[error("profile CMS digest or signature is invalid")]
    CmsIntegrity,
    #[error("profile is outside its validity period")]
    Expired,
    #[error("profile bundle name does not match")]
    BundleMismatch,
    #[error("profile does not include the connected device")]
    DeviceMismatch,
    #[error("profile development certificate does not match the signing key")]
    CertificateMismatch,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProvisionProfile {
    #[serde(rename = "type")]
    pub profile_type: String,
    pub validity: Validity,
    #[serde(rename = "bundle-info")]
    pub bundle_info: BundleInfo,
    #[serde(rename = "debug-info", default)]
    pub debug_info: DebugInfo,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Validity {
    #[serde(rename = "not-before")]
    pub not_before: i64,
    #[serde(rename = "not-after")]
    pub not_after: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BundleInfo {
    #[serde(rename = "bundle-name")]
    pub bundle_name: String,
    #[serde(rename = "development-certificate")]
    pub development_certificate: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DebugInfo {
    #[serde(rename = "device-ids", default)]
    pub device_ids: Vec<String>,
}

pub fn validate_profile(
    p7b: &[u8],
    expected_bundle: &str,
    expected_udid: &str,
    signer_certificate_der: &[u8],
    now_unix: i64,
) -> Result<ProvisionProfile, ProfileError> {
    let content_info =
        ContentInfo::from_der(p7b).map_err(|error| ProfileError::Malformed(error.to_string()))?;
    if content_info.content_type != const_oid::db::rfc5911::ID_SIGNED_DATA {
        return Err(ProfileError::Malformed(
            "content type is not SignedData".to_owned(),
        ));
    }
    let signed_data_der = content_info
        .content
        .to_der()
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let signed_data = SignedData::from_der(&signed_data_der)
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let content = signed_data
        .encap_content_info
        .econtent
        .as_ref()
        .ok_or_else(|| ProfileError::Malformed("profile content is missing".to_owned()))?
        .value();

    verify_cms_integrity(&signed_data, content)?;
    let profile: ProvisionProfile = serde_json::from_slice(content)
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    if now_unix < profile.validity.not_before || now_unix > profile.validity.not_after {
        return Err(ProfileError::Expired);
    }
    if profile.bundle_info.bundle_name != expected_bundle {
        return Err(ProfileError::BundleMismatch);
    }
    if !profile
        .debug_info
        .device_ids
        .iter()
        .any(|id| id == expected_udid)
    {
        return Err(ProfileError::DeviceMismatch);
    }
    if !development_certificate_matches(&profile, signer_certificate_der)? {
        return Err(ProfileError::CertificateMismatch);
    }
    Ok(profile)
}

fn verify_cms_integrity(signed_data: &SignedData, content: &[u8]) -> Result<(), ProfileError> {
    let signer_info = signed_data
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or_else(|| ProfileError::Malformed("SignerInfo is missing".to_owned()))?;
    if signer_info.digest_alg.oid != const_oid::db::rfc5912::ID_SHA_256
        || signer_info.signature_algorithm.oid != ECDSA_SHA256
    {
        return Err(ProfileError::Malformed(
            "unsupported profile signature algorithm".to_owned(),
        ));
    }
    let signed_attributes = signer_info
        .signed_attrs
        .as_ref()
        .ok_or_else(|| ProfileError::Malformed("signed attributes are missing".to_owned()))?;
    let expected_digest = Sha256::digest(content);
    let message_digest = signed_attributes
        .iter()
        .find(|attribute| attribute.oid == const_oid::db::rfc5911::ID_MESSAGE_DIGEST)
        .and_then(|attribute| attribute.values.iter().next())
        .map(|value| value.value())
        .ok_or_else(|| ProfileError::Malformed("messageDigest is missing".to_owned()))?;
    if message_digest != expected_digest.as_slice() {
        return Err(ProfileError::CmsIntegrity);
    }

    let signer_certificate = find_signer_certificate(signed_data, &signer_info.sid)?;
    let spki = signer_certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let attributes_der = signed_attributes
        .to_der()
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let signature_bytes = signer_info.signature.as_bytes();

    let verified = if let (Ok(key), Ok(signature)) = (
        P256VerifyingKey::from_public_key_der(&spki),
        P256Signature::try_from(signature_bytes),
    ) {
        key.verify(&attributes_der, &signature).is_ok()
    } else if let (Ok(key), Ok(signature)) = (
        P384VerifyingKey::from_public_key_der(&spki),
        P384Signature::try_from(signature_bytes),
    ) {
        key.verify(&attributes_der, &signature).is_ok()
    } else {
        false
    };
    if !verified {
        return Err(ProfileError::CmsIntegrity);
    }
    Ok(())
}

fn find_signer_certificate<'a>(
    signed_data: &'a SignedData,
    signer: &SignerIdentifier,
) -> Result<&'a Certificate, ProfileError> {
    let SignerIdentifier::IssuerAndSerialNumber(signer) = signer else {
        return Err(ProfileError::Malformed(
            "unsupported signer identifier".to_owned(),
        ));
    };
    signed_data
        .certificates
        .as_ref()
        .and_then(|certificates| {
            certificates.0.iter().find_map(|choice| match choice {
                CertificateChoices::Certificate(certificate)
                    if certificate.tbs_certificate.issuer == signer.issuer
                        && certificate.tbs_certificate.serial_number == signer.serial_number =>
                {
                    Some(certificate)
                }
                _ => None,
            })
        })
        .ok_or_else(|| ProfileError::Malformed("signer certificate is missing".to_owned()))
}

fn development_certificate_matches(
    profile: &ProvisionProfile,
    signer_certificate_der: &[u8],
) -> Result<bool, ProfileError> {
    let signer_certificate = Certificate::from_der(signer_certificate_der)
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let signer_spki = signer_certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let profile_pem = pem::parse(&profile.bundle_info.development_certificate)
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let profile_certificate = Certificate::from_der(profile_pem.contents())
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    let profile_spki = profile_certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|error| ProfileError::Malformed(error.to_string()))?;
    Ok(profile_spki == signer_spki)
}
