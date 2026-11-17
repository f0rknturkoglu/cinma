//! In-process container muxer using runtime-loaded Homebrew `libavformat.63` & `libavcodec.63`
//! (used by `yt-dlp` jobs to combine separate video + audio tracks without spawning ffmpeg,
//! ADR 0012).

use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::path::Path;
use std::sync::OnceLock;

const EXPECTED_AVFORMAT_MAJOR: u32 = 63;
const EXPECTED_AVCODEC_MAJOR: u32 = 63;
const OFF_FMT_OFORMAT: usize = 16;
const OFF_FMT_PB: usize = 32;
const OFF_FMT_NB_STREAMS: usize = 44;
const OFF_FMT_STREAMS: usize = 48;
const OFF_OFORMAT_FLAGS: usize = 44;
const OFF_STREAM_CODECPAR: usize = 16;
const OFF_STREAM_TIME_BASE: usize = 32;
const OFF_PKT_STREAM_INDEX: usize = 36;
const AVFMT_NOFILE: c_int = 0x1;
const AVIO_FLAG_WRITE: c_int = 0x2;

#[repr(C)]
#[derive(Clone, Copy)]
struct AVRational {
    num: c_int,
    den: c_int,
}

#[derive(Debug, thiserror::Error)]
pub enum MuxError {
    #[error("FFmpeg libraries (libavformat.63 / libavcodec.63) unavailable")]
    Unavailable,
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("FFmpeg error {code} during {step}")]
    Ffmpeg { step: &'static str, code: c_int },
}

struct MuxApi {
    _codec: libloading::Library,
    _format: libloading::Library,

    avcodec_parameters_copy: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    av_packet_unref: unsafe extern "C" fn(*mut c_void),
    av_packet_rescale_ts: unsafe extern "C" fn(*mut c_void, AVRational, AVRational),

    avformat_open_input: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_char,
        *const c_void,
        *mut *mut c_void,
    ) -> c_int,
    avformat_close_input: unsafe extern "C" fn(*mut *mut c_void),
    avformat_find_stream_info: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    avformat_alloc_output_context2: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_void,
        *const c_char,
        *const c_char,
    ) -> c_int,
    avformat_free_context: unsafe extern "C" fn(*mut c_void),
    avformat_new_stream: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void,
    avio_open: unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int) -> c_int,
    avio_closep: unsafe extern "C" fn(*mut *mut c_void) -> c_int,
    avformat_write_header: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    av_interleaved_write_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_write_trailer: unsafe extern "C" fn(*mut c_void) -> c_int,
    av_read_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
}

unsafe impl Send for MuxApi {}
unsafe impl Sync for MuxApi {}

static MUX_API: OnceLock<Option<MuxApi>> = OnceLock::new();

fn api() -> Option<&'static MuxApi> {
    MUX_API.get_or_init(load_mux_api).as_ref()
}

fn load_mux_api() -> Option<MuxApi> {
    let mut dirs = Vec::new();
    if let Ok(d) = std::env::var("CINMA_FFMPEG_LIB")
        && !d.trim().is_empty()
    {
        dirs.push(d);
    }
    dirs.extend([
        "/opt/homebrew/opt/ffmpeg/lib".to_owned(),
        "/opt/homebrew/lib".to_owned(),
        "/usr/local/lib".to_owned(),
    ]);
    for dir in dirs {
        let base = Path::new(&dir);
        if let Some(m) = try_load(base) {
            return Some(m);
        }
    }
    None
}

fn try_load(dir: &Path) -> Option<MuxApi> {
    unsafe {
        let codec = libloading::Library::new(dir.join("libavcodec.63.dylib")).ok()?;
        let format = libloading::Library::new(dir.join("libavformat.63.dylib")).ok()?;
        let avcodec_ver: unsafe extern "C" fn() -> c_uint =
            *codec.get(b"avcodec_version\0").ok()?;
        let avformat_ver: unsafe extern "C" fn() -> c_uint =
            *format.get(b"avformat_version\0").ok()?;
        if (avcodec_ver() >> 16) != EXPECTED_AVCODEC_MAJOR
            || (avformat_ver() >> 16) != EXPECTED_AVFORMAT_MAJOR
        {
            return None;
        }
        Some(MuxApi {
            avcodec_parameters_copy: *codec.get(b"avcodec_parameters_copy\0").ok()?,
            av_packet_alloc: *codec.get(b"av_packet_alloc\0").ok()?,
            av_packet_free: *codec.get(b"av_packet_free\0").ok()?,
            av_packet_unref: *codec.get(b"av_packet_unref\0").ok()?,
            av_packet_rescale_ts: *codec.get(b"av_packet_rescale_ts\0").ok()?,

            avformat_open_input: *format.get(b"avformat_open_input\0").ok()?,
            avformat_close_input: *format.get(b"avformat_close_input\0").ok()?,
            avformat_find_stream_info: *format.get(b"avformat_find_stream_info\0").ok()?,
            avformat_alloc_output_context2: *format
                .get(b"avformat_alloc_output_context2\0")
                .ok()?,
            avformat_free_context: *format.get(b"avformat_free_context\0").ok()?,
            avformat_new_stream: *format.get(b"avformat_new_stream\0").ok()?,
            avio_open: *format.get(b"avio_open\0").ok()?,
            avio_closep: *format.get(b"avio_closep\0").ok()?,
            avformat_write_header: *format.get(b"avformat_write_header\0").ok()?,
            av_interleaved_write_frame: *format.get(b"av_interleaved_write_frame\0").ok()?,
            av_write_trailer: *format.get(b"av_write_trailer\0").ok()?,
            av_read_frame: *format.get(b"av_read_frame\0").ok()?,

            _codec: codec,
            _format: format,
        })
    }
}

