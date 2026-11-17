//! External tracking adapters (Trakt, Simkl, AniList, MyAnimeList, Letterboxd),
//! anime ID map helper, and one-time Stremio account import (`CLAUDE.md` §5.13–5.14).

#![forbid(unsafe_code)]

pub mod anime_map;
pub mod letterboxd;
pub mod oauth;
pub mod stremio_import;

use serde::{Deserialize, Serialize};

pub use oauth::{OAuthProvider, PendingOAuthFlow, build_oauth_request, parse_oauth_callback};

#[derive(Debug, thiserror::Error)]
pub enum TrackingError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{provider} returned HTTP {status}: {detail}")]
    HttpStatus {
        provider: &'static str,
        status: u16,
        detail: String,
    },
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScrobblePayload {
    pub media_type: String,
    pub title: String,
    pub year: Option<u32>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tvdb_id: Option<i64>,
    pub mal_id: Option<i64>,
    pub anilist_id: Option<i64>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub progress_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraktDeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraktSyncEntry {
    pub media_type: String,
    pub title: String,
    pub year: Option<u32>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tvdb_id: Option<i64>,
    pub trakt_id: Option<i64>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub rating: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraktRecommendation {
    pub title: String,
    pub year: Option<u32>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
}

#[derive(Clone)]
pub struct TrackingClient {
    http: reqwest::Client,
    trakt_base: String,
    simkl_base: String,
    anilist_url: String,
    mal_base: String,
    letterboxd_base: String,
}

impl TrackingClient {
    pub fn new() -> Result<Self, TrackingError> {
        let http = reqwest::Client::builder()
            .user_agent("Cinma/0.1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            http,
            trakt_base: "https://api.trakt.tv".to_owned(),
            simkl_base: "https://api.simkl.com".to_owned(),
            anilist_url: "https://graphql.anilist.co".to_owned(),
            mal_base: "https://api.myanimelist.net/v2".to_owned(),
            letterboxd_base: "https://letterboxd.com".to_owned(),
        })
    }

    pub fn with_base_urls(
        trakt_base: String,
        simkl_base: String,
        anilist_url: String,
        mal_base: String,
        letterboxd_base: String,
    ) -> Result<Self, TrackingError> {
        let mut c = Self::new()?;
        c.trakt_base = trakt_base.trim_end_matches('/').to_owned();
        c.simkl_base = simkl_base.trim_end_matches('/').to_owned();
        c.anilist_url = anilist_url;
        c.mal_base = mal_base.trim_end_matches('/').to_owned();
        c.letterboxd_base = letterboxd_base.trim_end_matches('/').to_owned();
        Ok(c)
    }

    // ---- Trakt ----

    pub async fn trakt_device_code(
        &self,
        client_id: &str,
    ) -> Result<TraktDeviceCode, TrackingError> {
        let body = serde_json::json!({ "client_id": client_id });
        let resp = self
            .http
            .post(format!("{}/oauth/device/code", self.trakt_base))
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let data = check_status("Trakt", resp).await?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub async fn trakt_poll_token(
        &self,
        client_id: &str,
        client_secret: &str,
        device_code: &str,
    ) -> Result<Option<String>, TrackingError> {
        let body = serde_json::json!({
            "code": device_code,
            "client_id": client_id,
            "client_secret": client_secret,
        });
        let resp = self
            .http
            .post(format!("{}/oauth/device/token", self.trakt_base))
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        if resp.status().as_u16() == 400 {
            // Authorization pending
            return Ok(None);
        }
        let data = check_status("Trakt", resp).await?;
        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
        }
        let parsed: TokenResp = serde_json::from_slice(&data)?;
        Ok(Some(parsed.access_token))
    }

    pub async fn trakt_scrobble(
        &self,
        client_id: &str,
        token: &str,
        action: &str, // "start" | "pause" | "stop"
        payload: &ScrobblePayload,
    ) -> Result<(), TrackingError> {
        let body = trakt_payload_json(payload);
        let resp = self
            .http
            .post(format!("{}/scrobble/{action}", self.trakt_base))
            .header("Content-Type", "application/json")
            .header("trakt-api-version", "2")
            .header("trakt-api-key", client_id)
            .bearer_auth(token)
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let _ = check_status("Trakt", resp).await?;
        Ok(())
    }

    pub async fn trakt_sync_history(
        &self,
        client_id: &str,
        token: &str,
        payload: &ScrobblePayload,
    ) -> Result<(), TrackingError> {
        let item = trakt_item_ids(payload);
        let body = if payload.media_type == "series" {
            serde_json::json!({
                "shows": [{ "ids": item, "seasons": [{ "number": payload.season.unwrap_or(1), "episodes": [{ "number": payload.episode.unwrap_or(1) }] }] }]
            })
        } else {
            serde_json::json!({ "movies": [{ "ids": item }] })
        };
        let resp = self
            .http
            .post(format!("{}/sync/history", self.trakt_base))
            .header("Content-Type", "application/json")
            .header("trakt-api-version", "2")
            .header("trakt-api-key", client_id)
            .bearer_auth(token)
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let _ = check_status("Trakt", resp).await?;
        Ok(())
    }

    pub async fn trakt_recommendations(
        &self,
        client_id: &str,
        token: &str,
    ) -> Result<Vec<TraktRecommendation>, TrackingError> {
        let resp = self
            .http
            .get(format!(
                "{}/recommendations/movies?limit=15",
                self.trakt_base
            ))
            .header("trakt-api-version", "2")
            .header("trakt-api-key", client_id)
            .bearer_auth(token)
            .send()
            .await?;
        let data = check_status("Trakt", resp).await?;
        #[derive(Deserialize)]
        struct Ids {
            imdb: Option<String>,
            tmdb: Option<i64>,
        }
        #[derive(Deserialize)]
        struct Entry {
            title: String,
            year: Option<u32>,
            ids: Ids,
        }
        let entries: Vec<Entry> = serde_json::from_slice(&data)?;
        Ok(entries
            .into_iter()
            .map(|e| TraktRecommendation {
                title: e.title,
                year: e.year,
                imdb_id: e.ids.imdb,
                tmdb_id: e.ids.tmdb,
            })
            .collect())
    }

    // ---- Simkl ----

    pub async fn simkl_sync_history(
        &self,
        client_id: &str,
        token: &str,
        payload: &ScrobblePayload,
    ) -> Result<(), TrackingError> {
        let ids = trakt_item_ids(payload);
        let body = if payload.media_type == "series" {
            serde_json::json!({
                "shows": [{ "ids": ids, "seasons": [{ "number": payload.season.unwrap_or(1), "episodes": [{ "number": payload.episode.unwrap_or(1) }] }] }]
            })
        } else {
            serde_json::json!({ "movies": [{ "ids": ids }] })
        };
        let resp = self
            .http
            .post(format!("{}/sync/history", self.simkl_base))
            .header("Content-Type", "application/json")
            .header("simkl-api-key", client_id)
            .bearer_auth(token)
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let _ = check_status("Simkl", resp).await?;
        Ok(())
    }

    // ---- AniList ----

    pub async fn anilist_update_progress(
        &self,
        token: &str,
        anilist_id: i64,
        progress: u32,
        completed: bool,
    ) -> Result<(), TrackingError> {
        let query = "mutation ($mediaId: Int, $progress: Int, $status: MediaListStatus) { \
                     SaveMediaListEntry(mediaId: $mediaId, progress: $progress, status: $status) { id } }";
        let status = if completed { "COMPLETED" } else { "CURRENT" };
        let body = serde_json::json!({
            "query": query,
            "variables": { "mediaId": anilist_id, "progress": progress, "status": status }
        });
        let resp = self
            .http
            .post(&self.anilist_url)
            .header("Content-Type", "application/json")
            .bearer_auth(token)
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let _ = check_status("AniList", resp).await?;
        Ok(())
    }

    // ---- MyAnimeList ----

    pub async fn mal_update_progress(
        &self,
        token: &str,
        mal_id: i64,
        episodes_watched: u32,
        completed: bool,
    ) -> Result<(), TrackingError> {
        let status = if completed { "completed" } else { "watching" };
        let form = format!("status={status}&num_watched_episodes={episodes_watched}");
        let resp = self
            .http
            .put(format!("{}/anime/{mal_id}/my_list_status", self.mal_base))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .bearer_auth(token)
            .body(form)
            .send()
            .await?;
        let _ = check_status("MAL", resp).await?;
        Ok(())
    }

    pub async fn fetch_anime_id_dataset(
        &self,
        url: &str,
    ) -> Result<Vec<anime_map::AnimeIdRecord>, TrackingError> {
        let resp = self.http.get(url).send().await?;
        let data = check_status("AnimeMap", resp).await?;
        Ok(anime_map::parse_anime_id_dataset(&data)?)
    }

    pub async fn trakt_pull_watchlist(
        &self,
        client_id: &str,
        token: &str,
    ) -> Result<Vec<TraktSyncEntry>, TrackingError> {
        self.trakt_get_sync_list(client_id, token, "/sync/watchlist", false)
            .await
    }

    pub async fn trakt_pull_ratings(
        &self,
        client_id: &str,
        token: &str,
    ) -> Result<Vec<TraktSyncEntry>, TrackingError> {
        self.trakt_get_sync_list(client_id, token, "/sync/ratings", true)
            .await
    }

    pub async fn trakt_pull_history(
        &self,
        client_id: &str,
        token: &str,
    ) -> Result<Vec<TraktSyncEntry>, TrackingError> {
        self.trakt_get_sync_list(client_id, token, "/sync/history?limit=100", false)
            .await
    }

    async fn trakt_get_sync_list(
        &self,
        client_id: &str,
        token: &str,
        path: &str,
        has_rating: bool,
    ) -> Result<Vec<TraktSyncEntry>, TrackingError> {
        let resp = self
            .http
            .get(format!("{}{path}", self.trakt_base))
            .header("trakt-api-version", "2")
            .header("trakt-api-key", client_id)
            .bearer_auth(token)
            .send()
            .await?;
        let data = check_status("Trakt", resp).await?;
        let raw: Vec<serde_json::Value> = serde_json::from_slice(&data)?;
        let mut out = Vec::new();
        for item in raw {
            let rating = if has_rating {
                item.get("rating").and_then(|v| v.as_u64()).map(|n| n as u8)
            } else {
                None
            };
            if let Some(m) = item.get("movie") {
                out.push(TraktSyncEntry {
                    media_type: "movie".into(),
                    title: m["title"].as_str().unwrap_or("Unknown").to_owned(),
                    year: m["year"].as_u64().map(|y| y as u32),
                    imdb_id: m["ids"]["imdb"].as_str().map(str::to_owned),
                    tmdb_id: m["ids"]["tmdb"].as_i64(),
                    tvdb_id: m["ids"]["tvdb"].as_i64(),
                    trakt_id: m["ids"]["trakt"].as_i64(),
                    season: None,
                    episode: None,
                    rating,
                });
            } else if let Some(s) = item.get("show") {
                let ep = item.get("episode");
                out.push(TraktSyncEntry {
                    media_type: "series".into(),
                    title: s["title"].as_str().unwrap_or("Unknown").to_owned(),
                    year: s["year"].as_u64().map(|y| y as u32),
                    imdb_id: s["ids"]["imdb"].as_str().map(str::to_owned),
                    tmdb_id: s["ids"]["tmdb"].as_i64(),
                    tvdb_id: s["ids"]["tvdb"].as_i64(),
                    trakt_id: s["ids"]["trakt"].as_i64(),
                    season: ep.and_then(|e| e["season"].as_u64()).map(|n| n as u32),
                    episode: ep.and_then(|e| e["number"].as_u64()).map(|n| n as u32),
                    rating,
                });
            }
        }
        Ok(out)
    }

    // ---- Letterboxd RSS ----

    pub async fn letterboxd_fetch_rss(
        &self,
        username: &str,
    ) -> Result<Vec<letterboxd::LetterboxdEntry>, TrackingError> {
        let trimmed = username.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Err(TrackingError::Invalid("empty Letterboxd username".into()));
        }
        let resp = self
            .http
            .get(format!("{}/{trimmed}/rss/", self.letterboxd_base))
            .send()
            .await?;
        let data = check_status("Letterboxd", resp).await?;
        Ok(letterboxd::parse_rss(&String::from_utf8_lossy(&data)))
    }
}

