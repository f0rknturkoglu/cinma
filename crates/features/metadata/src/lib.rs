//! Metadata provider (TMDB primary with Cinemeta fallback handled by core)
//! and cross-provider ID mapping (§5.13).

#![forbid(unsafe_code)]

mod tmdb;

use serde::{Deserialize, Serialize};

pub use tmdb::{
    ResolvedIds, RichMeta, TmdbClient, UpcomingEpisode, format_episode_id, parse_tmdb_prefixed_id,
};

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("TMDB API returned HTTP {0}")]
    Status(u16),
    #[error("invalid TMDB response: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("metadata not found for {0}")]
    NotFound(String),
}

/// Serialized payload stored in `meta_cache`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedRichMeta {
    pub meta: cinma_stremio::Meta,
    pub season_numbers: Vec<u32>,
    pub next_episode: Option<UpcomingEpisode>,
}
