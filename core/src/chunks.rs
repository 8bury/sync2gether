use crate::protocol::Media;
use std::io;

pub const CHUNK_SIZE: u32 = 256 * 1024;

pub fn table_digest(hashes: &[[u8; 32]]) -> String {
    let mut hasher = blake3::Hasher::new();
    for hash in hashes {
        hasher.update(hash);
    }
    hasher.finalize().to_hex().to_string()
}

pub fn chunk_count(file_size: u64, chunk_size: u32) -> usize {
    if file_size == 0 {
        0
    } else {
        file_size.div_ceil(u64::from(chunk_size)) as usize
    }
}

pub fn verify_table(
    raw: &[u8],
    expected_digest: &str,
    file_size: u64,
    chunk_size: u32,
) -> io::Result<Vec<[u8; 32]>> {
    if raw.len() != chunk_count(file_size, chunk_size) * 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Tabela de hashes com tamanho inesperado",
        ));
    }
    let actual = blake3::hash(raw).to_hex().to_string();
    if !actual.eq_ignore_ascii_case(expected_digest) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Integridade da tabela de blocos falhou",
        ));
    }
    Ok(raw.as_chunks::<32>().0.to_vec())
}

pub fn chunk_matches(expected: &[u8; 32], data: &[u8]) -> bool {
    blake3::hash(data).as_bytes() == expected
}

pub struct Identifier {
    whole: blake3::Hasher,
    hashes: Vec<[u8; 32]>,
    pending: Vec<u8>,
    chunk_size: usize,
    bytes: u64,
}
impl Identifier {
    pub fn new(chunk_size: u32) -> Self {
        Self {
            whole: blake3::Hasher::new(),
            hashes: Vec::new(),
            pending: Vec::new(),
            chunk_size: chunk_size as usize,
            bytes: 0,
        }
    }
    pub fn update(&mut self, mut data: &[u8]) {
        self.whole.update(data);
        self.bytes += data.len() as u64;
        while !data.is_empty() {
            let take = (self.chunk_size - self.pending.len()).min(data.len());
            self.pending.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.pending.len() == self.chunk_size {
                self.hashes.push(*blake3::hash(&self.pending).as_bytes());
                self.pending.clear();
            }
        }
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn finish(mut self) -> (Media, Vec<[u8; 32]>) {
        if !self.pending.is_empty() {
            self.hashes.push(*blake3::hash(&self.pending).as_bytes());
        }
        (
            Media {
                hash: self.whole.finalize().to_hex().to_string(),
                bytes: self.bytes,
            },
            self.hashes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_identity_matches_whole_file_hashing_for_any_slicing() {
        let data: Vec<u8> = (0..10_000u32).map(|n| (n * 7 % 251) as u8).collect();
        let expected_hash = blake3::hash(&data).to_hex().to_string();
        let expected_chunks: Vec<[u8; 32]> = data
            .chunks(4096)
            .map(|chunk| *blake3::hash(chunk).as_bytes())
            .collect();
        for step in [1, 100, 4096, 5000, 10_000] {
            let mut id = Identifier::new(4096);
            for piece in data.chunks(step) {
                id.update(piece);
            }
            let (media, hashes) = id.finish();
            assert_eq!(media.hash, expected_hash);
            assert_eq!(media.bytes, data.len() as u64);
            assert_eq!(hashes, expected_chunks);
        }
        let digest = table_digest(&expected_chunks);
        let raw: Vec<u8> = expected_chunks.concat();
        assert_eq!(
            verify_table(&raw, &digest, data.len() as u64, 4096).unwrap(),
            expected_chunks
        );
        assert!(verify_table(&raw, &digest, data.len() as u64 + 5000, 4096).is_err());
        assert!(verify_table(&raw, &"0".repeat(64), data.len() as u64, 4096).is_err());
        assert!(chunk_matches(&expected_chunks[0], &data[..4096]));
        assert!(!chunk_matches(&expected_chunks[0], &data[1..4097]));
    }
}
