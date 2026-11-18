//! Unified Download Policy (§5.12):
//! - Download folders, seeding policy & live rate limit settings (`off` | `ratio` | `minutes`)

use cinma_torrent::{SeedingPolicy, TorrentPolicy};
use serde::{Deserialize, Serialize};

use crate::{Core, CoreError};

const DISK_SETTINGS_KEY: &str = "disk.policy";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DiskPolicySettings {
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
    /// Download speed cap in KB/s (`0` = unlimited).
    pub download_limit_kbps: u32,
    /// Optional BSD interface name (e.g. `utun3`) for torrent socket binding.
    pub bind_device_name: Option<String>,
    /// Refuse torrent traffic if `bind_device_name` is missing or unavailable.
    pub vpn_kill_switch: bool,
}

impl Default for DiskPolicySettings {
    fn default() -> Self {
        Self {
            movie_folder: None,
            series_folder: None,
            ytdlp_folder: None,
            audio_folder: None,
            seeding_mode: "off".to_owned(),
            seeding_ratio: 1.0,
            seeding_minutes: 60,
            upload_limit_kbps: 0,
            download_limit_kbps: 0,
            bind_device_name: None,
            vpn_kill_switch: false,
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
        let download_bps =
            (self.download_limit_kbps > 0).then(|| self.download_limit_kbps.saturating_mul(1024));
        TorrentPolicy {
            fastresume: true,
            upload_bps,
            download_bps,
            seeding,
            delete_on_release,
            bind_device_name: self.bind_device_name.clone(),
            vpn_kill_switch: self.vpn_kill_switch,
        }
    }
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
        let tp = settings.torrent_policy(false);
        if let Some(dl_engine) = self
            .torrent_download_engine
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
        {
            dl_engine.set_seeding(tp.seeding);
        }
        if let Some(engine) = self
            .torrents
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .clone()
            && let Ok(rt) = self.runtime()
        {
            rt.spawn(async move {
                engine.update_policy(tp).await;
            });
        }
        Ok(())
    }
}
