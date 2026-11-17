//! Phase 6 Integrations (`crates/core/src/integrations.rs`, ADR 0014):
//! - `Feature::Casting` (Chromecast + DLNA, zero idle sockets; off by default)
//! - `Feature::Tracking` (Trakt, Simkl, AniList, MAL, Letterboxd RSS/CSV,
//!   Anime ID Map, one-time Stremio import; off by default)

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cinma_casting::{ActiveCastSession, CastDevice, discover_lan_devices, server::MediaPayload};
use cinma_db::{IdMapEntry, MediaKind};
use cinma_stremio::Stream;
use cinma_torrent::{TorrentEngine, TorrentFileRequest};
use cinma_tracking::{
    ScrobblePayload, TrackingClient, TraktDeviceCode, TraktRecommendation,
    anime_map::{BUNDLED_STARTER_JSON, parse_anime_id_dataset},
    letterboxd::{LetterboxdCsvRow, LetterboxdEntry, export_csv},
    stremio_import::fetch_stremio_account_snapshot,
};

use crate::{Core, CoreError, CoreEvent, Feature, keychain};

const KNOWN_ADAPTERS: &[&str] = &["trakt", "simkl", "anilist", "mal"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterStatus {
    pub adapter: String,
    pub enabled: bool,
    pub connected: bool,
    pub last_sync_at: Option<i64>,
    pub outbox_pending: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StremioImportResult {
    pub addons_imported: u32,
    pub library_imported: u32,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mime_for_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("m4a") => "audio/mp4",
        _ => "video/mp4",
    }
    .to_owned()
}

/// After this many failed deliveries a row is dropped (about a day at the 1 h cap).
const MAX_OUTBOX_ATTEMPTS: i64 = 30;

/// Single-flight state of the outbox worker.
#[derive(Default)]
pub(crate) struct TrackingSyncState {
    running: bool,
    /// New rows arrived while the worker was running: drain once more.
    dirty: bool,
}

fn tracking_client_id_key(adapter: &str) -> String {
    format!("tracking.{adapter}.client_id")
}

fn tracking_client_secret_key(adapter: &str) -> String {
    format!("tracking.{adapter}.client_secret")
}

impl Core {
    // =========================================================================
    // Casting (Chromecast + DLNA, zero idle state)
    // =========================================================================

    pub async fn discover_cast_devices(
        self: &Arc<Self>,
        timeout_ms: u64,
    ) -> Result<Vec<CastDevice>, CoreError> {
        self.require(Feature::Casting)?;
        let timeout = Duration::from_millis(timeout_ms.clamp(250, 8000));
        self.spawn(async move {
            discover_lan_devices(timeout)
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn cast_download(
        self: &Arc<Self>,
        device: CastDevice,
        download_id: i64,
        title: String,
    ) -> Result<(), CoreError> {
        self.require(Feature::Casting)?;
        let source = self.download_byte_source(download_id)?;
        let payload = MediaPayload::Source {
            source,
            mime: "video/mp4".to_owned(),
        };
        self.start_cast_with_payload(device, payload, title).await
    }

    pub async fn cast_local_file(
        self: &Arc<Self>,
        device: CastDevice,
        path: String,
        title: String,
    ) -> Result<(), CoreError> {
        self.require(Feature::Casting)?;
        let pb = Path::new(&path).to_path_buf();
        let meta = std::fs::metadata(&pb)?;
        let payload = MediaPayload::LocalFile {
            mime: mime_for_path(&pb),
            path: pb,
            size: meta.len(),
        };
        self.start_cast_with_payload(device, payload, title).await
    }

    pub async fn cast_stream(
        self: &Arc<Self>,
        device: CastDevice,
        stream: Stream,
        title: String,
    ) -> Result<(), CoreError> {
        self.require(Feature::Casting)?;
        if let Some(info_hash) = stream.info_hash.clone() {
            let engine = self.torrents()?;
            let req = TorrentFileRequest {
                info_hash,
                file_idx: stream.file_idx.map(|i| i as usize),
                trackers: stream.trackers(),
            };
            let source = self
                .spawn(async move { Ok(engine.open_file(req).await?) })
                .await?;
            let mime = source
                .filename()
                .map(|f| mime_for_path(Path::new(f)))
                .unwrap_or_else(|| "video/mp4".to_owned());
            self.start_cast_with_payload(device, MediaPayload::Source { source, mime }, title)
                .await
        } else if let Some(url) = stream.url.as_deref() {
            let local = url.strip_prefix("file://").unwrap_or(url);
            if Path::new(local).is_file() {
                self.cast_local_file(device, local.to_owned(), title).await
            } else {
                Err(CoreError::Invalid(
                    "HTTP remote streams should be downloaded first or cast from Downloads".into(),
                ))
            }
        } else {
            Err(CoreError::Invalid(
                "stream has neither infoHash nor local file".into(),
            ))
        }
    }

    async fn start_cast_with_payload(
        self: &Arc<Self>,
        device: CastDevice,
        payload: MediaPayload,
        title: String,
    ) -> Result<(), CoreError> {
        self.cast_stop().await?;
        let session = self
            .spawn(async move {
                ActiveCastSession::start(device, payload, &title)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        *self.active_cast.lock().map_err(|_| CoreError::Poisoned)? = Some(Arc::new(session));
        self.emit(CoreEvent::CastChanged);
        Ok(())
    }

    pub async fn cast_play(self: &Arc<Self>) -> Result<(), CoreError> {
        let session = self
            .active_cast
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .clone()
            .ok_or_else(|| CoreError::Invalid("no active cast session".into()))?;
        self.spawn(async move {
            session
                .play()
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn cast_pause(self: &Arc<Self>) -> Result<(), CoreError> {
        let session = self
            .active_cast
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .clone()
            .ok_or_else(|| CoreError::Invalid("no active cast session".into()))?;
        self.spawn(async move {
            session
                .pause()
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn cast_seek(self: &Arc<Self>, seconds: f64) -> Result<(), CoreError> {
        let session = self
            .active_cast
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .clone()
            .ok_or_else(|| CoreError::Invalid("no active cast session".into()))?;
        self.spawn(async move {
            session
                .seek(seconds)
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn cast_stop(self: &Arc<Self>) -> Result<(), CoreError> {
        let existing = self
            .active_cast
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .take();
        if let Some(session) = existing {
            self.spawn(async move {
                let _ = session.stop().await;
                Ok(())
            })
            .await?;
            self.emit(CoreEvent::CastChanged);
        }
        Ok(())
    }

    pub fn active_cast_device(&self) -> Option<CastDevice> {
        self.active_cast
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.device.clone()))
    }

    // =========================================================================
    // Tracking (Trakt, Simkl, AniList, MAL, Letterboxd, Anime ID Map, Stremio)
    // =========================================================================

    pub fn tracking_client(&self) -> Result<TrackingClient, CoreError> {
        self.require(Feature::Tracking)?;
        let mut slot = self
            .tracking_client
            .lock()
            .map_err(|_| CoreError::Poisoned)?;
        if let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
        let client = TrackingClient::new().map_err(|e| CoreError::Invalid(e.to_string()))?;
        *slot = Some(client.clone());
        Ok(client)
    }

    pub fn set_tracking_client_override(&self, client: TrackingClient) -> Result<(), CoreError> {
        *self
            .tracking_client
            .lock()
            .map_err(|_| CoreError::Poisoned)? = Some(client);
        Ok(())
    }

    pub fn tracking_adapters(&self) -> Result<Vec<AdapterStatus>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let states = db.adapter_states(profile)?;
        let outbox_pending = db.outbox_count(profile)?;
        Ok(KNOWN_ADAPTERS
            .iter()
            .map(|&name| {
                let st = states.iter().find(|s| s.adapter == name);
                let connected = keychain::get_tracking_token(profile, name)
                    .ok()
                    .flatten()
                    .is_some();
                AdapterStatus {
                    adapter: name.to_owned(),
                    enabled: st.is_some_and(|s| s.enabled),
                    connected,
                    last_sync_at: st.and_then(|s| s.last_sync_at),
                    outbox_pending,
                }
            })
            .collect())
    }

    pub fn set_tracking_adapter_enabled(
        &self,
        adapter: &str,
        enabled: bool,
    ) -> Result<(), CoreError> {
        if !KNOWN_ADAPTERS.contains(&adapter) {
            return Err(CoreError::Invalid(format!("unknown adapter `{adapter}`")));
        }
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_adapter_enabled(profile, adapter, enabled)?;
        self.emit(CoreEvent::TrackingChanged);
        Ok(())
    }

    pub fn set_tracking_token(&self, adapter: &str, token: Option<&str>) -> Result<(), CoreError> {
        if !KNOWN_ADAPTERS.contains(&adapter) {
            return Err(CoreError::Invalid(format!("unknown adapter `{adapter}`")));
        }
        let profile = self.active_profile()?;
        keychain::set_tracking_token(profile, adapter, token)?;
        if token.is_some() {
            self.db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .set_adapter_enabled(profile, adapter, true)?;
        }
        self.emit(CoreEvent::TrackingChanged);
        if token.is_some() {
            self.kick_tracking_sync();
        }
        Ok(())
    }

    pub fn set_tracking_app_credentials(
        &self,
        adapter: &str,
        client_id: &str,
        client_secret: Option<&str>,
    ) -> Result<(), CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        db.set_app_setting(&tracking_client_id_key(adapter), client_id.trim())?;
        // Secrets live in the Keychain, never in SQLite (and so never in backups).
        // Older builds stored it as a setting: drop that row.
        db.delete_app_setting(&tracking_client_secret_key(adapter))?;
        if let Some(secret) = client_secret {
            keychain::set_tracking_client_secret(adapter, Some(secret))?;
        }
        Ok(())
    }

    pub async fn trakt_start_device_auth(
        self: &Arc<Self>,
        client_id: String,
    ) -> Result<TraktDeviceCode, CoreError> {
        let client = self.tracking_client()?;
        self.set_tracking_app_credentials("trakt", &client_id, None)?;
        self.spawn(async move {
            client
                .trakt_device_code(&client_id)
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn trakt_poll_device_auth(
        self: &Arc<Self>,
        client_id: String,
        client_secret: String,
        device_code: String,
    ) -> Result<bool, CoreError> {
        let client = self.tracking_client()?;
        self.set_tracking_app_credentials("trakt", &client_id, Some(&client_secret))?;
        let token = self
            .spawn(async move {
                client
                    .trakt_poll_token(&client_id, &client_secret, &device_code)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        if let Some(tok) = token {
            self.set_tracking_token("trakt", Some(&tok))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_watch_sync(
        &self,
        profile: i64,
        meta_id: &str,
        media_type: &str,
        title: &str,
        season: Option<u32>,
        episode: Option<u32>,
        progress_pct: f64,
    ) -> Result<u32, CoreError> {
        if !self.feature_enabled(Feature::Tracking)? {
            return Ok(0);
        }
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let states = db.adapter_states(profile)?;
        let enabled_adapters: Vec<&str> = KNOWN_ADAPTERS
            .iter()
            .copied()
            .filter(|a| states.iter().any(|s| s.adapter == *a && s.enabled))
            .collect();
        if enabled_adapters.is_empty() {
            return Ok(0);
        }
        let imdb_id = meta_id.starts_with("tt").then(|| meta_id.to_owned());
        let kind = if media_type == "series" {
            MediaKind::Series
        } else {
            MediaKind::Movie
        };
        let mapped = imdb_id
            .as_deref()
            .and_then(|id| db.find_id_map_by_imdb(kind, id).ok().flatten());
        let payload = ScrobblePayload {
            media_type: media_type.to_owned(),
            title: title.to_owned(),
            year: None,
            imdb_id,
            tmdb_id: mapped.as_ref().and_then(|m| m.tmdb_id),
            tvdb_id: mapped.as_ref().and_then(|m| m.tvdb_id),
            mal_id: mapped.as_ref().and_then(|m| m.mal_id),
            anilist_id: mapped.as_ref().and_then(|m| m.anilist_id),
            season,
            episode,
            progress_pct,
        };
        let json =
            serde_json::to_string(&payload).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let mut count = 0u32;
        for adapter in enabled_adapters {
            db.enqueue_outbox(profile, adapter, "sync_history", &json)?;
            count += 1;
        }
        drop(db);
        if count > 0 {
            self.emit(CoreEvent::TrackingChanged);
            self.kick_tracking_sync();
        }
        Ok(count)
    }

    /// Starts the outbox worker if tracking is on and rows are waiting.
    /// Event-driven: it drains, then sleeps only until the next row is due,
    /// and exits as soon as the outbox is empty. Nothing runs when idle.
    pub fn kick_tracking_sync(&self) {
        if !self.feature_enabled(Feature::Tracking).unwrap_or(false) {
            return;
        }
        let pending = self
            .db
            .lock()
            .ok()
            .and_then(|db| db.next_outbox_due().ok().flatten());
        if pending.is_none() {
            return;
        }
        {
            let Ok(mut state) = self.tracking_sync.lock() else {
                return;
            };
            if state.running {
                state.dirty = true;
                return;
            }
            state.running = true;
            state.dirty = false;
        }
        let Some(core) = self.me.upgrade() else {
            return;
        };
        let Ok(rt) = self.runtime() else {
            if let Ok(mut s) = self.tracking_sync.lock() {
                s.running = false;
            }
            return;
        };
        rt.spawn(async move {
            core.tracking_worker().await;
            let again = core
                .tracking_sync
                .lock()
                .map(|mut s| {
                    s.running = false;
                    std::mem::take(&mut s.dirty)
                })
                .unwrap_or(false);
            if again {
                core.kick_tracking_sync();
            }
        });
    }

    async fn tracking_worker(self: &Arc<Self>) {
        loop {
            if !self.feature_enabled(Feature::Tracking).unwrap_or(false) {
                return;
            }
            if let Ok(mut s) = self.tracking_sync.lock() {
                s.dirty = false;
            }
            // After a failed or empty pass, wait at least a minute: a row that
            // stays due (e.g. Keychain read error) must not cause a busy loop.
            let min_wait = match self.drain_tracking_outbox().await {
                Ok(delivered) if delivered > 0 => 0,
                Ok(_) => 60,
                Err(e) => {
                    eprintln!("[cinma] tracking sync: {e}");
                    60
                }
            };
            let next = self
                .db
                .lock()
                .ok()
                .and_then(|db| db.next_outbox_due().ok().flatten());
            let Some(due) = next else {
                return;
            };
            let wait = (due - unix_now()).clamp(min_wait, 3600) as u64;
            if wait > 0 {
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        }
    }

    pub async fn drain_tracking_outbox(self: &Arc<Self>) -> Result<u32, CoreError> {
        let client = self.tracking_client()?;
        let (rows, trakt_cid, simkl_cid) = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let rows = db.due_outbox_rows(unix_now(), 25)?;
            let trakt_cid = db
                .get_app_setting(&tracking_client_id_key("trakt"))?
                .unwrap_or_default();
            let simkl_cid = db
                .get_app_setting(&tracking_client_id_key("simkl"))?
                .unwrap_or_default();
            (rows, trakt_cid, simkl_cid)
        };
        let mut delivered = 0u32;
        for row in rows {
            let Ok(payload) = serde_json::from_str::<ScrobblePayload>(&row.payload_json) else {
                self.db
                    .lock()
                    .map_err(|_| CoreError::Poisoned)?
                    .delete_outbox_row(row.id)?;
                continue;
            };
            let Some(token) = keychain::get_tracking_token(row.profile_id, &row.adapter)? else {
                let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
                if row.attempts + 1 >= MAX_OUTBOX_ATTEMPTS {
                    db.delete_outbox_row(row.id)?;
                    continue;
                }
                let delay = backoff_seconds(row.attempts + 1);
                db.schedule_outbox_retry(
                    row.id,
                    row.attempts + 1,
                    unix_now() + delay,
                    "missing auth token",
                )?;
                continue;
            };
            let c = client.clone();
            let adapter = row.adapter.clone();
            let tc = trakt_cid.clone();
            let sc = simkl_cid.clone();
            let res = self
                .spawn(async move {
                    match adapter.as_str() {
                        "trakt" => c.trakt_sync_history(&tc, &token, &payload).await,
                        "simkl" => c.simkl_sync_history(&sc, &token, &payload).await,
                        "anilist" => match payload.anilist_id {
                            Some(aid) => {
                                let ep = payload.episode.unwrap_or(1);
                                c.anilist_update_progress(
                                    &token,
                                    aid,
                                    ep,
                                    payload.progress_pct >= 85.0,
                                )
                                .await
                            }
                            None => Err(cinma_tracking::TrackingError::Invalid(
                                "missing anilist_id".into(),
                            )),
                        },
                        "mal" => match payload.mal_id {
                            Some(mid) => {
                                let ep = payload.episode.unwrap_or(1);
                                c.mal_update_progress(&token, mid, ep, payload.progress_pct >= 85.0)
                                    .await
                            }
                            None => Err(cinma_tracking::TrackingError::Invalid(
                                "missing mal_id".into(),
                            )),
                        },
                        _ => Ok(()),
                    }
                    .map_err(|e| CoreError::Invalid(e.to_string()))
                })
                .await;

            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            match res {
                Ok(()) => {
                    db.delete_outbox_row(row.id)?;
                    db.update_adapter_sync(row.profile_id, &row.adapter, None)?;
                    delivered += 1;
                }
                Err(e) => {
                    let attempts = row.attempts + 1;
                    let permanent = e.to_string().contains("missing anilist_id")
                        || e.to_string().contains("missing mal_id");
                    if permanent || attempts >= MAX_OUTBOX_ATTEMPTS {
                        eprintln!(
                            "[cinma] tracking: dropping {} update after {attempts} attempts: {e}",
                            row.adapter
                        );
                        db.delete_outbox_row(row.id)?;
                    } else {
                        let delay = backoff_seconds(attempts);
                        db.schedule_outbox_retry(
                            row.id,
                            attempts,
                            unix_now() + delay,
                            &e.to_string(),
                        )?;
                    }
                }
            }
        }
        self.emit(CoreEvent::TrackingChanged);
        Ok(delivered)
    }

    pub fn backfill_tracking(&self) -> Result<u32, CoreError> {
        self.require(Feature::Tracking)?;
        let profile = self.active_profile()?;
        let watched = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.watched_items(profile, 500).unwrap_or_default()
        };
        let mut total = 0u32;
        for row in watched {
            total += self.enqueue_watch_sync(
                profile,
                &row.meta_id,
                &row.media_type,
                &row.title,
                row.season.and_then(|s| u32::try_from(s).ok()),
                row.episode.and_then(|e| u32::try_from(e).ok()),
                100.0,
            )?;
        }
        Ok(total)
    }

    pub async fn trakt_recommendations(
        self: &Arc<Self>,
    ) -> Result<Vec<TraktRecommendation>, CoreError> {
        let client = self.tracking_client()?;
        let profile = self.active_profile()?;
        let client_id = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_app_setting(&tracking_client_id_key("trakt"))?
            .unwrap_or_default();
        let token = keychain::get_tracking_token(profile, "trakt")?
            .ok_or_else(|| CoreError::Invalid("Trakt account is not connected".into()))?;
        self.spawn(async move {
            client
                .trakt_recommendations(&client_id, &token)
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub async fn letterboxd_rss(
        self: &Arc<Self>,
        username: String,
    ) -> Result<Vec<LetterboxdEntry>, CoreError> {
        let client = self.tracking_client()?;
        self.spawn(async move {
            client
                .letterboxd_fetch_rss(&username)
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))
        })
        .await
    }

    pub fn export_letterboxd_csv(&self, dest: &Path) -> Result<usize, CoreError> {
        let profile = self.active_profile()?;
        let rows: Vec<LetterboxdCsvRow> = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.watched_items(profile, 500)?
                .into_iter()
                .filter(|w| w.media_type == "movie")
                .map(|w| LetterboxdCsvRow {
                    title: w.title,
                    year: None,
                    watched_date: None,
                    rating10: None,
                    review: None,
                })
                .collect()
        };
        let count = rows.len();
        let csv = export_csv(&rows);
        std::fs::write(dest, csv)?;
        Ok(count)
    }

    pub fn seed_anime_id_map(&self) -> Result<usize, CoreError> {
        let records = parse_anime_id_dataset(BUNDLED_STARTER_JSON.as_bytes())
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let count = records.len();
        for rec in records {
            db.upsert_id_map(&IdMapEntry {
                media_kind: MediaKind::Series,
                imdb_id: rec.imdb_id,
                tmdb_id: rec.tmdb_id,
                tvdb_id: rec.tvdb_id,
                trakt_id: None,
                simkl_id: None,
                mal_id: rec.mal_id,
                anilist_id: rec.anilist_id,
                kitsu_id: rec.kitsu_id,
                season: None,
                episode: None,
            })?;
        }
        Ok(count)
    }

    pub async fn refresh_anime_id_map(self: &Arc<Self>, url: String) -> Result<usize, CoreError> {
        self.require(Feature::Tracking)?;
        let bytes = self
            .spawn(async move {
                let resp = reqwest::get(&url)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))?;
                if !resp.status().is_success() {
                    return Err(CoreError::Invalid(format!(
                        "HTTP {} fetching anime ID dataset",
                        resp.status().as_u16()
                    )));
                }
                resp.bytes()
                    .await
                    .map(|b| b.to_vec())
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let records =
            parse_anime_id_dataset(&bytes).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let count = records.len();
        for rec in records {
            db.upsert_id_map(&IdMapEntry {
                media_kind: MediaKind::Series,
                imdb_id: rec.imdb_id,
                tmdb_id: rec.tmdb_id,
                tvdb_id: rec.tvdb_id,
                trakt_id: None,
                simkl_id: None,
                mal_id: rec.mal_id,
                anilist_id: rec.anilist_id,
                kitsu_id: rec.kitsu_id,
                season: None,
                episode: None,
            })?;
        }
        Ok(count)
    }

    /// One-time Stremio import (addons + library items); credentials are never stored.
    pub async fn import_stremio_account(
        self: &Arc<Self>,
        email: String,
        password: String,
    ) -> Result<StremioImportResult, CoreError> {
        self.require(Feature::Addons)?;
        let snapshot = self
            .spawn(async move {
                fetch_stremio_account_snapshot(None, &email, &password)
                    .await
                    .map_err(|e| CoreError::Invalid(e.to_string()))
            })
            .await?;
        let mut addons_imported = 0u32;
        for url in snapshot.addon_urls {
            if self.install_addon(url).await.is_ok() {
                addons_imported += 1;
            }
        }
        let mut library_imported = 0u32;
        for item in snapshot.library {
            if self
                .add_to_library(item.id, item.media_type, item.name, item.poster)
                .is_ok()
            {
                library_imported += 1;
            }
        }
        Ok(StremioImportResult {
            addons_imported,
            library_imported,
        })
    }
}

fn backoff_seconds(attempts: i64) -> i64 {
    let shift = u32::try_from(attempts.clamp(0, 8)).unwrap_or(8);
    (30_i64.saturating_mul(1_i64 << shift)).min(3600)
}
