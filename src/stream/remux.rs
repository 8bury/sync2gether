mod probe;
mod transcode;

pub use probe::{AudioStreamMeta, MediaProbe, SegmentSpec, probe_file_with};

use super::ffmpeg::{
    AV_NOPTS_VALUE, AV_PKT_FLAG_KEY, AVRational, AVSEEK_FLAG_BACKWARD, Ffmpeg, FfmpegVersions,
    read_at, write_at,
};
use probe::{InputGuard, pts_to_ms, set_stream_time_base, stream_codecpar, stream_time_base};
use std::{
    collections::{HashMap, VecDeque},
    ffi::{c_int, c_void},
    path::{Path, PathBuf},
    sync::Mutex,
};
use sync2gether_core::datachannel::NO_AUDIO_TRACK;
use transcode::{AAC_SAMPLE_RATE, AudioTranscoder, EncoderGuard, verify_audio_stream_abi};

const AVIO_BUF_SIZE: c_int = 32 * 1024;
const MAX_CACHED_SEGMENTS: usize = 8;

struct OutputGuard<'a> {
    ff: &'a Ffmpeg,
    ctx: *mut c_void,
    pb: *mut c_void,
    buf: *mut Vec<u8>,
}

unsafe extern "C" fn write_vec(opaque: *mut c_void, data: *const u8, size: c_int) -> c_int {
    if opaque.is_null() || data.is_null() || size <= 0 {
        return 0;
    }
    // SAFETY: opaque points to the Box<Vec<u8>> owned by OutputGuard for the lifetime of pb.
    let target = unsafe { &mut *opaque.cast::<Vec<u8>>() };
    // SAFETY: FFmpeg guarantees data is valid for `size` bytes during this callback.
    let slice = unsafe { std::slice::from_raw_parts(data, size as usize) };
    target.extend_from_slice(slice);
    size
}

impl<'a> OutputGuard<'a> {
    fn new_mp4(ff: &'a Ffmpeg) -> Result<Self, String> {
        let mut ctx = std::ptr::null_mut();
        // SAFETY: Allocating an mp4 output AVFormatContext.
        if unsafe {
            (ff.avformat_alloc_output_context2)(
                &mut ctx,
                std::ptr::null(),
                c"mp4".as_ptr(),
                std::ptr::null(),
            )
        } < 0
            || ctx.is_null()
        {
            return Err("Falha ao criar muxer MP4.".into());
        }
        let buf = Box::into_raw(Box::new(Vec::<u8>::with_capacity(64 * 1024)));
        // SAFETY: Allocating AVIO buffer with av_malloc as required by avio_alloc_context.
        let io_buf = unsafe { (ff.av_malloc)(AVIO_BUF_SIZE as usize).cast::<u8>() };
        if io_buf.is_null() {
            // SAFETY: Reclaiming buf and freeing ctx on allocation failure.
            unsafe {
                drop(Box::from_raw(buf));
                (ff.avformat_free_context)(ctx);
            }
            return Err("Falha ao alocar buffer de saída.".into());
        }
        // SAFETY: Creating a writable memory AVIOContext backed by write_vec.
        let pb = unsafe {
            (ff.avio_alloc_context)(
                io_buf,
                AVIO_BUF_SIZE,
                1,
                buf.cast(),
                None,
                Some(write_vec),
                None,
            )
        };
        if pb.is_null() {
            let mut ptr = io_buf.cast::<c_void>();
            // SAFETY: Cleaning up on avio_alloc_context failure.
            unsafe {
                (ff.av_freep)((&mut ptr as *mut *mut c_void).cast());
                drop(Box::from_raw(buf));
                (ff.avformat_free_context)(ctx);
            }
            return Err("Falha ao inicializar saída em memória.".into());
        }
        // SAFETY: AVFormatContext.pb is at offset 32 in lavf 60..=63.
        unsafe { write_at(ctx, 32, pb) };
        Ok(Self { ff, ctx, pb, buf })
    }

