//! Non-blocking seek-bar keyframe thumbnail extractor via runtime-loaded Homebrew FFmpeg
//! (ADR 0011). For streams (`ByteSource`), reads strictly through `try_read_at` so hovering
//! never triggers network or torrent piece requests.

use std::collections::{HashMap, VecDeque};
use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use cinma_blobstore::ByteSource;

const EXPECTED_AVFORMAT_MAJOR: u32 = 63;
const EXPECTED_AVCODEC_MAJOR: u32 = 63;
const EXPECTED_SWSCALE_MAJOR: u32 = 10;

// Verified against Homebrew FFmpeg (libavformat 63, libavcodec 63, libswscale 10, arm64).
const AVMEDIA_TYPE_VIDEO: c_int = 0;
const AV_PIX_FMT_RGBA: c_int = 26;
const AVSEEK_FLAG_BACKWARD: c_int = 1;
const AVSEEK_SIZE: c_int = 0x10000;
const AVDISCARD_NONKEY: c_int = 32;
const SWS_BILINEAR: c_int = 2;
const AVFMT_FLAG_CUSTOM_IO: c_int = 0x80;
const AVERROR_EOF: c_int = -541_478_725;
const AVERROR_EAGAIN: c_int = -35;
const SEEK_SET: c_int = 0;
const SEEK_CUR: c_int = 1;
const SEEK_END: c_int = 2;

const OFF_FMT_PB: usize = 32;
const OFF_FMT_NB_STREAMS: usize = 44;
const OFF_FMT_STREAMS: usize = 48;
const OFF_FMT_DURATION: usize = 104;
const OFF_FMT_FLAGS: usize = 128;

const OFF_STREAM_CODECPAR: usize = 16;
const OFF_STREAM_TIME_BASE: usize = 32;

const OFF_CODECPAR_CODEC_ID: usize = 4;
const OFF_CODECPAR_WIDTH: usize = 72;
const OFF_CODECPAR_HEIGHT: usize = 76;

const OFF_CODECCTX_THREAD_COUNT: usize = 656;
const OFF_CODECCTX_SKIP_FRAME: usize = 704;

const OFF_PKT_STREAM_INDEX: usize = 36;

const OFF_FRAME_DATA: usize = 0;
const OFF_FRAME_LINESIZE: usize = 64;
const OFF_FRAME_WIDTH: usize = 104;
const OFF_FRAME_HEIGHT: usize = 108;
const OFF_FRAME_FORMAT: usize = 116;

const AVIO_BUF_SIZE: usize = 32 * 1024;
const TARGET_THUMB_WIDTH: i32 = 160;
const MAX_CACHE_ENTRIES: usize = 96;

