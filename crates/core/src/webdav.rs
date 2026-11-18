//! Direct WebDAV network source client (#N24: Zurg, TorBox WebDAV, Nextcloud, NAS).
//! Uses HTTP/HTTPS `PROPFIND Depth: 1` with optional Basic Auth credentials in Keychain.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::keychain::{get_webdav_password, set_webdav_password};
use crate::{Core, CoreError};

const KEY_WEBDAV_SOURCES: &str = "webdav_sources";
const ALLOWED_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv", "srt", "ass", "ssa",
    "vtt",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavSource {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebDavEntry {
    pub name: String,
    pub sub_path: String,
    pub stream_url: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

impl Core {
    pub fn webdav_sources(&self) -> Result<Vec<WebDavSource>, CoreError> {
        self.load_profile_json(KEY_WEBDAV_SOURCES)
    }

    pub fn add_webdav_source(
        &self,
        name: String,
        base_url: String,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<WebDavSource, CoreError> {
        let trimmed_url = base_url.trim().trim_end_matches('/').to_owned();
        let parsed = reqwest::Url::parse(&trimmed_url)
            .map_err(|e| CoreError::Invalid(format!("invalid WebDAV URL: {e}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(CoreError::Invalid(
                "WebDAV URL must use http:// or https://".into(),
            ));
        }
        let clean_name = if name.trim().is_empty() {
            parsed.host_str().unwrap_or("WebDAV").to_owned()
        } else {
            name.trim().to_owned()
        };
        let clean_user = username
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        let id = format!(
            "wd_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        );
        if let Some(pw) = password.filter(|p| !p.is_empty()) {
            set_webdav_password(&id, Some(&pw))?;
        }
        let src = WebDavSource {
            id,
            name: clean_name,
            base_url: trimmed_url,
            username: clean_user,
        };
        let mut all = self.webdav_sources()?;
        all.push(src.clone());
        self.save_profile_json(KEY_WEBDAV_SOURCES, &all)?;
        Ok(src)
    }

    pub fn remove_webdav_source(&self, id: &str) -> Result<(), CoreError> {
        let mut all = self.webdav_sources()?;
        all.retain(|s| s.id != id);
        let _ = set_webdav_password(id, None);
        self.save_profile_json(KEY_WEBDAV_SOURCES, &all)
    }

    pub async fn browse_webdav(
        self: &Arc<Self>,
        source_id: String,
        sub_path: String,
    ) -> Result<Vec<WebDavEntry>, CoreError> {
        let src = self
            .webdav_sources()?
            .into_iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| CoreError::Invalid(format!("unknown WebDAV source: {source_id}")))?;
        let password = get_webdav_password(&src.id)?;
        let clean_sub = sub_path.trim().trim_start_matches('/').to_owned();
        let target_url = if clean_sub.is_empty() {
            format!("{}/", src.base_url)
        } else {
            format!("{}/{clean_sub}", src.base_url)
        };
        let base_parsed = reqwest::Url::parse(&src.base_url)
            .map_err(|e| CoreError::Invalid(format!("invalid base URL: {e}")))?;

        let xml_body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:resourcetype/>
    <D:getcontentlength/>
  </D:prop>
</D:propfind>"#;

        let username = src.username.clone();
        let pw_clone = password.clone();
        let method = reqwest::Method::from_bytes(b"PROPFIND")
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let url_clone = target_url.clone();
        let client = self.shared_http()?;
        let body_text = self
            .spawn(async move {
                let mut req = client
                    .request(method, &url_clone)
                    .timeout(Duration::from_secs(15))
                    .header("Depth", "1")
                    .header("Content-Type", "application/xml")
                    .body(xml_body);
                if let Some(ref u) = username {
                    req = req.basic_auth(u, pw_clone.as_deref());
                }
                let resp = req
                    .send()
                    .await
                    .map_err(|e| CoreError::Invalid(format!("WebDAV request failed: {e}")))?;
                if !resp.status().is_success() {
                    return Err(CoreError::Invalid(format!("WebDAV HTTP {}", resp.status())));
                }
                resp.text()
                    .await
                    .map_err(|e| CoreError::Invalid(format!("WebDAV read failed: {e}")))
            })
            .await?;

        Ok(parse_propfind_xml(
            &body_text,
            &base_parsed,
            &target_url,
            src.username.as_deref(),
            password.as_deref(),
        ))
    }
}

pub(crate) fn parse_propfind_xml(
    xml: &str,
    base_url: &reqwest::Url,
    requested_url: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Vec<WebDavEntry> {
    let req_norm = reqwest::Url::parse(requested_url)
        .map(|u| u.path().trim_end_matches('/').to_owned())
        .unwrap_or_default();
    let base_prefix = base_url.path().trim_end_matches('/');

    let mut out = Vec::new();
    for block in split_xml_blocks(xml, "response") {
        let Some(href_raw) = extract_xml_tag(block, "href") else {
            continue;
        };
        let Ok(resolved) = base_url.join(href_raw.trim()) else {
            continue;
        };
        let path_norm = resolved.path().trim_end_matches('/');
        if path_norm == req_norm {
            continue;
        }
        let is_dir = has_xml_tag(block, "collection") || href_raw.trim().ends_with('/');
        let name = path_norm
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .map(percent_decode)
            .unwrap_or_else(|| path_norm.to_owned());
        if name.starts_with('.') {
            continue;
        }
        if !is_dir {
            let allowed = name
                .rsplit_once('.')
                .is_some_and(|(_, ext)| ALLOWED_EXTS.contains(&ext.to_ascii_lowercase().as_str()));
            if !allowed {
                continue;
            }
        }
        let size_bytes = extract_xml_tag(block, "getcontentlength")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let sub_path = path_norm
            .strip_prefix(base_prefix)
            .unwrap_or(path_norm)
            .trim_start_matches('/')
            .to_owned();
        let mut play_url = resolved.clone();
        if let Some(u) = username {
            let _ = play_url.set_username(u);
            if let Some(p) = password {
                let _ = play_url.set_password(Some(p));
            }
        }
        out.push(WebDavEntry {
            name,
            sub_path,
            stream_url: play_url.to_string(),
            is_dir,
            size_bytes,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    out
}

fn split_xml_blocks<'a>(xml: &'a str, local_name: &str) -> Vec<&'a str> {
    let mut blocks = Vec::new();
    let lower = xml.to_ascii_lowercase();
    let needle_close = format!("{local_name}>");
    let mut pos = 0usize;
    while pos < lower.len() {
        let rest = &lower[pos..];
        let Some(open_rel) = find_opening_tag(rest, local_name) else {
            break;
        };
        let start = pos + open_rel;
        if let Some(close_rel) = lower[start..].find(&needle_close) {
            let candidate_end = start + close_rel + needle_close.len();
            // Advance past closing `</...:response>` (not opening `<...:response ...>`)
            if let Some(real_close) = find_closing_tag(&lower[start..], local_name) {
                let end = start + real_close;
                blocks.push(&xml[start..end]);
                pos = end;
                continue;
            }
            pos = candidate_end;
        } else {
            break;
        }
    }
    blocks
}

fn find_opening_tag(lower_haystack: &str, local_name: &str) -> Option<usize> {
    let mut idx = 0usize;
    while let Some(lt) = lower_haystack[idx..].find('<') {
        let abs = idx + lt;
        let after = &lower_haystack[abs + 1..];
        if !after.starts_with('/') {
            let tag_end = after
                .find(['>', ' ', '\t', '\r', '\n'])
                .unwrap_or(after.len());
            let tag_name = &after[..tag_end];
            let local = tag_name.rsplit(':').next().unwrap_or(tag_name);
            if local == local_name {
                return Some(abs);
            }
        }
        idx = abs + 1;
    }
    None
}

fn find_closing_tag(lower_haystack: &str, local_name: &str) -> Option<usize> {
    let mut idx = 0usize;
    while let Some(lt) = lower_haystack[idx..].find("</") {
        let abs = idx + lt;
        let after = &lower_haystack[abs + 2..];
        if let Some(gt) = after.find('>') {
            let tag_name = after[..gt].trim();
            let local = tag_name.rsplit(':').next().unwrap_or(tag_name);
            if local == local_name {
                return Some(abs + 2 + gt + 1);
            }
        }
        idx = abs + 2;
    }
    None
}

fn extract_xml_tag<'a>(xml: &'a str, local_name: &str) -> Option<&'a str> {
    let lower = xml.to_ascii_lowercase();
    let start_lt = find_opening_tag(&lower, local_name)?;
    let open_end = lower[start_lt..].find('>')? + start_lt + 1;
    let close_lt = lower[open_end..].find("</")? + open_end;
    let after = &lower[close_lt + 2..];
    let gt = after.find('>')?;
    let local = after[..gt].trim().rsplit(':').next().unwrap_or("");
    (local == local_name).then(|| xml[open_end..close_lt].trim())
}

fn has_xml_tag(xml: &str, local_name: &str) -> bool {
    find_opening_tag(&xml.to_ascii_lowercase(), local_name).is_some()
}

fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            bytes.push(v);
            i += 3;
            continue;
        }
        bytes.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
