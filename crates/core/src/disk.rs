//! Unified Disk Policy (§5.12, ADR 0013):
//! - Global & per-item "keep watched" rules (`keep_rule` table)
//! - LRU quota eviction across BlobStore and librqbit download folders
//! - Seeding policy & rate limit settings (`off` | `ratio` | `minutes`)

use std::collections::HashSet;
use std::path::PathBuf;

use cinma_blobstore::BlobId;
use cinma_db::{DiskEntryRow, DiskKind};
use cinma_torrent::{SeedingPolicy, TorrentPolicy};
use serde::{Deserialize, Serialize};

use crate::{Core, CoreError};

const DISK_SETTINGS_KEY: &str = "disk.policy";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DiskPolicySettings {
    /// Streaming & download cache quota in megabytes (`0` = disable automatic LRU trim).
    pub cache_quota_mb: u64,
    /// Global default for keeping watched items offline.
    pub keep_watched_global: bool,
    pub movie_folder: Option<String>,
    pub series_folder: Option<String>,
    pub ytdlp_folder: Option<String>,
    pub audio_folder: Option<String>,
    /// `"off"` | `"ratio"` | `"minutes"`
    pub seeding_mode: String,
    pub seeding_ratio: f64,
    pub seeding_minutes: u64,
    /// Upload cap in KB/s (`0` = unlimited when seeding is active).
    pub upload_limit_kbps: u32,
}

impl Default for DiskPolicySettings {
    fn default() -> Self {
        Self {
            cache_quota_mb: 10_240,
            keep_watched_global: false,
            movie_folder: None,
            series_folder: None,
            ytdlp_folder: None,
            audio_folder: None,
            seeding_mode: "off".to_owned(),
            seeding_ratio: 1.0,
            seeding_minutes: 60,
            upload_limit_kbps: 0,
        }
    }
}

impl DiskPolicySettings {
    pub fn torrent_policy(&self, delete_on_release: bool) -> TorrentPolicy {
        let seeding = match self.seeding_mode.trim().to_ascii_lowercase().as_str() {
            "ratio" => SeedingPolicy::Ratio {
                ratio: self.seeding_ratio.max(0.1),
            },
            "minutes" => SeedingPolicy::Minutes {
                minutes: self.seeding_minutes.max(1),
            },
            _ => SeedingPolicy::Off,
        };
        let upload_bps = (self.upload_limit_kbps > 0).then(|| self.upload_limit_kbps * 1024);
        TorrentPolicy {
            fastresume: true,
            upload_bps,
            download_bps: None,
            seeding,
            delete_on_release,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskUsageSummary {
    pub total_bytes: u64,
    pub kept_bytes: u64,
    pub evictable_bytes: u64,
    pub entry_count: u32,
}

impl Core {
    pub fn disk_policy_settings(&self) -> Result<DiskPolicySettings, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        match db.get_app_setting(DISK_SETTINGS_KEY)? {
            Some(raw) => serde_json::from_str(&raw).map_err(|e| CoreError::Invalid(e.to_string())),
            None => Ok(DiskPolicySettings::default()),
        }
    }

    pub fn set_disk_policy_settings(&self, settings: &DiskPolicySettings) -> Result<(), CoreError> {
        let json =
            serde_json::to_string(settings).map_err(|e| CoreError::Invalid(e.to_string()))?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_app_setting(DISK_SETTINGS_KEY, &json)?;
        if let Some(dl_engine) = self
            .torrent_download_engine
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
        {
            dl_engine.set_seeding(settings.torrent_policy(false).seeding);
        }
        Ok(())
    }

    pub fn set_item_keep_watched(
        &self,
        meta_id: &str,
        media_type: &str,
        keep: bool,
    ) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_keep_rule(profile, meta_id, media_type, keep)?;
        Ok(())
    }

    pub fn item_keep_watched(&self, meta_id: &str, media_type: &str) -> Result<bool, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(explicit) = db.is_item_kept(profile, meta_id, media_type)? {
            return Ok(explicit);
        }
        let global = match db.get_app_setting(DISK_SETTINGS_KEY)? {
            Some(raw) => serde_json::from_str::<DiskPolicySettings>(&raw)
                .map(|s| s.keep_watched_global)
                .unwrap_or(false),
            None => false,
        };
        Ok(global)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_disk_entry(
        &self,
        entry_key: &str,
        kind: DiskKind,
        meta_id: Option<&str>,
        media_type: Option<&str>,
        path: &str,
        bytes: u64,
        pinned: bool,
    ) -> Result<(), CoreError> {
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .upsert_disk_entry(&DiskEntryRow {
                entry_key: entry_key.to_owned(),
                kind,
                meta_id: meta_id.map(str::to_owned),
                media_type: media_type.map(str::to_owned),
                path: path.to_owned(),
                bytes,
                last_accessed_at: 0,
                pinned,
            })?;
        Ok(())
    }

    pub fn disk_usage_summary(&self) -> Result<DiskUsageSummary, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        let global_keep = match db.get_app_setting(DISK_SETTINGS_KEY)? {
            Some(raw) => serde_json::from_str::<DiskPolicySettings>(&raw)
                .map(|s| s.keep_watched_global)
                .unwrap_or(false),
            None => false,
        };
        let entries = db.disk_entries_lru()?;
        let mut total = 0u64;
        let mut kept = 0u64;
        for e in &entries {
            total += e.bytes;
            let rule_kept = match (&e.meta_id, &e.media_type) {
                (Some(mid), Some(mt)) => db.any_profile_keeps_item(mid, mt)? || global_keep,
                _ => false,
            };
            if e.pinned || rule_kept {
                kept += e.bytes;
            }
        }
        Ok(DiskUsageSummary {
            total_bytes: total,
            kept_bytes: kept,
            evictable_bytes: total.saturating_sub(kept),
            entry_count: entries.len() as u32,
        })
    }

