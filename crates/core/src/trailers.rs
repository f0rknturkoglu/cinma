//! Short-lived `yt-dlp -J` extraction for trailers (§5.7, docs/adr/0010).

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::{Core, CoreError};

const YTDLP_MACOS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";
const YTDLP_SUMS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
const YTDLP_ASSET: &str = "yt-dlp_macos";

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
        let client = self.shared_http()?;
        self.spawn(async move {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let resp = client
                .get(YTDLP_MACOS_URL)
                .timeout(std::time::Duration::from_secs(120))
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
            // Verify against the release's published checksums before installing.
            let sums = client
                .get(YTDLP_SUMS_URL)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| CoreError::Invalid(format!("yt-dlp checksums: {e}")))?
                .text()
                .await
                .map_err(|e| CoreError::Invalid(format!("yt-dlp checksums: {e}")))?;
            verify_sha256(&bytes, &sums, YTDLP_ASSET)?;
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

/// Checks `bytes` against the `<hex>  <name>` line for `name` in a
/// `SHA2-256SUMS` file.
fn verify_sha256(bytes: &[u8], sums: &str, name: &str) -> Result<(), CoreError> {
    let expected = sums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let file = parts.next()?.trim_start_matches('*');
            (file == name).then(|| hash.to_ascii_lowercase())
        })
        .ok_or_else(|| CoreError::Invalid(format!("no checksum listed for {name}")))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let actual: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if actual == expected {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!(
            "{name} checksum mismatch; not installed (try again)"
        )))
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::verify_sha256;

    #[test]
    fn checksum_must_match_the_listed_asset() {
        // sha256("abc")
        let sums = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  yt-dlp_macos\n\
                    0000000000000000000000000000000000000000000000000000000000000000  yt-dlp.exe\n";
        assert!(verify_sha256(b"abc", sums, "yt-dlp_macos").is_ok());
        assert!(verify_sha256(b"abd", sums, "yt-dlp_macos").is_err());
        assert!(verify_sha256(b"abc", sums, "yt-dlp_linux").is_err());
    }
}