#[derive(Debug, Clone)]
pub struct ThumbnailImage {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8 (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AVRational {
    num: c_int,
    den: c_int,
}

type ReadPacketFn =
    unsafe extern "C" fn(opaque: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int;
type SeekFn = unsafe extern "C" fn(opaque: *mut c_void, offset: i64, whence: c_int) -> i64;

struct FfmpegApi {
    _util: libloading::Library,
    _codec: libloading::Library,
    _format: libloading::Library,
    _swscale: libloading::Library,

    av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    av_frame_unref: unsafe extern "C" fn(*mut c_void),

    avcodec_find_decoder: unsafe extern "C" fn(c_int) -> *const c_void,
    avcodec_alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    avcodec_free_context: unsafe extern "C" fn(*mut *mut c_void),
    avcodec_parameters_to_context: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    avcodec_open2: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
    avcodec_flush_buffers: unsafe extern "C" fn(*mut c_void),
    avcodec_send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    avcodec_receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    av_packet_unref: unsafe extern "C" fn(*mut c_void),

    avformat_alloc_context: unsafe extern "C" fn() -> *mut c_void,
    avformat_open_input: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_char,
        *const c_void,
        *mut *mut c_void,
    ) -> c_int,
    avformat_close_input: unsafe extern "C" fn(*mut *mut c_void),
    avformat_find_stream_info: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    av_find_best_stream:
        unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, *mut *const c_void, c_int) -> c_int,
    av_seek_frame: unsafe extern "C" fn(*mut c_void, c_int, i64, c_int) -> c_int,
    av_read_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    avio_alloc_context: unsafe extern "C" fn(
        *mut u8,
        c_int,
        c_int,
        *mut c_void,
        Option<ReadPacketFn>,
        *mut c_void,
        Option<SeekFn>,
    ) -> *mut c_void,
    avio_context_free: unsafe extern "C" fn(*mut *mut c_void),

    sws_get_context: unsafe extern "C" fn(
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *mut c_void,
        *mut c_void,
        *const f64,
    ) -> *mut c_void,
    sws_scale: unsafe extern "C" fn(
        *mut c_void,
        *const *const u8,
        *const c_int,
        c_int,
        c_int,
        *const *mut u8,
        *const c_int,
    ) -> c_int,
    sws_free_context: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: loaded dynamic symbols are stateless thread-safe C entry points;
// per-decoder contexts are confined to a single `Mutex<Session>` instance.
unsafe impl Send for FfmpegApi {}
unsafe impl Sync for FfmpegApi {}

static FFMPEG: OnceLock<Option<FfmpegApi>> = OnceLock::new();

fn api() -> Option<&'static FfmpegApi> {
    FFMPEG.get_or_init(load_ffmpeg).as_ref()
}

fn load_ffmpeg() -> Option<FfmpegApi> {
    let mut dirs: Vec<String> = Vec::new();
    if let Ok(env_dir) = std::env::var("CINMA_FFMPEG_LIB")
        && !env_dir.trim().is_empty()
    {
        dirs.push(env_dir);
    }
    dirs.extend([
        "/opt/homebrew/opt/ffmpeg/lib".to_owned(),
        "/opt/homebrew/lib".to_owned(),
        "/usr/local/lib".to_owned(),
    ]);

    for dir in dirs {
        let base = Path::new(&dir);
        if let Some(loaded) = try_load_from_dir(base) {
            return Some(loaded);
        }
    }
    None
}

fn try_load_from_dir(dir: &Path) -> Option<FfmpegApi> {
    // SAFETY: dlopen standard Homebrew FFmpeg libraries and verify major ABI.
    unsafe {
        let util = libloading::Library::new(dir.join("libavutil.61.dylib")).ok()?;
        let codec = libloading::Library::new(dir.join("libavcodec.63.dylib")).ok()?;
        let format = libloading::Library::new(dir.join("libavformat.63.dylib")).ok()?;
        let swscale = libloading::Library::new(dir.join("libswscale.10.dylib")).ok()?;

        let avcodec_version: unsafe extern "C" fn() -> c_uint =
            *codec.get(b"avcodec_version\0").ok()?;
        let avformat_version: unsafe extern "C" fn() -> c_uint =
            *format.get(b"avformat_version\0").ok()?;
        let swscale_version: unsafe extern "C" fn() -> c_uint =
            *swscale.get(b"swscale_version\0").ok()?;

        if (avcodec_version() >> 16) != EXPECTED_AVCODEC_MAJOR
            || (avformat_version() >> 16) != EXPECTED_AVFORMAT_MAJOR
            || (swscale_version() >> 16) != EXPECTED_SWSCALE_MAJOR
        {
            return None;
        }

        Some(FfmpegApi {
            av_malloc: *util.get(b"av_malloc\0").ok()?,
            av_frame_alloc: *util.get(b"av_frame_alloc\0").ok()?,
            av_frame_free: *util.get(b"av_frame_free\0").ok()?,
            av_frame_unref: *util.get(b"av_frame_unref\0").ok()?,

            avcodec_find_decoder: *codec.get(b"avcodec_find_decoder\0").ok()?,
            avcodec_alloc_context3: *codec.get(b"avcodec_alloc_context3\0").ok()?,
            avcodec_free_context: *codec.get(b"avcodec_free_context\0").ok()?,
            avcodec_parameters_to_context: *codec.get(b"avcodec_parameters_to_context\0").ok()?,
            avcodec_open2: *codec.get(b"avcodec_open2\0").ok()?,
            avcodec_flush_buffers: *codec.get(b"avcodec_flush_buffers\0").ok()?,
            avcodec_send_packet: *codec.get(b"avcodec_send_packet\0").ok()?,
            avcodec_receive_frame: *codec.get(b"avcodec_receive_frame\0").ok()?,
            av_packet_alloc: *codec.get(b"av_packet_alloc\0").ok()?,
            av_packet_free: *codec.get(b"av_packet_free\0").ok()?,
            av_packet_unref: *codec.get(b"av_packet_unref\0").ok()?,

            avformat_alloc_context: *format.get(b"avformat_alloc_context\0").ok()?,
            avformat_open_input: *format.get(b"avformat_open_input\0").ok()?,
            avformat_close_input: *format.get(b"avformat_close_input\0").ok()?,
            avformat_find_stream_info: *format.get(b"avformat_find_stream_info\0").ok()?,
            av_find_best_stream: *format.get(b"av_find_best_stream\0").ok()?,
            av_seek_frame: *format.get(b"av_seek_frame\0").ok()?,
            av_read_frame: *format.get(b"av_read_frame\0").ok()?,
            avio_alloc_context: *format.get(b"avio_alloc_context\0").ok()?,
            avio_context_free: *format.get(b"avio_context_free\0").ok()?,

            sws_get_context: *swscale.get(b"sws_getContext\0").ok()?,
            sws_scale: *swscale.get(b"sws_scale\0").ok()?,
            sws_free_context: *swscale.get(b"sws_freeContext\0").ok()?,

            _util: util,
            _codec: codec,
            _format: format,
            _swscale: swscale,
        })
    }
}

/// Non-blocking wrapper over a local file so local files and streams share one
/// decode path.
pub struct LocalFileSource {
    file: File,
    len: u64,
    name: String,
}

impl LocalFileSource {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("local")
            .to_owned();
        Ok(Self { file, len, name })
    }
}

