//! The only crate Swift sees. Thin UniFFI wrappers over `cinma-core`;
//! no business logic lives here.

use std::path::PathBuf;
use std::sync::Arc;

use cinma_core::{CommandContext, Core, CoreError, CoreEvent, EndReason, Feature, PlayerEvent, PlayerEvents};

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
pub struct CommandInfo {
    pub id: String,
    /// "global" | "player"
    pub context: String,
    /// Canonical combo, e.g. "cmd+shift+z", "space"; `None` = unbound.
    pub key: Option<String>,
}

/// Commands with their default bindings. Needs no core (launch path).
#[uniffi::export]
pub fn default_commands() -> Vec<CommandInfo> {
    cinma_core::COMMANDS
        .iter()
        .map(|c| CommandInfo {
            id: c.id.to_owned(),
            context: c.context.name().to_owned(),
            key: c.default_key.map(str::to_owned),
        })
        .collect()
}

#[derive(uniffi::Record)]
pub struct CommandInvocation {
    pub command: String,
    pub argument: Option<String>,
}

/// Maps a URL the app was opened with to a command (e.g. `stremio://…`).
#[uniffi::export]
pub fn parse_deep_link(url: String) -> Option<CommandInvocation> {
    cinma_core::parse_deep_link(&url).map(|i| CommandInvocation {
        command: i.command.to_owned(),
        argument: i.argument,
    })
}

#[derive(uniffi::Enum)]
pub enum CoreEventKind {
    AddonsChanged,
    KeyBindingsChanged,
}

/// Implemented in Swift. Called on whichever thread raised the event.
#[uniffi::export(with_foreign)]
pub trait CoreEventListener: Send + Sync {
    fn on_event(&self, event: CoreEventKind);
}

struct EventAdapter(Arc<dyn CoreEventListener>);

impl cinma_core::CoreEventListener for EventAdapter {
    fn on_event(&self, event: CoreEvent) {
        self.0.on_event(match event {
            CoreEvent::AddonsChanged => CoreEventKind::AddonsChanged,
            CoreEvent::KeyBindingsChanged => CoreEventKind::KeyBindingsChanged,
        });
    }
}

fn feature(name: &str) -> Result<Feature, CinmaError> {
    Feature::from_name(name).ok_or_else(|| CinmaError::Invalid { message: format!("unknown feature {name}") })
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
    pub configurable: bool,
}

impl From<cinma_core::InstalledAddon> for AddonInfo {
    fn from(a: cinma_core::InstalledAddon) -> Self {
        let configurable = a
            .manifest
            .behavior_hints
            .as_ref()
            .and_then(|h| h.configurable)
            .unwrap_or(false);
        Self {
            id: a.manifest.id,
            name: a.manifest.name,
            version: a.manifest.version,
            description: a.manifest.description,
            transport_url: a.transport_url,
            types: a.manifest.types,
            enabled: a.enabled,
            configurable,
        }
    }
}

#[derive(uniffi::Record)]
pub struct CatalogInfo {
    pub addon_id: String,
    pub addon_name: String,
    pub media_type: String,
    pub id: String,
    pub name: String,
    pub genres: Vec<String>,
    pub is_searchable: bool,
    pub is_browsable: bool,
    pub supports_skip: bool,
}

impl From<cinma_core::AddonCatalog> for CatalogInfo {
    fn from(c: cinma_core::AddonCatalog) -> Self {
        Self {
            addon_id: c.addon_id,
            addon_name: c.addon_name,
            media_type: c.catalog.media_type.clone(),
            id: c.catalog.id.clone(),
            name: c.catalog.display_name().to_owned(),
            genres: c.catalog.genres().to_vec(),
            is_searchable: c.catalog.is_searchable(),
            is_browsable: c.catalog.is_browsable(),
            supports_skip: c.catalog.supports_extra("skip"),
        }
    }
}

#[derive(uniffi::Record)]
pub struct CatalogQueryInput {
    pub search: Option<String>,
    pub genre: Option<String>,
    pub skip: Option<u32>,
}

