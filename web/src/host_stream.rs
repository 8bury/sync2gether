use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use sync2gether_core::{
    chunks,
    datachannel::{self, Item, Request},
};

const MAX_QUEUE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerAction {
    ReadChunk {
        index: u32,
        offset: u64,
        length: u32,
    },
    RemuxIndex,
    RemuxInit {
        track: u8,
    },
    RemuxSegment {
        track: u8,
        segment: u32,
    },
}

pub struct StreamServer {
    file_size: u64,
    chunk_size: u32,
    chunk_count: u32,
    hash_table: Vec<u8>,
    remux_enabled: bool,
    queue: VecDeque<Item>,
    outgoing: VecDeque<Vec<u8>>,
    in_flight: Option<Item>,
    unique_chunks: HashSet<u32>,
    sent_bytes: u64,
}

impl StreamServer {
    pub fn new(file_size: u64, chunk_size: u32, hash_table: Vec<u8>, remux_enabled: bool) -> Self {
        let chunk_count = chunks::chunk_count(file_size, chunk_size) as u32;
        Self {
            file_size,
            chunk_size,
            chunk_count,
            hash_table,
            remux_enabled,
            queue: VecDeque::new(),
            outgoing: VecDeque::new(),
            in_flight: None,
            unique_chunks: HashSet::new(),
            sent_bytes: 0,
        }
    }

    pub fn on_request(&mut self, bytes: &[u8]) -> Result<(), String> {
        let request = Request::decode(bytes)
            .map_err(|_| "O convidado enviou um pedido de streaming inválido.".to_owned())?;
        match request {
            Request::Cancel => {
                self.queue.clear();
                self.outgoing.clear();
                self.in_flight = None;
                Ok(())
            }
            Request::HashTable => self.enqueue(Item::HashTable),
            Request::Chunk(index) => self.enqueue(Item::Chunk(index)),
            Request::MseIndex => self.enqueue(Item::MseIndex),
            Request::MseInit(track) => self.enqueue(Item::MseInit(track)),
            Request::MseSegment { track, segment } => {
                self.enqueue(Item::MseSegment { track, segment })
            }
        }
    }

    fn enqueue(&mut self, item: Item) -> Result<(), String> {
        if self.queue.len() >= MAX_QUEUE {
            return Err("Fila de pedidos de streaming cheia.".to_owned());
        }
        self.queue.push_back(item);
        Ok(())
    }

    pub fn next_action(&mut self) -> Option<ServerAction> {
        if !self.outgoing.is_empty() || self.in_flight.is_some() {
            return None;
        }
        let item = self.queue.pop_front()?;
        match item {
            Item::HashTable => {
                self.push_fragments(Item::HashTable, &self.hash_table.clone());
                None
            }
            Item::Chunk(index) => {
                if index >= self.chunk_count {
                    self.outgoing.push_back(datachannel::failure(item));
                    return None;
                }
                let offset = u64::from(index) * u64::from(self.chunk_size);
                let length = self
                    .file_size
                    .saturating_sub(offset)
                    .min(u64::from(self.chunk_size)) as u32;
                self.in_flight = Some(item);
                Some(ServerAction::ReadChunk {
                    index,
                    offset,
                    length,
                })
            }
            Item::MseIndex => {
                if !self.remux_enabled {
                    self.outgoing.push_back(datachannel::failure(item));
                    return None;
                }
                self.in_flight = Some(item);
                Some(ServerAction::RemuxIndex)
            }
            Item::MseInit(track) => {
                if !self.remux_enabled {
                    self.outgoing.push_back(datachannel::failure(item));
                    return None;
                }
                self.in_flight = Some(item);
                Some(ServerAction::RemuxInit { track })
            }
            Item::MseSegment { track, segment } => {
                if !self.remux_enabled {
                    self.outgoing.push_back(datachannel::failure(item));
                    return None;
                }
                self.in_flight = Some(item);
                Some(ServerAction::RemuxSegment { track, segment })
            }
        }
    }

    pub fn provide_chunk(&mut self, index: u32, data: &[u8]) {
        if self.in_flight != Some(Item::Chunk(index)) {
            return;
        }
        self.in_flight = None;
        self.unique_chunks.insert(index);
        self.sent_bytes += data.len() as u64;
        self.push_fragments(Item::Chunk(index), data);
    }

    pub fn fail_chunk(&mut self, index: u32) {
        if self.in_flight != Some(Item::Chunk(index)) {
            return;
        }
        self.in_flight = None;
        self.outgoing
            .push_back(datachannel::failure(Item::Chunk(index)));
    }

    pub fn provide_mse_index(&mut self, data: &[u8]) {
        if self.in_flight != Some(Item::MseIndex) {
            return;
        }
        self.in_flight = None;
        self.sent_bytes += data.len() as u64;
        self.push_fragments(Item::MseIndex, data);
    }

    pub fn fail_mse_index(&mut self) {
        if self.in_flight != Some(Item::MseIndex) {
            return;
        }
        self.in_flight = None;
        self.outgoing
            .push_back(datachannel::failure(Item::MseIndex));
    }

    pub fn provide_mse_init(&mut self, track: u8, data: &[u8]) {
        if self.in_flight != Some(Item::MseInit(track)) {
            return;
        }
        self.in_flight = None;
        self.sent_bytes += data.len() as u64;
        self.push_fragments(Item::MseInit(track), data);
    }

    pub fn fail_mse_init(&mut self, track: u8) {
        if self.in_flight != Some(Item::MseInit(track)) {
            return;
        }
        self.in_flight = None;
        self.outgoing
            .push_back(datachannel::failure(Item::MseInit(track)));
    }

    pub fn provide_mse_segment(&mut self, track: u8, segment: u32, data: &[u8]) {
        let item = Item::MseSegment { track, segment };
        if self.in_flight != Some(item) {
            return;
        }
        self.in_flight = None;
        self.unique_chunks.insert(segment);
        self.sent_bytes += data.len() as u64;
        self.push_fragments(item, data);
    }

    pub fn fail_mse_segment(&mut self, track: u8, segment: u32) {
        let item = Item::MseSegment { track, segment };
        if self.in_flight != Some(item) {
            return;
        }
        self.in_flight = None;
        self.outgoing.push_back(datachannel::failure(item));
    }

    pub fn take_fragment(&mut self) -> Option<Vec<u8>> {
        self.outgoing.pop_front()
    }

    pub fn has_outgoing(&self) -> bool {
        !self.outgoing.is_empty()
    }

    pub fn sent_bytes(&self) -> u64 {
        self.sent_bytes
    }

    pub fn unique_chunks(&self) -> u32 {
        self.unique_chunks.len() as u32
    }

    pub fn total_chunks(&self) -> u32 {
        self.chunk_count
    }

    fn push_fragments(&mut self, item: Item, payload: &[u8]) {
        for piece in datachannel::fragments(item, payload) {
            self.outgoing.push_back(piece);
        }
    }
}
