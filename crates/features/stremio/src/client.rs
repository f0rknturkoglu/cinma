use std::time::Duration;

use url::Url;

use crate::{
    AddonError, AddonSubtitle, CatalogQuery, CatalogResponse, Manifest, Meta, Stream,
    SubtitleQuery, parse_catalog, parse_meta, parse_streams, parse_subtitles,
};

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
        return Err(AddonError::InvalidUrl(format!(
            "{trimmed}: must end in manifest.json"
        )));
    };
    let mut base = manifest.clone();
    base.set_path(base_path.trim_end_matches('/'));
    base.set_query(None);
    Ok((manifest, base))
}

fn resource_url(base: &Url, resource: &str, media_type: &str, id: &str) -> Result<Url, AddonError> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(base.to_string()))?
        .pop_if_empty()
        .extend([resource, media_type, &format!("{id}.json")]);
    Ok(url)
}

/// Builds `{base}/stream/{type}/{id}.json` (path segments percent-encoded).
pub fn stream_url(base: &Url, media_type: &str, id: &str) -> Result<Url, AddonError> {
    resource_url(base, "stream", media_type, id)
}

/// Builds `{base}/meta/{type}/{id}.json` (path segments percent-encoded).
pub fn meta_url(base: &Url, media_type: &str, id: &str) -> Result<Url, AddonError> {
    resource_url(base, "meta", media_type, id)
}

/// Builds `{base}/catalog/{type}/{id}.json` or
/// `{base}/catalog/{type}/{id}/{extra}.json`.
pub fn catalog_url(
    base: &Url,
    media_type: &str,
    catalog_id: &str,
    query: &CatalogQuery,
) -> Result<Url, AddonError> {
    if query.is_empty() {
        return resource_url(base, "catalog", media_type, catalog_id);
    }
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(base.to_string()))?
        .pop_if_empty()
        .extend(["catalog", media_type, catalog_id]);
    let extra = query.encode_extra();
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/{extra}.json"));
    Ok(url)
}

/// Builds `{base}/subtitles/{type}/{id}.json` or
/// `{base}/subtitles/{type}/{id}/{extra}.json` (`videoHash`, `videoSize`, `filename`).
pub fn subtitles_url(
    base: &Url,
    media_type: &str,
    id: &str,
    query: &SubtitleQuery,
) -> Result<Url, AddonError> {
    if query.is_empty() {
        return resource_url(base, "subtitles", media_type, id);
    }
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| AddonError::InvalidUrl(base.to_string()))?
        .pop_if_empty()
        .extend(["subtitles", media_type, id]);
    let extra = query.encode_extra();
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/{extra}.json"));
    Ok(url)
}

#[derive(Clone)]
pub struct AddonClient {
    http: reqwest::Client,
}

impl AddonClient {
    pub fn new() -> Result<Self, AddonError> {
        Self::with_timeout(15)
    }

    pub fn with_timeout(timeout_secs: u64) -> Result<Self, AddonError> {
        let secs = timeout_secs.clamp(3, 60);
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(secs.min(10)))
            .timeout(Duration::from_secs(secs))
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

    pub async fn fetch_catalog(
        &self,
        transport_url: &str,
        media_type: &str,
        catalog_id: &str,
        query: &CatalogQuery,
    ) -> Result<CatalogResponse, AddonError> {
        let (_, base) = normalize_manifest_url(transport_url)?;
        parse_catalog(
            &self
                .get(catalog_url(&base, media_type, catalog_id, query)?)
                .await?,
        )
    }

    pub async fn fetch_meta(
        &self,
        transport_url: &str,
        media_type: &str,
        id: &str,
    ) -> Result<Option<Meta>, AddonError> {
        let (_, base) = normalize_manifest_url(transport_url)?;
        parse_meta(&self.get(meta_url(&base, media_type, id)?).await?)
    }

    pub async fn fetch_subtitles(
        &self,
        transport_url: &str,
        media_type: &str,
        id: &str,
        query: &SubtitleQuery,
    ) -> Result<Vec<AddonSubtitle>, AddonError> {
        let (_, base) = normalize_manifest_url(transport_url)?;
        parse_subtitles(
            &self
                .get(subtitles_url(&base, media_type, id, query)?)
                .await?,
        )
    }
}
