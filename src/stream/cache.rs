use super::file_io;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::sync::Notify;

const LOOKAHEAD_CHUNKS: u32 = 32;
const MAX_CHUNK_RETRIES: u8 = 4;
const MAX_TOTAL_CACHE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_CACHED_RANGES: usize = 48;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub cached_bytes: u64,
    pub total_bytes: u64,
    pub chunks_cached: u64,
    pub chunks_total: u64,
    pub buffered_ahead_bytes: u64,
    pub cached_ranges: Vec<(u16, u16)>,
}

struct State {
    data: Option<Arc<File>>,
    bitmap_file: Option<Arc<File>>,
    bitmap: Vec<u8>,
    verified: Vec<u8>,
    chunks_cached: u64,
    cached_bytes: u64,
    urgent: VecDeque<u32>,
    in_flight: HashSet<u32>,
    failures: HashMap<u32, u8>,
    cursor_chunk: u32,
    closed: Option<String>,
}

pub struct ChunkCache {
    dir: PathBuf,
    file_hash: String,
    file_size: u64,
    chunk_size: u32,
    chunks_total: u32,
    hashes: Arc<Vec<[u8; 32]>>,
    state: Mutex<State>,
    bitmap_io: tokio::sync::Mutex<()>,
    chunk_ready: Notify,
    work_available: Notify,
}

pub fn cache_base_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(std::env::temp_dir)
            .join("sync2gether")
            .join("cache")
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(|home| PathBuf::from(home).join("Library").join("Caches"))
            .unwrap_or_else(std::env::temp_dir)
            .join("sync2gether")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|home| PathBuf::from(home).join(".cache"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join("sync2gether")
    }
}

pub fn default_cache_dir(file_hash: &str) -> PathBuf {
    cache_base_dir().join("streams").join(file_hash)
}

