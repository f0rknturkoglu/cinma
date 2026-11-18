use std::time::Duration;

use cinma_db::{IdMapEntry, MediaKind};
use cinma_stremio::{Meta, MetaPreview, MetaTrailer, MetaVideo};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MetadataError;

const DEFAULT_TMDB_BASE: &str = "https://api.themoviedb.org/3";
const TMDB_IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

#[derive(Debug, Clone, PartialEq)]
pub struct RichMeta {
    pub meta: Meta,
    pub id_map: IdMapEntry,
    pub season_numbers: Vec<u32>,
    pub next_episode: Option<UpcomingEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpcomingEpisode {
    pub show_id: String,
    pub show_name: String,
    pub show_poster: Option<String>,
    pub season: u32,
    pub episode: u32,
    pub episode_name: String,
    pub air_date: String,
    pub overview: Option<String>,
    pub still: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIds {
    pub tmdb_id: i64,
    pub media_type: String,
}

pub fn parse_tmdb_prefixed_id(id: &str) -> Option<i64> {
    id.strip_prefix("tmdb:")?.parse::<i64>().ok()
}

pub fn format_episode_id(base_id: &str, season: u32, episode: u32) -> String {
    format!("{base_id}:{season}:{episode}")
}

fn image_url(size: &str, path: Option<&str>) -> Option<String> {
    path.filter(|p| !p.is_empty())
        .map(|p| format!("{TMDB_IMAGE_BASE}/{size}{p}"))
}

#[derive(Clone)]
pub struct TmdbClient {
    http: reqwest::Client,
    base_url: Url,
}

impl TmdbClient {
    pub fn new() -> Result<Self, MetadataError> {
        Self::with_base_url(DEFAULT_TMDB_BASE)
    }

    pub fn with_base_url(base: &str) -> Result<Self, MetadataError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .build()?;
        let base_url = Url::parse(base.trim_end_matches('/'))?;
        Ok(Self { http, base_url })
    }

    fn endpoint(&self, segments: &[&str], params: &[(&str, &str)]) -> Result<Url, MetadataError> {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|()| url::ParseError::RelativeUrlWithoutBase)?
            .pop_if_empty()
            .extend(segments);
        {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, v);
            }
        }
        Ok(url)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: Url) -> Result<T, MetadataError> {
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(MetadataError::Status(status.as_u16()));
        }
        let bytes = resp.bytes().await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Validates an API key cheaply (`/configuration`).
    pub async fn verify_key(&self, api_key: &str) -> Result<(), MetadataError> {
        let url = self.endpoint(&["configuration"], &[("api_key", api_key)])?;
        let _: serde_json::Value = self.get_json(url).await?;
        Ok(())
    }

    /// Resolves an IMDb ID (`tt...`) to a TMDB ID and media type.
    pub async fn find_by_imdb(
        &self,
        api_key: &str,
        imdb_id: &str,
        language: &str,
    ) -> Result<Option<ResolvedIds>, MetadataError> {
        let url = self.endpoint(
            &["find", imdb_id],
            &[
                ("api_key", api_key),
                ("external_source", "imdb_id"),
                ("language", language),
            ],
        )?;
        let resp: TmdbFindResponse = self.get_json(url).await?;
        if let Some(m) = resp.movie_results.first() {
            return Ok(Some(ResolvedIds {
                tmdb_id: m.id,
                media_type: "movie".into(),
            }));
        }
        if let Some(t) = resp.tv_results.first() {
            return Ok(Some(ResolvedIds {
                tmdb_id: t.id,
                media_type: "series".into(),
            }));
        }
        Ok(None)
    }

    pub async fn trending(
        &self,
        api_key: &str,
        media_type: &str,
        language: &str,
    ) -> Result<Vec<MetaPreview>, MetadataError> {
        let tmdb_type = match media_type {
            "movie" => "movie",
            "series" | "tv" => "tv",
            _ => "all",
        };
        let url = self.endpoint(
            &["trending", tmdb_type, "week"],
            &[("api_key", api_key), ("language", language)],
        )?;
        let resp: TmdbListResponse = self.get_json(url).await?;
        Ok(resp
            .results
            .into_iter()
            .filter_map(|r| r.into_preview(media_type))
            .collect())
    }

    pub async fn search(
        &self,
        api_key: &str,
        query: &str,
        media_type: Option<&str>,
        language: &str,
    ) -> Result<Vec<MetaPreview>, MetadataError> {
        let kind = match media_type {
            Some("movie") => "movie",
            Some("series") | Some("tv") => "tv",
            _ => "multi",
        };
        let url = self.endpoint(
            &["search", kind],
            &[
                ("api_key", api_key),
                ("query", query),
                ("language", language),
                ("include_adult", "false"),
            ],
        )?;
        let resp: TmdbListResponse = self.get_json(url).await?;
        Ok(resp
            .results
            .into_iter()
            .filter_map(|r| {
                let fallback_type = match kind {
                    "movie" => "movie",
                    "tv" => "series",
                    _ => "movie",
                };
                r.into_preview(fallback_type)
            })
            .collect())
    }

    pub async fn movie_details(
        &self,
        api_key: &str,
        tmdb_id: i64,
        language: &str,
    ) -> Result<RichMeta, MetadataError> {
        self.movie_details_with_opts(api_key, tmdb_id, language, false, None)
            .await
    }

    pub async fn movie_details_with_opts(
        &self,
        api_key: &str,
        tmdb_id: i64,
        language: &str,
        prefer_original: bool,
        image_lang: Option<&str>,
    ) -> Result<RichMeta, MetadataError> {
        let id_str = tmdb_id.to_string();
        let (logo_lang, include_img_lang) = resolve_image_lang(language, image_lang);
        let url = self.endpoint(
            &["movie", &id_str],
            &[
                ("api_key", api_key),
                ("language", language),
                ("append_to_response", "external_ids,credits,videos,images"),
                ("include_image_language", &include_img_lang),
            ],
        )?;
        let raw: TmdbMovieDetails = self.get_json(url).await?;
        let imdb_id = raw.external_ids.as_ref().and_then(|e| e.imdb_id.clone());
        let canonical_id = imdb_id.clone().unwrap_or_else(|| format!("tmdb:{tmdb_id}"));

        let logo = pick_logo(raw.images.as_ref(), &logo_lang);
        let cast = raw
            .credits
            .as_ref()
            .map(|c| c.cast.iter().take(12).map(|m| m.name.clone()).collect())
            .unwrap_or_default();
        let director = raw
            .credits
            .as_ref()
            .map(|c| {
                c.crew
                    .iter()
                    .filter(|m| m.job.as_deref() == Some("Director"))
                    .map(|m| m.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let trailers = extract_trailers(raw.videos.as_ref());
        let release_info = raw
            .release_date
            .as_deref()
            .and_then(|d| d.get(..4))
            .map(str::to_owned);
        let name = if prefer_original {
            raw.original_title
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(raw.title)
        } else {
            raw.title
        };

        let meta = Meta {
            id: canonical_id,
            media_type: "movie".into(),
            name,
            poster: image_url("w500", raw.poster_path.as_deref()),
            poster_shape: Some("poster".into()),
            background: image_url("w1280", raw.backdrop_path.as_deref()),
            logo,
            description: raw.overview.filter(|s| !s.is_empty()),
            release_info,
            imdb_rating: raw
                .vote_average
                .filter(|v| *v > 0.0)
                .map(|v| format!("{v:.1}")),
            runtime: raw.runtime.filter(|r| *r > 0).map(|r| format!("{r} min")),
            country: None,
            awards: None,
            released: raw.release_date,
            genres: raw.genres.into_iter().map(|g| g.name).collect(),
            cast,
            director,
            writer: Vec::new(),
            videos: Vec::new(),
            trailers,
            links: Vec::new(),
            behavior_hints: None,
        };
        let id_map = IdMapEntry {
            media_kind: MediaKind::Movie,
            imdb_id,
            tmdb_id: Some(tmdb_id),
            tvdb_id: None,
            trakt_id: None,
            simkl_id: None,
            mal_id: None,
            anilist_id: None,
            kitsu_id: None,
            season: None,
            episode: None,
        };
        Ok(RichMeta {
            meta,
            id_map,
            season_numbers: Vec::new(),
            next_episode: None,
        })
    }

    pub async fn tv_details(
        &self,
        api_key: &str,
        tmdb_id: i64,
        language: &str,
    ) -> Result<RichMeta, MetadataError> {
        self.tv_details_with_opts(api_key, tmdb_id, language, false, None)
            .await
    }

    pub async fn tv_details_with_opts(
        &self,
        api_key: &str,
        tmdb_id: i64,
        language: &str,
        prefer_original: bool,
        image_lang: Option<&str>,
    ) -> Result<RichMeta, MetadataError> {
        let id_str = tmdb_id.to_string();
        let (logo_lang, include_img_lang) = resolve_image_lang(language, image_lang);
        let url = self.endpoint(
            &["tv", &id_str],
            &[
                ("api_key", api_key),
                ("language", language),
                ("append_to_response", "external_ids,credits,videos,images"),
                ("include_image_language", &include_img_lang),
            ],
        )?;
        let raw: TmdbTvDetails = self.get_json(url).await?;
        let imdb_id = raw.external_ids.as_ref().and_then(|e| e.imdb_id.clone());
        let tvdb_id = raw.external_ids.as_ref().and_then(|e| e.tvdb_id);
        let canonical_id = imdb_id.clone().unwrap_or_else(|| format!("tmdb:{tmdb_id}"));
        let poster = image_url("w500", raw.poster_path.as_deref());

        let logo = pick_logo(raw.images.as_ref(), &logo_lang);
        let cast = raw
            .credits
            .as_ref()
            .map(|c| c.cast.iter().take(12).map(|m| m.name.clone()).collect())
            .unwrap_or_default();
        let trailers = extract_trailers(raw.videos.as_ref());
        let year_start = raw.first_air_date.as_deref().and_then(|d| d.get(..4));
        let year_end = if raw.in_production == Some(true) {
            Some("")
        } else {
            raw.last_air_date.as_deref().and_then(|d| d.get(..4))
        };
        let release_info = match (year_start, year_end) {
            (Some(s), Some("")) => Some(format!("{s}–")),
            (Some(s), Some(e)) if s != e => Some(format!("{s}–{e}")),
            (Some(s), _) => Some(s.to_owned()),
            _ => None,
        };
        let season_numbers: Vec<u32> = raw
            .seasons
            .iter()
            .map(|s| s.season_number)
            .filter(|n| *n > 0)
            .collect();

        let show_name = if prefer_original {
            raw.original_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(raw.name)
        } else {
            raw.name
        };

        let next_episode = raw.next_episode_to_air.map(|ep| UpcomingEpisode {
            show_id: canonical_id.clone(),
            show_name: show_name.clone(),
            show_poster: poster.clone(),
            season: ep.season_number,
            episode: ep.episode_number,
            episode_name: ep.name,
            air_date: ep.air_date.unwrap_or_default(),
            overview: ep.overview.filter(|s| !s.is_empty()),
            still: image_url("w500", ep.still_path.as_deref()),
        });

        let runtime = raw
            .episode_run_time
            .first()
            .copied()
            .filter(|r| *r > 0)
            .map(|r| format!("{r} min"));

        let meta = Meta {
            id: canonical_id,
            media_type: "series".into(),
            name: show_name,
            poster,
            poster_shape: Some("poster".into()),
            background: image_url("w1280", raw.backdrop_path.as_deref()),
            logo,
            description: raw.overview.filter(|s| !s.is_empty()),
            release_info,
            imdb_rating: raw
                .vote_average
                .filter(|v| *v > 0.0)
                .map(|v| format!("{v:.1}")),
            runtime,
            country: None,
            awards: None,
            released: raw.first_air_date,
            genres: raw.genres.into_iter().map(|g| g.name).collect(),
            cast,
            director: Vec::new(),
            writer: Vec::new(),
            videos: Vec::new(),
            trailers,
            links: Vec::new(),
            behavior_hints: None,
        };
        let id_map = IdMapEntry {
            media_kind: MediaKind::Series,
            imdb_id,
            tmdb_id: Some(tmdb_id),
            tvdb_id,
            trakt_id: None,
            simkl_id: None,
            mal_id: None,
            anilist_id: None,
            kitsu_id: None,
            season: None,
            episode: None,
        };
        Ok(RichMeta {
            meta,
            id_map,
            season_numbers,
            next_episode,
        })
    }

    pub async fn tv_season(
        &self,
        api_key: &str,
        tmdb_id: i64,
        base_stream_id: &str,
        season: u32,
        language: &str,
    ) -> Result<Vec<MetaVideo>, MetadataError> {
        let id_str = tmdb_id.to_string();
        let season_str = season.to_string();
        let url = self.endpoint(
            &["tv", &id_str, "season", &season_str],
            &[("api_key", api_key), ("language", language)],
        )?;
        let raw: TmdbSeasonDetails = self.get_json(url).await?;
        Ok(raw
            .episodes
            .into_iter()
            .map(|ep| MetaVideo {
                id: format_episode_id(base_stream_id, ep.season_number, ep.episode_number),
                title: Some(ep.name),
                name: None,
                season: Some(ep.season_number),
                episode: Some(ep.episode_number),
                number: Some(ep.episode_number),
                released: ep.air_date,
                overview: ep.overview.filter(|s| !s.is_empty()),
                thumbnail: image_url("w500", ep.still_path.as_deref()),
            })
            .collect())
    }

    /// Fetches TMDB watch providers for a title in `region` (e.g. `"US"`, `"TR"`).
    pub async fn watch_providers(
        &self,
        api_key: &str,
        media_type: &str,
        tmdb_id: i64,
        region: &str,
    ) -> Result<Vec<WatchProvider>, MetadataError> {
        let kind = if media_type == "series" || media_type == "tv" {
            "tv"
        } else {
            "movie"
        };
        let id_str = tmdb_id.to_string();
        let url = self.endpoint(
            &[kind, &id_str, "watch", "providers"],
            &[("api_key", api_key)],
        )?;
        let raw: TmdbWatchProvidersResp = self.get_json(url).await?;
        let reg_key = region.trim().to_ascii_uppercase();
        let entry = raw.results.get(&reg_key).or_else(|| raw.results.get("US"));
        let Some(entry) = entry else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (kind_label, list) in [
            ("flatrate", &entry.flatrate),
            ("rent", &entry.rent),
            ("buy", &entry.buy),
        ] {
            for p in list {
                out.push(WatchProvider {
                    provider_id: p.provider_id,
                    name: p.provider_name.clone(),
                    logo_url: image_url("w185", p.logo_path.as_deref()),
                    kind: kind_label.to_owned(),
                });
            }
        }
        Ok(out)
    }

    /// Discovers popular movies or series available on a TMDB watch provider in `region`.
    #[allow(clippy::too_many_arguments)]
    pub async fn discover_by_provider(
        &self,
        api_key: &str,
        media_type: &str,
        provider_id: u32,
        region: &str,
        language: &str,
        page: u32,
        prefer_original: bool,
    ) -> Result<Vec<MetaPreview>, MetadataError> {
        let kind = if media_type == "series" || media_type == "tv" {
            "tv"
        } else {
            "movie"
        };
        let fallback_type = if kind == "tv" { "series" } else { "movie" };
        let pid = provider_id.to_string();
        let page_str = page.max(1).to_string();
        let url = self.endpoint(
            &["discover", kind],
            &[
                ("api_key", api_key),
                ("language", language),
                ("watch_region", region),
                ("with_watch_providers", &pid),
                ("sort_by", "popularity.desc"),
                ("page", &page_str),
                ("include_adult", "false"),
            ],
        )?;
        let resp: TmdbListResponse = self.get_json(url).await?;
        Ok(resp
            .results
            .into_iter()
            .filter_map(|r| r.into_preview_with_opts(fallback_type, prefer_original))
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchProvider {
    pub provider_id: i64,
    pub name: String,
    pub logo_url: Option<String>,
    pub kind: String,
}

fn resolve_image_lang(language: &str, image_lang: Option<&str>) -> (String, String) {
    let lang_short = language.split('-').next().unwrap_or("en");
    let pref = image_lang
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "auto");
    match pref {
        Some("null") => ("null".to_owned(), "null,en".to_owned()),
        Some(custom) => (custom.to_owned(), format!("{custom},en,null")),
        None => (lang_short.to_owned(), format!("{lang_short},en,null")),
    }
}

fn pick_logo(images: Option<&TmdbImages>, lang_short: &str) -> Option<String> {
    let logos = &images?.logos;
    let preferred = if lang_short == "null" {
        logos
            .iter()
            .find(|l| l.iso_639_1.is_none() && !l.file_path.ends_with(".svg"))
    } else {
        logos
            .iter()
            .find(|l| l.iso_639_1.as_deref() == Some(lang_short) && !l.file_path.ends_with(".svg"))
    };
    preferred
        .or_else(|| {
            logos
                .iter()
                .find(|l| l.iso_639_1.as_deref() == Some("en") && !l.file_path.ends_with(".svg"))
        })
        .or_else(|| logos.iter().find(|l| !l.file_path.ends_with(".svg")))
        .and_then(|l| image_url("w500", Some(&l.file_path)))
}

fn extract_trailers(videos: Option<&TmdbVideos>) -> Vec<MetaTrailer> {
    let Some(videos) = videos else {
        return Vec::new();
    };
    videos
        .results
        .iter()
        .filter(|v| {
            v.site.eq_ignore_ascii_case("YouTube")
                && matches!(v.video_type.as_deref(), Some("Trailer") | Some("Teaser"))
        })
        .map(|v| MetaTrailer {
            source: v.key.clone(),
            trailer_type: v.video_type.clone(),
        })
        .collect()
}

// ---- Serde DTOs for TMDB v3 JSON ----

#[derive(Deserialize)]
struct TmdbFindResponse {
    #[serde(default)]
    movie_results: Vec<TmdbIdOnly>,
    #[serde(default)]
    tv_results: Vec<TmdbIdOnly>,
}

#[derive(Deserialize)]
struct TmdbIdOnly {
    id: i64,
}

#[derive(Deserialize)]
struct TmdbListResponse {
    #[serde(default)]
    results: Vec<TmdbListItem>,
}

#[derive(Deserialize)]
struct TmdbListItem {
    id: i64,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    original_name: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    vote_average: Option<f64>,
}

impl TmdbListItem {
    fn into_preview(self, fallback_type: &str) -> Option<MetaPreview> {
        self.into_preview_with_opts(fallback_type, false)
    }

    fn into_preview_with_opts(
        self,
        fallback_type: &str,
        prefer_original: bool,
    ) -> Option<MetaPreview> {
        let media_type = match self.media_type.as_deref().unwrap_or(fallback_type) {
            "movie" => "movie",
            "tv" | "series" => "series",
            _ => return None,
        };
        let name = if prefer_original {
            self.original_title
                .filter(|s| !s.trim().is_empty())
                .or_else(|| self.original_name.filter(|s| !s.trim().is_empty()))
                .or(self.title)
                .or(self.name)?
        } else {
            self.title.or(self.name)?
        };
        let release_info = self
            .release_date
            .or(self.first_air_date)
            .and_then(|d| d.get(..4).map(str::to_owned));
        Some(MetaPreview {
            id: format!("tmdb:{}", self.id),
            media_type: media_type.into(),
            name,
            poster: image_url("w500", self.poster_path.as_deref()),
            poster_shape: Some("poster".into()),
            background: image_url("w1280", self.backdrop_path.as_deref()),
            logo: None,
            description: self.overview.filter(|s| !s.is_empty()),
            release_info,
            imdb_rating: self
                .vote_average
                .filter(|v| *v > 0.0)
                .map(|v| format!("{v:.1}")),
            genres: Vec::new(),
            runtime: None,
        })
    }
}

#[derive(Deserialize)]
struct TmdbGenre {
    name: String,
}

#[derive(Deserialize)]
struct TmdbExternalIds {
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    tvdb_id: Option<i64>,
}

#[derive(Deserialize)]
struct TmdbCredits {
    #[serde(default)]
    cast: Vec<TmdbCastMember>,
    #[serde(default)]
    crew: Vec<TmdbCrewMember>,
}

#[derive(Deserialize)]
struct TmdbCastMember {
    name: String,
}

#[derive(Deserialize)]
struct TmdbCrewMember {
    name: String,
    #[serde(default)]
    job: Option<String>,
}

#[derive(Deserialize)]
struct TmdbVideos {
    #[serde(default)]
    results: Vec<TmdbVideoItem>,
}

#[derive(Deserialize)]
struct TmdbVideoItem {
    key: String,
    site: String,
    #[serde(rename = "type", default)]
    video_type: Option<String>,
}

#[derive(Deserialize)]
struct TmdbImages {
    #[serde(default)]
    logos: Vec<TmdbImageItem>,
}

#[derive(Deserialize)]
struct TmdbImageItem {
    file_path: String,
    #[serde(default)]
    iso_639_1: Option<String>,
}

#[derive(Deserialize)]
struct TmdbMovieDetails {
    title: String,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
    #[serde(default)]
    vote_average: Option<f64>,
    #[serde(default)]
    genres: Vec<TmdbGenre>,
    #[serde(default)]
    external_ids: Option<TmdbExternalIds>,
    #[serde(default)]
    credits: Option<TmdbCredits>,
    #[serde(default)]
    videos: Option<TmdbVideos>,
    #[serde(default)]
    images: Option<TmdbImages>,
}

#[derive(Deserialize)]
struct TmdbSeasonSummary {
    season_number: u32,
}

#[derive(Deserialize)]
struct TmdbNextEpisode {
    season_number: u32,
    episode_number: u32,
    name: String,
    #[serde(default)]
    air_date: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    still_path: Option<String>,
}

#[derive(Deserialize)]
struct TmdbTvDetails {
    name: String,
    #[serde(default)]
    original_name: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    last_air_date: Option<String>,
    #[serde(default)]
    in_production: Option<bool>,
    #[serde(default)]
    episode_run_time: Vec<u32>,
    #[serde(default)]
    vote_average: Option<f64>,
    #[serde(default)]
    genres: Vec<TmdbGenre>,
    #[serde(default)]
    seasons: Vec<TmdbSeasonSummary>,
    #[serde(default)]
    next_episode_to_air: Option<TmdbNextEpisode>,
    #[serde(default)]
    external_ids: Option<TmdbExternalIds>,
    #[serde(default)]
    credits: Option<TmdbCredits>,
    #[serde(default)]
    videos: Option<TmdbVideos>,
    #[serde(default)]
    images: Option<TmdbImages>,
}

#[derive(Deserialize)]
struct TmdbSeasonDetails {
    #[serde(default)]
    episodes: Vec<TmdbEpisodeItem>,
}

#[derive(Deserialize)]
struct TmdbEpisodeItem {
    season_number: u32,
    episode_number: u32,
    name: String,
    #[serde(default)]
    air_date: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    still_path: Option<String>,
}

#[derive(Deserialize)]
struct TmdbWatchProvidersResp {
    #[serde(default)]
    results: std::collections::HashMap<String, TmdbWatchRegion>,
}

#[derive(Deserialize)]
struct TmdbWatchRegion {
    #[serde(default)]
    flatrate: Vec<TmdbWatchItem>,
    #[serde(default)]
    rent: Vec<TmdbWatchItem>,
    #[serde(default)]
    buy: Vec<TmdbWatchItem>,
}

#[derive(Deserialize)]
struct TmdbWatchItem {
    provider_id: i64,
    provider_name: String,
    #[serde(default)]
    logo_path: Option<String>,
}
