use std::io::{Cursor, Read, Write};

use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use der::{Decode, Encode};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::materials::SigningIdentity;

use super::cms::build_detached_cms;
use super::format::{HapFormatError, parse_hap};

const FSVERITY_BLOCK_SIZE: usize = 4096;
const SHA256_SIZE: usize = 32;
const MAX_ENTRY_UNCOMPRESSED_SIZE: u64 = 512 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_SIZE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum CodeSignError {
    #[error(transparent)]
    Format(#[from] HapFormatError),
    #[error("code-sign input is too large")]
    SizeLimit,
    #[error("code-sign ZIP processing failed: {0}")]
    Zip(String),
    #[error("code-sign I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("code-sign CMS failed: {0}")]
    Cms(String),
    #[error("code-sign Profile failed: {0}")]
    Profile(String),
    #[error("runnable ZIP entry is not 4 KiB aligned: {0}")]
    Alignment(String),
    #[error("runnable ZIP entry must be stored: {0}")]
    RunnableCompression(String),
    #[error("unsupported unsigned HAP code-sign input: {0}")]
    Unsupported(String),
    #[error("ZIP entry uncompressed size {size} exceeds limit {limit}")]
    EntryTooLarge { size: u64, limit: u64 },
    #[error("ZIP total uncompressed size exceeds limit {0}")]
    ArchiveTooLarge(u64),
    #[error("ZIP entry data size does not match metadata: {0}")]
    SizeMismatch(String),
    #[error("invalid code-sign structure: {0}")]
    InvalidStructure(String),
}

struct FsVerityOutput {
    root_hash: [u8; SHA256_SIZE],
    tree: Vec<u8>,
    formatted_digest: [u8; 12 + SHA256_SIZE],
}

fn generate_fsverity(data: &[u8], tree_offset: u64) -> Result<FsVerityOutput, CodeSignError> {
    let (root_hash, tree) = generate_merkle_tree(data)?;
    let mut descriptor = [0_u8; 256];
    descriptor[0] = 1;
    descriptor[1] = 1;
    descriptor[2] = 12;
    descriptor[8..16].copy_from_slice(
        &u64::try_from(data.len())
            .map_err(|_| CodeSignError::SizeLimit)?
            .to_le_bytes(),
    );
    descriptor[16..48].copy_from_slice(&root_hash);
    descriptor[112..116].copy_from_slice(&u32::from(tree_offset != 0).to_le_bytes());
    descriptor[120..128].copy_from_slice(&tree_offset.to_le_bytes());
    let descriptor_digest: [u8; SHA256_SIZE] = Sha256::digest(descriptor).into();

    let mut formatted_digest = [0_u8; 12 + SHA256_SIZE];
    formatted_digest[..8].copy_from_slice(b"FSVerity");
    formatted_digest[8..10].copy_from_slice(&1_u16.to_le_bytes());
    formatted_digest[10..12].copy_from_slice(&(SHA256_SIZE as u16).to_le_bytes());
    formatted_digest[12..].copy_from_slice(&descriptor_digest);
    Ok(FsVerityOutput {
        root_hash,
        tree,
        formatted_digest,
    })
}

fn generate_merkle_tree(data: &[u8]) -> Result<([u8; SHA256_SIZE], Vec<u8>), CodeSignError> {
    if data.is_empty() {
        return Ok(([0_u8; SHA256_SIZE], Vec::new()));
    }

    let page_count = data.len().div_ceil(FSVERITY_BLOCK_SIZE);
    let leaf_size = round_up(
        page_count
            .checked_mul(SHA256_SIZE)
            .ok_or(CodeSignError::SizeLimit)?,
        FSVERITY_BLOCK_SIZE,
    )?;
    let mut current = vec![0_u8; leaf_size];
    for (index, page) in data.chunks(FSVERITY_BLOCK_SIZE).enumerate() {
        let digest = hash_padded_page(page);
        let offset = index * SHA256_SIZE;
        current[offset..offset + SHA256_SIZE].copy_from_slice(&digest);
    }

    if data.len() <= FSVERITY_BLOCK_SIZE {
        return Ok((
            current[..SHA256_SIZE].try_into().expect("SHA-256 digest"),
            Vec::new(),
        ));
    }

    let mut levels = vec![current];
    while levels.last().expect("leaf level").len() > FSVERITY_BLOCK_SIZE {
        let child = levels.last().expect("child level");
        let digest_bytes = child
            .len()
            .div_ceil(FSVERITY_BLOCK_SIZE)
            .checked_mul(SHA256_SIZE)
            .ok_or(CodeSignError::SizeLimit)?;
        let parent_size = round_up(digest_bytes, FSVERITY_BLOCK_SIZE)?;
        let mut parent = vec![0_u8; parent_size];
        for (index, page) in child.chunks(FSVERITY_BLOCK_SIZE).enumerate() {
            let digest: [u8; SHA256_SIZE] = Sha256::digest(page).into();
            let offset = index * SHA256_SIZE;
            parent[offset..offset + SHA256_SIZE].copy_from_slice(&digest);
        }
        levels.push(parent);
    }

    let root_hash =
        Sha256::digest(&levels.last().expect("root level")[..FSVERITY_BLOCK_SIZE]).into();
    let tree_size = levels.iter().try_fold(0_usize, |total, level| {
        total
            .checked_add(level.len())
            .ok_or(CodeSignError::SizeLimit)
    })?;
    let mut tree = Vec::with_capacity(tree_size);
    for level in levels.iter().rev() {
        tree.extend_from_slice(level);
    }
    Ok((root_hash, tree))
}

fn hash_padded_page(page: &[u8]) -> [u8; SHA256_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(page);
    if page.len() < FSVERITY_BLOCK_SIZE {
        hasher.update(&[0_u8; FSVERITY_BLOCK_SIZE][page.len()..]);
    }
    hasher.finalize().into()
}

fn round_up(value: usize, alignment: usize) -> Result<usize, CodeSignError> {
    value
        .checked_add(alignment - 1)
        .map(|value| value / alignment * alignment)
        .ok_or(CodeSignError::SizeLimit)
}

const CODE_SIGN_SUB_BLOCK_TYPE: u32 = 0x3000_0001;
const CODE_SIGN_MAGIC: u64 = 0xE046_C8C6_5389_FCCD;
const FSVERITY_INFO_MAGIC: u32 = 0x1E38_31AB;
const HAP_INFO_MAGIC: u32 = 0xC1B5_CC66;
const NATIVE_INFO_MAGIC: u32 = 0x0ED2_E720;
const CODE_SIGN_FIXED_HEADER_SIZE: usize = 32 + 3 * 12;
const DEBUG_OWNER_ID: &str = "DEBUG_LIB_ID";
pub(super) fn has_runnable_files(input: &[u8]) -> Result<bool, CodeSignError> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(zip_error)?;
    let mut total_size = 0_u64;
    let mut has_runnable = false;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(zip_error)?;
        reject_unsupported_entry(file.name())?;
        total_size = checked_uncompressed_total(total_size, file.size())?;
        has_runnable |= classify_runnable(file.name(), file.is_dir(), file.compression())?;
    }
    Ok(has_runnable)
}