fn evict_old_caches(dir: &Path) {
    let Some(parent) = dir.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let mut dirs: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path == dir || !path.is_dir() {
            continue;
        }
        let data_meta = fs::metadata(path.join("data.bin")).ok();
        let size = data_meta.as_ref().map_or(0, |m| m.len());
        let modified = data_meta
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        total = total.saturating_add(size);
        dirs.push((path, modified, size));
    }
    if total <= MAX_TOTAL_CACHE_BYTES {
        return;
    }
    dirs.sort_by_key(|(_, modified, _)| *modified);
    for (path, _, size) in dirs {
        if total <= MAX_TOTAL_CACHE_BYTES {
            break;
        }
        if fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

impl ChunkCache {
    pub async fn open_async(
        dir: PathBuf,
        file_hash: String,
        hash_table_digest: String,
        file_size: u64,
        chunk_size: u32,
        hashes: Arc<Vec<[u8; 32]>>,
    ) -> io::Result<Arc<Self>> {
        tokio::task::spawn_blocking(move || {
            Self::open(
                &dir,
                &file_hash,
                &hash_table_digest,
                file_size,
                chunk_size,
                hashes,
            )
            .map(Arc::new)
        })
        .await
        .map_err(io::Error::other)?
    }

    pub fn open(
        dir: &Path,
        file_hash: &str,
        hash_table_digest: &str,
        file_size: u64,
        chunk_size: u32,
        hashes: Arc<Vec<[u8; 32]>>,
    ) -> io::Result<Self> {
        if chunk_size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Tamanho de bloco inválido",
            ));
        }
        let chunks_total = if file_size == 0 {
            0
        } else {
            file_size.div_ceil(u64::from(chunk_size)) as u32
        };
        if hashes.len() != chunks_total as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Tabela de hashes incompatível com o tamanho do arquivo",
            ));
        }
        evict_old_caches(dir);
        fs::create_dir_all(dir)?;
        let bitmap_bytes = (chunks_total as usize).div_ceil(8);
        let meta_path = dir.join("meta.txt");
        let data_path = dir.join("data.bin");
        let bitmap_path = dir.join("bitmap.bin");
        let expected_meta = format!("{file_hash}:{file_size}:{chunk_size}:{hash_table_digest}");

        let resumable = fs::read_to_string(&meta_path)
            .ok()
            .is_some_and(|m| m.trim() == expected_meta)
            && fs::metadata(&data_path)
                .ok()
                .is_some_and(|m| m.len() == file_size)
            && fs::metadata(&bitmap_path)
                .ok()
                .is_some_and(|m| m.len() as usize == bitmap_bytes);

        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(!resumable)
            .open(&data_path)?;
        data.set_len(file_size)?;

        let bitmap_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(!resumable)
            .open(&bitmap_path)?;
        bitmap_file.set_len(bitmap_bytes as u64)?;

        let mut bitmap = vec![0u8; bitmap_bytes];
        if resumable {
            file_io::read_exact_at(&bitmap_file, &mut bitmap, 0)?;
        } else {
            fs::write(&meta_path, expected_meta)?;
        }

        let mut chunks_cached = 0u64;
        let mut cached_bytes = 0u64;
        for idx in 0..chunks_total {
            if bit_get(&bitmap, idx) {
                chunks_cached += 1;
                cached_bytes += chunk_len_for(file_size, chunk_size, idx) as u64;
            }
        }

        Ok(Self {
            dir: dir.to_path_buf(),
            file_hash: file_hash.to_owned(),
            file_size,
            chunk_size,
            chunks_total,
            hashes,
            state: Mutex::new(State {
                data: Some(Arc::new(data)),
                bitmap_file: Some(Arc::new(bitmap_file)),
                bitmap,
                verified: vec![0u8; bitmap_bytes],
                chunks_cached,
                cached_bytes,
                urgent: VecDeque::new(),
                in_flight: HashSet::new(),
                failures: HashMap::new(),
                cursor_chunk: 0,
                closed: None,
            }),
            bitmap_io: tokio::sync::Mutex::new(()),
            chunk_ready: Notify::new(),
            work_available: Notify::new(),
        })
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    pub fn chunks_total(&self) -> u32 {
        self.chunks_total
    }

    pub fn chunk_len(&self, idx: u32) -> usize {
        chunk_len_for(self.file_size, self.chunk_size, idx)
    }

    pub fn has_chunk(&self, idx: u32) -> bool {
        if idx >= self.chunks_total {
            return false;
        }
        bit_get(&self.state.lock().unwrap().bitmap, idx)
    }

    pub fn is_complete(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.chunks_cached >= u64::from(self.chunks_total)
    }

    pub fn closed_reason(&self) -> Option<String> {
        self.state.lock().unwrap().closed.clone()
    }

    pub fn matches_offer(&self, file_hash: &str, file_size: u64, chunk_size: u32) -> bool {
        self.file_hash.eq_ignore_ascii_case(file_hash)
            && self.file_size == file_size
            && self.chunk_size == chunk_size
            && self.state.lock().unwrap().closed.is_none()
    }

    pub fn release_chunk(&self, idx: u32) {
        let mut state = self.state.lock().unwrap();
        state.in_flight.remove(&idx);
        drop(state);
        self.work_available.notify_waiters();
    }

    pub fn release_in_flight(&self) {
        let mut state = self.state.lock().unwrap();
        state.in_flight.clear();
        drop(state);
        self.work_available.notify_waiters();
    }

    pub fn set_cursor(&self, byte_offset: u64) {
        if self.chunks_total == 0 {
            return;
        }
        let chunk = ((byte_offset / u64::from(self.chunk_size)) as u32)
            .min(self.chunks_total.saturating_sub(1));
        self.state.lock().unwrap().cursor_chunk = chunk;
        self.work_available.notify_waiters();
    }

    pub fn close_with_error(&self, reason: String) {
        self.state.lock().unwrap().closed = Some(reason);
        self.chunk_ready.notify_waiters();
        self.work_available.notify_waiters();
    }

    pub fn request_chunk(&self, idx: u32) {
        if idx >= self.chunks_total {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if !bit_get(&state.bitmap, idx) && !state.urgent.contains(&idx) {
            state.urgent.push_back(idx);
            self.work_available.notify_one();
        }
    }

    fn data_file(&self) -> io::Result<Arc<File>> {
        self.state
            .lock()
            .unwrap()
            .data
            .clone()
            .ok_or_else(|| io::Error::other("Cache fechado"))
    }

    fn bitmap_file(&self) -> io::Result<Arc<File>> {
        self.state
            .lock()
            .unwrap()
            .bitmap_file
            .clone()
            .ok_or_else(|| io::Error::other("Cache fechado"))
    }

    pub async fn read_at_blocking(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        if offset >= self.file_size || len == 0 {
            return Ok(Vec::new());
        }
        self.set_cursor(offset);
        let chunk = (offset / u64::from(self.chunk_size)) as u32;
        loop {
            self.wait_chunk(chunk).await?;
            let needs_verify = {
                let state = self.state.lock().unwrap();
                !bit_get(&state.verified, chunk)
            };
            if needs_verify {
                let chunk_offset = u64::from(chunk) * u64::from(self.chunk_size);
                let chunk_len = self.chunk_len(chunk);
                let data_file = self.data_file()?;
                let full_chunk = tokio::task::spawn_blocking(move || {
                    let mut buf = vec![0u8; chunk_len];
                    file_io::read_exact_at(&data_file, &mut buf, chunk_offset).map(|()| buf)
                })
                .await
                .map_err(io::Error::other)??;

                let expected = self.hashes[chunk as usize];
                if blake3::hash(&full_chunk).as_bytes() == &expected {
                    let mut state = self.state.lock().unwrap();
                    let byte_idx = (chunk / 8) as usize;
                    state.verified[byte_idx] |= 1 << (chunk % 8);
                    let in_chunk = (offset - chunk_offset) as usize;
                    let available = chunk_len.saturating_sub(in_chunk);
                    let to_read = len.min(available);
                    return Ok(full_chunk[in_chunk..in_chunk + to_read].to_vec());
                }
                self.invalidate_corrupt_chunk(chunk).await;
                continue;
            }
            let chunk_start = u64::from(chunk) * u64::from(self.chunk_size);
            let chunk_end = (chunk_start + u64::from(self.chunk_size)).min(self.file_size);
            let available = (chunk_end - offset) as usize;
            let to_read = len.min(available);
            let data_file = self.data_file()?;
            return tokio::task::spawn_blocking(move || {
                let mut buf = vec![0u8; to_read];
                file_io::read_exact_at(&data_file, &mut buf, offset).map(|()| buf)
            })
            .await
            .map_err(io::Error::other)?;
        }
    }

    async fn persist_bitmap_byte(&self, byte_idx: usize) -> io::Result<()> {
        let _guard = self.bitmap_io.lock().await;
        let val = self.state.lock().unwrap().bitmap[byte_idx];
        let bitmap_file = self.bitmap_file()?;
        tokio::task::spawn_blocking(move || {
            file_io::write_all_at(&bitmap_file, &[val], byte_idx as u64)
        })
        .await
        .map_err(io::Error::other)?
    }

    async fn flush_bitmap(&self) -> io::Result<()> {
        let _guard = self.bitmap_io.lock().await;
        let bitmap = self.state.lock().unwrap().bitmap.clone();
        let bitmap_file = self.bitmap_file()?;
        tokio::task::spawn_blocking(move || file_io::write_all_at(&bitmap_file, &bitmap, 0))
            .await
            .map_err(io::Error::other)?
    }

    async fn invalidate_corrupt_chunk(&self, idx: u32) {
        let byte_idx = {
            let mut state = self.state.lock().unwrap();
            let byte_idx = (idx / 8) as usize;
            let mask = 1 << (idx % 8);
            state.verified[byte_idx] &= !mask;
            if (state.bitmap[byte_idx] & mask) != 0 {
                state.bitmap[byte_idx] &= !mask;
                state.chunks_cached = state.chunks_cached.saturating_sub(1);
                state.cached_bytes = state
                    .cached_bytes
                    .saturating_sub(self.chunk_len(idx) as u64);
                if !state.urgent.contains(&idx) {
                    state.urgent.push_front(idx);
                }
                Some(byte_idx)
            } else {
                None
            }
        };
        if let Some(byte_idx) = byte_idx {
            let _ = self.persist_bitmap_byte(byte_idx).await;
        }
        self.work_available.notify_waiters();
    }

    pub async fn wait_chunk(&self, idx: u32) -> io::Result<()> {
        if idx >= self.chunks_total {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Índice de bloco fora do arquivo",
            ));
        }
        loop {
            let notified = self.chunk_ready.notified();
            {
                let mut state = self.state.lock().unwrap();
                if bit_get(&state.bitmap, idx) {
                    return Ok(());
                }
                if let Some(reason) = &state.closed {
                    return Err(io::Error::other(reason.clone()));
                }
                if !state.urgent.contains(&idx) {
                    state.urgent.push_back(idx);
                }
            }
            self.work_available.notify_waiters();
            notified.await;
        }
    }

    pub async fn next_chunk_to_fetch(&self) -> Option<u32> {
        loop {
            let notified = self.work_available.notified();
            {
                let mut state = self.state.lock().unwrap();
                if state.closed.is_some() {
                    return None;
                }
                if state.chunks_cached >= u64::from(self.chunks_total) {
                    return None;
                }
                if let Some(idx) = pick_next(&mut state, self.chunks_total) {
                    state.in_flight.insert(idx);
                    return Some(idx);
                }
            }
            notified.await;
        }
    }

    pub async fn complete_chunk(&self, idx: u32, data: Vec<u8>) -> io::Result<()> {
        if idx >= self.chunks_total {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Índice de bloco inválido",
            ));
        }
        let expected_len = self.chunk_len(idx);
        let expected_hash = self.hashes[idx as usize];
        if data.len() != expected_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tamanho de bloco inesperado",
            ));
        }
        if blake3::hash(&data).as_bytes() != &expected_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Integridade do bloco falhou (BLAKE3)",
            ));
        }
        let offset = u64::from(idx) * u64::from(self.chunk_size);
        let data_len = data.len() as u64;
        let data_file = self.data_file()?;
        tokio::task::spawn_blocking(move || file_io::write_all_at(&data_file, &data, offset))
            .await
            .map_err(io::Error::other)??;

        let byte_idx = {
            let mut state = self.state.lock().unwrap();
            state.in_flight.remove(&idx);
            state.failures.remove(&idx);
            let byte_idx = (idx / 8) as usize;
            let mask = 1 << (idx % 8);
            state.verified[byte_idx] |= mask;
            if (state.bitmap[byte_idx] & mask) == 0 {
                state.bitmap[byte_idx] |= mask;
                state.chunks_cached += 1;
                state.cached_bytes += data_len;
                Some(byte_idx)
            } else {
                None
            }
        };
        if let Some(byte_idx) = byte_idx {
            self.persist_bitmap_byte(byte_idx).await?;
        }
        self.chunk_ready.notify_waiters();
        self.work_available.notify_waiters();
        Ok(())
    }

    pub fn record_chunk_failure(&self, idx: u32, reason: &str) -> Result<u8, String> {
        let mut state = self.state.lock().unwrap();
        state.in_flight.remove(&idx);
        let count = state.failures.entry(idx).or_insert(0);
        *count = count.saturating_add(1);
        let attempts = *count;
        if attempts >= MAX_CHUNK_RETRIES {
            let msg = format!("Falha ao receber bloco {idx}: {reason}");
            state.closed = Some(msg.clone());
            drop(state);
            self.chunk_ready.notify_waiters();
            self.work_available.notify_waiters();
            return Err(msg);
        }
        drop(state);
        self.work_available.notify_waiters();
        Ok(attempts)
    }

    pub async fn verify_full_file_hash(&self) -> io::Result<bool> {
        self.flush_bitmap().await?;
        let path = self.dir.join("data.bin");
        let expected = self.file_hash.clone();
        let file_size = self.file_size;
        tokio::task::spawn_blocking(move || {
            let mut file = File::open(&path)?;
            let mut hasher = blake3::Hasher::new();
            let mut buf = vec![0u8; 256 * 1024];
            let mut read_total = 0u64;
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                read_total += n as u64;
            }
            Ok(read_total == file_size
                && hasher.finalize().to_hex().eq_ignore_ascii_case(&expected))
        })
        .await
        .map_err(io::Error::other)?
    }

    pub async fn wipe_cache(&self) {
        let _guard = self.bitmap_io.lock().await;
        {
            let mut state = self.state.lock().unwrap();
            state.data = None;
            state.bitmap_file = None;
        }
        let dir = self.dir.clone();
        let _ = tokio::task::spawn_blocking(move || fs::remove_dir_all(&dir)).await;
    }

    pub fn stats(&self) -> CacheStats {
        let state = self.state.lock().unwrap();
        let mut buffered_ahead_bytes = 0u64;
        for idx in state.cursor_chunk..self.chunks_total {
            if bit_get(&state.bitmap, idx) {
                buffered_ahead_bytes += self.chunk_len(idx) as u64;
            } else {
                break;
            }
        }
        let cached_ranges = summarize_bitmap(&state.bitmap, self.chunks_total);
        CacheStats {
            cached_bytes: state.cached_bytes,
            total_bytes: self.file_size,
            chunks_cached: state.chunks_cached,
            chunks_total: u64::from(self.chunks_total),
            buffered_ahead_bytes,
            cached_ranges,
        }
    }
}

