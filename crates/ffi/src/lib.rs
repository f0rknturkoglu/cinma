//! The only crate Swift sees. Thin UniFFI wrappers over `cinma-core`;
//! no business logic lives here.

mod phase9_ffi;
mod playback_ffi;
mod records_core;
mod records_integrations;
mod records_playback;

use std::path::PathBuf;
use std::sync::Arc;

use cinma_core::{CommandContext, Core, CoreError, Feature};

pub use playback_ffi::*;
pub use records_core::*;
pub use records_integrations::*;
pub use records_playback::*;

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

fn feature(name: &str) -> Result<Feature, CinmaError> {
    Feature::from_name(name).ok_or_else(|| CinmaError::Invalid {
        message: format!("unknown feature {name}"),
    })
}

#[derive(uniffi::Object)]
pub struct CinmaCore {
    pub(crate) inner: Arc<Core>,
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

    pub fn library_meta_ids(&self) -> Result<Vec<String>, CinmaError> {
        Ok(self.inner.library_meta_ids()?)
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

    pub fn mark_episodes_watched(
        &self,
        meta_id: String,
        media_type: String,
        items: Vec<BatchEpisodeMarkFfi>,
        watched: bool,
    ) -> Result<(), CinmaError> {
        let mapped: Vec<(String, String, i64, i64)> = items
            .into_iter()
            .map(|e| (e.video_id, e.title, e.season, e.episode))
            .collect();
        Ok(self
            .inner
            .mark_episodes_watched(&meta_id, &media_type, &mapped, watched)?)
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

    pub async fn discover_by_provider(
        &self,
        media_type: String,
        provider_id: u32,
        page: u32,
    ) -> Result<Vec<MetaPreviewInfo>, CinmaError> {
        Ok(self
            .inner
            .discover_by_provider(media_type, provider_id, page)
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

    pub fn clear_series_source(&self, meta_id: String) -> Result<(), CinmaError> {
        Ok(self.inner.clear_series_source(&meta_id)?)
    }

    pub fn manual_skip_markers(&self, meta_id: String) -> Result<ManualSkipMarkersFfi, CinmaError> {
        Ok(self.inner.manual_skip_markers(&meta_id)?.into())
    }

    pub fn set_manual_skip_markers(
        &self,
        meta_id: String,
        markers: ManualSkipMarkersFfi,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .set_manual_skip_markers(&meta_id, &markers.into())?)
    }

    pub fn search_subtitle_cues(
        &self,
        path: String,
        query: String,
        limit: u32,
    ) -> Result<Vec<SubtitleCueFfi>, CinmaError> {
        Ok(self
            .inner
            .search_subtitle_cues(std::path::Path::new(&path), &query, limit as usize)?
            .into_iter()
            .map(Into::into)
            .collect())
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
