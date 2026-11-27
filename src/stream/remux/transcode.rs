use super::super::ffmpeg::{
    AV_CODEC_FLAG_GLOBAL_HEADER, AV_CODEC_ID_AAC, AV_OPT_SEARCH_CHILDREN, AV_SAMPLE_FMT_FLTP,
    AV_SAMPLE_FMT_NONE, AVChannelLayout, AVERROR_EOF, Ffmpeg, read_at, write_at,
};
use std::ffi::{c_int, c_void};

pub const AAC_SAMPLE_RATE: c_int = 48_000;
pub const AAC_CHANNELS: c_int = 2;
pub const AAC_BITRATE: i64 = 192_000;
pub const AAC_FRAME_SIZE: c_int = 1024;
const EAGAIN_POSIX: c_int = -11;
const EAGAIN_DARWIN: c_int = -35;

pub fn is_eagain(code: c_int) -> bool {
    code == EAGAIN_POSIX || code == EAGAIN_DARWIN
}

pub struct EncoderGuard<'a> {
    ff: &'a Ffmpeg,
    pub ctx: *mut c_void,
}

impl<'a> EncoderGuard<'a> {
    pub fn open_aac(ff: &'a Ffmpeg) -> Result<Self, String> {
        // SAFETY: avcodec_find_encoder returns a static codec descriptor or null.
        let codec = unsafe { (ff.avcodec_find_encoder)(AV_CODEC_ID_AAC) };
        if codec.is_null() {
            return Err("Codificador AAC indisponível no FFmpeg.".into());
        }
        // SAFETY: avcodec_alloc_context3 allocates a new AVCodecContext for the codec.
        let ctx = unsafe { (ff.avcodec_alloc_context3)(codec) };
        if ctx.is_null() {
            return Err("Falha ao alocar contexto do codificador AAC.".into());
        }
        let guard = Self { ff, ctx };
        let mut layout = AVChannelLayout::default();
        // SAFETY: Configuring AVCodecContext via public AVOptions and verified sample_fmt offset.
        let opened = unsafe {
            (ff.av_channel_layout_default)(&mut layout, AAC_CHANNELS);
            let opts_ok =
                (ff.av_opt_set_int)(ctx, c"b".as_ptr(), AAC_BITRATE, AV_OPT_SEARCH_CHILDREN) >= 0
                    && (ff.av_opt_set_int)(
                        ctx,
                        c"ar".as_ptr(),
                        i64::from(AAC_SAMPLE_RATE),
                        AV_OPT_SEARCH_CHILDREN,
                    ) >= 0
                    && (ff.av_opt_set_chlayout)(
                        ctx,
                        c"ch_layout".as_ptr(),
                        &layout,
                        AV_OPT_SEARCH_CHILDREN,
                    ) >= 0
                    && (ff.av_opt_set_int)(
                        ctx,
                        c"flags".as_ptr(),
                        AV_CODEC_FLAG_GLOBAL_HEADER,
                        AV_OPT_SEARCH_CHILDREN,
                    ) >= 0;
            (ff.av_channel_layout_uninit)(&mut layout);
            let fmt_ok = read_at::<c_int>(ctx, ff.abi.ctx_sample_fmt) == AV_SAMPLE_FMT_NONE;
            if opts_ok && fmt_ok {
                write_at::<c_int>(ctx, ff.abi.ctx_sample_fmt, AV_SAMPLE_FMT_FLTP);
                (ff.avcodec_open2)(ctx, codec, std::ptr::null_mut()) >= 0
            } else {
                false
            }
        };
        if !opened {
            return Err("Falha ao abrir codificador AAC.".into());
        }
        Ok(guard)
    }
}

impl Drop for EncoderGuard<'_> {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // SAFETY: self.ctx was allocated by avcodec_alloc_context3.
            unsafe { (self.ff.avcodec_free_context)(&mut self.ctx) };
        }
    }
}

pub struct AudioTranscoder<'a> {
    ff: &'a Ffmpeg,
    dec: *mut c_void,
    enc: EncoderGuard<'a>,
    swr: *mut c_void,
    fifo: *mut c_void,
    in_frame: *mut c_void,
    swr_frame: *mut c_void,
    enc_frame: *mut c_void,
    out_pkt: *mut c_void,
    frame_abi_verified: bool,
    pub next_pts: i64,
}