    fn flush(&mut self) {
        // SAFETY: self.pb is a valid writable AVIOContext.
        unsafe { (self.ff.avio_flush)(self.pb) };
    }

    fn clear_bytes(&mut self) {
        self.flush();
        // SAFETY: self.buf is exclusively owned while FFmpeg is not executing write_vec.
        unsafe { (*self.buf).clear() };
    }

    fn take_bytes(&mut self) -> Vec<u8> {
        self.flush();
        // SAFETY: self.buf is exclusively owned while FFmpeg is not executing write_vec.
        unsafe { std::mem::take(&mut *self.buf) }
    }
}

impl Drop for OutputGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: Releasing AVFormatContext, AVIOContext internal buffer, AVIOContext, and Vec.
        unsafe {
            if !self.ctx.is_null() {
                write_at::<*mut c_void>(self.ctx, 32, std::ptr::null_mut());
                (self.ff.avformat_free_context)(self.ctx);
            }
            if !self.pb.is_null() {
                let buf_field = self.pb.cast::<u8>().add(8).cast::<c_void>();
                (self.ff.av_freep)(buf_field);
                (self.ff.avio_context_free)(&mut self.pb);
            }
            if !self.buf.is_null() {
                drop(Box::from_raw(self.buf));
            }
        }
    }
}

pub struct RemuxSession {
    ff: Option<Ffmpeg>,
    path: PathBuf,
    probe: MediaProbe,
    cache: Mutex<SegmentCache>,
}

#[derive(Default)]
struct SegmentCache {
    inits: HashMap<u8, Vec<u8>>,
    segments: HashMap<(u8, u32), Vec<u8>>,
    order: VecDeque<(u8, u32)>,
}

impl RemuxSession {
    pub fn open(path: &Path) -> Result<Option<Self>, String> {
        if let Some(dir) = std::env::var_os("SYNC2GETHER_FFMPEG_DIR").filter(|v| !v.is_empty()) {
            let loaded = Ffmpeg::load_from_dir(Some(Path::new(&dir)))?;
            return Self::open_with_owned(loaded, path);
        }
        let ff_ref = super::ffmpeg::global()?;
        let probe = probe_file_with(ff_ref, path)?;
        if !probe.needs_remux || probe.segments.is_empty() {
            return Ok(None);
        }
        verify_transcode_abi(ff_ref, path, &probe)?;
        Ok(Some(Self {
            ff: None,
            path: path.to_path_buf(),
            probe,
            cache: Mutex::new(SegmentCache::default()),
        }))
    }

    pub fn open_with_owned(ff: Ffmpeg, path: &Path) -> Result<Option<Self>, String> {
        let probe = probe_file_with(&ff, path)?;
        if !probe.needs_remux || probe.segments.is_empty() {
            return Ok(None);
        }
        verify_transcode_abi(&ff, path, &probe)?;
        Ok(Some(Self {
            ff: Some(ff),
            path: path.to_path_buf(),
            probe,
            cache: Mutex::new(SegmentCache::default()),
        }))
    }

    fn ff(&self) -> &Ffmpeg {
        self.ff
            .as_ref()
            .unwrap_or_else(|| super::ffmpeg::global().expect("FFmpeg carregado"))
    }

    pub fn ffmpeg_versions(&self) -> FfmpegVersions {
        self.ff().versions
    }

    pub fn probe(&self) -> &MediaProbe {
        &self.probe
    }

    fn resolve_audio(&self, track: u8) -> Result<Option<&AudioStreamMeta>, String> {
        if track == NO_AUDIO_TRACK {
            return Ok(None);
        }
        self.probe
            .audio_streams
            .iter()
            .find(|a| a.info.index == track)
            .map(Some)
            .ok_or_else(|| "Faixa de áudio inexistente.".into())
    }

