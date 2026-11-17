//! Short-lived `yt-dlp -J` extraction for trailers (§5.7, docs/adr/0010).

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::{Core, CoreError};

const YTDLP_MACOS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtdlpStatus {
    pub available: bool,
    pub path: Option<String>,
    pub managed: bool,
}

#[derive(Deserialize)]
struct YtdlpInfo {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    formats: Vec<YtdlpFormat>,
}

#[derive(Deserialize)]
struct YtdlpFormat {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    vcodec: Option<String>,
    #[serde(default)]
    acodec: Option<String>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    protocol: Option<String>,
}

impl Core {
    fn managed_ytdlp_path(&self) -> PathBuf {
        self.data_dir.join("bin").join("yt-dlp")
    }

    pub fn ytdlp_status(&self) -> YtdlpStatus {
        if let Ok(env_path) = std::env::var("CINMA_YTDLP") {
            let p = PathBuf::from(env_path.trim());
            if p.is_file() {
                return YtdlpStatus {
                    available: true,
                    path: Some(p.to_string_lossy().into_owned()),
                    managed: false,
                };
            }
        }
        let managed = self.managed_ytdlp_path();
        if managed.is_file() {
            return YtdlpStatus {
                available: true,
                path: Some(managed.to_string_lossy().into_owned()),
                managed: true,
            };
        }
        for candidate in ["/opt/homebrew/bin/yt-dlp", "/usr/local/bin/yt-dlp"] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return YtdlpStatus {
                    available: true,
                    path: Some(candidate.to_owned()),
                    managed: false,
                };
            }
        }
        YtdlpStatus {
            available: false,
            path: None,
            managed: false,
        }
    }

    /// Runs a short-lived `yt-dlp -J` child process that exits once the
    /// stream URL is extracted (§1.1, §5.7).
    pub async fn extract_trailer_url(
        self: &Arc<Self>,
        source: String,
    ) -> Result<String, CoreError> {
        let status = self.ytdlp_status();
        let bin = status.path.ok_or_else(|| {
            CoreError::Invalid("yt-dlp is not installed (install via Settings or Homebrew)".into())
        })?;
        let target = if source.starts_with("http://") || source.starts_with("https://") {
            source
        } else {
            format!("https://www.youtube.com/watch?v={source}")
        };
        self.spawn(async move {
            let output = tokio::process::Command::new(&bin)
                .args([
                    "-J",
                    "--no-playlist",
                    "--no-warnings",
                    "-f",
                    "best[height<=1080][ext=mp4]/best[height<=1080]/best",
                    &target,
                ])
                .output()
                .await?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(CoreError::Invalid(format!("yt-dlp failed: {}", err.trim())));
            }
            let info: YtdlpInfo = serde_json::from_slice(&output.stdout)
                .map_err(|e| CoreError::Invalid(format!("yt-dlp JSON parse failed: {e}")))?;
            pick_best_stream_url(&info)
                .ok_or_else(|| CoreError::Invalid("no playable trailer format returned".into()))
        })
        .await
    }

    /// Downloads or updates `yt-dlp_macos` into `{data_dir}/bin/yt-dlp` ONLY
    /// when explicitly triggered by the user (§5.7).
    pub async fn install_or_update_ytdlp(self: &Arc<Self>) -> Result<String, CoreError> {
        let dest = self.managed_ytdlp_path();
        self.spawn(async move {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            let resp = client
                .get(YTDLP_MACOS_URL)
                .send()
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(CoreError::Invalid(format!(
                    "yt-dlp download failed: HTTP {}",
                    resp.status().as_u16()
                )));
            }
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            let tmp = dest.with_extension("tmp");
            std::fs::write(&tmp, &bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
            }
            std::fs::rename(&tmp, &dest)?;
            Ok(dest.to_string_lossy().into_owned())
        })
        .await
    }
}

fn pick_best_stream_url(info: &YtdlpInfo) -> Option<String> {
    if let Some(url) = info.url.as_deref().filter(|u| !u.is_empty()) {
        return Some(url.to_owned());
    }
    info.formats
        .iter()
        .filter(|f| {
            f.url.as_deref().is_some_and(|u| !u.is_empty())
                && f.vcodec.as_deref() != Some("none")
                && f.acodec.as_deref() != Some("none")
                && f.protocol.as_deref().is_none_or(|p| !p.contains("mhtml"))
        })
        .max_by_key(|f| {
            let h = f.height.unwrap_or(0).min(1080);
            let mp4_bonus = u32::from(f.ext.as_deref() == Some("mp4")) * 10_000;
            mp4_bonus + h
        })
        .and_then(|f| f.url.clone())
}
