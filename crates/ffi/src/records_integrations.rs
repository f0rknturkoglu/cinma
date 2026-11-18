use crate::{CinmaError, MetaPreviewInfo};

#[derive(uniffi::Record, Clone)]
pub struct PinnedShelfFfi {
    pub id: String,
    pub title: String,
    pub addon_id: String,
    pub media_type: String,
    pub catalog_id: String,
    pub genre: Option<String>,
}

impl From<cinma_core::PinnedShelf> for PinnedShelfFfi {
    fn from(s: cinma_core::PinnedShelf) -> Self {
        Self {
            id: s.id,
            title: s.title,
            addon_id: s.addon_id,
            media_type: s.media_type,
            catalog_id: s.catalog_id,
            genre: s.genre,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct TorrentFileItemInfo {
    pub file_idx: u64,
    pub name: String,
    pub size_bytes: u64,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl From<cinma_core::TorrentFileItem> for TorrentFileItemInfo {
    fn from(f: cinma_core::TorrentFileItem) -> Self {
        Self {
            file_idx: f.index as u64,
            name: f.name,
            size_bytes: f.size_bytes,
            season: f.season,
            episode: f.episode,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct TorrentBatchItemInput {
    pub file_idx: u64,
    pub title: String,
}

#[derive(uniffi::Record, Clone)]
pub struct LocalMediaEntryInfo {
    pub path: String,
    pub filename: String,
    pub title: String,
    pub year: Option<String>,
    pub media_type: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub size_bytes: u64,
}

impl From<cinma_core::LocalMediaEntry> for LocalMediaEntryInfo {
    fn from(e: cinma_core::LocalMediaEntry) -> Self {
        Self {
            path: e.path,
            filename: e.filename,
            title: e.title,
            year: e.year,
            media_type: e.media_type,
            season: e.season,
            episode: e.episode,
            size_bytes: e.size_bytes,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct WebDavSourceFfi {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub username: Option<String>,
}

impl From<cinma_core::WebDavSource> for WebDavSourceFfi {
    fn from(s: cinma_core::WebDavSource) -> Self {
        Self {
            id: s.id,
            name: s.name,
            base_url: s.base_url,
            username: s.username,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct WebDavEntryInfo {
    pub name: String,
    pub sub_path: String,
    pub stream_url: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

impl From<cinma_core::WebDavEntry> for WebDavEntryInfo {
    fn from(e: cinma_core::WebDavEntry) -> Self {
        Self {
            name: e.name,
            sub_path: e.sub_path,
            stream_url: e.stream_url,
            is_dir: e.is_dir,
            size_bytes: e.size_bytes,
        }
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
    pub seeding_mode: String,
    pub seeding_ratio: f64,
    pub seeding_minutes: u64,
    pub upload_limit_kbps: u32,
    pub download_limit_kbps: u32,
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
            seeding_mode: s.seeding_mode,
            seeding_ratio: s.seeding_ratio,
            seeding_minutes: s.seeding_minutes,
            upload_limit_kbps: s.upload_limit_kbps,
            download_limit_kbps: s.download_limit_kbps,
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

#[derive(uniffi::Record, Clone)]
pub struct BatchEpisodeMarkFfi {
    pub video_id: String,
    pub title: String,
    pub season: i64,
    pub episode: i64,
}

#[derive(uniffi::Record, Clone)]
pub struct ManualSkipMarkersFfi {
    pub intro_end_s: Option<f64>,
    pub outro_last_s: Option<f64>,
}

impl From<cinma_core::ManualSkipMarkers> for ManualSkipMarkersFfi {
    fn from(m: cinma_core::ManualSkipMarkers) -> Self {
        Self {
            intro_end_s: m.intro_end_s,
            outro_last_s: m.outro_last_s,
        }
    }
}

impl From<ManualSkipMarkersFfi> for cinma_core::ManualSkipMarkers {
    fn from(m: ManualSkipMarkersFfi) -> Self {
        Self {
            intro_end_s: m.intro_end_s,
            outro_last_s: m.outro_last_s,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SubtitleCueFfi {
    pub start_s: f64,
    pub end_s: f64,
    pub text: String,
}

impl From<cinma_core::SubtitleCue> for SubtitleCueFfi {
    fn from(c: cinma_core::SubtitleCue) -> Self {
        Self {
            start_s: c.start_s,
            end_s: c.end_s,
            text: c.text,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MovieCollectionInfo {
    pub id: i64,
    pub name: String,
    pub overview: Option<String>,
    pub parts: Vec<MetaPreviewInfo>,
}

impl From<cinma_core::MovieCollection> for MovieCollectionInfo {
    fn from(c: cinma_core::MovieCollection) -> Self {
        Self {
            id: c.id,
            name: c.name,
            overview: c.overview,
            parts: c.parts.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct PersonProfileInfo {
    pub id: i64,
    pub name: String,
    pub biography: Option<String>,
    pub birthday: Option<String>,
    pub place_of_birth: Option<String>,
    pub profile_image: Option<String>,
    pub known_for: Option<String>,
    pub credits: Vec<MetaPreviewInfo>,
}

impl From<cinma_core::PersonProfile> for PersonProfileInfo {
    fn from(p: cinma_core::PersonProfile) -> Self {
        Self {
            id: p.id,
            name: p.name,
            biography: p.biography,
            birthday: p.birthday,
            place_of_birth: p.place_of_birth,
            profile_image: p.profile_image,
            known_for: p.known_for,
            credits: p.credits.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub description: String,
}

impl From<cinma_core::AudioDeviceEntry> for AudioDeviceInfo {
    fn from(d: cinma_core::AudioDeviceEntry) -> Self {
        Self {
            name: d.name,
            description: d.description,
        }
    }
}

#[derive(uniffi::Record)]
pub struct RawFrameFfi {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgr0: Vec<u8>,
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
