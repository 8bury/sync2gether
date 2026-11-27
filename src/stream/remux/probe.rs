use super::super::ffmpeg::{
    AV_CODEC_ID_AAC, AV_CODEC_ID_AV1, AV_CODEC_ID_H264, AV_CODEC_ID_HEVC, AV_CODEC_ID_MP3,
    AV_CODEC_ID_OPUS, AV_CODEC_ID_VORBIS, AV_CODEC_ID_VP8, AV_CODEC_ID_VP9, AV_DISPOSITION_DEFAULT,
    AV_NOPTS_VALUE, AV_OPT_SEARCH_CHILDREN, AV_PKT_FLAG_KEY, AV_TIME_BASE, AVChannelLayout,
    AVMEDIA_TYPE_AUDIO, AVMEDIA_TYPE_SUBTITLE, AVMEDIA_TYPE_VIDEO, AVRational, Ffmpeg, read_at,
    write_at,
};
use std::{
    ffi::{CStr, c_int, c_uint, c_void},
    path::Path,
};
use sync2gether_core::{
    datachannel::{MseSegmentInfo, encode_mse_index},
    protocol::{AudioTrackInfo, MAX_AUDIO_TRACKS, MAX_MSE_SEGMENTS, WebStreamMse},
};

const TARGET_SEGMENT_MS: u64 = 4_000;
const AVINDEX_KEYFRAME: u32 = 1;

#[derive(Clone, Debug)]
pub struct AudioStreamMeta {
    pub info: AudioTrackInfo,
    pub stream_index: c_int,
    pub codec_id: c_int,
    pub time_base: AVRational,
    pub copy_aac: bool,
}

#[derive(Clone, Debug)]
pub struct SegmentSpec {
    pub start_pts: i64,
    pub end_pts: Option<i64>,
    pub start_ms: u64,
    pub duration_ms: u32,
}

#[derive(Clone, Debug)]
pub struct MediaProbe {
    pub format_name: String,
    pub video_stream: c_int,
    pub video_codec_id: c_int,
    pub video_codec_string: String,
    pub video_time_base: AVRational,
    pub origin_pts: i64,
    pub duration_secs: f64,
    pub audio_streams: Vec<AudioStreamMeta>,
    pub default_audio: Option<u8>,
    pub subtitle_streams: Vec<c_int>,
    pub segments: Vec<SegmentSpec>,
    pub needs_remux: bool,
}

impl MediaProbe {
    pub fn mime(&self, track: Option<u8>) -> String {
        let has_audio = track.is_some_and(|idx| {
            self.audio_streams
                .iter()
                .any(|audio| audio.info.index == idx)
        });
        if has_audio {
            format!(
                "video/mp4; codecs=\"{}, mp4a.40.2\"",
                self.video_codec_string
            )
        } else {
            format!("video/mp4; codecs=\"{}\"", self.video_codec_string)
        }
    }

    pub fn web_stream_mse(&self) -> WebStreamMse {
        let audio_transcoded = self.default_audio.is_some_and(|idx| {
            self.audio_streams
                .iter()
                .find(|a| a.info.index == idx)
                .is_some_and(|a| !a.copy_aac)
        });
        WebStreamMse {
            mime: self.mime(self.default_audio),
            duration: self.duration_secs,
            segments: self.segments.len() as u32,
            audio_tracks: self.audio_streams.iter().map(|a| a.info.clone()).collect(),
            default_audio: self.default_audio,
            audio_transcoded,
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
}

pub(super) struct InputGuard<'a> {
    ff: &'a Ffmpeg,
    pub ctx: *mut c_void,
}

impl<'a> InputGuard<'a> {
    pub fn open(ff: &'a Ffmpeg, path: &Path) -> Result<Self, String> {
        let cpath = Ffmpeg::path_cstring(path)?;
        let mut ctx = std::ptr::null_mut();
        // SAFETY: avformat_open_input initializes ctx from a valid NUL-terminated path.
        if unsafe {
            (ff.avformat_open_input)(
                &mut ctx,
                cpath.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        } < 0
            || ctx.is_null()
        {
            return Err("Não foi possível abrir o arquivo com o FFmpeg.".into());
        }
        let guard = Self { ff, ctx };
        // SAFETY: ctx is an open AVFormatContext.
        if unsafe { (ff.avformat_find_stream_info)(ctx, std::ptr::null_mut()) } < 0 {
            return Err("Não foi possível ler as faixas do arquivo.".into());
        }
        Ok(guard)
    }

    pub fn nb_streams(&self) -> usize {
        // SAFETY: AVFormatContext.nb_streams is at offset 44 in lavf 60..=63.
        unsafe { read_at::<c_uint>(self.ctx, 44) as usize }
    }

    pub fn stream(&self, index: usize) -> *mut c_void {
        // SAFETY: AVFormatContext.streams is at offset 48 in lavf 60..=63.
        unsafe {
            let streams: *mut *mut c_void = read_at(self.ctx, 48);
            *streams.add(index)
        }
    }

    pub fn format_name(&self) -> String {
        // SAFETY: AVFormatContext.iformat is at offset 8; AVInputFormat.name is at offset 0.
        unsafe {
            let iformat: *const c_void = read_at(self.ctx, 8);
            if iformat.is_null() {
                return String::new();
            }
            let name: *const std::ffi::c_char = read_at(iformat, 0);
            if name.is_null() {
                String::new()
            } else {
                CStr::from_ptr(name).to_string_lossy().into_owned()
            }
        }
    }

    pub fn duration_secs(&self) -> f64 {
        // SAFETY: Reading AVFormatContext.duration at ABI-verified offset.
        let dur: i64 = unsafe { read_at(self.ctx, self.ff.abi.fmt_duration) };
        if dur > 0 && dur != AV_NOPTS_VALUE {
            dur as f64 / AV_TIME_BASE as f64
        } else {
            0.0
        }
    }
}

impl Drop for InputGuard<'_> {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // SAFETY: self.ctx was opened by avformat_open_input.
            unsafe { (self.ff.avformat_close_input)(&mut self.ctx) };
        }
    }
}

