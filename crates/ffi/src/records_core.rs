use std::sync::Arc;

use cinma_core::CoreEvent;

use crate::CinmaError;

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

pub(crate) struct EventAdapter(pub(crate) Arc<dyn CoreEventListener>);

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

pub(crate) struct StreamBatchAdapter(pub(crate) Arc<dyn StreamBatchObserver>);

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

pub(crate) struct CatalogBatchAdapter(pub(crate) Arc<dyn CatalogBatchObserver>);

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