impl From<CatalogQueryInput> for cinma_core::CatalogQuery {
    fn from(q: CatalogQueryInput) -> Self {
        Self {
            search: q.search,
            genre: q.genre,
            skip: q.skip,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MetaPreviewInfo {
    pub id: String,
    pub media_type: String,
    pub name: String,
    pub poster: Option<String>,
    pub poster_shape: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    pub description: Option<String>,
    pub release_info: Option<String>,
    pub imdb_rating: Option<String>,
    pub genres: Vec<String>,
    pub runtime: Option<String>,
}

impl From<cinma_core::MetaPreview> for MetaPreviewInfo {
    fn from(m: cinma_core::MetaPreview) -> Self {
        Self {
            id: m.id,
            media_type: m.media_type,
            name: m.name,
            poster: m.poster,
            poster_shape: m.poster_shape,
            background: m.background,
            logo: m.logo,
            description: m.description,
            release_info: m.release_info,
            imdb_rating: m.imdb_rating,
            genres: m.genres,
            runtime: m.runtime,
        }
    }
}

#[derive(uniffi::Record)]
pub struct CatalogPageInfo {
    pub addon_id: String,
    pub addon_name: String,
    pub media_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub items: Vec<MetaPreviewInfo>,
    pub has_more: bool,
}

impl From<cinma_core::CatalogPage> for CatalogPageInfo {
    fn from(p: cinma_core::CatalogPage) -> Self {
        Self {
            addon_id: p.addon_id,
            addon_name: p.addon_name,
            media_type: p.media_type,
            catalog_id: p.catalog_id,
            catalog_name: p.catalog_name,
            items: p.items.into_iter().map(Into::into).collect(),
            has_more: p.has_more,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MetaVideoInfo {
    pub id: String,
    pub title: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub number: Option<u32>,
    pub released: Option<String>,
    pub overview: Option<String>,
    pub thumbnail: Option<String>,
}

impl From<cinma_core::MetaVideo> for MetaVideoInfo {
    fn from(v: cinma_core::MetaVideo) -> Self {
        let title = v.display_title().map(str::to_owned);
        Self {
            id: v.id,
            title,
            season: v.season,
            episode: v.episode,
            number: v.number,
            released: v.released,
            overview: v.overview,
            thumbnail: v.thumbnail,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MetaTrailerInfo {
    pub source: String,
    pub trailer_type: Option<String>,
}

impl From<cinma_core::MetaTrailer> for MetaTrailerInfo {
    fn from(t: cinma_core::MetaTrailer) -> Self {
        Self {
            source: t.source,
            trailer_type: t.trailer_type,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MetaInfo {
    pub addon_id: String,
    pub addon_name: String,
    pub id: String,
    pub media_type: String,
    pub name: String,
    pub poster: Option<String>,
    pub poster_shape: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    pub description: Option<String>,
    pub release_info: Option<String>,
    pub imdb_rating: Option<String>,
    pub runtime: Option<String>,
    pub country: Option<String>,
    pub awards: Option<String>,
    pub released: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub director: Vec<String>,
    pub writer: Vec<String>,
    pub videos: Vec<MetaVideoInfo>,
    pub trailers: Vec<MetaTrailerInfo>,
    pub default_video_id: Option<String>,
}

impl From<cinma_core::MetaResult> for MetaInfo {
    fn from(r: cinma_core::MetaResult) -> Self {
        let m = r.meta;
        Self {
            addon_id: r.addon_id,
            addon_name: r.addon_name,
            id: m.id,
            media_type: m.media_type,
            name: m.name,
            poster: m.poster,
            poster_shape: m.poster_shape,
            background: m.background,
            logo: m.logo,
            description: m.description,
            release_info: m.release_info,
            imdb_rating: m.imdb_rating,
            runtime: m.runtime,
            country: m.country,
            awards: m.awards,
            released: m.released,
            genres: m.genres,
            cast: m.cast,
            director: m.director,
            writer: m.writer,
            videos: m.videos.into_iter().map(Into::into).collect(),
            trailers: m.trailers.into_iter().map(Into::into).collect(),
            default_video_id: m.behavior_hints.and_then(|b| b.default_video_id),
        }
    }
}

#[derive(uniffi::Record)]
pub struct StreamBatchInfo {
    pub addon_id: String,
    pub addon_name: String,
    pub streams: Vec<StreamInfo>,
    pub error: Option<String>,
}

#[uniffi::export(with_foreign)]
pub trait StreamBatchObserver: Send + Sync {
    fn on_batch(&self, batch: StreamBatchInfo);
}

struct StreamBatchAdapter(Arc<dyn StreamBatchObserver>);

impl cinma_core::StreamBatchListener for StreamBatchAdapter {
    fn on_batch(&self, batch: cinma_core::StreamBatch) {
        self.0.on_batch(StreamBatchInfo {
            addon_id: batch.addon_id,
            addon_name: batch.addon_name,
            streams: batch.streams.into_iter().map(Into::into).collect(),
            error: batch.error,
        });
    }
}

#[derive(uniffi::Record)]
pub struct CatalogBatchInfo {
    pub addon_id: String,
    pub addon_name: String,
    pub media_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub items: Vec<MetaPreviewInfo>,
    pub has_more: bool,
    pub error: Option<String>,
}

#[uniffi::export(with_foreign)]
pub trait CatalogBatchObserver: Send + Sync {
    fn on_batch(&self, batch: CatalogBatchInfo);
}

struct CatalogBatchAdapter(Arc<dyn CatalogBatchObserver>);

impl cinma_core::CatalogBatchListener for CatalogBatchAdapter {
    fn on_batch(&self, batch: cinma_core::CatalogBatch) {
        self.0.on_batch(CatalogBatchInfo {
            addon_id: batch.addon_id,
            addon_name: batch.addon_name,
            media_type: batch.media_type,
            catalog_id: batch.catalog_id,
            catalog_name: batch.catalog_name,
            items: batch.items.into_iter().map(Into::into).collect(),
            has_more: batch.has_more,
            error: batch.error,
        });
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
pub enum RankTier {
    Best,
    Recommended,
    Good,
    Fallback,
}

impl From<cinma_core::StreamTier> for RankTier {
    fn from(t: cinma_core::StreamTier) -> Self {
        match t {
            cinma_core::StreamTier::Best => Self::Best,
            cinma_core::StreamTier::Recommended => Self::Recommended,
            cinma_core::StreamTier::Good => Self::Good,
            cinma_core::StreamTier::Fallback => Self::Fallback,
        }
    }
}

#[derive(uniffi::Enum)]
pub enum RankSafetyLevel {
    Strict,
    Standard,
    Permissive,
}

impl From<RankSafetyLevel> for cinma_core::SafetyLevel {
    fn from(s: RankSafetyLevel) -> Self {
        match s {
            RankSafetyLevel::Strict => Self::Strict,
            RankSafetyLevel::Standard => Self::Standard,
            RankSafetyLevel::Permissive => Self::Permissive,
        }
    }
}

#[derive(uniffi::Record)]
pub struct RankPreferencesInput {
    /// "2160p" | "1080p" | "720p" | "480p"
    pub max_resolution: String,
    pub prefer_hdr: bool,
    pub prefer_languages: Vec<String>,
    pub safety_level: RankSafetyLevel,
    pub min_seeders: u32,
}

impl TryFrom<RankPreferencesInput> for cinma_core::RankerPreferences {
    type Error = CinmaError;

    fn try_from(p: RankPreferencesInput) -> Result<Self, Self::Error> {
        let max_resolution = match p.max_resolution.to_ascii_lowercase().as_str() {
            "2160p" | "4k" => cinma_core::Resolution::Uhd2160,
            "1080p" => cinma_core::Resolution::Fhd1080,
            "720p" => cinma_core::Resolution::Hd720,
            "480p" => cinma_core::Resolution::Sd480,
            other => {
                return Err(CinmaError::Invalid {
                    message: format!("unknown max_resolution `{other}`"),
                });
            }
        };
        Ok(Self {
            max_resolution,
            prefer_hdr: p.prefer_hdr,
            prefer_languages: p.prefer_languages,
            safety_level: p.safety_level.into(),
            min_seeders: p.min_seeders,
        })
    }
}

#[derive(uniffi::Record)]
pub struct ScoreReasonInfo {
    pub code: String,
    pub detail: Option<String>,
    pub points: i32,
}

impl From<cinma_core::ScoreReason> for ScoreReasonInfo {
    fn from(r: cinma_core::ScoreReason) -> Self {
        Self { code: r.code, detail: r.detail, points: r.points }
    }
}

#[derive(uniffi::Record)]
pub struct ParsedStreamInfo {
    pub resolution: String,
    pub hdr_badges: Vec<String>,
    pub codec: String,
    pub audio: String,
    pub source: String,
    pub seeders: Option<u32>,
    pub size_bytes: Option<u64>,
    pub release_group: Option<String>,
    pub languages: Vec<String>,
    pub is_torrent: bool,
    pub cached_debrid: bool,
}

impl From<cinma_core::ParsedStream> for ParsedStreamInfo {
    fn from(p: cinma_core::ParsedStream) -> Self {
        Self {
            resolution: p.resolution.label().to_owned(),
            hdr_badges: p.hdr.badges().into_iter().map(str::to_owned).collect(),
            codec: p.codec.label().to_owned(),
            audio: p.audio.label().to_owned(),
            source: p.source.label().to_owned(),
            seeders: p.seeders,
            size_bytes: p.size_bytes,
            release_group: p.release_group,
            languages: p.languages,
            is_torrent: p.is_torrent,
            cached_debrid: p.cached_debrid,
        }
    }
}

#[derive(uniffi::Record)]
pub struct RankedStreamInfo {
    pub stream: StreamInfo,
    pub parsed: ParsedStreamInfo,
    pub score: i32,
    pub tier: RankTier,
    pub reasons: Vec<ScoreReasonInfo>,
}

impl From<cinma_core::RankedStream> for RankedStreamInfo {
    fn from(r: cinma_core::RankedStream) -> Self {
        Self {
            stream: cinma_core::StreamCandidate {
                addon_name: r.addon_name,
                stream: r.stream,
            }
            .into(),
            parsed: r.parsed.into(),
            score: r.score,
            tier: r.tier.into(),
            reasons: r.reasons.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct FilteredStreamInfo {
    pub stream: StreamInfo,
    pub reason: String,
}

impl From<cinma_core::FilteredOutStream> for FilteredStreamInfo {
    fn from(f: cinma_core::FilteredOutStream) -> Self {
        let reason = match f.reason {
            cinma_core::FilterReason::FakeOrMalicious => "fake_or_malicious",
            cinma_core::FilterReason::CamOrTelesync => "cam_or_telesync",
            cinma_core::FilterReason::BelowMinSeeders => "below_min_seeders",
            cinma_core::FilterReason::ExceedsMaxResolution => "exceeds_max_resolution",
            cinma_core::FilterReason::NoPlayableSource => "no_playable_source",
        };
        Self {
            stream: cinma_core::StreamCandidate {
                addon_name: f.addon_name,
                stream: f.stream,
            }
            .into(),
            reason: reason.to_owned(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct RankResultInfo {
    pub ranked: Vec<RankedStreamInfo>,
    pub filtered: Vec<FilteredStreamInfo>,
}

impl From<cinma_core::RankResult> for RankResultInfo {
    fn from(r: cinma_core::RankResult) -> Self {
        Self {
            ranked: r.ranked.into_iter().map(Into::into).collect(),
            filtered: r.filtered.into_iter().map(Into::into).collect(),
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

    /// Fetches an addon manifest without installing it (e.g. deep-link confirmation).
    pub async fn inspect_addon(&self, url: String) -> Result<AddonInfo, CinmaError> {
        Ok(self.inner.inspect_addon(url).await?.into())
    }

    /// Installs from a manifest URL or `stremio://` link.
    pub async fn install_addon(&self, url: String) -> Result<AddonInfo, CinmaError> {
        Ok(self.inner.install_addon(url).await?.into())
    }

    pub fn remove_addon(&self, id: String) -> Result<(), CinmaError> {
        Ok(self.inner.remove_addon(&id)?)
    }

    /// Enables/disables an installed addon for the active profile.
    pub fn set_addon_enabled(&self, id: String, enabled: bool) -> Result<(), CinmaError> {
        Ok(self.inner.set_addon_enabled(&id, enabled)?)
    }

    pub fn subscribe(&self, listener: Arc<dyn CoreEventListener>) -> Result<(), CinmaError> {
        Ok(self.inner.subscribe(Arc::new(EventAdapter(listener)))?)
    }

    /// Commands with effective key bindings (user overrides applied).
    pub fn commands(&self) -> Result<Vec<CommandInfo>, CinmaError> {
        Ok(self
            .inner
            .key_bindings()?
            .into_iter()
            .map(|(spec, key)| CommandInfo { id: spec.id.to_owned(), context: spec.context.name().to_owned(), key })
            .collect())
    }

    /// Command bound to `combo` in `context` ("global" | "player").
    pub fn resolve_key(&self, context: String, combo: String) -> Result<Option<String>, CinmaError> {
        let context = CommandContext::from_name(&context)
            .ok_or_else(|| CinmaError::Invalid { message: format!("unknown context {context}") })?;
        Ok(self.inner.resolve_key(context, &combo)?.map(str::to_owned))
    }

    /// Rebinds a command; `combo = None` unbinds it.
    pub fn set_key_binding(&self, id: String, combo: Option<String>) -> Result<(), CinmaError> {
        Ok(self.inner.set_key_binding(&id, combo.as_deref())?)
    }

    pub fn reset_key_binding(&self, id: String) -> Result<(), CinmaError> {
        Ok(self.inner.reset_key_binding(&id)?)
    }

    /// Feature names: "addons", "torrent".
    pub fn feature_enabled(&self, name: String) -> Result<bool, CinmaError> {
        Ok(self.inner.feature_enabled(feature(&name)?)?)
    }

    pub async fn set_feature_enabled(&self, name: String, enabled: bool) -> Result<(), CinmaError> {
        Ok(self.inner.set_feature_enabled(feature(&name)?, enabled).await?)
    }

    pub fn catalogs(&self) -> Result<Vec<CatalogInfo>, CinmaError> {
        Ok(self.inner.catalogs()?.into_iter().map(Into::into).collect())
    }

    pub async fn catalog(
        &self,
        addon_id: String,
        media_type: String,
        catalog_id: String,
        query: CatalogQueryInput,
    ) -> Result<CatalogPageInfo, CinmaError> {
        Ok(self
            .inner
            .catalog(addon_id, media_type, catalog_id, query.into())
            .await?
            .into())
    }

    pub async fn browse_catalogs(
        &self,
        media_type: Option<String>,
        observer: Arc<dyn CatalogBatchObserver>,
    ) -> Result<Vec<CatalogPageInfo>, CinmaError> {
        Ok(self
            .inner
            .browse_catalogs(media_type, Some(Arc::new(CatalogBatchAdapter(observer))))
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn search_catalogs(
        &self,
        query: String,
        media_type: Option<String>,
        observer: Arc<dyn CatalogBatchObserver>,
    ) -> Result<Vec<CatalogPageInfo>, CinmaError> {
        Ok(self
            .inner
            .search_catalogs(query, media_type, Some(Arc::new(CatalogBatchAdapter(observer))))
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn meta(&self, media_type: String, id: String) -> Result<MetaInfo, CinmaError> {
        Ok(self.inner.meta(media_type, id).await?.into())
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

    pub async fn streams_incremental(
        &self,
        media_type: String,
        id: String,
        observer: Arc<dyn StreamBatchObserver>,
    ) -> Result<Vec<StreamInfo>, CinmaError> {
        Ok(self
            .inner
            .streams_streaming(media_type, id, Some(Arc::new(StreamBatchAdapter(observer))))
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn hardware_av1_supported(&self) -> bool {
        self.inner.hardware_av1_supported()
    }

    pub fn rank_streams(
        &self,
        streams: Vec<StreamInfo>,
        preferences: Option<RankPreferencesInput>,
    ) -> Result<RankResultInfo, CinmaError> {
        let prefs = preferences.map(TryInto::try_into).transpose()?;
        let candidates = streams
            .into_iter()
            .map(|s| {
                let stream: cinma_stremio::Stream = serde_json::from_str(&s.raw)
                    .map_err(|e| CinmaError::Invalid { message: e.to_string() })?;
                Ok(cinma_core::StreamCandidate { addon_name: s.addon_name, stream })
            })
            .collect::<Result<Vec<_>, CinmaError>>()?;
        Ok(self.inner.rank_streams(candidates, prefs).into())
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
