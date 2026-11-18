//! Phase 9 Integrations (`docs/ROADMAP.md` §8, `docs/adr/0017`):
//! - Skip segments (`Feature::SkipSegments`): AniSkip + IntroDB with SQLite 7-day TTL
//! - Debrid resolvers (`Feature::Debrid`): Real-Debrid, AllDebrid, Premiumize, TorBox
//! - Tracking OAuth PKCE (Simkl, AniList, MAL) & two-way Trakt pull + Anime ID refresh
//! - OpenSubtitles account login & quota
//! - Fanart.tv logos/backdrops, TMDB watch providers, Kitsu anime fallback
//! - Live TV / IPTV (`Feature::Iptv`): M3U, Xtream Codes, XMLTV EPG
//! - Discord Rich Presence (`Feature::Discord`): in-process Unix domain socket IPC
//! - VPN interface binding & kill switch settings update

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cinma_db::{IdMapEntry, IptvChannelRow, IptvEpgRow, IptvSourceRow, MediaKind};
use cinma_debrid::{DebridClient, DebridProvider};
use cinma_discord::{DiscordPresenceClient, PresenceActivity};
use cinma_iptv::IptvClient;
use cinma_metadata::{
    FanartArt, FanartClient, KitsuClient, MovieCollection, PersonProfile, SkipKind, SkipSegment,
    SkipSegmentClient, WatchProvider,
};
use cinma_ranker::RankResult;
use cinma_stremio::Stream;
use cinma_subtitles::{OpenSubtitlesClient, OsUserQuota};
use cinma_tracking::{
    OAuthProvider, PendingOAuthFlow, TrackingClient, build_oauth_request, parse_oauth_callback,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::addons::StreamCandidate;
use crate::keychain::{
    get_debrid_key, get_fanart_api_key, get_opensubtitles_api_key, get_opensubtitles_token,
    get_tracking_token, set_debrid_key, set_fanart_api_key, set_opensubtitles_token,
    set_tracking_token,
};
use crate::library::WatchProgressInput;
use crate::{Core, CoreError, Feature, RichMetaResult};

const SKIP_TTL_SECS: i64 = 7 * 24 * 3600;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ManualSkipMarkers {
    pub intro_end_s: Option<f64>,
    pub outro_last_s: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebridProviderStatus {
    pub provider: String,
    pub display_name: String,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthStartResult {
    pub provider: String,
    pub authorize_url: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraktPullSummary {
    pub watchlist_items: u32,
    pub rating_items: u32,
    pub history_items: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordPresenceInput {
    pub title: String,
    pub subtitle: Option<String>,
    pub duration_s: Option<u64>,
    pub position_s: u64,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleExtras {
    pub clearlogo: Option<String>,
    pub fanart_backdrop: Option<String>,
    pub watch_providers: Vec<WatchProvider>,
}

impl Core {
    // =========================================================================
    // Lazy feature accessors (never constructed when disabled)
    // =========================================================================

    fn skip_client(&self) -> Result<SkipSegmentClient, CoreError> {
        self.require(Feature::SkipSegments)?;
        let mut guard = self.skip_client.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let c = SkipSegmentClient::new()?;
        *guard = Some(c.clone());
        Ok(c)
    }

    fn debrid_client(&self) -> Result<DebridClient, CoreError> {
        self.require(Feature::Debrid)?;
        let mut guard = self.debrid_client.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let c = DebridClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        *guard = Some(c.clone());
        Ok(c)
    }

    fn iptv_client(&self) -> Result<IptvClient, CoreError> {
        self.require(Feature::Iptv)?;
        let mut guard = self.iptv_client.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let c = IptvClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        *guard = Some(c.clone());
        Ok(c)
    }

    fn discord_client(&self) -> Result<Arc<AsyncMutex<DiscordPresenceClient>>, CoreError> {
        self.require(Feature::Discord)?;
        let mut guard = self
            .discord_client
            .lock()
            .map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = guard.as_ref() {
            return Ok(Arc::clone(c));
        }
        let c = Arc::new(AsyncMutex::new(DiscordPresenceClient::new(None)));
        *guard = Some(Arc::clone(&c));
        Ok(c)
    }

    // =========================================================================
    // 1. Skip intro/outro (AniSkip + IntroDB, SQLite TTL cache)
    // =========================================================================

    pub async fn skip_segments(
        self: &Arc<Self>,
        media_type: String,
        meta_id: String,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<Vec<SkipSegment>, CoreError> {
        let manual = self.manual_skip_markers(&meta_id).unwrap_or_default();
        let mut out = if self.feature_enabled(Feature::SkipSegments)? {
            let key = format!(
                "skip:{media_type}:{meta_id}:{}:{}",
                season.unwrap_or(0),
                episode.unwrap_or(0)
            );
            let min_ts = now_unix() - SKIP_TTL_SECS;
            if let Some(row) = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .get_skip_segment_cache(&key, min_ts)?
                && let Ok(segs) = serde_json::from_str::<Vec<SkipSegment>>(&row.segments_json)
            {
                segs
            } else {
                let (imdb_id, mal_id) = self.resolve_skip_ids(&media_type, &meta_id)?;
                if imdb_id.is_none() && mal_id.is_none() {
                    Vec::new()
                } else {
                    let client = self.skip_client()?;
                    let ep = episode.unwrap_or(1);
                    let segs = self
                        .spawn(async move {
                            let mut combined = Vec::new();
                            if let Some(mal) = mal_id
                                && let Ok(mut ani) = client.fetch_aniskip(mal, ep).await
                            {
                                combined.append(&mut ani);
                            }
                            if let Some(imdb) = imdb_id
                                && let Ok(mut intro) =
                                    client.fetch_introdb(&imdb, season, episode).await
                            {
                                combined.append(&mut intro);
                            }
                            Ok(combined)
                        })
                        .await?;

                    if let Ok(json) = serde_json::to_string(&segs) {
                        let _ = self
                            .db
                            .lock()
                            .map_err(|_| CoreError::Poisoned)?
                            .set_skip_segment_cache(&key, &json, now_unix());
                    }
                    segs
                }
            }
        } else {
            Vec::new()
        };

        if let Some(intro_end_s) = manual.intro_end_s.filter(|v| *v > 1.0) {
            out.retain(|s| s.kind != SkipKind::Intro);
            out.insert(
                0,
                SkipSegment {
                    kind: SkipKind::Intro,
                    start_s: 0.0,
                    end_s: intro_end_s,
                    source: "Manual".to_owned(),
                },
            );
        }
        Ok(out)
    }

    pub fn manual_skip_markers(&self, meta_id: &str) -> Result<ManualSkipMarkers, CoreError> {
        let base_id = meta_id.split(':').next().unwrap_or(meta_id);
        self.load_profile_json(&format!("manual_skip.{base_id}"))
    }

    pub fn set_manual_skip_markers(
        &self,
        meta_id: &str,
        markers: &ManualSkipMarkers,
    ) -> Result<(), CoreError> {
        let base_id = meta_id.split(':').next().unwrap_or(meta_id);
        self.save_profile_json(&format!("manual_skip.{base_id}"), markers)
    }

    fn resolve_skip_ids(
        &self,
        media_type: &str,
        meta_id: &str,
    ) -> Result<(Option<String>, Option<i64>), CoreError> {
        let base_id = meta_id.split(':').next().unwrap_or(meta_id);
        let kind = if media_type == "series" || media_type == "tv" {
            MediaKind::Series
        } else {
            MediaKind::Movie
        };
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        if base_id.starts_with("tt") {
            let mapped = db.find_id_map_by_imdb(kind, base_id)?;
            let mal = mapped.and_then(|m| m.mal_id);
            return Ok((Some(base_id.to_owned()), mal));
        }
        if let Some(tmdb_id) = cinma_metadata::parse_tmdb_prefixed_id(base_id)
            && let Some(m) = db.find_id_map_by_tmdb(kind, tmdb_id)?
        {
            return Ok((m.imdb_id, m.mal_id));
        }
        if let Some(kitsu_raw) = base_id.strip_prefix("kitsu:")
            && let Ok(kitsu_id) = kitsu_raw.parse::<i64>()
            && let Some(m) = db.find_id_map_by_kitsu(kitsu_id)?
        {
            return Ok((m.imdb_id, m.mal_id));
        }
        if let Some(mal_raw) = base_id.strip_prefix("mal:")
            && let Ok(mal_id) = mal_raw.parse::<i64>()
        {
            return Ok((None, Some(mal_id)));
        }
        Ok((None, None))
    }

    // =========================================================================
    // 2. Debrid Resolvers & Cached Stream Ranking
    // =========================================================================

    pub fn debrid_statuses(&self) -> Result<Vec<DebridProviderStatus>, CoreError> {
        let profile = self.active_profile()?;
        DebridProvider::ALL
            .into_iter()
            .map(|p| {
                Ok(DebridProviderStatus {
                    provider: p.id().to_owned(),
                    display_name: p.display_name().to_owned(),
                    configured: get_debrid_key(profile, p.id())?.is_some(),
                })
            })
            .collect()
    }

    pub fn set_debrid_key(&self, provider: String, key: Option<String>) -> Result<(), CoreError> {
        let p = DebridProvider::from_id(&provider)
            .ok_or_else(|| CoreError::Invalid(format!("unknown debrid provider: {provider}")))?;
        let profile = self.active_profile()?;
        set_debrid_key(profile, p.id(), key.as_deref())
    }

    pub async fn rank_streams_with_debrid(
        self: &Arc<Self>,
        candidates: Vec<StreamCandidate>,
        prefs: Option<cinma_ranker::RankerPreferences>,
    ) -> Result<RankResult, CoreError> {
        let mut cached_hashes = HashSet::new();
        if self.feature_enabled(Feature::Debrid)? {
            let hashes: Vec<String> = candidates
                .iter()
                .filter_map(|c| c.stream.info_hash.as_ref())
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            if !hashes.is_empty() {
                cached_hashes = self
                    .check_debrid_cache_hashes(hashes)
                    .await
                    .unwrap_or_default();
            }
        }
        Ok(self.rank_streams_for_debrid(candidates, &cached_hashes, prefs))
    }

    pub async fn check_debrid_cache_hashes(
        self: &Arc<Self>,
        hashes: Vec<String>,
    ) -> Result<HashSet<String>, CoreError> {
        if !self.feature_enabled(Feature::Debrid)? || hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let profile = self.active_profile()?;
        let mut keys = Vec::new();
        for p in DebridProvider::ALL {
            if let Some(k) = get_debrid_key(profile, p.id())? {
                keys.push((p, k));
            }
        }
        if keys.is_empty() {
            return Ok(HashSet::new());
        }
        let client = self.debrid_client()?;
        self.spawn(async move {
            let mut all = HashSet::new();
            let mut set = tokio::task::JoinSet::new();
            for (p, k) in keys {
                let (c, h) = (client.clone(), hashes.clone());
                set.spawn(async move { c.check_instant_cache(p, &k, &h).await });
            }
            while let Some(res) = set.join_next().await {
                if let Ok(Ok(found)) = res {
                    all.extend(found);
                }
            }
            Ok(all)
        })
        .await
    }

    pub(crate) async fn try_resolve_debrid_stream(
        self: &Arc<Self>,
        stream: &Stream,
    ) -> Result<Option<String>, CoreError> {
        if !self.feature_enabled(Feature::Debrid)? {
            return Ok(None);
        }
        let Some(ref info_hash) = stream.info_hash else {
            return Ok(None);
        };
        let profile = self.active_profile()?;
        let mut keys = Vec::new();
        for p in DebridProvider::ALL {
            if let Some(k) = get_debrid_key(profile, p.id())? {
                keys.push((p, k));
            }
        }
        if keys.is_empty() {
            return Ok(None);
        }
        let client = self.debrid_client()?;
        let hash = info_hash.clone();
        let file_idx = stream.file_idx.map(|i| i as usize);
        self.spawn(async move {
            for (p, k) in keys {
                if let Ok(cached) = client
                    .check_instant_cache(p, &k, std::slice::from_ref(&hash))
                    .await
                    && cached.contains(&hash.to_ascii_lowercase())
                    && let Ok(res) = client.resolve_torrent(p, &k, &hash, file_idx).await
                {
                    return Ok(Some(res.url));
                }
            }
            Ok(None)
        })
        .await
    }

    // =========================================================================
    // 3. OAuth PKCE (Simkl / AniList / MAL), Two-Way Trakt Pull & Anime Map Refresh
    // =========================================================================

    pub fn begin_tracking_oauth(
        &self,
        provider: String,
        client_id: String,
        client_secret: String,
    ) -> Result<OAuthStartResult, CoreError> {
        self.require(Feature::Tracking)?;
        let prov = OAuthProvider::parse(&provider)
            .ok_or_else(|| CoreError::Invalid(format!("unknown OAuth provider: {provider}")))?;
        let flow = build_oauth_request(prov, &client_id, &client_secret)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let res = OAuthStartResult {
            provider: prov.as_str().to_owned(),
            authorize_url: flow.authorize_url.clone(),
            state: flow.state.clone(),
        };
        let mut pending = self.pending_oauth.lock().map_err(|_| CoreError::Poisoned)?;
        pending.insert(flow.state.clone(), flow);
        Ok(res)
    }

    pub async fn complete_tracking_oauth_callback(
        self: &Arc<Self>,
        callback_url: String,
    ) -> Result<String, CoreError> {
        self.require(Feature::Tracking)?;
        let (prov, code, state) =
            parse_oauth_callback(&callback_url).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let flow: PendingOAuthFlow = {
            let mut pending = self.pending_oauth.lock().map_err(|_| CoreError::Poisoned)?;
            pending.remove(&state).ok_or_else(|| {
                CoreError::Invalid("OAuth state expired or does not match an active sign-in".into())
            })?
        };
        if flow.provider != prov {
            return Err(CoreError::Invalid("OAuth provider mismatch".into()));
        }
        let client = TrackingClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        let token = self
            .spawn(async move {
                client
                    .exchange_oauth_code(&flow, &code, None)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let profile = self.active_profile()?;
        set_tracking_token(profile, prov.as_str(), Some(&token))?;
        self.set_tracking_adapter_enabled(prov.as_str(), true)?;
        Ok(prov.as_str().to_owned())
    }

    pub async fn pull_trakt_sync(self: &Arc<Self>) -> Result<TraktPullSummary, CoreError> {
        self.require(Feature::Tracking)?;
        let profile = self.active_profile()?;
        let token = get_tracking_token(profile, "trakt")?
            .ok_or_else(|| CoreError::Invalid("Trakt account is not signed in".into()))?;
        let client_id = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_app_setting("tracking.trakt.client_id")?
            .ok_or_else(|| CoreError::Invalid("Trakt Client ID is not configured".into()))?;
        let client = TrackingClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        let (wl, rt, hist) = self
            .spawn(async move {
                let wl = client
                    .trakt_pull_watchlist(&client_id, &token)
                    .await
                    .unwrap_or_default();
                let rt = client
                    .trakt_pull_ratings(&client_id, &token)
                    .await
                    .unwrap_or_default();
                let hist = client
                    .trakt_pull_history(&client_id, &token)
                    .await
                    .unwrap_or_default();
                Ok((wl, rt, hist))
            })
            .await?;

        let mut wl_n = 0u32;
        let mut rt_n = 0u32;
        let mut hist_n = 0u32;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;

        for entry in wl {
            let meta_id = entry
                .imdb_id
                .clone()
                .or_else(|| entry.tmdb_id.map(|id| format!("tmdb:{id}")));
            if let Some(id) = meta_id {
                let _ = db.upsert_id_map(&IdMapEntry {
                    media_kind: if entry.media_type == "series" {
                        MediaKind::Series
                    } else {
                        MediaKind::Movie
                    },
                    imdb_id: entry.imdb_id,
                    tmdb_id: entry.tmdb_id,
                    tvdb_id: entry.tvdb_id,
                    trakt_id: entry.trakt_id,
                    simkl_id: None,
                    mal_id: None,
                    anilist_id: None,
                    kitsu_id: None,
                    season: entry.season.map(i64::from),
                    episode: entry.episode.map(i64::from),
                });
                db.add_library_item(profile, &id, &entry.media_type, &entry.title, None)?;
                wl_n += 1;
            }
        }
        for entry in rt {
            if let Some(r) = entry.rating
                && let Some(id) = entry
                    .imdb_id
                    .clone()
                    .or_else(|| entry.tmdb_id.map(|id| format!("tmdb:{id}")))
            {
                db.add_library_item(profile, &id, &entry.media_type, &entry.title, None)?;
                let _ = db.set_profile_setting(
                    profile,
                    &format!("rating.{}.{}", entry.media_type, id),
                    &r.to_string(),
                );
                rt_n += 1;
            }
        }
        drop(db);

        for entry in hist {
            let Some(meta_id) = entry
                .imdb_id
                .clone()
                .or_else(|| entry.tmdb_id.map(|id| format!("tmdb:{id}")))
            else {
                continue;
            };
            let video_id = match (entry.season, entry.episode) {
                (Some(s), Some(e)) => format!("{meta_id}:{s}:{e}"),
                _ => meta_id.clone(),
            };
            let _ = self.save_watch_progress(WatchProgressInput {
                media_type: entry.media_type,
                meta_id,
                video_id,
                title: entry.title,
                artwork: None,
                season: entry.season.map(i64::from),
                episode: entry.episode.map(i64::from),
                position_s: 1.0,
                duration_s: 1.0,
            });
            hist_n += 1;
        }

        let _ = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .update_adapter_sync(profile, "trakt", None);
        Ok(TraktPullSummary {
            watchlist_items: wl_n,
            rating_items: rt_n,
            history_items: hist_n,
        })
    }

    // =========================================================================
    // 4. OpenSubtitles Account Login & Quota
    // =========================================================================

    pub async fn opensubtitles_login(
        self: &Arc<Self>,
        username: String,
        password: String,
    ) -> Result<OsUserQuota, CoreError> {
        let api_key = get_opensubtitles_api_key()?.ok_or_else(|| {
            CoreError::Invalid("OpenSubtitles API key must be saved first".into())
        })?;
        let client = OpenSubtitlesClient::new(api_key.clone(), None)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let session = self
            .spawn(async move {
                client
                    .login(&username, &password)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        set_opensubtitles_token(Some(&session.token))?;
        self.opensubtitles_quota().await
    }

    pub async fn opensubtitles_quota(self: &Arc<Self>) -> Result<OsUserQuota, CoreError> {
        let api_key = get_opensubtitles_api_key()?
            .ok_or_else(|| CoreError::Invalid("OpenSubtitles API key is not configured".into()))?;
        let token = get_opensubtitles_token()?
            .ok_or_else(|| CoreError::Invalid("Not signed in to OpenSubtitles".into()))?;
        let client = OpenSubtitlesClient::new(api_key, Some(token))
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        self.spawn(async move {
            client
                .user_info()
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn opensubtitles_logout(self: &Arc<Self>) -> Result<(), CoreError> {
        if let (Ok(Some(api_key)), Ok(Some(token))) =
            (get_opensubtitles_api_key(), get_opensubtitles_token())
            && let Ok(client) = OpenSubtitlesClient::new(api_key, Some(token))
        {
            let _ = self
                .spawn(async move {
                    let _ = client.logout().await;
                    Ok(())
                })
                .await;
        }
        set_opensubtitles_token(None)
    }

    // =========================================================================
    // 6 & 7. Fanart.tv, TMDB Watch Providers & Kitsu Anime Metadata
    // =========================================================================

    pub fn fanart_api_key(&self) -> Result<Option<String>, CoreError> {
        get_fanart_api_key()
    }

    pub fn set_fanart_api_key(&self, key: Option<String>) -> Result<(), CoreError> {
        set_fanart_api_key(key.as_deref())
    }

    pub async fn title_extras(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<TitleExtras, CoreError> {
        let lang = self.metadata_language()?;
        let lang_short = lang.split('-').next().unwrap_or("en").to_owned();
        let region = lang.split('-').nth(1).unwrap_or("US").to_ascii_uppercase();

        let (tmdb_id, imdb_id, tvdb_id) = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let kind = if media_type == "series" || media_type == "tv" {
                MediaKind::Series
            } else {
                MediaKind::Movie
            };
            let mapped = if id.starts_with("tt") {
                db.find_id_map_by_imdb(kind, &id)?
            } else if let Some(tid) = cinma_metadata::parse_tmdb_prefixed_id(&id) {
                db.find_id_map_by_tmdb(kind, tid)?
            } else {
                None
            };
            let tid = cinma_metadata::parse_tmdb_prefixed_id(&id)
                .or_else(|| mapped.as_ref().and_then(|m| m.tmdb_id));
            let imdb = mapped
                .as_ref()
                .and_then(|m| m.imdb_id.clone())
                .or_else(|| id.starts_with("tt").then(|| id.clone()));
            let tvdb = mapped.and_then(|m| m.tvdb_id);
            (tid, imdb, tvdb)
        };

        let mut providers = Vec::new();
        if let (Some(api_key), Some(tid)) = (self.tmdb_key()?, tmdb_id) {
            let client = self.tmdb_client()?;
            let mt = media_type.clone();
            let reg = region.clone();
            providers = self
                .spawn(async move { Ok(client.watch_providers(&api_key, &mt, tid, &reg).await?) })
                .await
                .unwrap_or_default();
        }

        let mut art = FanartArt::default();
        if let Some(fanart_key) = get_fanart_api_key()? {
            let lookup = if media_type == "series" || media_type == "tv" {
                tvdb_id.map(|v| v.to_string())
            } else {
                tmdb_id.map(|v| v.to_string()).or(imdb_id)
            };
            if let Some(lid) = lookup {
                let fc = FanartClient::new()?;
                let mt = media_type;
                art = self
                    .spawn(
                        async move { Ok(fc.fetch_art(&fanart_key, &mt, &lid, &lang_short).await?) },
                    )
                    .await
                    .unwrap_or_default();
            }
        }

        Ok(TitleExtras {
            clearlogo: art.logo,
            fanart_backdrop: art.backdrop,
            watch_providers: providers,
        })
    }

    pub async fn title_collection(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<Option<MovieCollection>, CoreError> {
        if media_type != "movie" {
            return Ok(None);
        }
        let Some(api_key) = self.tmdb_key()? else {
            return Ok(None);
        };
        let lang = self.metadata_language()?;
        let prefer_original = self
            .player_preferences()
            .map(|p| p.prefer_original_titles)
            .unwrap_or(false);
        let Some((tmdb_id, _)) = self
            .resolve_tmdb_and_base(&api_key, &media_type, &id, &lang)
            .await?
        else {
            return Ok(None);
        };
        let client = self.tmdb_client()?;
        self.spawn(async move {
            Ok(client
                .movie_collection(&api_key, tmdb_id, &lang, prefer_original)
                .await?)
        })
        .await
    }

    pub async fn person_credits(
        self: &Arc<Self>,
        person_name: String,
    ) -> Result<Option<PersonProfile>, CoreError> {
        let Some(api_key) = self.tmdb_key()? else {
            return Ok(None);
        };
        let lang = self.metadata_language()?;
        let prefer_original = self
            .player_preferences()
            .map(|p| p.prefer_original_titles)
            .unwrap_or(false);
        let client = self.tmdb_client()?;
        self.spawn(async move {
            Ok(client
                .person_credits(&api_key, &person_name, &lang, prefer_original)
                .await?)
        })
        .await
    }

    pub async fn kitsu_meta(self: &Arc<Self>, kitsu_id: i64) -> Result<RichMetaResult, CoreError> {
        let client = KitsuClient::new()?;
        let rich = self
            .spawn(async move { Ok(client.anime_details(kitsu_id).await?) })
            .await?;
        let _ = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .upsert_id_map(&rich.id_map);
        Ok(RichMetaResult {
            source: "Kitsu".into(),
            meta: rich.meta,
            season_numbers: rich.season_numbers,
            next_episode: rich.next_episode,
        })
    }

    // =========================================================================
    // 9. Live TV / IPTV (`Feature::Iptv`)
    // =========================================================================

    pub async fn add_m3u_source(
        self: &Arc<Self>,
        name: String,
        url: String,
        epg_url: Option<String>,
    ) -> Result<u32, CoreError> {
        let client = self.iptv_client()?;
        let fetch_url = url.clone();
        let channels = self
            .spawn(async move {
                client
                    .fetch_m3u(&fetch_url)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let profile = self.active_profile()?;
        let count = channels.len() as u32;
        {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let sid = db.upsert_iptv_source(
                profile,
                name.trim(),
                "m3u",
                url.trim(),
                None,
                epg_url.as_deref(),
            )?;
            db.replace_iptv_channels(profile, sid, &channels)?;
        }
        if let Some(epg) = epg_url.filter(|s| !s.trim().is_empty()) {
            let _ = self.refresh_xmltv(epg).await;
        }
        Ok(count)
    }

    pub async fn add_xtream_source(
        self: &Arc<Self>,
        name: String,
        base_url: String,
        username: String,
        password: String,
    ) -> Result<u32, CoreError> {
        let client = self.iptv_client()?;
        let (u, pw, base) = (username.clone(), password.clone(), base_url.clone());
        let channels = self
            .spawn(async move {
                client
                    .fetch_xtream_live(&base, &u, &pw)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let profile = self.active_profile()?;
        let count = channels.len() as u32;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let epg_url = format!(
            "{}/xmltv.php?username={username}&password={password}",
            base_url.trim_end_matches('/')
        );
        let sid = db.upsert_iptv_source(
            profile,
            name.trim(),
            "xtream",
            base_url.trim(),
            Some(username.trim()),
            Some(&epg_url),
        )?;
        db.replace_iptv_channels(profile, sid, &channels)?;
        Ok(count)
    }

    pub async fn refresh_xmltv(self: &Arc<Self>, url: String) -> Result<u32, CoreError> {
        let client = self.iptv_client()?;
        let rows = self
            .spawn(async move {
                client
                    .fetch_xmltv(&url)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let count = rows.len() as u32;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .upsert_iptv_epg(&rows, now_unix() - 3600)?;
        Ok(count)
    }

    pub fn list_iptv_sources(&self) -> Result<Vec<IptvSourceRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? {
            return Ok(Vec::new());
        }
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .list_iptv_sources(profile)?)
    }

    pub fn remove_iptv_source(&self, source_id: i64) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .remove_iptv_source(profile, source_id)?)
    }

    pub fn list_iptv_channels(
        &self,
        group: Option<String>,
        query: Option<String>,
    ) -> Result<Vec<IptvChannelRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? {
            return Ok(Vec::new());
        }
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .list_iptv_channels(profile, group.as_deref(), query.as_deref())?)
    }

    pub fn set_iptv_favorite(&self, channel_id: i64, favorite: bool) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_iptv_channel_favorite(profile, channel_id, favorite)?)
    }

    pub fn iptv_epg_now_next(&self, epg_channel_id: String) -> Result<Vec<IptvEpgRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? || epg_channel_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .current_and_next_epg(&epg_channel_id, now_unix())?)
    }

    pub fn iptv_all_epg_now_next(&self) -> Result<Vec<IptvEpgRow>, CoreError> {
        if !self.feature_enabled(Feature::Iptv)? {
            return Ok(Vec::new());
        }
        Ok(self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .current_and_next_epg_all(now_unix())?)
    }

    // =========================================================================
    // 10. Discord Rich Presence (`Feature::Discord`)
    // =========================================================================

    pub async fn update_discord_presence(
        self: &Arc<Self>,
        input: DiscordPresenceInput,
    ) -> Result<(), CoreError> {
        if !self.feature_enabled(Feature::Discord)? {
            return Ok(());
        }
        let prefs = self.player_preferences().unwrap_or_default();
        if input.paused && !prefs.discord_show_when_paused {
            return self.clear_discord_presence().await;
        }
        let client = self.discord_client()?;
        let now = now_unix();
        let start_ts = (!input.paused).then(|| now - input.position_s as i64);
        let end_ts = match (input.paused, input.duration_s) {
            (false, Some(dur)) if dur > input.position_s => {
                Some(now + (dur - input.position_s) as i64)
            }
            _ => None,
        };
        let (details, state) = if prefs.discord_hide_title {
            ("Watching on Cinma".to_owned(), None)
        } else {
            (input.title, input.subtitle)
        };
        let act = PresenceActivity {
            details,
            state,
            start_ts,
            end_ts,
            paused: input.paused,
        };
        self.spawn(async move {
            let mut g = client.lock().await;
            // Best-effort: Discord desktop client may not be open.
            let _ = g.set_activity(&act).await;
            Ok(())
        })
        .await
    }

    pub async fn clear_discord_presence(self: &Arc<Self>) -> Result<(), CoreError> {
        let existing = self
            .discord_client
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
            .map(Arc::clone);
        let Some(client) = existing else {
            return Ok(());
        };
        self.spawn(async move {
            let mut g = client.lock().await;
            let _ = g.clear().await;
            Ok(())
        })
        .await
    }

    // =========================================================================
    // 11. VPN Interface Binding & Kill Switch
    // =========================================================================

    pub async fn set_vpn_torrent_binding(
        self: &Arc<Self>,
        bind_device_name: Option<String>,
        vpn_kill_switch: bool,
    ) -> Result<(), CoreError> {
        let mut policy_settings = self.disk_policy_settings()?;
        policy_settings.bind_device_name = bind_device_name
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        policy_settings.vpn_kill_switch = vpn_kill_switch;
        self.set_disk_policy_settings(&policy_settings)?;

        let existing = {
            self.torrents
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .clone()
        };
        if let Some(engine) = existing {
            let tp = self.disk_policy_settings()?.torrent_policy(false);
            self.spawn(async move {
                engine.update_policy(tp).await;
                Ok(())
            })
            .await?;
        }
        Ok(())
    }

    pub(crate) fn rank_streams_for_debrid(
        &self,
        candidates: Vec<StreamCandidate>,
        debrid_cached_hashes: &HashSet<String>,
        prefs: Option<cinma_ranker::RankerPreferences>,
    ) -> RankResult {
        let raw = self.filter_blocked_candidates(candidates);
        let caps = crate::addons::machine_capabilities_pub();
        let result = cinma_ranker::rank_streams_with_debrid(
            raw,
            debrid_cached_hashes,
            &prefs.unwrap_or_default(),
            &caps,
        );
        self.finalize_rank_result(result)
    }

    // =========================================================================
    // 12. Dalga 3 — Pinned Catalog Shelves (#N20) & Anime Filler Detection (#N22)
    // =========================================================================

    pub fn pinned_shelves(&self) -> Result<Vec<PinnedShelf>, CoreError> {
        self.load_profile_json(KEY_PINNED_SHELVES)
    }

    pub fn pin_shelf(
        &self,
        title: String,
        addon_id: String,
        media_type: String,
        catalog_id: String,
        genre: Option<String>,
    ) -> Result<PinnedShelf, CoreError> {
        let clean_genre = genre.map(|g| g.trim().to_owned()).filter(|g| !g.is_empty());
        let id = format!(
            "shelf:{addon_id}:{media_type}:{catalog_id}:{}",
            clean_genre.as_deref().unwrap_or("")
        );
        let shelf = PinnedShelf {
            id: id.clone(),
            title: title.trim().to_owned(),
            addon_id,
            media_type,
            catalog_id,
            genre: clean_genre,
        };
        let mut all = self.pinned_shelves()?;
        all.retain(|s| s.id != id);
        all.push(shelf.clone());
        self.save_profile_json(KEY_PINNED_SHELVES, &all)?;
        Ok(shelf)
    }

    pub fn unpin_shelf(&self, id: &str) -> Result<(), CoreError> {
        let mut all = self.pinned_shelves()?;
        all.retain(|s| s.id != id);
        self.save_profile_json(KEY_PINNED_SHELVES, &all)
    }

    /// Returns MAL episode numbers marked as filler via Jikan v4 (7-day SQLite TTL).
    pub async fn anime_filler_episodes(
        self: &Arc<Self>,
        meta_id: String,
    ) -> Result<Vec<u32>, CoreError> {
        let (_, mal_opt) = self.resolve_skip_ids("series", &meta_id)?;
        let Some(mal_id) = mal_opt else {
            return Ok(Vec::new());
        };
        let key = format!("filler:{mal_id}");
        let min_ts = now_unix() - SKIP_TTL_SECS;
        if let Some(row) = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_skip_segment_cache(&key, min_ts)?
            && let Ok(fillers) = serde_json::from_str::<Vec<u32>>(&row.segments_json)
        {
            return Ok(fillers);
        }
        let client = SkipSegmentClient::new().map_err(CoreError::from)?;
        let fillers = self
            .spawn(async move {
                client
                    .fetch_anime_filler_episodes(mal_id)
                    .await
                    .map_err(CoreError::from)
            })
            .await?;
        if let Ok(json) = serde_json::to_string(&fillers) {
            let _ = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .set_skip_segment_cache(&key, &json, now_unix());
        }
        Ok(fillers)
    }
}

const KEY_PINNED_SHELVES: &str = "pinned_shelves";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PinnedShelf {
    pub id: String,
    pub title: String,
    pub addon_id: String,
    pub media_type: String,
    pub catalog_id: String,
    pub genre: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod p1_debrid_ranker_tests {
    use super::*;
    use crate::PlayerPreferences;

    #[test]
    fn rank_streams_for_debrid_applies_block_words_debrid_only_and_sort() {
        let dir = tempfile::tempdir().expect("tempdir");
        let core = Core::open(dir.path()).expect("open core");
        let prefs = PlayerPreferences {
            track_block_words: vec!["camrip".to_owned()],
            debrid_only_mode: true,
            stream_sort: "size_asc".to_owned(),
            ..Default::default()
        };
        core.set_player_preferences(&prefs).expect("set prefs");

        let cached_big = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let cached_small = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let uncached_hash = "cccccccccccccccccccccccccccccccccccccccc";
        let candidates = vec![
            StreamCandidate {
                addon_name: "Torrentio".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("1080p".to_owned()),
                    title: Some("Show 1080p 💾 5.0 GB".to_owned()),
                    info_hash: Some(cached_big.to_owned()),
                    ..Default::default()
                },
            },
            StreamCandidate {
                addon_name: "Torrentio".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("1080p".to_owned()),
                    title: Some("Show 1080p 💾 1.5 GB".to_owned()),
                    info_hash: Some(cached_small.to_owned()),
                    ..Default::default()
                },
            },
            StreamCandidate {
                addon_name: "TorrentioCAM".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("720p CAMRip".to_owned()),
                    title: Some("Show CAMRip 💾 500 MB".to_owned()),
                    info_hash: Some(cached_small.to_owned()),
                    ..Default::default()
                },
            },
            StreamCandidate {
                addon_name: "P2P".to_owned(),
                stream: cinma_stremio::Stream {
                    name: Some("4K".to_owned()),
                    title: Some("Show 2160p 👤 999 💾 2.0 GB".to_owned()),
                    info_hash: Some(uncached_hash.to_owned()),
                    ..Default::default()
                },
            },
        ];

        let mut cached = HashSet::new();
        cached.insert(cached_big.to_owned());
        cached.insert(cached_small.to_owned());

        let ranked = core.rank_streams_for_debrid(candidates, &cached, None);
        assert_eq!(ranked.ranked.len(), 2);
        assert_eq!(
            ranked.ranked[0].stream.info_hash.as_deref(),
            Some(cached_small)
        );
        assert_eq!(
            ranked.ranked[1].stream.info_hash.as_deref(),
            Some(cached_big)
        );
    }

    #[test]
    fn p2_batch_epg_returns_max_two_upcoming_rows_per_channel() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(core.set_feature_enabled(Feature::Iptv, true))
            .unwrap();
        let now = now_unix();
        {
            let db = core.db.lock().unwrap();
            db.upsert_iptv_epg(
                &[
                    cinma_db::IptvEpgRow {
                        channel_id: "ch1".into(),
                        start_ts: now - 100,
                        stop_ts: now + 100,
                        title: "CH1 Now".into(),
                        description: None,
                    },
                    cinma_db::IptvEpgRow {
                        channel_id: "ch1".into(),
                        start_ts: now + 100,
                        stop_ts: now + 200,
                        title: "CH1 Next".into(),
                        description: None,
                    },
                    cinma_db::IptvEpgRow {
                        channel_id: "ch1".into(),
                        start_ts: now + 200,
                        stop_ts: now + 300,
                        title: "CH1 Later".into(),
                        description: None,
                    },
                    cinma_db::IptvEpgRow {
                        channel_id: "ch2".into(),
                        start_ts: now - 50,
                        stop_ts: now + 50,
                        title: "CH2 Now".into(),
                        description: None,
                    },
                ],
                now - 3600,
            )
            .unwrap();
        }
        let rows = core.iptv_all_epg_now_next().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].title, "CH1 Now");
        assert_eq!(rows[1].title, "CH1 Next");
        assert_eq!(rows[2].title, "CH2 Now");
    }
}