    fn setup_muxer<'a>(
        &self,
        ff: &'a Ffmpeg,
        input: &InputGuard<'a>,
        audio: Option<&AudioStreamMeta>,
        enc_ctx: Option<*mut c_void>,
    ) -> Result<(OutputGuard<'a>, AVRational, Option<AVRational>), String> {
        let out = OutputGuard::new_mp4(ff)?;
        // SAFETY: Creating video output stream and copying codecpar from input video stream.
        let out_v = unsafe { (ff.avformat_new_stream)(out.ctx, std::ptr::null()) };
        if out_v.is_null() {
            return Err("Falha ao criar faixa de vídeo no MP4.".into());
        }
        let in_v = input.stream(self.probe.video_stream as usize);
        // SAFETY: Copying video codecpar and clearing codec_tag for MP4 muxer.
        unsafe {
            if (ff.avcodec_parameters_copy)(stream_codecpar(out_v), stream_codecpar(in_v)) < 0 {
                return Err("Falha ao copiar parâmetros de vídeo.".into());
            }
            write_at::<u32>(stream_codecpar(out_v), 8, 0);
        }
        set_stream_time_base(out_v, self.probe.video_time_base);

        let mut out_a = std::ptr::null_mut();
        if let Some(meta) = audio {
            // SAFETY: Creating audio output stream.
            out_a = unsafe { (ff.avformat_new_stream)(out.ctx, std::ptr::null()) };
            if out_a.is_null() {
                return Err("Falha ao criar faixa de áudio no MP4.".into());
            }
            if meta.copy_aac {
                let in_a = input.stream(meta.stream_index as usize);
                // SAFETY: Copying compatible AAC codecpar directly.
                unsafe {
                    if (ff.avcodec_parameters_copy)(stream_codecpar(out_a), stream_codecpar(in_a))
                        < 0
                    {
                        return Err("Falha ao copiar parâmetros de áudio.".into());
                    }
                    write_at::<u32>(stream_codecpar(out_a), 8, 0);
                }
                set_stream_time_base(out_a, meta.time_base);
            } else {
                let enc_guard;
                let ctx = match enc_ctx {
                    Some(existing) => existing,
                    None => {
                        enc_guard = EncoderGuard::open_aac(ff)?;
                        enc_guard.ctx
                    }
                };
                // SAFETY: Populating output audio codecpar from the opened AAC encoder.
                unsafe {
                    if (ff.avcodec_parameters_from_context)(stream_codecpar(out_a), ctx) < 0 {
                        return Err("Falha ao configurar parâmetros AAC.".into());
                    }
                    write_at::<u32>(stream_codecpar(out_a), 8, 0);
                }
                set_stream_time_base(
                    out_a,
                    AVRational {
                        num: 1,
                        den: AAC_SAMPLE_RATE,
                    },
                );
            }
        }

        let mut opts = std::ptr::null_mut();
        // SAFETY: Setting fragmented MP4 movflags and writing initialization header.
        let wrote = unsafe {
            (ff.av_dict_set)(
                &mut opts,
                c"movflags".as_ptr(),
                c"frag_custom+empty_moov+default_base_moof+omit_tfhd_offset".as_ptr(),
                0,
            );
            let r = (ff.avformat_write_header)(out.ctx, &mut opts);
            (ff.av_dict_free)(&mut opts);
            r
        };
        if wrote < 0 {
            return Err("Falha ao escrever cabeçalho do MP4 fragmentado.".into());
        }
        let v_tb = stream_time_base(out_v);
        let a_tb = (!out_a.is_null()).then(|| stream_time_base(out_a));
        Ok((out, v_tb, a_tb))
    }

    pub fn init_segment(&self, track: u8) -> Result<Vec<u8>, String> {
        if let Some(cached) = self.cache.lock().unwrap().inits.get(&track).cloned() {
            return Ok(cached);
        }
        let audio = self.resolve_audio(track)?;
        let ff = self.ff();
        let input = InputGuard::open(ff, &self.path)?;
        let (mut out, _, _) = self.setup_muxer(ff, &input, audio, None)?;
        let bytes = out.take_bytes();
        self.cache
            .lock()
            .unwrap()
            .inits
            .insert(track, bytes.clone());
        Ok(bytes)
    }

    pub fn media_segment(&self, track: u8, index: u32) -> Result<Vec<u8>, String> {
        let Some(seg) = self.probe.segments.get(index as usize).cloned() else {
            return Err("Segmento fora do filme.".into());
        };
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap()
            .segments
            .get(&(track, index))
            .cloned()
        {
            return Ok(cached);
        }
        let audio = self.resolve_audio(track)?;
        let ff = self.ff();
        let input = InputGuard::open(ff, &self.path)?;

        let mut transcoder = match audio {
            Some(meta) if !meta.copy_aac => {
                let in_a = input.stream(meta.stream_index as usize);
                // SAFETY: av_rescale_q computes exact starting audio PTS at 48kHz.
                let start_48k = unsafe {
                    (ff.av_rescale_q)(
                        seg.start_pts.saturating_sub(self.probe.origin_pts).max(0),
                        self.probe.video_time_base,
                        AVRational {
                            num: 1,
                            den: AAC_SAMPLE_RATE,
                        },
                    )
                };
                Some(AudioTranscoder::open(ff, stream_codecpar(in_a), start_48k)?)
            }
            _ => None,
        };

        let (mut out, out_v_tb, out_a_tb) = self.setup_muxer(
            ff,
            &input,
            audio,
            transcoder.as_ref().map(AudioTranscoder::encoder_ctx),
        )?;
        out.clear_bytes();

        // SAFETY: Seeking input file to the segment's keyframe PTS.
        let _ = unsafe {
            (ff.av_seek_frame)(
                input.ctx,
                self.probe.video_stream,
                seg.start_pts,
                AVSEEK_FLAG_BACKWARD,
            )
        };

        let end_ms = seg.start_ms.saturating_add(u64::from(seg.duration_ms));
        let origin_audio_pts = audio.map(|meta| {
            // SAFETY: Converting video origin PTS into the input audio stream's time_base.
            unsafe {
                (ff.av_rescale_q)(
                    self.probe.origin_pts,
                    self.probe.video_time_base,
                    meta.time_base,
                )
            }
        });

        // SAFETY: Allocating packet for reading input frames.
        let mut pkt = unsafe { (ff.av_packet_alloc)() };
        if pkt.is_null() {
            return Err("Falha ao alocar pacote.".into());
        }
        let mut video_started = false;
        let mut video_done = false;
        let mut audio_done = audio.is_none();
        let mut last_v_dts: Option<i64> = None;
        let mut last_a_dts: Option<i64> = None;

        let mut remux_res: Result<(), String> = Ok(());
        while remux_res.is_ok() && !(video_done && audio_done) {
            // SAFETY: Reading next packet from input.
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

            if sidx == self.probe.video_stream && !video_done {
                let ts = if pts != AV_NOPTS_VALUE { pts } else { dts };
                let is_key = (flags & AV_PKT_FLAG_KEY) != 0;
                if !video_started
                    && ts != AV_NOPTS_VALUE
                    && (ts >= seg.start_pts || (is_key && ts >= seg.start_pts.saturating_sub(2)))
                {
                    video_started = true;
                }
                if video_started {
                    if let Some(end_pts) = seg.end_pts
                        && is_key
                        && ts != AV_NOPTS_VALUE
                        && ts >= end_pts
                    {
                        video_done = true;
                    } else {
                        remux_res = write_shifted_packet(
                            ff,
                            out.ctx,
                            pkt,
                            0,
                            self.probe.origin_pts,
                            self.probe.video_time_base,
                            out_v_tb,
                            &mut last_v_dts,
                        );
                    }
                }
            } else if let Some(meta) = audio
                && sidx == meta.stream_index
                && !audio_done
            {
                let ts = if pts != AV_NOPTS_VALUE { pts } else { dts };
                let pkt_ms = if ts != AV_NOPTS_VALUE {
                    pts_to_ms(ff, ts, origin_audio_pts.unwrap_or(0), meta.time_base)
                } else {
                    seg.start_ms
                };
                if seg.end_pts.is_some() && pkt_ms >= end_ms.saturating_add(40) {
                    audio_done = true;
                } else if pkt_ms + 80 >= seg.start_ms {
                    let a_tb = out_a_tb.unwrap();
                    if let Some(tc) = &mut transcoder {
                        remux_res = tc.push_packet(pkt, |enc_pkt| {
                            write_encoded_audio(ff, out.ctx, enc_pkt, a_tb, &mut last_a_dts)
                        });
                    } else {
                        remux_res = write_shifted_packet(
                            ff,
                            out.ctx,
                            pkt,
                            1,
                            origin_audio_pts.unwrap_or(0),
                            meta.time_base,
                            a_tb,
                            &mut last_a_dts,
                        );
                    }
                }
            }
            // SAFETY: Unreferencing read packet before next iteration.
            unsafe { (ff.av_packet_unref)(pkt) };
        }
        // SAFETY: Freeing input packet.
        unsafe { (ff.av_packet_free)(&mut pkt) };
        remux_res?;

        if let Some(tc) = &mut transcoder
            && let Some(a_tb) = out_a_tb
        {
            tc.flush(|enc_pkt| write_encoded_audio(ff, out.ctx, enc_pkt, a_tb, &mut last_a_dts))?;
        }

        // SAFETY: Flushing the current fMP4 fragment (moof + mdat) to memory.
        if unsafe { (ff.av_write_frame)(out.ctx, std::ptr::null_mut()) } < 0 {
            return Err("Falha ao finalizar fragmento MP4.".into());
        }
        let mut bytes = out.take_bytes();
        let rel_pts = seg.start_pts.saturating_sub(self.probe.origin_pts).max(0);
        // SAFETY: Rescaling segment start PTS to output video and audio timescales.
        let video_base = unsafe { (ff.av_rescale_q)(rel_pts, self.probe.video_time_base, out_v_tb) }
            .max(0) as u64;
        let audio_base = out_a_tb.map(|a_tb| {
            // SAFETY: Rescaling segment start PTS to output audio timescale.
            unsafe { (ff.av_rescale_q)(rel_pts, self.probe.video_time_base, a_tb) }.max(0) as u64
        });
        if video_base > 0 || audio_base.is_some_and(|v| v > 0) {
            patch_moof_tfdt(&mut bytes, video_base, audio_base);
        }
        let mut cache = self.cache.lock().unwrap();
        cache.segments.insert((track, index), bytes.clone());
        cache.order.push_back((track, index));
        while cache.segments.len() > MAX_CACHED_SEGMENTS {
            if let Some(evicted) = cache.order.pop_front() {
                cache.segments.remove(&evicted);
            }
        }
        Ok(bytes)
    }
}

