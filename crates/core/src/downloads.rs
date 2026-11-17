//! Unified Download Manager (`crates/core/src/downloads.rs`, §5.10 / §5.11 / ADR 0012):
//! - Lazy `BlobStore` + `Queue` initialization (zero runtime/threads until used)
//! - HTTP/1.1 segmented engine (`cinma-http`)
//! - Torrent full-download engine (`cinma-torrent::TorrentDownloadEngine`)
//! - `yt-dlp -J` downloader + audio-only + in-process FFmpeg mux (`cinma-player::mux`)
//! - "Play Now" (`download_byte_source`) via `Blob::source()` while queued/active/complete

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cinma_blobstore::{BlobId, BlobStore, ByteSource};
use cinma_db::{DiskKind, DownloadKind, DownloadRow, DownloadState, NewDownload};
use cinma_http::{HttpDownloadEngine, HttpSourceSpec};
use cinma_queue::{BoxFuture, DownloadEngine, Job, Queue};
use cinma_torrent::{TorrentDownloadEngine, TorrentSourceSpec};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::{Core, CoreError, CoreEvent, Feature};

const MENUBAR_ENABLED_KEY: &str = "app.menubar_enabled";
const MAX_CONCURRENT_DOWNLOADS: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtdlpSourceSpec {
    pub url: String,
    #[serde(default)]
    pub max_height: Option<u32>,
    #[serde(default)]
    pub audio_only: bool,
    #[serde(default)]
    pub format_id: Option<String>,
    #[serde(default)]
    pub meta_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YtdlpProbeFormat {
    url: Option<String>,
    ext: Option<String>,
    #[serde(default)]
    http_headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct YtdlpProbeOutput {
    url: Option<String>,
    ext: Option<String>,
    #[serde(default)]
    requested_formats: Vec<YtdlpProbeFormat>,
    #[serde(default)]
    http_headers: HashMap<String, String>,
}

struct YtdlpDownloadEngine {
    ytdlp_managed_path: PathBuf,
    temp_dir: PathBuf,
    http: reqwest::Client,
}

impl YtdlpDownloadEngine {
    fn new(ytdlp_managed_path: PathBuf, temp_dir: PathBuf) -> Result<Self, CoreError> {
        let http = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        Ok(Self {
            ytdlp_managed_path,
            temp_dir,
            http,
        })
    }

    fn resolve_bin(&self) -> Option<PathBuf> {
        if self.ytdlp_managed_path.is_file() {
            return Some(self.ytdlp_managed_path.clone());
        }
        [
            "/opt/homebrew/bin/yt-dlp",
            "/usr/local/bin/yt-dlp",
            "/usr/bin/yt-dlp",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
    }
}

impl DownloadEngine for YtdlpDownloadEngine {
    fn run(&self, job: Job) -> BoxFuture<Result<(), String>> {
        let bin = self.resolve_bin();
        let temp_dir = self.temp_dir.clone();
        let client = self.http.clone();
        Box::pin(async move {
            let Some(bin) = bin else {
                return Err("yt-dlp is not installed (install via Settings)".to_owned());
            };
            let spec: YtdlpSourceSpec =
                serde_json::from_str(&job.download.source_json).map_err(|e| e.to_string())?;
            let format_expr = if let Some(fid) = &spec.format_id {
                fid.clone()
            } else if spec.audio_only {
                "bestaudio[ext=m4a]/bestaudio".to_owned()
            } else if let Some(h) = spec.max_height {
                format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]/best")
            } else {
                "bestvideo+bestaudio/best".to_owned()
            };

            let out = tokio::process::Command::new(&bin)
                .args([
                    "-J",
                    "--no-playlist",
                    "--no-warnings",
                    "-f",
                    &format_expr,
                    &spec.url,
                ])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                let msg = String::from_utf8_lossy(&out.stderr);
                return Err(format!("yt-dlp probe failed: {}", msg.trim()));
            }
            let probe: YtdlpProbeOutput =
                serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;

            tokio::fs::create_dir_all(&temp_dir)
                .await
                .map_err(|e| e.to_string())?;
            let job_dir = temp_dir.join(format!("job_{}", job.download.id));
            tokio::fs::create_dir_all(&job_dir)
                .await
                .map_err(|e| e.to_string())?;

            let result = async {
                let tracks: Vec<(String, String, HashMap<String, String>)> =
                    if probe.requested_formats.len() >= 2 {
                        probe
                            .requested_formats
                            .iter()
                            .filter_map(|f| {
                                f.url.clone().map(|u| {
                                    (
                                        u,
                                        f.ext.clone().unwrap_or_else(|| "bin".to_owned()),
                                        f.http_headers.clone(),
                                    )
                                })
                            })
                            .collect()
                    } else if let Some(u) = probe
                        .url
                        .clone()
                        .or_else(|| probe.requested_formats.first().and_then(|f| f.url.clone()))
                    {
                        let ext = probe.ext.clone().unwrap_or_else(|| {
                            if spec.audio_only { "m4a" } else { "mp4" }.to_owned()
                        });
                        vec![(u, ext, probe.http_headers.clone())]
                    } else {
                        return Err("yt-dlp returned no downloadable track URL".to_owned());
                    };

                let final_path = if tracks.len() == 1 {
                    let (url, ext, headers) = &tracks[0];
                    let single_path = job_dir.join(format!("media.{ext}"));
                    download_url_to_file(&client, url, headers, &single_path, &job.cancel).await?;
                    single_path
                } else {
                    let mut part_paths = Vec::with_capacity(tracks.len());
                    for (idx, (url, ext, headers)) in tracks.iter().enumerate() {
                        let part = job_dir.join(format!("track_{idx}.{ext}"));
                        download_url_to_file(&client, url, headers, &part, &job.cancel).await?;
                        part_paths.push(part);
                    }
                    let mux_out = job_dir.join("muxed.mp4");
                    let inputs = part_paths.clone();
                    let mux_dest = mux_out.clone();
                    tokio::task::spawn_blocking(move || {
                        let refs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
                        cinma_player::mux::remux_files(&refs, &mux_dest).map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|e| e.to_string())??;
                    mux_out
                };

                if job.cancel.is_cancelled() {
                    return Ok(());
                }

                let meta = tokio::fs::metadata(&final_path)
                    .await
                    .map_err(|e| e.to_string())?;
                let total_size = meta.len();
                let blob = job.blob(total_size).map_err(|e| e.to_string())?;
                copy_file_to_blob(&final_path, &blob, &job.cancel).await?;
                let _ = job.report_progress(&blob);
                Ok(())
            }
            .await;

            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            result
        })
    }
}

async fn download_url_to_file(
    client: &reqwest::Client,
    url: &str,
    headers: &HashMap<String, String>,
    dest: &Path,
    cancel: &cinma_blobstore::CancellationToken,
) -> Result<(), String> {
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let mut resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} downloading track", resp.status().as_u16()));
    }
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(chunk) = tokio::select! {
        _ = cancel.cancelled() => return Ok(()),
        res = resp.chunk() => res.map_err(|e| e.to_string())?,
    } {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn copy_file_to_blob(
    path: &Path,
    blob: &cinma_blobstore::Blob,
    cancel: &cinma_blobstore::CancellationToken,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut offset = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        blob.write_available(offset, &buf[..n])
            .map_err(|e| e.to_string())?;
        offset += n as u64;
    }
    Ok(())
}

/// Inspects a completed download row and ensures it is indexed by Disk Policy,
/// then enforces the user's disk quota if needed.
fn index_completed_download(core: &Core, id: i64) {
    let row = match core.db.lock() {
        Ok(db) => {
            let Ok(profiles) = db.profiles() else {
                return;
            };
            profiles
                .into_iter()
                .find_map(|p| db.download(p.id, id).ok().flatten())
        }
        Err(_) => return,
    };
    let Some(row) = row else {
        return;
    };
    if row.state != DownloadState::Completed {
        return;
    }
    let Some(blob_hex) = row.blob_id.as_deref() else {
        return;
    };
    let (meta_id, media_type) = extract_meta_from_source(&row.source_json);
    let bytes = row.bytes_total.unwrap_or(row.bytes_done);
    let blob_path = core.data_dir.join("blobs").join(format!("{blob_hex}.blob"));
    let _ = core.record_disk_entry(
        &format!("blob:{blob_hex}"),
        DiskKind::Blob,
        meta_id.as_deref(),
        media_type.as_deref(),
        &blob_path.to_string_lossy(),
        bytes,
        false,
    );
    let _ = core.enforce_disk_quota();
}

fn extract_meta_from_source(source_json: &str) -> (Option<String>, Option<String>) {
    #[derive(Deserialize)]
    struct MetaFields {
        meta_id: Option<String>,
        media_type: Option<String>,
    }
    serde_json::from_str::<MetaFields>(source_json)
        .map(|m| (m.meta_id, m.media_type))
        .unwrap_or((None, None))
}

impl Core {
    pub fn blob_store(&self) -> Result<Arc<BlobStore>, CoreError> {
        let mut slot = self.blob_store.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(store) = slot.as_ref() {
            return Ok(Arc::clone(store));
        }
        let store = Arc::new(BlobStore::open(&self.data_dir.join("blobs"))?);
        *slot = Some(Arc::clone(&store));
        Ok(store)
    }

    /// Lazily constructs the unified download queue and registers all download engines.
    pub fn queue(self: &Arc<Self>) -> Result<Arc<Queue>, CoreError> {
        {
            let guard = self.queue.lock().map_err(|_| CoreError::Poisoned)?;
            if let Some(q) = guard.as_ref() {
                return Ok(Arc::clone(q));
            }
        }

        let store = self.blob_store()?;
        let rt_handle = self.runtime()?.handle().clone();
        let q = Arc::new(
            Queue::new(
                Arc::clone(&self.db),
                store,
                rt_handle,
                MAX_CONCURRENT_DOWNLOADS,
            )
            .map_err(|e| CoreError::Invalid(e.to_string()))?,
        );

        // HTTP/1.1 segmented downloader
        let http_engine = Arc::new(
            HttpDownloadEngine::new(self.data_dir.join("blobs"), 4)
                .map_err(|e| CoreError::Invalid(e.to_string()))?,
        );
        q.register_engine(DownloadKind::Http, http_engine)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;

        // yt-dlp downloader (in-process FFmpeg muxing)
        let ytdlp_engine = Arc::new(YtdlpDownloadEngine::new(
            self.data_dir.join("bin").join("yt-dlp"),
            self.data_dir.join("tmp").join("ytdlp"),
        )?);
        q.register_engine(DownloadKind::Ytdlp, ytdlp_engine)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;

        // Torrent downloader (uses persistent fastresume + seeding policy)
        if self.feature_enabled(Feature::Torrent)? {
            let torrent_engine = self.torrents()?;
            let seeding = self.disk_policy_settings()?.torrent_policy(false).seeding;
            let dl_engine = Arc::new(TorrentDownloadEngine::new(torrent_engine, seeding));
            *self
                .torrent_download_engine
                .lock()
                .map_err(|_| CoreError::Poisoned)? = Some(Arc::clone(&dl_engine));
            q.register_engine(DownloadKind::Torrent, dl_engine)
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
        }

        let weak_core = Arc::downgrade(self);
        q.set_change_listener(Some(Arc::new(move |id| {
            if let Some(core) = weak_core.upgrade() {
                index_completed_download(&core, id);
                core.emit(CoreEvent::DownloadsChanged);
            }
        })))
        .map_err(|e| CoreError::Invalid(e.to_string()))?;

        let mut guard = self.queue.lock().map_err(|_| CoreError::Poisoned)?;
        if let Some(existing) = guard.as_ref() {
            return Ok(Arc::clone(existing));
        }
        *guard = Some(Arc::clone(&q));
        Ok(q)
    }

    pub fn add_http_download(
        self: &Arc<Self>,
        title: String,
        url: String,
        headers: HashMap<String, String>,
        meta_id: Option<String>,
        media_type: Option<String>,
        priority: i64,
    ) -> Result<i64, CoreError> {
        let profile = self.active_profile()?;
        let spec = HttpSourceSpec {
            url,
            headers,
            meta_id,
            media_type,
        };
        let source_json =
            serde_json::to_string(&spec).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let id = self
            .queue()?
            .add(&NewDownload {
                profile_id: profile,
                kind: DownloadKind::Http,
                title,
                source_json,
                priority,
            })
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_torrent_download(
        self: &Arc<Self>,
        title: String,
        info_hash: String,
        file_idx: Option<usize>,
        trackers: Vec<String>,
        meta_id: Option<String>,
        media_type: Option<String>,
        priority: i64,
    ) -> Result<i64, CoreError> {
        self.require(Feature::Torrent)?;
        let profile = self.active_profile()?;
        let spec = TorrentSourceSpec {
            info_hash,
            file_idx,
            trackers,
            meta_id,
            media_type,
        };
        let source_json =
            serde_json::to_string(&spec).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let id = self
            .queue()?
            .add(&NewDownload {
                profile_id: profile,
                kind: DownloadKind::Torrent,
                title,
                source_json,
                priority,
            })
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        Ok(id)
    }

    pub fn add_ytdlp_download(
        self: &Arc<Self>,
        title: String,
        url: String,
        max_height: Option<u32>,
        audio_only: bool,
        priority: i64,
    ) -> Result<i64, CoreError> {
        let profile = self.active_profile()?;
        let spec = YtdlpSourceSpec {
            url,
            max_height,
            audio_only,
            format_id: None,
            meta_id: None,
            media_type: None,
        };
        let source_json =
            serde_json::to_string(&spec).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let id = self
            .queue()?
            .add(&NewDownload {
                profile_id: profile,
                kind: DownloadKind::Ytdlp,
                title,
                source_json,
                priority,
            })
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        Ok(id)
    }

    /// Reads the active profile's downloads straight from SQLite without spinning up engines.
    pub fn downloads(&self) -> Result<Vec<DownloadRow>, CoreError> {
        let profile = self.active_profile()?;
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.downloads(profile)?)
    }

    pub fn pause_download(self: &Arc<Self>, id: i64) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.queue()?
            .pause(profile, id)
            .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    pub fn resume_download(self: &Arc<Self>, id: i64) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        self.queue()?
            .resume(profile, id)
            .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    pub fn remove_download(self: &Arc<Self>, id: i64, delete_data: bool) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        let blob_id = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.download(profile, id)?.and_then(|r| r.blob_id)
        };
        if let Some(q) = self
            .queue
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
            .cloned()
        {
            q.remove(profile, id, delete_data)
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
        } else {
            self.db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .delete_download(profile, id)?;
            if delete_data
                && let Some(hex) = &blob_id
                && let Ok(parsed) = BlobId::parse(hex)
            {
                let _ = self.blob_store()?.remove(&parsed);
            }
            self.emit(CoreEvent::DownloadsChanged);
        }
        if delete_data && let Some(hex) = blob_id {
            let _ = self
                .db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .delete_disk_entry(&format!("blob:{hex}"));
        }
        Ok(())
    }

    pub fn has_pending_downloads(&self) -> Result<bool, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.has_pending_downloads()?)
    }

    /// Returns a live `ByteSource` for an active, paused, or completed download ("Play Now").
    pub fn download_byte_source(&self, id: i64) -> Result<Arc<dyn ByteSource>, CoreError> {
        let profile = self.active_profile()?;
        let row = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .download(profile, id)?
            .ok_or_else(|| CoreError::Invalid(format!("download {id} not found")))?;
        let Some(blob_hex) = row.blob_id else {
            return Err(CoreError::Invalid(
                "download has not allocated its media buffer yet".into(),
            ));
        };
        let blob_id = BlobId::parse(&blob_hex)?;
        let blob = self.blob_store()?.open_blob(&blob_id)?;
        let _ = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .touch_disk_entry(&format!("blob:{blob_hex}"));
        Ok(blob.source())
    }

    pub fn menubar_enabled(&self) -> Result<bool, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(match db.get_app_setting(MENUBAR_ENABLED_KEY)?.as_deref() {
            Some("0") => false,
            Some("1") | None => true,
            Some(_) => true,
        })
    }

    pub fn set_menubar_enabled(&self, enabled: bool) -> Result<(), CoreError> {
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_app_setting(MENUBAR_ENABLED_KEY, if enabled { "1" } else { "0" })?;
        self.emit(CoreEvent::DownloadsChanged);
        Ok(())
    }
}
