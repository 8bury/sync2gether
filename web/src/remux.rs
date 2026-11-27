mod parse;

use matroska_demuxer::{MatroskaFile, TrackType};
use parse::{
    AudioTrackMeta, VideoTrackMeta, parse_audio_track, parse_video_track, read_clusters_from,
    scan_ebml_layout,
};
use serde::Serialize;
use shiguredo_mp4::{
    Encode, TrackKind,
    boxes::{MfhdBox, MoofBox, TfdtBox, TfhdBox, TrafBox, TrunBox, TrunSample},
    mux::{Fmp4SegmentMuxer, Sample},
};
use std::{
    io::{Read, Seek, SeekFrom},
    num::NonZeroU32,
};
use sync2gether_core::{
    datachannel::{MseSegmentInfo, NO_AUDIO_TRACK, encode_mse_index},
    protocol::{MAX_AUDIO_TRACKS, MAX_MSE_SEGMENTS, WebStreamMse},
};

const TARGET_SEGMENT_MS: u64 = 4_000;
const VIDEO_TIMESCALE: u32 = 90_000;
const KEYFRAME_FLAGS: u32 = 0x0200_0000;
const NON_KEYFRAME_FLAGS: u32 = 0x0101_0000;
const AUDIO_SAMPLE_FLAGS: u32 = 0x0200_0000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemuxError {
    NotMatroska,
    UnsupportedVideo { codec: String },
    UnsupportedAudio { codecs: Vec<String> },
    NoVideo,
    NoKeyframes,
    InvalidFile { detail: String },
}

impl std::fmt::Display for RemuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotMatroska => write!(f, "Arquivo não é Matroska/MKV."),
            Self::UnsupportedVideo { codec } => {
                write!(f, "Codec de vídeo não suportado no navegador ({codec}).")
            }
            Self::UnsupportedAudio { codecs } => {
                write!(
                    f,
                    "Áudio incompatível com o navegador ({}).",
                    codecs.join(", ")
                )
            }
            Self::NoVideo => write!(f, "Nenhuma faixa de vídeo encontrada no arquivo."),
            Self::NoKeyframes => write!(f, "Não foi possível indexar os quadros-chave do arquivo."),
            Self::InvalidFile { detail } => write!(f, "{detail}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SegmentSpec {
    cluster_offset: u64,
    start_ticks: u64,
    end_ticks: Option<u64>,
    start_ms: u64,
    duration_ms: u32,
}

pub struct MkvRemuxer<R: Read + Seek> {
    reader: R,
    timestamp_scale: u64,
    duration_secs: f64,
    video: VideoTrackMeta,
    audio_tracks: Vec<AudioTrackMeta>,
    default_audio: Option<u8>,
    skipped_audio_codecs: Vec<String>,
    segments: Vec<SegmentSpec>,
}

impl<R: Read + Seek> MkvRemuxer<R> {
    pub fn open(mut reader: R) -> Result<Self, RemuxError> {
        let layout = scan_ebml_layout(&mut reader);
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|e| RemuxError::InvalidFile {
                detail: e.to_string(),
            })?;
        let mkv = MatroskaFile::open(&mut reader).map_err(|_| RemuxError::NotMatroska)?;
        let timestamp_scale = mkv.info().timestamp_scale().get();
        let mut duration_secs = mkv
            .info()
            .duration()
            .map(|d| d * (timestamp_scale as f64) / 1_000_000_000.0)
            .filter(|d| d.is_finite() && *d > 0.0)
            .unwrap_or(0.0);

        let mut video: Option<VideoTrackMeta> = None;
        let mut unsupported_video: Option<String> = None;
        let mut all_audio: Vec<AudioTrackMeta> = Vec::new();
        let mut skipped_audio: Vec<String> = Vec::new();
        let mut had_any_audio = false;