struct ZipEntryPlan {
    index: usize,
    name: String,
    compression: CompressionMethod,
    is_directory: bool,
    is_runnable: bool,
    uncompressed_size: u64,
}

struct NativeFilePlan {
    index: usize,
    name: String,
    size: u64,
}

struct CodeSignInputs {
    hap_data_size: usize,
    native_files: Vec<NativeFilePlan>,
}

pub(super) fn align_zip_for_code_sign(input: &[u8]) -> Result<Vec<u8>, CodeSignError> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(zip_error)?;
    let comment = String::from_utf8_lossy(archive.comment()).into_owned();
    let mut plans = Vec::with_capacity(archive.len());
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(zip_error)?;
        reject_unsupported_entry(file.name())?;
        // Existing page bitmaps describe pre-rewrite offsets. They are optional for
        // fs-verity verification, so drop them instead of preserving stale data.
        if file.name() == ".pages.info" {
            continue;
        }
        total_size = checked_uncompressed_total(total_size, file.size())?;
        let compression = file.compression();
        let is_directory = file.is_dir();
        plans.push(ZipEntryPlan {
            index,
            name: file.name().to_owned(),
            compression,
            is_directory,
            is_runnable: classify_runnable(file.name(), is_directory, compression)?,
            uncompressed_size: file.size(),
        });
    }
    plans.sort_by(|left, right| {
        entry_rank(left)
            .cmp(&entry_rank(right))
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    if !comment.is_empty() {
        writer.set_comment(comment);
    }
    let mut first_resource = true;
    for plan in plans {
        let file = archive.by_index(plan.index).map_err(zip_error)?;
        let alignment = if plan.is_runnable {
            FSVERITY_BLOCK_SIZE as u16
        } else if first_resource {
            first_resource = false;
            FSVERITY_BLOCK_SIZE as u16
        } else if plan.compression == CompressionMethod::Stored {
            4
        } else {
            1
        };
        let options: SimpleFileOptions = file.options().with_alignment(alignment);
        if plan.is_directory {
            writer
                .add_directory(&plan.name, options)
                .map_err(zip_error)?;
        } else {
            writer.start_file(&plan.name, options).map_err(zip_error)?;
            copy_zip_entry(file, &mut writer, plan.uncompressed_size, &plan.name)?;
        }
    }
    writer.finish().map(Cursor::into_inner).map_err(zip_error)
}

pub(super) fn build_code_sign_property(
    aligned_hap: &[u8],
    profile: &[u8],
    identity: &SigningIdentity,
    code_sign_offset: usize,
) -> Result<Vec<u8>, CodeSignError> {
    let owner_id = code_sign_owner_id(profile)?;
    let inputs = collect_code_sign_inputs(aligned_hap)?;
    let tree_offset = round_up_u64(
        u64::try_from(code_sign_offset)
            .map_err(|_| CodeSignError::SizeLimit)?
            .checked_add(CODE_SIGN_FIXED_HEADER_SIZE as u64)
            .ok_or(CodeSignError::SizeLimit)?,
        FSVERITY_BLOCK_SIZE as u64,
    )?;
    let zero_padding_size = usize::try_from(
        tree_offset
            .checked_sub(
                u64::try_from(code_sign_offset).map_err(|_| CodeSignError::SizeLimit)?
                    + CODE_SIGN_FIXED_HEADER_SIZE as u64,
            )
            .ok_or(CodeSignError::SizeLimit)?,
    )
    .map_err(|_| CodeSignError::SizeLimit)?;

    let hap_fsverity = generate_fsverity(
        aligned_hap
            .get(..inputs.hap_data_size)
            .ok_or(CodeSignError::SizeLimit)?,
        tree_offset,
    )?;
    let hap_cms = build_detached_cms(&hap_fsverity.formatted_digest, &owner_id, identity)
        .map_err(CodeSignError::Cms)?;
    let hap_sign_info = encode_sign_info(
        inputs.hap_data_size,
        &hap_cms,
        Some((&hap_fsverity, tree_offset)),
    )?;

    let mut native_sign_infos = Vec::with_capacity(inputs.native_files.len());
    let mut archive = ZipArchive::new(Cursor::new(aligned_hap)).map_err(zip_error)?;
    for native in inputs.native_files {
        let file = archive.by_index(native.index).map_err(zip_error)?;
        let data = read_zip_entry(file, native.size, &native.name)?;
        let fsverity = generate_fsverity(&data, 0)?;
        let cms = build_detached_cms(&fsverity.formatted_digest, &owner_id, identity)
            .map_err(CodeSignError::Cms)?;
        native_sign_infos.push((native.name, encode_sign_info(data.len(), &cms, None)?));
    }

    let fsverity_segment = encode_fsverity_info_segment();
    let mut hap_segment = Vec::with_capacity(4 + hap_sign_info.len());
    hap_segment.extend_from_slice(&HAP_INFO_MAGIC.to_le_bytes());
    hap_segment.extend_from_slice(&hap_sign_info);
    let native_segment = encode_native_segment(&native_sign_infos)?;
    let first_segment_offset = CODE_SIGN_FIXED_HEADER_SIZE
        .checked_add(zero_padding_size)
        .and_then(|size| size.checked_add(hap_fsverity.tree.len()))
        .ok_or(CodeSignError::SizeLimit)?;
    let hap_segment_offset = first_segment_offset
        .checked_add(fsverity_segment.len())
        .ok_or(CodeSignError::SizeLimit)?;
    let native_segment_offset = hap_segment_offset
        .checked_add(hap_segment.len())
        .ok_or(CodeSignError::SizeLimit)?;
    let block_size = native_segment_offset
        .checked_add(native_segment.len())
        .ok_or(CodeSignError::SizeLimit)?;

    let mut code_sign = Vec::with_capacity(block_size);
    code_sign.extend_from_slice(&CODE_SIGN_MAGIC.to_le_bytes());
    code_sign.extend_from_slice(&1_u32.to_le_bytes());
    put_u32(&mut code_sign, block_size)?;
    code_sign.extend_from_slice(&3_u32.to_le_bytes());
    let flags = 1_u32 | u32::from(!native_sign_infos.is_empty()) << 1;
    code_sign.extend_from_slice(&flags.to_le_bytes());
    code_sign.extend_from_slice(&[0_u8; 8]);
    encode_segment_header(
        &mut code_sign,
        1,
        first_segment_offset,
        fsverity_segment.len(),
    )?;
    encode_segment_header(&mut code_sign, 2, hap_segment_offset, hap_segment.len())?;
    encode_segment_header(
        &mut code_sign,
        3,
        native_segment_offset,
        native_segment.len(),
    )?;
    code_sign.resize(
        code_sign
            .len()
            .checked_add(zero_padding_size)
            .ok_or(CodeSignError::SizeLimit)?,
        0,
    );
    code_sign.extend_from_slice(&hap_fsverity.tree);
    code_sign.extend_from_slice(&fsverity_segment);
    code_sign.extend_from_slice(&hap_segment);
    code_sign.extend_from_slice(&native_segment);
    if code_sign.len() != block_size {
        return Err(CodeSignError::InvalidStructure(
            "code-sign block size mismatch".to_owned(),
        ));
    }

    let mut property = Vec::with_capacity(12 + code_sign.len());
    property.extend_from_slice(&CODE_SIGN_SUB_BLOCK_TYPE.to_le_bytes());
    put_u32(&mut property, code_sign.len())?;
    put_u32(&mut property, code_sign_offset)?;
    property.extend_from_slice(&code_sign);
    Ok(property)
}

fn collect_code_sign_inputs(aligned_hap: &[u8]) -> Result<CodeSignInputs, CodeSignError> {
    let layout = parse_hap(aligned_hap)?;
    if layout.signing_block.is_some() {
        return Err(CodeSignError::InvalidStructure(
            "aligned HAP still contains a signing block".to_owned(),
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(aligned_hap)).map_err(zip_error)?;
    let mut hap_data_size = None;
    let mut native_files = Vec::new();
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(zip_error)?;
        reject_unsupported_entry(file.name())?;
        total_size = checked_uncompressed_total(total_size, file.size())?;
        let is_directory = file.is_dir();
        let is_runnable = classify_runnable(file.name(), is_directory, file.compression())?;
        if hap_data_size.is_none() {
            if is_runnable {
                if file.data_start() % FSVERITY_BLOCK_SIZE as u64 != 0 {
                    return Err(CodeSignError::Alignment(file.name().to_owned()));
                }
            } else {
                let data_start =
                    usize::try_from(file.data_start()).map_err(|_| CodeSignError::SizeLimit)?;
                if data_start % FSVERITY_BLOCK_SIZE != 0 {
                    return Err(CodeSignError::Alignment(file.name().to_owned()));
                }
                hap_data_size = Some(data_start);
            }
        }
        if !is_directory && is_native_name(file.name()) {
            native_files.push(NativeFilePlan {
                index,
                name: file.name().to_owned(),
                size: file.size(),
            });
        }
    }
    Ok(CodeSignInputs {
        hap_data_size: hap_data_size.unwrap_or(0),
        native_files,
    })
}

fn encode_sign_info(
    data_size: usize,
    cms: &[u8],
    merkle: Option<(&FsVerityOutput, u64)>,
) -> Result<Vec<u8>, CodeSignError> {
    let signature_padding = (4 - cms.len() % 4) % 4;
    let extension_size = if merkle.is_some() { 88 } else { 0 };
    let extension_offset = 60_usize
        .checked_add(cms.len())
        .and_then(|size| size.checked_add(signature_padding))
        .ok_or(CodeSignError::SizeLimit)?;
    let total_size = extension_offset
        .checked_add(extension_size)
        .ok_or(CodeSignError::SizeLimit)?;
    let mut output = Vec::with_capacity(total_size);
    output.extend_from_slice(&0_u32.to_le_bytes());
    put_u32(&mut output, cms.len())?;
    output.extend_from_slice(&u32::from(merkle.is_some()).to_le_bytes());
    output.extend_from_slice(
        &u64::try_from(data_size)
            .map_err(|_| CodeSignError::SizeLimit)?
            .to_le_bytes(),
    );
    output.extend_from_slice(&[0_u8; 32]);
    output.extend_from_slice(&u32::from(merkle.is_some()).to_le_bytes());
    put_u32(&mut output, extension_offset)?;
    output.extend_from_slice(cms);
    output.resize(extension_offset, 0);
    if let Some((fsverity, tree_offset)) = merkle {
        output.extend_from_slice(&1_u32.to_le_bytes());
        output.extend_from_slice(&80_u32.to_le_bytes());
        output.extend_from_slice(
            &u64::try_from(fsverity.tree.len())
                .map_err(|_| CodeSignError::SizeLimit)?
                .to_le_bytes(),
        );
        output.extend_from_slice(&tree_offset.to_le_bytes());
        output.extend_from_slice(&fsverity.root_hash);
        output.extend_from_slice(&[0_u8; 32]);
    }
    Ok(output)
}

fn encode_fsverity_info_segment() -> [u8; 64] {
    let mut output = [0_u8; 64];
    output[..4].copy_from_slice(&FSVERITY_INFO_MAGIC.to_le_bytes());
    output[4] = 1;
    output[5] = 1;
    output[6] = 12;
    output
}

fn encode_native_segment(
    native_sign_infos: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, CodeSignError> {
    let positions_size = native_sign_infos
        .len()
        .checked_mul(16)
        .ok_or(CodeSignError::SizeLimit)?;
    let names_size = native_sign_infos
        .iter()
        .try_fold(0_usize, |total, (name, _)| {
            total
                .checked_add(name.len())
                .ok_or(CodeSignError::SizeLimit)
        })?;
    let names_padding = (4 - names_size % 4) % 4;
    let sign_infos_size = native_sign_infos
        .iter()
        .try_fold(0_usize, |total, (_, sign_info)| {
            total
                .checked_add(sign_info.len())
                .ok_or(CodeSignError::SizeLimit)
        })?;
    let names_base = 12_usize
        .checked_add(positions_size)
        .ok_or(CodeSignError::SizeLimit)?;
    let sign_infos_base = names_base
        .checked_add(names_size)
        .and_then(|size| size.checked_add(names_padding))
        .ok_or(CodeSignError::SizeLimit)?;
    let segment_size = sign_infos_base
        .checked_add(sign_infos_size)
        .ok_or(CodeSignError::SizeLimit)?;

    let mut output = Vec::with_capacity(segment_size);
    output.extend_from_slice(&NATIVE_INFO_MAGIC.to_le_bytes());
    put_u32(&mut output, segment_size)?;
    put_u32(&mut output, native_sign_infos.len())?;
    let mut name_offset = names_base;
    let mut sign_info_offset = sign_infos_base;
    for (name, sign_info) in native_sign_infos {
        put_u32(&mut output, name_offset)?;
        put_u32(&mut output, name.len())?;
        put_u32(&mut output, sign_info_offset)?;
        put_u32(&mut output, sign_info.len())?;
        name_offset = name_offset
            .checked_add(name.len())
            .ok_or(CodeSignError::SizeLimit)?;
        sign_info_offset = sign_info_offset
            .checked_add(sign_info.len())
            .ok_or(CodeSignError::SizeLimit)?;
    }
    for (name, _) in native_sign_infos {
        output.extend_from_slice(name.as_bytes());
    }
    output.resize(sign_infos_base, 0);
    for (_, sign_info) in native_sign_infos {
        output.extend_from_slice(sign_info);
    }
    Ok(output)
}

fn reject_unsupported_entry(name: &str) -> Result<(), CodeSignError> {
    if name.starts_with("hnp/") && name.ends_with(".hnp") {
        return Err(CodeSignError::Unsupported(format!(
            "nested HNP code signing is not supported: {name}"
        )));
    }
    Ok(())
}

fn classify_runnable(
    name: &str,
    is_directory: bool,
    compression: CompressionMethod,
) -> Result<bool, CodeSignError> {
    if is_directory || !is_runnable_name(name) {
        return Ok(false);
    }
    if compression != CompressionMethod::Stored {
        return Err(CodeSignError::RunnableCompression(name.to_owned()));
    }
    Ok(true)
}

fn checked_uncompressed_total(total: u64, entry_size: u64) -> Result<u64, CodeSignError> {
    if entry_size > MAX_ENTRY_UNCOMPRESSED_SIZE {
        return Err(CodeSignError::EntryTooLarge {
            size: entry_size,
            limit: MAX_ENTRY_UNCOMPRESSED_SIZE,
        });
    }
    let total = total
        .checked_add(entry_size)
        .ok_or(CodeSignError::ArchiveTooLarge(MAX_TOTAL_UNCOMPRESSED_SIZE))?;
    if total > MAX_TOTAL_UNCOMPRESSED_SIZE {
        return Err(CodeSignError::ArchiveTooLarge(MAX_TOTAL_UNCOMPRESSED_SIZE));
    }
    Ok(total)
}

fn copy_zip_entry<R: Read, W: Write>(
    reader: R,
    writer: &mut W,
    expected_size: u64,
    name: &str,
) -> Result<(), CodeSignError> {
    let limit = expected_size
        .checked_add(1)
        .ok_or(CodeSignError::SizeLimit)?;
    let copied = std::io::copy(&mut reader.take(limit), writer)?;
    if copied != expected_size {
        return Err(CodeSignError::SizeMismatch(name.to_owned()));
    }
    Ok(())
}

fn read_zip_entry<R: Read>(
    reader: R,
    expected_size: u64,
    name: &str,
) -> Result<Vec<u8>, CodeSignError> {
    let limit = expected_size
        .checked_add(1)
        .ok_or(CodeSignError::SizeLimit)?;
    let mut data = Vec::new();
    reader.take(limit).read_to_end(&mut data)?;
    if u64::try_from(data.len()).map_err(|_| CodeSignError::SizeLimit)? != expected_size {
        return Err(CodeSignError::SizeMismatch(name.to_owned()));
    }
    Ok(data)
}

fn code_sign_owner_id(profile: &[u8]) -> Result<String, CodeSignError> {
    let content_info = ContentInfo::from_der(profile)
        .map_err(|error| CodeSignError::Profile(error.to_string()))?;
    let signed_data_der = content_info
        .content
        .to_der()
        .map_err(|error| CodeSignError::Profile(error.to_string()))?;
    let signed_data = SignedData::from_der(&signed_data_der)
        .map_err(|error| CodeSignError::Profile(error.to_string()))?;
    let content = signed_data
        .encap_content_info
        .econtent
        .as_ref()
        .ok_or_else(|| CodeSignError::Profile("profile content is missing".to_owned()))?
        .value();
    let value: Value = serde_json::from_slice(content)
        .map_err(|error| CodeSignError::Profile(error.to_string()))?;
    match value.get("type").and_then(Value::as_str) {
        Some("debug") => Ok(DEBUG_OWNER_ID.to_owned()),
        Some("release") => value
            .pointer("/bundle-info/app-identifier")
            .and_then(Value::as_str)
            .filter(|owner| !owner.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| CodeSignError::Profile("app-identifier is missing".to_owned())),
        Some(profile_type) => Err(CodeSignError::Profile(format!(
            "unsupported profile type {profile_type}"
        ))),
        None => Err(CodeSignError::Profile("profile type is missing".to_owned())),
    }
}

fn is_runnable_name(name: &str) -> bool {
    name.ends_with(".abc") || name.ends_with(".an") || name.starts_with("libs/")
}

fn is_native_name(name: &str) -> bool {
    name.ends_with(".an") || name.starts_with("libs/")
}

fn entry_rank(entry: &ZipEntryPlan) -> u8 {
    if entry.is_runnable {
        0
    } else if entry.compression == CompressionMethod::Stored {
        1
    } else {
        2
    }
}

fn encode_segment_header(
    output: &mut Vec<u8>,
    segment_type: u32,
    offset: usize,
    size: usize,
) -> Result<(), CodeSignError> {
    output.extend_from_slice(&segment_type.to_le_bytes());
    put_u32(output, offset)?;
    put_u32(output, size)
}

fn put_u32(output: &mut Vec<u8>, value: usize) -> Result<(), CodeSignError> {
    output.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| CodeSignError::SizeLimit)?
            .to_le_bytes(),
    );
    Ok(())
}

fn round_up_u64(value: u64, alignment: u64) -> Result<u64, CodeSignError> {
    value
        .checked_add(alignment - 1)
        .map(|value| value / alignment * alignment)
        .ok_or(CodeSignError::SizeLimit)
}

fn zip_error(error: zip::result::ZipError) -> CodeSignError {
    CodeSignError::Zip(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_ENTRY_UNCOMPRESSED_SIZE, MAX_TOTAL_UNCOMPRESSED_SIZE, checked_uncompressed_total,
        generate_fsverity,
    };

    #[test]
    fn fsverity_matches_official_hap_sign_tool_vectors() {
        let single = generate_fsverity(b"abc", 0).expect("single-page fs-verity");
        assert_eq!(
            single.root_hash,
            decode_hex("73fbfd76aa2143de160edd509ff93771f44db16924bd51235f311f32aaf5fc42")
        );
        assert!(single.tree.is_empty());
        assert_eq!(
            single.formatted_digest,
            decode_hex(
                "465356657269747901002000700b6bd8510f0b4f9bac8b9cf0459151a1c4a99f467892bb4bd289a67df8e19c"
            )
        );

        let data = (0..4097)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let multi = generate_fsverity(&data, 8192).expect("multi-page fs-verity");
        assert_eq!(
            multi.root_hash,
            decode_hex("9281fce0c40dfec63487b986806368f10224370b496de24d42498a1db0a660f1")
        );
        assert_eq!(multi.tree.len(), 4096);
        assert_eq!(
            multi.formatted_digest,
            decode_hex(
                "465356657269747901002000dc41e0c246915e6f5c43b7a3960f7fbb17feb1bf70deff5f42c964469c054783"
            )
        );
    }

    #[test]
    fn rejects_entries_and_archives_above_the_uncompressed_budget() {
        assert_eq!(
            checked_uncompressed_total(0, MAX_ENTRY_UNCOMPRESSED_SIZE).unwrap(),
            MAX_ENTRY_UNCOMPRESSED_SIZE
        );
        assert!(checked_uncompressed_total(0, MAX_ENTRY_UNCOMPRESSED_SIZE + 1).is_err());
        assert!(checked_uncompressed_total(MAX_TOTAL_UNCOMPRESSED_SIZE, 1,).is_err());
    }

    fn decode_hex<const N: usize>(value: &str) -> [u8; N] {
        assert_eq!(value.len(), N * 2);
        let mut output = [0_u8; N];
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
        }
        output
    }
}
