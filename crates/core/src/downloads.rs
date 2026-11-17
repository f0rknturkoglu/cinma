//! Unified download manager (§5.10–5.12, ADR 0012, ADR 0016).
//! - Lazy `BlobStore` + `Queue` (no runtime or threads until used)
//! - HTTP/1.1 segmented engine (`cinma-http`); yt-dlp single tracks reuse it
//! - Torrent download engine (`cinma-torrent`), one copy on disk
//! - Finished files are moved (hard link, no copy) into the user's folders
//! - "Play Now" while downloading (blob or torrent), file playback when done

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cinma_blobstore::{
    BlobId, BlobStore, ByteSource, FileSource, move_file_no_overwrite, sanitize_file_name,
    unique_destination,
};
use cinma_db::{DownloadKind, DownloadRow, DownloadState, NewDownload};
use cinma_http::{HttpDownloadEngine, HttpSourceSpec};
use cinma_queue::{BoxFuture, DownloadEngine, Job, Queue};
use cinma_torrent::{
    FolderResolver, PARTIAL_DIR, TorrentDownloadEngine, TorrentSourceSpec, remove_staging,
    staging_dir,
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::{Core, CoreError, CoreEvent, Feature};

const MENUBAR_ENABLED_KEY: &str = "app.menubar_enabled";
const MAX_CONCURRENT_DOWNLOADS: usize = 2;

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// `~/Movies/Cinma`: default for movies, series and web video.
pub fn default_video_folder() -> PathBuf {
    home_dir().join("Movies").join("Cinma")
}

/// `~/Music/Cinma`: default for audio-only downloads.
pub fn default_audio_folder() -> PathBuf {
    home_dir().join("Music").join("Cinma")
}

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
    /// Single-track media goes through the segmented, resumable HTTP engine.
    http_engine: Arc<HttpDownloadEngine>,
    folder_for: FolderResolver,
    finalizer: Finalizer,
}

impl YtdlpDownloadEngine {
    fn new(
        ytdlp_managed_path: PathBuf,
        temp_dir: PathBuf,
        http_engine: Arc<HttpDownloadEngine>,
        folder_for: FolderResolver,
        finalizer: Finalizer,
    ) -> Result<Self, CoreError> {
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
            http_engine,
            folder_for,
            finalizer,
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
        let http_engine = Arc::clone(&self.http_engine);
        let folder = (self.folder_for)(&job.download);
        let finalize = self.finalizer.clone();
        Box::pin(async move {
            // Finished in a previous run.
            if let Some(done) = job.download.file_path.as_deref()
                && job.download.blob_id.is_none()
                && Path::new(done).is_file()
            {
                return Ok(());
            }
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
                .kill_on_drop(true)
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                let msg = String::from_utf8_lossy(&out.stderr);
                return Err(format!("yt-dlp probe failed: {}", msg.trim()));
            }
            let probe: YtdlpProbeOutput =
                serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;

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
                    let ext = probe
                        .ext
                        .clone()
                        .unwrap_or_else(|| if spec.audio_only { "m4a" } else { "mp4" }.to_owned());
                    vec![(u, ext, probe.http_headers.clone())]
                } else {
                    return Err("yt-dlp returned no downloadable track URL".to_owned());
                };
            let base_name = sanitize_file_name(&job.download.title);

            if let [(url, ext, headers)] = tracks.as_slice() {
                // One track: resumable, segmented, playable while downloading,
                // written once (blob), then moved into the folder without a copy.
                let http_spec = HttpSourceSpec {
                    url: url.clone(),
                    headers: headers.clone(),
                    meta_id: spec.meta_id.clone(),
                    media_type: spec.media_type.clone(),
                    url_may_change: true,
                    file_ext: Some(ext.clone()),
                };
                let json = serde_json::to_string(&http_spec).map_err(|e| e.to_string())?;
                let download_id = job.download.id;
                let cancel = job.cancel.clone();
                http_engine.run(job.with_source_json(json)).await?;
                if cancel.is_cancelled() {
                    return Ok(());
                }
                let dest = unique_destination(&folder, &with_ext(&base_name, ext));
                return tokio::task::spawn_blocking(move || finalize.finish(download_id, &dest))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string());
            }

