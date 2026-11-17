use serde::{Deserialize, Serialize};

use crate::deserialize_null_default;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub resources: Vec<Resource>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub types: Vec<String>,
    #[serde(default)]
    pub id_prefixes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub catalogs: Vec<CatalogDescriptor>,
    #[serde(default)]
    pub behavior_hints: Option<ManifestBehaviorHints>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestBehaviorHints {
    #[serde(default)]
    pub configurable: Option<bool>,
    #[serde(default)]
    pub configuration_required: Option<bool>,
    #[serde(default)]
    pub adult: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Resource {
    Short(String),
    #[serde(rename_all = "camelCase")]
    Full {
        name: String,
        #[serde(default)]
        types: Option<Vec<String>>,
        #[serde(default)]
        id_prefixes: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDescriptor {
    #[serde(rename = "type")]
    pub media_type: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub extra: Vec<CatalogExtra>,
    #[serde(default)]
    pub extra_supported: Option<Vec<String>>,
    #[serde(default)]
    pub extra_required: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogExtra {
    pub name: String,
    #[serde(default)]
    pub is_required: bool,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub options_limit: Option<u32>,
}

impl CatalogDescriptor {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    pub fn supports_extra(&self, name: &str) -> bool {
        self.extra.iter().any(|e| e.name == name)
            || self
                .extra_supported
                .as_ref()
                .is_some_and(|list| list.iter().any(|n| n == name))
    }

    pub fn requires_extra(&self, name: &str) -> bool {
        self.extra.iter().any(|e| e.name == name && e.is_required)
            || self
                .extra_required
                .as_ref()
                .is_some_and(|list| list.iter().any(|n| n == name))
    }

    pub fn is_searchable(&self) -> bool {
        self.supports_extra("search")
    }

    /// Whether this catalog can be fetched without required extra parameters.
    pub fn is_browsable(&self) -> bool {
        !self.extra.iter().any(|e| e.is_required)
            && self
                .extra_required
                .as_ref()
                .is_none_or(|list| list.is_empty())
    }

    pub fn genres(&self) -> &[String] {
        self.extra
            .iter()
            .find(|e| e.name == "genre")
            .and_then(|e| e.options.as_deref())
            .unwrap_or(&[])
    }
}

impl Manifest {
    /// Whether this addon claims to serve `resource` for `media_type`/`id`.
    /// Intended for item-level resources (`meta`, `stream`, `subtitles`).
    pub fn supports(&self, resource: &str, media_type: &str, id: &str) -> bool {
        let prefix_ok = |prefixes: Option<&Vec<String>>| {
            prefixes.is_none_or(|p| p.iter().any(|prefix| id.starts_with(prefix.as_str())))
        };
        self.resources.iter().any(|r| match r {
            Resource::Short(name) => {
                name == resource
                    && self.types.iter().any(|t| t == media_type)
                    && prefix_ok(self.id_prefixes.as_ref())
            }
            Resource::Full {
                name,
                types,
                id_prefixes,
            } => {
                name == resource
                    && types
                        .as_ref()
                        .unwrap_or(&self.types)
                        .iter()
                        .any(|t| t == media_type)
                    && prefix_ok(id_prefixes.as_ref().or(self.id_prefixes.as_ref()))
            }
        })
    }

    /// Whether this addon declares the `catalog` resource for `media_type`.
    pub fn supports_catalog_resource(&self, media_type: &str) -> bool {
        self.resources.iter().any(|r| match r {
            Resource::Short(name) => {
                name == "catalog"
                    && (self.types.is_empty() || self.types.iter().any(|t| t == media_type))
            }
            Resource::Full { name, types, .. } => {
                name == "catalog"
                    && types
                        .as_ref()
                        .unwrap_or(&self.types)
                        .iter()
                        .any(|t| t == media_type)
            }
        })
    }

    /// Finds a declared catalog matching `(media_type, catalog_id)` if the
    /// addon exposes the `catalog` resource.
    pub fn supports_catalog(
        &self,
        media_type: &str,
        catalog_id: &str,
    ) -> Option<&CatalogDescriptor> {
        if !self.supports_catalog_resource(media_type) {
            return None;
        }
        self.catalogs
            .iter()
            .find(|c| c.media_type == media_type && c.id == catalog_id)
    }

    /// All catalogs exposed by this addon (provided it declares the `catalog` resource).
    pub fn available_catalogs(&self) -> impl Iterator<Item = &CatalogDescriptor> {
        self.catalogs
            .iter()
            .filter(|c| self.supports_catalog_resource(&c.media_type))
    }

    pub fn browsable_catalogs(&self) -> impl Iterator<Item = &CatalogDescriptor> {
        self.available_catalogs().filter(|c| c.is_browsable())
    }

    pub fn searchable_catalogs(&self) -> impl Iterator<Item = &CatalogDescriptor> {
        self.available_catalogs().filter(|c| c.is_searchable())
    }
}
