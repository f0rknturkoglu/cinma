//! The only crate Swift sees. Thin UniFFI wrappers over `cinma-core`;
//! no business logic lives here.

use std::path::PathBuf;
use std::sync::Arc;

use cinma_core::{
    CommandContext, Core, CoreError, CoreEvent, EndReason, Feature, PlayerEvent, PlayerEvents,
};

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
            CoreError::Addon(_) | CoreError::Metadata(_) => Self::Network { message },
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
    ProfileChanged,
    LibraryChanged,
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
            CoreEvent::ProfileChanged => CoreEventKind::ProfileChanged,
            CoreEvent::LibraryChanged => CoreEventKind::LibraryChanged,
        });
    }
}

fn feature(name: &str) -> Result<Feature, CinmaError> {
    Feature::from_name(name).ok_or_else(|| CinmaError::Invalid {
        message: format!("unknown feature {name}"),
    })
}

#[derive(uniffi::Record)]
pub struct ProfileInfo {
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub has_pin: bool,
}

impl From<cinma_core::ProfileRow> for ProfileInfo {
    fn from(p: cinma_core::ProfileRow) -> Self {
        Self {
            id: p.id,
            name: p.name,
            avatar: p.avatar,
            has_pin: p.has_pin,
        }
    }
}

#[derive(uniffi::Record)]
pub struct LibraryItemInfo {
    pub meta_id: String,
    pub media_type: String,
    pub name: String,
    pub poster: Option<String>,
    pub added_at: i64,
}

impl From<cinma_core::LibraryItemRow> for LibraryItemInfo {
    fn from(r: cinma_core::LibraryItemRow) -> Self {
        Self {
            meta_id: r.meta_id,
            media_type: r.media_type,
            name: r.name,
            poster: r.poster,
            added_at: r.added_at,
        }
    }
}

#[derive(uniffi::Record)]
pub struct WatchProgressInfo {
    pub video_id: String,
    pub meta_id: String,
    pub media_type: String,
    pub title: String,
    pub artwork: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub position_s: f64,
    pub duration_s: f64,
    pub watched: bool,
    pub updated_at: i64,
}

impl From<cinma_core::WatchProgressRow> for WatchProgressInfo {
    fn from(r: cinma_core::WatchProgressRow) -> Self {
        Self {
            video_id: r.video_id,
            meta_id: r.meta_id,
            media_type: r.media_type,
            title: r.title,
            artwork: r.artwork,
            season: r.season,
            episode: r.episode,
            position_s: r.position_s,
            duration_s: r.duration_s,
            watched: r.watched,
            updated_at: r.updated_at,
        }
    }
}

#[derive(uniffi::Record)]
pub struct WatchProgressInputFfi {
    pub video_id: String,
    pub meta_id: String,
    pub media_type: String,
    pub title: String,
    pub artwork: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub position_s: f64,
    pub duration_s: f64,
}

#[derive(uniffi::Record)]
pub struct UpcomingEpisodeInfo {
    pub show_id: String,
    pub show_name: String,
    pub show_poster: Option<String>,
    pub season: u32,
    pub episode: u32,
    pub episode_name: String,
    pub air_date: String,
    pub overview: Option<String>,
    pub still: Option<String>,
}

impl From<cinma_core::UpcomingEpisode> for UpcomingEpisodeInfo {
    fn from(u: cinma_core::UpcomingEpisode) -> Self {
        Self {
            show_id: u.show_id,
            show_name: u.show_name,
            show_poster: u.show_poster,
            season: u.season,
            episode: u.episode,
            episode_name: u.episode_name,
            air_date: u.air_date,
            overview: u.overview,
            still: u.still,
        }
    }
}

#[derive(uniffi::Record)]
pub struct RichMetaInfo {
    pub source: String,
    pub meta: MetaInfo,
    pub season_numbers: Vec<u32>,
    pub next_episode: Option<UpcomingEpisodeInfo>,
}

impl From<cinma_core::RichMetaResult> for RichMetaInfo {
    fn from(r: cinma_core::RichMetaResult) -> Self {
        let source = r.source.clone();
        let meta_res = cinma_core::MetaResult {
            addon_id: r.source,
            addon_name: source.clone(),
            meta: r.meta,
        };
        Self {
            source,
            meta: meta_res.into(),
            season_numbers: r.season_numbers,
            next_episode: r.next_episode.map(Into::into),
        }
    }
}

