//! Installed addons (per-profile enablement) and stream lookup.

use std::sync::Arc;

use cinma_stremio::{Manifest, Stream};

use crate::events::CoreEvent;
use crate::settings::Feature;
use crate::{Core, CoreError};

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledAddon {
    pub transport_url: String,
    pub manifest: Manifest,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct StreamCandidate {
    pub addon_name: String,
    pub stream: Stream,
}

impl Core {
    /// Installed addons in user order, with the active profile's enabled flag.
    pub fn addons(&self) -> Result<Vec<InstalledAddon>, CoreError> {
        let profile = self.active_profile()?;
        let rows = self.db.lock().map_err(|_| CoreError::Poisoned)?.addons_for_profile(profile)?;
        rows.into_iter()
            .map(|row| {
                let manifest = serde_json::from_str(&row.manifest_json)
                    .map_err(|e| CoreError::Invalid(format!("stored manifest {}: {e}", row.id)))?;
                Ok(InstalledAddon { transport_url: row.transport_url, manifest, enabled: row.enabled })
            })
            .collect()
    }

    /// Fetches the manifest and installs (or refreshes) the addon, enabled
    /// for the active profile.
    pub async fn install_addon(self: &Arc<Self>, url: String) -> Result<InstalledAddon, CoreError> {
        let http = self.http()?;
        let (transport_url, manifest) = self
            .spawn(async move { Ok(http.fetch_manifest(&url).await?) })
            .await?;
        let manifest_json = serde_json::to_string(&manifest).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let profile = self.active_profile()?;
        self.db.lock().map_err(|_| CoreError::Poisoned)?.upsert_addon(
            profile,
            &manifest.id,
            transport_url.as_str(),
            &manifest_json,
        )?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(InstalledAddon { transport_url: transport_url.to_string(), manifest, enabled: true })
    }

    pub fn set_addon_enabled(&self, id: &str, enabled: bool) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.db.lock().map_err(|_| CoreError::Poisoned)?.set_addon_enabled(profile, id, enabled)?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(())
    }

    /// Uninstalls for every profile.
    pub fn remove_addon(&self, id: &str) -> Result<(), CoreError> {
        self.db.lock().map_err(|_| CoreError::Poisoned)?.remove_addon(id)?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(())
    }

    /// Queries every enabled addon that serves streams for `media_type`/`id`
    /// in parallel; results keep addon order. Failing addons are skipped.
    pub async fn streams(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<Vec<StreamCandidate>, CoreError> {
        self.require(Feature::Addons)?;
        let addons: Vec<InstalledAddon> = self
            .addons()?
            .into_iter()
            .filter(|a| a.enabled && a.manifest.supports("stream", &media_type, &id))
            .collect();
        if addons.is_empty() {
            return Err(CoreError::Invalid(format!(
                "no enabled addon serves streams for {media_type} {id}"
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
