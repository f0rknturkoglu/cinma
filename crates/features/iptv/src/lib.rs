//! Live TV / IPTV parsers & clients (M3U/M3U8, Xtream Codes, XMLTV EPG).
//! Loaded lazily only when `Feature::Iptv` is enabled (`CLAUDE.md` §1.5).

#![forbid(unsafe_code)]

pub mod m3u;
pub mod xmltv;
pub mod xtream;

use std::time::Duration;

use cinma_db::{IptvEpgRow, NewIptvChannel};

#[derive(Debug, thiserror::Error)]
pub enum IptvError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IPTV provider returned HTTP {0}")]
    HttpStatus(u16),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct IptvClient {
    http: reqwest::Client,
}

impl IptvClient {
    pub fn new() -> Result<Self, IptvError> {
        let http = reqwest::Client::builder()
            .user_agent("Cinma/0.1.0")
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http })
    }

    pub async fn fetch_m3u(&self, url: &str) -> Result<Vec<NewIptvChannel>, IptvError> {
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(IptvError::HttpStatus(resp.status().as_u16()));
        }
        let text = resp.text().await?;
        Ok(m3u::parse_m3u(&text))
    }

    pub async fn fetch_xtream_live(
        &self,
        base_url: &str,
        username: &str,
        password: &str,
    ) -> Result<Vec<NewIptvChannel>, IptvError> {
        xtream::fetch_live_channels(&self.http, base_url, username, password).await
    }

    pub async fn fetch_xmltv(&self, url: &str) -> Result<Vec<IptvEpgRow>, IptvError> {
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(IptvError::HttpStatus(resp.status().as_u16()));
        }
        let text = resp.text().await?;
        Ok(xmltv::parse_xmltv(&text))
    }
}
