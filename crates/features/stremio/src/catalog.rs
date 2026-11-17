use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::{AddonError, deserialize_null_default, deserialize_opt_string_or_num};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogQuery {
    pub search: Option<String>,
    pub genre: Option<String>,
    pub skip: Option<u32>,
}

impl CatalogQuery {
    pub fn is_empty(&self) -> bool {
        self.search.as_deref().is_none_or(str::is_empty)
            && self.genre.as_deref().is_none_or(str::is_empty)
            && self.skip.is_none()
    }

    /// Encodes non-empty extras in Stremio path format:
    /// `key=percent_encoded_val&key2=val2`.
    pub fn encode_extra(&self) -> String {
        let mut parts = Vec::new();
        if let Some(search) = self.search.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("search={}", percent_encode_value(search)));
        }
        if let Some(genre) = self.genre.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("genre={}", percent_encode_value(genre)));
        }
        if let Some(skip) = self.skip {
            parts.push(format!("skip={skip}"));
        }
        parts.join("&")
    }
}

fn percent_encode_value(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        if matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        ) {
            out.push(*byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetaPreview {
    pub id: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub poster_shape: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub release_info: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_or_num")]
    pub imdb_rating: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub genres: Vec<String>,
    #[serde(default)]
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogResponse {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub metas: Vec<MetaPreview>,
    #[serde(default)]
    pub has_more: Option<bool>,
}

/// Parses a catalog-resource response body.
pub fn parse_catalog(body: &[u8]) -> Result<CatalogResponse, AddonError> {
    Ok(serde_json::from_slice(body)?)
}
