//! Metadata orchestration: TMDB primary (Keychain key) + Cinemeta/addon
//! fallback, SQLite TTL cache, cross-provider ID map, and calendar air dates.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cinma_db::MediaKind;
use cinma_metadata::{
    CachedRichMeta, RichMeta, TmdbClient, UpcomingEpisode, parse_tmdb_prefixed_id,
};
use cinma_stremio::{CatalogQuery, Meta, MetaPreview, MetaVideo};

use crate::{Core, CoreError, Feature, keychain};

const META_TTL_SECS: i64 = 24 * 60 * 60;
const LIST_TTL_SECS: i64 = 6 * 60 * 60;
const SEASON_TTL_SECS: i64 = 12 * 60 * 60;
const LANG_SETTING_KEY: &str = "metadata.language";

#[derive(Debug, Clone, PartialEq)]
pub struct RichMetaResult {
    pub source: String,
    pub meta: Meta,
    pub season_numbers: Vec<u32>,
    pub next_episode: Option<UpcomingEpisode>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Core {
    pub fn metadata_language(&self) -> Result<String, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db
            .get_profile_setting(profile, LANG_SETTING_KEY)?
            .unwrap_or_else(|| "en-US".to_owned()))
    }

    pub fn set_metadata_language(&self, lang: &str) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        db.set_profile_setting(profile, LANG_SETTING_KEY, lang.trim())?;
        Ok(())
    }

    pub fn has_tmdb_key(&self) -> Result<bool, CoreError> {
        Ok(self.tmdb_key()?.is_some())
    }

    pub fn tmdb_key(&self) -> Result<Option<String>, CoreError> {
        if let Some(override_val) = self
            .tmdb_key_override
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
        {
            return Ok(override_val.clone());
        }
        keychain::get_tmdb_api_key()
    }

    pub fn set_tmdb_key(&self, key: Option<String>) -> Result<(), CoreError> {
        let trimmed = key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
        keychain::set_tmdb_api_key(trimmed.as_deref())?;
        *self
            .tmdb_key_override
            .lock()
            .map_err(|_| CoreError::Poisoned)? = None;
        Ok(())
    }

    /// Test helper: sets a TMDB key and base URL in memory without touching
    /// the macOS Keychain.
    pub fn configure_tmdb_for_test(
        &self,
        base_url: &str,
        api_key: Option<String>,
    ) -> Result<(), CoreError> {
        *self.tmdb.lock().map_err(|_| CoreError::Poisoned)? =
            Some(TmdbClient::with_base_url(base_url)?);
        *self
            .tmdb_key_override
            .lock()
            .map_err(|_| CoreError::Poisoned)? = Some(api_key);
        Ok(())
    }

    fn tmdb_client(&self) -> Result<TmdbClient, CoreError> {
        let mut slot = self.tmdb.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(client) = slot.as_ref() {
            return Ok(client.clone());
        }
        let client = TmdbClient::new()?;
        *slot = Some(client.clone());
        Ok(client)
    }

    pub async fn verify_tmdb_key(self: &Arc<Self>, api_key: String) -> Result<(), CoreError> {
        let client = self.tmdb_client()?;
        self.spawn(async move { Ok(client.verify_key(&api_key).await?) })
            .await
    }

    /// Reads cached home/trending rows from SQLite without touching the network (§5.16).
    pub fn cached_trending(&self, media_type: &str) -> Result<Vec<MetaPreview>, CoreError> {
        let lang = self.metadata_language()?;
        let key = format!("trending:{media_type}:{lang}");
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let Some(raw) = db.get_meta_cache(&key, now_unix())? else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    /// Trending items: TMDB when a key is present, otherwise the first browsable
    /// Stremio catalog for `media_type`.
    pub async fn trending(
        self: &Arc<Self>,
        media_type: String,
    ) -> Result<Vec<MetaPreview>, CoreError> {
        let lang = self.metadata_language()?;
        let cache_key = format!("trending:{media_type}:{lang}");
        let now = now_unix();
        if let Some(raw) = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_meta_cache(&cache_key, now)?
        {
            if let Ok(items) = serde_json::from_str::<Vec<MetaPreview>>(&raw) {
                return Ok(items);
            }
        }

        if let Some(api_key) = self.tmdb_key()? {
            let client = self.tmdb_client()?;
            let mt = media_type.clone();
            let lang_req = lang.clone();
            if let Ok(items) = self
                .spawn(async move { Ok(client.trending(&api_key, &mt, &lang_req).await?) })
                .await
            {
                if let Ok(json) = serde_json::to_string(&items) {
                    let _ = self
                        .db
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .put_meta_cache(&cache_key, "tmdb", &json, now + LIST_TTL_SECS, now);
                }
                return Ok(items);
            }
        }

        // Keyless fallback: first browsable Stremio catalog matching `media_type`.
        if self.feature_enabled(Feature::Addons)? {
            let first = self
                .catalogs()?
                .into_iter()
                .find(|c| c.catalog.is_browsable() && c.catalog.media_type == media_type);
            if let Some(cat) = first {
                let page = self
                    .catalog(
                        cat.addon_id,
                        cat.catalog.media_type,
                        cat.catalog.id,
                        CatalogQuery::default(),
                    )
                    .await?;
                if let Ok(json) = serde_json::to_string(&page.items) {
                    let _ = self
                        .db
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .put_meta_cache(&cache_key, "addon", &json, now + LIST_TTL_SECS, now);
                }
                return Ok(page.items);
            }
        }
        Ok(Vec::new())
    }

    pub async fn search_tmdb(
        self: &Arc<Self>,
        query: String,
        media_type: Option<String>,
    ) -> Result<Vec<MetaPreview>, CoreError> {
        let trimmed = query.trim().to_owned();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let Some(api_key) = self.tmdb_key()? else {
            return Ok(Vec::new());
        };
        let lang = self.metadata_language()?;
        let client = self.tmdb_client()?;
        self.spawn(async move {
            Ok(client
                .search(&api_key, &trimmed, media_type.as_deref(), &lang)
                .await?)
        })
        .await
    }

    /// Rich metadata: checks `meta_cache`, then TMDB (if key set), then Stremio
    /// addons (Cinemeta fallback).
    pub async fn rich_meta(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<RichMetaResult, CoreError> {
        let lang = self.metadata_language()?;
        let cache_key = format!("meta:{media_type}:{id}:{lang}");
        let now = now_unix();
        if let Some(raw) = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_meta_cache(&cache_key, now)?
        {
            if let Ok(cached) = serde_json::from_str::<CachedRichMeta>(&raw) {
                return Ok(RichMetaResult {
                    source: "cache".into(),
                    meta: cached.meta,
                    season_numbers: cached.season_numbers,
                    next_episode: cached.next_episode,
                });
            }
        }

        if let Some(api_key) = self.tmdb_key()? {
            if let Ok(mut rich) = self
                .fetch_tmdb_rich(&api_key, &media_type, &id, &lang)
                .await
            {
                if rich.meta.media_type == "series"
                    && let Some(&first_season) = rich.season_numbers.first()
                {
                    let base_id = rich.meta.id.clone();
                    if let Some(tmdb_id) = rich.id_map.tmdb_id {
                        let client = self.tmdb_client()?;
                        let key = api_key.clone();
                        let l = lang.clone();
                        if let Ok(eps) = self
                            .spawn(async move {
                                Ok(client
                                    .tv_season(&key, tmdb_id, &base_id, first_season, &l)
                                    .await?)
                            })
                            .await
                        {
                            rich.meta.videos = eps;
                        }
                    }
                }
                self.persist_rich_meta(&cache_key, "tmdb", &rich, now)?;
                return Ok(RichMetaResult {
                    source: "tmdb".into(),
                    meta: rich.meta,
                    season_numbers: rich.season_numbers,
                    next_episode: rich.next_episode,
                });
            }
        }

        // Fallback to Stremio addon meta (e.g. Cinemeta).
        let addon_id = self.resolve_imdb_or_id(&media_type, &id)?;
        let addon_res = self.meta(media_type, addon_id).await?;
        let mut seasons: Vec<u32> = addon_res
            .meta
            .videos
            .iter()
            .filter_map(|v| v.season)
            .filter(|s| *s > 0)
            .collect();
        seasons.sort_unstable();
        seasons.dedup();
        let cached = CachedRichMeta {
            meta: addon_res.meta.clone(),
            season_numbers: seasons.clone(),
            next_episode: None,
        };
        if let Ok(json) = serde_json::to_string(&cached) {
            let _ = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .put_meta_cache(
                    &cache_key,
                    &addon_res.addon_name,
                    &json,
                    now + LIST_TTL_SECS,
                    now,
                );
        }
        Ok(RichMetaResult {
            source: addon_res.addon_name,
            meta: addon_res.meta,
            season_numbers: seasons,
            next_episode: None,
        })
    }

    pub async fn season_episodes(
        self: &Arc<Self>,
        media_type: String,
        meta_id: String,
        season: u32,
    ) -> Result<Vec<MetaVideo>, CoreError> {
        let lang = self.metadata_language()?;
        let cache_key = format!("season:{media_type}:{meta_id}:s{season}:{lang}");
        let now = now_unix();
        if let Some(raw) = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_meta_cache(&cache_key, now)?
        {
            if let Ok(eps) = serde_json::from_str::<Vec<MetaVideo>>(&raw) {
                return Ok(eps);
            }
        }

        if let Some(api_key) = self.tmdb_key()? {
            if let Some((tmdb_id, base_stream_id)) = self
                .resolve_tmdb_and_base(&api_key, &media_type, &meta_id, &lang)
                .await?
            {
                let client = self.tmdb_client()?;
                let l = lang.clone();
                let eps = self
                    .spawn(async move {
                        Ok(client
                            .tv_season(&api_key, tmdb_id, &base_stream_id, season, &l)
                            .await?)
                    })
                    .await?;
                if let Ok(json) = serde_json::to_string(&eps) {
                    let _ = self
                        .db
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .put_meta_cache(&cache_key, "tmdb", &json, now + SEASON_TTL_SECS, now);
                }
                return Ok(eps);
            }
        }

        let rich = self.rich_meta(media_type, meta_id).await?;
        Ok(rich
            .meta
            .videos
            .into_iter()
            .filter(|v| v.season == Some(season))
            .collect())
    }

    /// Upcoming episodes for all series in the active profile's library.
    pub async fn calendar_entries(self: &Arc<Self>) -> Result<Vec<UpcomingEpisode>, CoreError> {
        let items: Vec<_> = self
            .library_items()?
            .into_iter()
            .filter(|i| i.media_type == "series")
            .collect();
        let mut upcoming = Vec::new();
        for item in items {
            if let Ok(rich) = self.rich_meta("series".into(), item.meta_id).await
                && let Some(ep) = rich.next_episode
                && !ep.air_date.is_empty()
            {
                upcoming.push(ep);
            }
        }
        upcoming.sort_by(|a, b| a.air_date.cmp(&b.air_date));
        Ok(upcoming)
    }

    async fn fetch_tmdb_rich(
        self: &Arc<Self>,
        api_key: &str,
        media_type: &str,
        id: &str,
        lang: &str,
    ) -> Result<RichMeta, CoreError> {
        let Some((tmdb_id, _)) = self
            .resolve_tmdb_and_base(api_key, media_type, id, lang)
            .await?
        else {
            return Err(CoreError::Invalid(format!(
                "cannot resolve TMDB ID for {id}"
            )));
        };
        let client = self.tmdb_client()?;
        let key = api_key.to_owned();
        let l = lang.to_owned();
        let is_series = matches!(media_type, "series" | "tv");
        self.spawn(async move {
            if is_series {
                Ok(client.tv_details(&key, tmdb_id, &l).await?)
            } else {
                Ok(client.movie_details(&key, tmdb_id, &l).await?)
            }
        })
        .await
    }

    async fn resolve_tmdb_and_base(
        self: &Arc<Self>,
        api_key: &str,
        media_type: &str,
        id: &str,
        lang: &str,
    ) -> Result<Option<(i64, String)>, CoreError> {
        let kind = if matches!(media_type, "series" | "tv") {
            MediaKind::Series
        } else {
            MediaKind::Movie
        };
        if let Some(tmdb_id) = parse_tmdb_prefixed_id(id) {
            let imdb = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .find_id_map_by_tmdb(kind, tmdb_id)?
                .and_then(|e| e.imdb_id);
            let base = imdb.unwrap_or_else(|| id.to_owned());
            return Ok(Some((tmdb_id, base)));
        }
        if id.starts_with("tt") {
            if let Some(mapped) = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .find_id_map_by_imdb(kind, id)?
                && let Some(tmdb_id) = mapped.tmdb_id
            {
                return Ok(Some((tmdb_id, id.to_owned())));
            }
            let client = self.tmdb_client()?;
            let key = api_key.to_owned();
            let imdb = id.to_owned();
            let l = lang.to_owned();
            if let Some(found) = self
                .spawn(async move { Ok(client.find_by_imdb(&key, &imdb, &l).await?) })
                .await?
            {
                return Ok(Some((found.tmdb_id, id.to_owned())));
            }
        }
        Ok(None)
    }

    fn resolve_imdb_or_id(&self, media_type: &str, id: &str) -> Result<String, CoreError> {
        if let Some(tmdb_id) = parse_tmdb_prefixed_id(id) {
            let kind = if matches!(media_type, "series" | "tv") {
                MediaKind::Series
            } else {
                MediaKind::Movie
            };
            if let Some(entry) = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .find_id_map_by_tmdb(kind, tmdb_id)?
                && let Some(imdb) = entry.imdb_id
            {
                return Ok(imdb);
            }
        }
        Ok(id.to_owned())
    }

    fn persist_rich_meta(
        &self,
        cache_key: &str,
        source: &str,
        rich: &RichMeta,
        now: i64,
    ) -> Result<(), CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        db.upsert_id_map(&rich.id_map)?;
        let payload = CachedRichMeta {
            meta: rich.meta.clone(),
            season_numbers: rich.season_numbers.clone(),
            next_episode: rich.next_episode.clone(),
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            db.put_meta_cache(cache_key, source, &json, now + META_TTL_SECS, now)?;
        }
        // Also cache under canonical IMDb ID if requested via `tmdb:<id>`.
        if let Some(ref imdb) = rich.id_map.imdb_id {
            let alt_key = format!(
                "meta:{}:{}:{}",
                rich.meta.media_type,
                imdb,
                cache_key.rsplit(':').next().unwrap_or("tr-TR")
            );
            if alt_key != cache_key
                && let Ok(json) = serde_json::to_string(&payload)
            {
                let _ = db.put_meta_cache(&alt_key, source, &json, now + META_TTL_SECS, now);
            }
        }
        Ok(())
    }
}