        for track in mkv.tracks() {
            match track.track_type() {
                TrackType::Video if video.is_none() => match parse_video_track(track) {
                    Ok(meta) => video = Some(meta),
                    Err(codec) => {
                        if unsupported_video.is_none() {
                            unsupported_video = Some(codec);
                        }
                    }
                },
                TrackType::Audio if all_audio.len() < MAX_AUDIO_TRACKS => {
                    had_any_audio = true;
                    match parse_audio_track(track, all_audio.len() as u8) {
                        Ok(meta) => all_audio.push(meta),
                        Err(label) => {
                            if !skipped_audio.contains(&label) {
                                skipped_audio.push(label);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        drop(mkv);

        let Some(video) = video else {
            return match unsupported_video {
                Some(codec) => Err(RemuxError::UnsupportedVideo { codec }),
                None => Err(RemuxError::NoVideo),
            };
        };

        if had_any_audio && all_audio.is_empty() {
            return Err(RemuxError::UnsupportedAudio {
                codecs: skipped_audio,
            });
        }

        let preferred_codec = all_audio
            .iter()
            .find(|a| a.info.default)
            .or_else(|| all_audio.first())
            .map(|a| a.info.codec.clone());

        let mut audio_tracks: Vec<AudioTrackMeta> = all_audio
            .into_iter()
            .filter(|a| preferred_codec.as_deref().is_none_or(|c| a.info.codec == c))
            .collect();
        for (idx, track) in audio_tracks.iter_mut().enumerate() {
            track.info.index = idx as u8;
        }
        let default_audio = audio_tracks
            .iter()
            .find(|a| a.info.default)
            .or_else(|| audio_tracks.first())
            .map(|a| a.info.index);

        let first_cluster_offset = layout.as_ref().map_or(0, |l| l.first_cluster_offset);
        let mut key_points: Vec<(u64, u64)> = layout
            .map(|l| {
                l.cues
                    .into_iter()
                    .filter(|c| c.track.is_none_or(|t| t == video.track_number))
                    .map(|c| (c.time_ticks, c.cluster_offset))
                    .collect()
            })
            .unwrap_or_default();

        if key_points.len() < 2 {
            key_points.clear();
            let mut max_ticks = 0u64;
            let _ = read_clusters_from(&mut reader, first_cluster_offset, |cluster_off, frame| {
                if frame.timestamp > max_ticks {
                    max_ticks = frame.timestamp;
                }
                if frame.track == video.track_number && frame.is_keyframe {
                    key_points.push((frame.timestamp, cluster_off));
                }
                true
            });
            if duration_secs <= 0.0 && max_ticks > 0 {
                duration_secs = ticks_to_ms(max_ticks, timestamp_scale) as f64 / 1000.0;
            }
        }
        key_points.sort_unstable_by_key(|&(ts, _)| ts);
        key_points.dedup_by_key(|(ts, _)| *ts);
        if key_points.is_empty() {
            return Err(RemuxError::NoKeyframes);
        }

        let segments = build_segments(&key_points, timestamp_scale, duration_secs);
        if segments.is_empty() {
            return Err(RemuxError::NoKeyframes);
        }
        if duration_secs <= 0.0
            && let Some(last) = segments.last()
        {
            duration_secs = (last.start_ms + u64::from(last.duration_ms)) as f64 / 1000.0;
        }

        Ok(Self {
            reader,
            timestamp_scale,
            duration_secs,
            video,
            audio_tracks,
            default_audio,
            skipped_audio_codecs: skipped_audio,
            segments,
        })
    }

    pub fn skipped_audio_codecs(&self) -> &[String] {
        &self.skipped_audio_codecs
    }

    pub fn web_stream_mse(&self) -> WebStreamMse {
        let mime = match self
            .default_audio
            .and_then(|idx| self.audio_tracks.iter().find(|a| a.info.index == idx))
        {
            Some(audio) => format!(
                "video/mp4; codecs=\"{}, {}\"",
                self.video.codec_string, audio.codec_string
            ),
            None => format!("video/mp4; codecs=\"{}\"", self.video.codec_string),
        };
        WebStreamMse {
            mime,
            duration: self.duration_secs,
            segments: self.segments.len() as u32,
            audio_tracks: self.audio_tracks.iter().map(|a| a.info.clone()).collect(),
            default_audio: self.default_audio,
            audio_transcoded: false,
        }
    }

    pub fn encoded_index(&self) -> Vec<u8> {
        let infos: Vec<MseSegmentInfo> = self
            .segments
            .iter()
            .map(|seg| MseSegmentInfo {
                start_ms: seg.start_ms,
                duration_ms: seg.duration_ms,
            })
            .collect();
        encode_mse_index(&infos)
    }

    pub fn init_segment(&self, track: u8) -> Result<Vec<u8>, String> {
        let audio = self.resolve_audio(track)?;
        let mut muxer = Fmp4SegmentMuxer::new().map_err(|e| e.to_string())?;
        let mut samples = vec![Sample {
            track_kind: TrackKind::Video,
            timescale: NonZeroU32::new(VIDEO_TIMESCALE).unwrap(),
            sample_entry: Some(self.video.sample_entry.clone()),
            duration: 3000,
            keyframe: true,
            composition_time_offset: None,
            data_offset: 0,
            data_size: 4,
        }];
        if let Some(audio) = audio {
            samples.push(Sample {
                track_kind: TrackKind::Audio,
                timescale: NonZeroU32::new(audio.timescale.max(1)).unwrap(),
                sample_entry: Some(audio.sample_entry.clone()),
                duration: audio.default_duration_samples.max(1),
                keyframe: true,
                composition_time_offset: None,
                data_offset: 4,
                data_size: 4,
            });
        }
        muxer
            .create_media_segment_metadata(&samples)
            .map_err(|e| e.to_string())?;
        muxer.init_segment_bytes().map_err(|e| e.to_string())
    }

    pub fn media_segment(&mut self, track: u8, segment_index: u32) -> Result<Vec<u8>, String> {
        let seg = *self
            .segments
            .get(segment_index as usize)
            .ok_or_else(|| "Segmento fora do intervalo.".to_owned())?;
        let audio_meta = self.resolve_audio(track)?.cloned();
        let origin_ticks = self.segments[0].start_ticks;

        let mut video_frames: Vec<(u64, bool, Vec<u8>)> = Vec::new();
        let mut audio_frames: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut started_video = false;
        let mut video_start_ticks = seg.start_ticks;
        let mut video_end_ticks = seg.end_ticks;
        let video_track = self.video.track_number;
        let audio_track = audio_meta.as_ref().map(|a| a.track_number);

        read_clusters_from(&mut self.reader, seg.cluster_offset, |_, frame| {
            if frame.track == video_track {
                if !started_video {
                    if frame.timestamp < seg.start_ticks || !frame.is_keyframe {
                        return true;
                    }
                    started_video = true;
                    video_start_ticks = frame.timestamp;
                } else if let Some(end) = seg.end_ticks
                    && frame.timestamp >= end
                    && frame.is_keyframe
                {
                    video_end_ticks = Some(frame.timestamp);
                    return false;
                }
                video_frames.push((frame.timestamp, frame.is_keyframe, frame.data));
            } else if audio_track == Some(frame.track) {
                if frame.timestamp < seg.start_ticks {
                    return true;
                }
                if let Some(end) = video_end_ticks
                    && frame.timestamp >= end
                {
                    return true;
                }
                audio_frames.push((frame.timestamp, frame.data));
            }
            true
        })?;

        if video_frames.is_empty() {
            return Err("Nenhum quadro de vídeo encontrado no segmento.".into());
        }

        audio_frames.retain(|(ts, _)| {
            *ts >= video_start_ticks && video_end_ticks.is_none_or(|end| *ts < end)
        });

        let video_pts: Vec<u64> = video_frames
            .iter()
            .map(|(ts, _, _)| {
                ticks_to_timescale(
                    ts.saturating_sub(origin_ticks),
                    self.timestamp_scale,
                    VIDEO_TIMESCALE,
                )
            })
            .collect();
        let (video_dts, video_cts) = compute_dts_and_cts(&video_pts);
        let video_base_dts = video_dts[0];
        let default_v_dur = ticks_to_timescale(
            self.video.default_duration_ticks.max(1),
            self.timestamp_scale,
            VIDEO_TIMESCALE,
        )
        .max(1) as u32;
        let video_end_dts = video_end_ticks.map(|end| {
            ticks_to_timescale(
                end.saturating_sub(origin_ticks),
                self.timestamp_scale,
                VIDEO_TIMESCALE,
            )
        });
        let has_cts = video_cts.iter().any(|&c| c != 0);

        let mut video_trun_samples = Vec::with_capacity(video_frames.len());
        let mut video_payload = Vec::new();
        for (i, (_, is_key, data)) in video_frames.iter().enumerate() {
            let dur = if let Some(&next) = video_dts.get(i + 1) {
                next.saturating_sub(video_dts[i]).max(1) as u32
            } else if let Some(end_dts) = video_end_dts {
                end_dts
                    .saturating_sub(video_dts[i])
                    .max(1)
                    .min(u64::from(default_v_dur) * 4) as u32
            } else {
                default_v_dur
            };
            let flags = if *is_key {
                shiguredo_mp4::SampleFlags::new(KEYFRAME_FLAGS)
            } else {
                shiguredo_mp4::SampleFlags::new(NON_KEYFRAME_FLAGS)
            };
            video_trun_samples.push(TrunSample {
                duration: Some(dur),
                size: Some(data.len() as u32),
                flags: Some(flags),
                composition_time_offset: has_cts.then_some(video_cts[i]),
            });
            video_payload.extend_from_slice(data);
        }

        let mut traf_boxes = vec![make_traf(1, video_base_dts, video_trun_samples)];
        let mut audio_payload = Vec::new();
        if let Some(ref audio) = audio_meta
            && !audio_frames.is_empty()
        {
            let audio_pts: Vec<u64> = audio_frames
                .iter()
                .map(|(ts, _)| {
                    ticks_to_timescale(
                        ts.saturating_sub(origin_ticks),
                        self.timestamp_scale,
                        audio.timescale,
                    )
                })
                .collect();
            let audio_base_dts = audio_pts[0];
            let mut audio_trun_samples = Vec::with_capacity(audio_frames.len());
            for (i, (_, data)) in audio_frames.iter().enumerate() {
                let dur = if let Some(&next) = audio_pts.get(i + 1) {
                    let delta = next.saturating_sub(audio_pts[i]);
                    if delta > 0 {
                        delta as u32
                    } else {
                        audio.default_duration_samples
                    }
                } else {
                    audio.default_duration_samples
                };
                audio_trun_samples.push(TrunSample {
                    duration: Some(dur.max(1)),
                    size: Some(data.len() as u32),
                    flags: Some(shiguredo_mp4::SampleFlags::new(AUDIO_SAMPLE_FLAGS)),
                    composition_time_offset: None,
                });
                audio_payload.extend_from_slice(data);
            }
            traf_boxes.push(make_traf(2, audio_base_dts, audio_trun_samples));
        }

        encode_moof_mdat(
            segment_index + 1,
            traf_boxes,
            &video_payload,
            &audio_payload,
        )
    }

    fn resolve_audio(&self, track: u8) -> Result<Option<&AudioTrackMeta>, String> {
        if track == NO_AUDIO_TRACK {
            return Ok(None);
        }
        self.audio_tracks
            .iter()
            .find(|a| a.info.index == track)
            .map(Some)
            .ok_or_else(|| "Faixa de áudio inexistente.".into())
    }
}

fn make_traf(track_id: u32, base_media_decode_time: u64, samples: Vec<TrunSample>) -> TrafBox {
    TrafBox {
        tfhd_box: TfhdBox {
            track_id,
            base_data_offset: None,
            sample_description_index: Some(1),
            default_sample_duration: None,
            default_sample_size: None,
            default_sample_flags: None,
            duration_is_empty: false,
            default_base_is_moof: true,
        },
        tfdt_box: Some(TfdtBox {
            version: 1,
            base_media_decode_time,
        }),
        trun_boxes: vec![TrunBox {
            data_offset: Some(0),
            first_sample_flags: None,
            samples,
        }],
        unknown_boxes: vec![],
    }
}

fn encode_moof_mdat(
    sequence_number: u32,
    traf_boxes: Vec<TrafBox>,
    video_payload: &[u8],
    audio_payload: &[u8],
) -> Result<Vec<u8>, String> {
    let mut moof = MoofBox {
        mfhd_box: MfhdBox { sequence_number },
        traf_boxes,
        unknown_boxes: vec![],
    };
    let moof_size = moof.encode_to_vec().map_err(|e| e.to_string())?.len();
    moof.traf_boxes[0].trun_boxes[0].data_offset = Some((moof_size + 8) as i32);
    if moof.traf_boxes.len() > 1 {
        moof.traf_boxes[1].trun_boxes[0].data_offset =
            Some((moof_size + 8 + video_payload.len()) as i32);
    }
    let moof_bytes = moof.encode_to_vec().map_err(|e| e.to_string())?;
    let mdat_payload_len = video_payload.len() + audio_payload.len();
    let mdat_size = (8 + mdat_payload_len) as u32;
    let mut out = Vec::with_capacity(moof_bytes.len() + 8 + mdat_payload_len);
    out.extend_from_slice(&moof_bytes);
    out.extend_from_slice(&mdat_size.to_be_bytes());
    out.extend_from_slice(b"mdat");
    out.extend_from_slice(video_payload);
    out.extend_from_slice(audio_payload);
    Ok(out)
}

fn ticks_to_ms(ticks: u64, timestamp_scale: u64) -> u64 {
    ((u128::from(ticks) * u128::from(timestamp_scale)) / 1_000_000) as u64
}

fn ticks_to_timescale(ticks: u64, timestamp_scale: u64, timescale: u32) -> u64 {
    ((u128::from(ticks) * u128::from(timestamp_scale) * u128::from(timescale)) / 1_000_000_000)
        as u64
}

fn compute_dts_and_cts(pts: &[u64]) -> (Vec<u64>, Vec<i64>) {
    let monotonic = pts.windows(2).all(|w| w[1] >= w[0]);
    if monotonic {
        return (pts.to_vec(), vec![0; pts.len()]);
    }
    let mut sorted = pts.to_vec();
    sorted.sort_unstable();
    let cts: Vec<i64> = pts
        .iter()
        .zip(sorted.iter())
        .map(|(&p, &d)| p as i64 - d as i64)
        .collect();
    (sorted, cts)
}

fn build_segments(
    keys: &[(u64, u64)],
    timestamp_scale: u64,
    duration_secs: f64,
) -> Vec<SegmentSpec> {
    let origin = keys[0].0;
    let total_ms = (duration_secs * 1000.0).round().max(1.0) as u64;
    let mut chosen: Vec<(u64, u64, u64)> = Vec::new();
    for &(ticks, cluster_offset) in keys {
        let ms = ticks_to_ms(ticks.saturating_sub(origin), timestamp_scale);
        if chosen
            .last()
            .is_none_or(|&(_, _, prev_ms)| ms.saturating_sub(prev_ms) >= TARGET_SEGMENT_MS)
        {
            chosen.push((ticks, cluster_offset, ms));
            if chosen.len() >= MAX_MSE_SEGMENTS as usize {
                break;
            }
        }
    }
    let mut segments = Vec::with_capacity(chosen.len());
    for i in 0..chosen.len() {
        let (start_ticks, cluster_offset, start_ms) = chosen[i];
        let (end_ticks, end_ms) = if let Some(&(next_ticks, _, next_ms)) = chosen.get(i + 1) {
            (Some(next_ticks), next_ms)
        } else {
            (None, total_ms.max(start_ms + 1))
        };
        let duration_ms = end_ms
            .saturating_sub(start_ms)
            .max(1)
            .min(u64::from(u32::MAX)) as u32;
        segments.push(SegmentSpec {
            cluster_offset,
            start_ticks,
            end_ticks,
            start_ms,
            duration_ms,
        });
    }
    segments
}
