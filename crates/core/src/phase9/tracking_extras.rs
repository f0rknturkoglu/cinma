use std::sync::Arc;

use cinma_db::{IdMapEntry, MediaKind};
use cinma_metadata::{FanartArt, FanartClient, KitsuClient, MovieCollection, PersonProfile};
use cinma_subtitles::{OpenSubtitlesClient, OsUserQuota};
use cinma_tracking::{
    OAuthProvider, PendingOAuthFlow, TrackingClient, build_oauth_request, parse_oauth_callback,
};

use super::{OAuthStartResult, TitleExtras, TraktPullSummary};
use crate::keychain::{
    get_fanart_api_key, get_opensubtitles_api_key, get_opensubtitles_token, get_tracking_token,
    set_fanart_api_key, set_opensubtitles_token, set_tracking_token,
};
use crate::library::WatchProgressInput;
use crate::{Core, CoreError, Feature, RichMetaResult};

impl Core {
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
}