impl<'a> AudioTranscoder<'a> {
    pub fn open(
        ff: &'a Ffmpeg,
        in_codecpar: *const c_void,
        start_pts_48k: i64,
    ) -> Result<Self, String> {
        // SAFETY: in_codecpar is a valid AVCodecParameters pointer from the input stream.
        let codec_id: c_int = unsafe { read_at(in_codecpar, 4) };
        // SAFETY: avcodec_find_decoder returns a static codec descriptor or null.
        let dec_codec = unsafe { (ff.avcodec_find_decoder)(codec_id) };
        if dec_codec.is_null() {
            return Err("Decodificador de áudio indisponível no FFmpeg.".into());
        }
        // SAFETY: Allocating and opening decoder context from input codecpar.
        let dec = unsafe { (ff.avcodec_alloc_context3)(dec_codec) };
        if dec.is_null() {
            return Err("Falha ao alocar decodificador de áudio.".into());
        }
        // SAFETY: Copying codecpar into decoder context and opening it.
        let opened = unsafe {
            (ff.avcodec_parameters_to_context)(dec, in_codecpar) >= 0
                && (ff.avcodec_open2)(dec, dec_codec, std::ptr::null_mut()) >= 0
        };
        if !opened {
            let mut d = dec;
            // SAFETY: Cleaning up allocated decoder context on failure.
            unsafe { (ff.avcodec_free_context)(&mut d) };
            return Err("Falha ao abrir decodificador de áudio.".into());
        }
        let enc = match EncoderGuard::open_aac(ff) {
            Ok(enc) => enc,
            Err(err) => {
                let mut d = dec;
                // SAFETY: Cleaning up allocated decoder context on failure.
                unsafe { (ff.avcodec_free_context)(&mut d) };
                return Err(err);
            }
        };
        // SAFETY: Allocating swresample, audio FIFO, frames, and packet.
        let (swr, fifo, in_frame, swr_frame, enc_frame, out_pkt) = unsafe {
            (
                (ff.swr_alloc)(),
                (ff.av_audio_fifo_alloc)(AV_SAMPLE_FMT_FLTP, AAC_CHANNELS, AAC_FRAME_SIZE * 4),
                (ff.av_frame_alloc)(),
                (ff.av_frame_alloc)(),
                (ff.av_frame_alloc)(),
                (ff.av_packet_alloc)(),
            )
        };
        let this = Self {
            ff,
            dec,
            enc,
            swr,
            fifo,
            in_frame,
            swr_frame,
            enc_frame,
            out_pkt,
            frame_abi_verified: false,
            next_pts: start_pts_48k,
        };
        if swr.is_null()
            || fifo.is_null()
            || in_frame.is_null()
            || swr_frame.is_null()
            || enc_frame.is_null()
            || out_pkt.is_null()
        {
            return Err("Falha ao alocar buffers de áudio.".into());
        }
        Ok(this)
    }

    pub fn encoder_ctx(&self) -> *mut c_void {
        self.enc.ctx
    }

    fn configure_output_frame(&mut self, frame: *mut c_void, nb_samples: c_int) {
        let mut layout = AVChannelLayout::default();
        // SAFETY: Writing standard FLTP stereo 48kHz properties into an unreferenced AVFrame.
        unsafe {
            (self.ff.av_channel_layout_default)(&mut layout, AAC_CHANNELS);
            write_at::<c_int>(frame, 112, nb_samples);
            write_at::<c_int>(frame, 116, AV_SAMPLE_FMT_FLTP);
            write_at::<c_int>(frame, self.ff.abi.frame_sample_rate, AAC_SAMPLE_RATE);
            let dst = frame
                .cast::<u8>()
                .add(self.ff.abi.frame_ch_layout)
                .cast::<AVChannelLayout>();
            let _ = (self.ff.av_channel_layout_copy)(dst, &layout);
            (self.ff.av_channel_layout_uninit)(&mut layout);
        }
    }

