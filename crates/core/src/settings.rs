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
}

impl Feature {
    pub const ALL: [Feature; 2] = [Feature::Addons, Feature::Torrent];

    pub fn name(self) -> &'static str {
        match self {
            Feature::Addons => "addons",
            Feature::Torrent => "torrent",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    fn key(self) -> String {
        format!("feature.{}", self.name())
    }

    fn default_enabled(self) -> bool {
        true
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
    pub async fn set_feature_enabled(self: &Arc<Self>, feature: Feature, enabled: bool) -> Result<(), CoreError> {
        {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.set_app_setting(&feature.key(), if enabled { "1" } else { "0" })?;
        }
        if !enabled {
            match feature {
                Feature::Torrent => {
                    let engine = self.torrents.lock().map_err(|_| CoreError::Poisoned)?.take();
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
            }
        }
        Ok(())
    }
}
