//! libmpv glue: lazy-loaded libmpv, `cinma://` byte-stream protocol, and a
//! small event-driven player handle. Rendering uses the libmpv render API
//! into the host's `CAOpenGLLayer` (docs/adr/0004).

pub mod mux;
mod render;
mod stream;
mod sys;
pub mod thumbnails;

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cinma_blobstore::ByteSource;

pub use render::UpdateCallback;
pub use stream::PROTOCOL;
pub use thumbnails::{ThumbnailExtractor, ThumbnailImage};

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("{0}")]
    LibraryUnavailable(String),
    #[error("mpv: {0}")]
    Mpv(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    FileLoaded,
    /// First frame after a load or seek is on screen.
    PlaybackRestart,
    Position {
        seconds: f64,
    },
    Duration {
        seconds: f64,
    },
    Paused(bool),
    Buffering(bool),
    /// Transfer function of the current video (`pq`, `hlg`, `bt.1886`, …).
    VideoGamma(String),
    /// Timing measurement (`ttff`, `seek`) in milliseconds.
    Metric {
        name: &'static str,
        millis: f64,
    },
    TracksChanged,
    ChaptersChanged,
    EndFile {
        reason: EndReason,
        error: Option<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// True = PQ/BT.2020 + 16-bit float framebuffer for HDR/EDR displays;
    /// False = 8-bit SDR BT.709/BT.1886 surface (avoids float FBO overhead).
    pub hdr_surface: bool,
    pub hdr_target_peak: Option<u32>,
    pub exact_seek: bool,
    pub gl_advanced_control: bool,
    pub shader_paths: Vec<String>,
    pub tone_mapping: String,
    pub force_yuv420p: bool,
    pub scale_preset: String,
    pub hwdec_mode: String,
    pub demuxer_max_mb: u32,
    pub extra_options: String,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            hdr_surface: true,
            hdr_target_peak: None,
            exact_seek: true,
            gl_advanced_control: true,
            shader_paths: Vec::new(),
            tone_mapping: "auto".to_owned(),
            force_yuv420p: false,
            scale_preset: "balanced".to_owned(),
            hwdec_mode: "videotoolbox".to_owned(),
            demuxer_max_mb: 8,
            extra_options: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaTrack {
    pub id: i64,
    pub track_type: String,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
    pub external: bool,
    pub forced: bool,
    pub hearing_impaired: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub start_s: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerStats {
    pub video_codec: String,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub estimated_fps: f64,
    pub hwdec: String,
    pub gamma: String,
    pub audio_codec: String,
    pub audio_channels: i64,
    pub cache_seconds: f64,
    pub dropped_frames: i64,
    pub ab_loop_a: Option<f64>,
    pub ab_loop_b: Option<f64>,
    pub speed: f64,
    pub volume: f64,
    pub muted: bool,
    pub sub_delay: f64,
    pub audio_delay: f64,
}

pub trait PlayerEvents: Send + Sync {
    /// Called on the mpv event thread; implementations must not block.
    fn on_event(&self, event: PlayerEvent);
}

const POSITION_INTERVAL: Duration = Duration::from_millis(250);

// Observed property ids.
const PROP_TIME_POS: u64 = 1;
const PROP_DURATION: u64 = 2;
const PROP_PAUSE: u64 = 3;
const PROP_PAUSED_FOR_CACHE: u64 = 4;
const PROP_VIDEO_GAMMA: u64 = 5;
const PROP_TRACK_LIST: u64 = 6;
const PROP_CHAPTER_LIST: u64 = 7;

#[derive(Clone, Copy)]
struct Handle(*mut sys::mpv_handle);
// SAFETY: the libmpv client API is thread-safe for a single handle.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

#[derive(Default)]
struct Timing {
    load_started: Option<Instant>,
    seek_started: Option<Instant>,
}

pub struct Player {
    api: &'static sys::Api,
    handle: Handle,
    config: PlayerConfig,
    registry: Arc<stream::Registry>,
    timing: Arc<Mutex<Timing>>,
    shut_down: AtomicBool,
    events_thread: Mutex<Option<JoinHandle<()>>>,
    renderer: Mutex<Option<render::Renderer>>,
}

impl Player {
    /// Creates mpv (no video output yet — see [`Player::render_init`]).
    /// Blocking (loads libmpv on first use) — never call on the main thread.
    pub fn new(events: Arc<dyn PlayerEvents>) -> Result<Self, PlayerError> {
        Self::new_with_config(events, PlayerConfig::default())
    }

    pub fn new_with_config(
        events: Arc<dyn PlayerEvents>,
        config: PlayerConfig,
    ) -> Result<Self, PlayerError> {
        let api = sys::api().map_err(PlayerError::LibraryUnavailable)?;
        // SAFETY: plain constructor; null on OOM.
        let raw = unsafe { (api.create)() };
        if raw.is_null() {
            return Err(PlayerError::Mpv("mpv_create failed".into()));
        }
        let handle = Handle(raw);
        let player = Self {
            api,
            handle,
            config,
            registry: Arc::new(stream::Registry::default()),
            timing: Arc::new(Mutex::new(Timing::default())),
            shut_down: AtomicBool::new(false),
            events_thread: Mutex::new(None),
            renderer: Mutex::new(None),
        };
        if let Err(e) = player.configure() {
            // SAFETY: not initialized or partially initialized; destroy is valid.
            unsafe { (api.terminate_destroy)(raw) };
            player.shut_down.store(true, Ordering::SeqCst);
            return Err(e);
        }
        let registry = Arc::clone(&player.registry);
        let timing = Arc::clone(&player.timing);
        let thread = std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || event_loop(api, handle, registry, timing, events))
            .map_err(|e| PlayerError::Mpv(e.to_string()))?;
        *player
            .events_thread
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(thread);
        Ok(player)
    }

    fn configure(&self) -> Result<(), PlayerError> {
        let (trc, prim, fbo_fmt) = if self.config.hdr_surface {
            ("pq", "bt.2020", "rgba16f")
        } else {
            ("bt.1886", "bt.709", "rgba8")
        };
        let hwdec = match self.config.hwdec_mode.trim() {
            "no" => "no",
            "auto-safe" => "auto-safe",
            _ => "videotoolbox",
        };
        let scale = match self.config.scale_preset.trim() {
            "fast" => "bilinear",
            "high" => "ewa_lanczossharp",
            _ => "spline36",
        };
        let mb = self.config.demuxer_max_mb.clamp(4, 128);
        let back_mb = (mb / 4).max(2);
        let mut options: Vec<(&str, String)> = vec![
            // Never read user config, run scripts or spawn yt-dlp.
            ("config", "no".into()),
            ("load-scripts", "no".into()),
            ("ytdl", "no".into()),
            ("osc", "no".into()),
            ("terminal", "no".into()),
            ("input-default-bindings", "no".into()),
            ("input-vo-keyboard", "no".into()),
            // Rendering through the render API (ADR 0004 / ADR 0011).
            ("vo", "libmpv".into()),
            ("hwdec", hwdec.into()),
            ("scale", scale.into()),
            ("cscale", scale.into()),
            ("target-trc", trc.into()),
            ("target-prim", prim.into()),
            ("fbo-format", fbo_fmt.into()),
            // Behaviour & trimmed playback memory footprint.
            ("idle", "yes".into()),
            ("keep-open", "yes".into()),
            ("cache", "yes".into()),
            ("demuxer-max-bytes", format!("{mb}MiB")),
            ("demuxer-max-back-bytes", format!("{back_mb}MiB")),
            ("demuxer-readahead-secs", "10".into()),
            ("cache-pause-wait", "1".into()),
            ("vd-lavc-threads", "4".into()),
            ("ad-lavc-threads", "2".into()),
            ("opengl-pbo", "no".into()),
            // Respect ASS/SSA script styling by default (§5.8).
            ("sub-ass-override", "no".into()),
        ];
        if self.config.gl_advanced_control {
            options.push(("vd-lavc-dr", "yes".into()));
        }
        if let Some(peak) = self.config.hdr_target_peak {
            options.push(("target-peak", peak.to_string()));
        }
        let tm = self.config.tone_mapping.trim();
        if !tm.is_empty() && tm != "auto" {
            options.push(("tone-mapping", tm.to_owned()));
        }
        if self.config.force_yuv420p {
            options.push(("vf", "format=yuv420p".into()));
        }
        if !self.config.shader_paths.is_empty() {
            options.push(("glsl-shaders", self.config.shader_paths.join(":")));
        }
        for (name, value) in options {
            self.set_option(name, &value)?;
        }
        for pair in self.config.extra_options.split([',', '\n']) {
            let Some((raw_k, raw_v)) = pair.split_once('=') else {
                continue;
            };
            let k = raw_k.trim();
            let v = raw_v.trim();
            if k.is_empty() || matches!(k, "vo" | "config" | "load-scripts" | "ytdl") {
                continue;
            }
            let _ = self.set_option(k, v);
        }
        // SAFETY: handle is valid and not yet initialized.
        self.check(unsafe { (self.api.initialize)(self.handle.0) })?;
        // CINMA_MPV_LOG=v|debug for diagnostics; warnings otherwise.
        let level = cstring(&std::env::var("CINMA_MPV_LOG").unwrap_or_else(|_| "warn".into()))?;
        // SAFETY: valid handle and NUL-terminated string.
        unsafe { (self.api.request_log_messages)(self.handle.0, level.as_ptr()) };
        let protocol = CString::new(PROTOCOL).map_err(|e| PlayerError::Mpv(e.to_string()))?;
        // SAFETY: the registry outlives the handle (the event thread holds an
        // Arc until after mpv_terminate_destroy).
        self.check(unsafe {
            (self.api.stream_cb_add_ro)(
                self.handle.0,
                protocol.as_ptr(),
                Arc::as_ptr(&self.registry) as *mut c_void,
                stream::open_fn,
            )
        })?;
        for (id, name, format) in [
            (PROP_TIME_POS, c"time-pos", sys::FORMAT_DOUBLE),
            (PROP_DURATION, c"duration", sys::FORMAT_DOUBLE),
            (PROP_PAUSE, c"pause", sys::FORMAT_FLAG),
            (PROP_PAUSED_FOR_CACHE, c"paused-for-cache", sys::FORMAT_FLAG),
            (PROP_VIDEO_GAMMA, c"video-params/gamma", sys::FORMAT_STRING),
            (PROP_TRACK_LIST, c"track-list", sys::FORMAT_NONE),
            (PROP_CHAPTER_LIST, c"chapter-list", sys::FORMAT_NONE),
        ] {
            // SAFETY: valid handle, static C string.
            self.check(unsafe {
                (self.api.observe_property)(self.handle.0, id, name.as_ptr(), format)
            })?;
        }
        Ok(())
    }

    fn set_option(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        let (n, v) = (cstring(name)?, cstring(value)?);
        // SAFETY: valid handle and NUL-terminated strings.
        let rc = unsafe { (self.api.set_option_string)(self.handle.0, n.as_ptr(), v.as_ptr()) };
        self.check(rc)
            .map_err(|e| PlayerError::Mpv(format!("option {name}={value}: {e}")))
    }

    fn check(&self, rc: c_int) -> Result<(), PlayerError> {
        if rc >= 0 {
            return Ok(());
        }
        // SAFETY: mpv_error_string returns a static string for any code.
        let msg = unsafe { CStr::from_ptr((self.api.error_string)(rc)) };
        Err(PlayerError::Mpv(msg.to_string_lossy().into_owned()))
    }

    /// Asynchronous command: never blocks the caller (safe on the main thread).
    fn command(&self, args: &[&str]) -> Result<(), PlayerError> {
        if self.shut_down.load(Ordering::SeqCst) {
            return Err(PlayerError::Mpv("player is shut down".into()));
        }
        let owned = args
            .iter()
            .map(|a| cstring(a))
            .collect::<Result<Vec<_>, _>>()?;
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|s| s.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: valid handle; NULL-terminated argv of C strings (copied by mpv).
        self.check(unsafe { (self.api.command_async)(self.handle.0, 0, ptrs.as_mut_ptr()) })
    }

    /// Plays bytes from `source` through the `cinma://` protocol.
    pub fn load_source(&self, source: Arc<dyn ByteSource>) -> Result<(), PlayerError> {
        let uri = self.registry.register_exclusive(source);
        self.load(&uri)
    }

    /// Plays a URL mpv can open itself (plain HTTP(S) streams, local files).
    pub fn load_url(&self, url: &str) -> Result<(), PlayerError> {
        self.registry.clear();
        self.load(url)
    }

    /// Starts the time-to-first-frame clock now (call when the user asks to
    /// play, before resolving the stream). Otherwise it starts at `loadfile`.
    pub fn begin_load(&self) {
        if let Ok(mut t) = self.timing.lock() {
            *t = Timing {
                load_started: Some(Instant::now()),
                seek_started: None,
            };
        }
    }

    fn load(&self, uri: &str) -> Result<(), PlayerError> {
        if let Ok(mut t) = self.timing.lock() {
            t.seek_started = None;
            t.load_started.get_or_insert_with(Instant::now);
        }
        self.command(&["loadfile", uri, "replace"])
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), PlayerError> {
        self.command(&["set", "pause", if paused { "yes" } else { "no" }])
    }

    pub fn toggle_pause(&self) -> Result<(), PlayerError> {
        self.command(&["cycle", "pause"])
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), PlayerError> {
        self.mark_seek();
        self.command(&["seek", &format!("{seconds:.3}"), "absolute"])
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), PlayerError> {
        self.seek_relative_mode(seconds, self.config.exact_seek)
    }

    pub fn seek_relative_mode(&self, seconds: f64, exact: bool) -> Result<(), PlayerError> {
        self.mark_seek();
        let mode = if exact { "relative+exact" } else { "relative" };
        self.command(&["seek", &format!("{seconds:.3}"), mode])
    }

    pub fn seek_absolute_exact(&self, seconds: f64) -> Result<(), PlayerError> {
        self.mark_seek();
        self.command(&["seek", &format!("{seconds:.3}"), "absolute+exact"])
    }

    pub fn set_property_str(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        if self.shut_down.load(Ordering::SeqCst) {
            return Err(PlayerError::Mpv("player is shut down".into()));
        }
        let (n, v) = (cstring(name)?, cstring(value)?);
        // SAFETY: valid handle and NUL-terminated strings.
        self.check(unsafe { (self.api.set_property_string)(self.handle.0, n.as_ptr(), v.as_ptr()) })
    }

    fn get_property_string_opt(&self, name: &str) -> Option<String> {
        if self.shut_down.load(Ordering::SeqCst) {
            return None;
        }
        let n = cstring(name).ok()?;
        // SAFETY: valid handle and property name; result freed with `mpv_free`.
        let ptr = unsafe { (self.api.get_property_string)(self.handle.0, n.as_ptr()) };
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.api.free)(ptr.cast()) };
        Some(s)
    }

    fn get_property_double_opt(&self, name: &str) -> Option<f64> {
        if self.shut_down.load(Ordering::SeqCst) {
            return None;
        }
        let n = cstring(name).ok()?;
        let mut out: f64 = 0.0;
        let rc = unsafe {
            (self.api.get_property)(
                self.handle.0,
                n.as_ptr(),
                sys::FORMAT_DOUBLE,
                (&raw mut out).cast(),
            )
        };
        (rc >= 0).then_some(out)
    }

    fn get_property_i64_opt(&self, name: &str) -> Option<i64> {
        if self.shut_down.load(Ordering::SeqCst) {
            return None;
        }
        let n = cstring(name).ok()?;
        let mut out: i64 = 0;
        let rc = unsafe {
            (self.api.get_property)(
                self.handle.0,
                n.as_ptr(),
                sys::FORMAT_INT64,
                (&raw mut out).cast(),
            )
        };
        (rc >= 0).then_some(out)
    }

    fn get_property_flag_opt(&self, name: &str) -> Option<bool> {
        if self.shut_down.load(Ordering::SeqCst) {
            return None;
        }
        let n = cstring(name).ok()?;
        let mut out: c_int = 0;
        let rc = unsafe {
            (self.api.get_property)(
                self.handle.0,
                n.as_ptr(),
                sys::FORMAT_FLAG,
                (&raw mut out).cast(),
            )
        };
        (rc >= 0).then_some(out != 0)
    }

    fn get_property_node(&self, name: &str) -> Result<MpvNode, PlayerError> {
        if self.shut_down.load(Ordering::SeqCst) {
            return Err(PlayerError::Mpv("player is shut down".into()));
        }
        let n = cstring(name)?;
        let mut raw = sys::mpv_node {
            u: sys::mpv_node_union { int64: 0 },
            format: sys::FORMAT_NONE,
        };
        // SAFETY: valid handle; `raw` freed unconditionally below.
        let rc = unsafe {
            (self.api.get_property)(
                self.handle.0,
                n.as_ptr(),
                sys::FORMAT_NODE,
                (&raw mut raw).cast(),
            )
        };
        if rc < 0 {
            self.check(rc)?;
        }
        let owned = unsafe { MpvNode::from_raw(&raw) };
        unsafe { (self.api.free_node_contents)(&raw mut raw) };
        Ok(owned)
    }

    pub fn tracks(&self) -> Result<Vec<MediaTrack>, PlayerError> {
        let node = self.get_property_node("track-list")?;
        let MpvNode::Array(items) = node else {
            return Ok(Vec::new());
        };
        Ok(items
            .into_iter()
            .filter_map(|item| {
                let MpvNode::Map(fields) = item else {
                    return None;
                };
                let id = map_i64(&fields, "id")?;
                let track_type = map_str(&fields, "type")?.to_owned();
                Some(MediaTrack {
                    id,
                    track_type,
                    title: map_str(&fields, "title").map(str::to_owned),
                    lang: map_str(&fields, "lang").map(str::to_owned),
                    codec: map_str(&fields, "codec").map(str::to_owned),
                    selected: map_bool(&fields, "selected").unwrap_or(false),
                    external: map_bool(&fields, "external").unwrap_or(false),
                    forced: map_bool(&fields, "forced").unwrap_or(false),
                    hearing_impaired: map_bool(&fields, "hearing-impaired").unwrap_or(false),
                })
            })
            .collect())
    }

    pub fn chapters(&self) -> Result<Vec<Chapter>, PlayerError> {
        let node = self.get_property_node("chapter-list")?;
        let MpvNode::Array(items) = node else {
            return Ok(Vec::new());
        };
        Ok(items
            .into_iter()
            .filter_map(|item| {
                let MpvNode::Map(fields) = item else {
                    return None;
                };
                let start_s = map_f64(&fields, "time")?;
                let title = map_str(&fields, "title").unwrap_or("Chapter").to_owned();
                Some(Chapter { title, start_s })
            })
            .collect())
    }

    pub fn stats(&self) -> PlayerStats {
        let ab_a = self
            .get_property_string_opt("ab-loop-a")
            .and_then(|s| s.parse::<f64>().ok());
        let ab_b = self
            .get_property_string_opt("ab-loop-b")
            .and_then(|s| s.parse::<f64>().ok());
        PlayerStats {
            video_codec: self
                .get_property_string_opt("video-codec")
                .unwrap_or_default(),
            width: self.get_property_i64_opt("video-params/w").unwrap_or(0),
            height: self.get_property_i64_opt("video-params/h").unwrap_or(0),
            fps: self.get_property_double_opt("container-fps").unwrap_or(0.0),
            estimated_fps: self
                .get_property_double_opt("estimated-vf-fps")
                .unwrap_or(0.0),
            hwdec: self
                .get_property_string_opt("hwdec-current")
                .unwrap_or_default(),
            gamma: self
                .get_property_string_opt("video-params/gamma")
                .unwrap_or_default(),
            audio_codec: self
                .get_property_string_opt("audio-codec-name")
                .unwrap_or_default(),
            audio_channels: self
                .get_property_i64_opt("audio-params/channel-count")
                .unwrap_or(0),
            cache_seconds: self
                .get_property_double_opt("demuxer-cache-duration")
                .unwrap_or(0.0),
            dropped_frames: self.get_property_i64_opt("frame-drop-count").unwrap_or(0),
            ab_loop_a: ab_a,
            ab_loop_b: ab_b,
            speed: self.get_property_double_opt("speed").unwrap_or(1.0),
            volume: self.get_property_double_opt("volume").unwrap_or(100.0),
            muted: self.get_property_flag_opt("mute").unwrap_or(false),
            sub_delay: self.get_property_double_opt("sub-delay").unwrap_or(0.0),
            audio_delay: self.get_property_double_opt("audio-delay").unwrap_or(0.0),
        }
    }

    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), PlayerError> {
        let val = id.map_or_else(|| "no".to_owned(), |i| i.to_string());
        self.set_property_str("aid", &val)
    }

    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<(), PlayerError> {
        let val = id.map_or_else(|| "no".to_owned(), |i| i.to_string());
        self.set_property_str("sid", &val)
    }

    pub fn set_secondary_subtitle_track(&self, id: Option<i64>) -> Result<(), PlayerError> {
        let val = id.map_or_else(|| "no".to_owned(), |i| i.to_string());
        self.set_property_str("secondary-sid", &val)
    }

    pub fn add_subtitle(
        &self,
        path: &str,
        title: Option<&str>,
        lang: Option<&str>,
        select: bool,
    ) -> Result<(), PlayerError> {
        let flag = if select { "select" } else { "auto" };
        let title = title.unwrap_or("");
        let lang = lang.unwrap_or("");
        self.command(&["sub-add", path, flag, title, lang])
    }

    pub fn set_subtitle_delay(&self, seconds: f64) -> Result<(), PlayerError> {
        self.set_property_str("sub-delay", &format!("{seconds:.3}"))
    }

    pub fn set_audio_delay(&self, seconds: f64) -> Result<(), PlayerError> {
        self.set_property_str("audio-delay", &format!("{seconds:.3}"))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_subtitle_style(
        &self,
        font: Option<&str>,
        font_size: Option<u32>,
        color_hex: Option<&str>,
        border_size: Option<u32>,
        border_color_hex: Option<&str>,
        back_color_hex: Option<&str>,
        bold: bool,
        pos: Option<u32>,
        ass_override: bool,
        opacity_pct: u32,
        box_opacity_pct: u32,
        align_x: &str,
        secondary_pos: u32,
    ) -> Result<(), PlayerError> {
        if let Some(font) = font.filter(|s| !s.trim().is_empty()) {
            self.set_property_str("sub-font", font)?;
        }
        if let Some(size) = font_size {
            self.set_property_str("sub-font-size", &size.clamp(14, 96).to_string())?;
        }
        let text_opacity = opacity_pct.clamp(10, 100);
        let base_color = color_hex
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("#FFFFFF");
        let color = apply_hex_alpha(base_color, text_opacity);
        self.set_property_str("sub-color", &color)?;
        if let Some(b_size) = border_size {
            self.set_property_str("sub-border-size", &b_size.clamp(0, 8).to_string())?;
        }
        let base_border_color = border_color_hex
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("#000000");
        let b_color = apply_hex_alpha(base_border_color, text_opacity);
        self.set_property_str("sub-border-color", &b_color)?;
        let box_opacity = box_opacity_pct.min(100);
        if box_opacity == 0 {
            self.set_property_str("sub-back-color", "#00000000")?;
            self.set_property_str("sub-border-style", "outline-and-shadow")?;
        } else {
            let base_bg = back_color_hex
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("#000000");
            let bg_color = apply_hex_alpha(base_bg, box_opacity);
            self.set_property_str("sub-back-color", &bg_color)?;
            self.set_property_str("sub-border-style", "background-box")?;
        }
        self.set_property_str("sub-bold", if bold { "yes" } else { "no" })?;
        if let Some(pos) = pos {
            self.set_subtitle_pos(pos)?;
        }
        let align = match align_x.trim() {
            "left" => "left",
            "right" => "right",
            _ => "center",
        };
        self.set_property_str("sub-align-x", align)?;
        self.set_property_str(
            "secondary-sub-pos",
            &secondary_pos.clamp(5, 100).to_string(),
        )?;
        self.set_property_str(
            "sub-ass-override",
            if ass_override { "force" } else { "no" },
        )
    }

    pub fn set_subtitle_pos(&self, pos: u32) -> Result<(), PlayerError> {
        self.set_property_str("sub-pos", &pos.clamp(5, 100).to_string())
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), PlayerError> {
        self.set_property_str("speed", &format!("{:.2}", speed.clamp(0.25, 4.0)))
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), PlayerError> {
        self.set_property_str("volume", &format!("{:.1}", volume.clamp(0.0, 200.0)))
    }

    pub fn set_volume_max(&self, max_pct: u32) -> Result<(), PlayerError> {
        self.set_property_str("volume-max", &max_pct.clamp(100, 200).to_string())
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), PlayerError> {
        self.set_property_str("mute", if muted { "yes" } else { "no" })
    }

    pub fn set_crop_mode(&self, mode: &str) -> Result<(), PlayerError> {
        let (panscan, aspect) = match mode.trim() {
            "fill" => ("1.0", "no"),
            "16:9" => ("0.0", "16:9"),
            "21:9" => ("0.0", "21:9"),
            "4:3" => ("0.0", "4:3"),
            _ => ("0.0", "no"),
        };
        self.set_property_str("panscan", panscan)?;
        self.set_property_str("video-aspect-override", aspect)
    }

    pub fn set_audio_processing(
        &self,
        normalize: bool,
        downmix_stereo: bool,
    ) -> Result<(), PlayerError> {
        let channels = if downmix_stereo {
            "stereo"
        } else {
            "auto-safe"
        };
        self.set_property_str("audio-channels", channels)?;
        let af = if normalize { "dynaudnorm" } else { "" };
        self.set_property_str("af", af)
    }

    pub fn set_ab_loop_a(&self, seconds: Option<f64>) -> Result<(), PlayerError> {
        let val = seconds.map_or_else(|| "no".to_owned(), |s| format!("{s:.3}"));
        self.set_property_str("ab-loop-a", &val)
    }

    pub fn set_ab_loop_b(&self, seconds: Option<f64>) -> Result<(), PlayerError> {
        let val = seconds.map_or_else(|| "no".to_owned(), |s| format!("{s:.3}"));
        self.set_property_str("ab-loop-b", &val)
    }

    pub fn set_hdr_target_peak(&self, nits: Option<u32>) -> Result<(), PlayerError> {
        let val = nits.map_or_else(|| "auto".to_owned(), |n| n.clamp(100, 10_000).to_string());
        self.set_property_str("target-peak", &val)
    }

    pub fn set_tone_mapping(&self, mode: &str) -> Result<(), PlayerError> {
        let m = mode.trim();
        self.set_property_str("tone-mapping", if m.is_empty() { "auto" } else { m })
    }

    pub fn set_glsl_shaders(&self, paths: &[String]) -> Result<(), PlayerError> {
        if paths.is_empty() {
            self.command(&["change-list", "glsl-shaders", "clr", ""])
        } else {
            self.set_property_str("glsl-shaders", &paths.join(":"))
        }
    }

    pub fn screenshot_to_file(&self, path: &Path) -> Result<(), PlayerError> {
        let s = path
            .to_str()
            .ok_or_else(|| PlayerError::Mpv("non-UTF-8 screenshot path".into()))?;
        self.command(&["screenshot-to-file", s, "subtitles"])
    }

    fn mark_seek(&self) {
        if let Ok(mut t) = self.timing.lock() {
            t.seek_started = Some(Instant::now());
        }
    }

    /// Creates the OpenGL render context. The host layer's CGL context must
    /// be current on the calling thread. `on_update` fires (on an mpv thread)
    /// whenever a new frame should be drawn.
    pub fn render_init(&self, on_update: UpdateCallback) -> Result<(), PlayerError> {
        self.render_init_with_options(on_update, self.config.gl_advanced_control)
    }

    pub fn render_init_with_options(
        &self,
        on_update: UpdateCallback,
        advanced_control: bool,
    ) -> Result<(), PlayerError> {
        let mut guard = self.renderer.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
        if self.shut_down.load(Ordering::SeqCst) {
            return Err(PlayerError::Mpv("player is shut down".into()));
        }
        let mut init = sys::mpv_opengl_init_params {
            get_proc_address: Some(render::get_proc_address),
            get_proc_address_ctx: std::ptr::null_mut(),
        };
        let api_type = c"opengl";
        let mut adv: c_int = if advanced_control { 1 } else { 0 };
        let mut params = [
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&raw mut init).cast(),
            },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_ADVANCED_CONTROL,
                data: (&raw mut adv).cast(),
            },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let mut ctx: *mut sys::mpv_render_context = std::ptr::null_mut();
        // SAFETY: valid handle; params are a terminated array living across the call.
        self.check(unsafe {
            (self.api.render_context_create)(&mut ctx, self.handle.0, params.as_mut_ptr())
        })?;
        let callback = Box::into_raw(Box::new(on_update));
        // SAFETY: ctx is valid; callback stays alive until render_free.
        unsafe {
            (self.api.render_context_set_update_callback)(
                ctx,
                Some(render::on_update),
                callback.cast(),
            );
        }
        *guard = Some(render::Renderer { ctx, callback });
        Ok(())
    }

    /// Queries libmpv whether a new video frame is ready to render (`MPV_RENDER_UPDATE_FRAME`).
    /// Safe to invoke inside `CAOpenGLLayer.canDraw(inCGLContext:)` on the CA display link thread.
    pub fn render_should_draw(&self) -> bool {
        let Ok(guard) = self.renderer.try_lock() else {
            return false;
        };
        let Some(renderer) = guard.as_ref() else {
            return false;
        };
        // SAFETY: renderer.ctx is live under mutex; render_context_update is lock-free/safe.
        let flags = unsafe { (self.api.render_context_update)(renderer.ctx) };
        (flags & sys::RENDER_UPDATE_FRAME) != 0
    }

    /// Renders the current frame into `fbo` (`width`×`height` pixels).
    /// Call from the layer's draw method with its CGL context current.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), PlayerError> {
        self.render_frame(fbo, width, height, false)
    }

    pub fn render_frame(
        &self,
        fbo: i32,
        width: i32,
        height: i32,
        block_for_target_time: bool,
    ) -> Result<(), PlayerError> {
        let guard = self.renderer.lock().unwrap_or_else(|e| e.into_inner());
        let Some(renderer) = guard.as_ref() else {
            return Ok(());
        };
        let mut target = sys::mpv_opengl_fbo {
            fbo,
            w: width,
            h: height,
            internal_format: 0,
        };
        let mut flip: c_int = 1;
        let mut block: c_int = if block_for_target_time { 1 } else { 0 };
        let mut params = [
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_OPENGL_FBO,
                data: (&raw mut target).cast(),
            },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_FLIP_Y,
                data: (&raw mut flip).cast(),
            },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: (&raw mut block).cast(),
            },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        // SAFETY: renderer.ctx is live (guarded); params outlive the call.
        self.check(unsafe { (self.api.render_context_render)(renderer.ctx, params.as_mut_ptr()) })
    }

    /// Frees the render context. Must run with the CGL context current and
    /// before [`Player::shutdown`].
    pub fn render_free(&self) {
        let renderer = self
            .renderer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(renderer) = renderer {
            // SAFETY: ctx is live; after free mpv no longer calls the callback.
            unsafe {
                (self.api.render_context_free)(renderer.ctx);
                drop(Box::from_raw(renderer.callback));
            }
        }
    }

    /// Stops playback and destroys mpv (asynchronously, on the event thread).
    /// Call [`Player::render_free`] first.
    pub fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.renderer.lock().map(|r| r.is_some()).unwrap_or(false) {
            eprintln!("[cinma] player: shutdown before render_free; freeing render context now");
            self.render_free();
        }
        let quit = c"quit";
        let mut argv = [quit.as_ptr(), std::ptr::null()];
        // SAFETY: valid handle until the event thread sees SHUTDOWN.
        unsafe { (self.api.command_async)(self.handle.0, 0, argv.as_mut_ptr()) };
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
        // Don't join: Drop may run on the main thread, which mpv's macOS code
        // can be waiting on. The event thread finishes on its own.
        drop(self.events_thread.lock().map(|mut t| t.take()));
    }
}

