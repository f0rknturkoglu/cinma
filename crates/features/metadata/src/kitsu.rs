//! Kitsu JSON:API metadata fallback for anime titles (`ROADMAP.md` §8 priority 7).

use std::time::Duration;

use cinma_db::{IdMapEntry, MediaKind};
use cinma_stremio::Meta;
use serde::Deserialize;

use crate::{MetadataError, RichMeta};

#[derive(Clone)]
pub struct KitsuClient {
    http: reqwest::Client,
    base_url: String,
}

impl KitsuClient {
    pub fn new() -> Result<Self, MetadataError> {
        let http = reqwest::Client::builder()
            .user_agent("Cinma/0.1.0")
            .timeout(Duration::from_secs(12))
            .build()?;
        Ok(Self {
            http,
            base_url: "https://kitsu.io/api/edge".to_owned(),
        })
    }

    pub fn with_base_url(base_url: String) -> Result<Self, MetadataError> {
        let mut c = Self::new()?;
        c.base_url = base_url.trim_end_matches('/').to_owned();
        Ok(c)
    }

    pub async fn anime_details(&self, kitsu_id: i64) -> Result<RichMeta, MetadataError> {
        let url = format!("{}/anime/{kitsu_id}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(MetadataError::NotFound(format!("kitsu:{kitsu_id}")));
        }
        if !status.is_success() {
            return Err(MetadataError::Status(status.as_u16()));
        }
        let body = resp.bytes().await?;
        let parsed: KitsuEnvelope = serde_json::from_slice(&body)?;
        let attr = parsed.data.attributes;
        let title = attr
            .canonical_title
            .or(attr.titles.and_then(|t| t.en.or(t.en_jp)))
            .unwrap_or_else(|| format!("Kitsu #{kitsu_id}"));
        let is_movie = attr.subtype.as_deref() == Some("movie");
        let meta = Meta {
            id: format!("kitsu:{kitsu_id}"),
            media_type: if is_movie { "movie" } else { "series" }.to_owned(),
            name: title,
            poster: attr
                .poster_image
                .and_then(|p| p.large.or(p.medium).or(p.original)),
            poster_shape: Some("poster".to_owned()),
            background: attr.cover_image.and_then(|c| c.original.or(c.large)),
            logo: None,
            description: attr.synopsis,
            release_info: attr
                .start_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .map(str::to_owned),
            imdb_rating: attr
                .average_rating
                .and_then(|r| r.parse::<f64>().ok())
                .map(|pct| format!("{:.1}", pct / 10.0)),
            runtime: attr.episode_length.map(|m| format!("{m} min")),
            country: Some("JP".to_owned()),
            awards: None,
            released: attr.start_date,
            genres: Vec::new(),
            cast: Vec::new(),
            director: Vec::new(),
            writer: Vec::new(),
            videos: Vec::new(),
            trailers: Vec::new(),
            links: Vec::new(),
            behavior_hints: None,
        };
        let id_map = IdMapEntry {
            media_kind: if is_movie {
                MediaKind::Movie
            } else {
                MediaKind::Series
            },
            imdb_id: None,
            tmdb_id: None,
            tvdb_id: None,
            trakt_id: None,
            simkl_id: None,
            mal_id: None,
            anilist_id: None,
            kitsu_id: Some(kitsu_id),
            season: None,
            episode: None,
        };
        Ok(RichMeta {
            meta,
            id_map,
            season_numbers: if is_movie { Vec::new() } else { vec![1] },
            next_episode: None,
        })
    }
}

#[derive(Deserialize)]
struct KitsuEnvelope {
    data: KitsuData,
}

#[derive(Deserialize)]
struct KitsuData {
    attributes: KitsuAttr,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KitsuAttr {
    canonical_title: Option<String>,
    titles: Option<KitsuTitles>,
    synopsis: Option<String>,
    subtype: Option<String>,
    start_date: Option<String>,
    average_rating: Option<String>,
    episode_length: Option<u32>,
    poster_image: Option<KitsuImages>,
    cover_image: Option<KitsuImages>,
}

#[derive(Deserialize)]
struct KitsuTitles {
    en: Option<String>,
    en_jp: Option<String>,
}

#[derive(Deserialize)]
struct KitsuImages {
    original: Option<String>,
    large: Option<String>,
    medium: Option<String>,
}