fn verify_transcode_abi(ff: &Ffmpeg, path: &Path, probe: &MediaProbe) -> Result<(), String> {
    let Some(meta) = probe.audio_streams.iter().find(|a| !a.copy_aac) else {
        return Ok(());
    };
    let input = InputGuard::open(ff, path)?;
    let in_a = input.stream(meta.stream_index as usize);
    verify_audio_stream_abi(ff, input.ctx, meta.stream_index, stream_codecpar(in_a))
}

#[allow(clippy::too_many_arguments)]
fn write_shifted_packet(
    ff: &Ffmpeg,
    out_ctx: *mut c_void,
    pkt: *mut c_void,
    out_stream_index: c_int,
    origin_pts: i64,
    in_tb: AVRational,
    out_tb: AVRational,
    last_dts: &mut Option<i64>,
) -> Result<(), String> {
    // SAFETY: Shifting and rescaling packet timestamps for the output stream.
    unsafe {
        let mut pts: i64 = read_at(pkt, 8);
        let mut dts: i64 = read_at(pkt, 16);
        if pts != AV_NOPTS_VALUE {
            pts = pts.saturating_sub(origin_pts);
        }
        if dts != AV_NOPTS_VALUE {
            dts = dts.saturating_sub(origin_pts);
        } else {
            dts = pts;
        }
        if pts == AV_NOPTS_VALUE {
            pts = dts;
        }
        if pts < dts {
            pts = dts;
        }
        write_at(pkt, 8, pts);
        write_at(pkt, 16, dts);
        (ff.av_packet_rescale_ts)(pkt, in_tb, out_tb);
        let mut out_dts: i64 = read_at(pkt, 16);
        let mut out_pts: i64 = read_at(pkt, 8);
        if let Some(prev) = *last_dts
            && out_dts <= prev
        {
            out_dts = prev + 1;
            if out_pts < out_dts {
                out_pts = out_dts;
            }
            write_at(pkt, 8, out_pts);
            write_at(pkt, 16, out_dts);
        }
        *last_dts = Some(out_dts);
        write_at::<c_int>(pkt, 36, out_stream_index);
        write_at::<i64>(pkt, 72, -1);
        if (ff.av_interleaved_write_frame)(out_ctx, pkt) < 0 {
            return Err("Falha ao gravar pacote no MP4 fragmentado.".into());
        }
    }
    Ok(())
}

