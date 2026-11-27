use super::super::ffmpeg::{AV_NOPTS_VALUE, AVRational, Ffmpeg, read_at};
use super::probe::{InputGuard, MediaProbe, pts_to_ms};
use std::{
    ffi::c_int,
    path::{Path, PathBuf},
};
use sync2gether_core::{
    protocol::{SubtitleTrackInfo, valid_file_name},
    subtitle::{
        SubtitleCue, SubtitleError, build_webvtt, decode_bytes, sanitize_ass_packet,
        sanitize_srt_text,
    },
};

const FALLBACK_CUE_MS: i64 = 4_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSubtitleCodec {
    SubRip,
    Ass,
    WebVtt,
    Text,
    MovText,
}

impl TextSubtitleCodec {
    pub fn from_codec_name(name: &str) -> Option<Self> {
        match name {
            "subrip" | "srt" => Some(Self::SubRip),
            "ass" | "ssa" => Some(Self::Ass),
            "webvtt" => Some(Self::WebVtt),
            "text" => Some(Self::Text),
            "mov_text" => Some(Self::MovText),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubtitleStreamMeta {
    pub info: SubtitleTrackInfo,
    pub stream_index: c_int,
    pub codec: TextSubtitleCodec,
    pub time_base: AVRational,
}

#[derive(Clone, Debug)]
pub struct EmbeddedSubtitles {
    path: PathBuf,
    origin_pts: i64,
    video_time_base: AVRational,
    tracks: Vec<SubtitleStreamMeta>,
}

impl EmbeddedSubtitles {
    pub fn empty() -> Self {
        Self {
            path: PathBuf::new(),
            origin_pts: 0,
            video_time_base: AVRational { num: 1, den: 1000 },
            tracks: Vec::new(),
        }
    }

    pub fn from_probe(path: PathBuf, probe: &MediaProbe, use_origin: bool) -> Self {
        Self {
            path,
            origin_pts: if use_origin { probe.origin_pts } else { 0 },
            video_time_base: probe.video_time_base,
            tracks: probe.subtitle_streams.clone(),
        }
    }

    pub fn tracks(&self) -> Vec<SubtitleTrackInfo> {
        self.tracks.iter().map(|t| t.info.clone()).collect()
    }

    pub fn extract(&self, track_index: u8) -> Result<(String, String), String> {
        if let Some(dir) = std::env::var_os("SYNC2GETHER_FFMPEG_DIR").filter(|v| !v.is_empty()) {
            let ff = Ffmpeg::load_from_dir(Some(Path::new(&dir)))?;
            return self.extract_with(&ff, track_index);
        }
        let ff = super::super::ffmpeg::global()?;
        self.extract_with(ff, track_index)
    }

    pub fn extract_with(&self, ff: &Ffmpeg, track_index: u8) -> Result<(String, String), String> {
        let Some(meta) = self.tracks.iter().find(|t| t.info.index == track_index) else {
            return Err("Faixa de legenda inexistente.".into());
        };
        let vtt = extract_track_with(ff, &self.path, meta, self.origin_pts, self.video_time_base)?;
        let name = track_display_name(&meta.info);
        Ok((name, vtt))
    }
}

fn track_display_name(info: &SubtitleTrackInfo) -> String {
    let raw = match (&info.title, &info.language) {
        (Some(title), Some(lang)) => format!("{title} ({lang})"),
        (Some(title), None) => title.clone(),
        (None, Some(lang)) => lang.clone(),
        (None, None) => format!("Faixa {}", info.index + 1),
    };
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '/' | '\\' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
                )
            {
                ' '
            } else {
                c
            }
        })
        .collect();
    let trimmed = clean.trim().to_owned();
    if valid_file_name(&trimmed) {
        trimmed
    } else {
        format!("Faixa {}", info.index + 1)
    }
}

fn decode_packet_text(codec: TextSubtitleCodec, raw: &[u8]) -> String {
    let payload = match codec {
        TextSubtitleCodec::MovText => {
            if raw.len() < 2 {
                return String::new();
            }
            let len = u16::from_be_bytes([raw[0], raw[1]]) as usize;
            &raw[2..(2 + len).min(raw.len())]
        }
        _ => raw,
    };
    let Ok(decoded) = decode_bytes(payload) else {
        return String::new();
    };
    match codec {
        TextSubtitleCodec::Ass => sanitize_ass_packet(&decoded),
        TextSubtitleCodec::SubRip
        | TextSubtitleCodec::WebVtt
        | TextSubtitleCodec::Text
        | TextSubtitleCodec::MovText => sanitize_srt_text(&decoded),
    }
}

