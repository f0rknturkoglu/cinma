//! Playback session: resolves streams to byte sources, drives mpv, exposes
//! non-blocking seek-bar thumbnails & OpenSubtitles hashing, and handles
//! next-episode prefetching (`CLAUDE.md` §5.2, §5.8, §5.9).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cinma_blobstore::{
    ArchiveByteSource, ByteSource, CancellationToken, compute_opensubtitles_hash_file,
    compute_opensubtitles_hash_nonblocking, detect_archive_kind,
};
use cinma_player::{Player, PlayerConfig, PlayerEvents, ThumbnailExtractor, ThumbnailImage};
use cinma_stremio::Stream;
use cinma_torrent::{TorrentEngine, TorrentFileRequest};
use tokio::task::AbortHandle;

use crate::addons::StreamCandidate;
use crate::{Core, CoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFileIdentity {
    pub hash: Option<String>,
    pub size: Option<u64>,
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamStats {
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

#[derive(Debug, Clone)]
pub struct NextEpisodePrefetch {
    pub video_id: String,
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub still: Option<String>,
    pub best_stream: Option<StreamCandidate>,
}

type PrefetchedSource = (String, Option<u32>, Arc<dyn ByteSource>);

/// One player bound to a host view. Dropping it (or `stop`) destroys mpv and
/// cancels all background prefetch/thumbnail state.
pub struct Playback {
    core: Arc<Core>,
    player: Player,
    source: Mutex<Option<Arc<dyn ByteSource>>>,
    local_path: Mutex<Option<PathBuf>>,
    source_url: Mutex<Option<String>>,
    thumbs: Mutex<Option<Arc<ThumbnailExtractor>>>,
    cached_identity: Mutex<Option<VideoFileIdentity>>,
    prefetch_abort: Mutex<Option<AbortHandle>>,
    prefetch_source: Mutex<Option<PrefetchedSource>>,
    prefetched_next: Mutex<Option<NextEpisodePrefetch>>,
}

impl Core {
    /// Creates the player (loads libmpv on first use). Blocking — call off
    /// the main thread.
    pub fn create_playback(
        self: &Arc<Self>,
        events: Arc<dyn PlayerEvents>,
    ) -> Result<Playback, CoreError> {
        self.create_playback_for_surface(events, true)
    }

    pub fn create_playback_for_surface(
        self: &Arc<Self>,
        events: Arc<dyn PlayerEvents>,
        hdr_surface: bool,
    ) -> Result<Playback, CoreError> {
        let prefs = self.player_preferences().unwrap_or_default();
        let config = PlayerConfig {
            hdr_surface: (hdr_surface && prefs.mac_edr_enabled) || prefs.force_hdr_surface,
            hdr_target_peak: prefs.hdr_target_peak,
            exact_seek: prefs.exact_seek,
            gl_advanced_control: true,
            shader_paths: self.resolve_shader_paths(&prefs.shader_preset)?,
            tone_mapping: prefs.hdr_tone_mapping,
            force_yuv420p: prefs.force_yuv420p,
            scale_preset: prefs.mpv_scale_preset,
            hwdec_mode: prefs.mpv_hwdec_mode,
            demuxer_max_mb: prefs.mpv_buffer_mb,
            extra_options: prefs.mpv_extra_options,
        };
        self.create_playback_with_config(events, config)
    }

    pub fn create_playback_with_config(
        self: &Arc<Self>,
        events: Arc<dyn PlayerEvents>,
        config: PlayerConfig,
    ) -> Result<Playback, CoreError> {
        Ok(Playback {
            core: Arc::clone(self),
            player: Player::new_with_config(events, config)?,
            source: Mutex::new(None),
            local_path: Mutex::new(None),
            source_url: Mutex::new(None),
            thumbs: Mutex::new(None),
            cached_identity: Mutex::new(None),
            prefetch_abort: Mutex::new(None),
            prefetch_source: Mutex::new(None),
            prefetched_next: Mutex::new(None),
        })
    }

    /// Resolves the next episode (`season`, `episode`, `video_id`) from rich series metadata.
    pub async fn resolve_next_episode(
        self: &Arc<Self>,
        meta_id: &str,
        current_season: u32,
        current_episode: u32,
    ) -> Result<Option<NextEpisodePrefetch>, CoreError> {
        let prefs = self.player_preferences().unwrap_or_default();
        let filler_eps: std::collections::HashSet<u32> = if prefs.auto_skip_anime_fillers {
            self.anime_filler_episodes(meta_id.to_owned())
                .await
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            std::collections::HashSet::new()
        };
        let eps = self
            .season_episodes("series".to_owned(), meta_id.to_owned(), current_season)
            .await
            .unwrap_or_default();
        let check_fillers = !filler_eps.is_empty()
            && (current_season <= 1
                || eps
                    .iter()
                    .filter_map(|e| e.episode.or(e.number))
                    .min()
                    .unwrap_or(1)
                    > 1);
        if let Some(next_ep) = eps
            .into_iter()
            .filter(|e| {
                let n = e.episode.or(e.number).unwrap_or(0);
                n > current_episode && (!check_fillers || !filler_eps.contains(&n))
            })
            .min_by_key(|e| e.episode.or(e.number).unwrap_or(u32::MAX))
        {
            let ep_num = next_ep
                .episode
                .or(next_ep.number)
                .unwrap_or(current_episode + 1);
            return Ok(Some(NextEpisodePrefetch {
                video_id: next_ep.id,
                season: next_ep.season.unwrap_or(current_season),
                episode: ep_num,
                title: next_ep.title.unwrap_or_else(|| format!("Episode {ep_num}")),
                still: next_ep.thumbnail,
                best_stream: None,
            }));
        }

        let rich = self
            .rich_meta("series".to_owned(), meta_id.to_owned())
            .await?;
        let next_season = rich
            .season_numbers
            .into_iter()
            .filter(|&s| s > current_season)
            .min();
        let Some(ns) = next_season else {
            return Ok(None);
        };
        let next_season_eps = self
            .season_episodes("series".to_owned(), meta_id.to_owned(), ns)
            .await
            .unwrap_or_default();
        Ok(next_season_eps
            .into_iter()
            .min_by_key(|e| e.episode.or(e.number).unwrap_or(u32::MAX))
            .map(|first| {
                let ep_num = first.episode.or(first.number).unwrap_or(1);
                NextEpisodePrefetch {
                    video_id: first.id,
                    season: first.season.unwrap_or(ns),
                    episode: ep_num,
                    title: first.title.unwrap_or_else(|| format!("Episode {ep_num}")),
                    still: first.thumbnail,
                    best_stream: None,
                }
            }))
    }
}

fn wrap_archive_if_needed(source: Arc<dyn ByteSource>) -> Arc<dyn ByteSource> {
    if detect_archive_kind(source.filename()).is_some()
        && let Ok(wrapped) =
            ArchiveByteSource::open_largest_video(Arc::clone(&source), &CancellationToken::new())
    {
        return Arc::new(wrapped);
    }
    source
}

impl Playback {
    fn take_matching_prefetch_source(
        &self,
        info_hash: &str,
        file_idx: Option<u32>,
    ) -> Option<Arc<dyn ByteSource>> {
        let mut guard = self.prefetch_source.lock().ok()?;
        let matches = guard
            .as_ref()
            .is_some_and(|(h, idx, _)| h.eq_ignore_ascii_case(info_hash) && *idx == file_idx);
        if matches {
            guard.take().map(|(_, _, src)| src)
        } else {
            None
        }
    }

    /// Resolves `stream` (debrid instant cache if configured, else torrent →
    /// byte source, or URL → direct) and starts it.
    pub async fn play(&self, stream: Stream) -> Result<(), CoreError> {
        let reused_prefetch = stream
            .info_hash
            .as_deref()
            .and_then(|hash| self.take_matching_prefetch_source(hash, stream.file_idx));
        self.reset_media_state();
        self.player.begin_load();
        if let Ok(Some(debrid_url)) = self.core.try_resolve_debrid_stream(&stream).await {
            return self.play_url(&debrid_url);
        }
        if let Some(info_hash) = stream.info_hash.clone() {
            let source = if let Some(src) = reused_prefetch {
                src
            } else {
                let engine = self.core.torrents()?;
                let request = TorrentFileRequest {
                    info_hash,
                    file_idx: stream.file_idx.map(|i| i as usize),
                    trackers: stream.trackers(),
                };
                self.core
                    .spawn(async move {
                        let src = engine.open_file(request).await?;
                        Ok(wrap_archive_if_needed(src))
                    })
                    .await?
            };
            if let Ok(mut guard) = self.source.lock() {
                *guard = Some(Arc::clone(&source));
            }
            self.player.load_source(source)?;
            Ok(())
        } else if let Some(url) = stream.url.as_deref() {
            self.play_url(url)
        } else {
            Err(CoreError::Invalid(
                "stream has neither infoHash nor url".into(),
            ))
        }
    }

    /// Loads a local file path or HTTP(S) URL into mpv.
    pub fn play_url(&self, url: &str) -> Result<(), CoreError> {
        self.reset_media_state();
        self.player.begin_load();
        let local = url.strip_prefix("file://").unwrap_or(url);
        if Path::new(local).is_file()
            && let Ok(mut guard) = self.local_path.lock()
        {
            *guard = Some(PathBuf::from(local));
        } else if let Ok(mut guard) = self.source_url.lock() {
            *guard = Some(url.to_owned());
        }
        self.player.load_url(url)?;
        Ok(())
    }

    /// Plays a download: a finished file directly (mpv opens it), otherwise
    /// the bytes so far (blob or live torrent) through the stream protocol.
    pub fn play_download(&self, download_id: i64) -> Result<(), CoreError> {
        let finished = self
            .core
            .downloads()?
            .into_iter()
            .find(|r| r.id == download_id)
            .filter(|r| r.blob_id.is_none())
            .and_then(|r| r.file_path)
            .filter(|p| Path::new(p).is_file());
        if let Some(path) = finished {
            return self.play_url(&path);
        }
        self.reset_media_state();
        self.player.begin_load();
        let source = self.core.download_byte_source(download_id)?;
        if let Ok(mut guard) = self.source.lock() {
            *guard = Some(Arc::clone(&source));
        }
        self.player.load_source(source)?;
        Ok(())
    }

    fn reset_media_state(&self) {
        self.cancel_prefetch();
        if let Ok(mut g) = self.source.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.local_path.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.source_url.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.thumbs.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.cached_identity.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.prefetched_next.lock() {
            *g = None;
        }
    }

    /// Non-blocking check for OpenSubtitles hash, byte size, and file name.
    /// Returns cached hash once head + tail (64 KiB each) have arrived.
    pub fn video_identity(&self) -> Option<VideoFileIdentity> {
        if let Ok(guard) = self.cached_identity.lock()
            && let Some(id) = guard.as_ref()
            && id.hash.is_some()
        {
            return Some(id.clone());
        }

        if let Some(source) = self.source.lock().ok().and_then(|g| g.clone()) {
            let hash = compute_opensubtitles_hash_nonblocking(source.as_ref());
            let id = VideoFileIdentity {
                hash: hash.clone(),
                size: Some(source.size()),
                filename: source.filename().map(str::to_owned),
            };
            if hash.is_some()
                && let Ok(mut guard) = self.cached_identity.lock()
            {
                *guard = Some(id.clone());
            }
            return Some(id);
        }

        if let Some(path) = self.local_path.lock().ok().and_then(|g| g.clone()) {
            let size = std::fs::metadata(&path).ok().map(|m| m.len());
            let hash = compute_opensubtitles_hash_file(&path).ok().flatten();
            let filename = path.file_name().and_then(|s| s.to_str()).map(str::to_owned);
            let id = VideoFileIdentity {
                hash: hash.clone(),
                size,
                filename,
            };
            if hash.is_some()
                && let Ok(mut guard) = self.cached_identity.lock()
            {
                *guard = Some(id.clone());
            }
            return Some(id);
        }

        None
    }

    /// Decodes a ~160 px keyframe thumbnail around `seconds` from already-available
    /// bytes only (no network/torrent fetches).
    pub fn thumbnail_at(&self, seconds: f64) -> Option<ThumbnailImage> {
        let extractor = {
            let mut guard = self.thumbs.lock().ok()?;
            if guard.is_none() {
                if let Some(source) = self.source.lock().ok().and_then(|g| g.clone()) {
                    *guard = Some(Arc::new(ThumbnailExtractor::from_source(source)));
                } else if let Some(path) = self.local_path.lock().ok().and_then(|g| g.clone())
                    && let Ok(ext) = ThumbnailExtractor::from_file(&path)
                {
                    *guard = Some(Arc::new(ext));
                }
            }
            guard.clone()?
        };
        extractor.thumbnail_at(seconds)
    }

    /// Starts background prefetching for the next episode (streams + ranker +
    /// optional bingeGroup match + head/tail warm-up). Cancelled automatically
    /// if playback stops.
    pub fn start_next_episode_prefetch(
        self: &Arc<Self>,
        meta_id: String,
        current_season: u32,
        current_episode: u32,
        binge_group: Option<String>,
    ) {
        self.cancel_prefetch();
        let Ok(rt) = self.core.runtime() else {
            return;
        };
        let weak_self = Arc::downgrade(self);
        let core = Arc::clone(&self.core);
        let handle = rt
            .spawn(async move {
                let Ok(Some(mut plan)) = core
                    .resolve_next_episode(&meta_id, current_season, current_episode)
                    .await
                else {
                    return;
                };
                if let Ok(streams) = core
                    .streams("series".to_owned(), plan.video_id.clone())
                    .await
                    && let Ok(ranked) = core.rank_streams_with_debrid(streams, None).await
                {
                    let prefs = core.player_preferences().unwrap_or_default();
                    let saved_src = core.series_source(&meta_id).unwrap_or_default();
                    let chosen_ranked = if prefs.keep_source_next_episode {
                        let target_bg = binge_group.as_deref().or(saved_src.binge_group.as_deref());
                        let binge_match = target_bg.and_then(|bg| {
                            ranked.ranked.iter().find(|r| {
                                r.stream
                                    .behavior_hints
                                    .as_ref()
                                    .and_then(|h| h.binge_group.as_deref())
                                    == Some(bg)
                            })
                        });
                        let addon_res_match = saved_src.addon_name.as_deref().and_then(|addon| {
                            saved_src.resolution.as_deref().and_then(|res| {
                                ranked.ranked.iter().find(|r| {
                                    r.addon_name == addon && r.parsed.resolution.label() == res
                                })
                            })
                        });
                        let addon_match = saved_src
                            .addon_name
                            .as_deref()
                            .and_then(|addon| ranked.ranked.iter().find(|r| r.addon_name == addon));
                        binge_match
                            .or(addon_res_match)
                            .or(addon_match)
                            .or_else(|| ranked.ranked.first())
                    } else {
                        ranked.ranked.first()
                    };
                    let chosen_cached_debrid =
                        chosen_ranked.is_some_and(|r| r.parsed.cached_debrid);
                    let chosen = chosen_ranked.map(|r| StreamCandidate {
                        addon_name: r.addon_name.clone(),
                        stream: r.stream.clone(),
                    });
                    if !chosen_cached_debrid
                        && let Some(cand) = &chosen
                        && let Some(hash) = &cand.stream.info_hash
                        && let Ok(engine) = core.torrents()
                    {
                        let req = TorrentFileRequest {
                            info_hash: hash.clone(),
                            file_idx: cand.stream.file_idx.map(|i| i as usize),
                            trackers: cand.stream.trackers(),
                        };
                        if let Ok(src) = engine.open_file(req).await
                            && let Some(this) = weak_self.upgrade()
                            && let Ok(mut g) = this.prefetch_source.lock()
                        {
                            *g = Some((
                                hash.to_ascii_lowercase(),
                                cand.stream.file_idx,
                                wrap_archive_if_needed(src),
                            ));
                        }
                    }
                    plan.best_stream = chosen;
                }
                if let Some(this) = weak_self.upgrade()
                    && let Ok(mut g) = this.prefetched_next.lock()
                {
                    *g = Some(plan);
                }
            })
            .abort_handle();
        if let Ok(mut guard) = self.prefetch_abort.lock() {
            *guard = Some(handle);
        }
    }

    pub fn prefetched_next_episode(&self) -> Option<NextEpisodePrefetch> {
        self.prefetched_next.lock().ok().and_then(|g| g.clone())
    }

    pub fn cancel_prefetch(&self) {
        if let Ok(mut guard) = self.prefetch_abort.lock()
            && let Some(h) = guard.take()
        {
            h.abort();
        }
        if let Ok(mut guard) = self.prefetch_source.lock() {
            *guard = None;
        }
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn stream_stats(&self) -> StreamStats {
        if let Some(source) = self.source.lock().ok().and_then(|g| g.clone()) {
            let st = source.stats().unwrap_or_default();
            return StreamStats {
                is_torrent: st.is_torrent,
                info_hash: st.info_hash,
                filename: source.filename().map(str::to_owned),
                source_url: None,
                download_speed_bps: st.download_speed_bps,
                upload_speed_bps: st.upload_speed_bps,
                peers_connected: st.peers_connected,
                peers_queued: st.peers_queued,
                peers_seen: st.peers_seen,
                downloaded_bytes: st.downloaded_bytes,
                total_bytes: if st.total_bytes > 0 {
                    st.total_bytes
                } else {
                    source.size()
                },
                finished: st.finished,
            };
        }
        if let Some(path) = self.local_path.lock().ok().and_then(|g| g.clone()) {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            return StreamStats {
                filename: path.file_name().and_then(|s| s.to_str()).map(str::to_owned),
                downloaded_bytes: size,
                total_bytes: size,
                finished: true,
                ..StreamStats::default()
            };
        }
        let url = self.source_url.lock().ok().and_then(|g| g.clone());
        StreamStats {
            source_url: url,
            ..StreamStats::default()
        }
    }

    /// Destroys mpv, cancels prefetching, and stops torrent activity (unless
    /// a background torrent download is active) so the app goes back to idle
    /// and trims native heap pages.
    pub async fn stop(&self) -> Result<(), CoreError> {
        self.reset_media_state();
        self.player.shutdown();
        // Any profile's torrent download keeps the session alive.
        let has_torrent_download = self
            .core
            .db
            .lock()
            .ok()
            .and_then(|db| db.has_unfinished_torrent_downloads().ok())
            .unwrap_or(false);
        let engine = self
            .core
            .torrents
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .clone();
        if let Some(engine) = engine {
            self.core
                .spawn(async move {
                    if has_torrent_download {
                        // Keep the session for the download; drop only what we streamed.
                        engine.release_streams().await;
                    } else {
                        engine.release_all().await;
                    }
                    Ok(())
                })
                .await?;
        }
        cinma_player::release_heap_pressure();
        Ok(())
    }
}
