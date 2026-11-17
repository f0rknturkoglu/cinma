//! Stremio Addon Protocol client (manifest, catalog, meta, stream).
//! Spec: https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/protocol.md

#![forbid(unsafe_code)]

mod catalog;
mod client;
mod manifest;
mod meta;
mod stream;

use serde::{Deserialize, Deserializer};

pub use catalog::{CatalogQuery, CatalogResponse, MetaPreview, parse_catalog};
pub use client::{AddonClient, catalog_url, meta_url, normalize_manifest_url, stream_url};
pub use manifest::{CatalogDescriptor, CatalogExtra, Manifest, ManifestBehaviorHints, Resource};
pub use meta::{Meta, MetaBehaviorHints, MetaLink, MetaTrailer, MetaVideo, parse_meta};
pub use stream::{BehaviorHints, Stream, parse_streams};

#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    #[error("invalid addon URL: {0}")]
    InvalidUrl(String),
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("addon returned HTTP {0}")]
    Status(u16),
    #[error("invalid addon response: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Deserializes JSON `null` as `T::default()` (real-world addons often send
/// `"cast": null` or `"genres": null` instead of omitting the field).
pub(crate) fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Tolerates `"imdbRating": "8.7"`, `"imdbRating": 8.7`, or `null`.
pub(crate) fn deserialize_opt_string_or_num<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNum {
        Str(String),
        Num(serde_json::Number),
    }

    Ok(
        Option::<StringOrNum>::deserialize(deserializer)?.and_then(|v| match v {
            StringOrNum::Str(s) => (!s.trim().is_empty()).then_some(s),
            StringOrNum::Num(n) => Some(n.to_string()),
        }),
    )
}
