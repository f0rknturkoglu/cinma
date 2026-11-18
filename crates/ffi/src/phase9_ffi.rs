use std::path::PathBuf;

use crate::{
    AdapterStatusInfo, CastDeviceInfo, CinmaCore, CinmaError, DebridProviderStatusInfo,
    DiskPolicySettingsInfo, DownloadInfo, IptvChannelInfo, IptvEpgInfo, IptvSourceInfo,
    LetterboxdEntryInfo, LocalMediaEntryInfo, MovieCollectionInfo, OAuthStartInfo,
    OpenSubtitlesQuotaInfo, PersonProfileInfo, PinnedShelfFfi, RankPreferencesInput,
    RankResultInfo, SkipSegmentInfo, StreamInfo, StremioImportResultInfo, TitleExtrasInfo,
    TorrentBatchItemInput, TorrentFileItemInfo, TraktDeviceCodeInfo, TraktPullSummaryInfo,
    TraktRecommendationInfo, VpnBindingInfo, WebDavEntryInfo, WebDavSourceFfi,
};

#[uniffi::export]
impl CinmaCore {
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

    pub fn parse_magnet_stream(&self, uri: String) -> Result<StreamInfo, CinmaError> {
        let stream = self.inner.parse_magnet_stream(&uri)?;
        Ok(cinma_core::StreamCandidate {
            addon_name: "Magnet".to_owned(),
            stream,
        }
        .into())
    }

    pub fn parse_torrent_file_stream(&self, path: String) -> Result<StreamInfo, CinmaError> {
        let stream = self
            .inner
            .parse_torrent_file_stream(std::path::Path::new(&path))?;
        Ok(cinma_core::StreamCandidate {
            addon_name: "Torrent File".to_owned(),
            stream,
        }
        .into())
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
        current.seeding_mode = s.seeding_mode;
        current.seeding_ratio = s.seeding_ratio;
        current.seeding_minutes = s.seeding_minutes;
        current.upload_limit_kbps = s.upload_limit_kbps;
        current.download_limit_kbps = s.download_limit_kbps;
        Ok(self.inner.set_disk_policy_settings(&current)?)
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

    pub async fn title_collection(
        &self,
        media_type: String,
        id: String,
    ) -> Result<Option<MovieCollectionInfo>, CinmaError> {
        Ok(self
            .inner
            .title_collection(media_type, id)
            .await?
            .map(Into::into))
    }

    pub async fn person_credits(
        &self,
        person_name: String,
    ) -> Result<Option<PersonProfileInfo>, CinmaError> {
        Ok(self
            .inner
            .person_credits(person_name)
            .await?
            .map(Into::into))
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

    pub fn iptv_all_epg_now_next(&self) -> Result<Vec<IptvEpgInfo>, CinmaError> {
        Ok(self
            .inner
            .iptv_all_epg_now_next()?
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

    pub fn pinned_shelves(&self) -> Result<Vec<PinnedShelfFfi>, CinmaError> {
        Ok(self
            .inner
            .pinned_shelves()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn pin_shelf(
        &self,
        title: String,
        addon_id: String,
        media_type: String,
        catalog_id: String,
        genre: Option<String>,
    ) -> Result<PinnedShelfFfi, CinmaError> {
        Ok(self
            .inner
            .pin_shelf(title, addon_id, media_type, catalog_id, genre)?
            .into())
    }

    pub fn unpin_shelf(&self, id: String) -> Result<(), CinmaError> {
        Ok(self.inner.unpin_shelf(&id)?)
    }

    pub async fn list_torrent_files(
        &self,
        info_hash: String,
        trackers: Vec<String>,
    ) -> Result<Vec<TorrentFileItemInfo>, CinmaError> {
        Ok(self
            .inner
            .list_torrent_files(info_hash, trackers)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn queue_torrent_batch(
        &self,
        info_hash: String,
        trackers: Vec<String>,
        meta_id: Option<String>,
        media_type: Option<String>,
        items: Vec<TorrentBatchItemInput>,
        priority: i64,
    ) -> Result<Vec<i64>, CinmaError> {
        let pairs = items
            .into_iter()
            .map(|i| (i.file_idx as usize, i.title))
            .collect();
        Ok(self
            .inner
            .queue_torrent_batch(info_hash, trackers, meta_id, media_type, pairs, priority)?)
    }

    pub async fn anime_filler_episodes(&self, meta_id: String) -> Result<Vec<u32>, CinmaError> {
        Ok(self.inner.anime_filler_episodes(meta_id).await?)
    }

    pub fn local_media_entries(&self) -> Result<Vec<LocalMediaEntryInfo>, CinmaError> {
        Ok(self
            .inner
            .local_media_entries()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn scan_local_folders(&self) -> Result<Vec<LocalMediaEntryInfo>, CinmaError> {
        Ok(self
            .inner
            .scan_local_folders()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn webdav_sources(&self) -> Result<Vec<WebDavSourceFfi>, CinmaError> {
        Ok(self
            .inner
            .webdav_sources()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn add_webdav_source(
        &self,
        name: String,
        base_url: String,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<WebDavSourceFfi, CinmaError> {
        Ok(self
            .inner
            .add_webdav_source(name, base_url, username, password)?
            .into())
    }

    pub fn remove_webdav_source(&self, id: String) -> Result<(), CinmaError> {
        Ok(self.inner.remove_webdav_source(&id)?)
    }

    pub async fn browse_webdav(
        &self,
        source_id: String,
        sub_path: String,
    ) -> Result<Vec<WebDavEntryInfo>, CinmaError> {
        Ok(self
            .inner
            .browse_webdav(source_id, sub_path)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn enforce_download_schedule(&self, current_hour: Option<u8>) -> Result<(), CinmaError> {
        Ok(self.inner.enforce_download_schedule(current_hour)?)
    }

    pub fn local_api_socket_path(&self) -> String {
        self.inner.local_api_socket_path()
    }
}