impl ByteSource for LocalFileSource {
    fn size(&self) -> u64 {
        self.len
    }

    fn open_reader(&self) -> std::io::Result<Box<dyn cinma_blobstore::SourceReader>> {
        Err(std::io::Error::other(
            "LocalFileSource only supports non-blocking reads",
        ))
    }

    fn describe(&self) -> String {
        format!("file:{}", self.name)
    }

    fn filename(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn try_read_at(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        if offset >= self.len {
            return Some(0);
        }
        if buf.is_empty() {
            return Some(0);
        }
        let max_n = (self.len - offset).min(buf.len() as u64) as usize;
        self.file.read_at(&mut buf[..max_n], offset).ok()
    }
}

struct AvioState {
    source: Arc<dyn ByteSource>,
    pos: u64,
}

unsafe extern "C" fn avio_read_packet(opaque: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int {
    if opaque.is_null() || buf.is_null() || buf_size <= 0 {
        return AVERROR_EOF;
    }
    // SAFETY: `opaque` points to boxed `AvioState` alive for `AVIOContext`'s lifetime.
    let state = unsafe { &mut *(opaque as *mut AvioState) };
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, buf_size as usize) };
    match state.source.try_read_at(state.pos, slice) {
        Some(0) | None => AVERROR_EOF,
        Some(n) => {
            state.pos = state.pos.saturating_add(n as u64);
            n as c_int
        }
    }
}

unsafe extern "C" fn avio_seek(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    if opaque.is_null() {
        return -1;
    }
    // SAFETY: `opaque` points to boxed `AvioState`.
    let state = unsafe { &mut *(opaque as *mut AvioState) };
    let size = state.source.size() as i64;
    if whence == AVSEEK_SIZE {
        return size;
    }
    let target = match whence & !0x20000 /* AVSEEK_FORCE */ {
        SEEK_SET => offset,
        SEEK_CUR => (state.pos as i64).saturating_add(offset),
        SEEK_END => size.saturating_add(offset),
        _ => return -1,
    };
    if target < 0 || target > size {
        return -1;
    }
    state.pos = target as u64;
    target
}

