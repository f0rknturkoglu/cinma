//! One-time Stremio account import (addons & library items); credentials are
//! never stored after importing.

use serde::{Deserialize, Serialize};

use crate::{TrackingError, check_status};

const DEFAULT_STREMIO_API: &str = "https://api.strem.io/api";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StremioLibraryEntry {
    pub id: String,
    pub media_type: String,
    pub name: String,
    pub poster: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StremioAccountSnapshot {
    pub addon_urls: Vec<String>,
    pub library: Vec<StremioLibraryEntry>,
}

pub async fn fetch_stremio_account_snapshot(
    api_base: Option<&str>,
    email: &str,
    password: &str,
) -> Result<StremioAccountSnapshot, TrackingError> {
    let base = api_base
        .unwrap_or(DEFAULT_STREMIO_API)
        .trim_end_matches('/');
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    let login_body = serde_json::json!({
        "type": "Login",
        "email": email.trim(),
        "password": password,
    });
    let login_resp = http
        .post(format!("{base}/login"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_vec(&login_body)?)
        .send()
        .await?;
    let login_bytes = check_status("Stremio", login_resp).await?;

    #[derive(Deserialize)]
    struct ApiWrap<T> {
        result: Option<T>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LoginRes {
        auth_key: String,
    }
    let auth_key = serde_json::from_slice::<ApiWrap<LoginRes>>(&login_bytes)?
        .result
        .ok_or_else(|| TrackingError::Invalid("Stremio login failed".into()))?
        .auth_key;

    let addons_req =
        serde_json::json!({ "type": "AddonCollectionGet", "authKey": auth_key, "update": true });
    let addons_resp = http
        .post(format!("{base}/addonCollectionGet"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_vec(&addons_req)?)
        .send()
        .await?;
    let addons_bytes = check_status("Stremio", addons_resp).await?;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AddonsRes {
        addons: Vec<StremioAddonEntry>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StremioAddonEntry {
        transport_url: String,
    }
    let addon_urls = serde_json::from_slice::<ApiWrap<AddonsRes>>(&addons_bytes)?
        .result
        .map(|r| r.addons.into_iter().map(|a| a.transport_url).collect())
        .unwrap_or_default();

    let lib_req = serde_json::json!({
        "type": "DatastoreGet",
        "authKey": auth_key,
        "collection": "libraryItem",
        "all": true
    });
    let lib_resp = http
        .post(format!("{base}/datastoreGet"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_vec(&lib_req)?)
        .send()
        .await?;
    let lib_bytes = check_status("Stremio", lib_resp).await?;

    #[derive(Deserialize)]
    struct RawLibItem {
        #[serde(rename = "_id")]
        id: String,
        #[serde(rename = "type")]
        media_type: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        poster: Option<String>,
        #[serde(default)]
        removed: bool,
    }
    let raw_items: Vec<RawLibItem> =
        serde_json::from_slice::<ApiWrap<Vec<RawLibItem>>>(&lib_bytes)?
            .result
            .unwrap_or_default();
    let library = raw_items
        .into_iter()
        .filter(|i| !i.removed && !i.name.trim().is_empty())
        .map(|i| StremioLibraryEntry {
            id: i.id,
            media_type: i.media_type,
            name: i.name,
            poster: i.poster,
        })
        .collect();

    Ok(StremioAccountSnapshot {
        addon_urls,
        library,
    })
}
