//! AniSkip (anime OP/ED/Recap) and IntroDB (TV/movie intro & outro) clients.
//! Used lazily only when `Feature::SkipSegments` is enabled.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::MetadataError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkipKind {
    Intro,
    Outro,
    Recap,
    Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkipSegment {
    pub kind: SkipKind,
    pub start_s: f64,
    pub end_s: f64,
    pub source: String,
}

#[derive(Clone)]
pub struct SkipSegmentClient {
    http: reqwest::Client,
    aniskip_base: String,
    introdb_base: String,
    jikan_base: String,
}

impl SkipSegmentClient {
    pub fn new() -> Result<Self, MetadataError> {
        let http = reqwest::Client::builder()
            .user_agent("Cinma/0.1.0")
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            aniskip_base: "https://api.aniskip.com/v2".to_owned(),
            introdb_base: "https://api.introdb.app".to_owned(),
            jikan_base: "https://api.jikan.moe/v4".to_owned(),
        })
    }

    pub fn with_base_urls(
        aniskip_base: String,
        introdb_base: String,
    ) -> Result<Self, MetadataError> {
        let mut c = Self::new()?;
        c.aniskip_base = aniskip_base.trim_end_matches('/').to_owned();
        c.introdb_base = introdb_base.trim_end_matches('/').to_owned();
        Ok(c)
    }

    pub fn with_jikan_base_url(mut self, jikan_base: String) -> Self {
        self.jikan_base = jikan_base.trim_end_matches('/').to_owned();
        self
    }

    pub async fn fetch_aniskip(
        &self,
        mal_id: i64,
        episode: u32,
    ) -> Result<Vec<SkipSegment>, MetadataError> {
        let url = format!(
            "{}/skip-times/{mal_id}/{episode}?types=op&types=ed&types=recap&types=mixed-op&types=mixed-ed&episodeLength=0",
            self.aniskip_base
        );
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(Vec::new());
        }
        if !status.is_success() {
            return Err(MetadataError::Status(status.as_u16()));
        }
        let body = resp.bytes().await?;
        let parsed: AniSkipResponse = serde_json::from_slice(&body)?;
        if !parsed.found.unwrap_or(true) {
            return Ok(Vec::new());
        }
        Ok(parsed
            .results
            .into_iter()
            .filter_map(|r| {
                let kind = match r.skip_type.as_str() {
                    "op" | "mixed-op" => SkipKind::Intro,
                    "ed" | "mixed-ed" => SkipKind::Outro,
                    "recap" => SkipKind::Recap,
                    _ => return None,
                };
                let iv = r.interval?;
                (iv.end_time > iv.start_time).then(|| SkipSegment {
                    kind,
                    start_s: iv.start_time,
                    end_s: iv.end_time,
                    source: "AniSkip".to_owned(),
                })
            })
            .collect())
    }

    pub async fn fetch_introdb(
        &self,
        imdb_id: &str,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<Vec<SkipSegment>, MetadataError> {
        let mut url = reqwest::Url::parse(&format!("{}/segments", self.introdb_base))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("imdb_id", imdb_id);
            if let Some(s) = season {
                pairs.append_pair("season", &s.to_string());
            }
            if let Some(e) = episode {
                pairs.append_pair("episode", &e.to_string());
            }
        }
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(Vec::new());
        }
        if !status.is_success() {
            return Err(MetadataError::Status(status.as_u16()));
        }
        let body = resp.bytes().await?;
        let parsed: IntroDbResponse = serde_json::from_slice(&body)?;
        let mut out = Vec::new();
        for seg in parsed.into_vec() {
            let kind = match seg.segment_type.as_deref().unwrap_or("intro") {
                "outro" | "credits" => SkipKind::Outro,
                "recap" => SkipKind::Recap,
                "preview" => SkipKind::Preview,
                _ => SkipKind::Intro,
            };
            let start_s = seg.start_sec.unwrap_or(0.0);
            let end_s = seg.end_sec.unwrap_or(0.0);
            if end_s > start_s {
                out.push(SkipSegment {
                    kind,
                    start_s,
                    end_s,
                    source: "IntroDB".to_owned(),
                });
            }
        }
        Ok(out)
    }

    /// Fetches episode numbers flagged as filler on MyAnimeList via Jikan v4 (`/anime/{mal_id}/episodes`).
    pub async fn fetch_anime_filler_episodes(
        &self,
        mal_id: i64,
    ) -> Result<Vec<u32>, MetadataError> {
        let mut fillers = Vec::new();
        for page in 1u32..=5 {
            let url = format!("{}/anime/{mal_id}/episodes?page={page}", self.jikan_base);
            let resp = self.http.get(&url).send().await?;
            let status = resp.status();
            if status.as_u16() == 404 {
                break;
            }
            if !status.is_success() {
                return Err(MetadataError::Status(status.as_u16()));
            }
            let body = resp.bytes().await?;
            let parsed: JikanEpisodesResp = serde_json::from_slice(&body)?;
            for ep in parsed.data {
                if ep.filler
                    && let Ok(num) = u32::try_from(ep.mal_id)
                {
                    fillers.push(num);
                }
            }
            if !parsed
                .pagination
                .and_then(|p| p.has_next_page)
                .unwrap_or(false)
            {
                break;
            }
        }
        fillers.sort_unstable();
        fillers.dedup();
        Ok(fillers)
    }
}

#[derive(Deserialize)]
struct JikanEpisodesResp {
    #[serde(default)]
    data: Vec<JikanEpisodeItem>,
    pagination: Option<JikanPagination>,
}

#[derive(Deserialize)]
struct JikanEpisodeItem {
    mal_id: i64,
    #[serde(default)]
    filler: bool,
}

#[derive(Deserialize)]
struct JikanPagination {
    has_next_page: Option<bool>,
}

#[derive(Deserialize)]
struct AniSkipResponse {
    found: Option<bool>,
    #[serde(default)]
    results: Vec<AniSkipItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniSkipItem {
    skip_type: String,
    interval: Option<AniSkipInterval>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniSkipInterval {
    start_time: f64,
    end_time: f64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IntroDbResponse {
    Wrapped { segments: Vec<IntroDbSegment> },
    Direct(Vec<IntroDbSegment>),
    Single(IntroDbSingle),
}

impl IntroDbResponse {
    fn into_vec(self) -> Vec<IntroDbSegment> {
        match self {
            Self::Wrapped { segments } => segments,
            Self::Direct(v) => v,
            Self::Single(s) => {
                let mut v = Vec::new();
                if let (Some(st), Some(en)) = (s.start_sec, s.end_sec) {
                    v.push(IntroDbSegment {
                        segment_type: Some("intro".to_owned()),
                        start_sec: Some(st),
                        end_sec: Some(en),
                    });
                }
                v
            }
        }
    }
}

#[derive(Deserialize)]
struct IntroDbSingle {
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

#[derive(Deserialize)]
struct IntroDbSegment {
    #[serde(rename = "type", alias = "segment_type")]
    segment_type: Option<String>,
    #[serde(alias = "start")]
    start_sec: Option<f64>,
    #[serde(alias = "end")]
    end_sec: Option<f64>,
}
