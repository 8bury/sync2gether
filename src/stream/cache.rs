use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tokio::sync::Notify;

const LOOKAHEAD_CHUNKS: u32 = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub cached_bytes: u64,
    pub total_bytes: u64,
    pub chunks_cached: u64,
    pub chunks_total: u64,
    pub buffered_ahead_bytes: u64,
}

struct State {
    bitmap: Vec<u8>,
    chunks_cached: u64,
    cached_bytes: u64,
    urgent: VecDeque<u32>,
    in_flight: HashSet<u32>,
    cursor_chunk: u32,
    closed: Option<String>,
}

pub struct ChunkCache {
    data: File,
    bitmap_file: File,
    file_size: u64,
    chunk_size: u32,
    chunks_total: u32,
    state: Mutex<State>,
    chunk_ready: Notify,
    work_available: Notify,
}

pub fn default_cache_dir(file_hash: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".cache"))
        })
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("sync2gether").join("streams").join(file_hash)
}

impl ChunkCache {
    pub fn open(dir: &Path, file_hash: &str, file_size: u64, chunk_size: u32) -> io::Result<Self> {
        if chunk_size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Tamanho de bloco inválido",
            ));
        }
        fs::create_dir_all(dir)?;
        let chunks_total = if file_size == 0 {
            0
        } else {
            file_size.div_ceil(u64::from(chunk_size)) as u32
        };
        let bitmap_bytes = (chunks_total as usize).div_ceil(8);
        let meta_path = dir.join("meta.txt");
        let data_path = dir.join("data.bin");
        let bitmap_path = dir.join("bitmap.bin");
        let expected_meta = format!("{file_hash}:{file_size}:{chunk_size}");

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
            bitmap_file.read_exact_at(&mut bitmap, 0)?;
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
            data,
            bitmap_file,
            file_size,
            chunk_size,
            chunks_total,
            state: Mutex::new(State {
                bitmap,
                chunks_cached,
                cached_bytes,
                urgent: VecDeque::new(),
                in_flight: HashSet::new(),
                cursor_chunk: 0,
                closed: None,
            }),
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

    pub async fn read_at_blocking(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.file_size || buf.is_empty() {
            return Ok(0);
        }
        self.set_cursor(offset);
        let chunk = (offset / u64::from(self.chunk_size)) as u32;
        self.wait_chunk(chunk).await?;
        let chunk_start = u64::from(chunk) * u64::from(self.chunk_size);
        let chunk_end = (chunk_start + u64::from(self.chunk_size)).min(self.file_size);
        let available = (chunk_end - offset) as usize;
        let to_read = buf.len().min(available);
        self.data.read_exact_at(&mut buf[..to_read], offset)?;
        Ok(to_read)
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

    pub fn complete_chunk(
        &self,
        idx: u32,
        data: &[u8],
        expected_hash: &[u8; 32],
    ) -> io::Result<()> {
        if idx >= self.chunks_total {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Índice de bloco inválido",
            ));
        }
        let expected_len = self.chunk_len(idx);
        if data.len() != expected_len {
            self.release_in_flight(idx);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tamanho de bloco inesperado",
            ));
        }
        if blake3::hash(data).as_bytes() != expected_hash {
            self.release_in_flight(idx);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Integridade do bloco falhou (BLAKE3)",
            ));
        }
        let offset = u64::from(idx) * u64::from(self.chunk_size);
        self.data.write_all_at(data, offset)?;
        {
            let mut state = self.state.lock().unwrap();
            state.in_flight.remove(&idx);
            if !bit_get(&state.bitmap, idx) {
                let byte_idx = (idx / 8) as usize;
                state.bitmap[byte_idx] |= 1 << (idx % 8);
                let byte = [state.bitmap[byte_idx]];
                let _ = self.bitmap_file.write_all_at(&byte, byte_idx as u64);
                state.chunks_cached += 1;
                state.cached_bytes += data.len() as u64;
            }
        }
        self.chunk_ready.notify_waiters();
        self.work_available.notify_waiters();
        Ok(())
    }

    pub fn release_in_flight(&self, idx: u32) {
        self.state.lock().unwrap().in_flight.remove(&idx);
        self.work_available.notify_waiters();
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
        CacheStats {
            cached_bytes: state.cached_bytes,
            total_bytes: self.file_size,
            chunks_cached: state.chunks_cached,
            chunks_total: u64::from(self.chunks_total),
            buffered_ahead_bytes,
        }
    }
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

    #[tokio::test]
    async fn cache_verifies_blake3_persists_and_resumes() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-cache-test");
        let _ = fs::remove_dir_all(&dir);
        let data: Vec<u8> = (0..(1024 * 600)).map(|i| (i % 251) as u8).collect();
        let hash = blake3::hash(&data).to_hex().to_string();
        let chunk_size = 256 * 1024;
        let cache = ChunkCache::open(&dir, &hash, data.len() as u64, chunk_size).unwrap();
        assert_eq!(cache.chunks_total(), 3);
        assert_eq!(cache.stats().cached_bytes, 0);

        let c0 = &data[0..chunk_size as usize];
        let h0 = *blake3::hash(c0).as_bytes();
        let bad_hash = [0u8; 32];
        assert!(cache.complete_chunk(0, c0, &bad_hash).is_err());
        assert!(!cache.has_chunk(0));

        cache.complete_chunk(0, c0, &h0).unwrap();
        assert!(cache.has_chunk(0));
        assert_eq!(cache.stats().cached_bytes, chunk_size as u64);

        drop(cache);
        let resumed = ChunkCache::open(&dir, &hash, data.len() as u64, chunk_size).unwrap();
        assert!(resumed.has_chunk(0));
        assert!(!resumed.has_chunk(1));
        let mut out = vec![0u8; 64];
        let n = resumed.read_at_blocking(128, &mut out).await.unwrap();
        assert_eq!(n, 64);
        assert_eq!(&out, &data[128..192]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scheduler_prioritizes_urgent_over_lookahead_and_background() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/stream-sched-test");
        let _ = fs::remove_dir_all(&dir);
        let cache = ChunkCache::open(&dir, &"a".repeat(64), 10 * 1024, 1024).unwrap();
        cache.set_cursor(3 * 1024);
        cache.request_chunk(9);
        assert_eq!(cache.next_chunk_to_fetch().await, Some(9));
        assert_eq!(cache.next_chunk_to_fetch().await, Some(3));
        assert_eq!(cache.next_chunk_to_fetch().await, Some(4));
        let _ = fs::remove_dir_all(&dir);
    }
}
