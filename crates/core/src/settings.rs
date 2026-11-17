//! Settings and runtime feature toggles (§5.5). A disabled feature is never
//! constructed: its accessor returns `CoreError::FeatureDisabled` before any
//! runtime, client or session is created.

use std::sync::Arc;

use crate::{Core, CoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Stremio addon protocol client (HTTP).
    Addons,
    /// Torrent streaming (librqbit session, DHT).
    Torrent,
    /// On-demand Chromecast & DLNA LAN casting (off by default).
    Casting,
    /// External tracking adapters (Trakt, Simkl, AniList, MAL, Letterboxd — off by default).
    Tracking,
    /// AniSkip & IntroDB skip-segment lookups (off by default).
    SkipSegments,
    /// Native Debrid cache check & stream unrestricting (off by default).
    Debrid,
    /// Live TV / IPTV playlists & XMLTV EPG (off by default).
    Iptv,
    /// In-process Discord Rich Presence IPC (off by default).
    Discord,
}

impl Feature {
    pub const ALL: [Feature; 8] = [
        Feature::Addons,
        Feature::Torrent,
        Feature::Casting,
        Feature::Tracking,
        Feature::SkipSegments,
        Feature::Debrid,
        Feature::Iptv,
        Feature::Discord,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Feature::Addons => "addons",
            Feature::Torrent => "torrent",
            Feature::Casting => "casting",
            Feature::Tracking => "tracking",
            Feature::SkipSegments => "skip_segments",
            Feature::Debrid => "debrid",
            Feature::Iptv => "iptv",
            Feature::Discord => "discord",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    fn key(self) -> String {
        format!("feature.{}", self.name())
    }

    fn default_enabled(self) -> bool {
        matches!(self, Feature::Addons | Feature::Torrent)
    }
}

impl Core {
    pub fn feature_enabled(&self, feature: Feature) -> Result<bool, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(match db.get_app_setting(&feature.key())?.as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => feature.default_enabled(),
        })
    }

    pub(crate) fn require(&self, feature: Feature) -> Result<(), CoreError> {
        if self.feature_enabled(feature)? {
            Ok(())
        } else {
            Err(CoreError::FeatureDisabled(feature.name()))
        }
    }

    /// Persists the toggle. Turning a feature off tears it down immediately:
    /// for `Torrent` that stops the session, so a torrent that is playing
    /// stops receiving data (its reads fail and mpv ends the file).
    pub async fn set_feature_enabled(
        self: &Arc<Self>,
        feature: Feature,
        enabled: bool,
    ) -> Result<(), CoreError> {
        {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.set_app_setting(&feature.key(), if enabled { "1" } else { "0" })?;
        }
        if enabled && feature == Feature::Tracking {
            self.kick_tracking_sync();
        }
        if !enabled {
            match feature {
                Feature::Torrent => {
                    let engine = self
                        .torrents
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                    if let Some(engine) = engine {
                        self.spawn(async move {
                            cinma_torrent::TorrentEngine::release_all(&*engine).await;
                            Ok(())
                        })
                        .await?;
                    }
                }
                Feature::Addons => {
                    self.http.lock().map_err(|_| CoreError::Poisoned)?.take();
                }
                Feature::Casting => {
                    let cast = self
                        .active_cast
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                    if let Some(session) = cast {
                        self.spawn(async move {
                            let _ = session.stop().await;
                            Ok(())
                        })
                        .await?;
                    }
                }
                Feature::Tracking => {
                    self.tracking_client
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                }
                Feature::SkipSegments => {
                    self.skip_client
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                }
                Feature::Debrid => {
                    self.debrid_client
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                }
                Feature::Iptv => {
                    self.iptv_client
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                }
                Feature::Discord => {
                    let client = self
                        .discord_client
                        .lock()
                        .map_err(|_| CoreError::Poisoned)?
                        .take();
                    if let Some(client) = client {
                        self.spawn(async move {
                            let mut g = client.lock().await;
                            let _ = g.clear().await;
                            Ok(())
                        })
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }
}
