use cms::builder::{SignedDataBuilder, SignerInfoBuilder, create_signing_time_attribute};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
use const_oid::ObjectIdentifier;
use der::asn1::{SetOfVec, Utf8StringRef};
use der::{Any, Decode, Encode, Tag};
use p256::ecdsa::DerSignature;
use sha2::{Digest, Sha256};
use x509_cert::Certificate;
use x509_cert::attr::{Attribute, AttributeValue};
use x509_cert::spki::AlgorithmIdentifierOwned;

use crate::materials::SigningIdentity;

const OWNER_ID_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.2011.2.376.1.4.1");

pub(super) fn build_attached_cms(
    content: &[u8],
    identity: &SigningIdentity,
) -> Result<Vec<u8>, String> {
    build_cms(content, identity, true, None)
}

pub(super) fn build_detached_cms(
    content: &[u8],
    owner_id: &str,
    identity: &SigningIdentity,
) -> Result<Vec<u8>, String> {
    build_cms(content, identity, false, Some(owner_id))
}

fn build_cms(
    content: &[u8],
    identity: &SigningIdentity,
    attached: bool,
    owner_id: Option<&str>,
) -> Result<Vec<u8>, String> {
    let leaf =
        Certificate::from_der(&identity.certificates[0]).map_err(|error| error.to_string())?;
    let encapsulated = EncapsulatedContentInfo {
        econtent_type: const_oid::db::rfc5911::ID_DATA,
        econtent: if attached {
            Some(Any::new(Tag::OctetString, content.to_vec()).map_err(|error| error.to_string())?)
        } else {
            None
        },
    };
    let digest_algorithm = AlgorithmIdentifierOwned {
        oid: const_oid::db::rfc5912::ID_SHA_256,
        parameters: None,
    };
    let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
        issuer: leaf.tbs_certificate.issuer.clone(),
        serial_number: leaf.tbs_certificate.serial_number.clone(),
    });
    let external_digest = (!attached).then(|| Sha256::digest(content));
    let mut signer_builder = SignerInfoBuilder::new(
        &identity.signing_key,
        sid,
        digest_algorithm.clone(),
        &encapsulated,
        external_digest.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    signer_builder
        .add_signed_attribute(create_signing_time_attribute().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    if let Some(owner_id) = owner_id {
        signer_builder
            .add_signed_attribute(owner_id_attribute(owner_id)?)
            .map_err(|error| error.to_string())?;
    }

    let mut builder = SignedDataBuilder::new(&encapsulated);
    builder
        .add_digest_algorithm(digest_algorithm)
        .map_err(|error| error.to_string())?;
    for certificate_der in &identity.certificates {
        let certificate =
            Certificate::from_der(certificate_der).map_err(|error| error.to_string())?;
        builder
            .add_certificate(CertificateChoices::Certificate(certificate))
            .map_err(|error| error.to_string())?;
    }
    builder
        .add_signer_info::<p256::ecdsa::SigningKey, DerSignature>(signer_builder)
        .map_err(|error| error.to_string())?;
    builder
        .build()
        .and_then(|content_info| content_info.to_der().map_err(Into::into))
        .map_err(|error| error.to_string())
}

fn owner_id_attribute(owner_id: &str) -> Result<Attribute, String> {
    let value = Utf8StringRef::new(owner_id).map_err(|error| error.to_string())?;
    let mut values = SetOfVec::<AttributeValue>::new();
    values
        .insert(Any::from(value))
        .map_err(|error| error.to_string())?;
    Ok(Attribute {
        oid: OWNER_ID_OID,
        values,
    })
}
