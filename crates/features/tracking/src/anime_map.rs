//! Compact anime ID mapping dataset parser & starter seed (for populating
//! `id_map` offline and refreshing from user-triggered community JSON datasets).

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AnimeIdRecord {
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<i64>,
    pub tvdb_id: Option<i64>,
    pub mal_id: Option<i64>,
    pub anilist_id: Option<i64>,
    pub kitsu_id: Option<i64>,
}

/// Starter bundled mappings for immediate offline lookup of classic series/films
/// before fetching full community mappings (e.g. Fribb/anime-lists).
pub const BUNDLED_STARTER_JSON: &str = r#"[
  {"imdb_id":"tt0245429","tmdb_id":129,"mal_id":199,"anilist_id":199,"kitsu_id":176},
  {"imdb_id":"tt0388629","tmdb_id":1429,"mal_id":16498,"anilist_id":16498,"kitsu_id":7442},
  {"imdb_id":"tt0988824","tmdb_id":31910,"mal_id":1735,"anilist_id":1735,"kitsu_id":1555},
  {"imdb_id":"tt0877057","tmdb_id":13916,"mal_id":1535,"anilist_id":1535,"kitsu_id":1376},
  {"imdb_id":"tt1355642","tmdb_id":31911,"mal_id":5114,"anilist_id":5114,"kitsu_id":3936}
]"#;

pub fn parse_anime_id_dataset(bytes: &[u8]) -> Result<Vec<AnimeIdRecord>, serde_json::Error> {
    serde_json::from_slice(bytes)
}
