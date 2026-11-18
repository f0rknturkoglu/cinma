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
    DownloadsChanged,
    CastChanged,
    TrackingChanged,
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
            CoreEvent::DownloadsChanged => CoreEventKind::DownloadsChanged,
            CoreEvent::CastChanged => CoreEventKind::CastChanged,
            CoreEvent::TrackingChanged => CoreEventKind::TrackingChanged,
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
pub struct StreamStatsInfo {
    pub is_torrent: bool,
    pub info_hash: Option<String>,
    pub filename: Option<String>,
    pub source_url: Option<String>,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub peers_connected: u32,
    pub peers_queued: u32,
    pub peers_seen: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub finished: bool,
}

impl From<cinma_core::StreamStats> for StreamStatsInfo {
    fn from(s: cinma_core::StreamStats) -> Self {
        Self {
            is_torrent: s.is_torrent,
            info_hash: s.info_hash,
            filename: s.filename,
            source_url: s.source_url,
            download_speed_bps: s.download_speed_bps,
            upload_speed_bps: s.upload_speed_bps,
            peers_connected: s.peers_connected,
            peers_queued: s.peers_queued,
            peers_seen: s.peers_seen,
            downloaded_bytes: s.downloaded_bytes,
            total_bytes: s.total_bytes,
            finished: s.finished,
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
    pub srt_border_size: u32,
    pub srt_border_color: Option<String>,
    pub srt_back_color: Option<String>,
    pub srt_bold: bool,
    pub srt_pos: u32,
    pub ass_override: bool,
    pub srt_opacity: u32,
    pub srt_box_opacity: u32,
    pub srt_align_x: String,
    pub secondary_pos: u32,
    pub lift_above_controls: bool,
    pub off_by_default: bool,
    pub prefer_embedded: bool,
    pub forced_when_native_audio: bool,
    pub hide_sdh: bool,
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
            srt_border_size: s.srt_border_size,
            srt_border_color: s.srt_border_color,
            srt_back_color: s.srt_back_color,
            srt_bold: s.srt_bold,
            srt_pos: s.srt_pos,
            ass_override: s.ass_override,
            srt_opacity: s.srt_opacity,
            srt_box_opacity: s.srt_box_opacity,
            srt_align_x: s.srt_align_x,
            secondary_pos: s.secondary_pos,
            lift_above_controls: s.lift_above_controls,
            off_by_default: s.off_by_default,
            prefer_embedded: s.prefer_embedded,
            forced_when_native_audio: s.forced_when_native_audio,
            hide_sdh: s.hide_sdh,
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
            srt_border_size: s.srt_border_size,
            srt_border_color: s.srt_border_color,
            srt_back_color: s.srt_back_color,
            srt_bold: s.srt_bold,
            srt_pos: s.srt_pos,
            ass_override: s.ass_override,
            srt_opacity: s.srt_opacity,
            srt_box_opacity: s.srt_box_opacity,
            srt_align_x: s.srt_align_x,
            secondary_pos: s.secondary_pos,
            lift_above_controls: s.lift_above_controls,
            off_by_default: s.off_by_default,
            prefer_embedded: s.prefer_embedded,
            forced_when_native_audio: s.forced_when_native_audio,
            hide_sdh: s.hide_sdh,
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
    pub seek_back_step_s: u32,
    pub seek_forward_step_s: u32,
    pub default_speed_pct: u32,
    pub volume_boost_max: u32,
    pub pause_minimized: bool,
    pub pause_unfocused: bool,
    pub auto_skip_intro: bool,
    pub auto_skip_recap: bool,
    pub auto_skip_outro: bool,
    pub show_skip_button: bool,
    pub auto_next_stream_on_stall: bool,
    pub stall_timeout_s: u32,
    pub crop_mode: String,
    pub audio_normalize: bool,
    pub downmix_stereo: bool,
    pub hide_spoilers: bool,
    pub remember_last_stream: bool,
    pub keep_source_next_episode: bool,
    pub track_block_words: Vec<String>,
    pub stream_sort: String,
    pub picker_show_filename: bool,
    pub fullscreen_clock: bool,
    pub fullscreen_clock_end_time: bool,
    pub next_episode_lead_s: u32,
    pub still_watching_after_episodes: u32,
    pub hide_watched_in_catalogs: bool,
    pub hide_unreleased: bool,
    pub episode_sort_asc: bool,
    pub episode_compact_layout: bool,
    pub confirm_on_leave: bool,
    pub resume_behavior: String,
    pub volume_hud: bool,
    pub screen_lock_enabled: bool,
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
            seek_back_step_s: p.seek_back_step_s,
            seek_forward_step_s: p.seek_forward_step_s,
            default_speed_pct: p.default_speed_pct,
            volume_boost_max: p.volume_boost_max,
            pause_minimized: p.pause_minimized,
            pause_unfocused: p.pause_unfocused,
            auto_skip_intro: p.auto_skip_intro,
            auto_skip_recap: p.auto_skip_recap,
            auto_skip_outro: p.auto_skip_outro,
            show_skip_button: p.show_skip_button,
            auto_next_stream_on_stall: p.auto_next_stream_on_stall,
            stall_timeout_s: p.stall_timeout_s,
            crop_mode: p.crop_mode,
            audio_normalize: p.audio_normalize,
            downmix_stereo: p.downmix_stereo,
            hide_spoilers: p.hide_spoilers,
            remember_last_stream: p.remember_last_stream,
            keep_source_next_episode: p.keep_source_next_episode,
            track_block_words: p.track_block_words,
            stream_sort: p.stream_sort,
            picker_show_filename: p.picker_show_filename,
            fullscreen_clock: p.fullscreen_clock,
            fullscreen_clock_end_time: p.fullscreen_clock_end_time,
            next_episode_lead_s: p.next_episode_lead_s,
            still_watching_after_episodes: p.still_watching_after_episodes,
            hide_watched_in_catalogs: p.hide_watched_in_catalogs,
            hide_unreleased: p.hide_unreleased,
            episode_sort_asc: p.episode_sort_asc,
            episode_compact_layout: p.episode_compact_layout,
            confirm_on_leave: p.confirm_on_leave,
            resume_behavior: p.resume_behavior,
            volume_hud: p.volume_hud,
            screen_lock_enabled: p.screen_lock_enabled,
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
            seek_back_step_s: p.seek_back_step_s,
            seek_forward_step_s: p.seek_forward_step_s,
            default_speed_pct: p.default_speed_pct,
            volume_boost_max: p.volume_boost_max,
            pause_minimized: p.pause_minimized,
            pause_unfocused: p.pause_unfocused,
            auto_skip_intro: p.auto_skip_intro,
            auto_skip_recap: p.auto_skip_recap,
            auto_skip_outro: p.auto_skip_outro,
            show_skip_button: p.show_skip_button,
            auto_next_stream_on_stall: p.auto_next_stream_on_stall,
            stall_timeout_s: p.stall_timeout_s,
            crop_mode: p.crop_mode,
            audio_normalize: p.audio_normalize,
            downmix_stereo: p.downmix_stereo,
            hide_spoilers: p.hide_spoilers,
            remember_last_stream: p.remember_last_stream,
            keep_source_next_episode: p.keep_source_next_episode,
            track_block_words: p.track_block_words,
            stream_sort: p.stream_sort,
            picker_show_filename: p.picker_show_filename,
            fullscreen_clock: p.fullscreen_clock,
            fullscreen_clock_end_time: p.fullscreen_clock_end_time,
            next_episode_lead_s: p.next_episode_lead_s,
            still_watching_after_episodes: p.still_watching_after_episodes,
            hide_watched_in_catalogs: p.hide_watched_in_catalogs,
            hide_unreleased: p.hide_unreleased,
            episode_sort_asc: p.episode_sort_asc,
            episode_compact_layout: p.episode_compact_layout,
            confirm_on_leave: p.confirm_on_leave,
            resume_behavior: p.resume_behavior,
            volume_hud: p.volume_hud,
            screen_lock_enabled: p.screen_lock_enabled,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct PreferredSeriesSourceFfi {
    pub addon_name: Option<String>,
    pub binge_group: Option<String>,
    pub resolution: Option<String>,
}

impl From<cinma_core::PreferredSeriesSource> for PreferredSeriesSourceFfi {
    fn from(s: cinma_core::PreferredSeriesSource) -> Self {
        Self {
            addon_name: s.addon_name,
            binge_group: s.binge_group,
            resolution: s.resolution,
        }
    }
}

impl From<PreferredSeriesSourceFfi> for cinma_core::PreferredSeriesSource {
    fn from(s: PreferredSeriesSourceFfi) -> Self {
        Self {
            addon_name: s.addon_name,
            binge_group: s.binge_group,
            resolution: s.resolution,
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

    pub fn watched_meta_ids(&self) -> Result<Vec<String>, CinmaError> {
        Ok(self.inner.watched_meta_ids()?)
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

    pub fn series_source(&self, meta_id: String) -> Result<PreferredSeriesSourceFfi, CinmaError> {
        Ok(self.inner.series_source(&meta_id)?.into())
    }

    pub fn set_series_source(
        &self,
        meta_id: String,
        source: PreferredSeriesSourceFfi,
    ) -> Result<(), CinmaError> {
        Ok(self.inner.set_series_source(&meta_id, &source.into())?)
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

    // ---- Phase 5: Unified Downloads, Disk Policy & Menu Bar ----

    pub fn add_http_download(
        &self,
        title: String,
        url: String,
        meta_id: Option<String>,
        media_type: Option<String>,
        priority: i64,
    ) -> Result<i64, CinmaError> {
        Ok(self.inner.add_http_download(
            title,
            url,
            std::collections::HashMap::new(),
            meta_id,
            media_type,
            priority,
        )?)
    }

    pub fn add_torrent_download(
        &self,
        title: String,
        stream: StreamInfo,
        meta_id: Option<String>,
        media_type: Option<String>,
        priority: i64,
    ) -> Result<i64, CinmaError> {
        let parsed: cinma_stremio::Stream =
            serde_json::from_str(&stream.raw).map_err(|e| CinmaError::Invalid {
                message: e.to_string(),
            })?;
        let trackers = parsed.trackers();
        let info_hash = parsed.info_hash.ok_or_else(|| CinmaError::Invalid {
            message: "stream has no infoHash".into(),
        })?;
        Ok(self.inner.add_torrent_download(
            title,
            info_hash,
            parsed.file_idx.map(|i| i as usize),
            trackers,
            meta_id,
            media_type,
            priority,
        )?)
    }

    pub fn add_ytdlp_download(
        &self,
        title: String,
        url: String,
        max_height: Option<u32>,
        audio_only: bool,
        priority: i64,
    ) -> Result<i64, CinmaError> {
        Ok(self
            .inner
            .add_ytdlp_download(title, url, max_height, audio_only, priority)?)
    }

    pub fn downloads(&self) -> Result<Vec<DownloadInfo>, CinmaError> {
        Ok(self
            .inner
            .downloads()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn pause_download(&self, id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.pause_download(id)?)
    }

    pub fn resume_download(&self, id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.resume_download(id)?)
    }

    pub fn remove_download(&self, id: i64, delete_data: bool) -> Result<(), CinmaError> {
        Ok(self.inner.remove_download(id, delete_data)?)
    }

    pub fn has_pending_downloads(&self) -> Result<bool, CinmaError> {
        Ok(self.inner.has_pending_downloads()?)
    }

    pub fn menubar_enabled(&self) -> Result<bool, CinmaError> {
        Ok(self.inner.menubar_enabled()?)
    }

    pub fn set_menubar_enabled(&self, enabled: bool) -> Result<(), CinmaError> {
        Ok(self.inner.set_menubar_enabled(enabled)?)
    }

    pub fn disk_policy_settings(&self) -> Result<DiskPolicySettingsInfo, CinmaError> {
        Ok(self.inner.disk_policy_settings()?.into())
    }

    pub fn set_disk_policy_settings(&self, s: DiskPolicySettingsInfo) -> Result<(), CinmaError> {
        let mut current = self.inner.disk_policy_settings()?;
        let video = Some(s.video_folder.trim().to_owned()).filter(|p| !p.is_empty());
        current.movie_folder.clone_from(&video);
        current.series_folder.clone_from(&video);
        current.ytdlp_folder = video;
        current.audio_folder = Some(s.audio_folder.trim().to_owned()).filter(|p| !p.is_empty());
        current.cache_quota_mb = s.cache_quota_mb;
        current.keep_watched_global = s.keep_watched_global;
        current.seeding_mode = s.seeding_mode;
        current.seeding_ratio = s.seeding_ratio;
        current.seeding_minutes = s.seeding_minutes;
        current.upload_limit_kbps = s.upload_limit_kbps;
        Ok(self.inner.set_disk_policy_settings(&current)?)
    }

    pub fn set_item_keep_watched(
        &self,
        meta_id: String,
        media_type: String,
        keep: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .set_item_keep_watched(&meta_id, &media_type, keep)?)
    }

    pub fn item_keep_watched(
        &self,
        meta_id: String,
        media_type: String,
    ) -> Result<bool, CinmaError> {
        Ok(self.inner.item_keep_watched(&meta_id, &media_type)?)
    }

    pub fn disk_usage_summary(&self) -> Result<DiskUsageSummaryInfo, CinmaError> {
        Ok(self.inner.disk_usage_summary()?.into())
    }

    pub fn enforce_disk_quota(&self) -> Result<u64, CinmaError> {
        Ok(self.inner.enforce_disk_quota()?)
    }

    // ---- Phase 6: Casting & Tracking Integrations ----

    pub async fn discover_cast_devices(
        &self,
        timeout_ms: u64,
    ) -> Result<Vec<CastDeviceInfo>, CinmaError> {
        Ok(self
            .inner
            .discover_cast_devices(timeout_ms)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn cast_download(
        &self,
        device: CastDeviceInfo,
        download_id: i64,
        title: String,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .cast_download(device.try_into()?, download_id, title)
            .await?)
    }

    pub async fn cast_local_file(
        &self,
        device: CastDeviceInfo,
        path: String,
        title: String,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .cast_local_file(device.try_into()?, path, title)
            .await?)
    }

    pub async fn cast_stream(
        &self,
        device: CastDeviceInfo,
        stream: StreamInfo,
        title: String,
    ) -> Result<(), CinmaError> {
        let parsed: cinma_stremio::Stream =
            serde_json::from_str(&stream.raw).map_err(|e| CinmaError::Invalid {
                message: e.to_string(),
            })?;
        Ok(self
            .inner
            .cast_stream(device.try_into()?, parsed, title)
            .await?)
    }

    pub async fn cast_play(&self) -> Result<(), CinmaError> {
        Ok(self.inner.cast_play().await?)
    }

    pub async fn cast_pause(&self) -> Result<(), CinmaError> {
        Ok(self.inner.cast_pause().await?)
    }

    pub async fn cast_seek(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self.inner.cast_seek(seconds).await?)
    }

    pub async fn cast_stop(&self) -> Result<(), CinmaError> {
        Ok(self.inner.cast_stop().await?)
    }

    pub fn active_cast_device(&self) -> Option<CastDeviceInfo> {
        self.inner.active_cast_device().map(Into::into)
    }

    pub fn tracking_adapters(&self) -> Result<Vec<AdapterStatusInfo>, CinmaError> {
        Ok(self
            .inner
            .tracking_adapters()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn set_tracking_adapter_enabled(
        &self,
        adapter: String,
        enabled: bool,
    ) -> Result<(), CinmaError> {
        Ok(self.inner.set_tracking_adapter_enabled(&adapter, enabled)?)
    }

    pub fn set_tracking_token(
        &self,
        adapter: String,
        token: Option<String>,
    ) -> Result<(), CinmaError> {
        Ok(self.inner.set_tracking_token(&adapter, token.as_deref())?)
    }

    pub fn set_tracking_app_credentials(
        &self,
        adapter: String,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<(), CinmaError> {
        Ok(self.inner.set_tracking_app_credentials(
            &adapter,
            &client_id,
            client_secret.as_deref(),
        )?)
    }

    pub async fn trakt_start_device_auth(
        &self,
        client_id: String,
    ) -> Result<TraktDeviceCodeInfo, CinmaError> {
        Ok(self.inner.trakt_start_device_auth(client_id).await?.into())
    }

    pub async fn trakt_poll_device_auth(
        &self,
        client_id: String,
        client_secret: String,
        device_code: String,
    ) -> Result<bool, CinmaError> {
        Ok(self
            .inner
            .trakt_poll_device_auth(client_id, client_secret, device_code)
            .await?)
    }

    pub async fn drain_tracking_outbox(&self) -> Result<u32, CinmaError> {
        Ok(self.inner.drain_tracking_outbox().await?)
    }

    pub fn backfill_tracking(&self) -> Result<u32, CinmaError> {
        Ok(self.inner.backfill_tracking()?)
    }

    pub async fn trakt_recommendations(&self) -> Result<Vec<TraktRecommendationInfo>, CinmaError> {
        Ok(self
            .inner
            .trakt_recommendations()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn letterboxd_rss(
        &self,
        username: String,
    ) -> Result<Vec<LetterboxdEntryInfo>, CinmaError> {
        Ok(self
            .inner
            .letterboxd_rss(username)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn export_letterboxd_csv(&self, dest_path: String) -> Result<u32, CinmaError> {
        Ok(self
            .inner
            .export_letterboxd_csv(&PathBuf::from(dest_path))? as u32)
    }

    pub fn seed_anime_id_map(&self) -> Result<u32, CinmaError> {
        Ok(self.inner.seed_anime_id_map()? as u32)
    }

    pub async fn refresh_anime_id_map(&self, url: String) -> Result<u32, CinmaError> {
        Ok(self.inner.refresh_anime_id_map(url).await? as u32)
    }

    pub async fn import_stremio_account(
        &self,
        email: String,
        password: String,
    ) -> Result<StremioImportResultInfo, CinmaError> {
        Ok(self
            .inner
            .import_stremio_account(email, password)
            .await?
            .into())
    }

    // ---- Phase 7: Profile PINs & SQLite Backup / Restore ----

    pub fn set_profile_pin(&self, profile_id: i64, pin: Option<String>) -> Result<(), CinmaError> {
        Ok(self.inner.set_profile_pin(profile_id, pin.as_deref())?)
    }

    pub fn verify_profile_pin(&self, profile_id: i64, pin: String) -> Result<bool, CinmaError> {
        Ok(self.inner.verify_profile_pin(profile_id, &pin)?)
    }

    pub fn export_backup(&self, dest_path: String) -> Result<(), CinmaError> {
        Ok(self.inner.export_backup(&PathBuf::from(dest_path))?)
    }

    pub fn import_backup(&self, src_path: String) -> Result<u32, CinmaError> {
        Ok(self.inner.import_backup(&PathBuf::from(src_path))?)
    }

    // ---- Phase 9: Integrations & Add-ons ----

    pub async fn skip_segments(
        &self,
        media_type: String,
        meta_id: String,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<Vec<SkipSegmentInfo>, CinmaError> {
        Ok(self
            .inner
            .skip_segments(media_type, meta_id, season, episode)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn debrid_statuses(&self) -> Result<Vec<DebridProviderStatusInfo>, CinmaError> {
        Ok(self
            .inner
            .debrid_statuses()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn set_debrid_key(&self, provider: String, key: Option<String>) -> Result<(), CinmaError> {
        Ok(self.inner.set_debrid_key(provider, key)?)
    }

    pub async fn rank_streams_with_debrid(
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
        Ok(self
            .inner
            .rank_streams_with_debrid(candidates, prefs)
            .await?
            .into())
    }

    pub fn begin_tracking_oauth(
        &self,
        provider: String,
        client_id: String,
        client_secret: String,
    ) -> Result<OAuthStartInfo, CinmaError> {
        Ok(self
            .inner
            .begin_tracking_oauth(provider, client_id, client_secret)?
            .into())
    }

    pub async fn complete_tracking_oauth_callback(
        &self,
        callback_url: String,
    ) -> Result<String, CinmaError> {
        Ok(self
            .inner
            .complete_tracking_oauth_callback(callback_url)
            .await?)
    }

    pub async fn pull_trakt_sync(&self) -> Result<TraktPullSummaryInfo, CinmaError> {
        Ok(self.inner.pull_trakt_sync().await?.into())
    }

    pub async fn opensubtitles_login(
        &self,
        username: String,
        password: String,
    ) -> Result<OpenSubtitlesQuotaInfo, CinmaError> {
        Ok(self
            .inner
            .opensubtitles_login(username, password)
            .await?
            .into())
    }

    pub async fn opensubtitles_quota(&self) -> Result<OpenSubtitlesQuotaInfo, CinmaError> {
        Ok(self.inner.opensubtitles_quota().await?.into())
    }

    pub async fn opensubtitles_logout(&self) -> Result<(), CinmaError> {
        Ok(self.inner.opensubtitles_logout().await?)
    }

    pub fn fanart_api_key(&self) -> Result<Option<String>, CinmaError> {
        Ok(self.inner.fanart_api_key()?)
    }

    pub fn set_fanart_api_key(&self, key: Option<String>) -> Result<(), CinmaError> {
        Ok(self.inner.set_fanart_api_key(key)?)
    }

    pub async fn title_extras(
        &self,
        media_type: String,
        id: String,
    ) -> Result<TitleExtrasInfo, CinmaError> {
        Ok(self.inner.title_extras(media_type, id).await?.into())
    }

    pub async fn add_m3u_source(
        &self,
        name: String,
        url: String,
        epg_url: Option<String>,
    ) -> Result<u32, CinmaError> {
        Ok(self.inner.add_m3u_source(name, url, epg_url).await?)
    }

    pub async fn add_xtream_source(
        &self,
        name: String,
        base_url: String,
        username: String,
        password: String,
    ) -> Result<u32, CinmaError> {
        Ok(self
            .inner
            .add_xtream_source(name, base_url, username, password)
            .await?)
    }

    pub async fn refresh_xmltv(&self, url: String) -> Result<u32, CinmaError> {
        Ok(self.inner.refresh_xmltv(url).await?)
    }

    pub fn list_iptv_sources(&self) -> Result<Vec<IptvSourceInfo>, CinmaError> {
        Ok(self
            .inner
            .list_iptv_sources()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn remove_iptv_source(&self, source_id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.remove_iptv_source(source_id)?)
    }

    pub fn list_iptv_channels(
        &self,
        group: Option<String>,
        query: Option<String>,
    ) -> Result<Vec<IptvChannelInfo>, CinmaError> {
        Ok(self
            .inner
            .list_iptv_channels(group, query)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn set_iptv_favorite(&self, channel_id: i64, favorite: bool) -> Result<(), CinmaError> {
        Ok(self.inner.set_iptv_favorite(channel_id, favorite)?)
    }

    pub fn iptv_epg_now_next(
        &self,
        epg_channel_id: String,
    ) -> Result<Vec<IptvEpgInfo>, CinmaError> {
        Ok(self
            .inner
            .iptv_epg_now_next(epg_channel_id)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn update_discord_presence(
        &self,
        title: String,
        subtitle: Option<String>,
        duration_s: Option<u64>,
        position_s: u64,
        paused: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .update_discord_presence(cinma_core::DiscordPresenceInput {
                title,
                subtitle,
                duration_s,
                position_s,
                paused,
            })
            .await?)
    }

    pub async fn clear_discord_presence(&self) -> Result<(), CinmaError> {
        Ok(self.inner.clear_discord_presence().await?)
    }

    pub fn vpn_torrent_binding(&self) -> Result<VpnBindingInfo, CinmaError> {
        let s = self.inner.disk_policy_settings()?;
        Ok(VpnBindingInfo {
            bind_device_name: s.bind_device_name,
            vpn_kill_switch: s.vpn_kill_switch,
        })
    }

    pub async fn set_vpn_torrent_binding(
        &self,
        bind_device_name: Option<String>,
        vpn_kill_switch: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .set_vpn_torrent_binding(bind_device_name, vpn_kill_switch)
            .await?)
    }
}

#[derive(uniffi::Record)]
pub struct DownloadInfo {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub state: String,
    pub priority: i64,
    pub bytes_total: Option<u64>,
    pub bytes_done: u64,
    pub error: Option<String>,
    pub can_play_now: bool,
    /// The finished file in the download folder (for "Show in Finder").
    pub file_path: Option<String>,
}

impl From<cinma_core::DownloadRow> for DownloadInfo {
    fn from(r: cinma_core::DownloadRow) -> Self {
        Self {
            id: r.id,
            kind: r.kind.as_str().to_owned(),
            title: r.title,
            state: r.state.as_str().to_owned(),
            priority: r.priority,
            bytes_total: r.bytes_total,
            bytes_done: r.bytes_done,
            can_play_now: r.blob_id.is_some()
                || r.file_path.is_some()
                || (r.kind == cinma_core::DownloadKind::Torrent
                    && r.state == cinma_core::DownloadState::Active),
            error: r.error,
            file_path: r.file_path,
        }
    }
}

#[derive(uniffi::Record)]
pub struct DiskPolicySettingsInfo {
    /// Folder for movies, series and web video (resolved default if unset).
    pub video_folder: String,
    /// Folder for audio-only downloads (resolved default if unset).
    pub audio_folder: String,
    pub cache_quota_mb: u64,
    pub keep_watched_global: bool,
    pub seeding_mode: String,
    pub seeding_ratio: f64,
    pub seeding_minutes: u64,
    pub upload_limit_kbps: u32,
}

impl From<cinma_core::DiskPolicySettings> for DiskPolicySettingsInfo {
    fn from(s: cinma_core::DiskPolicySettings) -> Self {
        let folder = |v: &Option<String>, fallback: std::path::PathBuf| {
            v.clone()
                .filter(|p| !p.trim().is_empty())
                .unwrap_or_else(|| fallback.to_string_lossy().into_owned())
        };
        Self {
            video_folder: folder(&s.movie_folder, cinma_core::default_video_folder()),
            audio_folder: folder(&s.audio_folder, cinma_core::default_audio_folder()),
            cache_quota_mb: s.cache_quota_mb,
            keep_watched_global: s.keep_watched_global,
            seeding_mode: s.seeding_mode,
            seeding_ratio: s.seeding_ratio,
            seeding_minutes: s.seeding_minutes,
            upload_limit_kbps: s.upload_limit_kbps,
        }
    }
}

#[derive(uniffi::Record)]
pub struct DiskUsageSummaryInfo {
    pub total_bytes: u64,
    pub kept_bytes: u64,
    pub evictable_bytes: u64,
    pub entry_count: u32,
}

impl From<cinma_core::DiskUsageSummary> for DiskUsageSummaryInfo {
    fn from(s: cinma_core::DiskUsageSummary) -> Self {
        Self {
            total_bytes: s.total_bytes,
            kept_bytes: s.kept_bytes,
            evictable_bytes: s.evictable_bytes,
            entry_count: s.entry_count,
        }
    }
}

#[derive(uniffi::Record)]
pub struct CastDeviceInfo {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub control_url: Option<String>,
}

impl From<cinma_core::CastDevice> for CastDeviceInfo {
    fn from(d: cinma_core::CastDevice) -> Self {
        Self {
            id: d.id,
            kind: match d.kind {
                cinma_core::CastKind::Chromecast => "chromecast".to_owned(),
                cinma_core::CastKind::Dlna => "dlna".to_owned(),
            },
            name: d.name,
            host: d.host,
            port: d.port,
            control_url: d.control_url,
        }
    }
}

impl TryFrom<CastDeviceInfo> for cinma_core::CastDevice {
    type Error = CinmaError;
    fn try_from(d: CastDeviceInfo) -> Result<Self, Self::Error> {
        let kind = match d.kind.as_str() {
            "chromecast" => cinma_core::CastKind::Chromecast,
            "dlna" => cinma_core::CastKind::Dlna,
            other => {
                return Err(CinmaError::Invalid {
                    message: format!("unknown cast kind `{other}`"),
                });
            }
        };
        Ok(Self {
            id: d.id,
            kind,
            name: d.name,
            host: d.host,
            port: d.port,
            control_url: d.control_url,
        })
    }
}

#[derive(uniffi::Record)]
pub struct TraktDeviceCodeInfo {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

impl From<cinma_core::TraktDeviceCode> for TraktDeviceCodeInfo {
    fn from(c: cinma_core::TraktDeviceCode) -> Self {
        Self {
            device_code: c.device_code,
            user_code: c.user_code,
            verification_url: c.verification_url,
            expires_in: c.expires_in,
            interval: c.interval,
        }
    }
}

#[derive(uniffi::Record)]
pub struct AdapterStatusInfo {
    pub adapter: String,
    pub enabled: bool,
    pub connected: bool,
    pub last_sync_at: Option<i64>,
    pub outbox_pending: i64,
}

impl From<cinma_core::AdapterStatus> for AdapterStatusInfo {
    fn from(a: cinma_core::AdapterStatus) -> Self {
        Self {
            adapter: a.adapter,
            enabled: a.enabled,
            connected: a.connected,
            last_sync_at: a.last_sync_at,
            outbox_pending: a.outbox_pending,
        }
    }
}

#[derive(uniffi::Record)]
pub struct TraktRecommendationInfo {
    pub title: String,
    pub year: Option<u32>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
}

impl From<cinma_core::TraktRecommendation> for TraktRecommendationInfo {
    fn from(r: cinma_core::TraktRecommendation) -> Self {
        Self {
            title: r.title,
            year: r.year,
            imdb_id: r.imdb_id,
            tmdb_id: r.tmdb_id,
        }
    }
}

#[derive(uniffi::Record)]
pub struct SkipSegmentInfo {
    pub kind: String,
    pub start_s: f64,
    pub end_s: f64,
    pub source: String,
}

impl From<cinma_core::SkipSegment> for SkipSegmentInfo {
    fn from(s: cinma_core::SkipSegment) -> Self {
        let kind = match s.kind {
            cinma_core::SkipKind::Intro => "intro",
            cinma_core::SkipKind::Outro => "outro",
            cinma_core::SkipKind::Recap => "recap",
            cinma_core::SkipKind::Preview => "preview",
        };
        Self {
            kind: kind.to_owned(),
            start_s: s.start_s,
            end_s: s.end_s,
            source: s.source,
        }
    }
}

#[derive(uniffi::Record)]
pub struct DebridProviderStatusInfo {
    pub provider: String,
    pub display_name: String,
    pub configured: bool,
}

impl From<cinma_core::DebridProviderStatus> for DebridProviderStatusInfo {
    fn from(d: cinma_core::DebridProviderStatus) -> Self {
        Self {
            provider: d.provider,
            display_name: d.display_name,
            configured: d.configured,
        }
    }
}

#[derive(uniffi::Record)]
pub struct OAuthStartInfo {
    pub provider: String,
    pub authorize_url: String,
    pub state: String,
}

impl From<cinma_core::OAuthStartResult> for OAuthStartInfo {
    fn from(o: cinma_core::OAuthStartResult) -> Self {
        Self {
            provider: o.provider,
            authorize_url: o.authorize_url,
            state: o.state,
        }
    }
}

#[derive(uniffi::Record)]
pub struct TraktPullSummaryInfo {
    pub watchlist_items: u32,
    pub rating_items: u32,
    pub history_items: u32,
}

impl From<cinma_core::TraktPullSummary> for TraktPullSummaryInfo {
    fn from(t: cinma_core::TraktPullSummary) -> Self {
        Self {
            watchlist_items: t.watchlist_items,
            rating_items: t.rating_items,
            history_items: t.history_items,
        }
    }
}

#[derive(uniffi::Record)]
pub struct OpenSubtitlesQuotaInfo {
    pub allowed_downloads: u32,
    pub remaining_downloads: u32,
    pub downloads_count: u32,
    pub level: String,
    pub vip: bool,
}

impl From<cinma_core::OsUserQuota> for OpenSubtitlesQuotaInfo {
    fn from(q: cinma_core::OsUserQuota) -> Self {
        Self {
            allowed_downloads: q.allowed_downloads,
            remaining_downloads: q.remaining_downloads,
            downloads_count: q.downloads_count,
            level: q.level,
            vip: q.vip,
        }
    }
}

#[derive(uniffi::Record)]
pub struct WatchProviderInfo {
    pub provider_id: i64,
    pub name: String,
    pub logo_url: Option<String>,
    pub kind: String,
}

impl From<cinma_core::WatchProvider> for WatchProviderInfo {
    fn from(p: cinma_core::WatchProvider) -> Self {
        Self {
            provider_id: p.provider_id,
            name: p.name,
            logo_url: p.logo_url,
            kind: p.kind,
        }
    }
}

#[derive(uniffi::Record)]
pub struct TitleExtrasInfo {
    pub clearlogo: Option<String>,
    pub fanart_backdrop: Option<String>,
    pub watch_providers: Vec<WatchProviderInfo>,
}

impl From<cinma_core::TitleExtras> for TitleExtrasInfo {
    fn from(e: cinma_core::TitleExtras) -> Self {
        Self {
            clearlogo: e.clearlogo,
            fanart_backdrop: e.fanart_backdrop,
            watch_providers: e.watch_providers.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct IptvSourceInfo {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    pub epg_url: Option<String>,
}

impl From<cinma_core::IptvSourceRow> for IptvSourceInfo {
    fn from(s: cinma_core::IptvSourceRow) -> Self {
        Self {
            id: s.id,
            name: s.name,
            kind: s.kind,
            url: s.url,
            username: s.username,
            epg_url: s.epg_url,
        }
    }
}

#[derive(uniffi::Record)]
pub struct IptvChannelInfo {
    pub id: i64,
    pub source_id: i64,
    pub stream_id: String,
    pub name: String,
    pub group_title: Option<String>,
    pub logo_url: Option<String>,
    pub stream_url: String,
    pub epg_channel_id: Option<String>,
    pub is_favorite: bool,
}

impl From<cinma_core::IptvChannelRow> for IptvChannelInfo {
    fn from(c: cinma_core::IptvChannelRow) -> Self {
        Self {
            id: c.id,
            source_id: c.source_id,
            stream_id: c.stream_id,
            name: c.name,
            group_title: c.group_title,
            logo_url: c.logo_url,
            stream_url: c.stream_url,
            epg_channel_id: c.epg_channel_id,
            is_favorite: c.is_favorite,
        }
    }
}

#[derive(uniffi::Record)]
pub struct IptvEpgInfo {
    pub channel_id: String,
    pub start_ts: i64,
    pub stop_ts: i64,
    pub title: String,
    pub description: Option<String>,
}

impl From<cinma_core::IptvEpgRow> for IptvEpgInfo {
    fn from(e: cinma_core::IptvEpgRow) -> Self {
        Self {
            channel_id: e.channel_id,
            start_ts: e.start_ts,
            stop_ts: e.stop_ts,
            title: e.title,
            description: e.description,
        }
    }
}

#[derive(uniffi::Record)]
pub struct VpnBindingInfo {
    pub bind_device_name: Option<String>,
    pub vpn_kill_switch: bool,
}

#[derive(uniffi::Record)]
pub struct LetterboxdEntryInfo {
    pub title: String,
    pub year: Option<u32>,
    pub watched_date: Option<String>,
    pub rating: Option<f32>,
    pub link: Option<String>,
}

impl From<cinma_core::LetterboxdEntry> for LetterboxdEntryInfo {
    fn from(e: cinma_core::LetterboxdEntry) -> Self {
        Self {
            title: e.title,
            year: e.year,
            watched_date: e.watched_date,
            rating: e.rating,
            link: e.link,
        }
    }
}

#[derive(uniffi::Record)]
pub struct StremioImportResultInfo {
    pub addons_imported: u32,
    pub library_imported: u32,
}

impl From<cinma_core::StremioImportResult> for StremioImportResultInfo {
    fn from(r: cinma_core::StremioImportResult) -> Self {
        Self {
            addons_imported: r.addons_imported,
            library_imported: r.library_imported,
        }
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

    /// Streams an active, paused, or completed download directly via `Blob::source()`.
    pub fn play_download(&self, download_id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.play_download(download_id)?)
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

    pub fn stream_stats(&self) -> StreamStatsInfo {
        self.inner.stream_stats().into()
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

    #[allow(clippy::too_many_arguments)]
    pub fn set_subtitle_style(
        &self,
        font: Option<String>,
        font_size: Option<u32>,
        color_hex: Option<String>,
        border_size: Option<u32>,
        border_color_hex: Option<String>,
        back_color_hex: Option<String>,
        bold: bool,
        pos: Option<u32>,
        ass_override: bool,
        opacity_pct: u32,
        box_opacity_pct: u32,
        align_x: String,
        secondary_pos: u32,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_style(
                font.as_deref(),
                font_size,
                color_hex.as_deref(),
                border_size,
                border_color_hex.as_deref(),
                back_color_hex.as_deref(),
                bold,
                pos,
                ass_override,
                opacity_pct,
                box_opacity_pct,
                &align_x,
                secondary_pos,
            )
            .map_err(CoreError::from)?)
    }

    pub fn set_subtitle_pos(&self, pos: u32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_pos(pos)
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

    pub fn set_volume_max(&self, max_pct: u32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_volume_max(max_pct)
            .map_err(CoreError::from)?)
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_muted(muted)
            .map_err(CoreError::from)?)
    }

    pub fn set_crop_mode(&self, mode: String) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_crop_mode(&mode)
            .map_err(CoreError::from)?)
    }

    pub fn set_audio_processing(
        &self,
        normalize: bool,
        downmix_stereo: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_processing(normalize, downmix_stereo)
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
