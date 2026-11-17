//! Installed addons and stream lookup.
//!
//! Phase 1 stores the addon list as JSON in `app_setting` (key `addons.v1`);
//! proper tables with per-profile enablement come with the Phase 2 schema.

use std::sync::Arc;

use cinma_db::Database;
use cinma_stremio::{Manifest, Stream};
use settings::{get_setting, set_setting};
use serde::{Deserialize, Serialize};

use crate::{Core, CoreError};

const ADDONS_KEY: &str = "addons.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledAddon {
    pub transport_url: String,
    pub manifest: Manifest,
}

#[derive(Debug, Clone)]
pub struct StreamCandidate {
    pub addon_name: String,
    pub stream: Stream,
}

pub(crate) fn load(db: &Database) -> Result<Vec<InstalledAddon>, CoreError> {
    match get_setting(db, ADDONS_KEY)? {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| CoreError::Invalid(format!("stored addon list: {e}"))),
        None => Ok(Vec::new()),
    }
}

impl Core {
    pub fn addons(&self) -> Result<Vec<InstalledAddon>, CoreError> {
        Ok(self.addons.lock().map_err(|_| CoreError::Poisoned)?.clone())
    }

    /// Fetches the manifest and installs (or refreshes) the addon.
    pub async fn install_addon(self: &Arc<Self>, url: String) -> Result<InstalledAddon, CoreError> {
        let http = self.http()?;
        let (transport_url, manifest) = self
            .spawn(async move { Ok(http.fetch_manifest(&url).await?) })
            .await?;
        let addon = InstalledAddon {
            transport_url: transport_url.to_string(),
            manifest,
        };
        let json = {
            let mut addons = self.addons.lock().map_err(|_| CoreError::Poisoned)?;
            match addons.iter_mut().find(|a| a.manifest.id == addon.manifest.id) {
                Some(existing) => *existing = addon.clone(),
                None => addons.push(addon.clone()),
            }
            serde_json::to_string(&*addons).map_err(|e| CoreError::Invalid(e.to_string()))?
        };
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        set_setting(&db, ADDONS_KEY, &json)?;
        Ok(addon)
    }

    pub fn remove_addon(&self, id: &str) -> Result<(), CoreError> {
        let json = {
            let mut addons = self.addons.lock().map_err(|_| CoreError::Poisoned)?;
            addons.retain(|a| a.manifest.id != id);
            serde_json::to_string(&*addons).map_err(|e| CoreError::Invalid(e.to_string()))?
        };
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        set_setting(&db, ADDONS_KEY, &json)?;
        Ok(())
    }

    /// Queries every installed addon that serves streams for `media_type`/`id`
    /// in parallel; results keep addon order. Failing addons are skipped.
    pub async fn streams(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<Vec<StreamCandidate>, CoreError> {
        let addons: Vec<InstalledAddon> = self
            .addons()?
            .into_iter()
            .filter(|a| a.manifest.supports("stream", &media_type, &id))
            .collect();
        if addons.is_empty() {
            return Err(CoreError::Invalid(format!(
                "no installed addon serves streams for {media_type} {id}"
            )));
        }
        let http = self.http()?;
        self.spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for (index, addon) in addons.into_iter().enumerate() {
                let (http, media_type, id) = (http.clone(), media_type.clone(), id.clone());
                tasks.spawn(async move {
                    let result = http.fetch_streams(&addon.transport_url, &media_type, &id).await;
                    (index, addon.manifest.name, result)
                });
            }
            let mut results = Vec::new();
            let mut errors = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                let Ok((index, addon_name, result)) = joined else { continue };
                match result {
                    Ok(streams) => results.push((index, addon_name, streams)),
                    Err(e) => errors.push(format!("{addon_name}: {e}")),
                }
            }
            if results.is_empty() && !errors.is_empty() {
                return Err(CoreError::Invalid(errors.join("; ")));
            }
            results.sort_by_key(|(index, _, _)| *index);
            Ok(results
                .into_iter()
                .flat_map(|(_, addon_name, streams)| {
                    streams.into_iter().map(move |stream| StreamCandidate {
                        addon_name: addon_name.clone(),
                        stream,
                    })
                })
                .collect())
        })
        .await
    }
}

/// Tiny `app_setting` accessors (kept here until the Phase 2 settings module).
mod settings {
    use cinma_db::{Database, DbError};

    pub fn get_setting(db: &Database, key: &str) -> Result<Option<String>, DbError> {
        db.get_app_setting(key)
    }

    pub fn set_setting(db: &Database, key: &str, value: &str) -> Result<(), DbError> {
        db.set_app_setting(key, value)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn addon_list_round_trips_through_settings() {
        let db = Database::open_in_memory().unwrap();
        assert!(load(&db).unwrap().is_empty());
        let addon = InstalledAddon {
            transport_url: "https://a.example/manifest.json".into(),
            manifest: serde_json::from_str(r#"{"id":"a","name":"A","resources":["stream"],"types":["movie"]}"#)
                .unwrap(),
        };
        set_setting(&db, ADDONS_KEY, &serde_json::to_string(&vec![addon.clone()]).unwrap()).unwrap();
        assert_eq!(load(&db).unwrap(), vec![addon]);
    }
}
