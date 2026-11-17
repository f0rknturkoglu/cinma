use serde::{Deserialize, Serialize};

use crate::AddonError;
use crate::catalog::percent_encode_value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleQuery {
    pub video_hash: Option<String>,
    pub video_size: Option<u64>,
    pub filename: Option<String>,
}

impl SubtitleQuery {
    pub fn is_empty(&self) -> bool {
        self.video_hash.as_deref().is_none_or(str::is_empty)
            && self.video_size.is_none()
            && self.filename.as_deref().is_none_or(str::is_empty)
    }

    pub fn encode_extra(&self) -> String {
        let mut parts = Vec::new();
        if let Some(hash) = self.video_hash.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("videoHash={}", percent_encode_value(hash)));
        }
        if let Some(size) = self.video_size {
            parts.push(format!("videoSize={size}"));
        }
        if let Some(name) = self.filename.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("filename={}", percent_encode_value(name)));
        }
        parts.join("&")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AddonSubtitle {
    pub id: String,
    pub url: String,
    pub lang: String,
    #[serde(default, alias = "SubEncoding")]
    pub sub_encoding: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, alias = "hearing_impaired", alias = "SubHearingImpaired")]
    pub hearing_impaired: Option<bool>,
    #[serde(default)]
    pub forced: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubtitlesResponse {
    #[serde(default)]
    pub subtitles: Vec<AddonSubtitle>,
}

pub fn parse_subtitles(body: &[u8]) -> Result<Vec<AddonSubtitle>, AddonError> {
    let parsed: SubtitlesResponse = serde_json::from_slice(body)?;
    Ok(parsed.subtitles)
}
