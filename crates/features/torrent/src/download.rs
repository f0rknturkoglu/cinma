//! Torrent download engine (`cinma-queue::DownloadEngine`), ADR 0016.
//!
//! One copy on disk: librqbit writes straight into a staging folder next to
//! the destination (`<folder>/.cinma-partial/<info hash>/`, same volume), and
//! the finished file is moved — hard link + unlink, no data copied — to
//! `<folder>/<file name>`. Pausing detaches the torrent from the session and
//! keeps the partial file; resuming re-adds it and librqbit re-checks the
//! pieces already on disk. "Play Now" streams from the same torrent.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cinma_blobstore::{move_file_no_overwrite, sanitize_file_name, unique_destination};
use cinma_db::DownloadRow;
use cinma_queue::{BoxFuture, DownloadEngine, Job};
use librqbit::ManagedTorrent;
use serde::{Deserialize, Serialize};

use crate::{RqbitEngine, TorrentFileRequest, normalize_info_hash};

/// Folder (inside each download folder) that holds unfinished torrents.
pub const PARTIAL_DIR: &str = ".cinma-partial";
/// Progress is written to SQLite this often while downloading.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum SeedingPolicy {
    Off,
    Ratio { ratio: f64 },
    Minutes { minutes: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentSourceSpec {
    pub info_hash: String,
    #[serde(default)]
    pub file_idx: Option<usize>,
    #[serde(default)]
    pub trackers: Vec<String>,
    #[serde(default)]
    pub meta_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
}

impl TorrentSourceSpec {
    pub fn request(&self) -> TorrentFileRequest {
        TorrentFileRequest {
            info_hash: self.info_hash.clone(),
            file_idx: self.file_idx,
            trackers: self.trackers.clone(),
        }
    }
}

/// Picks the download folder for a row (user setting by media type).
pub type FolderResolver = Arc<dyn Fn(&DownloadRow) -> PathBuf + Send + Sync>;

/// `<folder>/.cinma-partial/<info hash>`: the only place a torrent download
/// writes before it finishes, and the only folder removal may delete.
pub fn staging_dir(folder: &Path, info_hash: &str) -> Option<PathBuf> {
    let hash = normalize_info_hash(info_hash).ok()?;
    Some(folder.join(PARTIAL_DIR).join(hash))
}

pub struct TorrentDownloadEngine {
    engine: Arc<RqbitEngine>,
    default_seeding: Mutex<SeedingPolicy>,
    folder_for: FolderResolver,
}

impl TorrentDownloadEngine {
    pub fn new(
        engine: Arc<RqbitEngine>,
        default_seeding: SeedingPolicy,
        folder_for: FolderResolver,
    ) -> Self {
        Self {
            engine,
            default_seeding: Mutex::new(default_seeding),
            folder_for,
        }
    }

    pub fn set_seeding(&self, policy: SeedingPolicy) {
        if let Ok(mut g) = self.default_seeding.lock() {
            *g = policy;
        }
    }
}

impl DownloadEngine for TorrentDownloadEngine {
    fn run(&self, job: Job) -> BoxFuture<Result<(), String>> {
        let engine = Arc::clone(&self.engine);
        let seeding = self
            .default_seeding
            .lock()
            .map(|g| *g)
            .unwrap_or(SeedingPolicy::Off);
        let folder = (self.folder_for)(&job.download);
        Box::pin(async move { run_torrent_download(engine, job, seeding, folder).await })
    }
}

async fn run_torrent_download(
    engine: Arc<RqbitEngine>,
    job: Job,
    seeding: SeedingPolicy,
    folder: PathBuf,
) -> Result<(), String> {
    let spec: TorrentSourceSpec =
        serde_json::from_str(&job.download.source_json).map_err(|e| e.to_string())?;
    let info_hash = normalize_info_hash(&spec.info_hash).map_err(|e| e.to_string())?;
    let staging = staging_dir(&folder, &info_hash).ok_or("invalid info hash")?;

    // Finished in a previous run (crash after the move): nothing to do.
    if let Some(done) = job.download.file_path.as_deref()
        && Path::new(done).is_file()
        && !staging.exists()
    {
        return Ok(());
    }

    let result = download(&engine, &job, &spec, seeding, &folder, &staging).await;
    engine.unmark_download(&info_hash);
    result
}

async fn download(
    engine: &Arc<RqbitEngine>,
    job: &Job,
    spec: &TorrentSourceSpec,
    seeding: SeedingPolicy,
    folder: &Path,
    staging: &Path,
) -> Result<(), String> {
    let (handle, source, file_path) = engine
        .open_download(spec.request(), staging)
        .await
        .map_err(|e| e.to_string())?;
    let total = source.size();
    drop(source);

    // Download, writing progress every few seconds (only while active).
    let completed = loop {
        tokio::select! {
            () = job.cancel.cancelled() => break false,
            res = handle.wait_until_completed() => {
                res.map_err(|e| e.to_string())?;
                break true;
            }
            () = tokio::time::sleep(PROGRESS_INTERVAL) => {
                let done = handle.stats().progress_bytes.min(total);
                let _ = job.report_bytes(done, Some(total));
            }
        }
    };
    if !completed {
        // Paused or removed: stop network activity, keep the partial file.
        engine.forget(&handle).await;
        let done = handle.stats().progress_bytes.min(total);
        let _ = job.report_bytes(done, Some(total));
        return Ok(());
    }
    let _ = job.report_bytes(total, Some(total));

    seed(engine, &handle, job, seeding, total).await;
    engine.forget(&handle).await;
    if job.cancel.is_cancelled() {
        return Ok(());
    }

    // Move the finished file next to the staging folder (same volume).
    let name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(sanitize_file_name)
        .unwrap_or_else(|| sanitize_file_name(&job.download.title));
    let dest = unique_destination(folder, &name);
    job.set_file_path(&dest).map_err(|e| e.to_string())?;
    let src = file_path.clone();
    let dest_clone = dest.clone();
    tokio::task::spawn_blocking(move || move_file_no_overwrite(&src, &dest_clone))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("could not move the finished file: {e}"))?;
    remove_staging(staging, folder);
    Ok(())
}

