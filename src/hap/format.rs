use std::ops::Range;

use thiserror::Error;

pub const MAGIC_V3: &[u8; 16] = b"<hap sign block>";
pub const MAGIC_V2: &[u8; 16] = b"HAP Sig Block 42";
pub const TYPE_SIGNER: u32 = 0x2000_0000;
pub const TYPE_PROFILE: u32 = 0x2000_0002;
pub const TYPE_PROPERTY: u32 = 0x2000_0003;

const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const EOCD_MIN_SIZE: usize = 22;
const MAX_ZIP_COMMENT: usize = u16::MAX as usize;
const SIGNING_TRAILER_SIZE: usize = 32;
const ENTRY_SIZE: usize = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HapFormatError {
    #[error("ZIP end-of-central-directory record not found")]
    EocdNotFound,
    #[error("ZIP central directory is outside the file")]
    InvalidCentralDirectory,
    #[error("HAP signing block has an invalid size")]
    InvalidSigningBlockSize,
    #[error("HAP signing block table is corrupt")]
    InvalidSigningBlockTable,
    #[error("HAP signing block value range is corrupt")]
    InvalidSigningBlockValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningBlockEntry {
    pub block_type: u32,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningBlock {
    pub start: usize,
    pub size: usize,
    pub version: u32,
    pub entries: Vec<SigningBlockEntry>,
}

impl SigningBlock {
    pub fn block_value<'a>(&self, data: &'a [u8], block_type: u32) -> Option<&'a [u8]> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.block_type == block_type)?;
        data.get(entry.range.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HapLayout {
    pub eocd_offset: usize,
    pub central_directory_offset: usize,
    pub central_directory_size: usize,
    pub signing_block: Option<SigningBlock>,
}

pub fn parse_hap(data: &[u8]) -> Result<HapLayout, HapFormatError> {
    let eocd_offset = find_eocd(data).ok_or(HapFormatError::EocdNotFound)?;
    let central_directory_size = read_u32(data, eocd_offset + 12)? as usize;
    let central_directory_offset = read_u32(data, eocd_offset + 16)? as usize;
    if central_directory_offset
        .checked_add(central_directory_size)
        .filter(|end| *end == eocd_offset)
        .is_none()
    {
        return Err(HapFormatError::InvalidCentralDirectory);
    }

    let signing_block = parse_signing_block(data, central_directory_offset)?;
    Ok(HapLayout {
        eocd_offset,
        central_directory_offset,
        central_directory_size,
        signing_block,
    })
}

fn find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < EOCD_MIN_SIZE {
        return None;
    }
    let last = data.len() - EOCD_MIN_SIZE;
    let first = data.len().saturating_sub(EOCD_MIN_SIZE + MAX_ZIP_COMMENT);
    for offset in (first..=last).rev() {
        if data.get(offset..offset + 4) != Some(EOCD_SIGNATURE.as_slice()) {
            continue;
        }
        let comment_len = u16::from_le_bytes(
            data[offset + 20..offset + 22]
                .try_into()
                .expect("fixed EOCD comment length"),
        ) as usize;
        if offset + EOCD_MIN_SIZE + comment_len == data.len() {
            return Some(offset);
        }
    }
    None
}

fn parse_signing_block(
    data: &[u8],
    central_directory_offset: usize,
) -> Result<Option<SigningBlock>, HapFormatError> {
    if central_directory_offset < SIGNING_TRAILER_SIZE {
        return Ok(None);
    }
    let magic = data
        .get(central_directory_offset - 20..central_directory_offset - 4)
        .ok_or(HapFormatError::InvalidSigningBlockSize)?;
    if magic != MAGIC_V3 && magic != MAGIC_V2 {
        return Ok(None);
    }

    let size_u64 = read_u64(data, central_directory_offset - 28)?;
    let size = usize::try_from(size_u64).map_err(|_| HapFormatError::InvalidSigningBlockSize)?;
    if size < SIGNING_TRAILER_SIZE || size > central_directory_offset {
        return Err(HapFormatError::InvalidSigningBlockSize);
    }
    let start = central_directory_offset - size;
    let count = read_u32(data, central_directory_offset - 32)? as usize;
    let table_size = count
        .checked_mul(ENTRY_SIZE)
        .ok_or(HapFormatError::InvalidSigningBlockTable)?;
    let values_end = central_directory_offset - SIGNING_TRAILER_SIZE;
    if start
        .checked_add(table_size)
        .filter(|end| *end <= values_end)
        .is_none()
    {
        return Err(HapFormatError::InvalidSigningBlockTable);
    }

    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let row = start + index * ENTRY_SIZE;
        let block_type = read_u32(data, row)?;
        let length = read_u32(data, row + 4)? as usize;
        let relative_offset = read_u32(data, row + 8)? as usize;
        let value_start = start
            .checked_add(relative_offset)
            .ok_or(HapFormatError::InvalidSigningBlockValue)?;
        let value_end = value_start
            .checked_add(length)
            .ok_or(HapFormatError::InvalidSigningBlockValue)?;
        if value_start < start + table_size || value_end > values_end {
            return Err(HapFormatError::InvalidSigningBlockValue);
        }
        entries.push(SigningBlockEntry {
            block_type,
            range: value_start..value_end,
        });
    }

    Ok(Some(SigningBlock {
        start,
        size,
        version: read_u32(data, central_directory_offset - 4)?,
        entries,
    }))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, HapFormatError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(HapFormatError::InvalidSigningBlockSize)?;
    Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64, HapFormatError> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or(HapFormatError::InvalidSigningBlockSize)?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
}