pub fn mux_available() -> bool {
    api().is_some()
}

/// Remuxes streams from `inputs` (e.g. separate video + audio) into `output`
/// without transcoding. Prefers Matroska (`.mkv`) when not `.mp4` for universal
/// codec compatibility.
pub fn remux_files(inputs: &[&Path], output: &Path) -> Result<(), MuxError> {
    let api = api().ok_or(MuxError::Unavailable)?;
    if inputs.is_empty() {
        return Err(MuxError::InvalidPath("no input files".into()));
    }
    let out_str = output
        .to_str()
        .ok_or_else(|| MuxError::InvalidPath(output.display().to_string()))?;
    let c_out = CString::new(out_str).map_err(|_| MuxError::InvalidPath(out_str.to_owned()))?;
    let c_matroska = CString::new("matroska").ok();
    let format_hint = if output
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mkv"))
    {
        c_matroska.as_ref().map_or(std::ptr::null(), |s| s.as_ptr())
    } else {
        std::ptr::null()
    };

    unsafe {
        let mut out_ctx: *mut c_void = std::ptr::null_mut();
        let rc = (api.avformat_alloc_output_context2)(
            &mut out_ctx,
            std::ptr::null(),
            format_hint,
            c_out.as_ptr(),
        );
        if rc < 0 || out_ctx.is_null() {
            return Err(MuxError::Ffmpeg {
                step: "alloc_output_context2",
                code: rc,
            });
        }

        let res = remux_inner(api, out_ctx, &c_out, inputs);
        let oformat: *const c_void = ptr_read(out_ctx, OFF_FMT_OFORMAT);
        if !oformat.is_null() {
            let flags: c_int = ptr_read(oformat, OFF_OFORMAT_FLAGS);
            if (flags & AVFMT_NOFILE) == 0 {
                let mut pb: *mut c_void = ptr_read(out_ctx, OFF_FMT_PB);
                if !pb.is_null() {
                    (api.avio_closep)(&mut pb);
                    ptr_write(out_ctx, OFF_FMT_PB, pb);
                }
            }
        }
        (api.avformat_free_context)(out_ctx);
        res
    }
}