pub(super) fn stream_codecpar(stream: *const c_void) -> *mut c_void {
    // SAFETY: AVStream.codecpar is at offset 16 in lavf 60..=63.
    unsafe { read_at(stream, 16) }
}

pub(super) fn stream_time_base(stream: *const c_void) -> AVRational {
    // SAFETY: AVStream.time_base is at offset 32 in lavf 60..=63.
    unsafe { read_at(stream, 32) }
}

pub(super) fn set_stream_time_base(stream: *mut c_void, tb: AVRational) {
    // SAFETY: AVStream.time_base is at offset 32 in lavf 60..=63.
    unsafe { write_at(stream, 32, tb) };
}

pub(super) fn pts_to_ms(ff: &Ffmpeg, pts: i64, origin: i64, tb: AVRational) -> u64 {
    let rel = pts.saturating_sub(origin).max(0);
    // SAFETY: av_rescale_q is a pure arithmetic function on AVRational.
    let ms = unsafe { (ff.av_rescale_q)(rel, tb, AVRational { num: 1, den: 1000 }) };
    ms.max(0) as u64
}

fn codecpar_channels(ff: &Ffmpeg, par: *const c_void) -> u8 {
    // SAFETY: Allocating a scratch AVCodecContext to read ch_layout through public AVOptions.
    let mut ctx = unsafe { (ff.avcodec_alloc_context3)(std::ptr::null()) };
    if ctx.is_null() {
        return 0;
    }
    let mut layout = AVChannelLayout::default();
    // SAFETY: Copying codecpar into scratch context, reading ch_layout via AVOptions, and freeing.
    unsafe {
        let ok = (ff.avcodec_parameters_to_context)(ctx, par) >= 0
            && (ff.av_opt_get_chlayout)(
                ctx,
                c"ch_layout".as_ptr(),
                AV_OPT_SEARCH_CHILDREN,
                &mut layout,
            ) >= 0;
        let channels = if ok {
            layout.nb_channels.clamp(0, 255) as u8
        } else {
            0
        };
        (ff.av_channel_layout_uninit)(&mut layout);
        (ff.avcodec_free_context)(&mut ctx);
        channels
    }
}

fn h264_codec_string(extradata: &[u8]) -> String {
    if extradata.len() >= 4 && extradata[0] == 1 {
        return format!(
            "avc1.{:02x}{:02x}{:02x}",
            extradata[1], extradata[2], extradata[3]
        );
    }
    let mut i = 0;
    while i + 6 < extradata.len() {
        let start = if extradata[i] == 0 && extradata[i + 1] == 0 && extradata[i + 2] == 1 {
            Some(i + 3)
        } else if extradata[i] == 0
            && extradata[i + 1] == 0
            && extradata[i + 2] == 0
            && extradata[i + 3] == 1
        {
            Some(i + 4)
        } else {
            None
        };
        if let Some(pos) = start
            && pos + 3 < extradata.len()
            && (extradata[pos] & 0x1f) == 7
        {
            return format!(
                "avc1.{:02x}{:02x}{:02x}",
                extradata[pos + 1],
                extradata[pos + 2],
                extradata[pos + 3]
            );
        }
        i += 1;
    }
    "avc1.64001f".into()
}