struct FfmpegSession {
    api: &'static FfmpegApi,
    fmt_ctx: *mut c_void,
    avio_ctx: *mut c_void,
    avio_state: *mut AvioState,
    codec_ctx: *mut c_void,
    pkt: *mut c_void,
    frame: *mut c_void,
    sws_ctx: *mut c_void,
    video_stream_index: c_int,
    stream_time_base: AVRational,
    dst_w: c_int,
    dst_h: c_int,
    last_src_pix_fmt: c_int,
    last_src_w: c_int,
    last_src_h: c_int,
}

// SAFETY: only accessed while holding `ExtractorState::session` mutex.
unsafe impl Send for FfmpegSession {}

impl Drop for FfmpegSession {
    fn drop(&mut self) {
        // SAFETY: pointers are either null or FFmpeg-allocated in `open`.
        unsafe {
            if !self.sws_ctx.is_null() {
                (self.api.sws_free_context)(self.sws_ctx);
            }
            if !self.frame.is_null() {
                (self.api.av_frame_free)(&mut self.frame);
            }
            if !self.pkt.is_null() {
                (self.api.av_packet_free)(&mut self.pkt);
            }
            if !self.codec_ctx.is_null() {
                (self.api.avcodec_free_context)(&mut self.codec_ctx);
            }
            if !self.fmt_ctx.is_null() {
                (self.api.avformat_close_input)(&mut self.fmt_ctx);
            }
            if !self.avio_ctx.is_null() {
                (self.api.avio_context_free)(&mut self.avio_ctx);
            }
            if !self.avio_state.is_null() {
                drop(Box::from_raw(self.avio_state));
            }
        }
    }
}

impl FfmpegSession {
    fn open(source: Arc<dyn ByteSource>) -> Option<Self> {
        let api = api()?;
        unsafe {
            let avio_buf = (api.av_malloc)(AVIO_BUF_SIZE) as *mut u8;
            if avio_buf.is_null() {
                return None;
            }
            let avio_state = Box::into_raw(Box::new(AvioState { source, pos: 0 }));
            let avio_ctx = (api.avio_alloc_context)(
                avio_buf,
                AVIO_BUF_SIZE as c_int,
                0,
                avio_state.cast(),
                Some(avio_read_packet),
                std::ptr::null_mut(),
                Some(avio_seek),
            );
            if avio_ctx.is_null() {
                drop(Box::from_raw(avio_state));
                return None;
            }

            let mut fmt_ctx = (api.avformat_alloc_context)();
            if fmt_ctx.is_null() {
                let mut tmp = avio_ctx;
                (api.avio_context_free)(&mut tmp);
                drop(Box::from_raw(avio_state));
                return None;
            }
            ptr_write(fmt_ctx, OFF_FMT_PB, avio_ctx);
            let existing_flags: c_int = ptr_read(fmt_ctx, OFF_FMT_FLAGS);
            ptr_write(
                fmt_ctx,
                OFF_FMT_FLAGS,
                existing_flags | AVFMT_FLAG_CUSTOM_IO,
            );

            let dummy = CString::new("cinma://thumb").ok()?;
            if (api.avformat_open_input)(
                &mut fmt_ctx,
                dummy.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
            ) < 0
            {
                let mut tmp = avio_ctx;
                (api.avio_context_free)(&mut tmp);
                drop(Box::from_raw(avio_state));
                return None;
            }
            if (api.avformat_find_stream_info)(fmt_ctx, std::ptr::null_mut()) < 0 {
                let mut s = Self::partial(api, fmt_ctx, avio_ctx, avio_state);
                s.video_stream_index = -1;
                drop(s);
                return None;
            }

            let mut decoder_ptr: *const c_void = std::ptr::null();
            let stream_idx =
                (api.av_find_best_stream)(fmt_ctx, AVMEDIA_TYPE_VIDEO, -1, -1, &mut decoder_ptr, 0);
            if stream_idx < 0 {
                drop(Self::partial(api, fmt_ctx, avio_ctx, avio_state));
                return None;
            }

            let nb_streams: c_uint = ptr_read(fmt_ctx, OFF_FMT_NB_STREAMS);
            if (stream_idx as c_uint) >= nb_streams {
                drop(Self::partial(api, fmt_ctx, avio_ctx, avio_state));
                return None;
            }
            let streams: *const *const c_void = ptr_read(fmt_ctx, OFF_FMT_STREAMS);
            let stream = *streams.add(stream_idx as usize);
            let codecpar: *const c_void = ptr_read(stream, OFF_STREAM_CODECPAR);
            let time_base: AVRational = ptr_read(stream, OFF_STREAM_TIME_BASE);
            let src_w: c_int = ptr_read(codecpar, OFF_CODECPAR_WIDTH);
            let src_h: c_int = ptr_read(codecpar, OFF_CODECPAR_HEIGHT);
            if src_w <= 0 || src_h <= 0 || time_base.den <= 0 {
                drop(Self::partial(api, fmt_ctx, avio_ctx, avio_state));
                return None;
            }

            if decoder_ptr.is_null() {
                let codec_id: c_int = ptr_read(codecpar, OFF_CODECPAR_CODEC_ID);
                decoder_ptr = (api.avcodec_find_decoder)(codec_id);
                if decoder_ptr.is_null() {
                    drop(Self::partial(api, fmt_ctx, avio_ctx, avio_state));
                    return None;
                }
            }

            let codec_ctx = (api.avcodec_alloc_context3)(decoder_ptr);
            if codec_ctx.is_null() {
                drop(Self::partial(api, fmt_ctx, avio_ctx, avio_state));
                return None;
            }
            let mut session = Self::partial(api, fmt_ctx, avio_ctx, avio_state);
            session.codec_ctx = codec_ctx;
            session.video_stream_index = stream_idx;
            session.stream_time_base = time_base;

            if (api.avcodec_parameters_to_context)(codec_ctx, codecpar) < 0 {
                return None;
            }
            // Keyframes only on 1 thread for fast hover latency & minimal RAM.
            ptr_write(codec_ctx, OFF_CODECCTX_THREAD_COUNT, 1 as c_int);
            ptr_write(codec_ctx, OFF_CODECCTX_SKIP_FRAME, AVDISCARD_NONKEY);
            if (api.avcodec_open2)(codec_ctx, decoder_ptr, std::ptr::null_mut()) < 0 {
                return None;
            }

            session.pkt = (api.av_packet_alloc)();
            session.frame = (api.av_frame_alloc)();
            if session.pkt.is_null() || session.frame.is_null() {
                return None;
            }

            session.dst_w = TARGET_THUMB_WIDTH.min(src_w.max(1));
            session.dst_h = ((src_h as i64 * session.dst_w as i64) / src_w.max(1) as i64)
                .clamp(16, TARGET_THUMB_WIDTH as i64) as c_int;
            Some(session)
        }
    }