            // Separate video + audio: download the parts, mux in-process into
            // a hidden file in the destination folder, then move it into place.
            let job_dir = temp_dir.join(format!("job_{}", job.download.id));
            tokio::fs::create_dir_all(&job_dir)
                .await
                .map_err(|e| e.to_string())?;
            let result = async {
                let mut part_paths = Vec::with_capacity(tracks.len());
                for (idx, (url, ext, headers)) in tracks.iter().enumerate() {
                    let part = job_dir.join(format!("track_{idx}.{ext}"));
                    download_url_to_file(&client, url, headers, &part, &job.cancel).await?;
                    if job.cancel.is_cancelled() {
                        return Ok(());
                    }
                    part_paths.push(part);
                }
                let staging = folder.join(PARTIAL_DIR);
                tokio::fs::create_dir_all(&staging)
                    .await
                    .map_err(|e| e.to_string())?;
                let muxed = staging.join(format!("ytdlp-{}.mp4", job.download.id));
                let _ = tokio::fs::remove_file(&muxed).await;
                let inputs = part_paths.clone();
                let mux_dest = muxed.clone();
                tokio::task::spawn_blocking(move || {
                    let refs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
                    cinma_player::mux::remux_files(&refs, &mux_dest).map_err(|e| e.to_string())
                })
                .await
                .map_err(|e| e.to_string())??;
                let dest = unique_destination(&folder, &with_ext(&base_name, "mp4"));
                job.set_file_path(&dest).map_err(|e| e.to_string())?;
                let (from, to) = (muxed.clone(), dest.clone());
                tokio::task::spawn_blocking(move || move_file_no_overwrite(&from, &to))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                let _ = std::fs::remove_dir(&staging);
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

const MEDIA_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "webm", "mov", "avi", "ts", "m2ts", "wmv", "mp3", "m4a", "aac", "flac",
    "opus", "ogg", "wav",
];

fn known_ext(candidate: &str) -> Option<String> {
    let ext = candidate.to_ascii_lowercase();
    MEDIA_EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// `name.ext` unless `name` already ends with a media extension.
fn with_ext(name: &str, ext: &str) -> String {
    let has = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .and_then(known_ext)
        .is_some();
    if has || ext.is_empty() {
        name.to_owned()
    } else {
        format!("{name}.{ext}")
    }
}

/// Container type from the first bytes of a file.
fn sniff_ext(path: &Path) -> Option<&'static str> {
    use std::io::Read;
    let mut head = [0u8; 12];
    let mut f = std::fs::File::open(path).ok()?;
    let n = f.read(&mut head).ok()?;
    let head = &head[..n];
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("mkv")
    } else if head.len() >= 8 && &head[4..8] == b"ftyp" {
        Some("mp4")
    } else if head.starts_with(b"ID3") {
        Some("mp3")
    } else if head.starts_with(b"fLaC") {
        Some("flac")
    } else if head.starts_with(b"OggS") {
        Some("ogg")
    } else if head.first() == Some(&0x47) {
        Some("ts")
    } else {
        None
    }
}

/// File name for a finished HTTP download: the title, with the extension
/// from the title, the source spec, the URL, or the file's own header.
fn http_file_name(row: &DownloadRow, data: &Path) -> String {
    let base = sanitize_file_name(&row.title);
    if Path::new(&base)
        .extension()
        .and_then(|e| e.to_str())
        .and_then(known_ext)
        .is_some()
    {
        return base;
    }
    let spec = serde_json::from_str::<HttpSourceSpec>(&row.source_json).ok();
    let ext = spec
        .as_ref()
        .and_then(|s| s.file_ext.as_deref().and_then(known_ext))
        .or_else(|| {
            // Last path segment of the URL, without query or fragment.
            let url = &spec.as_ref()?.url;
            let path = url.split(['?', '#']).next()?;
            let last = path.rsplit('/').next()?;
            Path::new(last)
                .extension()
                .and_then(|e| e.to_str())
                .and_then(known_ext)
        })
        .or_else(|| sniff_ext(data).map(str::to_owned))
        .unwrap_or_else(|| "bin".to_owned());
    with_ext(&base, &ext)
}

/// Moves a completed blob download into its folder (HTTP downloads, and
/// crash recovery for any blob that finished but was not moved).
fn finalize_completed_download(core: &Core, id: i64) {
    let row = match core.db.lock() {
        Ok(db) => db.download_any_profile(id).ok().flatten(),
        Err(_) => return,
    };
    let Some(row) = row else {
        return;
    };
    if row.state != DownloadState::Completed || row.blob_id.is_none() {
        return;
    }
    let Ok(store) = core.blob_store() else {
        return;
    };
    let Some(blob_hex) = row.blob_id.as_deref() else {
        return;
    };
    let Ok(blob_id) = BlobId::parse(blob_hex) else {
        return;
    };
    let folder = core.download_folder(&row);
    let dest = match row.file_path.as_deref() {
        // Recorded before a crash: finish that move.
        Some(p) => PathBuf::from(p),
        None => {
            let data = core.data_dir.join("blobs").join(format!("{blob_hex}.blob"));
            unique_destination(&folder, &http_file_name(&row, &data))
        }
    };
    let finalizer = Finalizer {
        db: Arc::clone(&core.db),
        store,
    };
    if let Err(e) = finalizer.finish_blob(id, &blob_id, &dest) {
        eprintln!(
            "[cinma] download {id}: could not move to {}: {e}",
            dest.display()
        );
    } else {
        core.emit(CoreEvent::DownloadsChanged);
    }
}

