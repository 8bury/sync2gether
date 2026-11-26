use std::collections::{HashMap, HashSet, VecDeque};
use sync2gether_core::{
    chunks,
    datachannel::{self, Assembly, Item, Request},
    protocol::StreamProgress,
};

pub const MAX_IN_FLIGHT: usize = 3;
pub const READ_WINDOW: u64 = 2 * 1024 * 1024;
const RANGE_BUCKETS: u64 = 1000;

pub struct StreamClient {
    file_size: u64,
    chunk_size: u32,
    count: u32,
    digest: String,
    hashes: Option<Vec<[u8; 32]>>,
    assembly: Option<Assembly>,
    cache: HashMap<u32, Vec<u8>>,
    order: VecDeque<u32>,
    budget: usize,
    ahead: u32,
    in_flight: VecDeque<u32>,
    needed: VecDeque<u32>,
    seen: HashSet<u32>,
    playhead: u32,
    outgoing: Vec<Vec<u8>>,
    received: u64,
    error: Option<String>,
}

impl StreamClient {
    pub fn new(file_size: u64, chunk_size: u32, digest: &str, budget_bytes: u64) -> Self {
        let count = chunks::chunk_count(file_size, chunk_size) as u32;
        let budget = (budget_bytes / u64::from(chunk_size)).max(8) as usize;
        Self {
            file_size,
            chunk_size,
            count,
            digest: digest.to_owned(),
            hashes: None,
            assembly: None,
            cache: HashMap::new(),
            order: VecDeque::new(),
            budget,
            ahead: (budget as u32 / 2).max(4),
            in_flight: VecDeque::new(),
            needed: VecDeque::new(),
            seen: HashSet::new(),
            playhead: 0,
            outgoing: vec![Request::HashTable.encode()],
            received: 0,
            error: None,
        }
    }

    pub fn ready(&self) -> bool {
        self.hashes.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
    pub fn received_bytes(&self) -> u64 {
        self.received
    }
    pub fn take_requests(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.outgoing)
    }

    pub fn reset_transport(&mut self) {
        self.assembly = None;
        self.in_flight.clear();
        self.outgoing.clear();
        if self.hashes.is_none() {
            self.outgoing.push(Request::HashTable.encode());
        }
        self.schedule();
    }