pub fn extract_track_with(
    ff: &Ffmpeg,
    path: &Path,
    meta: &SubtitleStreamMeta,
    origin_pts: i64,
    video_time_base: AVRational,
) -> Result<String, String> {
    let input = InputGuard::open(ff, path)?;
    let origin_sub_pts = if origin_pts != 0 {
        // SAFETY: Rescaling video origin PTS into the subtitle stream's time_base.
        unsafe { (ff.av_rescale_q)(origin_pts, video_time_base, meta.time_base) }
    } else {
        0
    };
    // SAFETY: Allocating packet for scanning subtitle frames.
    let mut pkt = unsafe { (ff.av_packet_alloc)() };
    if pkt.is_null() {
        return Err("Falha ao alocar pacote de legenda.".into());
    }
    let mut cues: Vec<SubtitleCue> = Vec::new();
    let mut open_cue: Option<(i64, String)> = None;

    loop {
        // SAFETY: Reading next demuxed packet from input.
        if unsafe { (ff.av_read_frame)(input.ctx, pkt) } < 0 {
            break;
        }
        // SAFETY: Reading AVPacket fields at verified offsets in lavc 60..=63.
        let (sidx, pts, dts, data, size, duration): (c_int, i64, i64, *const u8, c_int, i64) = unsafe {
            (
                read_at(pkt, 36),
                read_at(pkt, 8),
                read_at(pkt, 16),
                read_at(pkt, 24),
                read_at(pkt, 32),
                read_at(pkt, 64),
            )
        };
        if sidx == meta.stream_index {
            let ts = if pts != AV_NOPTS_VALUE { pts } else { dts };
            if ts != AV_NOPTS_VALUE {
                let start_ms = pts_to_ms(ff, ts, origin_sub_pts, meta.time_base) as i64;
                if let Some((prev_start, prev_text)) = open_cue.take() {
                    let end_ms = if start_ms > prev_start {
                        start_ms.min(prev_start + FALLBACK_CUE_MS)
                    } else {
                        prev_start + FALLBACK_CUE_MS
                    };
                    cues.push(SubtitleCue {
                        start_ms: prev_start,
                        end_ms,
                        text: prev_text,
                    });
                }
                if !data.is_null() && size > 0 {
                    // SAFETY: FFmpeg guarantees pkt.data is valid for pkt.size bytes.
                    let raw = unsafe { std::slice::from_raw_parts(data, size as usize) };
                    let text = decode_packet_text(meta.codec, raw);
                    if !text.is_empty() {
                        let dur_ms = if duration > 0 {
                            // SAFETY: Rescaling packet duration to milliseconds.
                            unsafe {
                                (ff.av_rescale_q)(
                                    duration,
                                    meta.time_base,
                                    AVRational { num: 1, den: 1000 },
                                )
                            }
                            .max(1)
                        } else {
                            0
                        };
                        if dur_ms > 0 {
                            cues.push(SubtitleCue {
                                start_ms,
                                end_ms: start_ms.saturating_add(dur_ms),
                                text,
                            });
                        } else {
                            open_cue = Some((start_ms, text));
                        }
                    }
                }
            }
        }
        // SAFETY: Unreferencing packet before next iteration.
        unsafe { (ff.av_packet_unref)(pkt) };
    }
    // SAFETY: Freeing scan packet.
    unsafe { (ff.av_packet_free)(&mut pkt) };

    if let Some((prev_start, prev_text)) = open_cue {
        cues.push(SubtitleCue {
            start_ms: prev_start,
            end_ms: prev_start.saturating_add(FALLBACK_CUE_MS),
            text: prev_text,
        });
    }

    build_webvtt(&cues).map_err(|err| match err {
        SubtitleError::TooLarge => "A legenda embutida excede o limite de 1 MiB.".into(),
        SubtitleError::Empty | SubtitleError::InvalidFormat | SubtitleError::InvalidName => {
            "A faixa de legenda embutida não contém falas válidas.".into()
        }
    })
}
