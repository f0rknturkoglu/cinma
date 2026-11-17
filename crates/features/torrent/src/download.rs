//! Torrent download engine (`cinma-queue::DownloadEngine`):
//! full completion, multi-file selection (`file_idx`), `fastresume`, upload/download
//! caps, seeding policies, and live mid-download playback via `Blob::source()`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cinma_blobstore::{Blob, BlobId, PriorityListener, ReadHint};
use cinma_queue::{BoxFuture, DownloadEngine, Job};
use serde::{Deserialize, Serialize};

use crate::{RqbitEngine, TorrentFileRequest};

const CHUNK_BYTES: usize = 256 * 1024;
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(2);

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

pub struct TorrentDownloadEngine {
    engine: Arc<RqbitEngine>,
    default_seeding: Mutex<SeedingPolicy>,
}

impl TorrentDownloadEngine {
    pub fn new(engine: Arc<RqbitEngine>, default_seeding: SeedingPolicy) -> Self {
        Self {
            engine,
            default_seeding: Mutex::new(default_seeding),
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
        Box::pin(async move { run_torrent_download(engine, job, seeding).await })
    }
}

async fn run_torrent_download(
    engine: Arc<RqbitEngine>,
    job: Job,
    seeding: SeedingPolicy,
) -> Result<(), String> {
    let spec: TorrentSourceSpec =
        serde_json::from_str(&job.download.source_json).map_err(|e| e.to_string())?;
    let info_hash = crate::normalize_info_hash(&spec.info_hash).map_err(|e| e.to_string())?;
    engine.mark_download(&info_hash);
    let result = download_and_seed(&engine, &job, &spec, seeding).await;
    engine.unmark_download(&info_hash);
    result
}

async fn download_and_seed(
    engine: &Arc<RqbitEngine>,
    job: &Job,
    spec: &TorrentSourceSpec,
    seeding: SeedingPolicy,
) -> Result<(), String> {
    let (handle, source) = engine
        .open_with_handle(TorrentFileRequest {
            info_hash: spec.info_hash.clone(),
            file_idx: spec.file_idx,
            trackers: spec.trackers.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;

    let total_size = source.size();
    let blob = job.blob(total_size).map_err(|e| e.to_string())?;
    if !blob.is_complete() {
        copy_source_to_blob(Arc::clone(&source), &blob, job).await?;
    }
    let _ = job.report_progress(&blob);

    if job.cancel.is_cancelled() {
        return Ok(());
    }

    // Apply post-completion seeding policy if applicable.
    match seeding {
        SeedingPolicy::Off => {
            if let Ok(sess) = engine.session().await {
                let _ = sess.pause(&handle).await;
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
            let target_uploaded = (total_size as f64 * ratio.max(0.0)) as u64;
            let deadline = Instant::now() + Duration::from_secs(24 * 3600);
            while !job.cancel.is_cancelled() && Instant::now() < deadline {
                let st = handle.stats();
                if st.uploaded_bytes >= target_uploaded {
                    break;
                }
                tokio::select! {
                    () = job.cancel.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_secs(2)) => {}
                }
            }
        }
    }
    Ok(())
}

async fn copy_source_to_blob(
    source: Arc<dyn cinma_blobstore::ByteSource>,
    blob: &Arc<Blob>,
    job: &Job,
) -> Result<(), String> {
    let total = source.size();
    let urgent_offset = Arc::new(Mutex::new(None::<u64>));
    blob.set_priority_listener(Some(Arc::new(TorrentCursorListener {
        blob_id: blob.id().clone(),
        urgent_offset: Arc::clone(&urgent_offset),
    })));

    let src_for_blocking = Arc::clone(&source);
    let blob_ref = Arc::clone(blob);
    let cancel = job.cancel.clone();
    let job_ref = job;
    let mut reader = tokio::task::spawn_blocking(move || src_for_blocking.open_reader())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let mut last_cp = Instant::now();
    loop {
        if cancel.is_cancelled() {
            break;
        }
        let missing = blob_ref.available().missing(0..total);
        if missing.is_empty() {
            break;
        }
        let next_gap = if let Ok(mut g) = urgent_offset.lock()
            && let Some(cursor) = g.take()
            && let Some(gap) = blob_ref
                .available()
                .missing(cursor.min(total)..total)
                .into_iter()
                .next()
        {
            gap
        } else {
            missing[0].clone()
        };

        let mut offset = next_gap.start;
        let end = next_gap.end;
        while offset < end && !cancel.is_cancelled() {
            // Check if a play cursor jump was requested mid-gap.
            if let Ok(g) = urgent_offset.lock()
                && g.is_some()
            {
                break;
            }
            let to_read = (end - offset).min(CHUNK_BYTES as u64) as usize;
            let c_tok = cancel.clone();
            let (returned_reader, res) = tokio::task::spawn_blocking(move || {
                let mut buf = vec![0u8; to_read];
                let res = reader.read_at(offset, &mut buf, &c_tok);
                (
                    reader,
                    res.map(|n| {
                        buf.truncate(n);
                        buf
                    }),
                )
            })
            .await
            .map_err(|e| e.to_string())?;
            reader = returned_reader;

            match res {
                Ok(buf) if buf.is_empty() => break,
                Ok(buf) => {
                    let n = buf.len() as u64;
                    blob_ref
                        .write_available(offset, &buf)
                        .map_err(|e| e.to_string())?;
                    offset += n;
                    if last_cp.elapsed() >= CHECKPOINT_INTERVAL {
                        let _ = job_ref.report_progress(&blob_ref);
                        last_cp = Instant::now();
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => break,
                Err(e) => {
                    blob.set_priority_listener(None);
                    let _ = job_ref.report_progress(&blob_ref);
                    return Err(e.to_string());
                }
            }
        }
    }

    blob.set_priority_listener(None);
    let _ = job_ref.report_progress(&blob_ref);
    Ok(())
}

struct TorrentCursorListener {
    blob_id: BlobId,
    urgent_offset: Arc<Mutex<Option<u64>>>,
}

impl PriorityListener for TorrentCursorListener {
    fn on_read(&self, blob: &BlobId, hint: ReadHint) {
        if *blob == self.blob_id
            && let Ok(mut g) = self.urgent_offset.lock()
        {
            *g = Some(hint.offset);
        }
    }
}
