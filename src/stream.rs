pub mod cache;
pub mod ffmpeg;
pub(crate) mod file_io;
pub mod guest;
pub mod host;
pub mod http;
pub mod nat;
pub mod remux;
pub mod tls;
pub mod webrtc;
pub mod wire;

use crate::protocol::Media;
use std::{
    fs::File,
    io::{self, Read},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};

pub use sync2gether_core::chunks::CHUNK_SIZE;

#[derive(Clone, Debug)]
pub struct ChunkIndex {
    pub chunk_size: u32,
    pub file_size: u64,
    pub hashes: Vec<[u8; 32]>,
    pub digest: String,
}

impl ChunkIndex {
    pub fn from_hashes(chunk_size: u32, file_size: u64, hashes: Vec<[u8; 32]>) -> Self {
        let digest = sync2gether_core::chunks::table_digest(&hashes);
        Self {
            chunk_size,
            file_size,
            hashes,
            digest,
        }
    }

    pub fn serialize_hashes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.hashes.len() * 32);
        for hash in &self.hashes {
            out.extend_from_slice(hash);
        }
        out
    }

    pub fn deserialize_verified(
        raw: &[u8],
        expected_digest: &str,
        file_size: u64,
        chunk_size: u32,
    ) -> io::Result<Vec<[u8; 32]>> {
        let t = crate::i18n::text();
        if raw.len() != sync2gether_core::chunks::chunk_count(file_size, chunk_size) * 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                t.stream_errors.hash_table_size,
            ));
        }
        sync2gether_core::chunks::verify_table(raw, expected_digest, file_size, chunk_size).map_err(
            |_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    t.stream_errors.hash_table_integrity,
                )
            },
        )
    }

    pub fn compute_file(path: &Path, chunk_size: u32) -> io::Result<(Media, Self)> {
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        let mut whole = blake3::Hasher::new();
        let mut hashes = Vec::new();
        let mut buf = vec![0u8; chunk_size as usize];
        let mut total = 0u64;
        loop {
            let mut filled = 0;
            while filled < buf.len() {
                let n = file.read(&mut buf[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            whole.update(&buf[..filled]);
            hashes.push(*blake3::hash(&buf[..filled]).as_bytes());
            total += filled as u64;
        }
        if total != meta.len() {
            return Err(io::Error::other(
                crate::i18n::text().stream_errors.file_changed_indexing,
            ));
        }
        let media = Media {
            hash: whole.finalize().to_hex().to_string(),
            bytes: total,
        };
        Ok((media, Self::from_hashes(chunk_size, total, hashes)))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StreamPhase {
    #[default]
    Idle,
    Offering,
    Connecting,
    Probing,
    Streaming,
    Reconnecting,
    Complete,
    Error(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BitrateAdvice {
    #[default]
    Unknown,
    Ready,
    Tight,
    Prebuffer {
        estimated_secs: u64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamTelemetry {
    pub active: bool,
    pub is_sender: bool,
    pub phase: StreamPhase,
    pub peer_addr: Option<SocketAddr>,
    pub link_bps: Option<u64>,
    pub file_bitrate_bps: Option<u64>,
    pub transfer_bps: u64,
    pub cached_bytes: u64,
    pub total_bytes: u64,
    pub buffered_ahead_bytes: u64,
    pub chunks_cached: u64,
    pub chunks_total: u64,
    pub advice: BitrateAdvice,
    pub cached_ranges: Vec<(u16, u16)>,
}

impl StreamTelemetry {
    pub fn cache_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.cached_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }

    pub fn buffered_seconds(&self) -> Option<f64> {
        let bitrate = self.file_bitrate_bps.filter(|&b| b > 0)?;
        Some((self.buffered_ahead_bytes as f64 * 8.0) / bitrate as f64)
    }
}

pub fn file_bitrate_bps(file_bytes: u64, duration_secs: f64) -> Option<u64> {
    if file_bytes == 0 || !duration_secs.is_finite() || duration_secs <= 0.5 {
        return None;
    }
    Some(((file_bytes as f64 * 8.0) / duration_secs).round() as u64)
}

pub fn compute_bitrate_advice(
    link_bps: Option<u64>,
    file_bitrate_bps: Option<u64>,
    remaining_bytes: u64,
) -> BitrateAdvice {
    let (Some(link), Some(bitrate)) = (link_bps, file_bitrate_bps) else {
        return BitrateAdvice::Unknown;
    };
    if link == 0 || bitrate == 0 {
        return BitrateAdvice::Unknown;
    }
    let ratio = link as f64 / bitrate as f64;
    if ratio >= 1.35 {
        BitrateAdvice::Ready
    } else if ratio >= 0.85 {
        BitrateAdvice::Tight
    } else {
        let deficit_ratio = (1.0 - ratio).clamp(0.05, 0.99);
        let bytes_needed = remaining_bytes as f64 * deficit_ratio;
        let secs = ((bytes_needed * 8.0) / link as f64).ceil() as u64;
        BitrateAdvice::Prebuffer {
            estimated_secs: secs.max(1),
        }
    }
}

pub fn format_bps(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:.0} kbps", bps as f64 / 1_000.0)
    } else {
        format!("{bps} bps")
    }
}

pub enum StreamEvent {
    Telemetry {
        id: u64,
        telemetry: StreamTelemetry,
    },
    Ready {
        id: u64,
        url: String,
        media: Media,
        file_name: String,
        hash_table_digest: String,
        cache: Arc<cache::ChunkCache>,
        proxy: Arc<http::LocalProxy>,
        telemetry: StreamTelemetry,
    },
    Disconnected {
        id: u64,
        reason: String,
    },
    Ended {
        id: u64,
    },
    Failed {
        id: u64,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_advice_classifies_ready_tight_and_prebuffer() {
        assert_eq!(
            compute_bitrate_advice(Some(50_000_000), Some(10_000_000), 1_000_000),
            BitrateAdvice::Ready
        );
        assert_eq!(
            compute_bitrate_advice(Some(10_000_000), Some(10_000_000), 1_000_000),
            BitrateAdvice::Tight
        );
        assert!(matches!(
            compute_bitrate_advice(Some(5_000_000), Some(20_000_000), 100_000_000),
            BitrateAdvice::Prebuffer { estimated_secs } if estimated_secs > 10
        ));
    }

    #[test]
    fn chunk_index_roundtrips_and_rejects_tampering() {
        let h1 = *blake3::hash(b"chunk-one").as_bytes();
        let h2 = *blake3::hash(b"chunk-two").as_bytes();
        let index = ChunkIndex::from_hashes(1024, 1500, vec![h1, h2]);
        let raw = index.serialize_hashes();
        let decoded = ChunkIndex::deserialize_verified(&raw, &index.digest, 1500, 1024).unwrap();
        assert_eq!(decoded, vec![h1, h2]);

        let mut tampered = raw.clone();
        tampered[0] ^= 0xff;
        assert!(ChunkIndex::deserialize_verified(&tampered, &index.digest, 1500, 1024).is_err());
    }
}
