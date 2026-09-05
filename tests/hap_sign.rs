mod support;

use std::fs;
use std::io::{Cursor, Read, Write};

use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use der::{Decode, Encode};
use hap_resigner::hap::format::{TYPE_PROFILE, TYPE_PROPERTY, TYPE_SIGNER, parse_hap};
use hap_resigner::hap::sign::sign_hap;
use hap_resigner::materials::load_signing_identity;
use p256::ecdsa::{DerSignature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use signature::Verifier;
use x509_cert::Certificate;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[test]
fn signs_hap_and_produces_verifiable_cms() {
    let p12 = fs::read("tests/fixtures/placeholder.p12").expect("P12 fixture");
    let chain = fs::read("tests/fixtures/formal-chain.pem").expect("certificate chain fixture");
    let identity = load_signing_identity(&p12, "123456", &chain).expect("identity");

    let property = b"code-sign property";
    let old_block = support::signing_block(&[
        (TYPE_PROPERTY, property),
        (TYPE_PROFILE, b"old profile"),
        (TYPE_SIGNER, b"old signer"),
    ]);
    let input = support::insert_before_central_directory(&support::empty_zip(), &old_block);
    let profile = b"new profile";

    let output = sign_hap(&input, &identity, profile).expect("signed HAP");

    let layout = parse_hap(&output).expect("valid output HAP");
    let signing = layout.signing_block.as_ref().expect("new signing block");
    assert_eq!(signing.start, 0, "old signing block must be removed");
    assert_eq!(
        signing.block_value(&output, TYPE_PROPERTY),
        Some(property.as_slice())
    );
    assert_eq!(
        signing.block_value(&output, TYPE_PROFILE),
        Some(profile.as_slice())
    );

    let cms_bytes = signing
        .block_value(&output, TYPE_SIGNER)
        .expect("CMS signer block");
    let content_info = ContentInfo::from_der(cms_bytes).expect("ContentInfo");
    assert_eq!(
        content_info.content_type,
        const_oid::db::rfc5911::ID_SIGNED_DATA
    );
    let signed_data_der = content_info.content.to_der().expect("SignedData DER");
    let signed_data = SignedData::from_der(&signed_data_der).expect("SignedData");
    assert_eq!(
        signed_data
            .certificates
            .as_ref()
            .expect("certificates")
            .0
            .len(),
        2
    );
    let pairs = signed_data
        .encap_content_info
        .econtent
        .as_ref()
        .expect("digest pairs")
        .value();
    assert_eq!(pairs.len(), 52);
    assert_eq!(
        &pairs[..20],
        &[2, 0, 0, 0, 1, 0, 0, 0, 40, 0, 0, 0, 1, 2, 0, 0, 32, 0, 0, 0,]
    );

    let signer_info = signed_data
        .signer_infos
        .0
        .iter()
        .next()
        .expect("SignerInfo");
    let signature = DerSignature::try_from(signer_info.signature.as_bytes()).expect("ECDSA DER");
    let signed_attributes = signer_info
        .signed_attrs
        .as_ref()
        .expect("signed attributes")
        .to_der()
        .expect("signed attributes DER");
    let leaf = Certificate::from_der(&identity.certificates[0]).expect("leaf certificate");
    let leaf_spki = leaf
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .expect("leaf SPKI");
    let verifying_key = VerifyingKey::from_public_key_der(&leaf_spki).expect("P-256 key");
    verifying_key
        .verify(&signed_attributes, &signature)
        .expect("valid CMS signature");
}

#[test]
fn unsigned_hap_with_runnable_files_gets_code_sign_property() {
    let p12 = fs::read("tests/fixtures/placeholder.p12").expect("P12 fixture");
    let chain = fs::read("tests/fixtures/formal-chain.pem").expect("certificate chain fixture");
    let profile = fs::read("tests/fixtures/profile.p7b").expect("profile fixture");
    let identity = load_signing_identity(&p12, "123456", &chain).expect("identity");

    let output = sign_hap(&unsigned_runnable_hap(), &identity, &profile).expect("signed HAP");
    let layout = parse_hap(&output).expect("valid output HAP");
    let signing = layout.signing_block.as_ref().expect("signing block");
    let property_entry = signing
        .entries
        .iter()
        .find(|entry| entry.block_type == TYPE_PROPERTY)
        .expect("code-sign property");
    let property = &output[property_entry.range.clone()];
    assert_eq!(read_u32(property, 0), 0x3000_0001);
    let code_sign_length = read_u32(property, 4) as usize;
    assert_eq!(code_sign_length + 12, property.len());
    assert_eq!(
        read_u32(property, 8) as usize,
        property_entry.range.start + 12
    );
    let code_sign = &property[12..];
    assert_eq!(read_u64(code_sign, 0), 0xE046_C8C6_5389_FCCD);
    assert_eq!(read_u32(code_sign, 8), 1);
    assert_eq!(read_u32(code_sign, 16), 3);
    assert_eq!(read_u32(code_sign, 20), 3);

    let hap_segment_offset = read_u32(code_sign, 48) as usize;
    assert_eq!(read_u32(code_sign, hap_segment_offset), 0xC1B5_CC66);
    let sign_info_offset = hap_segment_offset + 4;
    let cms_size = read_u32(code_sign, sign_info_offset + 4) as usize;
    let code_cms = &code_sign[sign_info_offset + 60..sign_info_offset + 60 + cms_size];
    let code_content_info = ContentInfo::from_der(code_cms).expect("code-sign ContentInfo");
    let code_signed_data = SignedData::from_der(
        &code_content_info
            .content
            .to_der()
            .expect("code-sign SignedData DER"),
    )
    .expect("code-sign SignedData");
    assert!(code_signed_data.encap_content_info.econtent.is_none());
    let code_signer = code_signed_data
        .signer_infos
        .0
        .iter()
        .next()
        .expect("code-sign SignerInfo");
    let owner = code_signer
        .signed_attrs
        .as_ref()
        .expect("code-sign signed attributes")
        .iter()
        .find(|attribute| {
            attribute.oid == const_oid::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.2011.2.376.1.4.1")
        })
        .and_then(|attribute| attribute.values.iter().next())
        .expect("code-sign owner ID");
    assert_eq!(owner.value(), b"DEBUG_LIB_ID");
    let code_signature =
        DerSignature::try_from(code_signer.signature.as_bytes()).expect("code-sign ECDSA DER");
    let code_attributes = code_signer
        .signed_attrs
        .as_ref()
        .expect("code-sign attributes")
        .to_der()
        .expect("code-sign attributes DER");
    let leaf = Certificate::from_der(&identity.certificates[0]).expect("code-sign leaf");
    let leaf_spki = leaf
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .expect("code-sign leaf SPKI");
    let verifying_key = VerifyingKey::from_public_key_der(&leaf_spki).expect("code-sign key");
    verifying_key
        .verify(&code_attributes, &code_signature)
        .expect("valid code-sign CMS signature");

    let native_segment_offset = read_u32(code_sign, 60) as usize;
    assert_eq!(read_u32(code_sign, native_segment_offset), 0x0ED2_E720);
    assert_eq!(read_u32(code_sign, native_segment_offset + 8), 1);

    let mut archive = ZipArchive::new(Cursor::new(&output)).expect("output ZIP");
    for index in 0..archive.len() {
        let file = archive.by_index(index).expect("ZIP entry");
        if file.name().ends_with(".abc") || file.name().starts_with("libs/") {
            assert_eq!(file.data_start() % 4096, 0, "{} alignment", file.name());
        }
    }
    let mut resource = archive
        .by_name("resources/rawfile/data.txt")
        .expect("compressed resource");
    let mut resource_data = Vec::new();
    resource.read_to_end(&mut resource_data).unwrap();
    assert_eq!(resource_data, b"compressed resource".repeat(100));
    drop(resource);
    drop(archive);

    let second_output = sign_hap(&output, &identity, &profile).expect("re-signed HAP");
    let second_layout = parse_hap(&second_output).expect("second output layout");
    let second_property = second_layout
        .signing_block
        .as_ref()
        .and_then(|block| block.block_value(&second_output, TYPE_PROPERTY))
        .expect("preserved generated property");
    assert_eq!(second_property, property);
}

#[test]
fn compressed_runnable_file_is_rejected() {
    let p12 = fs::read("tests/fixtures/placeholder.p12").expect("P12 fixture");
    let chain = fs::read("tests/fixtures/formal-chain.pem").expect("certificate chain fixture");
    let profile = fs::read("tests/fixtures/profile.p7b").expect("profile fixture");
    let identity = load_signing_identity(&p12, "123456", &chain).expect("identity");

    let error = sign_hap(&compressed_runnable_hap(), &identity, &profile)
        .expect_err("compressed runnable must fail closed")
        .to_string();
    assert!(
        error.contains("must be stored"),
        "unexpected error: {error}"
    );
}

#[test]
fn unsigned_nested_hnp_is_rejected() {
    let p12 = fs::read("tests/fixtures/placeholder.p12").expect("P12 fixture");
    let chain = fs::read("tests/fixtures/formal-chain.pem").expect("certificate chain fixture");
    let profile = fs::read("tests/fixtures/profile.p7b").expect("profile fixture");
    let identity = load_signing_identity(&p12, "123456", &chain).expect("identity");

    let error = sign_hap(&unsigned_hnp_hap(), &identity, &profile)
        .expect_err("unsigned HNP must fail closed")
        .to_string();
    assert!(error.contains("nested HNP"), "unexpected error: {error}");
}

fn unsigned_runnable_hap() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer.start_file("module.json", deflated).unwrap();
    writer
        .write_all(br#"{"app":{"bundleName":"com.example.test"}}"#)
        .unwrap();
    writer
        .start_file("libs/arm64-v8a/libdemo.so", stored)
        .unwrap();
    writer.write_all(&vec![0x5a; 5000]).unwrap();
    writer.start_file("ets/modules.abc", stored).unwrap();
    writer.write_all(&vec![0xa5; 6000]).unwrap();
    writer
        .start_file("resources/rawfile/data.txt", deflated)
        .unwrap();
    writer
        .write_all(&b"compressed resource".repeat(100))
        .unwrap();
    writer.finish().unwrap().into_inner()
}

fn compressed_runnable_hap() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer.start_file("module.json", stored).unwrap();
    writer.write_all(b"{}").unwrap();
    writer.start_file("ets/modules.abc", deflated).unwrap();
    writer.write_all(&vec![0x5a; 5000]).unwrap();
    writer.finish().unwrap().into_inner()
}

fn unsigned_hnp_hap() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.start_file("module.json", stored).unwrap();
    writer.write_all(b"{}").unwrap();
    writer.start_file("hnp/demo.hnp", stored).unwrap();
    writer.write_all(b"nested native package").unwrap();
    writer.finish().unwrap().into_inner()
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}