    pub fn push_packet(
        &mut self,
        pkt: *const c_void,
        mut emit: impl FnMut(*mut c_void) -> Result<(), String>,
    ) -> Result<(), String> {
        // SAFETY: Sending an input audio packet (or null to flush) to the decoder.
        let send = unsafe { (self.ff.avcodec_send_packet)(self.dec, pkt) };
        if send < 0 && !is_eagain(send) && send != AVERROR_EOF {
            return Ok(());
        }
        loop {
            // SAFETY: Receiving decoded audio frames from the decoder.
            let r = unsafe { (self.ff.avcodec_receive_frame)(self.dec, self.in_frame) };
            if r < 0 {
                break;
            }
            if !self.frame_abi_verified {
                if let Err(err) = verify_frame_abi(self.ff, self.dec, self.in_frame) {
                    // SAFETY: Unreferencing decoded frame before aborting on ABI mismatch.
                    unsafe { (self.ff.av_frame_unref)(self.in_frame) };
                    return Err(err);
                }
                self.frame_abi_verified = true;
            }
            self.resample_and_queue(self.in_frame)?;
            // SAFETY: Unreferencing decoded frame after resampling.
            unsafe { (self.ff.av_frame_unref)(self.in_frame) };
            self.drain_fifo(false, &mut emit)?;
        }
        Ok(())
    }

    pub fn flush(
        &mut self,
        mut emit: impl FnMut(*mut c_void) -> Result<(), String>,
    ) -> Result<(), String> {
        self.push_packet(std::ptr::null(), &mut emit)?;
        if self.frame_abi_verified {
            self.resample_and_queue(std::ptr::null())?;
        }
        self.drain_fifo(true, &mut emit)?;
        // SAFETY: Flushing the AAC encoder with a null frame.
        let _ = unsafe { (self.ff.avcodec_send_frame)(self.enc.ctx, std::ptr::null()) };
        self.drain_encoder(&mut emit)?;
        Ok(())
    }

    fn resample_and_queue(&mut self, in_frame: *const c_void) -> Result<(), String> {
        // SAFETY: Unreferencing and configuring swr output frame before swr_convert_frame.
        unsafe { (self.ff.av_frame_unref)(self.swr_frame) };
        self.configure_output_frame(self.swr_frame, 0);
        // SAFETY: swr_convert_frame converts/resamples in_frame into swr_frame.
        let ret = unsafe { (self.ff.swr_convert_frame)(self.swr, self.swr_frame, in_frame) };
        if ret < 0 {
            return Ok(());
        }
        // SAFETY: Reading nb_samples and extended_data from populated swr_frame.
        let samples: c_int = unsafe { read_at(self.swr_frame, 112) };
        if samples > 0 {
            // SAFETY: extended_data points to planar audio buffers with `samples` per channel.
            let ext: *mut *mut c_void = unsafe { read_at(self.swr_frame, 96) };
            // SAFETY: Pushing converted samples into AVAudioFifo.
            let written = unsafe { (self.ff.av_audio_fifo_write)(self.fifo, ext, samples) };
            if written < 0 {
                return Err("Falha ao gravar amostras no buffer de áudio.".into());
            }
        }
        // SAFETY: Unreferencing swr_frame after copying samples into FIFO.
        unsafe { (self.ff.av_frame_unref)(self.swr_frame) };
        Ok(())
    }

