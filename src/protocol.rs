//! Protocolo de sala, sem caminhos locais ou conteúdo do vídeo.
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 4;
pub const PORT: u16 = 7842;
pub const MAX_MESSAGE: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub hash: String,
    pub bytes: u64,
}

/// Metadados de apresentação da sala, separados da identidade do conteúdo.
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

/// Posição medida no instante host_ms do relógio monotônico do anfitrião.
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Presence {
    pub media: Option<Media>,
    pub file_name: Option<String>,
    pub loading: bool,
    pub verification: Option<Verification>,
    pub ready: bool,
    pub blocked: bool,
    pub ended: bool,
    pub duration: f64,
    pub clock_ready: bool,
    pub rtt_ms: f64,
    pub uncertainty_ms: f64,
    pub playback: Option<Playback>,
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
                .media
                .as_ref()
                .is_none_or(|m| m.hash.len() == 64 && m.hash.bytes().all(|b| b.is_ascii_hexdigit()))
    }
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

/// Preparação é confirmada por operação. start_at_ms só existe após ambos prontos.
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
    Presence(Presence),
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
    Bye,
}

#[cfg(test)]
mod tests {
    use super::*;

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