pub fn summarize_bitmap(bitmap: &[u8], chunks_total: u32) -> Vec<(u16, u16)> {
    if chunks_total == 0 {
        return Vec::new();
    }
    let total = u64::from(chunks_total);
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    let mut idx = 0u32;
    while idx < chunks_total {
        if !bit_get(bitmap, idx) {
            idx += 1;
            continue;
        }
        let start_chunk = idx;
        while idx < chunks_total && bit_get(bitmap, idx) {
            idx += 1;
        }
        let mut start = ((u64::from(start_chunk) * 1000) / total) as u16;
        let mut end = ((u64::from(idx) * 1000).div_ceil(total)).min(1000) as u16;
        if start >= end {
            if start >= 1000 {
                start = 999;
                end = 1000;
            } else {
                end = (start + 1).min(1000);
            }
        }
        if let Some(last) = ranges.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            ranges.push((start, end));
        }
    }
    while ranges.len() > MAX_CACHED_RANGES {
        let mut best_i = 0;
        let mut best_gap = u16::MAX;
        for i in 0..ranges.len() - 1 {
            let gap = ranges[i + 1].0.saturating_sub(ranges[i].1);
            if gap < best_gap {
                best_gap = gap;
                best_i = i;
            }
        }
        let next = ranges.remove(best_i + 1);
        ranges[best_i].1 = ranges[best_i].1.max(next.1);
    }
    ranges
}

