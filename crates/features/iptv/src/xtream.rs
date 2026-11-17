//! Xtream Codes JSON client (`player_api.php`).

use std::collections::HashMap;

use cinma_db::NewIptvChannel;
use serde::Deserialize;
use url::form_urlencoded;

use crate::IptvError;

pub async fn fetch_live_channels(
    http: &reqwest::Client,
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<Vec<NewIptvChannel>, IptvError> {
    let base = base_url.trim_end_matches('/');
    if base.is_empty() || username.trim().is_empty() {
        return Err(IptvError::Invalid(
            "Xtream URL and username are required".into(),
        ));
    }

    let cats_q = form_urlencoded::Serializer::new(String::new())
        .append_pair("username", username)
        .append_pair("password", password)
        .append_pair("action", "get_live_categories")
        .finish();
    let mut cat_map = HashMap::new();
    if let Ok(resp) = http
        .get(format!("{base}/player_api.php?{cats_q}"))
        .send()
        .await
        && resp.status().is_success()
        && let Ok(bytes) = resp.bytes().await
        && let Ok(cats) = serde_json::from_slice::<Vec<XtreamCategory>>(&bytes)
    {
        for c in cats {
            cat_map.insert(c.category_id, c.category_name);
        }
    }

    let streams_q = form_urlencoded::Serializer::new(String::new())
        .append_pair("username", username)
        .append_pair("password", password)
        .append_pair("action", "get_live_streams")
        .finish();
    let resp = http
        .get(format!("{base}/player_api.php?{streams_q}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(IptvError::HttpStatus(resp.status().as_u16()));
    }
    let streams: Vec<XtreamLiveStream> = serde_json::from_slice(&resp.bytes().await?)?;
    Ok(streams
        .into_iter()
        .map(|s| {
            let group_title = s
                .category_id
                .as_deref()
                .and_then(|cid| cat_map.get(cid).cloned());
            let sid = s.stream_id.to_string();
            let stream_url = format!("{base}/live/{username}/{password}/{sid}.ts");
            NewIptvChannel {
                stream_id: format!("xtream:{sid}"),
                name: s.name,
                group_title,
                logo_url: s.stream_icon.filter(|u| !u.is_empty()),
                stream_url,
                epg_channel_id: s.epg_channel_id.filter(|e| !e.is_empty()),
            }
        })
        .collect())
}

#[derive(Deserialize)]
struct XtreamCategory {
    category_id: String,
    category_name: String,
}

#[derive(Deserialize)]
struct XtreamLiveStream {
    stream_id: i64,
    name: String,
    #[serde(default)]
    stream_icon: Option<String>,
    #[serde(default)]
    epg_channel_id: Option<String>,
    #[serde(default)]
    category_id: Option<String>,
}