    fn partial(
        api: &'static FfmpegApi,
        fmt_ctx: *mut c_void,
        avio_ctx: *mut c_void,
        avio_state: *mut AvioState,
    ) -> Self {
        Self {
            api,
            fmt_ctx,
            avio_ctx,
            avio_state,
            codec_ctx: std::ptr::null_mut(),
            pkt: std::ptr::null_mut(),
            frame: std::ptr::null_mut(),
            sws_ctx: std::ptr::null_mut(),
            video_stream_index: -1,
            stream_time_base: AVRational { num: 1, den: 1000 },
            dst_w: TARGET_THUMB_WIDTH,
            dst_h: 90,
            last_src_pix_fmt: -1,
            last_src_w: 0,
            last_src_h: 0,
        }
    }

    fn decode_at(&mut self, seconds: f64) -> Option<ThumbnailImage> {
        let tb = self.stream_time_base;
        let ts = ((seconds.max(0.0) * tb.den as f64) / tb.num.max(1) as f64) as i64;
        unsafe {
            if (self.api.av_seek_frame)(
                self.fmt_ctx,
                self.video_stream_index,
                ts,
                AVSEEK_FLAG_BACKWARD,
            ) < 0
            {
                return None;
            }
            (self.api.avcodec_flush_buffers)(self.codec_ctx);

            // Inspect up to 64 packets to catch the first decodable keyframe
            // from already-available bytes, then flush the reorder buffer.
            for _ in 0..64 {
                if (self.api.av_read_frame)(self.fmt_ctx, self.pkt) < 0 {
                    break;
                }
                let pkt_stream: c_int = ptr_read(self.pkt, OFF_PKT_STREAM_INDEX);
                if pkt_stream != self.video_stream_index {
                    (self.api.av_packet_unref)(self.pkt);
                    continue;
                }
                let send_rc = (self.api.avcodec_send_packet)(self.codec_ctx, self.pkt);
                (self.api.av_packet_unref)(self.pkt);
                if send_rc < 0 && send_rc != AVERROR_EAGAIN {
                    continue;
                }
                let recv_rc = (self.api.avcodec_receive_frame)(self.codec_ctx, self.frame);
                if recv_rc == 0 {
                    let thumb = self.scale_frame_to_rgba();
                    (self.api.av_frame_unref)(self.frame);
                    return thumb;
                }
            }
            if (self.api.avcodec_send_packet)(self.codec_ctx, std::ptr::null()) >= 0
                && (self.api.avcodec_receive_frame)(self.codec_ctx, self.frame) == 0
            {
                let thumb = self.scale_frame_to_rgba();
                (self.api.av_frame_unref)(self.frame);
                return thumb;
            }
            None
        }
    }