    pub fn on_message(&mut self, bytes: &[u8]) {
        let Ok(fragment) = datachannel::decode(bytes) else {
            self.error = Some("O anfitrião enviou dados de streaming inválidos.".into());
            return;
        };
        match fragment.item {
            Item::HashTable => {
                if self.hashes.is_some() {
                    return;
                }
                if !fragment.ok {
                    self.error = Some("O anfitrião não enviou a tabela de blocos.".into());
                    return;
                }
                let max = self.count as usize * 32;
                if fragment.offset == 0
                    || self
                        .assembly
                        .as_ref()
                        .is_none_or(|a| a.item() != Item::HashTable)
                {
                    self.assembly = Some(Assembly::new(Item::HashTable));
                }
                let Some(assembly) = &mut self.assembly else {
                    return;
                };
                match assembly.push(&fragment, max) {
                    Ok(Some(table)) => {
                        self.assembly = None;
                        match chunks::verify_table(
                            &table,
                            &self.digest,
                            self.file_size,
                            self.chunk_size,
                        ) {
                            Ok(hashes) => {
                                self.hashes = Some(hashes);
                                self.schedule();
                            }
                            Err(_) => {
                                self.error = Some(
                                    "A tabela de blocos não confere com a do anfitrião.".into(),
                                )
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(_) => self.assembly = None,
                }
            }
            Item::Chunk(index) => {
                if !self.in_flight.contains(&index) {
                    return;
                }
                if !fragment.ok {
                    self.in_flight.retain(|i| *i != index);
                    self.error = Some("O anfitrião não conseguiu ler um trecho do filme.".into());
                    return;
                }
                if fragment.offset == 0
                    || self
                        .assembly
                        .as_ref()
                        .is_none_or(|a| a.item() != Item::Chunk(index))
                {
                    if fragment.offset != 0 {
                        return;
                    }
                    self.assembly = Some(Assembly::new(Item::Chunk(index)));
                }
                let max = self.chunk_len(index);
                let Some(assembly) = &mut self.assembly else {
                    return;
                };
                match assembly.push(&fragment, max) {
                    Ok(Some(data)) => {
                        self.assembly = None;
                        self.in_flight.retain(|i| *i != index);
                        self.received += data.len() as u64;
                        let valid = data.len() == max
                            && self
                                .hashes
                                .as_ref()
                                .and_then(|h| h.get(index as usize))
                                .is_some_and(|hash| chunks::chunk_matches(hash, &data));
                        if valid {
                            self.store(index, data);
                        }
                        self.schedule();
                    }
                    Ok(None) => {}
                    Err(_) => self.assembly = None,
                }
            }
        }
    }

    fn chunk_len(&self, index: u32) -> usize {
        let start = u64::from(index) * u64::from(self.chunk_size);
        self.file_size
            .saturating_sub(start)
            .min(u64::from(self.chunk_size)) as usize
    }

    fn store(&mut self, index: u32, data: Vec<u8>) {
        self.seen.insert(index);
        self.needed.retain(|i| *i != index);
        self.cache.insert(index, data);
        self.touch(index);
        while self.cache.len() > self.budget {
            let keep_from = self.playhead.saturating_sub(2);
            let keep_to = self.playhead.saturating_add(self.ahead);
            let Some(position) = self
                .order
                .iter()
                .position(|i| *i < keep_from || *i > keep_to)
            else {
                break;
            };
            if let Some(evicted) = self.order.remove(position) {
                self.cache.remove(&evicted);
            }
        }
    }

    fn touch(&mut self, index: u32) {
        self.order.retain(|i| *i != index);
        self.order.push_back(index);
    }

    pub fn read(&mut self, start: u64, end_inclusive: u64) -> Option<Vec<u8>> {
        if start >= self.file_size {
            return Some(Vec::new());
        }
        let end = end_inclusive
            .saturating_add(1)
            .min(self.file_size)
            .min(start + READ_WINDOW);
        let chunk_size = u64::from(self.chunk_size);
        let first = (start / chunk_size) as u32;
        let last = ((end.max(start + 1) - 1) / chunk_size) as u32;
        self.playhead = first;
        if !self.cache.contains_key(&first) {
            self.need(first);
            return None;
        }
        let mut out = Vec::with_capacity((end - start) as usize);
        for index in first..=last {
            let Some(data) = self.cache.get(&index) else {
                break;
            };
            let chunk_start = u64::from(index) * chunk_size;
            let from = start.saturating_sub(chunk_start) as usize;
            let to = (end - chunk_start).min(data.len() as u64) as usize;
            out.extend_from_slice(&data[from..to]);
            self.touch(index);
        }
        self.schedule();
        Some(out)
    }

    fn need(&mut self, index: u32) {
        if !self.needed.contains(&index) {
            self.needed.push_front(index);
        }
        let useful = |i: &u32| *i >= index && *i <= index.saturating_add(MAX_IN_FLIGHT as u32);
        if !self.in_flight.is_empty() && !self.in_flight.iter().any(useful) {
            self.outgoing.push(Request::Cancel.encode());
            self.in_flight.clear();
            self.assembly = None;
        }
        self.schedule();
    }

    fn schedule(&mut self) {
        if self.hashes.is_none() {
            return;
        }
        while self.in_flight.len() < MAX_IN_FLIGHT {
            let next = self
                .needed
                .iter()
                .copied()
                .find(|i| !self.cache.contains_key(i) && !self.in_flight.contains(i))
                .or_else(|| {
                    (self.playhead..self.playhead.saturating_add(self.ahead).min(self.count))
                        .find(|i| !self.cache.contains_key(i) && !self.in_flight.contains(i))
                });
            let Some(index) = next else {
                break;
            };
            self.needed.retain(|i| *i != index);
            self.in_flight.push_back(index);
            self.outgoing.push(Request::Chunk(index).encode());
        }
    }

    pub fn progress(&self, transfer_bps: u64, reconnecting: bool) -> StreamProgress {
        let cached_bytes: u64 = self.cache.values().map(|data| data.len() as u64).sum();
        let mut ahead = 0;
        let mut index = self.playhead;
        while let Some(data) = self.cache.get(&index) {
            ahead += data.len() as u64;
            index += 1;
        }
        let mut ranges: Vec<(u16, u16)> = Vec::new();
        let mut cached: Vec<u32> = self.cache.keys().copied().collect();
        cached.sort_unstable();
        let chunk_size = u64::from(self.chunk_size);
        for index in cached {
            let start = u64::from(index) * chunk_size * RANGE_BUCKETS / self.file_size.max(1);
            let end = ((u64::from(index) + 1) * chunk_size).min(self.file_size) * RANGE_BUCKETS
                / self.file_size.max(1);
            let (start, end) = (start as u16, (end as u16).max(start as u16 + 1).min(1000));
            if let Some(last) = ranges.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
            } else if ranges.len() < 64 {
                ranges.push((start, end));
            }
        }
        StreamProgress {
            cached_bytes: cached_bytes.min(self.file_size),
            total_bytes: self.file_size,
            transfer_bps,
            link_bps: None,
            buffered_ahead_bytes: ahead.min(self.file_size),
            reconnecting,
            complete: false,
            cached_ranges: ranges,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync2gether_core::chunks::Identifier;

    const CHUNK: u32 = 4096;

    fn host(data: &[u8]) -> (Vec<[u8; 32]>, String) {
        let mut id = Identifier::new(CHUNK);
        id.update(data);
        let (_, hashes) = id.finish();
        let digest = chunks::table_digest(&hashes);
        (hashes, digest)
    }

    fn serve(client: &mut StreamClient, data: &[u8], hashes: &[[u8; 32]], corrupt: Option<u32>) {
        for request in client.take_requests() {
            match Request::decode(&request).unwrap() {
                Request::HashTable => {
                    let table = hashes.concat();
                    for piece in datachannel::fragments(Item::HashTable, &table) {
                        client.on_message(&piece);
                    }
                }
                Request::Chunk(index) => {
                    let start = index as usize * CHUNK as usize;
                    let end = (start + CHUNK as usize).min(data.len());
                    let mut chunk = data[start..end].to_vec();
                    if corrupt == Some(index) {
                        chunk[0] ^= 1;
                    }
                    for piece in datachannel::fragments(Item::Chunk(index), &chunk) {
                        client.on_message(&piece);
                    }
                }
                Request::Cancel => {}
            }
        }
    }

    #[test]
    fn reads_wait_for_verified_chunks_and_return_exact_ranges() {
        let data: Vec<u8> = (0..(CHUNK as usize * 20 + 100))
            .map(|n| (n * 13 % 255) as u8)
            .collect();
        let (hashes, digest) = host(&data);
        let mut client = StreamClient::new(data.len() as u64, CHUNK, &digest, 64 * 1024);
        assert_eq!(client.read(0, 99), None);
        for _ in 0..10 {
            serve(&mut client, &data, &hashes, None);
        }
        assert!(client.ready());
        assert_eq!(client.read(0, 99).unwrap(), data[..100]);
        let start = CHUNK as u64 * 19 + 50;
        let mut got = None;
        for _ in 0..10 {
            got = client.read(start, data.len() as u64 + 10);
            if got.is_some() {
                break;
            }
            serve(&mut client, &data, &hashes, None);
        }
        assert_eq!(got.unwrap(), data[start as usize..]);
        assert!(client.cache.len() <= client.budget);
        let progress = client.progress(0, false);
        assert!(progress.valid());
        assert_eq!(progress.total_bytes, data.len() as u64);
        assert!(client.error().is_none());
    }

    #[test]
    fn corrupted_chunks_are_dropped_and_far_seeks_cancel_prefetch() {
        let data: Vec<u8> = (0..(CHUNK as usize * 40))
            .map(|n| (n % 241) as u8)
            .collect();
        let (hashes, digest) = host(&data);
        let mut client = StreamClient::new(data.len() as u64, CHUNK, &digest, 16 * CHUNK as u64);
        serve(&mut client, &data, &hashes, None);
        assert!(client.ready());
        assert_eq!(client.read(0, 10), None);
        serve(&mut client, &data, &hashes, Some(0));
        assert!(!client.cache.contains_key(&0));
        let mut first = None;
        for _ in 0..5 {
            first = client.read(0, 10);
            if first.is_some() {
                break;
            }
            serve(&mut client, &data, &hashes, None);
        }
        assert_eq!(first.unwrap(), data[..11]);
        let far = CHUNK as u64 * 35;
        assert_eq!(client.read(far, far + 10), None);
        let requests: Vec<Request> = client
            .take_requests()
            .iter()
            .map(|r| Request::decode(r).unwrap())
            .collect();
        let cancel = requests.iter().position(|r| *r == Request::Cancel).unwrap();
        assert_eq!(
            requests[cancel..]
                .iter()
                .find(|r| matches!(r, Request::Chunk(_))),
            Some(&Request::Chunk(35))
        );
    }

    #[test]
    fn a_wrong_hash_table_is_reported() {
        let data = vec![7u8; CHUNK as usize * 2];
        let (hashes, _) = host(&data);
        let mut client = StreamClient::new(data.len() as u64, CHUNK, &"0".repeat(64), 1 << 20);
        serve(&mut client, &data, &hashes, None);
        assert!(!client.ready());
        assert!(client.error().is_some());
    }
}
