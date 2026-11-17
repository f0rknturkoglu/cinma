use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<u32>,
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    #[serde(default)]
    pub behavior_hints: Option<BehaviorHints>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorHints {
    #[serde(default)]
    pub binge_group: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub video_size: Option<u64>,
    #[serde(default)]
    pub not_web_ready: Option<bool>,
}

impl Stream {
    /// Tracker URLs from `sources` (`tracker:<url>` entries).
    pub fn trackers(&self) -> Vec<String> {
        self.sources
            .iter()
            .flatten()
            .filter_map(|s| s.strip_prefix("tracker:").map(str::to_owned))
            .collect()
    }

    /// The text the addon intends users to read: `description` (newer addons)
    /// falling back to `title`.
    pub fn display_title(&self) -> Option<&str> {
        self.description.as_deref().or(self.title.as_deref())
    }
}

#[derive(Debug, Deserialize)]
struct StreamsResponse {
    #[serde(default)]
    streams: Vec<Stream>,
}

/// Parses a stream-resource response body.
pub fn parse_streams(body: &[u8]) -> Result<Vec<Stream>, crate::AddonError> {
    Ok(serde_json::from_slice::<StreamsResponse>(body)?.streams)
}