fn video_codec_string(codec_id: c_int, codecpar: *const c_void) -> Option<String> {
    match codec_id {
        AV_CODEC_ID_H264 => {
            // SAFETY: AVCodecParameters.extradata (16) and extradata_size (24) in lavc 60..=63.
            let (ptr, size): (*const u8, c_int) =
                unsafe { (read_at(codecpar, 16), read_at(codecpar, 24)) };
            let extra = if !ptr.is_null() && size > 0 {
                // SAFETY: extradata is valid for extradata_size bytes.
                unsafe { std::slice::from_raw_parts(ptr, size as usize) }
            } else {
                &[]
            };
            Some(h264_codec_string(extra))
        }
        AV_CODEC_ID_HEVC => Some("hvc1.1.6.L93.B0".into()),
        AV_CODEC_ID_VP9 => Some("vp09.00.10.08".into()),
        AV_CODEC_ID_AV1 => Some("av01.0.04M.08".into()),
        _ => None,
    }
}

fn collect_keyframes(
    ff: &Ffmpeg,
    input: &InputGuard<'_>,
    video_idx: c_int,
    video_stream: *mut c_void,
) -> Vec<i64> {
    let mut keys = Vec::new();
    // SAFETY: avformat_index_get_entries_count reads the stream's demuxer index.
    let count = unsafe { (ff.avformat_index_get_entries_count)(video_stream) };
    if count > 0 {
        for i in 0..count {
            // SAFETY: i is within 0..count for this stream.
            let entry = unsafe { (ff.avformat_index_get_entry)(video_stream, i) };
            if entry.is_null() {
                continue;
            }
            // SAFETY: AVIndexEntry.timestamp (8) and flags bitfield word (16).
            let (ts, flags_word): (i64, u32) =
                unsafe { (read_at(entry.cast(), 8), read_at(entry.cast(), 16)) };
            if ts != AV_NOPTS_VALUE && (flags_word & AVINDEX_KEYFRAME) != 0 {
                keys.push(ts);
            }
        }
    }
    if keys.len() < 2 {
        keys.clear();
        // SAFETY: Allocating an AVPacket to scan keyframe PTS without decoding.
        let mut pkt = unsafe { (ff.av_packet_alloc)() };
        if !pkt.is_null() {
            loop {
                // SAFETY: Reading next demuxed packet header.
                if unsafe { (ff.av_read_frame)(input.ctx, pkt) } < 0 {
                    break;
                }
                // SAFETY: Reading stream_index (36), flags (40), pts (8), dts (16).
                let (sidx, flags, pts, dts): (c_int, c_int, i64, i64) = unsafe {
                    (
                        read_at(pkt, 36),
                        read_at(pkt, 40),
                        read_at(pkt, 8),
                        read_at(pkt, 16),
                    )
                };
                if sidx == video_idx && (flags & AV_PKT_FLAG_KEY) != 0 {
                    let ts = if pts != AV_NOPTS_VALUE { pts } else { dts };
                    if ts != AV_NOPTS_VALUE {
                        keys.push(ts);
                    }
                }
                // SAFETY: Unreferencing packet after inspection.
                unsafe { (ff.av_packet_unref)(pkt) };
            }
            // SAFETY: Freeing temporary scan packet.
            unsafe { (ff.av_packet_free)(&mut pkt) };
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn build_segments(
    ff: &Ffmpeg,
    keys: &[i64],
    origin: i64,
    tb: AVRational,
    duration_secs: f64,
) -> Vec<SegmentSpec> {
    let total_ms = (duration_secs * 1000.0).round().max(1.0) as u64;
    if keys.is_empty() {
        return vec![SegmentSpec {
            start_pts: origin,
            end_pts: None,
            start_ms: 0,
            duration_ms: total_ms.min(u64::from(u32::MAX)) as u32,
        }];
    }
    let mut chosen: Vec<(i64, u64)> = Vec::new();
    for &pts in keys {
        let ms = pts_to_ms(ff, pts, origin, tb);
        if chosen
            .last()
            .is_none_or(|&(_, prev_ms)| ms.saturating_sub(prev_ms) >= TARGET_SEGMENT_MS)
        {
            chosen.push((pts, ms));
            if chosen.len() >= MAX_MSE_SEGMENTS as usize {
                break;
            }
        }
    }
    let mut segments = Vec::with_capacity(chosen.len());
    for i in 0..chosen.len() {
        let (start_pts, start_ms) = chosen[i];
        let (end_pts, end_ms) = if let Some(&(next_pts, next_ms)) = chosen.get(i + 1) {
            (Some(next_pts), next_ms)
        } else {
            (None, total_ms.max(start_ms + 1))
        };
        let duration_ms = end_ms.saturating_sub(start_ms).max(1) as u32;
        segments.push(SegmentSpec {
            start_pts,
            end_pts,
            start_ms,
            duration_ms,
        });
    }
    segments
}

fn should_remux(
    path: &Path,
    format_name: &str,
    video_codec_id: c_int,
    audio_streams: &[AudioStreamMeta],
) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let is_mp4 = format_name.contains("mp4") || format_name.contains("mov");
    let is_webm = format_name.contains("webm") && ext == "webm";
    let native_container = is_mp4 || is_webm;
    if !native_container {
        return true;
    }
    if audio_streams.len() > 1 {
        return true;
    }
    if is_mp4 {
        let video_ok = matches!(
            video_codec_id,
            AV_CODEC_ID_H264 | AV_CODEC_ID_VP9 | AV_CODEC_ID_AV1
        );
        let audio_ok = audio_streams.iter().all(|a| {
            matches!(
                a.codec_id,
                AV_CODEC_ID_AAC | AV_CODEC_ID_OPUS | AV_CODEC_ID_MP3
            ) && a.info.channels <= 2
        });
        return !(video_ok && audio_ok);
    }
    let video_ok = matches!(
        video_codec_id,
        AV_CODEC_ID_VP8 | AV_CODEC_ID_VP9 | AV_CODEC_ID_AV1
    );
    let audio_ok = audio_streams
        .iter()
        .all(|a| matches!(a.codec_id, AV_CODEC_ID_OPUS | AV_CODEC_ID_VORBIS));
    !(video_ok && audio_ok)
}

pub fn probe_file_with(ff: &Ffmpeg, path: &Path) -> Result<MediaProbe, String> {
    let input = InputGuard::open(ff, path)?;
    let format_name = input.format_name();
    let mut video: Option<(c_int, c_int, String, AVRational, *mut c_void)> = None;
    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();

    for i in 0..input.nb_streams() {
        let stream = input.stream(i);
        let par = stream_codecpar(stream);
        // SAFETY: AVCodecParameters.codec_type (0) and codec_id (4) in lavc 60..=63.
        let (codec_type, codec_id): (c_int, c_int) = unsafe { (read_at(par, 0), read_at(par, 4)) };
        match codec_type {
            AVMEDIA_TYPE_VIDEO if video.is_none() => {
                if let Some(codec_str) = video_codec_string(codec_id, par) {
                    video = Some((
                        i as c_int,
                        codec_id,
                        codec_str,
                        stream_time_base(stream),
                        stream,
                    ));
                }
            }
            AVMEDIA_TYPE_AUDIO if audio_streams.len() < MAX_AUDIO_TRACKS => {
                let channels = codecpar_channels(ff, par);
                // SAFETY: AVStream.disposition (64) and metadata (80) in lavf 60..=63.
                let (disposition, metadata): (c_int, *const c_void) =
                    unsafe { (read_at(stream, 64), read_at(stream, 80)) };
                // SAFETY: metadata is a valid AVDictionary pointer from AVStream.
                let language = unsafe { ff.dict_string(metadata, c"language") }.map(|mut s| {
                    s.truncate(32);
                    s
                });
                // SAFETY: metadata is a valid AVDictionary pointer from AVStream.
                let title = unsafe { ff.dict_string(metadata, c"title") }.map(|mut s| {
                    s.truncate(128);
                    s
                });
                let mut codec = ff.codec_name(codec_id);
                codec.truncate(32);
                let idx = audio_streams.len() as u8;
                audio_streams.push(AudioStreamMeta {
                    info: AudioTrackInfo {
                        index: idx,
                        language,
                        title,
                        codec,
                        channels,
                        default: (disposition & AV_DISPOSITION_DEFAULT) != 0,
                    },
                    stream_index: i as c_int,
                    codec_id,
                    time_base: stream_time_base(stream),
                    copy_aac: codec_id == AV_CODEC_ID_AAC && channels <= 2,
                });
            }
            AVMEDIA_TYPE_SUBTITLE => subtitle_streams.push(i as c_int),
            _ => {}
        }
    }

    let Some((video_stream, video_codec_id, video_codec_string, video_time_base, v_ptr)) = video
    else {
        return Err("Nenhuma faixa de vídeo compatível com MSE encontrada.".into());
    };

    let default_audio = audio_streams
        .iter()
        .find(|a| a.info.default)
        .or_else(|| audio_streams.first())
        .map(|a| a.info.index);

    let needs_remux = should_remux(path, &format_name, video_codec_id, &audio_streams);
    let duration_secs = input.duration_secs();
    let keys = if needs_remux {
        collect_keyframes(ff, &input, video_stream, v_ptr)
    } else {
        Vec::new()
    };
    let origin_pts = keys.first().copied().unwrap_or(0);
    let segments = if needs_remux {
        build_segments(ff, &keys, origin_pts, video_time_base, duration_secs)
    } else {
        Vec::new()
    };

    Ok(MediaProbe {
        format_name,
        video_stream,
        video_codec_id,
        video_codec_string,
        video_time_base,
        origin_pts,
        duration_secs,
        audio_streams,
        default_audio,
        subtitle_streams,
        segments,
        needs_remux,
    })
}
