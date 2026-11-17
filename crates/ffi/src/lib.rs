//! The only crate Swift sees. Thin UniFFI wrappers over `cinma-core`;
//! no business logic lives here.

use std::path::PathBuf;
use std::sync::Arc;

use cinma_core::{Core, CoreError, EndReason, PlayerEvent, PlayerEvents};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CinmaError {
    #[error("{message}")]
    Database { message: String },
    #[error("{message}")]
    Io { message: String },
    #[error("{message}")]
    Network { message: String },
    #[error("{message}")]
    Torrent { message: String },
    #[error("{message}")]
    Player { message: String },
    #[error("{message}")]
    Invalid { message: String },
    #[error("{message}")]
    Internal { message: String },
}

impl From<CoreError> for CinmaError {
    fn from(error: CoreError) -> Self {
        let message = error.to_string();
        match error {
            CoreError::Database(_) => Self::Database { message },
            CoreError::Io(_) => Self::Io { message },
            CoreError::Addon(_) => Self::Network { message },
            CoreError::Torrent(_) => Self::Torrent { message },
            CoreError::Player(_) => Self::Player { message },
            CoreError::Invalid(_) | CoreError::FeatureDisabled(_) => Self::Invalid { message },
            CoreError::Poisoned | CoreError::Task(_) => Self::Internal { message },
        }
    }
}

/// Version of the Rust core.
#[uniffi::export]
pub fn core_version() -> String {
    cinma_core::VERSION.to_string()
}

#[derive(uniffi::Record)]
pub struct AddonInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub transport_url: String,
    pub types: Vec<String>,
    /// Enabled for the active profile.
    pub enabled: bool,
}

impl From<cinma_core::InstalledAddon> for AddonInfo {
    fn from(a: cinma_core::InstalledAddon) -> Self {
        Self {
            id: a.manifest.id,
            name: a.manifest.name,
            version: a.manifest.version,
            description: a.manifest.description,
            transport_url: a.transport_url,
            types: a.manifest.types,
            enabled: a.enabled,
        }
    }
}

#[derive(uniffi::Record)]
pub struct StreamInfo {
    pub addon_name: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub info_hash: Option<String>,
    pub file_idx: Option<u32>,
    pub url: Option<String>,
    pub filename: Option<String>,
    pub video_size: Option<u64>,
    /// Opaque: the original stream object, handed back to `play`.
    pub raw: String,
}

impl From<cinma_core::StreamCandidate> for StreamInfo {
    fn from(c: cinma_core::StreamCandidate) -> Self {
        let s = c.stream;
        let hints = s.behavior_hints.clone().unwrap_or_default();
        Self {
            addon_name: c.addon_name,
            name: s.name.clone(),
            title: s.display_title().map(str::to_owned),
            info_hash: s.info_hash.clone(),
            file_idx: s.file_idx,
            url: s.url.clone(),
            filename: hints.filename,
            video_size: hints.video_size,
            raw: serde_json::to_string(&s).unwrap_or_default(),
        }
    }
}

#[derive(uniffi::Enum)]
pub enum PlaybackEndReason {
    Eof,
    Stopped,
    Error,
}

#[derive(uniffi::Enum)]
pub enum PlaybackEvent {
    FileLoaded,
    PlaybackRestart,
    Position { seconds: f64 },
    Duration { seconds: f64 },
    Paused { paused: bool },
    Buffering { buffering: bool },
    VideoGamma { gamma: String },
    Metric { name: String, millis: f64 },
    EndFile { reason: PlaybackEndReason, error: Option<String> },
    Shutdown,
}

impl From<PlayerEvent> for PlaybackEvent {
    fn from(e: PlayerEvent) -> Self {
        match e {
            PlayerEvent::FileLoaded => Self::FileLoaded,
            PlayerEvent::PlaybackRestart => Self::PlaybackRestart,
            PlayerEvent::Position { seconds } => Self::Position { seconds },
            PlayerEvent::Duration { seconds } => Self::Duration { seconds },
            PlayerEvent::Paused(paused) => Self::Paused { paused },
            PlayerEvent::Buffering(buffering) => Self::Buffering { buffering },
            PlayerEvent::VideoGamma(gamma) => Self::VideoGamma { gamma },
            PlayerEvent::Metric { name, millis } => Self::Metric { name: name.to_owned(), millis },
            PlayerEvent::EndFile { reason, error } => Self::EndFile {
                reason: match reason {
                    EndReason::Eof => PlaybackEndReason::Eof,
                    EndReason::Stopped => PlaybackEndReason::Stopped,
                    EndReason::Error => PlaybackEndReason::Error,
                },
                error,
            },
            PlayerEvent::Shutdown => Self::Shutdown,
        }
    }
}

