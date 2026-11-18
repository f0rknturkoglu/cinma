use std::collections::HashSet;
use std::sync::Arc;

use cinma_db::MediaKind;
use cinma_debrid::DebridProvider;
use cinma_metadata::{SkipKind, SkipSegment, SkipSegmentClient};
use cinma_ranker::RankResult;
use cinma_stremio::Stream;

use super::{
    DebridProviderStatus, KEY_HOME_CATALOGS, KEY_PINNED_SHELVES, ManualSkipMarkers, PinnedShelf,
    SKIP_TTL_SECS, now_unix,
};
use crate::addons::StreamCandidate;
use crate::keychain::{get_debrid_key, set_debrid_key};
use crate::{Core, CoreError, Feature};

impl Core {
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
    // 12. Dalga 3 — Pinned Catalog Shelves (#N20) & Anime Filler Detection (#N22)
    // =========================================================================

    /// Addon catalogs chosen for Home, in display order, as
    /// `addon_id|type|catalog_id` keys. Empty means "automatic": the first
    /// browsable catalogs in addon order.
    pub fn home_catalog_keys(&self) -> Result<Vec<String>, CoreError> {
        self.load_profile_json(KEY_HOME_CATALOGS)
    }

    pub fn set_home_catalog_keys(&self, keys: Vec<String>) -> Result<(), CoreError> {
        let mut seen = std::collections::HashSet::new();
        let clean: Vec<String> = keys
            .into_iter()
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty() && seen.insert(k.clone()))
            .collect();
        self.save_profile_json(KEY_HOME_CATALOGS, &clean)
    }

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
}