async fn seed(
    engine: &Arc<RqbitEngine>,
    handle: &Arc<ManagedTorrent>,
    job: &Job,
    seeding: SeedingPolicy,
    total: u64,
) {
    match seeding {
        SeedingPolicy::Off => {
            if let Ok(sess) = engine.session().await {
                let _ = sess.pause(handle).await;
            }
        }
        SeedingPolicy::Minutes { minutes } => {
            let wait = Duration::from_secs(minutes.saturating_mul(60));
            tokio::select! {
                () = job.cancel.cancelled() => {}
                () = tokio::time::sleep(wait) => {}
            }
        }
        SeedingPolicy::Ratio { ratio } => {
            let target = (total as f64 * ratio.max(0.0)) as u64;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(24 * 3600);
            loop {
                if handle.stats().uploaded_bytes >= target {
                    break;
                }
                tokio::select! {
                    () = job.cancel.cancelled() => break,
                    () = tokio::time::sleep_until(deadline) => break,
                    () = tokio::time::sleep(Duration::from_secs(30)) => {}
                }
            }
        }
    }
}

/// Deletes `<folder>/.cinma-partial/<hash>` and the parent if it is empty.
/// Only paths built by [`staging_dir`] ever reach here.
pub fn remove_staging(staging: &Path, folder: &Path) {
    if staging.parent() != Some(folder.join(PARTIAL_DIR).as_path()) {
        return;
    }
    let _ = std::fs::remove_dir_all(staging);
    let _ = std::fs::remove_dir(folder.join(PARTIAL_DIR));
}
