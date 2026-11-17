use serde::{Deserialize, Serialize};

use crate::{AddonError, deserialize_null_default, deserialize_opt_string_or_num};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
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
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub awards: Option<String>,
    #[serde(default)]
    pub released: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub cast: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub director: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub writer: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub videos: Vec<MetaVideo>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub trailers: Vec<MetaTrailer>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub links: Vec<MetaLink>,
    #[serde(default)]
    pub behavior_hints: Option<MetaBehaviorHints>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetaVideo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub released: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
}

impl MetaVideo {
    pub fn display_title(&self) -> Option<&str> {
        self.title.as_deref().or(self.name.as_deref())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetaTrailer {
    pub source: String,
    #[serde(rename = "type", default)]
    pub trailer_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetaLink {
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetaBehaviorHints {
    #[serde(default)]
    pub default_video_id: Option<String>,
    #[serde(default)]
    pub has_scheduled_videos: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MetaResponse {
    #[serde(default)]
    meta: Option<Meta>,
}

/// Parses a meta-resource response body (`{"meta": null}` returns `Ok(None)`).
pub fn parse_meta(body: &[u8]) -> Result<Option<Meta>, AddonError> {
    Ok(serde_json::from_slice::<MetaResponse>(body)?.meta)
}
