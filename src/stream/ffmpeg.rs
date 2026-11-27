use libloading::Library;
use std::{
    ffi::{CStr, CString, OsString, c_char, c_int, c_uint, c_void},
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub const AVMEDIA_TYPE_SUBTITLE: c_int = 3;

pub const AV_CODEC_ID_H264: c_int = 27;
pub const AV_CODEC_ID_VP8: c_int = 139;
pub const AV_CODEC_ID_VP9: c_int = 167;
pub const AV_CODEC_ID_HEVC: c_int = 173;
pub const AV_CODEC_ID_AV1: c_int = 226;
pub const AV_CODEC_ID_MP3: c_int = 86017;
pub const AV_CODEC_ID_AAC: c_int = 86018;
pub const AV_CODEC_ID_AC3: c_int = 86019;
pub const AV_CODEC_ID_DTS: c_int = 86020;
pub const AV_CODEC_ID_VORBIS: c_int = 86021;
pub const AV_CODEC_ID_EAC3: c_int = 86056;
pub const AV_CODEC_ID_OPUS: c_int = 86076;

pub const AV_SAMPLE_FMT_NONE: c_int = -1;
pub const AV_SAMPLE_FMT_FLTP: c_int = 8;
pub const AV_PKT_FLAG_KEY: c_int = 1;
pub const AVSEEK_FLAG_BACKWARD: c_int = 1;
pub const AV_CODEC_FLAG_GLOBAL_HEADER: i64 = 1 << 22;
pub const AV_DISPOSITION_DEFAULT: c_int = 1;
pub const AV_NOPTS_VALUE: i64 = i64::MIN;
pub const AV_TIME_BASE: i64 = 1_000_000;
pub const AVERROR_EOF: c_int = -541_478_725;
pub const AV_OPT_SEARCH_CHILDREN: c_int = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AVRational {
    pub num: c_int,
    pub den: c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AVChannelLayout {
    pub order: c_int,
    pub nb_channels: c_int,
    pub u: u64,
    pub opaque: *mut c_void,
}

impl Default for AVChannelLayout {
    fn default() -> Self {
        Self {
            order: 0,
            nb_channels: 0,
            u: 0,
            opaque: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
pub struct AVDictionaryEntry {
    pub key: *mut c_char,
    pub value: *mut c_char,
}

#[repr(C)]
pub struct AVIndexEntry {
    pub pos: i64,
    pub timestamp: i64,
}

pub(crate) type ReadPacketFn = unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int;
pub(crate) type WritePacketFn =
    unsafe extern "C" fn(opaque: *mut c_void, buf: *const u8, buf_size: c_int) -> c_int;
pub(crate) type SeekFn = unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfmpegVersions {
    pub lavf: u32,
    pub lavc: u32,
    pub lavu: u32,
    pub swr: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiOffsets {
    pub fmt_duration: usize,
    pub ctx_sample_fmt: usize,
    pub frame_sample_rate: usize,
    pub frame_ch_layout: usize,
}

impl AbiOffsets {
    pub fn for_versions(lavf: u32, lavc: u32, lavu: u32, swr: u32) -> Result<Self, String> {
        match (lavf, lavc, lavu, swr) {
            (60, 60, 58, 4) => Ok(Self {
                fmt_duration: 72,
                ctx_sample_fmt: 360,
                frame_sample_rate: 208,
                frame_ch_layout: 448,
            }),
            (61, 61, 59, 5) => Ok(Self {
                fmt_duration: 104,
                ctx_sample_fmt: 348,
                frame_sample_rate: 192,
                frame_ch_layout: 408,
            }),
            (62, 62, 60, 6) | (63, 63, 61, 7) => Ok(Self {
                fmt_duration: 104,
                ctx_sample_fmt: 348,
                frame_sample_rate: 180,
                frame_ch_layout: 384,
            }),
            _ => Err(format!(
                "Versão incompatível do FFmpeg (lavf={lavf}, lavc={lavc}, lavu={lavu}, swr={swr})"
            )),
        }
    }
}

pub struct Ffmpeg {
    _avutil: Library,
    _swresample: Library,
    _avcodec: Library,
    _avformat: Library,
    pub versions: FfmpegVersions,
    pub abi: AbiOffsets,
    pub avformat_open_input: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_char,
        *const c_void,
        *mut *mut c_void,
    ) -> c_int,
    pub avformat_find_stream_info: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    pub avformat_close_input: unsafe extern "C" fn(*mut *mut c_void),
    pub avformat_alloc_output_context2: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_void,
        *const c_char,
        *const c_char,
    ) -> c_int,
    pub avformat_free_context: unsafe extern "C" fn(*mut c_void),
    pub avformat_new_stream: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void,
    pub avformat_write_header: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    pub av_write_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub av_interleaved_write_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub av_read_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub av_seek_frame: unsafe extern "C" fn(*mut c_void, c_int, i64, c_int) -> c_int,
    pub avformat_index_get_entries_count: unsafe extern "C" fn(*const c_void) -> c_int,
    pub avformat_index_get_entry: unsafe extern "C" fn(*mut c_void, c_int) -> *const AVIndexEntry,
    pub(crate) avio_alloc_context: unsafe extern "C" fn(
        *mut u8,
        c_int,
        c_int,
        *mut c_void,
        Option<ReadPacketFn>,
        Option<WritePacketFn>,
        Option<SeekFn>,
    ) -> *mut c_void,
    pub avio_context_free: unsafe extern "C" fn(*mut *mut c_void),
    pub avio_flush: unsafe extern "C" fn(*mut c_void),
    pub avcodec_find_decoder: unsafe extern "C" fn(c_int) -> *const c_void,
    pub avcodec_find_encoder: unsafe extern "C" fn(c_int) -> *const c_void,
    pub avcodec_alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub avcodec_free_context: unsafe extern "C" fn(*mut *mut c_void),
    pub avcodec_parameters_to_context: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub avcodec_parameters_from_context: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub avcodec_parameters_copy: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub avcodec_open2: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
    pub avcodec_send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub avcodec_receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub avcodec_send_frame: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub avcodec_receive_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub avcodec_get_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    pub av_packet_unref: unsafe extern "C" fn(*mut c_void),
    pub av_packet_rescale_ts: unsafe extern "C" fn(*mut c_void, AVRational, AVRational),
    pub av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    pub av_frame_unref: unsafe extern "C" fn(*mut c_void),
    pub av_frame_get_buffer: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    pub av_freep: unsafe extern "C" fn(*mut c_void),
    pub av_dict_set:
        unsafe extern "C" fn(*mut *mut c_void, *const c_char, *const c_char, c_int) -> c_int,
    pub av_dict_get: unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        *const AVDictionaryEntry,
        c_int,
    ) -> *const AVDictionaryEntry,
    pub av_dict_free: unsafe extern "C" fn(*mut *mut c_void),
    pub av_opt_set_int: unsafe extern "C" fn(*mut c_void, *const c_char, i64, c_int) -> c_int,
    pub av_opt_set_chlayout:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const AVChannelLayout, c_int) -> c_int,
    pub av_opt_get_int: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut i64) -> c_int,
    pub av_opt_get_chlayout:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut AVChannelLayout) -> c_int,
    pub av_channel_layout_default: unsafe extern "C" fn(*mut AVChannelLayout, c_int),
    pub av_channel_layout_copy:
        unsafe extern "C" fn(*mut AVChannelLayout, *const AVChannelLayout) -> c_int,
    pub av_channel_layout_uninit: unsafe extern "C" fn(*mut AVChannelLayout),
    pub av_rescale_q: unsafe extern "C" fn(i64, AVRational, AVRational) -> i64,
    pub av_audio_fifo_alloc: unsafe extern "C" fn(c_int, c_int, c_int) -> *mut c_void,
    pub av_audio_fifo_free: unsafe extern "C" fn(*mut c_void),
    pub av_audio_fifo_write: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, c_int) -> c_int,
    pub av_audio_fifo_read: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, c_int) -> c_int,
    pub av_audio_fifo_size: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub av_log_set_level: unsafe extern "C" fn(c_int),
    pub swr_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub swr_free: unsafe extern "C" fn(*mut *mut c_void),
    pub swr_convert_frame: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void) -> c_int,
}

