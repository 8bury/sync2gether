use std::io;

pub fn encode(bytes: &[u8], max: usize) -> io::Result<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > max {
        return Err(io::Error::other("Quadro com tamanho inválido"));
    }
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(bytes);
    Ok(frame)
}

pub fn encode_json<T: serde::Serialize>(message: &T, max: usize) -> io::Result<Vec<u8>> {
    encode(&serde_json::to_vec(message).map_err(io::Error::other)?, max)
}

#[derive(Default)]
pub struct Reader {
    buffer: Vec<u8>,
    start: usize,
}
impl Reader {
    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 && self.start == self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
        }
        self.buffer.extend_from_slice(bytes);
    }
    pub fn next(&mut self, max: usize) -> io::Result<Option<Vec<u8>>> {
        let pending = &self.buffer[self.start..];
        let Some(header) = pending.first_chunk::<4>() else {
            return Ok(None);
        };
        let size = u32::from_be_bytes(*header) as usize;
        if size == 0 || size > max {
            return Err(io::Error::other("Quadro com tamanho inválido"));
        }
        if pending.len() < 4 + size {
            return Ok(None);
        }
        let frame = pending[4..4 + size].to_vec();
        self.start += 4 + size;
        if self.start > 64 * 1024 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
        Ok(Some(frame))
    }
    pub fn buffered(&self) -> usize {
        self.buffer.len() - self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_arbitrary_splits() {
        let mut wire = encode(b"hello", 16).unwrap();
        wire.extend(encode(b"world!", 16).unwrap());
        for split in 0..wire.len() {
            let mut reader = Reader::default();
            reader.push(&wire[..split]);
            let mut frames = Vec::new();
            while let Some(frame) = reader.next(16).unwrap() {
                frames.push(frame);
            }
            reader.push(&wire[split..]);
            while let Some(frame) = reader.next(16).unwrap() {
                frames.push(frame);
            }
            assert_eq!(frames, vec![b"hello".to_vec(), b"world!".to_vec()]);
            assert_eq!(reader.buffered(), 0);
        }
    }

    #[test]
    fn empty_and_oversized_frames_are_rejected() {
        assert!(encode(b"", 16).is_err());
        assert!(encode(&[0; 17], 16).is_err());
        let mut reader = Reader::default();
        reader.push(&17u32.to_be_bytes());
        assert!(reader.next(16).is_err());
        let mut reader = Reader::default();
        reader.push(&0u32.to_be_bytes());
        assert!(reader.next(16).is_err());
    }
}