/// Implemented in Swift. Called on a background thread; must not block.
#[uniffi::export(with_foreign)]
pub trait PlaybackObserver: Send + Sync {
    fn on_event(&self, event: PlaybackEvent);
}

/// Implemented in Swift: asks the video layer to redraw. Called on an mpv
/// thread for every new frame; must not block.
#[uniffi::export(with_foreign)]
pub trait RenderObserver: Send + Sync {
    fn needs_display(&self);
}

struct ObserverAdapter(Arc<dyn PlaybackObserver>);

impl PlayerEvents for ObserverAdapter {
    fn on_event(&self, event: PlayerEvent) {
        self.0.on_event(event.into());
    }
}

#[derive(uniffi::Object)]
pub struct CinmaCore {
    inner: Arc<Core>,
}

#[uniffi::export]
impl CinmaCore {
    /// Opens the core. Blocking (touches disk) — call off the main thread.
    #[uniffi::constructor]
    pub fn open(data_dir: String) -> Result<Arc<Self>, CinmaError> {
        let inner = Core::open(&PathBuf::from(data_dir))?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn schema_version(&self) -> Result<u32, CinmaError> {
        Ok(self.inner.schema_version()?)
    }

    pub fn addons(&self) -> Result<Vec<AddonInfo>, CinmaError> {
        Ok(self.inner.addons()?.into_iter().map(Into::into).collect())
    }

    /// Installs from a manifest URL or `stremio://` link.
    pub async fn install_addon(&self, url: String) -> Result<AddonInfo, CinmaError> {
        Ok(self.inner.install_addon(url).await?.into())
    }

    pub fn remove_addon(&self, id: String) -> Result<(), CinmaError> {
        Ok(self.inner.remove_addon(&id)?)
    }

    /// `media_type`: "movie" | "series"; `id`: e.g. "tt1254207" or "tt0944947:1:1".
    pub async fn streams(&self, media_type: String, id: String) -> Result<Vec<StreamInfo>, CinmaError> {
        Ok(self
            .inner
            .streams(media_type, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Creates a player. Blocking (loads libmpv on first use) — call off the
    /// main thread, then attach a video layer with `render_init`.
    pub fn create_playback(&self, observer: Arc<dyn PlaybackObserver>) -> Result<Arc<PlaybackSession>, CinmaError> {
        let inner = self.inner.create_playback(Arc::new(ObserverAdapter(observer)))?;
        Ok(Arc::new(PlaybackSession { inner }))
    }
}

#[derive(uniffi::Object)]
pub struct PlaybackSession {
    inner: cinma_core::Playback,
}

#[uniffi::export]
impl PlaybackSession {
    pub async fn play(&self, stream: StreamInfo) -> Result<(), CinmaError> {
        let stream: cinma_stremio::Stream = serde_json::from_str(&stream.raw)
            .map_err(|e| CinmaError::Invalid { message: e.to_string() })?;
        Ok(self.inner.play(stream).await?)
    }

    /// Plays a local file or URL directly (debug / render tests).
    pub fn play_url(&self, url: String) -> Result<(), CinmaError> {
        Ok(self.inner.player().load_url(&url).map_err(CoreError::from)?)
    }

    pub fn toggle_pause(&self) -> Result<(), CinmaError> {
        Ok(self.inner.player().toggle_pause().map_err(CoreError::from)?)
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self.inner.player().seek_absolute(seconds).map_err(CoreError::from)?)
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self.inner.player().seek_relative(seconds).map_err(CoreError::from)?)
    }

    /// Creates the OpenGL render context. Call with the video layer's CGL
    /// context current (e.g. inside `CAOpenGLLayer.draw`).
    pub fn render_init(&self, observer: Arc<dyn RenderObserver>) -> Result<(), CinmaError> {
        let callback: cinma_core::UpdateCallback = Arc::new(move || observer.needs_display());
        Ok(self.inner.player().render_init(callback).map_err(CoreError::from)?)
    }

    /// Draws the current frame into `fbo`. CGL context must be current.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), CinmaError> {
        Ok(self.inner.player().render(fbo, width, height).map_err(CoreError::from)?)
    }

    /// Frees the render context. CGL context must be current. Call before `stop`.
    pub fn render_free(&self) {
        self.inner.player().render_free();
    }

    /// Destroys mpv and stops torrent traffic. Call `render_free` first.
    pub async fn stop(&self) -> Result<(), CinmaError> {
        Ok(self.inner.stop().await?)
    }
}
