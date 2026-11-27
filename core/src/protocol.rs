use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 5;
pub const PORT: u16 = 7842;
pub const MAX_MESSAGE: usize = 16 * 1024;
pub const MAX_HASH_TABLE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SUBTITLE_BYTES: usize = 1024 * 1024;
pub const MAX_SUBTITLE_CHUNK: usize = 6 * 1024;
pub const MAX_SUBTITLE_CHUNKS: u32 = 256;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub hash: String,
    pub bytes: u64,
}
impl Media {
    pub fn valid(&self) -> bool {
        self.hash.len() == 64 && self.hash.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StreamOffer {
    pub id: u64,
    pub media: Media,
    pub file_name: String,
    pub duration: f64,
    pub chunk_size: u32,
    pub hash_table_digest: String,
    pub cert_fingerprint: String,
    pub candidates: Vec<String>,
}
impl StreamOffer {
    pub fn valid(&self) -> bool {
        let max_chunks = (MAX_HASH_TABLE_BYTES / 32) as u64;
        self.media.valid()
            && self.media.bytes > 0
            && valid_file_name(&self.file_name)
            && self.duration.is_finite()
            && self.duration >= 0.0
            && (4096..=4 * 1024 * 1024).contains(&self.chunk_size)
            && self.media.bytes.div_ceil(u64::from(self.chunk_size)) <= max_chunks
            && valid_hex_64(&self.hash_table_digest)
            && valid_hex_64(&self.cert_fingerprint)
            && !self.candidates.is_empty()
            && self.candidates.len() <= 16
            && self
                .candidates
                .iter()
                .all(|c| c.parse::<std::net::SocketAddr>().is_ok())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamAnswer {
    pub id: u64,
    pub cert_fingerprint: String,
    pub candidates: Vec<String>,
}
impl StreamAnswer {
    pub fn valid(&self) -> bool {
        valid_hex_64(&self.cert_fingerprint)
            && !self.candidates.is_empty()
            && self.candidates.len() <= 16
            && self
                .candidates
                .iter()
                .all(|c| c.parse::<std::net::SocketAddr>().is_ok())
    }
}

pub const MAX_SDP: usize = 12 * 1024;

pub fn valid_sdp(sdp: &str) -> bool {
    !sdp.is_empty()
        && sdp.len() <= MAX_SDP
        && sdp.starts_with("v=0")
        && sdp
            .lines()
            .any(|line| line.trim_end().starts_with("a=fingerprint:sha-256 "))
        && !sdp
            .chars()
            .any(|c| c.is_control() && c != '\r' && c != '\n')
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebStreamOffer {
    pub id: u64,
    pub sdp: String,
}
impl WebStreamOffer {
    pub fn valid(&self) -> bool {
        self.id != 0 && valid_sdp(&self.sdp)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebStreamAnswer {
    pub id: u64,
    pub sdp: String,
    pub media: Media,
    pub file_name: String,
    pub duration: f64,
    pub chunk_size: u32,
    pub hash_table_digest: String,
}
impl WebStreamAnswer {
    pub fn valid(&self) -> bool {
        let max_chunks = (MAX_HASH_TABLE_BYTES / 32) as u64;
        self.id != 0
            && valid_sdp(&self.sdp)
            && self.media.valid()
            && self.media.bytes > 0
            && valid_file_name(&self.file_name)
            && self.duration.is_finite()
            && self.duration >= 0.0
            && (4096..=4 * 1024 * 1024).contains(&self.chunk_size)
            && self.media.bytes.div_ceil(u64::from(self.chunk_size)) <= max_chunks
            && valid_hex_64(&self.hash_table_digest)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleChunk {
    pub id: u64,
    pub name: String,
    pub index: u32,
    pub total: u32,
    pub data: String,
}
impl SubtitleChunk {
    pub fn valid(&self) -> bool {
        self.id != 0
            && valid_file_name(&self.name)
            && self.total > 0
            && self.total <= MAX_SUBTITLE_CHUNKS
            && self.index < self.total
            && !self.data.is_empty()
            && self.data.len() <= MAX_SUBTITLE_CHUNK
            && !self.data.contains('\0')
    }
}

fn valid_hex_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub bytes: u64,
    pub total: u64,
}
impl Verification {
    pub fn valid(&self) -> bool {
        self.bytes <= self.total
    }
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.bytes as f32 / self.total as f32
        }
    }
}

pub fn valid_file_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 1024
        && !name.chars().any(|c| {
            c.is_control()
                || matches!(c, '/' | '\\' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playback {
    pub position: f64,
    pub host_ms: f64,
    pub speed: f64,
    pub paused: bool,
    pub seeking: bool,
}
impl Playback {
    pub fn valid(&self) -> bool {
        self.position.is_finite()
            && self.position >= 0.0
            && self.host_ms.is_finite()
            && self.speed.is_finite()
            && (0.5..=2.0).contains(&self.speed)
    }
    pub fn position_at(&self, host_ms: f64) -> f64 {
        self.position
            + if self.paused || self.seeking {
                0.0
            } else {
                ((host_ms - self.host_ms) / 1000.0).max(0.0) * self.speed
            }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamProgress {
    pub cached_bytes: u64,
    pub total_bytes: u64,
    pub transfer_bps: u64,
    pub link_bps: Option<u64>,
    pub buffered_ahead_bytes: u64,
    #[serde(default)]
    pub reconnecting: bool,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub cached_ranges: Vec<(u16, u16)>,
}
impl StreamProgress {
    pub fn valid(&self) -> bool {
        self.cached_bytes <= self.total_bytes
            && self.buffered_ahead_bytes <= self.total_bytes
            && self.cached_ranges.len() <= 64
            && self
                .cached_ranges
                .iter()
                .all(|&(start, end)| start < end && end <= 1000)
    }
    pub fn cache_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.cached_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Presence {
    pub media: Option<Media>,
    pub file_name: Option<String>,
    pub loading: bool,
    pub verification: Option<Verification>,
    pub ready: bool,
    pub blocked: bool,
    #[serde(default)]
    pub buffering: bool,
    pub ended: bool,
    pub duration: f64,
    pub clock_ready: bool,
    pub rtt_ms: f64,
    pub uncertainty_ms: f64,
    pub playback: Option<Playback>,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub stream_progress: Option<StreamProgress>,
    #[serde(default)]
    pub web_guest: bool,
    #[serde(default)]
    pub web_stream: bool,
}
impl Presence {
    pub fn valid(&self) -> bool {
        self.file_name.as_deref().is_none_or(valid_file_name)
            && self.verification.as_ref().is_none_or(Verification::valid)
            && self.duration.is_finite()
            && self.duration >= 0.0
            && self.rtt_ms.is_finite()
            && (0.0..=3000.0).contains(&self.rtt_ms)
            && self.uncertainty_ms.is_finite()
            && (0.0..=3000.0).contains(&self.uncertainty_ms)
            && self.playback.as_ref().is_none_or(Playback::valid)
            && self
                .stream_progress
                .as_ref()
                .is_none_or(StreamProgress::valid)
            && self
                .media
                .as_ref()
                .is_none_or(|m| m.hash.len() == 64 && m.hash.bytes().all(|b| b.is_ascii_hexdigit()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Host,
    Guest,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Control {
    Play,
    Pause,
    Seek(f64),
}
impl Control {
    pub fn valid(self) -> bool {
        !matches!(self, Self::Seek(p) if !p.is_finite() || p < 0.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    pub id: u64,
    pub position: f64,
    pub resume: bool,
    pub start_at_ms: Option<f64>,
}
impl Transition {
    pub fn valid(&self) -> bool {
        self.position.is_finite()
            && self.position >= 0.0
            && self.start_at_ms.is_none_or(|t| t.is_finite() && t >= 0.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    pub position: f64,
    pub paused: bool,
    #[serde(default)]
    pub waiting_buffer: bool,
    #[serde(default)]
    pub paused_by: Option<Side>,
    pub host_ms: f64,
    pub transition: Option<Transition>,
    pub host: Presence,
    pub guest: Presence,
}
impl Snapshot {
    pub fn valid(&self) -> bool {
        self.position.is_finite()
            && self.position >= 0.0
            && self.host_ms.is_finite()
            && self.host_ms >= 0.0
            && self.host.valid()
            && self.guest.valid()
            && self.transition.as_ref().is_none_or(Transition::valid)
    }
    pub fn playing_at(&self, now: f64) -> bool {
        self.transition.as_ref().map_or(!self.paused, |t| {
            t.resume && t.start_at_ms.is_some_and(|start| now >= start)
        })
    }
    pub fn position_at(&self, now: f64) -> f64 {
        if let Some(t) = &self.transition {
            t.position
                + if t.resume {
                    t.start_at_ms
                        .map_or(0.0, |start| ((now - start) / 1000.0).max(0.0))
                } else {
                    0.0
                }
        } else {
            self.position
                + if self.paused {
                    0.0
                } else {
                    ((now - self.host_ms) / 1000.0).max(0.0)
                }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Message {
    Discover {
        version: u32,
    },
    RoomInfo {
        version: u32,
        name: String,
        available: bool,
    },
    JoinRequest {
        version: u32,
        name: String,
    },
    Pending,
    Approved {
        version: u32,
        token: String,
    },
    Rejected,
    Busy,

    Hello {
        version: u32,
        key: String,
    },
    Welcome {
        version: u32,
    },
    Presence(Box<Presence>),
    Request(Control),
    State(Box<Snapshot>),
    Prepared {
        id: u64,
    },
    Armed {
        id: u64,
    },
    Failed {
        id: u64,
    },
    Ping {
        sent_ms: f64,
    },
    Pong {
        sent_ms: f64,
        received_ms: f64,
        sent_host_ms: f64,
    },
    StreamRequest,
    StreamOffer(Box<StreamOffer>),
    StreamAnswer(Box<StreamAnswer>),
    StreamStop {
        id: u64,
    },
    Bye,
    WebStreamOffer(Box<WebStreamOffer>),
    WebStreamAnswer(Box<WebStreamAnswer>),
    SubtitleChunk(Box<SubtitleChunk>),
    SubtitleClear,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDP: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\na=fingerprint:sha-256 AA:BB\r\n";

    #[test]
    fn web_stream_signaling_is_bounded_and_needs_a_fingerprint() {
        assert!(
            WebStreamOffer {
                id: 1,
                sdp: SDP.into()
            }
            .valid()
        );
        assert!(
            !WebStreamOffer {
                id: 0,
                sdp: SDP.into()
            }
            .valid()
        );
        assert!(
            !WebStreamOffer {
                id: 1,
                sdp: "v=0\r\n".into()
            }
            .valid()
        );
        assert!(
            !WebStreamOffer {
                id: 1,
                sdp: format!("{SDP}{}", "a".repeat(MAX_SDP))
            }
            .valid()
        );
        assert!(
            !WebStreamOffer {
                id: 1,
                sdp: format!("{SDP}\u{0}")
            }
            .valid()
        );
        let answer = WebStreamAnswer {
            id: 1,
            sdp: SDP.into(),
            media: Media {
                hash: "a".repeat(64),
                bytes: 10,
            },
            file_name: "Filme.mp4".into(),
            duration: 60.0,
            chunk_size: 256 * 1024,
            hash_table_digest: "b".repeat(64),
        };
        assert!(answer.valid());
        assert!(
            !WebStreamAnswer {
                file_name: "/home/x.mp4".into(),
                ..answer.clone()
            }
            .valid()
        );
        assert!(
            !WebStreamAnswer {
                chunk_size: 1,
                ..answer.clone()
            }
            .valid()
        );
        let json = serde_json::to_string(&Message::WebStreamOffer(Box::new(WebStreamOffer {
            id: 9,
            sdp: SDP.into(),
        })))
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<Message>(&json).unwrap(),
            Message::WebStreamOffer(offer) if offer.id == 9
        ));
    }

    #[test]
    fn presentation_metadata_is_bounded_and_never_accepts_paths() {
        for name in [
            "/home/person/movie.mkv",
            "C:\\Users\\film.mkv",
            "bad\nname",
            "\u{202e}film",
            "",
            " ",
        ] {
            assert!(
                !Presence {
                    file_name: Some(name.into()),
                    ..Default::default()
                }
                .valid()
            );
        }
        assert!(
            !Presence {
                file_name: Some("a".repeat(1025)),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !Presence {
                verification: Some(Verification { bytes: 2, total: 1 }),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            Presence {
                file_name: Some("Filme.1080p.mkv".into()),
                loading: true,
                verification: Some(Verification {
                    bytes: 42,
                    total: 100
                }),
                ..Default::default()
            }
            .valid()
        );
    }
}