    unsafe fn scale_frame_to_rgba(&mut self) -> Option<ThumbnailImage> {
        unsafe {
            let src_w: c_int = ptr_read(self.frame, OFF_FRAME_WIDTH);
            let src_h: c_int = ptr_read(self.frame, OFF_FRAME_HEIGHT);
            let pix_fmt: c_int = ptr_read(self.frame, OFF_FRAME_FORMAT);
            if src_w <= 0 || src_h <= 0 || pix_fmt < 0 {
                return None;
            }
            if self.sws_ctx.is_null()
                || self.last_src_w != src_w
                || self.last_src_h != src_h
                || self.last_src_pix_fmt != pix_fmt
            {
                if !self.sws_ctx.is_null() {
                    (self.api.sws_free_context)(self.sws_ctx);
                    self.sws_ctx = std::ptr::null_mut();
                }
                self.dst_w = TARGET_THUMB_WIDTH.min(src_w);
                self.dst_h =
                    ((src_h as i64 * self.dst_w as i64) / src_w as i64).clamp(16, 160) as c_int;
                self.sws_ctx = (self.api.sws_get_context)(
                    src_w,
                    src_h,
                    pix_fmt,
                    self.dst_w,
                    self.dst_h,
                    AV_PIX_FMT_RGBA,
                    SWS_BILINEAR,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                if self.sws_ctx.is_null() {
                    return None;
                }
                self.last_src_w = src_w;
                self.last_src_h = src_h;
                self.last_src_pix_fmt = pix_fmt;
            }

            let src_data: [*const u8; 8] = ptr_read(self.frame, OFF_FRAME_DATA);
            let src_linesize: [c_int; 8] = ptr_read(self.frame, OFF_FRAME_LINESIZE);
            let dst_stride = self.dst_w * 4;
            let mut rgba = vec![0u8; (dst_stride * self.dst_h) as usize];
            let dst_ptrs: [*mut u8; 4] = [
                rgba.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ];
            let dst_strides: [c_int; 4] = [dst_stride, 0, 0, 0];
            let out_h = (self.api.sws_scale)(
                self.sws_ctx,
                src_data.as_ptr(),
                src_linesize.as_ptr(),
                0,
                src_h,
                dst_ptrs.as_ptr(),
                dst_strides.as_ptr(),
            );
            if out_h <= 0 {
                return None;
            }
            Some(ThumbnailImage {
                width: self.dst_w as u32,
                height: self.dst_h as u32,
                rgba,
            })
        }
    }
}

#[derive(Default)]
struct ThumbCache {
    map: HashMap<i64, ThumbnailImage>,
    lru: VecDeque<i64>,
}

impl ThumbCache {
    fn get(&mut self, bucket: i64) -> Option<ThumbnailImage> {
        let img = self.map.get(&bucket)?.clone();
        self.lru.retain(|&b| b != bucket);
        self.lru.push_back(bucket);
        Some(img)
    }

