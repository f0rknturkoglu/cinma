//! Optional Fanart.tv v3 client for HD clearlogos & backdrops (requires user API key).

use std::time::Duration;

use serde::Deserialize;

use crate::MetadataError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FanartArt {
    pub logo: Option<String>,
    pub backdrop: Option<String>,
}

#[derive(Clone)]
pub struct FanartClient {
    http: reqwest::Client,
    base_url: String,
}

impl FanartClient {
    pub fn new() -> Result<Self, MetadataError> {
        let http = reqwest::Client::builder()
            .user_agent("Cinma/0.1.0")
            .timeout(Duration::from_secs(12))
            .build()?;
        Ok(Self {
            http,
            base_url: "https://webservice.fanart.tv/v3".to_owned(),
        })
    }

    pub fn with_base_url(base_url: String) -> Result<Self, MetadataError> {
        let mut c = Self::new()?;
        c.base_url = base_url.trim_end_matches('/').to_owned();
        Ok(c)
    }

    pub async fn fetch_art(
        &self,
        api_key: &str,
        media_type: &str,
        lookup_id: &str,
        lang_short: &str,
    ) -> Result<FanartArt, MetadataError> {
        let kind_path = if media_type == "series" || media_type == "tv" {
            "tv"
        } else {
            "movies"
        };
        let url = format!(
            "{}/{kind_path}/{lookup_id}?api_key={api_key}",
            self.base_url
        );
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(FanartArt::default());
        }
        if !status.is_success() {
            return Err(MetadataError::Status(status.as_u16()));
        }
        let body = resp.bytes().await?;
        let parsed: FanartPayload = serde_json::from_slice(&body)?;
        let logos = if kind_path == "tv" {
            parsed
                .hdtvlogo
                .into_iter()
                .chain(parsed.clearlogo)
                .collect::<Vec<_>>()
        } else {
            parsed
                .hdmovielogo
                .into_iter()
                .chain(parsed.movielogo)
                .collect::<Vec<_>>()
        };
        let backdrops = if kind_path == "tv" {
            parsed.showbackground
        } else {
            parsed.moviebackground
        };
        Ok(FanartArt {
            logo: pick_best(&logos, lang_short),
            backdrop: pick_best(&backdrops, lang_short),
        })
    }
}

fn pick_best(items: &[FanartImage], lang_short: &str) -> Option<String> {
    items
        .iter()
        .find(|i| i.lang.as_deref() == Some(lang_short) && !i.url.ends_with(".svg"))
        .or_else(|| {
            items
                .iter()
                .find(|i| i.lang.as_deref() == Some("en") && !i.url.ends_with(".svg"))
        })
        .or_else(|| {
            items
                .iter()
                .find(|i| i.lang.as_deref() == Some("00") && !i.url.ends_with(".svg"))
        })
        .or_else(|| items.iter().find(|i| !i.url.ends_with(".svg")))
        .map(|i| i.url.clone())
}

#[derive(Deserialize)]
struct FanartPayload {
    #[serde(default)]
    hdmovielogo: Vec<FanartImage>,
    #[serde(default)]
    movielogo: Vec<FanartImage>,
    #[serde(default)]
    hdtvlogo: Vec<FanartImage>,
    #[serde(default)]
    clearlogo: Vec<FanartImage>,
    #[serde(default)]
    moviebackground: Vec<FanartImage>,
    #[serde(default)]
    showbackground: Vec<FanartImage>,
}

#[derive(Deserialize)]
struct FanartImage {
    url: String,
    #[serde(default)]
    lang: Option<String>,
}