    /// Evicts oldest non-kept, non-queued items (`Blob`s and `librqbit` folders)
    /// until total tracked cache is at or below `cache_quota_mb`. Returns bytes freed.
    pub fn enforce_disk_quota(&self) -> Result<u64, CoreError> {
        let settings = self.disk_policy_settings()?;
        if settings.cache_quota_mb == 0 {
            return Ok(0);
        }
        let quota_bytes = settings.cache_quota_mb.saturating_mul(1024 * 1024);

        let (entries, protected_blobs, kept_flags) = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            let protected: HashSet<String> =
                db.all_active_or_queued_blob_ids()?.into_iter().collect();
            let entries = db.disk_entries_lru()?;
            let mut flags = Vec::with_capacity(entries.len());
            for e in &entries {
                let rule_kept = match (&e.meta_id, &e.media_type) {
                    (Some(mid), Some(mt)) => {
                        db.any_profile_keeps_item(mid, mt)? || settings.keep_watched_global
                    }
                    _ => false,
                };
                flags.push(e.pinned || rule_kept);
            }
            (entries, protected, flags)
        };

        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        if total <= quota_bytes {
            return Ok(0);
        }

        let mut freed = 0u64;
        let store = self.blob_store()?;
        for (entry, is_kept) in entries.into_iter().zip(kept_flags) {
            if total <= quota_bytes {
                break;
            }
            if is_kept {
                continue;
            }
            if let Some(blob_hex) = entry.entry_key.strip_prefix("blob:")
                && protected_blobs.contains(blob_hex)
            {
                continue;
            }

            match entry.kind {
                DiskKind::Blob => {
                    if let Some(blob_hex) = entry.entry_key.strip_prefix("blob:")
                        && let Ok(id) = BlobId::parse(blob_hex)
                    {
                        let _ = store.remove(&id);
                    }
                }
                DiskKind::Torrent => {
                    let p = PathBuf::from(&entry.path);
                    if p.is_dir() {
                        let _ = std::fs::remove_dir_all(&p);
                    } else if p.is_file() {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
            let _ = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .delete_disk_entry(&entry.entry_key);
            total = total.saturating_sub(entry.bytes);
            freed = freed.saturating_add(entry.bytes);
        }
        Ok(freed)
    }
}