    fn drain_fifo(
        &mut self,
        flush_tail: bool,
        emit: &mut impl FnMut(*mut c_void) -> Result<(), String>,
    ) -> Result<(), String> {
        loop {
            // SAFETY: Querying available samples in AVAudioFifo.
            let available = unsafe { (self.ff.av_audio_fifo_size)(self.fifo) };
            let take = if available >= AAC_FRAME_SIZE {
                AAC_FRAME_SIZE
            } else if flush_tail && available > 0 {
                available
            } else {
                break;
            };
            // SAFETY: Resetting enc_frame before allocating new sample buffers.
            unsafe { (self.ff.av_frame_unref)(self.enc_frame) };
            self.configure_output_frame(self.enc_frame, take);
            // SAFETY: Allocating sample buffer for enc_frame and reading from FIFO.
            let ok = unsafe {
                if (self.ff.av_frame_get_buffer)(self.enc_frame, 0) < 0 {
                    false
                } else {
                    let ext: *mut *mut c_void = read_at(self.enc_frame, 96);
                    (self.ff.av_audio_fifo_read)(self.fifo, ext, take) >= 0
                }
            };
            if !ok {
                return Err("Falha ao preparar quadro de áudio AAC.".into());
            }
            // SAFETY: Setting monotonic PTS in 48kHz samples on enc_frame.
            unsafe { write_at::<i64>(self.enc_frame, 136, self.next_pts) };
            self.next_pts += i64::from(take);
            // SAFETY: Sending audio frame to AAC encoder.
            let sent = unsafe { (self.ff.avcodec_send_frame)(self.enc.ctx, self.enc_frame) };
            // SAFETY: Unreferencing enc_frame after sending to encoder.
            unsafe { (self.ff.av_frame_unref)(self.enc_frame) };
            if sent < 0 && !is_eagain(sent) {
                return Err("Falha ao codificar áudio para AAC.".into());
            }
            self.drain_encoder(emit)?;
        }
        Ok(())
    }

    fn drain_encoder(
        &mut self,
        emit: &mut impl FnMut(*mut c_void) -> Result<(), String>,
    ) -> Result<(), String> {
        loop {
            // SAFETY: Receiving encoded AAC packets from encoder.
            let r = unsafe { (self.ff.avcodec_receive_packet)(self.enc.ctx, self.out_pkt) };
            if r < 0 {
                break;
            }
            let res = emit(self.out_pkt);
            // SAFETY: Unreferencing encoded packet after muxing.
            unsafe { (self.ff.av_packet_unref)(self.out_pkt) };
            res?;
        }
        Ok(())
    }
}

impl Drop for AudioTranscoder<'_> {
    fn drop(&mut self) {
        // SAFETY: Freeing all FFmpeg resources allocated in AudioTranscoder::open.
        unsafe {
            if !self.out_pkt.is_null() {
                (self.ff.av_packet_free)(&mut self.out_pkt);
            }
            if !self.enc_frame.is_null() {
                (self.ff.av_frame_free)(&mut self.enc_frame);
            }
            if !self.swr_frame.is_null() {
                (self.ff.av_frame_free)(&mut self.swr_frame);
            }
            if !self.in_frame.is_null() {
                (self.ff.av_frame_free)(&mut self.in_frame);
            }
            if !self.fifo.is_null() {
                (self.ff.av_audio_fifo_free)(self.fifo);
            }
            if !self.swr.is_null() {
                (self.ff.swr_free)(&mut self.swr);
            }
            if !self.dec.is_null() {
                (self.ff.avcodec_free_context)(&mut self.dec);
            }
        }
    }
}

fn verify_frame_abi(ff: &Ffmpeg, dec: *mut c_void, frame: *const c_void) -> Result<(), String> {
    let mut expected_rate: i64 = 0;
    let mut expected_layout = AVChannelLayout::default();
    // SAFETY: Querying decoder sample_rate and ch_layout through public AVOptions.
    let expected = unsafe {
        let rate_ok = (ff.av_opt_get_int)(
            dec,
            c"ar".as_ptr(),
            AV_OPT_SEARCH_CHILDREN,
            &mut expected_rate,
        ) >= 0;
        let layout_ok = (ff.av_opt_get_chlayout)(
            dec,
            c"ch_layout".as_ptr(),
            AV_OPT_SEARCH_CHILDREN,
            &mut expected_layout,
        ) >= 0;
        let channels = expected_layout.nb_channels;
        (ff.av_channel_layout_uninit)(&mut expected_layout);
        if rate_ok && layout_ok && expected_rate > 0 && channels > 0 {
            Some((expected_rate, channels))
        } else {
            None
        }
    };
    let Some((expected_rate, expected_channels)) = expected else {
        return Err("Falha ao consultar parâmetros do decodificador de áudio.".into());
    };
    // SAFETY: Reading AVFrame fields at verified offsets before any struct write occurs.
    let (frame_rate, frame_layout, nb_samples, ext): (
        c_int,
        AVChannelLayout,
        c_int,
        *mut *mut c_void,
    ) = unsafe {
        (
            read_at(frame, ff.abi.frame_sample_rate),
            read_at(frame, ff.abi.frame_ch_layout),
            read_at(frame, 112),
            read_at(frame, 96),
        )
    };
    if i64::from(frame_rate) != expected_rate
        || frame_layout.nb_channels != expected_channels
        || nb_samples <= 0
        || ext.is_null()
    {
        return Err(format!(
            "Verificação de ABI do AVFrame falhou (rate={frame_rate}/{expected_rate}, ch={}/{expected_channels}, samples={nb_samples}).",
            frame_layout.nb_channels
        ));
    }
    Ok(())
}

