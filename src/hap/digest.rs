use sha2::{Digest, Sha256};

const CHUNK_SIZE: usize = 1024 * 1024;
const CHUNK_PREFIX: u8 = 0xa5;
const TOP_LEVEL_PREFIX: u8 = 0x5a;

pub fn compute_content_digest(segments: &[&[u8]], optional_values: &[&[u8]]) -> [u8; 32] {
    let chunk_count = segments
        .iter()
        .map(|segment| segment.len().div_ceil(CHUNK_SIZE))
        .sum::<usize>();
    let mut chunk_digests = Vec::with_capacity(chunk_count);

    for segment in segments {
        for chunk in segment.chunks(CHUNK_SIZE) {
            let mut hasher = Sha256::new();
            hasher.update([CHUNK_PREFIX]);
            hasher.update((chunk.len() as u32).to_le_bytes());
            hasher.update(chunk);
            chunk_digests.push(hasher.finalize());
        }
    }

    let mut hasher = Sha256::new();
    hasher.update([TOP_LEVEL_PREFIX]);
    hasher.update((chunk_count as u32).to_le_bytes());
    for digest in chunk_digests {
        hasher.update(digest);
    }
    for value in optional_values {
        hasher.update(value);
    }
    hasher.finalize().into()
}
