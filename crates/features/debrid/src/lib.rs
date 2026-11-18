//! Native Debrid resolvers (Real-Debrid, AllDebrid, Premiumize, TorBox).
//! Constructed lazily only when `Feature::Debrid` is enabled (`CLAUDE.md` §1.5).

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::time::Duration;

use serde::Deserialize;
use url::form_urlencoded;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "wmv",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebridProvider {
    RealDebrid,
    AllDebrid,
    Premiumize,
    TorBox,
    EasyDebrid,
    DebridLink,
}

impl DebridProvider {
    pub const ALL: [Self; 6] = [
        Self::RealDebrid,
        Self::AllDebrid,
        Self::Premiumize,
        Self::TorBox,
        Self::EasyDebrid,
        Self::DebridLink,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::RealDebrid => "realdebrid",
            Self::AllDebrid => "alldebrid",
            Self::Premiumize => "premiumize",
            Self::TorBox => "torbox",
            Self::EasyDebrid => "easydebrid",
            Self::DebridLink => "debridlink",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::RealDebrid => "Real-Debrid",
            Self::AllDebrid => "AllDebrid",
            Self::Premiumize => "Premiumize",
            Self::TorBox => "TorBox",
            Self::EasyDebrid => "EasyDebrid",
            Self::DebridLink => "Debrid-Link",
        }
    }

    pub fn from_id(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "realdebrid" | "real-debrid" | "rd" => Some(Self::RealDebrid),
            "alldebrid" | "all-debrid" | "ad" => Some(Self::AllDebrid),
            "premiumize" | "pm" => Some(Self::Premiumize),
            "torbox" | "tb" => Some(Self::TorBox),
            "easydebrid" | "easy-debrid" | "ed" => Some(Self::EasyDebrid),
            "debridlink" | "debrid-link" | "dl" => Some(Self::DebridLink),
            _ => None,
        }
    }

    fn default_base(self) -> &'static str {
        match self {
            Self::RealDebrid => "https://api.real-debrid.com/rest/1.0",
            Self::AllDebrid => "https://api.alldebrid.com/v4",
            Self::Premiumize => "https://www.premiumize.me/api",
            Self::TorBox => "https://api.torbox.app/v1/api",
            Self::EasyDebrid => "https://easydebrid.com/api/v1",
            Self::DebridLink => "https://debrid-link.com/api/v2",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DebridError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{provider} returned HTTP {status}: {detail}")]
    HttpStatus {
        provider: &'static str,
        status: u16,
        detail: String,
    },
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct DebridResolvedStream {
    pub provider: DebridProvider,
    pub url: String,
    pub filename: Option<String>,
}

#[derive(Clone)]
pub struct DebridClient {
    http: reqwest::Client,
    rd_base: String,
    ad_base: String,
    pm_base: String,
    tb_base: String,
    ed_base: String,
    dl_base: String,
}

impl DebridClient {
    pub fn new() -> Result<Self, DebridError> {
        let http = reqwest::Client::builder()
            .user_agent("Cinma/0.1.0")
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http,
            rd_base: DebridProvider::RealDebrid.default_base().to_owned(),
            ad_base: DebridProvider::AllDebrid.default_base().to_owned(),
            pm_base: DebridProvider::Premiumize.default_base().to_owned(),
            tb_base: DebridProvider::TorBox.default_base().to_owned(),
            ed_base: DebridProvider::EasyDebrid.default_base().to_owned(),
            dl_base: DebridProvider::DebridLink.default_base().to_owned(),
        })
    }

    pub fn with_base_urls(
        rd_base: String,
        ad_base: String,
        pm_base: String,
        tb_base: String,
    ) -> Result<Self, DebridError> {
        let mut c = Self::new()?;
        c.rd_base = rd_base.trim_end_matches('/').to_owned();
        c.ad_base = ad_base.trim_end_matches('/').to_owned();
        c.pm_base = pm_base.trim_end_matches('/').to_owned();
        c.tb_base = tb_base.trim_end_matches('/').to_owned();
        Ok(c)
    }

    pub fn with_extra_base_urls(mut self, ed_base: String, dl_base: String) -> Self {
        self.ed_base = ed_base.trim_end_matches('/').to_owned();
        self.dl_base = dl_base.trim_end_matches('/').to_owned();
        self
    }

    fn base_for(&self, provider: DebridProvider) -> &str {
        match provider {
            DebridProvider::RealDebrid => &self.rd_base,
            DebridProvider::AllDebrid => &self.ad_base,
            DebridProvider::Premiumize => &self.pm_base,
            DebridProvider::TorBox => &self.tb_base,
            DebridProvider::EasyDebrid => &self.ed_base,
            DebridProvider::DebridLink => &self.dl_base,
        }
    }

    /// Checks which lowercase info hashes are cached on the given provider.
    pub async fn check_instant_cache(
        &self,
        provider: DebridProvider,
        api_key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let key = api_key.trim();
        if key.is_empty() || hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let clean: Vec<String> = hashes
            .iter()
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        if clean.is_empty() {
            return Ok(HashSet::new());
        }

        let mut cached = HashSet::new();
        for chunk in clean.chunks(40) {
            let hits = match provider {
                DebridProvider::RealDebrid => self.rd_check_cache(key, chunk).await?,
                DebridProvider::AllDebrid => self.ad_check_cache(key, chunk).await?,
                DebridProvider::Premiumize => self.pm_check_cache(key, chunk).await?,
                DebridProvider::TorBox => self.tb_check_cache(key, chunk).await?,
                DebridProvider::EasyDebrid => self.ed_check_cache(key, chunk).await?,
                DebridProvider::DebridLink => self.dl_check_cache(key, chunk).await?,
            };
            cached.extend(hits);
        }
        Ok(cached)
    }

    /// Resolves an info hash (and optional file index) into a direct HTTPS URL.
    pub async fn resolve_torrent(
        &self,
        provider: DebridProvider,
        api_key: &str,
        info_hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let key = api_key.trim();
        let hash = info_hash.trim().to_ascii_lowercase();
        if key.is_empty() || hash.is_empty() {
            return Err(DebridError::Invalid(
                "missing API key or info hash".to_owned(),
            ));
        }
        match provider {
            DebridProvider::RealDebrid => self.rd_resolve(key, &hash, file_idx).await,
            DebridProvider::AllDebrid => self.ad_resolve(key, &hash, file_idx).await,
            DebridProvider::Premiumize => self.pm_resolve(key, &hash, file_idx).await,
            DebridProvider::TorBox => self.tb_resolve(key, &hash, file_idx).await,
            DebridProvider::EasyDebrid => self.ed_resolve(key, &hash, file_idx).await,
            DebridProvider::DebridLink => self.dl_resolve(key, &hash, file_idx).await,
        }
    }

    // ---- Real-Debrid ----

    async fn rd_check_cache(
        &self,
        key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let joined = hashes.join("/");
        let url = format!("{}/torrents/instantAvailability/{joined}", self.rd_base);
        let resp = self.http.get(&url).bearer_auth(key).send().await?;
        let bytes = read_ok(DebridProvider::RealDebrid, resp).await?;
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(&bytes)?;
        let mut cached = HashSet::new();
        for (h, val) in map {
            let has_variants = val
                .as_object()
                .and_then(|o| o.get("rd"))
                .and_then(|r| r.as_array())
                .is_some_and(|arr| !arr.is_empty());
            if has_variants {
                cached.insert(h.to_ascii_lowercase());
            }
        }
        Ok(cached)
    }

    async fn rd_resolve(
        &self,
        key: &str,
        hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let magnet = format!("magnet:?xt=urn:btih:{hash}");
        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("magnet", &magnet)
            .finish();
        let add_resp = self
            .http
            .post(format!("{}/torrents/addMagnet", self.rd_base))
            .bearer_auth(key)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let add: RdAddResponse =
            serde_json::from_slice(&read_ok(DebridProvider::RealDebrid, add_resp).await?)?;

        let info_resp = self
            .http
            .get(format!("{}/torrents/info/{}", self.rd_base, add.id))
            .bearer_auth(key)
            .send()
            .await?;
        let info: RdTorrentInfo =
            serde_json::from_slice(&read_ok(DebridProvider::RealDebrid, info_resp).await?)?;

        let selected = pick_rd_file(&info.files, file_idx).ok_or_else(|| {
            DebridError::Invalid("no playable file found inside Real-Debrid torrent".into())
        })?;
        let select_body = form_urlencoded::Serializer::new(String::new())
            .append_pair("files", &selected.id.to_string())
            .finish();
        let sel_resp = self
            .http
            .post(format!("{}/torrents/selectFiles/{}", self.rd_base, add.id))
            .bearer_auth(key)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(select_body)
            .send()
            .await?;
        let _ = read_ok(DebridProvider::RealDebrid, sel_resp).await?;

        let info2_resp = self
            .http
            .get(format!("{}/torrents/info/{}", self.rd_base, add.id))
            .bearer_auth(key)
            .send()
            .await?;
        let info2: RdTorrentInfo =
            serde_json::from_slice(&read_ok(DebridProvider::RealDebrid, info2_resp).await?)?;
        let link = info2.links.into_iter().next().ok_or_else(|| {
            DebridError::Invalid("Real-Debrid torrent is not yet cached/ready".into())
        })?;

        let unrestrict_body = form_urlencoded::Serializer::new(String::new())
            .append_pair("link", &link)
            .finish();
        let un_resp = self
            .http
            .post(format!("{}/unrestrict/link", self.rd_base))
            .bearer_auth(key)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(unrestrict_body)
            .send()
            .await?;
        let un: RdUnrestrict =
            serde_json::from_slice(&read_ok(DebridProvider::RealDebrid, un_resp).await?)?;
        Ok(DebridResolvedStream {
            provider: DebridProvider::RealDebrid,
            url: un.download,
            filename: un.filename.or(Some(selected.path)),
        })
    }

    // ---- AllDebrid ----

    async fn ad_check_cache(
        &self,
        key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let url = {
            let mut ser = form_urlencoded::Serializer::new(String::new());
            ser.append_pair("agent", "Cinma");
            ser.append_pair("apikey", key);
            for h in hashes {
                ser.append_pair("magnets[]", h);
            }
            format!("{}/magnet/instant?{}", self.ad_base, ser.finish())
        };
        let resp = self.http.get(url).send().await?;
        let parsed: AdEnvelope<AdInstantData> =
            serde_json::from_slice(&read_ok(DebridProvider::AllDebrid, resp).await?)?;
        let mut cached = HashSet::new();
        for item in parsed.data.map(|d| d.magnets).unwrap_or_default() {
            if item.instant {
                cached.insert(item.hash.to_ascii_lowercase());
            }
        }
        Ok(cached)
    }

    async fn ad_resolve(
        &self,
        key: &str,
        hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let upload_url = {
            let magnet = format!("magnet:?xt=urn:btih:{hash}");
            let mut ser = form_urlencoded::Serializer::new(String::new());
            ser.append_pair("agent", "Cinma");
            ser.append_pair("apikey", key);
            ser.append_pair("magnets[]", &magnet);
            format!("{}/magnet/upload?{}", self.ad_base, ser.finish())
        };
        let upload_resp = self.http.get(upload_url).send().await?;
        let upload: AdEnvelope<AdUploadData> =
            serde_json::from_slice(&read_ok(DebridProvider::AllDebrid, upload_resp).await?)?;
        let uploaded = upload
            .data
            .and_then(|d| d.magnets.into_iter().next())
            .ok_or_else(|| DebridError::Invalid("AllDebrid returned no magnet entry".into()))?;

        let status_url = format!(
            "{}/magnet/status?agent=Cinma&apikey={}&id={}",
            self.ad_base, key, uploaded.id
        );
        let st_resp = self.http.get(status_url).send().await?;
        let st: AdEnvelope<AdStatusData> =
            serde_json::from_slice(&read_ok(DebridProvider::AllDebrid, st_resp).await?)?;
        let links = st.data.map(|d| d.magnets.links).unwrap_or_default();
        let picked = pick_named_link(&links, file_idx)
            .ok_or_else(|| DebridError::Invalid("AllDebrid link not ready".into()))?;

        let unlock_url = {
            let mut unlock_ser = form_urlencoded::Serializer::new(String::new());
            unlock_ser.append_pair("agent", "Cinma");
            unlock_ser.append_pair("apikey", key);
            unlock_ser.append_pair("link", &picked.link);
            format!("{}/link/unlock?{}", self.ad_base, unlock_ser.finish())
        };
        let un_resp = self.http.get(unlock_url).send().await?;
        let unlocked: AdEnvelope<AdUnlockData> =
            serde_json::from_slice(&read_ok(DebridProvider::AllDebrid, un_resp).await?)?;
        let data = unlocked
            .data
            .ok_or_else(|| DebridError::Invalid("AllDebrid unlock returned no data".into()))?;
        Ok(DebridResolvedStream {
            provider: DebridProvider::AllDebrid,
            url: data.link,
            filename: data.filename.or(picked.filename),
        })
    }

    // ---- Premiumize ----

    async fn pm_check_cache(
        &self,
        key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let url = {
            let mut ser = form_urlencoded::Serializer::new(String::new());
            ser.append_pair("apikey", key);
            for h in hashes {
                ser.append_pair("items[]", h);
            }
            format!("{}/cache/check?{}", self.pm_base, ser.finish())
        };
        let resp = self.http.get(url).send().await?;
        let parsed: PmCacheResp =
            serde_json::from_slice(&read_ok(DebridProvider::Premiumize, resp).await?)?;
        let mut out = HashSet::new();
        for (h, ok) in hashes.iter().zip(parsed.response) {
            if ok {
                out.insert(h.clone());
            }
        }
        Ok(out)
    }

    async fn pm_resolve(
        &self,
        key: &str,
        hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let magnet = format!("magnet:?xt=urn:btih:{hash}");
        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("apikey", key)
            .append_pair("src", &magnet)
            .finish();
        let resp = self
            .http
            .post(format!("{}/transfer/directdl", self.pm_base))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let parsed: PmDirectDlResp =
            serde_json::from_slice(&read_ok(DebridProvider::Premiumize, resp).await?)?;
        let item = pick_pm_item(&parsed.content, file_idx)
            .ok_or_else(|| DebridError::Invalid("Premiumize returned no cached content".into()))?;
        Ok(DebridResolvedStream {
            provider: DebridProvider::Premiumize,
            url: item.link,
            filename: Some(item.path),
        })
    }

    // ---- TorBox ----

    async fn tb_check_cache(
        &self,
        key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let joined = hashes.join(",");
        let url = format!(
            "{}/torrents/checkcached?hash={joined}&format=list",
            self.tb_base
        );
        let resp = self.http.get(url).bearer_auth(key).send().await?;
        let parsed: TbEnvelope<Vec<TbCachedItem>> =
            serde_json::from_slice(&read_ok(DebridProvider::TorBox, resp).await?)?;
        let mut out = HashSet::new();
        for item in parsed.data.unwrap_or_default() {
            if !item.hash.is_empty() {
                out.insert(item.hash.to_ascii_lowercase());
            }
        }
        Ok(out)
    }

    async fn tb_resolve(
        &self,
        key: &str,
        hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let magnet = format!("magnet:?xt=urn:btih:{hash}");
        let body = serde_json::json!({ "magnet": magnet });
        let create_resp = self
            .http
            .post(format!("{}/torrents/createtorrent", self.tb_base))
            .bearer_auth(key)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let created: TbEnvelope<TbCreateData> =
            serde_json::from_slice(&read_ok(DebridProvider::TorBox, create_resp).await?)?;
        let torrent_id = created
            .data
            .map(|d| d.torrent_id)
            .ok_or_else(|| DebridError::Invalid("TorBox did not return torrent_id".into()))?;

        let list_resp = self
            .http
            .get(format!("{}/torrents/mylist?id={torrent_id}", self.tb_base))
            .bearer_auth(key)
            .send()
            .await?;
        let info: TbEnvelope<TbTorrentInfo> =
            serde_json::from_slice(&read_ok(DebridProvider::TorBox, list_resp).await?)?;
        let files = info.data.map(|d| d.files).unwrap_or_default();
        let file = pick_tb_file(&files, file_idx)
            .ok_or_else(|| DebridError::Invalid("TorBox torrent has no playable file".into()))?;

        let dl_resp = self
            .http
            .get(format!(
                "{}/torrents/requestdl?token={key}&torrent_id={torrent_id}&file_id={}",
                self.tb_base, file.id
            ))
            .bearer_auth(key)
            .send()
            .await?;
        let dl: TbEnvelope<String> =
            serde_json::from_slice(&read_ok(DebridProvider::TorBox, dl_resp).await?)?;
        let url = dl
            .data
            .ok_or_else(|| DebridError::Invalid("TorBox requestdl returned no URL".into()))?;
        Ok(DebridResolvedStream {
            provider: DebridProvider::TorBox,
            url,
            filename: Some(file.name),
        })
    }

    // ---- EasyDebrid ----

    async fn ed_check_cache(
        &self,
        key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let magnets: Vec<String> = hashes
            .iter()
            .map(|h| format!("magnet:?xt=urn:btih:{h}"))
            .collect();
        let body = serde_json::json!({ "urls": magnets });
        let resp = self
            .http
            .post(format!("{}/link/lookup", self.ed_base))
            .bearer_auth(key)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let parsed: EdLookupResp =
            serde_json::from_slice(&read_ok(DebridProvider::EasyDebrid, resp).await?)?;
        let mut out = HashSet::new();
        for (h, ok) in hashes.iter().zip(parsed.cached) {
            if ok {
                out.insert(h.to_ascii_lowercase());
            }
        }
        Ok(out)
    }

    async fn ed_resolve(
        &self,
        key: &str,
        hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let body = serde_json::json!({ "url": format!("magnet:?xt=urn:btih:{hash}") });
        let resp = self
            .http
            .post(format!("{}/link/generate", self.ed_base))
            .bearer_auth(key)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;
        let parsed: EdGenerateResp =
            serde_json::from_slice(&read_ok(DebridProvider::EasyDebrid, resp).await?)?;
        if let Some(chosen) = pick_ed_file(&parsed.files, file_idx) {
            return Ok(DebridResolvedStream {
                provider: DebridProvider::EasyDebrid,
                url: chosen.url,
                filename: chosen.filename,
            });
        }
        let url = parsed
            .url
            .filter(|s| !s.is_empty())
            .ok_or_else(|| DebridError::Invalid("EasyDebrid returned no playable file".into()))?;
        Ok(DebridResolvedStream {
            provider: DebridProvider::EasyDebrid,
            url,
            filename: parsed.filename,
        })
    }

    // ---- Debrid-Link ----

    async fn dl_check_cache(
        &self,
        key: &str,
        hashes: &[String],
    ) -> Result<HashSet<String>, DebridError> {
        let joined = hashes.join(",");
        let resp = self
            .http
            .get(format!("{}/seedbox/cached?url={joined}", self.dl_base))
            .bearer_auth(key)
            .send()
            .await?;
        let parsed: DlCachedResp =
            serde_json::from_slice(&read_ok(DebridProvider::DebridLink, resp).await?)?;
        Ok(parsed
            .value
            .into_keys()
            .map(|k| k.to_ascii_lowercase())
            .collect())
    }

    async fn dl_resolve(
        &self,
        key: &str,
        hash: &str,
        file_idx: Option<usize>,
    ) -> Result<DebridResolvedStream, DebridError> {
        let magnet = format!("magnet:?xt=urn:btih:{hash}");
        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("url", &magnet)
            .append_pair("async", "true")
            .finish();
        let resp = self
            .http
            .post(format!("{}/seedbox/add", self.dl_base))
            .bearer_auth(key)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let parsed: DlAddResp =
            serde_json::from_slice(&read_ok(DebridProvider::DebridLink, resp).await?)?;
        let files = parsed.value.map(|v| v.files).unwrap_or_default();
        let file = pick_dl_file(&files, file_idx)
            .ok_or_else(|| DebridError::Invalid("Debrid-Link returned no playable file".into()))?;
        let url = file.download_url.filter(|s| !s.is_empty()).ok_or_else(|| {
            DebridError::Invalid("Debrid-Link file has no downloadUrl yet".into())
        })?;
        Ok(DebridResolvedStream {
            provider: DebridProvider::DebridLink,
            url,
            filename: Some(file.name),
        })
    }

    /// Reads the configured base URL for logging/diagnostics without exposing API keys.
    pub fn endpoint_for(&self, provider: DebridProvider) -> &str {
        self.base_for(provider)
    }
}

