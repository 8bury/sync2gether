use std::io;

pub const LABEL: &str = "sync2gether-stream";
pub const FRAGMENT: usize = 16 * 1024 - HEADER;
pub const HEADER: usize = 14;
pub const NO_AUDIO_TRACK: u8 = u8::MAX;
pub const MAX_MSE_SEGMENT_INDEX: u32 = 0x00FF_FFFF;

const OP_HASH_TABLE: u8 = 1;
const OP_CHUNK: u8 = 2;
const OP_CANCEL: u8 = 3;
const OP_MSE_INDEX: u8 = 4;
const OP_MSE_INIT: u8 = 5;
const OP_MSE_SEGMENT: u8 = 6;

const STATUS_OK: u8 = 0;
const STATUS_ERR: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    HashTable,
    Chunk(u32),
    Cancel,
    MseIndex,
    MseInit(u8),
    MseSegment { track: u8, segment: u32 },
}

impl Request {
    pub fn encode(self) -> Vec<u8> {
        match self {
            Self::HashTable => vec![OP_HASH_TABLE],
            Self::Chunk(index) => {
                let mut out = vec![OP_CHUNK];
                out.extend_from_slice(&index.to_be_bytes());
                out
            }
            Self::Cancel => vec![OP_CANCEL],
            Self::MseIndex => vec![OP_MSE_INDEX],
            Self::MseInit(track) => vec![OP_MSE_INIT, track],
            Self::MseSegment { track, segment } => {
                let mut out = vec![OP_MSE_SEGMENT, track];
                out.extend_from_slice(&segment.to_be_bytes());
                out
            }
        }
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        match bytes {
            [OP_HASH_TABLE] => Ok(Self::HashTable),
            [OP_CHUNK, a, b, c, d] => Ok(Self::Chunk(u32::from_be_bytes([*a, *b, *c, *d]))),
            [OP_CANCEL] => Ok(Self::Cancel),
            [OP_MSE_INDEX] => Ok(Self::MseIndex),
            [OP_MSE_INIT, track] => Ok(Self::MseInit(*track)),
            [OP_MSE_SEGMENT, track, a, b, c, d] => {
                let segment = u32::from_be_bytes([*a, *b, *c, *d]);
                if segment > MAX_MSE_SEGMENT_INDEX {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Pedido de streaming inválido",
                    ));
                }
                Ok(Self::MseSegment {
                    track: *track,
                    segment,
                })
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Pedido de streaming inválido",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    HashTable,
    Chunk(u32),
    MseIndex,
    MseInit(u8),
    MseSegment { track: u8, segment: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MseSegmentInfo {
    pub start_ms: u64,
    pub duration_ms: u32,
}

pub fn encode_mse_index(segments: &[MseSegmentInfo]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + segments.len() * 12);
    out.extend_from_slice(&(segments.len() as u32).to_be_bytes());
    for seg in segments {
        out.extend_from_slice(&seg.start_ms.to_be_bytes());
        out.extend_from_slice(&seg.duration_ms.to_be_bytes());
    }
    out
}

pub fn decode_mse_index(bytes: &[u8]) -> io::Result<Vec<MseSegmentInfo>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Índice MSE inválido");
    if bytes.len() < 4 {
        return Err(invalid());
    }
    let count = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if count > crate::protocol::MAX_MSE_SEGMENTS as usize || bytes.len() != 4 + count * 12 {
        return Err(invalid());
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 4 + i * 12;
        let start_ms = u64::from_be_bytes(bytes[base..base + 8].try_into().unwrap());
        let duration_ms = u32::from_be_bytes(bytes[base + 8..base + 12].try_into().unwrap());
        out.push(MseSegmentInfo {
            start_ms,
            duration_ms,
        });
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment<'a> {
    pub item: Item,
    pub ok: bool,
    pub offset: u32,
    pub total: u32,
    pub data: &'a [u8],
}

fn item_parts(item: Item) -> (u8, u32) {
    match item {
        Item::HashTable => (OP_HASH_TABLE, 0),
        Item::Chunk(index) => (OP_CHUNK, index),
        Item::MseIndex => (OP_MSE_INDEX, 0),
        Item::MseInit(track) => (OP_MSE_INIT, u32::from(track)),
        Item::MseSegment { track, segment } => (
            OP_MSE_SEGMENT,
            (u32::from(track) << 24) | (segment & MAX_MSE_SEGMENT_INDEX),
        ),
    }
}

pub fn fragments(item: Item, payload: &[u8]) -> impl Iterator<Item = Vec<u8>> + '_ {
    let (kind, index) = item_parts(item);
    let total = payload.len() as u32;
    let pieces = payload.len().div_ceil(FRAGMENT).max(1);
    (0..pieces).map(move |piece| {
        let start = piece * FRAGMENT;
        let end = (start + FRAGMENT).min(payload.len());
        let mut out = Vec::with_capacity(HEADER + end - start);
        out.push(STATUS_OK);
        out.push(kind);
        out.extend_from_slice(&index.to_be_bytes());
        out.extend_from_slice(&(start as u32).to_be_bytes());
        out.extend_from_slice(&total.to_be_bytes());
        out.extend_from_slice(&payload[start..end]);
        out
    })
}

pub fn failure(item: Item) -> Vec<u8> {
    let (kind, index) = item_parts(item);
    let mut out = vec![STATUS_ERR, kind];
    out.extend_from_slice(&index.to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    out
}

pub fn decode(bytes: &[u8]) -> io::Result<Fragment<'_>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Resposta de streaming inválida");
    if bytes.len() < HEADER {
        return Err(invalid());
    }
    let word =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let index = word(2);
    let item = match bytes[1] {
        OP_HASH_TABLE if index == 0 => Item::HashTable,
        OP_CHUNK => Item::Chunk(index),
        OP_MSE_INDEX if index == 0 => Item::MseIndex,
        OP_MSE_INIT if index <= u32::from(u8::MAX) => Item::MseInit(index as u8),
        OP_MSE_SEGMENT => Item::MseSegment {
            track: (index >> 24) as u8,
            segment: index & MAX_MSE_SEGMENT_INDEX,
        },
        _ => return Err(invalid()),
    };
    let ok = match bytes[0] {
        STATUS_OK => true,
        STATUS_ERR => false,
        _ => return Err(invalid()),
    };
    let fragment = Fragment {
        item,
        ok,
        offset: word(6),
        total: word(10),
        data: &bytes[HEADER..],
    };
    if fragment.ok
        && (fragment.data.len() > FRAGMENT
            || u64::from(fragment.offset) + fragment.data.len() as u64 > u64::from(fragment.total))
    {
        return Err(invalid());
    }
    Ok(fragment)
}

pub struct Assembly {
    item: Item,
    buffer: Vec<u8>,
    received: usize,
}

impl Assembly {
    pub fn new(item: Item) -> Self {
        Self {
            item,
            buffer: Vec::new(),
            received: 0,
        }
    }
    pub fn item(&self) -> Item {
        self.item
    }
    pub fn push(&mut self, fragment: &Fragment<'_>, max: usize) -> io::Result<Option<Vec<u8>>> {
        let total = fragment.total as usize;
        if fragment.item != self.item || total > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Fragmento fora de ordem",
            ));
        }
        if self.buffer.is_empty() {
            self.buffer = vec![0; total];
        }
        let start = fragment.offset as usize;
        if self.buffer.len() != total || start != self.received {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Fragmento fora de ordem",
            ));
        }
        self.buffer[start..start + fragment.data.len()].copy_from_slice(fragment.data);
        self.received += fragment.data.len();
        Ok((self.received == total).then(|| std::mem::take(&mut self.buffer)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_roundtrip_and_reject_garbage() {
        for request in [
            Request::HashTable,
            Request::Chunk(77_000),
            Request::Cancel,
            Request::MseIndex,
            Request::MseInit(0),
            Request::MseInit(NO_AUDIO_TRACK),
            Request::MseSegment {
                track: 2,
                segment: 1_234,
            },
        ] {
            assert_eq!(Request::decode(&request.encode()).unwrap(), request);
        }
        assert!(Request::decode(&[]).is_err());
        assert!(Request::decode(&[2, 0, 0]).is_err());
        assert!(Request::decode(&[6, 0, 0x01, 0, 0, 0]).is_err());
        assert!(Request::decode(&[9]).is_err());
    }

    #[test]
    fn mse_index_and_items_roundtrip() {
        let entries = vec![
            MseSegmentInfo {
                start_ms: 0,
                duration_ms: 4000,
            },
            MseSegmentInfo {
                start_ms: 4000,
                duration_ms: 3960,
            },
        ];
        let encoded = encode_mse_index(&entries);
        assert_eq!(decode_mse_index(&encoded).unwrap(), entries);
        assert!(decode_mse_index(&[0, 0]).is_err());
        assert!(decode_mse_index(&[0, 0, 0, 2, 0]).is_err());

        for item in [
            Item::MseIndex,
            Item::MseInit(1),
            Item::MseSegment {
                track: 3,
                segment: 42_000,
            },
        ] {
            let pieces: Vec<Vec<u8>> = fragments(item, b"hello").collect();
            let frag = decode(&pieces[0]).unwrap();
            assert_eq!(frag.item, item);
            assert!(frag.ok);
            assert_eq!(frag.data, b"hello");
        }
    }

    #[test]
    fn payloads_are_fragmented_and_reassembled_in_order() {
        let payload: Vec<u8> = (0..(FRAGMENT * 3 + 17)).map(|n| (n % 253) as u8).collect();
        let pieces: Vec<Vec<u8>> = fragments(Item::Chunk(5), &payload).collect();
        assert_eq!(pieces.len(), 4);
        assert!(pieces.iter().all(|piece| piece.len() <= 16 * 1024));
        let mut assembly = Assembly::new(Item::Chunk(5));
        let mut done = None;
        for piece in &pieces {
            let fragment = decode(piece).unwrap();
            assert!(fragment.ok);
            done = assembly.push(&fragment, 1 << 20).unwrap();
        }
        assert_eq!(done.unwrap(), payload);

        let mut wrong_order = Assembly::new(Item::Chunk(5));
        assert!(
            wrong_order
                .push(&decode(&pieces[1]).unwrap(), 1 << 20)
                .is_err()
        );
        let mut other = Assembly::new(Item::Chunk(6));
        assert!(other.push(&decode(&pieces[0]).unwrap(), 1 << 20).is_err());
        let mut small = Assembly::new(Item::Chunk(5));
        assert!(small.push(&decode(&pieces[0]).unwrap(), 10).is_err());
    }

    #[test]
    fn empty_payloads_and_failures_are_explicit() {
        let pieces: Vec<Vec<u8>> = fragments(Item::HashTable, &[]).collect();
        assert_eq!(pieces.len(), 1);
        let fragment = decode(&pieces[0]).unwrap();
        assert_eq!((fragment.item, fragment.total), (Item::HashTable, 0));
        let failed = failure(Item::Chunk(3));
        let fragment = decode(&failed).unwrap();
        assert!(!fragment.ok);
        assert_eq!(fragment.item, Item::Chunk(3));
        assert!(decode(&[0, 2, 0]).is_err());
    }
}