#[derive(uniffi::Record)]
pub struct YtdlpStatusInfo {
    pub available: bool,
    pub path: Option<String>,
    pub managed: bool,
}

impl From<cinma_core::YtdlpStatus> for YtdlpStatusInfo {
    fn from(s: cinma_core::YtdlpStatus) -> Self {
        Self {
            available: s.available,
            path: s.path,
            managed: s.managed,
        }
    }
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
        Self {
            code: r.code,
            detail: r.detail,
            points: r.points,
        }
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
    Position {
        seconds: f64,
    },
    Duration {
        seconds: f64,
    },
    Paused {
        paused: bool,
    },
    Buffering {
        buffering: bool,
    },
    VideoGamma {
        gamma: String,
    },
    Metric {
        name: String,
        millis: f64,
    },
    TracksChanged,
    ChaptersChanged,
    EndFile {
        reason: PlaybackEndReason,
        error: Option<String>,
    },
    Shutdown,
}

#[derive(uniffi::Record)]
pub struct MediaTrackInfo {
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

impl From<cinma_core::MediaTrack> for MediaTrackInfo {
    fn from(t: cinma_core::MediaTrack) -> Self {
        Self {
            id: t.id,
            track_type: t.track_type,
            title: t.title,
            lang: t.lang,
            codec: t.codec,
            selected: t.selected,
            external: t.external,
            forced: t.forced,
            hearing_impaired: t.hearing_impaired,
        }
    }
}

#[derive(uniffi::Record)]
pub struct ChapterInfo {
    pub title: String,
    pub start_s: f64,
}

impl From<cinma_core::Chapter> for ChapterInfo {
    fn from(c: cinma_core::Chapter) -> Self {
        Self {
            title: c.title,
            start_s: c.start_s,
        }
    }
}

#[derive(uniffi::Record)]
pub struct PlayerStatsInfo {
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

impl From<cinma_core::PlayerStats> for PlayerStatsInfo {
    fn from(s: cinma_core::PlayerStats) -> Self {
        Self {
            video_codec: s.video_codec,
            width: s.width,
            height: s.height,
            fps: s.fps,
            estimated_fps: s.estimated_fps,
            hwdec: s.hwdec,
            gamma: s.gamma,
            audio_codec: s.audio_codec,
            audio_channels: s.audio_channels,
            cache_seconds: s.cache_seconds,
            dropped_frames: s.dropped_frames,
            ab_loop_a: s.ab_loop_a,
            ab_loop_b: s.ab_loop_b,
            speed: s.speed,
            volume: s.volume,
            muted: s.muted,
            sub_delay: s.sub_delay,
            audio_delay: s.audio_delay,
        }
    }
}

#[derive(uniffi::Record)]
pub struct ThumbnailImageInfo {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(uniffi::Record)]
pub struct VideoFileIdentityInfo {
    pub hash: Option<String>,
    pub size: Option<u64>,
    pub filename: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleSettingsFfi {
    pub preferred_langs: Vec<String>,
    pub audio_langs: Vec<String>,
    pub dual_secondary_lang: Option<String>,
    pub srt_font: Option<String>,
    pub srt_font_size: u32,
    pub srt_color: Option<String>,
    pub srt_pos: u32,
    pub ass_override: bool,
}

impl From<cinma_core::SubtitleSettings> for SubtitleSettingsFfi {
    fn from(s: cinma_core::SubtitleSettings) -> Self {
        Self {
            preferred_langs: s.preferred_langs,
            audio_langs: s.audio_langs,
            dual_secondary_lang: s.dual_secondary_lang,
            srt_font: s.srt_font,
            srt_font_size: s.srt_font_size,
            srt_color: s.srt_color,
            srt_pos: s.srt_pos,
            ass_override: s.ass_override,
        }
    }
}

impl From<SubtitleSettingsFfi> for cinma_core::SubtitleSettings {
    fn from(s: SubtitleSettingsFfi) -> Self {
        Self {
            preferred_langs: s.preferred_langs,
            audio_langs: s.audio_langs,
            dual_secondary_lang: s.dual_secondary_lang,
            srt_font: s.srt_font,
            srt_font_size: s.srt_font_size,
            srt_color: s.srt_color,
            srt_pos: s.srt_pos,
            ass_override: s.ass_override,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct PlayerPreferencesFfi {
    pub exact_seek: bool,
    pub hdr_target_peak: Option<u32>,
    pub shader_preset: String,
    pub autoplay_next: bool,
    pub autoplay_countdown_s: u32,
    pub force_hdr_surface: bool,
}

impl From<cinma_core::PlayerPreferences> for PlayerPreferencesFfi {
    fn from(p: cinma_core::PlayerPreferences) -> Self {
        Self {
            exact_seek: p.exact_seek,
            hdr_target_peak: p.hdr_target_peak,
            shader_preset: p.shader_preset,
            autoplay_next: p.autoplay_next,
            autoplay_countdown_s: p.autoplay_countdown_s,
            force_hdr_surface: p.force_hdr_surface,
        }
    }
}

impl From<PlayerPreferencesFfi> for cinma_core::PlayerPreferences {
    fn from(p: PlayerPreferencesFfi) -> Self {
        Self {
            exact_seek: p.exact_seek,
            hdr_target_peak: p.hdr_target_peak,
            shader_preset: p.shader_preset,
            autoplay_next: p.autoplay_next,
            autoplay_countdown_s: p.autoplay_countdown_s,
            force_hdr_surface: p.force_hdr_surface,
        }
    }
}

#[derive(uniffi::Record)]
pub struct VideoOffsetsFfi {
    pub sub_delay: f64,
    pub audio_delay: f64,
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleSearchInput {
    pub media_type: String,
    pub video_id: String,
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub video_hash: Option<String>,
    pub video_size: Option<u64>,
    pub filename: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleCandidateInfo {
    pub id: String,
    pub provider: String,
    pub lang: String,
    pub title: Option<String>,
    pub download_url: String,
    pub opensubtitles_file_id: Option<i64>,
    pub encoding_hint: Option<String>,
    pub is_sdh: bool,
    pub is_forced: bool,
    pub hash_matched: bool,
}

impl From<cinma_core::SubtitleCandidate> for SubtitleCandidateInfo {
    fn from(c: cinma_core::SubtitleCandidate) -> Self {
        Self {
            id: c.id,
            provider: c.provider,
            lang: c.lang,
            title: c.title,
            download_url: c.download_url,
            opensubtitles_file_id: c.opensubtitles_file_id,
            encoding_hint: c.encoding_hint,
            is_sdh: c.is_sdh,
            is_forced: c.is_forced,
            hash_matched: c.hash_matched,
        }
    }
}

impl From<SubtitleCandidateInfo> for cinma_core::SubtitleCandidate {
    fn from(c: SubtitleCandidateInfo) -> Self {
        Self {
            id: c.id,
            provider: c.provider,
            lang: c.lang,
            title: c.title,
            download_url: c.download_url,
            opensubtitles_file_id: c.opensubtitles_file_id,
            encoding_hint: c.encoding_hint,
            is_sdh: c.is_sdh,
            is_forced: c.is_forced,
            hash_matched: c.hash_matched,
        }
    }
}

#[derive(uniffi::Record)]
pub struct PreparedSubtitleInfo {
    pub local_path: String,
    pub format: String,
    pub detected_encoding: String,
    pub restyled_ass: bool,
}

#[derive(uniffi::Record)]
pub struct NextEpisodePrefetchInfo {
    pub video_id: String,
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub still: Option<String>,
    pub best_stream: Option<StreamInfo>,
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
            PlayerEvent::Metric { name, millis } => Self::Metric {
                name: name.to_owned(),
                millis,
            },
            PlayerEvent::TracksChanged => Self::TracksChanged,
            PlayerEvent::ChaptersChanged => Self::ChaptersChanged,
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

    pub fn reorder_addons(&self, ordered_ids: Vec<String>) -> Result<(), CinmaError> {
        Ok(self.inner.reorder_addons(ordered_ids)?)
    }

    pub fn onboarding_completed(&self) -> Result<bool, CinmaError> {
        Ok(self.inner.onboarding_completed()?)
    }

    pub fn set_onboarding_completed(&self, completed: bool) -> Result<(), CinmaError> {
        Ok(self.inner.set_onboarding_completed(completed)?)
    }

    pub fn profiles(&self) -> Result<Vec<ProfileInfo>, CinmaError> {
        Ok(self.inner.profiles()?.into_iter().map(Into::into).collect())
    }

    pub fn active_profile_id(&self) -> Result<i64, CinmaError> {
        Ok(self.inner.active_profile()?)
    }

    pub fn set_active_profile(&self, id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.set_active_profile(id)?)
    }

    pub fn create_profile(&self, name: String, avatar: Option<String>) -> Result<i64, CinmaError> {
        Ok(self.inner.create_profile(name, avatar)?)
    }

    pub fn rename_profile(&self, id: i64, name: String) -> Result<(), CinmaError> {
        Ok(self.inner.rename_profile(id, name)?)
    }

    pub fn delete_profile(&self, id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.delete_profile(id)?)
    }

    pub fn library_items(&self) -> Result<Vec<LibraryItemInfo>, CinmaError> {
        Ok(self
            .inner
            .library_items()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn is_in_library(&self, meta_id: String, media_type: String) -> Result<bool, CinmaError> {
        Ok(self.inner.is_in_library(&meta_id, &media_type)?)
    }

    pub fn add_to_library(
        &self,
        meta_id: String,
        media_type: String,
        name: String,
        poster: Option<String>,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .add_to_library(meta_id, media_type, name, poster)?)
    }

    pub fn remove_from_library(
        &self,
        meta_id: String,
        media_type: String,
    ) -> Result<(), CinmaError> {
        Ok(self.inner.remove_from_library(&meta_id, &media_type)?)
    }

    pub fn continue_watching(&self, limit: u32) -> Result<Vec<WatchProgressInfo>, CinmaError> {
        Ok(self
            .inner
            .continue_watching(limit as usize)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn get_watch_progress(
        &self,
        video_id: String,
    ) -> Result<Option<WatchProgressInfo>, CinmaError> {
        Ok(self.inner.get_watch_progress(&video_id)?.map(Into::into))
    }

    pub fn progress_for_meta(&self, meta_id: String) -> Result<Vec<WatchProgressInfo>, CinmaError> {
        Ok(self
            .inner
            .progress_for_meta(&meta_id)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn save_watch_progress(&self, input: WatchProgressInputFfi) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .save_watch_progress(cinma_core::WatchProgressInput {
                video_id: input.video_id,
                meta_id: input.meta_id,
                media_type: input.media_type,
                title: input.title,
                artwork: input.artwork,
                season: input.season,
                episode: input.episode,
                position_s: input.position_s,
                duration_s: input.duration_s,
            })?)
    }

    pub fn set_watched(
        &self,
        video_id: String,
        meta_id: String,
        media_type: String,
        title: String,
        watched: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .set_watched(video_id, meta_id, media_type, title, watched)?)
    }

    pub fn has_tmdb_key(&self) -> Result<bool, CinmaError> {
        Ok(self.inner.has_tmdb_key()?)
    }

    pub fn set_tmdb_key(&self, key: Option<String>) -> Result<(), CinmaError> {
        Ok(self.inner.set_tmdb_key(key)?)
    }

    pub async fn verify_tmdb_key(&self, key: String) -> Result<(), CinmaError> {
        Ok(self.inner.verify_tmdb_key(key).await?)
    }

    pub fn metadata_language(&self) -> Result<String, CinmaError> {
        Ok(self.inner.metadata_language()?)
    }

    pub fn set_metadata_language(&self, lang: String) -> Result<(), CinmaError> {
        Ok(self.inner.set_metadata_language(&lang)?)
    }

    pub fn cached_trending(&self, media_type: String) -> Result<Vec<MetaPreviewInfo>, CinmaError> {
        Ok(self
            .inner
            .cached_trending(&media_type)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn trending(&self, media_type: String) -> Result<Vec<MetaPreviewInfo>, CinmaError> {
        Ok(self
            .inner
            .trending(media_type)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn search_tmdb(
        &self,
        query: String,
        media_type: Option<String>,
    ) -> Result<Vec<MetaPreviewInfo>, CinmaError> {
        Ok(self
            .inner
            .search_tmdb(query, media_type)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn rich_meta(
        &self,
        media_type: String,
        id: String,
    ) -> Result<RichMetaInfo, CinmaError> {
        Ok(self.inner.rich_meta(media_type, id).await?.into())
    }

    pub async fn season_episodes(
        &self,
        media_type: String,
        meta_id: String,
        season: u32,
    ) -> Result<Vec<MetaVideoInfo>, CinmaError> {
        Ok(self
            .inner
            .season_episodes(media_type, meta_id, season)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn calendar_entries(&self) -> Result<Vec<UpcomingEpisodeInfo>, CinmaError> {
        Ok(self
            .inner
            .calendar_entries()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn ytdlp_status(&self) -> YtdlpStatusInfo {
        self.inner.ytdlp_status().into()
    }

    pub async fn extract_trailer_url(&self, source: String) -> Result<String, CinmaError> {
        Ok(self.inner.extract_trailer_url(source).await?)
    }

    pub async fn install_or_update_ytdlp(&self) -> Result<String, CinmaError> {
        Ok(self.inner.install_or_update_ytdlp().await?)
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
            .map(|(spec, key)| CommandInfo {
                id: spec.id.to_owned(),
                context: spec.context.name().to_owned(),
                key,
            })
            .collect())
    }

    /// Command bound to `combo` in `context` ("global" | "player").
    pub fn resolve_key(
        &self,
        context: String,
        combo: String,
    ) -> Result<Option<String>, CinmaError> {
        let context = CommandContext::from_name(&context).ok_or_else(|| CinmaError::Invalid {
            message: format!("unknown context {context}"),
        })?;
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
        Ok(self
            .inner
            .set_feature_enabled(feature(&name)?, enabled)
            .await?)
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
            .search_catalogs(
                query,
                media_type,
                Some(Arc::new(CatalogBatchAdapter(observer))),
            )
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn meta(&self, media_type: String, id: String) -> Result<MetaInfo, CinmaError> {
        Ok(self.inner.meta(media_type, id).await?.into())
    }

    /// `media_type`: "movie" | "series"; `id`: e.g. "tt1254207" or "tt0944947:1:1".
    pub async fn streams(
        &self,
        media_type: String,
        id: String,
    ) -> Result<Vec<StreamInfo>, CinmaError> {
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
                let stream: cinma_stremio::Stream =
                    serde_json::from_str(&s.raw).map_err(|e| CinmaError::Invalid {
                        message: e.to_string(),
                    })?;
                Ok(cinma_core::StreamCandidate {
                    addon_name: s.addon_name,
                    stream,
                })
            })
            .collect::<Result<Vec<_>, CinmaError>>()?;
        Ok(self.inner.rank_streams(candidates, prefs).into())
    }

    /// Creates a player. Blocking (loads libmpv on first use) — call off the
    /// main thread, then attach a video layer with `render_init`.
    pub fn create_playback(
        &self,
        observer: Arc<dyn PlaybackObserver>,
    ) -> Result<Arc<PlaybackSession>, CinmaError> {
        self.create_playback_for_surface(observer, true)
    }

    pub fn create_playback_for_surface(
        &self,
        observer: Arc<dyn PlaybackObserver>,
        hdr_surface: bool,
    ) -> Result<Arc<PlaybackSession>, CinmaError> {
        let inner = self
            .inner
            .create_playback_for_surface(Arc::new(ObserverAdapter(observer)), hdr_surface)?;
        Ok(Arc::new(PlaybackSession {
            inner: Arc::new(inner),
        }))
    }

    pub fn subtitle_settings(&self) -> Result<SubtitleSettingsFfi, CinmaError> {
        Ok(self.inner.subtitle_settings()?.into())
    }

    pub fn set_subtitle_settings(&self, settings: SubtitleSettingsFfi) -> Result<(), CinmaError> {
        Ok(self.inner.set_subtitle_settings(&settings.into())?)
    }

    pub fn player_preferences(&self) -> Result<PlayerPreferencesFfi, CinmaError> {
        Ok(self.inner.player_preferences()?.into())
    }

    pub fn set_player_preferences(&self, prefs: PlayerPreferencesFfi) -> Result<(), CinmaError> {
        Ok(self.inner.set_player_preferences(&prefs.into())?)
    }

    pub fn video_offsets(&self, video_id: String) -> Result<VideoOffsetsFfi, CinmaError> {
        let o = self.inner.video_offsets(&video_id)?;
        Ok(VideoOffsetsFfi {
            sub_delay: o.sub_delay,
            audio_delay: o.audio_delay,
        })
    }

    pub fn set_video_offsets(
        &self,
        video_id: String,
        offsets: VideoOffsetsFfi,
    ) -> Result<(), CinmaError> {
        Ok(self.inner.set_video_offsets(
            &video_id,
            &cinma_core::VideoOffsets {
                sub_delay: offsets.sub_delay,
                audio_delay: offsets.audio_delay,
            },
        )?)
    }

    pub fn has_opensubtitles_key(&self) -> Result<bool, CinmaError> {
        Ok(self.inner.has_opensubtitles_key()?)
    }

    pub fn set_opensubtitles_credentials(
        &self,
        api_key: Option<String>,
        token: Option<String>,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .set_opensubtitles_credentials(api_key.as_deref(), token.as_deref())?)
    }

    pub async fn fetch_subtitles(
        &self,
        input: SubtitleSearchInput,
    ) -> Result<Vec<SubtitleCandidateInfo>, CinmaError> {
        Ok(self
            .inner
            .fetch_subtitles(cinma_core::SubtitleSearchRequest {
                media_type: input.media_type,
                video_id: input.video_id,
                imdb_id: input.imdb_id,
                season: input.season,
                episode: input.episode,
                video_hash: input.video_hash,
                video_size: input.video_size,
                filename: input.filename,
            })
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn download_and_prepare_subtitle(
        &self,
        candidate: SubtitleCandidateInfo,
    ) -> Result<PreparedSubtitleInfo, CinmaError> {
        let res = self
            .inner
            .download_and_prepare_subtitle(candidate.into())
            .await?;
        Ok(PreparedSubtitleInfo {
            local_path: res.local_path.to_string_lossy().into_owned(),
            format: res.format,
            detected_encoding: res.detected_encoding,
            restyled_ass: res.restyled_ass,
        })
    }

    pub fn prepare_local_subtitle(&self, path: String) -> Result<PreparedSubtitleInfo, CinmaError> {
        let res = self.inner.prepare_local_subtitle(&PathBuf::from(path))?;
        Ok(PreparedSubtitleInfo {
            local_path: res.local_path.to_string_lossy().into_owned(),
            format: res.format,
            detected_encoding: res.detected_encoding,
            restyled_ass: res.restyled_ass,
        })
    }

    pub fn resolve_shader_paths(&self, preset: String) -> Result<Vec<String>, CinmaError> {
        Ok(self.inner.resolve_shader_paths(&preset)?)
    }
}

#[derive(uniffi::Object)]
pub struct PlaybackSession {
    inner: Arc<cinma_core::Playback>,
}

#[uniffi::export]
impl PlaybackSession {
    pub async fn play(&self, stream: StreamInfo) -> Result<(), CinmaError> {
        let stream: cinma_stremio::Stream =
            serde_json::from_str(&stream.raw).map_err(|e| CinmaError::Invalid {
                message: e.to_string(),
            })?;
        Ok(self.inner.play(stream).await?)
    }

    /// Plays a local file or URL directly (debug / render tests).
    pub fn play_url(&self, url: String) -> Result<(), CinmaError> {
        Ok(self.inner.play_url(&url)?)
    }

    pub fn toggle_pause(&self) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .toggle_pause()
            .map_err(CoreError::from)?)
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_absolute(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn seek_absolute_exact(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_absolute_exact(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_relative(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn seek_relative_mode(&self, seconds: f64, exact: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_relative_mode(seconds, exact)
            .map_err(CoreError::from)?)
    }

    pub fn tracks(&self) -> Result<Vec<MediaTrackInfo>, CinmaError> {
        Ok(self
            .inner
            .player()
            .tracks()
            .map_err(CoreError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn chapters(&self) -> Result<Vec<ChapterInfo>, CinmaError> {
        Ok(self
            .inner
            .player()
            .chapters()
            .map_err(CoreError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn stats(&self) -> PlayerStatsInfo {
        self.inner.player().stats().into()
    }

    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_track(id)
            .map_err(CoreError::from)?)
    }

    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_track(id)
            .map_err(CoreError::from)?)
    }

    pub fn set_secondary_subtitle_track(&self, id: Option<i64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_secondary_subtitle_track(id)
            .map_err(CoreError::from)?)
    }

    pub fn add_subtitle(
        &self,
        path: String,
        title: Option<String>,
        lang: Option<String>,
        select: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .add_subtitle(&path, title.as_deref(), lang.as_deref(), select)
            .map_err(CoreError::from)?)
    }

    pub fn set_subtitle_delay(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_delay(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_audio_delay(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_delay(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_subtitle_style(
        &self,
        font: Option<String>,
        font_size: Option<u32>,
        color_hex: Option<String>,
        pos: Option<u32>,
        ass_override: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_style(
                font.as_deref(),
                font_size,
                color_hex.as_deref(),
                pos,
                ass_override,
            )
            .map_err(CoreError::from)?)
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_speed(speed)
            .map_err(CoreError::from)?)
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_volume(volume)
            .map_err(CoreError::from)?)
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_muted(muted)
            .map_err(CoreError::from)?)
    }

    pub fn set_ab_loop_a(&self, seconds: Option<f64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_ab_loop_a(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_ab_loop_b(&self, seconds: Option<f64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_ab_loop_b(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_hdr_target_peak(&self, nits: Option<u32>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_hdr_target_peak(nits)
            .map_err(CoreError::from)?)
    }

    pub fn set_glsl_shaders(&self, paths: Vec<String>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_glsl_shaders(&paths)
            .map_err(CoreError::from)?)
    }

    pub fn screenshot_to_file(&self, path: String) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .screenshot_to_file(&PathBuf::from(path))
            .map_err(CoreError::from)?)
    }

    pub fn video_identity(&self) -> Option<VideoFileIdentityInfo> {
        self.inner.video_identity().map(|id| VideoFileIdentityInfo {
            hash: id.hash,
            size: id.size,
            filename: id.filename,
        })
    }

    pub fn thumbnail_at(&self, seconds: f64) -> Option<ThumbnailImageInfo> {
        self.inner
            .thumbnail_at(seconds)
            .map(|t| ThumbnailImageInfo {
                width: t.width,
                height: t.height,
                rgba: t.rgba,
            })
    }

    pub fn start_next_episode_prefetch(
        &self,
        meta_id: String,
        current_season: u32,
        current_episode: u32,
        binge_group: Option<String>,
    ) {
        self.inner.start_next_episode_prefetch(
            meta_id,
            current_season,
            current_episode,
            binge_group,
        );
    }

    pub fn prefetched_next_episode(&self) -> Option<NextEpisodePrefetchInfo> {
        self.inner
            .prefetched_next_episode()
            .map(|p| NextEpisodePrefetchInfo {
                video_id: p.video_id,
                season: p.season,
                episode: p.episode,
                title: p.title,
                still: p.still,
                best_stream: p.best_stream.map(Into::into),
            })
    }

    pub fn cancel_prefetch(&self) {
        self.inner.cancel_prefetch();
    }

    /// Creates the OpenGL render context. Call with the video layer's CGL
    /// context current (e.g. inside `CAOpenGLLayer.draw`).
    pub fn render_init(&self, observer: Arc<dyn RenderObserver>) -> Result<(), CinmaError> {
        let callback: cinma_core::UpdateCallback = Arc::new(move || observer.needs_display());
        Ok(self
            .inner
            .player()
            .render_init(callback)
            .map_err(CoreError::from)?)
    }

    pub fn render_should_draw(&self) -> bool {
        self.inner.player().render_should_draw()
    }

    /// Draws the current frame into `fbo`. CGL context must be current.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .render(fbo, width, height)
            .map_err(CoreError::from)?)
    }

    pub fn render_frame(
        &self,
        fbo: i32,
        width: i32,
        height: i32,
        block_for_target_time: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .render_frame(fbo, width, height, block_for_target_time)
            .map_err(CoreError::from)?)
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
