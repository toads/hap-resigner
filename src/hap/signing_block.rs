use super::format::{HapFormatError, SigningBlock, TYPE_PROPERTY};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockValue {
    pub block_type: u32,
    pub value: Vec<u8>,
}

pub fn preserved_optional_blocks(
    data: &[u8],
    signing_block: Option<&SigningBlock>,
) -> Result<Vec<BlockValue>, HapFormatError> {
    let Some(signing_block) = signing_block else {
        return Ok(Vec::new());
    };

    signing_block
        .entries
        .iter()
        .filter(|entry| entry.block_type == TYPE_PROPERTY)
        .map(|entry| {
            let value = data
                .get(entry.range.clone())
                .ok_or(HapFormatError::InvalidSigningBlockValue)?;
            Ok(BlockValue {
                block_type: entry.block_type,
                value: value.to_vec(),
            })
        })
        .collect()
}