/// Moves a finished blob to its destination: record the path first, then
/// link/move, then forget the blob — each step safe to redo after a crash.
#[derive(Clone)]
pub(crate) struct Finalizer {
    db: Arc<std::sync::Mutex<cinma_db::Database>>,
    store: Arc<BlobStore>,
}

impl Finalizer {
    fn finish_blob(&self, id: i64, blob: &BlobId, dest: &Path) -> Result<(), String> {
        self.db
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .set_download_file_path(id, Some(&dest.to_string_lossy()))
            .map_err(|e| e.to_string())?;
        self.store
            .finalize_to(blob, dest)
            .map_err(|e| e.to_string())?;
        self.db
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .clear_download_blob(id)
            .map_err(|e| e.to_string())
    }

    /// For engines that finish inside `run` (yt-dlp single track).
    fn finish(&self, id: i64, dest: &Path) -> Result<(), String> {
        let blob = self
            .db
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .download_any_profile(id)
            .map_err(|e| e.to_string())?
            .and_then(|r| r.blob_id)
            .ok_or("download has no data")?;
        let blob = BlobId::parse(&blob).map_err(|e| e.to_string())?;
        self.finish_blob(id, &blob, dest)
    }
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

        // Where finished files go (user folders by media type).
        let weak = Arc::downgrade(self);
        let folder_for: FolderResolver = Arc::new(move |row: &DownloadRow| {
            weak.upgrade()
                .map(|core| core.download_folder(row))
                .unwrap_or_else(default_video_folder)
        });
        let finalizer = Finalizer {
            db: Arc::clone(&self.db),
            store: self.blob_store()?,
        };

        // The listener goes in first so no completion is missed; it moves
        // finished HTTP downloads into their folder off the async workers.
        let weak_core = Arc::downgrade(self);
        q.set_change_listener(Some(Arc::new(move |id| {
            if let Some(core) = weak_core.upgrade() {
                let needs_move = core
                    .db
                    .lock()
                    .ok()
                    .and_then(|db| db.download_any_profile(id).ok().flatten())
                    .is_some_and(|r| {
                        r.state == DownloadState::Completed
                            && r.blob_id.is_some()
                            && r.kind == DownloadKind::Http
                    });
                if needs_move && let Ok(rt) = core.runtime() {
                    let core2 = Arc::clone(&core);
                    rt.spawn_blocking(move || finalize_completed_download(&core2, id));
                }
                core.emit(CoreEvent::DownloadsChanged);
            }
        })))
        .map_err(|e| CoreError::Invalid(e.to_string()))?;

        // HTTP/1.1 segmented downloader
        let http_engine = Arc::new(
            HttpDownloadEngine::new(self.data_dir.join("blobs"), 4)
                .map_err(|e| CoreError::Invalid(e.to_string()))?,
        );
        q.register_engine(
            DownloadKind::Http,
            Arc::clone(&http_engine) as Arc<dyn DownloadEngine>,
        )
        .map_err(|e| CoreError::Invalid(e.to_string()))?;

        // yt-dlp downloader (single track via the HTTP engine, or in-process mux)
        let ytdlp_engine = Arc::new(YtdlpDownloadEngine::new(
            self.data_dir.join("bin").join("yt-dlp"),
            self.data_dir.join("tmp").join("ytdlp"),
            http_engine,
            Arc::clone(&folder_for),
            finalizer,
        )?);
        q.register_engine(DownloadKind::Ytdlp, ytdlp_engine)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;

        // Torrent downloader: one copy on disk, moved into the folder when done.
        if self.feature_enabled(Feature::Torrent)? {
            let torrent_engine = self.torrents()?;
            let seeding = self.disk_policy_settings()?.torrent_policy(false).seeding;
            let dl_engine = Arc::new(TorrentDownloadEngine::new(
                torrent_engine,
                seeding,
                Arc::clone(&folder_for),
            ));
            *self
                .torrent_download_engine
                .lock()
                .map_err(|_| CoreError::Poisoned)? = Some(Arc::clone(&dl_engine));
            q.register_engine(DownloadKind::Torrent, dl_engine)
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
        }

        // Crash recovery: finished downloads whose bytes were not moved yet.
        let leftovers = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .completed_downloads_with_blob()?;
        if !leftovers.is_empty() {
            let core = Arc::clone(self);
            self.runtime()?.spawn_blocking(move || {
                for row in leftovers {
                    finalize_completed_download(&core, row.id);
                }
            });
        }

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
            ..Default::default()
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

