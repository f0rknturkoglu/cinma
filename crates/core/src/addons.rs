//! Installed addons (per-profile enablement) and Stremio resource queries
//! (streams, catalogs, search, meta).

use std::sync::Arc;

use cinma_ranker::{MachineCapabilities, RankResult, RankerPreferences, rank_streams};
use cinma_stremio::{CatalogDescriptor, CatalogQuery, Manifest, Meta, MetaPreview, Stream};

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

#[derive(Debug, Clone)]
pub struct StreamBatch {
    pub addon_id: String,
    pub addon_name: String,
    pub streams: Vec<StreamCandidate>,
    pub error: Option<String>,
}

pub trait StreamBatchListener: Send + Sync {
    fn on_batch(&self, batch: StreamBatch);
}

#[derive(Debug, Clone, PartialEq)]
pub struct AddonCatalog {
    pub addon_id: String,
    pub addon_name: String,
    pub transport_url: String,
    pub catalog: CatalogDescriptor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPage {
    pub addon_id: String,
    pub addon_name: String,
    pub media_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub items: Vec<MetaPreview>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogBatch {
    pub addon_id: String,
    pub addon_name: String,
    pub media_type: String,
    pub catalog_id: String,
    pub catalog_name: String,
    pub items: Vec<MetaPreview>,
    pub has_more: bool,
    pub error: Option<String>,
}

pub trait CatalogBatchListener: Send + Sync {
    fn on_batch(&self, batch: CatalogBatch);
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaResult {
    pub addon_id: String,
    pub addon_name: String,
    pub meta: Meta,
}

impl Core {
    /// Installed addons in user order, with the active profile's enabled flag.
    pub fn addons(&self) -> Result<Vec<InstalledAddon>, CoreError> {
        let profile = self.active_profile()?;
        let rows = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .addons_for_profile(profile)?;
        rows.into_iter()
            .map(|row| {
                let manifest = serde_json::from_str(&row.manifest_json)
                    .map_err(|e| CoreError::Invalid(format!("stored manifest {}: {e}", row.id)))?;
                Ok(InstalledAddon {
                    transport_url: row.transport_url,
                    manifest,
                    enabled: row.enabled,
                })
            })
            .collect()
    }

    /// Fetches and parses an addon's manifest without installing it.
    pub async fn inspect_addon(self: &Arc<Self>, url: String) -> Result<InstalledAddon, CoreError> {
        let http = self.http()?;
        let (transport_url, manifest) = self
            .spawn(async move { Ok(http.fetch_manifest(&url).await?) })
            .await?;
        Ok(InstalledAddon {
            transport_url: transport_url.to_string(),
            manifest,
            enabled: false,
        })
    }

    /// Fetches the manifest and installs (or refreshes) the addon, enabled
    /// for the active profile.
    pub async fn install_addon(self: &Arc<Self>, url: String) -> Result<InstalledAddon, CoreError> {
        let preview = self.inspect_addon(url).await?;
        let manifest_json = serde_json::to_string(&preview.manifest)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .upsert_addon(
                profile,
                &preview.manifest.id,
                &preview.transport_url,
                &manifest_json,
            )?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(InstalledAddon {
            enabled: true,
            ..preview
        })
    }

    pub fn set_addon_enabled(&self, id: &str, enabled: bool) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_addon_enabled(profile, id, enabled)?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(())
    }

    /// Uninstalls for every profile.
    pub fn remove_addon(&self, id: &str) -> Result<(), CoreError> {
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .remove_addon(id)?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(())
    }

    pub fn reorder_addons(&self, ordered_ids: Vec<String>) -> Result<(), CoreError> {
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .reorder_addons(&ordered_ids)?;
        self.emit(CoreEvent::AddonsChanged);
        Ok(())
    }

    /// Catalogs declared by enabled addons in user order.
    pub fn catalogs(&self) -> Result<Vec<AddonCatalog>, CoreError> {
        self.require(Feature::Addons)?;
        Ok(self
            .addons()?
            .into_iter()
            .filter(|a| a.enabled)
            .flat_map(|addon| {
                let addon_id = addon.manifest.id.clone();
                let addon_name = addon.manifest.name.clone();
                let transport_url = addon.transport_url.clone();
                addon
                    .manifest
                    .available_catalogs()
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(move |catalog| AddonCatalog {
                        addon_id: addon_id.clone(),
                        addon_name: addon_name.clone(),
                        transport_url: transport_url.clone(),
                        catalog,
                    })
            })
            .collect())
    }

    /// Queries one catalog on one enabled addon (supporting `search`, `genre`, `skip`).
    pub async fn catalog(
        self: &Arc<Self>,
        addon_id: String,
        media_type: String,
        catalog_id: String,
        query: CatalogQuery,
    ) -> Result<CatalogPage, CoreError> {
        self.require(Feature::Addons)?;
        let addon = self
            .addons()?
            .into_iter()
            .find(|a| a.enabled && a.manifest.id == addon_id)
            .ok_or_else(|| CoreError::Invalid(format!("enabled addon `{addon_id}` not found")))?;
        let descriptor = addon
            .manifest
            .supports_catalog(&media_type, &catalog_id)
            .ok_or_else(|| {
                CoreError::Invalid(format!(
                    "addon `{}` does not serve catalog {media_type}/{catalog_id}",
                    addon.manifest.name
                ))
            })?;
        let catalog_name = descriptor.display_name().to_owned();
        let descriptor_media_type = descriptor.media_type.clone();
        let descriptor_id = descriptor.id.clone();
        let addon_name = addon.manifest.name;
        let transport_url = addon.transport_url;
        let http = self.http()?;
        let response = self
            .spawn(async move {
                Ok(http
                    .fetch_catalog(&transport_url, &media_type, &catalog_id, &query)
                    .await?)
            })
            .await?;
        let has_more = response.has_more.unwrap_or(false);
        Ok(CatalogPage {
            addon_id,
            addon_name,
            media_type: descriptor_media_type,
            catalog_id: descriptor_id,
            catalog_name,
            items: response.metas,
            has_more,
        })
    }

    /// Loads every browsable catalog from enabled addons in parallel, emitting
    /// partial batches as each addon answers.
    pub async fn browse_catalogs(
        self: &Arc<Self>,
        media_type: Option<String>,
        listener: Option<Arc<dyn CatalogBatchListener>>,
    ) -> Result<Vec<CatalogPage>, CoreError> {
        let targets: Vec<AddonCatalog> = self
            .catalogs()?
            .into_iter()
            .filter(|c| {
                c.catalog.is_browsable()
                    && media_type
                        .as_deref()
                        .is_none_or(|t| c.catalog.media_type == t)
            })
            .collect();
        self.fetch_catalog_fanout(targets, CatalogQuery::default(), listener)
            .await
    }

    /// Searches every searchable catalog across enabled addons in parallel,
    /// emitting partial batches as each addon answers.
    pub async fn search_catalogs(
        self: &Arc<Self>,
        query: String,
        media_type: Option<String>,
        listener: Option<Arc<dyn CatalogBatchListener>>,
    ) -> Result<Vec<CatalogPage>, CoreError> {
        let trimmed = query.trim().to_owned();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let targets: Vec<AddonCatalog> = self
            .catalogs()?
            .into_iter()
            .filter(|c| {
                c.catalog.is_searchable()
                    && media_type
                        .as_deref()
                        .is_none_or(|t| c.catalog.media_type == t)
            })
            .collect();
        self.fetch_catalog_fanout(
            targets,
            CatalogQuery {
                search: Some(trimmed),
                genre: None,
                skip: None,
            },
            listener,
        )
        .await
    }

    async fn fetch_catalog_fanout(
        self: &Arc<Self>,
        targets: Vec<AddonCatalog>,
        query: CatalogQuery,
        listener: Option<Arc<dyn CatalogBatchListener>>,
    ) -> Result<Vec<CatalogPage>, CoreError> {
        self.require(Feature::Addons)?;
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let http = self.http()?;
        self.spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for (index, target) in targets.into_iter().enumerate() {
                let (http, query) = (http.clone(), query.clone());
                tasks.spawn(async move {
                    let result = http
                        .fetch_catalog(
                            &target.transport_url,
                            &target.catalog.media_type,
                            &target.catalog.id,
                            &query,
                        )
                        .await;
                    (index, target, result)
                });
            }
            let mut results = Vec::new();
            let mut errors = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                let Ok((index, target, result)) = joined else {
                    continue;
                };
                let catalog_name = target.catalog.display_name().to_owned();
                match result {
                    Ok(resp) => {
                        let has_more = resp.has_more.unwrap_or(false);
                        if let Some(ref listener) = listener {
                            listener.on_batch(CatalogBatch {
                                addon_id: target.addon_id.clone(),
                                addon_name: target.addon_name.clone(),
                                media_type: target.catalog.media_type.clone(),
                                catalog_id: target.catalog.id.clone(),
                                catalog_name: catalog_name.clone(),
                                items: resp.metas.clone(),
                                has_more,
                                error: None,
                            });
                        }
                        results.push((
                            index,
                            CatalogPage {
                                addon_id: target.addon_id,
                                addon_name: target.addon_name,
                                media_type: target.catalog.media_type,
                                catalog_id: target.catalog.id,
                                catalog_name,
                                items: resp.metas,
                                has_more,
                            },
                        ));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if let Some(ref listener) = listener {
                            listener.on_batch(CatalogBatch {
                                addon_id: target.addon_id,
                                addon_name: target.addon_name,
                                media_type: target.catalog.media_type,
                                catalog_id: target.catalog.id,
                                catalog_name,
                                items: Vec::new(),
                                has_more: false,
                                error: Some(msg.clone()),
                            });
                        }
                        errors.push(msg);
                    }
                }
            }
            if results.is_empty() && !errors.is_empty() {
                return Err(CoreError::Invalid(errors.join("; ")));
            }
            results.sort_by_key(|(index, _)| *index);
            Ok(results.into_iter().map(|(_, page)| page).collect())
        })
        .await
    }

    /// Queries enabled addons that serve `meta` for `media_type`/`id` in
    /// parallel and returns the first valid metadata in addon order.
    pub async fn meta(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<MetaResult, CoreError> {
        self.require(Feature::Addons)?;
        let addons: Vec<InstalledAddon> = self
            .addons()?
            .into_iter()
            .filter(|a| a.enabled && a.manifest.supports("meta", &media_type, &id))
            .collect();
        if addons.is_empty() {
            return Err(CoreError::Invalid(format!(
                "no enabled addon serves meta for {media_type} {id}"
            )));
        }
        let http = self.http()?;
        self.spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for (index, addon) in addons.into_iter().enumerate() {
                let (http, media_type, id) = (http.clone(), media_type.clone(), id.clone());
                tasks.spawn(async move {
                    let result = http
                        .fetch_meta(&addon.transport_url, &media_type, &id)
                        .await;
                    (index, addon.manifest.id, addon.manifest.name, result)
                });
            }
            let mut found = Vec::new();
            let mut errors = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                let Ok((index, addon_id, addon_name, result)) = joined else {
                    continue;
                };
                match result {
                    Ok(Some(meta)) => found.push((
                        index,
                        MetaResult {
                            addon_id,
                            addon_name,
                            meta,
                        },
                    )),
                    Ok(None) => {}
                    Err(e) => errors.push(format!("{addon_name}: {e}")),
                }
            }
            found.sort_by_key(|(index, _)| *index);
            if let Some((_, best)) = found.into_iter().next() {
                return Ok(best);
            }
            if !errors.is_empty() {
                return Err(CoreError::Invalid(errors.join("; ")));
            }
            Err(CoreError::Invalid(format!(
                "meta not found for {media_type} {id}"
            )))
        })
        .await
    }

    /// Queries every enabled addon that serves streams for `media_type`/`id`
    /// in parallel; results keep addon order. Failing addons are skipped.
    pub async fn streams(
        self: &Arc<Self>,
        media_type: String,
        id: String,
    ) -> Result<Vec<StreamCandidate>, CoreError> {
        self.streams_streaming(media_type, id, None).await
    }

    /// Same as [`Core::streams`], notifying `listener` as each addon responds.
    pub async fn streams_streaming(
        self: &Arc<Self>,
        media_type: String,
        id: String,
        listener: Option<Arc<dyn StreamBatchListener>>,
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
                    let result = http
                        .fetch_streams(&addon.transport_url, &media_type, &id)
                        .await;
                    (index, addon.manifest.id, addon.manifest.name, result)
                });
            }
            let mut results = Vec::new();
            let mut errors = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                let Ok((index, addon_id, addon_name, result)) = joined else {
                    continue;
                };
                match result {
                    Ok(streams) => {
                        let candidates: Vec<StreamCandidate> = streams
                            .into_iter()
                            .map(|stream| StreamCandidate {
                                addon_name: addon_name.clone(),
                                stream,
                            })
                            .collect();
                        if let Some(ref listener) = listener {
                            listener.on_batch(StreamBatch {
                                addon_id,
                                addon_name,
                                streams: candidates.clone(),
                                error: None,
                            });
                        }
                        results.push((index, candidates));
                    }
                    Err(e) => {
                        let msg = format!("{addon_name}: {e}");
                        if let Some(ref listener) = listener {
                            listener.on_batch(StreamBatch {
                                addon_id,
                                addon_name,
                                streams: Vec::new(),
                                error: Some(msg.clone()),
                            });
                        }
                        errors.push(msg);
                    }
                }
            }
            if results.is_empty() && !errors.is_empty() {
                return Err(CoreError::Invalid(errors.join("; ")));
            }
            results.sort_by_key(|(index, _)| *index);
            Ok(results.into_iter().flat_map(|(_, c)| c).collect())
        })
        .await
    }

    /// Ranks stream candidates using machine capabilities (e.g. M3+ AV1 decode),
    /// active block-word filters, stream sort order, and the provided or default preferences.
    pub fn rank_streams(
        &self,
        candidates: Vec<StreamCandidate>,
        prefs: Option<RankerPreferences>,
    ) -> RankResult {
        let caps = machine_capabilities();
        let prefs = prefs.unwrap_or_default();
        let player_prefs = self.player_preferences().unwrap_or_default();
        let block_words: Vec<String> = player_prefs
            .track_block_words
            .iter()
            .map(|w| w.trim().to_ascii_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        let raw = candidates
            .into_iter()
            .filter(|c| {
                if block_words.is_empty() {
                    return true;
                }
                let filename = c
                    .stream
                    .behavior_hints
                    .as_ref()
                    .and_then(|h| h.filename.as_deref())
                    .unwrap_or("");
                let combined = format!(
                    "{} {} {} {}",
                    c.stream.name.as_deref().unwrap_or(""),
                    c.stream.title.as_deref().unwrap_or(""),
                    c.stream.description.as_deref().unwrap_or(""),
                    filename
                )
                .to_ascii_lowercase();
                !block_words.iter().any(|w| combined.contains(w))
            })
            .map(|c| (c.addon_name, c.stream))
            .collect();
        let mut result = rank_streams(raw, &prefs, &caps);
        if player_prefs.debrid_only_mode {
            result
                .ranked
                .retain(|r| !r.parsed.is_torrent || r.parsed.cached_debrid);
        }
        match player_prefs.stream_sort.as_str() {
            "seeds" => result.ranked.sort_by(|a, b| {
                b.parsed
                    .seeders
                    .unwrap_or(0)
                    .cmp(&a.parsed.seeders.unwrap_or(0))
                    .then_with(|| b.score.cmp(&a.score))
            }),
            "size_desc" => result.ranked.sort_by(|a, b| {
                b.parsed
                    .size_bytes
                    .unwrap_or(0)
                    .cmp(&a.parsed.size_bytes.unwrap_or(0))
                    .then_with(|| b.score.cmp(&a.score))
            }),
            "size_asc" => result.ranked.sort_by(|a, b| {
                a.parsed
                    .size_bytes
                    .unwrap_or(u64::MAX)
                    .cmp(&b.parsed.size_bytes.unwrap_or(u64::MAX))
                    .then_with(|| b.score.cmp(&a.score))
            }),
            _ => {}
        }
        result
    }

    pub fn hardware_av1_supported(&self) -> bool {
        machine_capabilities().hardware_av1
    }
}

pub(crate) fn machine_capabilities_pub() -> MachineCapabilities {
    machine_capabilities()
}

/// Detected once per process, in-process via `sysctlbyname`.
fn machine_capabilities() -> MachineCapabilities {
    static CACHED: std::sync::OnceLock<MachineCapabilities> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        let brand = cinma_player::sysctl_string("machdep.cpu.brand_string").unwrap_or_default();
        let model = cinma_player::sysctl_string("hw.model").unwrap_or_default();
        MachineCapabilities::from_sysctl(&brand, &model)
    })
}