async fn read_ok(
    provider: DebridProvider,
    resp: reqwest::Response,
) -> Result<Vec<u8>, DebridError> {
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(DebridError::HttpStatus {
            provider: provider.display_name(),
            status: status.as_u16(),
            detail: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Ok(bytes.to_vec())
}

fn is_video(name: &str) -> bool {
    let Some(ext) = name.rsplit('.').next() else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    VIDEO_EXTENSIONS.iter().any(|e| *e == lower)
}

#[derive(Deserialize)]
struct RdAddResponse {
    id: String,
}

#[derive(Deserialize)]
struct RdTorrentInfo {
    #[serde(default)]
    files: Vec<RdFile>,
    #[serde(default)]
    links: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct RdFile {
    id: i64,
    path: String,
    bytes: u64,
}

#[derive(Deserialize)]
struct RdUnrestrict {
    download: String,
    filename: Option<String>,
}

fn pick_rd_file(files: &[RdFile], idx: Option<usize>) -> Option<RdFile> {
    if let Some(i) = idx
        && let Some(f) = files.get(i)
    {
        return Some(f.clone());
    }
    files
        .iter()
        .filter(|f| is_video(&f.path))
        .max_by_key(|f| f.bytes)
        .cloned()
        .or_else(|| files.iter().max_by_key(|f| f.bytes).cloned())
}

#[derive(Deserialize)]
struct AdEnvelope<T> {
    data: Option<T>,
}

#[derive(Deserialize)]
struct AdInstantData {
    #[serde(default)]
    magnets: Vec<AdInstantItem>,
}

#[derive(Deserialize)]
struct AdInstantItem {
    hash: String,
    #[serde(default)]
    instant: bool,
}

#[derive(Deserialize)]
struct AdUploadData {
    #[serde(default)]
    magnets: Vec<AdUploadItem>,
}

#[derive(Deserialize)]
struct AdUploadItem {
    id: i64,
}

#[derive(Deserialize)]
struct AdStatusData {
    magnets: AdStatusMagnet,
}

#[derive(Deserialize)]
struct AdStatusMagnet {
    #[serde(default)]
    links: Vec<AdLinkItem>,
}

#[derive(Clone, Deserialize)]
struct AdLinkItem {
    link: String,
    filename: Option<String>,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct AdUnlockData {
    link: String,
    filename: Option<String>,
}

fn pick_named_link(links: &[AdLinkItem], idx: Option<usize>) -> Option<AdLinkItem> {
    if let Some(i) = idx
        && let Some(l) = links.get(i)
    {
        return Some(l.clone());
    }
    links
        .iter()
        .filter(|l| l.filename.as_deref().is_some_and(is_video))
        .max_by_key(|l| l.size)
        .cloned()
        .or_else(|| links.iter().max_by_key(|l| l.size).cloned())
}

#[derive(Deserialize)]
struct PmCacheResp {
    #[serde(default)]
    response: Vec<bool>,
}

#[derive(Deserialize)]
struct PmDirectDlResp {
    #[serde(default)]
    content: Vec<PmContentItem>,
}

#[derive(Clone, Deserialize)]
struct PmContentItem {
    path: String,
    link: String,
    #[serde(default)]
    size: u64,
}

fn pick_pm_item(items: &[PmContentItem], idx: Option<usize>) -> Option<PmContentItem> {
    if let Some(i) = idx
        && let Some(it) = items.get(i)
    {
        return Some(it.clone());
    }
    items
        .iter()
        .filter(|it| is_video(&it.path))
        .max_by_key(|it| it.size)
        .cloned()
        .or_else(|| items.iter().max_by_key(|it| it.size).cloned())
}

#[derive(Deserialize)]
struct TbEnvelope<T> {
    data: Option<T>,
}

#[derive(Deserialize)]
struct TbCachedItem {
    #[serde(default)]
    hash: String,
}

#[derive(Deserialize)]
struct TbCreateData {
    torrent_id: i64,
}

#[derive(Deserialize)]
struct TbTorrentInfo {
    #[serde(default)]
    files: Vec<TbFile>,
}

#[derive(Clone, Deserialize)]
struct TbFile {
    id: i64,
    name: String,
    #[serde(default)]
    size: u64,
}

fn pick_tb_file(files: &[TbFile], idx: Option<usize>) -> Option<TbFile> {
    if let Some(i) = idx
        && let Some(f) = files.get(i)
    {
        return Some(f.clone());
    }
    files
        .iter()
        .filter(|f| is_video(&f.name))
        .max_by_key(|f| f.size)
        .cloned()
        .or_else(|| files.iter().max_by_key(|f| f.size).cloned())
}

#[derive(Deserialize)]
struct EdLookupResp {
    #[serde(default)]
    cached: Vec<bool>,
}

#[derive(Deserialize)]
struct EdGenerateResp {
    url: Option<String>,
    filename: Option<String>,
    #[serde(default)]
    files: Vec<EdFileItem>,
}

#[derive(Clone, Deserialize)]
struct EdFileItem {
    url: String,
    filename: Option<String>,
    #[serde(default)]
    size: u64,
}

fn pick_ed_file(files: &[EdFileItem], idx: Option<usize>) -> Option<EdFileItem> {
    if let Some(i) = idx
        && let Some(f) = files.get(i)
    {
        return Some(f.clone());
    }
    files
        .iter()
        .filter(|f| f.filename.as_deref().is_some_and(is_video))
        .max_by_key(|f| f.size)
        .cloned()
        .or_else(|| files.iter().max_by_key(|f| f.size).cloned())
}

#[derive(Deserialize)]
struct DlCachedResp {
    #[serde(default)]
    value: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct DlAddResp {
    value: Option<DlTorrentValue>,
}

#[derive(Deserialize)]
struct DlTorrentValue {
    #[serde(default)]
    files: Vec<DlFileItem>,
}

#[derive(Clone, Deserialize)]
struct DlFileItem {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
}

fn pick_dl_file(files: &[DlFileItem], idx: Option<usize>) -> Option<DlFileItem> {
    if let Some(i) = idx
        && let Some(f) = files.get(i)
    {
        return Some(f.clone());
    }
    files
        .iter()
        .filter(|f| is_video(&f.name))
        .max_by_key(|f| f.size)
        .cloned()
        .or_else(|| files.iter().max_by_key(|f| f.size).cloned())
}
