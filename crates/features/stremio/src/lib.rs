//! Stremio Addon Protocol client (manifest + stream resources for now).
//! Spec: https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/protocol.md

#![forbid(unsafe_code)]

use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

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
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub id_prefixes: Option<Vec<String>>,
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

impl Manifest {
    /// Whether this addon claims to serve `resource` for `media_type`/`id`.
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
            Resource::Full { name, types, id_prefixes } => {
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
}

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
pub fn parse_streams(body: &[u8]) -> Result<Vec<Stream>, AddonError> {
    Ok(serde_json::from_slice::<StreamsResponse>(body)?.streams)
}

/// Normalizes user input (`https://…/manifest.json`, `stremio://…`) to the
/// addon's transport URL (the manifest URL) and its base URL.
pub fn normalize_manifest_url(input: &str) -> Result<(Url, Url), AddonError> {
    let trimmed = input.trim();
    let with_scheme = match trimmed.strip_prefix("stremio://") {
        Some(rest) => format!("https://{rest}"),
        None => trimmed.to_owned(),
    };
    let manifest =
        Url::parse(&with_scheme).map_err(|e| AddonError::InvalidUrl(format!("{trimmed}: {e}")))?;
    if !matches!(manifest.scheme(), "http" | "https") {
        return Err(AddonError::InvalidUrl(trimmed.to_owned()));
    }
    let path = manifest.path();
    let Some(base_path) = path.strip_suffix("manifest.json") else {
        return Err(AddonError::InvalidUrl(format!("{trimmed}: must end in manifest.json")));
    };
    let mut base = manifest.clone();
    base.set_path(base_path.trim_end_matches('/'));
    base.set_query(None);
    Ok((manifest, base))
}

/// Builds `{base}/stream/{type}/{id}.json` (path segments percent-encoded).
pub fn stream_url(base: &Url, media_type: &str, id: &str) -> Result<Url, AddonError> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(base.to_string()))?
        .pop_if_empty()
        .extend(["stream", media_type, &format!("{id}.json")]);
    Ok(url)
}

#[derive(Clone)]
pub struct AddonClient {
    http: reqwest::Client,
}

impl AddonClient {
    pub fn new() -> Result<Self, AddonError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { http })
    }

    async fn get(&self, url: Url) -> Result<Vec<u8>, AddonError> {
        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(AddonError::Status(status.as_u16()));
        }
        Ok(response.bytes().await?.to_vec())
    }

    /// Fetches a manifest. Returns (transport URL, manifest).
    pub async fn fetch_manifest(&self, input: &str) -> Result<(Url, Manifest), AddonError> {
        let (manifest_url, _) = normalize_manifest_url(input)?;
        let body = self.get(manifest_url.clone()).await?;
        Ok((manifest_url, serde_json::from_slice(&body)?))
    }

    pub async fn fetch_streams(
        &self,
        transport_url: &str,
        media_type: &str,
        id: &str,
    ) -> Result<Vec<Stream>, AddonError> {
        let (_, base) = normalize_manifest_url(transport_url)?;
        parse_streams(&self.get(stream_url(&base, media_type, id)?).await?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const TORRENTIO_LIKE: &str = include_str!("../tests/fixtures/streams_torrent.json");
    const MANIFEST: &str = include_str!("../tests/fixtures/manifest.json");

    #[test]
    fn parses_torrent_streams() {
        let streams = parse_streams(TORRENTIO_LIKE.as_bytes()).unwrap();
        assert_eq!(streams.len(), 3);
        let s = &streams[0];
        assert_eq!(s.info_hash.as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(s.file_idx, Some(2));
        assert_eq!(s.trackers(), vec!["udp://tracker.example.org:1337/announce".to_owned()]);
        let hints = s.behavior_hints.as_ref().unwrap();
        assert_eq!(hints.filename.as_deref(), Some("Example.Movie.2024.2160p.WEB-DL.DV.HDR.mkv"));
        assert_eq!(hints.video_size, Some(15_032_385_536));
        assert_eq!(hints.binge_group.as_deref(), Some("example|2160p|DV"));
        assert!(streams[1].file_idx.is_none());
        assert_eq!(streams[2].url.as_deref(), Some("https://cdn.example.org/video.mp4"));
    }

    #[test]
    fn tolerates_unknown_and_missing_fields() {
        let streams = parse_streams(br#"{"streams":[{"infoHash":"aa","extra":1}],"cacheMaxAge":5}"#)
            .unwrap();
        assert_eq!(streams[0].info_hash.as_deref(), Some("aa"));
        assert!(parse_streams(b"{}").unwrap().is_empty());
        assert!(parse_streams(b"not json").is_err());
    }

    #[test]
    fn parses_manifest_and_checks_support() {
        let m: Manifest = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(m.id, "org.example.fixture");
        assert!(m.supports("stream", "movie", "tt1234567"));
        assert!(m.supports("stream", "series", "tt1234567:1:2"));
        assert!(!m.supports("stream", "movie", "kitsu:1"));
        assert!(!m.supports("stream", "tv", "tt1"));
        assert!(!m.supports("subtitles", "movie", "tt1"));
    }

    #[test]
    fn full_resource_overrides_types_and_prefixes() {
        let m: Manifest = serde_json::from_str(
            r#"{"id":"x","name":"x","types":["movie"],"resources":[
                {"name":"stream","types":["series"],"idPrefixes":["kitsu:"]}]}"#,
        )
        .unwrap();
        assert!(m.supports("stream", "series", "kitsu:5"));
        assert!(!m.supports("stream", "movie", "kitsu:5"));
        assert!(!m.supports("stream", "series", "tt1"));
    }

    #[test]
    fn normalizes_urls() {
        let (manifest, base) =
            normalize_manifest_url("stremio://addon.example.org/cfg=a|b/manifest.json").unwrap();
        assert_eq!(manifest.as_str(), "https://addon.example.org/cfg=a|b/manifest.json");
        assert_eq!(base.as_str(), "https://addon.example.org/cfg=a|b");
        assert!(normalize_manifest_url("https://example.org/foo").is_err());
        assert!(normalize_manifest_url("ftp://example.org/manifest.json").is_err());
    }

    #[test]
    fn builds_stream_urls() {
        let (_, base) = normalize_manifest_url("https://a.example/sub/manifest.json").unwrap();
        assert_eq!(
            stream_url(&base, "series", "tt1:1:2").unwrap().as_str(),
            "https://a.example/sub/stream/series/tt1:1:2.json"
        );
        let (_, root) = normalize_manifest_url("http://127.0.0.1:7000/manifest.json").unwrap();
        assert_eq!(
            stream_url(&root, "movie", "tt1").unwrap().as_str(),
            "http://127.0.0.1:7000/stream/movie/tt1.json"
        );
    }
}
