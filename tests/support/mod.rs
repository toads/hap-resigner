use hap_resigner::hap::format::MAGIC_V3;

pub fn empty_zip() -> Vec<u8> {
    let mut eocd = Vec::with_capacity(22);
    eocd.extend_from_slice(b"PK\x05\x06");
    eocd.extend_from_slice(&[0; 16]);
    eocd.extend_from_slice(&0_u16.to_le_bytes());
    eocd
}

pub fn signing_block(blocks: &[(u32, &[u8])]) -> Vec<u8> {
    let mut header = Vec::with_capacity(blocks.len() * 12);
    let mut offset = blocks.len() * 12;
    for (block_type, value) in blocks {
        header.extend_from_slice(&block_type.to_le_bytes());
        header.extend_from_slice(&(value.len() as u32).to_le_bytes());
        header.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += value.len();
    }

    let values_len = blocks.iter().map(|(_, value)| value.len()).sum::<usize>();
    let size = header.len() + values_len + 32;
    let mut result = Vec::with_capacity(size);
    result.extend_from_slice(&header);
    for (_, value) in blocks {
        result.extend_from_slice(value);
    }
    result.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
    result.extend_from_slice(&(size as u64).to_le_bytes());
    result.extend_from_slice(MAGIC_V3);
    result.extend_from_slice(&3_u32.to_le_bytes());
    result
}

pub fn insert_before_central_directory(unsigned_zip: &[u8], block: &[u8]) -> Vec<u8> {
    let eocd_offset = unsigned_zip.len() - 22;
    let old_cd_offset = u32::from_le_bytes(
        unsigned_zip[eocd_offset + 16..eocd_offset + 20]
            .try_into()
            .expect("EOCD central-directory offset"),
    ) as usize;
    let mut eocd = unsigned_zip[eocd_offset..].to_vec();
    let new_cd_offset = old_cd_offset + block.len();
    eocd[16..20].copy_from_slice(&(new_cd_offset as u32).to_le_bytes());

    let mut signed = Vec::with_capacity(unsigned_zip.len() + block.len());
    signed.extend_from_slice(&unsigned_zip[..old_cd_offset]);
    signed.extend_from_slice(block);
    signed.extend_from_slice(&unsigned_zip[old_cd_offset..eocd_offset]);
    signed.extend_from_slice(&eocd);
    signed
}