unsafe fn remux_inner(
    api: &MuxApi,
    out_ctx: *mut c_void,
    c_out: &CString,
    inputs: &[&Path],
) -> Result<(), MuxError> {
    unsafe {
        let mut in_ctxs: Vec<*mut c_void> = Vec::with_capacity(inputs.len());
        let mut stream_maps: Vec<Vec<(c_int, AVRational, AVRational)>> =
            Vec::with_capacity(inputs.len());

        let cleanup = |ctxs: &mut Vec<*mut c_void>| {
            for ctx in ctxs.iter_mut() {
                if !ctx.is_null() {
                    (api.avformat_close_input)(ctx);
                }
            }
        };

        for path in inputs {
            let Some(p_str) = path.to_str() else {
                cleanup(&mut in_ctxs);
                return Err(MuxError::InvalidPath(path.display().to_string()));
            };
            let Ok(c_in) = CString::new(p_str) else {
                cleanup(&mut in_ctxs);
                return Err(MuxError::InvalidPath(p_str.to_owned()));
            };
            let mut in_ctx: *mut c_void = std::ptr::null_mut();
            let rc = (api.avformat_open_input)(
                &mut in_ctx,
                c_in.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            if rc < 0 {
                cleanup(&mut in_ctxs);
                return Err(MuxError::Ffmpeg {
                    step: "open_input",
                    code: rc,
                });
            }
            in_ctxs.push(in_ctx);
            let rc = (api.avformat_find_stream_info)(in_ctx, std::ptr::null_mut());
            if rc < 0 {
                cleanup(&mut in_ctxs);
                return Err(MuxError::Ffmpeg {
                    step: "find_stream_info",
                    code: rc,
                });
            }

            let nb: c_uint = ptr_read(in_ctx, OFF_FMT_NB_STREAMS);
            let streams: *const *const c_void = ptr_read(in_ctx, OFF_FMT_STREAMS);
            let mut map = Vec::with_capacity(nb as usize);
            for i in 0..nb as usize {
                let in_st = *streams.add(i);
                let in_par: *const c_void = ptr_read(in_st, OFF_STREAM_CODECPAR);
                let in_tb: AVRational = ptr_read(in_st, OFF_STREAM_TIME_BASE);
                let out_st = (api.avformat_new_stream)(out_ctx, std::ptr::null());
                if out_st.is_null() {
                    cleanup(&mut in_ctxs);
                    return Err(MuxError::Ffmpeg {
                        step: "new_stream",
                        code: -12,
                    });
                }
                let out_par: *mut c_void = ptr_read(out_st, OFF_STREAM_CODECPAR);
                let rc = (api.avcodec_parameters_copy)(out_par, in_par);
                if rc < 0 {
                    cleanup(&mut in_ctxs);
                    return Err(MuxError::Ffmpeg {
                        step: "parameters_copy",
                        code: rc,
                    });
                }
                // Clear codec_tag at offset 8 inside AVCodecParameters? Keeping out_st index safe:
                let out_tb: AVRational = ptr_read(out_st, OFF_STREAM_TIME_BASE);
                let out_idx = ptr_read::<c_uint>(out_ctx, OFF_FMT_NB_STREAMS) as c_int - 1;
                map.push((out_idx, in_tb, out_tb));
            }
            stream_maps.push(map);
        }

        let oformat: *const c_void = ptr_read(out_ctx, OFF_FMT_OFORMAT);
        let flags: c_int = ptr_read(oformat, OFF_OFORMAT_FLAGS);
        if (flags & AVFMT_NOFILE) == 0 {
            let mut pb: *mut c_void = ptr_read(out_ctx, OFF_FMT_PB);
            let rc = (api.avio_open)(&mut pb, c_out.as_ptr(), AVIO_FLAG_WRITE);
            ptr_write(out_ctx, OFF_FMT_PB, pb);
            if rc < 0 {
                cleanup(&mut in_ctxs);
                return Err(MuxError::Ffmpeg {
                    step: "avio_open",
                    code: rc,
                });
            }
        }

        let rc = (api.avformat_write_header)(out_ctx, std::ptr::null_mut());
        if rc < 0 {
            cleanup(&mut in_ctxs);
            return Err(MuxError::Ffmpeg {
                step: "write_header",
                code: rc,
            });
        }

        let mut pkt = (api.av_packet_alloc)();
        if pkt.is_null() {
            cleanup(&mut in_ctxs);
            return Err(MuxError::Ffmpeg {
                step: "packet_alloc",
                code: -12,
            });
        }

        // Refresh output stream timebases after `write_header` (muxer may adjust them).
        let out_streams: *const *const c_void = ptr_read(out_ctx, OFF_FMT_STREAMS);
        for (input_idx, &in_ctx) in in_ctxs.iter().enumerate() {
            while (api.av_read_frame)(in_ctx, pkt) >= 0 {
                let in_idx: c_int = ptr_read(pkt, OFF_PKT_STREAM_INDEX);
                if let Some(&(out_idx, in_tb, _)) = stream_maps[input_idx].get(in_idx as usize) {
                    let out_st = *out_streams.add(out_idx as usize);
                    let out_tb: AVRational = ptr_read(out_st, OFF_STREAM_TIME_BASE);
                    (api.av_packet_rescale_ts)(pkt, in_tb, out_tb);
                    ptr_write(pkt, OFF_PKT_STREAM_INDEX, out_idx);
                    let _ = (api.av_interleaved_write_frame)(out_ctx, pkt);
                }
                (api.av_packet_unref)(pkt);
            }
        }
        (api.av_packet_free)(&mut pkt);
        let rc = (api.av_write_trailer)(out_ctx);
        cleanup(&mut in_ctxs);
        if rc < 0 {
            return Err(MuxError::Ffmpeg {
                step: "write_trailer",
                code: rc,
            });
        }
        Ok(())
    }
}

#[inline]
unsafe fn ptr_read<T: Copy>(base: *const c_void, offset: usize) -> T {
    unsafe { std::ptr::read_unaligned((base as *const u8).add(offset) as *const T) }
}

#[inline]
unsafe fn ptr_write<T>(base: *mut c_void, offset: usize, value: T) {
    unsafe { std::ptr::write_unaligned((base as *mut u8).add(offset) as *mut T, value) }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ThumbnailExtractor;

    #[test]
    fn remuxes_local_corpus_if_present() {
        let sample = Path::new("/Users/furkanturkoglu/Movies/cinma-corpus/sdr_h264_1080p.mkv");
        if !sample.exists() || !mux_available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("cinma-mux-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("remuxed.mkv");
        remux_files(&[sample], &out).unwrap();
        let meta = std::fs::metadata(&out).unwrap();
        assert!(meta.len() > 100_000);
        let thumb = ThumbnailExtractor::from_file(&out)
            .unwrap()
            .thumbnail_at(1.0);
        assert!(thumb.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