    fn insert(&mut self, bucket: i64, img: ThumbnailImage) {
        if self.map.contains_key(&bucket) {
            self.lru.retain(|&b| b != bucket);
        } else if self.map.len() >= MAX_CACHE_ENTRIES
            && let Some(evict) = self.lru.pop_front()
        {
            self.map.remove(&evict);
        }
        self.lru.push_back(bucket);
        self.map.insert(bucket, img);
    }
}

pub struct ThumbnailExtractor {
    source: Arc<dyn ByteSource>,
    session: Mutex<Option<FfmpegSession>>,
    cache: Mutex<ThumbCache>,
}

impl ThumbnailExtractor {
    pub fn from_source(source: Arc<dyn ByteSource>) -> Self {
        Self {
            source,
            session: Mutex::new(None),
            cache: Mutex::new(ThumbCache::default()),
        }
    }

    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let source = Arc::new(LocalFileSource::open(path)?);
        Ok(Self::from_source(source))
    }

    pub fn ffmpeg_available() -> bool {
        api().is_some()
    }

    /// Decodes a ~160 px RGBA keyframe thumbnail around `seconds` from
    /// already-available bytes only. Uses 0.5 s bucketed LRU caching.
    pub fn thumbnail_at(&self, seconds: f64) -> Option<ThumbnailImage> {
        if !seconds.is_finite() || seconds < 0.0 {
            return None;
        }
        let bucket = (seconds * 2.0).round() as i64;
        if let Ok(mut cache) = self.cache.lock()
            && let Some(hit) = cache.get(bucket)
        {
            return Some(hit);
        }

        let mut guard = self.session.lock().ok()?;
        if guard.is_none() {
            *guard = FfmpegSession::open(Arc::clone(&self.source));
        }
        let session = guard.as_mut()?;
        let decoded = session.decode_at(bucket as f64 / 2.0)?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(bucket, decoded.clone());
        }
        Some(decoded)
    }
}

#[allow(dead_code)]
fn _duration_unused(_: c_void) -> usize {
    OFF_FMT_DURATION
}

#[inline]
unsafe fn ptr_read<T: Copy>(base: *const c_void, offset: usize) -> T {
    // SAFETY: offsets are compile/startup-verified against matching FFmpeg major ABI.
    unsafe { std::ptr::read_unaligned((base as *const u8).add(offset) as *const T) }
}

#[inline]
unsafe fn ptr_write<T>(base: *mut c_void, offset: usize, value: T) {
    // SAFETY: offsets are verified against matching FFmpeg major ABI.
    unsafe { std::ptr::write_unaligned((base as *mut u8).add(offset) as *mut T, value) }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use cinma_blobstore::MemorySource;
    use std::time::Instant;

    #[test]
    fn empty_stream_thumbnail_returns_none_without_blocking() {
        let source = Arc::new(MemorySource::new(vec![0u8; 4096], 0));
        let extractor = ThumbnailExtractor::from_source(source);
        let started = Instant::now();
        assert!(extractor.thumbnail_at(1.0).is_none());
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn decodes_local_corpus_keyframe_if_present() {
        let sample = Path::new("/Users/furkanturkoglu/Movies/cinma-corpus/sdr_h264_1080p.mkv");
        if !sample.exists() || !ThumbnailExtractor::ffmpeg_available() {
            return;
        }
        let extractor = ThumbnailExtractor::from_file(sample).unwrap();
        let started = Instant::now();
        let thumb = extractor.thumbnail_at(2.0).expect("decoded thumbnail");
        assert!(thumb.width > 0 && thumb.width <= 160);
        assert!(thumb.height > 0);
        assert_eq!(thumb.rgba.len(), (thumb.width * thumb.height * 4) as usize);
        let cold_ms = started.elapsed().as_millis();
        assert!(cold_ms < 500, "cold thumb took {cold_ms} ms");

        let hit_start = Instant::now();
        let hit = extractor.thumbnail_at(2.0).expect("cached hit");
        assert_eq!(hit.rgba.len(), thumb.rgba.len());
        assert!(hit_start.elapsed() < std::time::Duration::from_millis(5));
    }
}
