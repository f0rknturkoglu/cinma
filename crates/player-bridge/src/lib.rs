//! libmpv glue: lazy-loaded libmpv, `cinma://` byte-stream protocol, and a
//! small event-driven player handle. Rendering uses the libmpv render API
//! into the host's `CAOpenGLLayer` (docs/adr/0004).

mod render;
mod stream;
mod sys;

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cinma_blobstore::ByteSource;

pub use render::UpdateCallback;
pub use stream::PROTOCOL;

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
    Position { seconds: f64 },
    Duration { seconds: f64 },
    Paused(bool),
    Buffering(bool),
    /// Transfer function of the current video (`pq`, `hlg`, `bt.1886`, …).
    VideoGamma(String),
    /// Timing measurement (`ttff`, `seek`) in milliseconds.
    Metric { name: &'static str, millis: f64 },
    EndFile { reason: EndReason, error: Option<String> },
    Shutdown,
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
        *player.events_thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(thread);
        Ok(player)
    }

    fn configure(&self) -> Result<(), PlayerError> {
        let options: &[(&str, &str)] = &[
            // Never read user config, run scripts or spawn yt-dlp.
            ("config", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            ("osc", "no"),
            ("terminal", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            // Rendering through the render API (ADR 0004).
            ("vo", "libmpv"),
            ("hwdec", "videotoolbox"),
            // Behaviour.
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("cache", "yes"),
            ("demuxer-max-bytes", "64MiB"),
            ("demuxer-max-back-bytes", "16MiB"),
        ];
        for (name, value) in options {
            self.set_option(name, value)?;
        }
        // SAFETY: handle is valid and not yet initialized.
        self.check(unsafe { (self.api.initialize)(self.handle.0) })?;
        let level = c"warn";
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
        ] {
            // SAFETY: valid handle, static C string.
            self.check(unsafe { (self.api.observe_property)(self.handle.0, id, name.as_ptr(), format) })?;
        }
        Ok(())
    }

    fn set_option(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        let (n, v) = (cstring(name)?, cstring(value)?);
        // SAFETY: valid handle and NUL-terminated strings.
        let rc = unsafe { (self.api.set_option_string)(self.handle.0, n.as_ptr(), v.as_ptr()) };
        self.check(rc).map_err(|e| PlayerError::Mpv(format!("option {name}={value}: {e}")))
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
        let owned = args.iter().map(|a| cstring(a)).collect::<Result<Vec<_>, _>>()?;
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
            *t = Timing { load_started: Some(Instant::now()), seek_started: None };
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
        self.mark_seek();
        self.command(&["seek", &format!("{seconds:.3}"), "relative"])
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
        let mut params = [
            sys::mpv_render_param { kind: sys::RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            sys::mpv_render_param {
                kind: sys::RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&raw mut init).cast(),
            },
            sys::mpv_render_param { kind: sys::RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let mut ctx: *mut sys::mpv_render_context = std::ptr::null_mut();
        // SAFETY: valid handle; params are a terminated array living across the call.
        self.check(unsafe { (self.api.render_context_create)(&mut ctx, self.handle.0, params.as_mut_ptr()) })?;
        let callback = Box::into_raw(Box::new(on_update));
        // SAFETY: ctx is valid; callback stays alive until render_free.
        unsafe {
            (self.api.render_context_set_update_callback)(ctx, Some(render::on_update), callback.cast());
        }
        *guard = Some(render::Renderer { ctx, callback });
        Ok(())
    }

    /// Renders the current frame into `fbo` (`width`×`height` pixels).
    /// Call from the layer's draw method with its CGL context current.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), PlayerError> {
        let guard = self.renderer.lock().unwrap_or_else(|e| e.into_inner());
        let Some(renderer) = guard.as_ref() else { return Ok(()) };
        let mut target = sys::mpv_opengl_fbo { fbo, w: width, h: height, internal_format: 0 };
        let mut flip: c_int = 1;
        let mut params = [
            sys::mpv_render_param { kind: sys::RENDER_PARAM_OPENGL_FBO, data: (&raw mut target).cast() },
            sys::mpv_render_param { kind: sys::RENDER_PARAM_FLIP_Y, data: (&raw mut flip).cast() },
            sys::mpv_render_param { kind: sys::RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        // SAFETY: renderer.ctx is live (guarded); params outlive the call.
        self.check(unsafe { (self.api.render_context_render)(renderer.ctx, params.as_mut_ptr()) })
    }

    /// Frees the render context. Must run with the CGL context current and
    /// before [`Player::shutdown`].
    pub fn render_free(&self) {
        let renderer = self.renderer.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(renderer) = renderer {
            // SAFETY: ctx is live; after free mpv no longer calls the callback.
            unsafe {
                (self.api.render_context_free)(renderer.ctx);
                drop(Box::from_raw(renderer.callback));
            }
        }
    }

    /// Output transfer/primaries for EDR: `Some(("pq", "bt.2020"))` for HDR,
    /// `None` to let mpv pick (SDR).
    pub fn set_output_colorspace(&self, target: Option<(&str, &str)>) -> Result<(), PlayerError> {
        let (trc, prim) = target.unwrap_or(("auto", "auto"));
        self.command(&["set", "target-trc", trc])?;
        self.command(&["set", "target-prim", prim])
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
                        events.on_event(PlayerEvent::VideoGamma(value.to_string_lossy().into_owned()));
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