pub fn release_heap_pressure() {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut c_void, goal: usize) -> usize;
        }
        let _ = malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}

/// Reads a string `sysctl` value in-process (no `/usr/sbin/sysctl` child).
pub fn sysctl_string(name: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn sysctlbyname(
                name: *const c_char,
                oldp: *mut c_void,
                oldlenp: *mut usize,
                newp: *mut c_void,
                newlen: usize,
            ) -> c_int;
        }
        let name = CString::new(name).ok()?;
        let mut len: usize = 0;
        // SAFETY: size query with a valid NUL-terminated name and null buffer.
        let rc = unsafe {
            sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || len == 0 || len > 4096 {
            return None;
        }
        let mut buf = vec![0u8; len];
        // SAFETY: `buf` has `len` writable bytes, as reported by the size query.
        let rc = unsafe {
            sysctlbyname(
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        buf.truncate(len);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        None
    }
}

/// Checks whether a local `Contents/Frameworks/libmpv.2.dylib` or Homebrew dylib exists.
pub use sys::api as ensure_libmpv_available;

fn cstring(s: &str) -> Result<CString, PlayerError> {
    CString::new(s).map_err(|_| PlayerError::Mpv(format!("NUL in argument: {s:?}")))
}

fn measure_enabled() -> bool {
    std::env::var("CINMA_MEASURE").is_ok_and(|v| v == "1")
}

fn event_loop(
    api: &'static sys::Api,
    handle: Handle,
    registry: Arc<stream::Registry>,
    timing: Arc<Mutex<Timing>>,
    events: Arc<dyn PlayerEvents>,
) {
    let mut last_position = Instant::now() - POSITION_INTERVAL;
    loop {
        // SAFETY: valid handle; blocks until an event (no polling).
        let event = unsafe { &*(api.wait_event)(handle.0, -1.0) };
        match event.event_id {
            sys::EVENT_SHUTDOWN => break,
            sys::EVENT_FILE_LOADED => events.on_event(PlayerEvent::FileLoaded),
            sys::EVENT_PLAYBACK_RESTART => {
                let metric = timing.lock().ok().and_then(|mut t| {
                    if let Some(start) = t.load_started.take() {
                        t.seek_started = None;
                        Some(("ttff", start))
                    } else {
                        t.seek_started.take().map(|start| ("seek", start))
                    }
                });
                if let Some((name, start)) = metric {
                    let millis = start.elapsed().as_secs_f64() * 1000.0;
                    if measure_enabled() {
                        eprintln!("CINMA_{}_MS={millis:.1}", name.to_ascii_uppercase());
                    }
                    events.on_event(PlayerEvent::Metric { name, millis });
                }
                last_position = Instant::now() - POSITION_INTERVAL;
                events.on_event(PlayerEvent::PlaybackRestart);
            }
            sys::EVENT_END_FILE => {
                // SAFETY: END_FILE events carry mpv_event_end_file.
                let data = unsafe { &*(event.data as *const sys::mpv_event_end_file) };
                let reason = match data.reason {
                    sys::END_FILE_REASON_EOF => EndReason::Eof,
                    sys::END_FILE_REASON_ERROR => EndReason::Error,
                    sys::END_FILE_REASON_STOP | sys::END_FILE_REASON_QUIT => EndReason::Stopped,
                    _ => EndReason::Stopped,
                };
                let error = (data.error < 0).then(|| {
                    // SAFETY: static string for any error code.
                    unsafe { CStr::from_ptr((api.error_string)(data.error)) }
                        .to_string_lossy()
                        .into_owned()
                });
                events.on_event(PlayerEvent::EndFile { reason, error });
            }
            sys::EVENT_PROPERTY_CHANGE => {
                // SAFETY: PROPERTY_CHANGE events carry mpv_event_property.
                let prop = unsafe { &*(event.data as *const sys::mpv_event_property) };
                match event.reply_userdata {
                    PROP_TRACK_LIST => {
                        events.on_event(PlayerEvent::TracksChanged);
                        continue;
                    }
                    PROP_CHAPTER_LIST => {
                        events.on_event(PlayerEvent::ChaptersChanged);
                        continue;
                    }
                    _ => {}
                }
                if prop.format == sys::FORMAT_NONE || prop.data.is_null() {
                    continue;
                }
                match (event.reply_userdata, prop.format) {
                    (PROP_TIME_POS, sys::FORMAT_DOUBLE) => {
                        if last_position.elapsed() >= POSITION_INTERVAL {
                            last_position = Instant::now();
                            // SAFETY: DOUBLE format => data points to f64.
                            let seconds = unsafe { *(prop.data as *const f64) };
                            events.on_event(PlayerEvent::Position { seconds });
                        }
                    }
                    (PROP_DURATION, sys::FORMAT_DOUBLE) => {
                        let seconds = unsafe { *(prop.data as *const f64) };
                        events.on_event(PlayerEvent::Duration { seconds });
                    }
                    (PROP_PAUSE, sys::FORMAT_FLAG) => {
                        // SAFETY: FLAG format => data points to c_int.
                        let flag = unsafe { *(prop.data as *const c_int) } != 0;
                        events.on_event(PlayerEvent::Paused(flag));
                    }
                    (PROP_PAUSED_FOR_CACHE, sys::FORMAT_FLAG) => {
                        let flag = unsafe { *(prop.data as *const c_int) } != 0;
                        events.on_event(PlayerEvent::Buffering(flag));
                    }
                    (PROP_VIDEO_GAMMA, sys::FORMAT_STRING) => {
                        // SAFETY: STRING format => data points to a char*.
                        let value = unsafe { CStr::from_ptr(*(prop.data as *const *const c_char)) };
                        events.on_event(PlayerEvent::VideoGamma(
                            value.to_string_lossy().into_owned(),
                        ));
                    }
                    _ => {}
                }
            }
            sys::EVENT_LOG_MESSAGE => {
                // SAFETY: LOG_MESSAGE events carry mpv_event_log_message.
                let log = unsafe { &*(event.data as *const sys::mpv_event_log_message) };
                let (prefix, level, text) = unsafe {
                    (
                        CStr::from_ptr(log.prefix).to_string_lossy(),
                        CStr::from_ptr(log.level).to_string_lossy(),
                        CStr::from_ptr(log.text).to_string_lossy(),
                    )
                };
                eprint!("[mpv/{prefix}] {level}: {text}");
            }
            _ => {}
        }
    }
    // SAFETY: no other thread uses the handle after SHUTDOWN (Player methods
    // check `shut_down`, which is set before `quit` is sent).
    unsafe { (api.terminate_destroy)(handle.0) };
    registry.clear();
    events.on_event(PlayerEvent::Shutdown);
}

enum MpvNode {
    None,
    Str(String),
    Bool(bool),
    Int(i64),
    Double(f64),
    Array(Vec<MpvNode>),
    Map(Vec<(String, MpvNode)>),
}

impl MpvNode {
    unsafe fn from_raw(node: &sys::mpv_node) -> Self {
        unsafe {
            match node.format {
                sys::FORMAT_STRING => {
                    if node.u.string.is_null() {
                        Self::None
                    } else {
                        Self::Str(CStr::from_ptr(node.u.string).to_string_lossy().into_owned())
                    }
                }
                sys::FORMAT_FLAG => Self::Bool(node.u.flag != 0),
                sys::FORMAT_INT64 => Self::Int(node.u.int64),
                sys::FORMAT_DOUBLE => Self::Double(node.u.double_),
                sys::FORMAT_NODE_ARRAY => {
                    if node.u.list.is_null() {
                        Self::Array(Vec::new())
                    } else {
                        let list = &*node.u.list;
                        let n = list.num.max(0) as usize;
                        let slice = if list.values.is_null() || n == 0 {
                            &[]
                        } else {
                            std::slice::from_raw_parts(list.values, n)
                        };
                        Self::Array(slice.iter().map(|c| Self::from_raw(c)).collect())
                    }
                }
                sys::FORMAT_NODE_MAP => {
                    if node.u.list.is_null() {
                        Self::Map(Vec::new())
                    } else {
                        let list = &*node.u.list;
                        let n = list.num.max(0) as usize;
                        if list.values.is_null() || list.keys.is_null() || n == 0 {
                            Self::Map(Vec::new())
                        } else {
                            let keys = std::slice::from_raw_parts(list.keys, n);
                            let vals = std::slice::from_raw_parts(list.values, n);
                            let out = keys
                                .iter()
                                .zip(vals.iter())
                                .filter(|(k, _)| !k.is_null())
                                .map(|(&k, v)| {
                                    (
                                        CStr::from_ptr(k).to_string_lossy().into_owned(),
                                        Self::from_raw(v),
                                    )
                                })
                                .collect();
                            Self::Map(out)
                        }
                    }
                }
                _ => Self::None,
            }
        }
    }
}

fn map_find<'a>(fields: &'a [(String, MpvNode)], key: &str) -> Option<&'a MpvNode> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn map_str<'a>(fields: &'a [(String, MpvNode)], key: &str) -> Option<&'a str> {
    match map_find(fields, key)? {
        MpvNode::Str(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

fn map_bool(fields: &[(String, MpvNode)], key: &str) -> Option<bool> {
    match map_find(fields, key)? {
        MpvNode::Bool(b) => Some(*b),
        MpvNode::Int(i) => Some(*i != 0),
        _ => None,
    }
}

fn map_i64(fields: &[(String, MpvNode)], key: &str) -> Option<i64> {
    match map_find(fields, key)? {
        MpvNode::Int(i) => Some(*i),
        MpvNode::Double(d) => Some(*d as i64),
        _ => None,
    }
}

fn map_f64(fields: &[(String, MpvNode)], key: &str) -> Option<f64> {
    match map_find(fields, key)? {
        MpvNode::Double(d) => Some(*d),
        MpvNode::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn apply_hex_alpha(hex: &str, opacity_pct: u32) -> String {
    let raw = hex.trim().trim_start_matches('#');
    let rgb = match raw.len() {
        6 if raw.bytes().all(|b| b.is_ascii_hexdigit()) => raw,
        8 if raw.bytes().all(|b| b.is_ascii_hexdigit()) => &raw[2..],
        _ => return hex.trim().to_owned(),
    };
    let alpha = ((opacity_pct.min(100) * 255 + 50) / 100) as u8;
    format!("#{alpha:02X}{}", rgb.to_ascii_uppercase())
}

#[cfg(all(test, target_os = "macos"))]
mod sysctl_tests {
    #[test]
    fn reads_cpu_brand_in_process() {
        let brand = super::sysctl_string("machdep.cpu.brand_string").unwrap_or_default();
        assert!(!brand.is_empty(), "brand string should be readable");
        assert!(super::sysctl_string("no.such.sysctl").is_none());
    }

    #[test]
    fn applies_alpha_to_rgb_and_argb_hex() {
        assert_eq!(super::apply_hex_alpha("#ffffff", 100), "#FFFFFFFF");
        assert_eq!(super::apply_hex_alpha("#FFFFFF", 50), "#80FFFFFF");
        assert_eq!(super::apply_hex_alpha("#80ffdd33", 0), "#00FFDD33");
    }
}