    /// Removes a download. With `delete_data`, also deletes its bytes: the
    /// blob, the torrent's own staging folder, and the finished file — only
    /// these exact paths, never anything else in the user's folder.
    pub fn remove_download(self: &Arc<Self>, id: i64, delete_data: bool) -> Result<(), CoreError> {
        let profile = self.active_profile()?;
        let row = {
            let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
            db.download(profile, id)?
        };
        let Some(row) = row else {
            return Ok(());
        };
        let queue = self
            .queue
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .as_ref()
            .cloned();
        if let Some(q) = queue {
            q.remove(profile, id, delete_data)
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
        } else {
            self.db
                .lock()
                .map_err(|_| CoreError::Poisoned)?
                .delete_download(profile, id)?;
            if delete_data
                && let Some(hex) = &row.blob_id
                && let Ok(parsed) = BlobId::parse(hex)
            {
                let _ = self.blob_store()?.remove(&parsed);
            }
            self.emit(CoreEvent::DownloadsChanged);
        }
        if delete_data {
            if row.kind == DownloadKind::Torrent
                && let Ok(spec) = serde_json::from_str::<TorrentSourceSpec>(&row.source_json)
            {
                let folder = self.download_folder(&row);
                if let Some(staging) = staging_dir(&folder, &spec.info_hash) {
                    remove_staging(&staging, &folder);
                }
            }
            if let Some(path) = row.file_path.as_deref().map(Path::new)
                && path.is_file()
            {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// The folder a download's finished file goes to (settings by type,
    /// defaults `~/Movies/Cinma` and `~/Music/Cinma`).
    pub fn download_folder(&self, row: &DownloadRow) -> PathBuf {
        let settings = self.disk_policy_settings().unwrap_or_default();
        let pick = |configured: &Option<String>, fallback: PathBuf| {
            configured
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map_or(fallback, PathBuf::from)
        };
        match row.kind {
            DownloadKind::Ytdlp => {
                let audio = serde_json::from_str::<YtdlpSourceSpec>(&row.source_json)
                    .is_ok_and(|s| s.audio_only);
                if audio {
                    pick(&settings.audio_folder, default_audio_folder())
                } else {
                    pick(&settings.ytdlp_folder, default_video_folder())
                }
            }
            _ => {
                let media_type = serde_json::from_str::<serde_json::Value>(&row.source_json)
                    .ok()
                    .and_then(|v| v.get("media_type")?.as_str().map(str::to_owned));
                if media_type.as_deref() == Some("series") {
                    pick(&settings.series_folder, default_video_folder())
                } else {
                    pick(&settings.movie_folder, default_video_folder())
                }
            }
        }
    }

    pub fn has_pending_downloads(&self) -> Result<bool, CoreError> {
        let db = self.db.lock().map_err(|_| CoreError::Poisoned)?;
        Ok(db.has_pending_downloads()?)
    }

    /// Bytes of a download for "Play Now" and casting: the finished file, the
    /// blob while downloading, or the live torrent (streams with priority).
    pub fn download_byte_source(&self, id: i64) -> Result<Arc<dyn ByteSource>, CoreError> {
        let profile = self.active_profile()?;
        let row = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .download(profile, id)?
            .ok_or_else(|| CoreError::Invalid(format!("download {id} not found")))?;
        if row.blob_id.is_none()
            && let Some(path) = row.file_path.as_deref().map(Path::new)
            && path.is_file()
        {
            return Ok(Arc::new(FileSource::open(path)?));
        }
        if let Some(blob_hex) = &row.blob_id {
            let blob = self.blob_store()?.open_blob(&BlobId::parse(blob_hex)?)?;
            return Ok(blob.source());
        }
        if row.kind == DownloadKind::Torrent && row.state == DownloadState::Active {
            let spec: TorrentSourceSpec = serde_json::from_str(&row.source_json)
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            let folder = self.download_folder(&row);
            let staging = staging_dir(&folder, &spec.info_hash)
                .ok_or_else(|| CoreError::Invalid("invalid info hash".into()))?;
            let engine = self.torrents()?;
            // Already in the session while active: no network wait here.
            return self.runtime()?.block_on(async move {
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    engine.open_download(spec.request(), &staging),
                )
                .await
                .map_err(|_| CoreError::Invalid("torrent is not ready yet".into()))?
                .map(|(_, source, _)| source)
                .map_err(CoreError::from)
            });
        }
        Err(CoreError::Invalid(
            "nothing to play yet: start or resume the download first".into(),
        ))
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