// SAFETY: Ffmpeg holds dynamically loaded C function pointers and read-only Library handles
// that are safe to share across threads; individual FFmpeg contexts are never shared across threads.
unsafe impl Send for Ffmpeg {}
// SAFETY: See Send impl above.
unsafe impl Sync for Ffmpeg {}

fn load_symbol<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: Caller ensures T matches the C function signature exported by FFmpeg.
    unsafe {
        lib.get::<T>(name)
            .map(|sym| *sym)
            .map_err(|e| format!("Símbolo FFmpeg ausente: {e}"))
    }
}

fn lib_candidates(base: &str, versions: &[u32], custom_dir: Option<&Path>) -> Vec<OsString> {
    let mut out = Vec::new();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));

    #[cfg(target_os = "windows")]
    let names: Vec<String> = versions
        .iter()
        .map(|v| format!("{base}-{v}.dll"))
        .chain(std::iter::once(format!("{base}.dll")))
        .collect();

    #[cfg(target_os = "macos")]
    let names: Vec<String> = versions
        .iter()
        .map(|v| format!("lib{base}.{v}.dylib"))
        .chain(std::iter::once(format!("lib{base}.dylib")))
        .collect();

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let names: Vec<String> = versions
        .iter()
        .map(|v| format!("lib{base}.so.{v}"))
        .chain(std::iter::once(format!("lib{base}.so")))
        .collect();

    if let Some(dir) = custom_dir {
        for name in &names {
            out.push(dir.join(name).into_os_string());
        }
        return out;
    }

    #[cfg(target_os = "macos")]
    if let Some(dir) = &exe_dir {
        for name in &names {
            out.push(dir.join("../Frameworks").join(name).into_os_string());
            out.push(dir.join(name).into_os_string());
        }
    }
    #[cfg(target_os = "windows")]
    if let Some(dir) = &exe_dir {
        for name in &names {
            out.push(dir.join(name).into_os_string());
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = exe_dir;

    for name in &names {
        out.push(OsString::from(name));
    }

    #[cfg(target_os = "macos")]
    for prefix in ["/opt/homebrew/lib", "/usr/local/lib"] {
        for name in &names {
            out.push(Path::new(prefix).join(name).into_os_string());
        }
    }

    out
}

fn open_first(candidates: &[OsString], label: &str) -> Result<Library, String> {
    let mut last = String::new();
    for candidate in candidates {
        // SAFETY: Loading a platform shared library by path or soname.
        match unsafe { Library::new(candidate) } {
            Ok(lib) => return Ok(lib),
            Err(err) => last = err.to_string(),
        }
    }
    Err(format!("Biblioteca {label} indisponível ({last})"))
}

impl Ffmpeg {
    pub fn load_from_dir(custom_dir: Option<&Path>) -> Result<Self, String> {
        let avutil = open_first(
            &lib_candidates("avutil", &[61, 60, 59, 58], custom_dir),
            "libavutil",
        )?;
        let swresample = open_first(
            &lib_candidates("swresample", &[7, 6, 5, 4], custom_dir),
            "libswresample",
        )?;
        let avcodec = open_first(
            &lib_candidates("avcodec", &[63, 62, 61, 60], custom_dir),
            "libavcodec",
        )?;
        let avformat = open_first(
            &lib_candidates("avformat", &[63, 62, 61, 60], custom_dir),
            "libavformat",
        )?;

        let avutil_version: unsafe extern "C" fn() -> c_uint =
            load_symbol(&avutil, b"avutil_version\0")?;
        let swr_version: unsafe extern "C" fn() -> c_uint =
            load_symbol(&swresample, b"swresample_version\0")?;
        let avcodec_version: unsafe extern "C" fn() -> c_uint =
            load_symbol(&avcodec, b"avcodec_version\0")?;
        let avformat_version: unsafe extern "C" fn() -> c_uint =
            load_symbol(&avformat, b"avformat_version\0")?;

        // SAFETY: FFmpeg version functions are pure C getters with no preconditions.
        let (lavu, swr, lavc, lavf) = unsafe {
            (
                avutil_version() >> 16,
                swr_version() >> 16,
                avcodec_version() >> 16,
                avformat_version() >> 16,
            )
        };
        let versions = FfmpegVersions {
            lavf,
            lavc,
            lavu,
            swr,
        };
        let abi = AbiOffsets::for_versions(lavf, lavc, lavu, swr)?;

        let api = Self {
            avformat_open_input: load_symbol(&avformat, b"avformat_open_input\0")?,
            avformat_find_stream_info: load_symbol(&avformat, b"avformat_find_stream_info\0")?,
            avformat_close_input: load_symbol(&avformat, b"avformat_close_input\0")?,
            avformat_alloc_output_context2: load_symbol(
                &avformat,
                b"avformat_alloc_output_context2\0",
            )?,
            avformat_free_context: load_symbol(&avformat, b"avformat_free_context\0")?,
            avformat_new_stream: load_symbol(&avformat, b"avformat_new_stream\0")?,
            avformat_write_header: load_symbol(&avformat, b"avformat_write_header\0")?,
            av_write_frame: load_symbol(&avformat, b"av_write_frame\0")?,
            av_interleaved_write_frame: load_symbol(&avformat, b"av_interleaved_write_frame\0")?,
            av_read_frame: load_symbol(&avformat, b"av_read_frame\0")?,
            av_seek_frame: load_symbol(&avformat, b"av_seek_frame\0")?,
            avformat_index_get_entries_count: load_symbol(
                &avformat,
                b"avformat_index_get_entries_count\0",
            )?,
            avformat_index_get_entry: load_symbol(&avformat, b"avformat_index_get_entry\0")?,
            avio_alloc_context: load_symbol(&avformat, b"avio_alloc_context\0")?,
            avio_context_free: load_symbol(&avformat, b"avio_context_free\0")?,
            avio_flush: load_symbol(&avformat, b"avio_flush\0")?,
            avcodec_find_decoder: load_symbol(&avcodec, b"avcodec_find_decoder\0")?,
            avcodec_find_encoder: load_symbol(&avcodec, b"avcodec_find_encoder\0")?,
            avcodec_alloc_context3: load_symbol(&avcodec, b"avcodec_alloc_context3\0")?,
            avcodec_free_context: load_symbol(&avcodec, b"avcodec_free_context\0")?,
            avcodec_parameters_to_context: load_symbol(
                &avcodec,
                b"avcodec_parameters_to_context\0",
            )?,
            avcodec_parameters_from_context: load_symbol(
                &avcodec,
                b"avcodec_parameters_from_context\0",
            )?,
            avcodec_parameters_copy: load_symbol(&avcodec, b"avcodec_parameters_copy\0")?,
            avcodec_open2: load_symbol(&avcodec, b"avcodec_open2\0")?,
            avcodec_send_packet: load_symbol(&avcodec, b"avcodec_send_packet\0")?,
            avcodec_receive_frame: load_symbol(&avcodec, b"avcodec_receive_frame\0")?,
            avcodec_send_frame: load_symbol(&avcodec, b"avcodec_send_frame\0")?,
            avcodec_receive_packet: load_symbol(&avcodec, b"avcodec_receive_packet\0")?,
            avcodec_get_name: load_symbol(&avcodec, b"avcodec_get_name\0")?,
            av_packet_alloc: load_symbol(&avcodec, b"av_packet_alloc\0")?,
            av_packet_free: load_symbol(&avcodec, b"av_packet_free\0")?,
            av_packet_unref: load_symbol(&avcodec, b"av_packet_unref\0")?,
            av_packet_rescale_ts: load_symbol(&avcodec, b"av_packet_rescale_ts\0")?,
            av_frame_alloc: load_symbol(&avutil, b"av_frame_alloc\0")?,
            av_frame_free: load_symbol(&avutil, b"av_frame_free\0")?,
            av_frame_unref: load_symbol(&avutil, b"av_frame_unref\0")?,
            av_frame_get_buffer: load_symbol(&avutil, b"av_frame_get_buffer\0")?,
            av_malloc: load_symbol(&avutil, b"av_malloc\0")?,
            av_freep: load_symbol(&avutil, b"av_freep\0")?,
            av_dict_set: load_symbol(&avutil, b"av_dict_set\0")?,
            av_dict_get: load_symbol(&avutil, b"av_dict_get\0")?,
            av_dict_free: load_symbol(&avutil, b"av_dict_free\0")?,
            av_opt_set_int: load_symbol(&avutil, b"av_opt_set_int\0")?,
            av_opt_set_chlayout: load_symbol(&avutil, b"av_opt_set_chlayout\0")?,
            av_opt_get_int: load_symbol(&avutil, b"av_opt_get_int\0")?,
            av_opt_get_chlayout: load_symbol(&avutil, b"av_opt_get_chlayout\0")?,
            av_channel_layout_default: load_symbol(&avutil, b"av_channel_layout_default\0")?,
            av_channel_layout_copy: load_symbol(&avutil, b"av_channel_layout_copy\0")?,
            av_channel_layout_uninit: load_symbol(&avutil, b"av_channel_layout_uninit\0")?,
            av_rescale_q: load_symbol(&avutil, b"av_rescale_q\0")?,
            av_audio_fifo_alloc: load_symbol(&avutil, b"av_audio_fifo_alloc\0")?,
            av_audio_fifo_free: load_symbol(&avutil, b"av_audio_fifo_free\0")?,
            av_audio_fifo_write: load_symbol(&avutil, b"av_audio_fifo_write\0")?,
            av_audio_fifo_read: load_symbol(&avutil, b"av_audio_fifo_read\0")?,
            av_audio_fifo_size: load_symbol(&avutil, b"av_audio_fifo_size\0")?,
            av_log_set_level: load_symbol(&avutil, b"av_log_set_level\0")?,
            swr_alloc: load_symbol(&swresample, b"swr_alloc\0")?,
            swr_free: load_symbol(&swresample, b"swr_free\0")?,
            swr_convert_frame: load_symbol(&swresample, b"swr_convert_frame\0")?,
            _avutil: avutil,
            _swresample: swresample,
            _avcodec: avcodec,
            _avformat: avformat,
            versions,
            abi,
        };
        // SAFETY: av_log_set_level(AV_LOG_QUIET = -8) silences noisy FFmpeg stderr logs.
        unsafe { (api.av_log_set_level)(-8) };
        Ok(api)
    }

    pub fn load() -> Result<Self, String> {
        let custom = std::env::var_os("SYNC2GETHER_FFMPEG_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        Self::load_from_dir(custom.as_deref())
    }

    pub fn codec_name(&self, codec_id: c_int) -> String {
        // SAFETY: avcodec_get_name always returns a valid static C string.
        let ptr = unsafe { (self.avcodec_get_name)(codec_id) };
        if ptr.is_null() {
            "unknown".into()
        } else {
            // SAFETY: ptr is non-null and NUL-terminated.
            unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }

    pub(crate) unsafe fn dict_string(&self, dict: *const c_void, key: &CStr) -> Option<String> {
        if dict.is_null() {
            return None;
        }
        // SAFETY: dict is a valid AVDictionary pointer and key is NUL-terminated.
        let entry = unsafe { (self.av_dict_get)(dict, key.as_ptr(), std::ptr::null(), 0) };
        if entry.is_null() {
            return None;
        }
        // SAFETY: entry is non-null and its value field is a valid C string when non-null.
        let val = unsafe { (*entry).value };
        if val.is_null() {
            return None;
        }
        // SAFETY: val is non-null and NUL-terminated.
        let s = unsafe { CStr::from_ptr(val).to_string_lossy().trim().to_owned() };
        (!s.is_empty()).then_some(s)
    }

    pub fn path_cstring(path: &Path) -> Result<CString, String> {
        let text = path
            .to_str()
            .ok_or_else(|| "Caminho do arquivo não é UTF-8 válido.".to_owned())?;
        CString::new(text).map_err(|_| "Caminho do arquivo contém byte nulo.".to_owned())
    }
}

static GLOBAL: OnceLock<Result<Ffmpeg, String>> = OnceLock::new();

pub fn global() -> Result<&'static Ffmpeg, String> {
    if std::env::var_os("SYNC2GETHER_FFMPEG_DIR").is_some() {
        return Err("SYNC2GETHER_FFMPEG_DIR ativo; use Ffmpeg::load()".into());
    }
    GLOBAL
        .get_or_init(Ffmpeg::load)
        .as_ref()
        .map_err(Clone::clone)
}

pub fn is_available() -> bool {
    if let Some(custom) = std::env::var_os("SYNC2GETHER_FFMPEG_DIR").filter(|v| !v.is_empty()) {
        return Ffmpeg::load_from_dir(Some(Path::new(&custom))).is_ok();
    }
    global().is_ok()
}

pub(crate) unsafe fn read_at<T: Copy>(base: *const c_void, offset: usize) -> T {
    // SAFETY: Caller guarantees base + offset is valid and aligned for T.
    unsafe { std::ptr::read(base.cast::<u8>().add(offset).cast::<T>()) }
}

pub(crate) unsafe fn write_at<T: Copy>(base: *mut c_void, offset: usize, value: T) {
    // SAFETY: Caller guarantees base + offset is valid and aligned for T.
    unsafe { std::ptr::write(base.cast::<u8>().add(offset).cast::<T>(), value) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_ffmpeg_directory_fails_cleanly() {
        let err = Ffmpeg::load_from_dir(Some(Path::new("/nonexistent/sync2gether-ffmpeg")))
            .err()
            .expect("diretório inexistente deve falhar");
        assert!(err.contains("indisponível"));
    }

    #[test]
    fn abi_table_accepts_only_verified_exact_major_sets() {
        let v6 = AbiOffsets::for_versions(60, 60, 58, 4).unwrap();
        let v7 = AbiOffsets::for_versions(61, 61, 59, 5).unwrap();
        let v8 = AbiOffsets::for_versions(62, 62, 60, 6).unwrap();
        let v9 = AbiOffsets::for_versions(63, 63, 61, 7).unwrap();
        assert_eq!((v6.frame_sample_rate, v6.frame_ch_layout), (208, 448));
        assert_eq!((v7.frame_sample_rate, v7.frame_ch_layout), (192, 408));
        assert_eq!((v8.frame_sample_rate, v8.frame_ch_layout), (180, 384));
        assert_eq!(v8, v9);
        assert!(AbiOffsets::for_versions(59, 59, 57, 3).is_err());
        assert!(AbiOffsets::for_versions(64, 64, 62, 8).is_err());
        assert!(AbiOffsets::for_versions(60, 61, 58, 4).is_err());
        assert!(AbiOffsets::for_versions(61, 61, 58, 5).is_err());
        assert!(AbiOffsets::for_versions(62, 62, 60, 5).is_err());
        assert!(AbiOffsets::for_versions(63, 62, 61, 7).is_err());
    }
}
