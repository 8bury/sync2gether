use std::io;

pub const LABEL: &str = "sync2gether-stream";
pub const FRAGMENT: usize = 16 * 1024 - HEADER;
pub const HEADER: usize = 14;

const OP_HASH_TABLE: u8 = 1;
const OP_CHUNK: u8 = 2;
const OP_CANCEL: u8 = 3;

const STATUS_OK: u8 = 0;
const STATUS_ERR: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    HashTable,
    Chunk(u32),
    Cancel,
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
        }
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        match bytes {
            [OP_HASH_TABLE] => Ok(Self::HashTable),
            [OP_CHUNK, a, b, c, d] => Ok(Self::Chunk(u32::from_be_bytes([*a, *b, *c, *d]))),
            [OP_CANCEL] => Ok(Self::Cancel),
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
        for request in [Request::HashTable, Request::Chunk(77_000), Request::Cancel] {
            assert_eq!(Request::decode(&request.encode()).unwrap(), request);
        }
        assert!(Request::decode(&[]).is_err());
        assert!(Request::decode(&[2, 0, 0]).is_err());
        assert!(Request::decode(&[9]).is_err());
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
