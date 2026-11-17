//! Optional direct OpenSubtitles REST API v1 client (requires user API key).

use serde::Deserialize;
use url::Url;

use crate::{SubtitleCandidate, SubtitleError, SubtitleQueryParams, normalize_lang_code};

const DEFAULT_BASE_URL: &str = "https://api.opensubtitles.com/api/v1";

#[derive(Clone)]
pub struct OpenSubtitlesClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    token: Option<String>,
}

impl OpenSubtitlesClient {
    pub fn new(api_key: String, token: Option<String>) -> Result<Self, SubtitleError> {
        Self::with_base_url(DEFAULT_BASE_URL.to_owned(), api_key, token)
    }

    pub fn with_base_url(
        base_url: String,
        api_key: String,
        token: Option<String>,
    ) -> Result<Self, SubtitleError> {
        let trimmed_key = api_key.trim().to_owned();
        if trimmed_key.is_empty() {
            return Err(SubtitleError::Invalid(
                "OpenSubtitles API key cannot be empty".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .user_agent("Cinma v0.1.0")
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: trimmed_key,
            token: token.filter(|t| !t.trim().is_empty()),
        })
    }

    pub async fn search(
        &self,
        params: &SubtitleQueryParams,
    ) -> Result<Vec<SubtitleCandidate>, SubtitleError> {
        let mut url = Url::parse(&format!("{}/subtitles", self.base_url))
            .map_err(|e| SubtitleError::Invalid(e.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(hash) = params.video_hash.as_deref().filter(|s| !s.is_empty()) {
                pairs.append_pair("moviehash", hash);
            }
            if let Some(imdb) = params.imdb_id.as_deref().filter(|s| !s.is_empty()) {
                let numeric = imdb.trim_start_matches("tt");
                if !numeric.is_empty() {
                    pairs.append_pair("imdb_id", numeric);
                }
            }
            if let Some(season) = params.season {
                pairs.append_pair("season_number", &season.to_string());
            }
            if let Some(ep) = params.episode {
                pairs.append_pair("episode_number", &ep.to_string());
            }
            if !params.languages.is_empty() {
                let langs: Vec<String> = params
                    .languages
                    .iter()
                    .filter_map(|l| to_opensubtitles_iso2(l))
                    .collect();
                if !langs.is_empty() {
                    pairs.append_pair("languages", &langs.join(","));
                }
            }
        }
        let resp = self
            .http
            .get(url)
            .header("Api-Key", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SubtitleError::HttpStatus(resp.status().as_u16()));
        }
        let body = resp.bytes().await?;
        let parsed: OsSearchResponse = serde_json::from_slice(&body)?;
        Ok(parsed
            .data
            .into_iter()
            .filter_map(|item| {
                let file = item.attributes.files.into_iter().next()?;
                let lang = normalize_lang_code(&item.attributes.language);
                let title = item
                    .attributes
                    .release
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| file.file_name.clone());
                Some(SubtitleCandidate {
                    id: format!("os:{}", file.file_id),
                    provider: "OpenSubtitles".to_owned(),
                    lang,
                    title,
                    download_url: format!("opensubtitles://file/{}", file.file_id),
                    opensubtitles_file_id: Some(file.file_id),
                    encoding_hint: None,
                    is_sdh: item.attributes.hearing_impaired.unwrap_or(false),
                    is_forced: item.attributes.foreign_parts_only.unwrap_or(false),
                    hash_matched: item.attributes.moviehash_match.unwrap_or(false),
                })
            })
            .collect())
    }

    /// Resolves a temporary download URL for an OpenSubtitles `file_id` and
    /// fetches its bytes.
    pub async fn download_file(&self, file_id: i64) -> Result<(Vec<u8>, String), SubtitleError> {
        let url = format!("{}/download", self.base_url);
        let payload = serde_json::to_vec(&serde_json::json!({ "file_id": file_id }))?;
        let mut req = self
            .http
            .post(url)
            .header("Api-Key", &self.api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(payload);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(SubtitleError::HttpStatus(resp.status().as_u16()));
        }
        let dl_bytes = resp.bytes().await?;
        let dl: OsDownloadResponse = serde_json::from_slice(&dl_bytes)?;
        let file_resp = self.http.get(&dl.link).send().await?;
        if !file_resp.status().is_success() {
            return Err(SubtitleError::HttpStatus(file_resp.status().as_u16()));
        }
        let ext = dl
            .file_name
            .as_deref()
            .and_then(|n| n.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()))
            .unwrap_or_else(|| "srt".to_owned());
        Ok((file_resp.bytes().await?.to_vec(), ext))
    }
}

fn to_opensubtitles_iso2(code: &str) -> Option<String> {
    match normalize_lang_code(code).as_str() {
        "tur" => Some("tr".to_owned()),
        "eng" => Some("en".to_owned()),
        "deu" => Some("de".to_owned()),
        "fra" => Some("fr".to_owned()),
        "spa" => Some("es".to_owned()),
        "ita" => Some("it".to_owned()),
        "por" => Some("pt-pt".to_owned()),
        "rus" => Some("ru".to_owned()),
        "jpn" => Some("ja".to_owned()),
        "kor" => Some("ko".to_owned()),
        "ara" => Some("ar".to_owned()),
        other if other.len() == 2 => Some(other.to_owned()),
        _ => None,
    }
}

#[derive(Deserialize)]
struct OsSearchResponse {
    #[serde(default)]
    data: Vec<OsSearchItem>,
}

#[derive(Deserialize)]
struct OsSearchItem {
    attributes: OsAttributes,
}

#[derive(Deserialize)]
struct OsAttributes {
    #[serde(default)]
    language: String,
    #[serde(default)]
    release: Option<String>,
    #[serde(default)]
    hearing_impaired: Option<bool>,
    #[serde(default)]
    foreign_parts_only: Option<bool>,
    #[serde(default)]
    moviehash_match: Option<bool>,
    #[serde(default)]
    files: Vec<OsFile>,
}

#[derive(Deserialize)]
struct OsFile {
    file_id: i64,
    #[serde(default)]
    file_name: Option<String>,
}

#[derive(Deserialize)]
struct OsDownloadResponse {
    link: String,
    #[serde(default)]
    file_name: Option<String>,
}