fn write_encoded_audio(
    ff: &Ffmpeg,
    out_ctx: *mut c_void,
    pkt: *mut c_void,
    out_tb: AVRational,
    last_dts: &mut Option<i64>,
) -> Result<(), String> {
    let enc_tb = AVRational {
        num: 1,
        den: AAC_SAMPLE_RATE,
    };
    write_shifted_packet(ff, out_ctx, pkt, 1, 0, enc_tb, out_tb, last_dts)
}

fn patch_moof_tfdt(bytes: &mut [u8], video_base: u64, audio_base: Option<u64>) {
    let mut pos = 0;
    while pos + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        if size < 8 || pos + size > bytes.len() {
            break;
        }
        if &bytes[pos + 4..pos + 8] == b"moof" {
            let mut inner = pos + 8;
            let moof_end = pos + size;
            while inner + 8 <= moof_end {
                let csize =
                    u32::from_be_bytes(bytes[inner..inner + 4].try_into().unwrap()) as usize;
                if csize < 8 || inner + csize > moof_end {
                    break;
                }
                if &bytes[inner + 4..inner + 8] == b"traf" {
                    patch_traf_tfdt(&mut bytes[inner + 8..inner + csize], video_base, audio_base);
                }
                inner += csize;
            }
        }
        pos += size;
    }
}