fn pick_next(state: &mut State, total: u32) -> Option<u32> {
    while let Some(idx) = state.urgent.pop_front() {
        if idx < total && !bit_get(&state.bitmap, idx) && !state.in_flight.contains(&idx) {
            return Some(idx);
        }
    }
    let lookahead_end = state
        .cursor_chunk
        .saturating_add(LOOKAHEAD_CHUNKS)
        .min(total);
    for idx in state.cursor_chunk..lookahead_end {
        if !bit_get(&state.bitmap, idx) && !state.in_flight.contains(&idx) {
            return Some(idx);
        }
    }
    for offset in 0..total {
        let idx = (lookahead_end + offset) % total;
        if !bit_get(&state.bitmap, idx) && !state.in_flight.contains(&idx) {
            return Some(idx);
        }
    }
    None
}

fn bit_get(bitmap: &[u8], idx: u32) -> bool {
    let byte = (idx / 8) as usize;
    byte < bitmap.len() && (bitmap[byte] & (1 << (idx % 8))) != 0
}

pub fn chunk_len_for(file_size: u64, chunk_size: u32, idx: u32) -> usize {
    let start = u64::from(idx) * u64::from(chunk_size);
    if start >= file_size {
        return 0;
    }
    (file_size - start).min(u64::from(chunk_size)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::ChunkIndex;

    fn make_hashes(data: &[u8], chunk_size: u32) -> (String, ChunkIndex) {
        let hash = blake3::hash(data).to_hex().to_string();
        let hashes: Vec<[u8; 32]> = data
            .chunks(chunk_size as usize)
            .map(|c| *blake3::hash(c).as_bytes())
            .collect();
        (
            hash,
            ChunkIndex::from_hashes(chunk_size, data.len() as u64, hashes),
        )
    }

    #[tokio::test]
    async fn cache_verifies_blake3_persists_resumes_and_detects_corruption() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-cache-test");
        let _ = fs::remove_dir_all(&dir);
        let data: Vec<u8> = (0..(1024 * 600)).map(|i| (i % 251) as u8).collect();
        let chunk_size = 256 * 1024;
        let (hash, index) = make_hashes(&data, chunk_size);
        let hashes = Arc::new(index.hashes.clone());

        let cache = ChunkCache::open(
            &dir,
            &hash,
            &index.digest,
            data.len() as u64,
            chunk_size,
            hashes.clone(),
        )
        .unwrap();
        assert_eq!(cache.chunks_total(), 3);

        let mut bad = data[0..chunk_size as usize].to_vec();
        bad[0] ^= 0xff;
        assert!(cache.complete_chunk(0, bad).await.is_err());
        assert!(!cache.has_chunk(0));

        cache
            .complete_chunk(0, data[0..chunk_size as usize].to_vec())
            .await
            .unwrap();
        assert!(cache.has_chunk(0));
        drop(cache);

        let wrong_digest_cache = ChunkCache::open(
            &dir,
            &hash,
            &"0".repeat(64),
            data.len() as u64,
            chunk_size,
            hashes.clone(),
        )
        .unwrap();
        assert!(!wrong_digest_cache.has_chunk(0));
        drop(wrong_digest_cache);

        let cache = ChunkCache::open(
            &dir,
            &hash,
            &index.digest,
            data.len() as u64,
            chunk_size,
            hashes.clone(),
        )
        .unwrap();
        cache
            .complete_chunk(0, data[0..chunk_size as usize].to_vec())
            .await
            .unwrap();
        drop(cache);

        let resumed = ChunkCache::open(
            &dir,
            &hash,
            &index.digest,
            data.len() as u64,
            chunk_size,
            hashes.clone(),
        )
        .unwrap();
        assert!(resumed.has_chunk(0));
        let out = resumed.read_at_blocking(128, 64).await.unwrap();
        assert_eq!(out, data[128..192]);
        drop(resumed);

        let data_file = OpenOptions::new()
            .write(true)
            .open(dir.join("data.bin"))
            .unwrap();
        file_io::write_all_at(&data_file, &[0xde, 0xad], 0).unwrap();
        drop(data_file);

        let corrupted = Arc::new(
            ChunkCache::open(
                &dir,
                &hash,
                &index.digest,
                data.len() as u64,
                chunk_size,
                hashes,
            )
            .unwrap(),
        );
        assert!(corrupted.has_chunk(0));
        let reader = {
            let corrupted = corrupted.clone();
            tokio::spawn(async move { corrupted.read_at_blocking(0, 16).await })
        };
        let refetch = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !corrupted.has_chunk(0) {
                    return corrupted.next_chunk_to_fetch().await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(refetch, Some(0));
        corrupted
            .complete_chunk(0, data[0..chunk_size as usize].to_vec())
            .await
            .unwrap();
        assert_eq!(reader.await.unwrap().unwrap(), data[0..16]);
        drop(corrupted);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scheduler_prioritizes_urgent_over_lookahead_and_background() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-sched-test");
        let _ = fs::remove_dir_all(&dir);
        let hashes = Arc::new(vec![[0u8; 32]; 10]);
        let cache = ChunkCache::open(
            &dir,
            &"a".repeat(64),
            &"b".repeat(64),
            10 * 1024,
            1024,
            hashes,
        )
        .unwrap();
        cache.set_cursor(3 * 1024);
        cache.request_chunk(9);
        assert_eq!(cache.next_chunk_to_fetch().await, Some(9));
        assert_eq!(cache.next_chunk_to_fetch().await, Some(3));
        assert_eq!(cache.next_chunk_to_fetch().await, Some(4));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_completions_in_same_bitmap_byte_persist_all_bits() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-bitmap-race-test");
        let chunk_size = 4096u32;
        let data: Vec<u8> = (0..(chunk_size as usize * 8))
            .map(|i| ((i * 17) ^ (i >> 3)) as u8)
            .collect();
        let (hash, index) = make_hashes(&data, chunk_size);
        let hashes = Arc::new(index.hashes.clone());

        for _ in 0..16 {
            let _ = fs::remove_dir_all(&dir);
            let cache = Arc::new(
                ChunkCache::open(
                    &dir,
                    &hash,
                    &index.digest,
                    data.len() as u64,
                    chunk_size,
                    hashes.clone(),
                )
                .unwrap(),
            );
            let mut tasks = tokio::task::JoinSet::new();
            for idx in 0..8u32 {
                let cache = cache.clone();
                let start = idx as usize * chunk_size as usize;
                let chunk = data[start..start + chunk_size as usize].to_vec();
                tasks.spawn(async move {
                    cache.complete_chunk(idx, chunk).await.unwrap();
                });
            }
            while let Some(res) = tasks.join_next().await {
                res.unwrap();
            }
            drop(cache);

            let resumed = ChunkCache::open(
                &dir,
                &hash,
                &index.digest,
                data.len() as u64,
                chunk_size,
                hashes.clone(),
            )
            .unwrap();
            assert!(resumed.is_complete());
            assert_eq!(resumed.stats().cached_bytes, data.len() as u64);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn summarize_bitmap_merges_runs_handles_gaps_and_caps_span_count() {
        assert!(summarize_bitmap(&[], 0).is_empty());
        assert!(summarize_bitmap(&[0u8; 4], 32).is_empty());

        let mut bitmap = vec![0u8; 13];
        for idx in [0u32, 1, 2, 3, 50, 51, 99] {
            bitmap[(idx / 8) as usize] |= 1 << (idx % 8);
        }
        let ranges = summarize_bitmap(&bitmap, 100);
        assert_eq!(ranges, vec![(0, 40), (500, 520), (990, 1000)]);

        let full = vec![0xffu8; 13];
        assert_eq!(summarize_bitmap(&full, 100), vec![(0, 1000)]);

        let mut fragmented = vec![0u8; 250];
        for idx in (0..2000u32).step_by(4) {
            fragmented[(idx / 8) as usize] |= 1 << (idx % 8);
        }
        let capped = summarize_bitmap(&fragmented, 2000);
        assert!(capped.len() <= MAX_CACHED_RANGES);
        assert!(capped.iter().all(|&(s, e)| s < e && e <= 1000));
    }

    #[tokio::test]
    async fn release_in_flight_allows_re_fetching_after_disconnect() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-release-test");
        let _ = fs::remove_dir_all(&dir);
        let hashes = Arc::new(vec![[0u8; 32]; 4]);
        let cache = ChunkCache::open(
            &dir,
            &"c".repeat(64),
            &"d".repeat(64),
            4 * 1024,
            1024,
            hashes,
        )
        .unwrap();
        assert_eq!(cache.next_chunk_to_fetch().await, Some(0));
        assert_eq!(cache.next_chunk_to_fetch().await, Some(1));
        assert!(cache.record_chunk_failure(0, "blip").is_ok());
        assert!(cache.record_chunk_failure(0, "blip").is_ok());
        assert!(cache.record_chunk_failure(0, "blip").is_ok());
        cache.release_in_flight();
        assert!(cache.closed_reason().is_none());
        assert_eq!(cache.next_chunk_to_fetch().await, Some(0));
        assert_eq!(cache.next_chunk_to_fetch().await, Some(1));
        assert!(cache.record_chunk_failure(0, "corrupt").is_err());
        assert!(cache.closed_reason().is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