fn trakt_item_ids(payload: &ScrobblePayload) -> serde_json::Value {
    let mut ids = serde_json::Map::new();
    if let Some(imdb) = &payload.imdb_id {
        ids.insert("imdb".into(), serde_json::json!(imdb));
    }
    if let Some(tmdb) = payload.tmdb_id {
        ids.insert("tmdb".into(), serde_json::json!(tmdb));
    }
    if let Some(tvdb) = payload.tvdb_id {
        ids.insert("tvdb".into(), serde_json::json!(tvdb));
    }
    serde_json::Value::Object(ids)
}

fn trakt_payload_json(payload: &ScrobblePayload) -> serde_json::Value {
    let ids = trakt_item_ids(payload);
    if payload.media_type == "series" {
        serde_json::json!({
            "show": { "title": payload.title, "year": payload.year, "ids": ids },
            "episode": { "season": payload.season.unwrap_or(1), "number": payload.episode.unwrap_or(1) },
            "progress": payload.progress_pct.clamp(0.0, 100.0)
        })
    } else {
        serde_json::json!({
            "movie": { "title": payload.title, "year": payload.year, "ids": ids },
            "progress": payload.progress_pct.clamp(0.0, 100.0)
        })
    }
}

async fn check_status(
    provider: &'static str,
    resp: reqwest::Response,
) -> Result<Vec<u8>, TrackingError> {
    let status = resp.status();
    let bytes = resp.bytes().await?.to_vec();
    if !status.is_success() {
        return Err(TrackingError::HttpStatus {
            provider,
            status: status.as_u16(),
            detail: String::from_utf8_lossy(&bytes).chars().take(160).collect(),
        });
    }
    Ok(bytes)
}
