use thiserror::Error;

use crate::materials::SigningIdentity;

use super::cms::build_attached_cms;
use super::code_sign::{
    CodeSignError, align_zip_for_code_sign, build_code_sign_property, has_runnable_files,
};
use super::digest::compute_content_digest;
use super::format::{
    HapFormatError, MAGIC_V3, TYPE_PROFILE, TYPE_PROPERTY, TYPE_SIGNER, parse_hap,
};
use super::signing_block::{BlockValue, preserved_optional_blocks};

const DIGEST_PAIR_VERSION: u32 = 2;
const DIGEST_PAIR_COUNT: u32 = 1;
const SHA256_ECDSA_ID: u32 = 0x201;
const SIGNING_BLOCK_VERSION: u32 = 3;

#[derive(Debug, Error)]
pub enum HapSignError {
    #[error(transparent)]
    Format(#[from] HapFormatError),
    #[error("HAP offset or block length exceeds the v3 format limit")]
    SizeLimit,
    #[error("failed to construct CMS: {0}")]
    Cms(String),
    #[error(transparent)]
    CodeSign(#[from] CodeSignError),
}

pub fn sign_hap(
    input: &[u8],
    identity: &SigningIdentity,
    profile: &[u8],
) -> Result<Vec<u8>, HapSignError> {
    let initial_layout = parse_hap(input)?;
    let property_missing = initial_layout
        .signing_block
        .as_ref()
        .and_then(|block| block.block_value(input, TYPE_PROPERTY))
        .is_none();
    let needs_code_sign = property_missing && has_runnable_files(input)?;
    let aligned_input;
    let input = if needs_code_sign {
        aligned_input = align_zip_for_code_sign(input)?;
        aligned_input.as_slice()
    } else {
        input
    };
    let layout = parse_hap(input)?;
    let block_start = layout
        .signing_block
        .as_ref()
        .map_or(layout.central_directory_offset, |block| block.start);
    let mut optional_blocks = preserved_optional_blocks(input, layout.signing_block.as_ref())?;
    optional_blocks.push(BlockValue {
        block_type: TYPE_PROFILE,
        value: profile.to_vec(),
    });
    if needs_code_sign {
        let final_block_count = optional_blocks
            .len()
            .checked_add(2)
            .ok_or(HapSignError::SizeLimit)?;
        let code_sign_offset = block_start
            .checked_add(
                final_block_count
                    .checked_mul(12)
                    .ok_or(HapSignError::SizeLimit)?,
            )
            .and_then(|offset| offset.checked_add(12))
            .ok_or(HapSignError::SizeLimit)?;
        let property = build_code_sign_property(input, profile, identity, code_sign_offset)?;
        optional_blocks.insert(
            0,
            BlockValue {
                block_type: TYPE_PROPERTY,
                value: property,
            },
        );
    }
    let before = &input[..block_start];
    let central_directory = &input[layout.central_directory_offset
        ..layout.central_directory_offset + layout.central_directory_size];
    let mut eocd = input[layout.eocd_offset..].to_vec();
    write_u32(
        &mut eocd,
        16,
        u32::try_from(block_start).map_err(|_| HapSignError::SizeLimit)?,
    )?;

    let optional_values = optional_blocks
        .iter()
        .map(|block| block.value.as_slice())
        .collect::<Vec<_>>();
    let digest = compute_content_digest(
        &[before, central_directory, eocd.as_slice()],
        &optional_values,
    );
    let pairs = encode_digest_pairs(&digest);
    let cms = build_attached_cms(&pairs, identity).map_err(HapSignError::Cms)?;

    let mut blocks = optional_blocks;
    blocks.push(BlockValue {
        block_type: TYPE_SIGNER,
        value: cms,
    });
    let signing_block = encode_signing_block(&blocks)?;
    let new_cd_offset = block_start
        .checked_add(signing_block.len())
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or(HapSignError::SizeLimit)?;
    write_u32(&mut eocd, 16, new_cd_offset)?;

    let mut output = Vec::with_capacity(
        before.len() + signing_block.len() + central_directory.len() + eocd.len(),
    );
    output.extend_from_slice(before);
    output.extend_from_slice(&signing_block);
    output.extend_from_slice(central_directory);
    output.extend_from_slice(&eocd);
    Ok(output)
}

fn encode_digest_pairs(digest: &[u8; 32]) -> Vec<u8> {
    let mut pairs = Vec::with_capacity(52);
    pairs.extend_from_slice(&DIGEST_PAIR_VERSION.to_le_bytes());
    pairs.extend_from_slice(&DIGEST_PAIR_COUNT.to_le_bytes());
    pairs.extend_from_slice(&(8_u32 + digest.len() as u32).to_le_bytes());
    pairs.extend_from_slice(&SHA256_ECDSA_ID.to_le_bytes());
    pairs.extend_from_slice(&(digest.len() as u32).to_le_bytes());
    pairs.extend_from_slice(digest);
    pairs
}

fn encode_signing_block(blocks: &[BlockValue]) -> Result<Vec<u8>, HapSignError> {
    let table_size = blocks
        .len()
        .checked_mul(12)
        .ok_or(HapSignError::SizeLimit)?;
    let values_size = blocks.iter().try_fold(0_usize, |total, block| {
        total
            .checked_add(block.value.len())
            .ok_or(HapSignError::SizeLimit)
    })?;
    let size = table_size
        .checked_add(values_size)
        .and_then(|value| value.checked_add(32))
        .ok_or(HapSignError::SizeLimit)?;

    let mut output = Vec::with_capacity(size);
    let mut offset = table_size;
    for block in blocks {
        output.extend_from_slice(&block.block_type.to_le_bytes());
        output.extend_from_slice(
            &u32::try_from(block.value.len())
                .map_err(|_| HapSignError::SizeLimit)?
                .to_le_bytes(),
        );
        output.extend_from_slice(
            &u32::try_from(offset)
                .map_err(|_| HapSignError::SizeLimit)?
                .to_le_bytes(),
        );
        offset = offset
            .checked_add(block.value.len())
            .ok_or(HapSignError::SizeLimit)?;
    }
    for block in blocks {
        output.extend_from_slice(&block.value);
    }
    output.extend_from_slice(
        &u32::try_from(blocks.len())
            .map_err(|_| HapSignError::SizeLimit)?
            .to_le_bytes(),
    );
    output.extend_from_slice(&(size as u64).to_le_bytes());
    output.extend_from_slice(MAGIC_V3);
    output.extend_from_slice(&SIGNING_BLOCK_VERSION.to_le_bytes());
    Ok(output)
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) -> Result<(), HapSignError> {
    let target = data
        .get_mut(offset..offset + 4)
        .ok_or(HapFormatError::InvalidCentralDirectory)?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}