pub fn verify_audio_stream_abi(
    ff: &Ffmpeg,
    input_ctx: *mut c_void,
    stream_index: c_int,
    in_codecpar: *const c_void,
) -> Result<(), String> {
    // SAFETY: in_codecpar is a valid AVCodecParameters pointer from the input stream.
    let codec_id: c_int = unsafe { read_at(in_codecpar, 4) };
    // SAFETY: avcodec_find_decoder returns a static codec descriptor or null.
    let dec_codec = unsafe { (ff.avcodec_find_decoder)(codec_id) };
    if dec_codec.is_null() {
        return Err("Decodificador de áudio indisponível no FFmpeg.".into());
    }
    // SAFETY: Allocating and opening decoder context, packet, and frame for read-only ABI check.
    let mut dec = unsafe { (ff.avcodec_alloc_context3)(dec_codec) };
    if dec.is_null() {
        return Err("Falha ao alocar decodificador de áudio.".into());
    }
    // SAFETY: Copying codecpar into decoder context and opening decoder.
    let opened = unsafe {
        (ff.avcodec_parameters_to_context)(dec, in_codecpar) >= 0
            && (ff.avcodec_open2)(dec, dec_codec, std::ptr::null_mut()) >= 0
    };
    if !opened {
        // SAFETY: Freeing decoder context on open failure.
        unsafe { (ff.avcodec_free_context)(&mut dec) };
        return Err("Falha ao abrir decodificador de áudio.".into());
    }
    // SAFETY: Allocating temporary packet and frame for first-frame ABI verification.
    let (mut pkt, mut frame) = unsafe { ((ff.av_packet_alloc)(), (ff.av_frame_alloc)()) };
    let mut result = if pkt.is_null() || frame.is_null() {
        Err("Falha ao alocar pacote/quadro para verificação de ABI.".into())
    } else {
        Ok(())
    };
    let mut verified = false;
    while result.is_ok() && !verified {
        // SAFETY: Reading next packet from input to locate the first audio frame.
        if unsafe { (ff.av_read_frame)(input_ctx, pkt) } < 0 {
            break;
        }
        // SAFETY: AVPacket.stream_index is at offset 36 in lavc 60..=63.
        let sidx: c_int = unsafe { read_at(pkt, 36) };
        if sidx == stream_index {
            // SAFETY: Sending packet to decoder and receiving first decoded audio frame.
            let sent = unsafe { (ff.avcodec_send_packet)(dec, pkt) };
            if sent >= 0 || is_eagain(sent) {
                // SAFETY: Receiving decoded frame for read-only ABI self-check.
                if unsafe { (ff.avcodec_receive_frame)(dec, frame) } >= 0 {
                    result = verify_frame_abi(ff, dec, frame);
                    // SAFETY: Unreferencing decoded frame after verification.
                    unsafe { (ff.av_frame_unref)(frame) };
                    verified = true;
                }
            }
        }
        // SAFETY: Unreferencing packet after inspection.
        unsafe { (ff.av_packet_unref)(pkt) };
    }
    // SAFETY: Freeing temporary frame, packet, and decoder context.
    unsafe {
        if !frame.is_null() {
            (ff.av_frame_free)(&mut frame);
        }
        if !pkt.is_null() {
            (ff.av_packet_free)(&mut pkt);
        }
        (ff.avcodec_free_context)(&mut dec);
    }
    result
}