fn patch_traf_tfdt(traf: &mut [u8], video_base: u64, audio_base: Option<u64>) {
    let mut track_id: u32 = 0;
    let mut tfdt_pos: Option<(usize, usize)> = None;
    let mut pos = 0;
    while pos + 8 <= traf.len() {
        let size = u32::from_be_bytes(traf[pos..pos + 4].try_into().unwrap()) as usize;
        if size < 8 || pos + size > traf.len() {
            break;
        }
        match &traf[pos + 4..pos + 8] {
            b"tfhd" if size >= 16 => {
                track_id = u32::from_be_bytes(traf[pos + 12..pos + 16].try_into().unwrap());
            }
            b"tfdt" if size >= 16 => {
                tfdt_pos = Some((pos, size));
            }
            _ => {}
        }
        pos += size;
    }
    let delta = match track_id {
        1 => video_base,
        2 => audio_base.unwrap_or(0),
        _ => return,
    };
    if let Some((start, size)) = tfdt_pos {
        let version = traf[start + 8];
        if version == 1 && size >= 20 {
            let cur = u64::from_be_bytes(traf[start + 12..start + 20].try_into().unwrap());
            let next = cur.saturating_add(delta);
            traf[start + 12..start + 20].copy_from_slice(&next.to_be_bytes());
        } else if version == 0 {
            let cur = u32::from_be_bytes(traf[start + 12..start + 16].try_into().unwrap());
            let next = cur.saturating_add(delta.min(u64::from(u32::MAX)) as u32);
            traf[start + 12..start + 16].copy_from_slice(&next.to_be_bytes());
        }
    }
}
