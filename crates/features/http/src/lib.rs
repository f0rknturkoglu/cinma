//! Segmented HTTP/1.1 downloader (`CLAUDE.md` §5.4).
//!
//! - Uses `.http1_only()` so parallel byte-range connections don't contend on one HTTP/2 socket.
//! - Dynamically splits the largest remaining unclaimed byte gap when a worker finishes.
//! - Validates resume against persisted `ETag` / `Last-Modified` validators.
//! - Subscribes a `PriorityListener` on the `Blob` so "Play Now" steers a worker to the read cursor.

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cinma_blobstore::{Blob, BlobId, CancellationToken, PriorityListener, RangeSet, ReadHint};
use cinma_queue::{BoxFuture, DownloadEngine, Job};
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, ETAG, LAST_MODIFIED, RANGE};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, Semaphore};
use url::Url;

const CHUNK_SIZE: u64 = 2 * 1024 * 1024;
const CURSOR_WINDOW: u64 = 2 * 1024 * 1024;
/// Each checkpoint fsyncs the blob, rewrites its sidecar and commits SQLite.
/// 10 s keeps SSD writes low; a crash re-downloads at most ~10 s of data.
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpSourceSpec {
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub meta_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    /// Signed URLs (yt-dlp) change on every probe; resume then matches on
    /// size + ETag/Last-Modified only.
    #[serde(default)]
    pub url_may_change: bool,
    /// Extension of the finished file when the URL doesn't tell (yt-dlp).
    #[serde(default)]
    pub file_ext: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RemoteValidator {
    url: String,
    size: u64,
    etag: Option<String>,
    last_modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeInfo {
    pub size: Option<u64>,
    pub accepts_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpDownloadError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server returned HTTP {0}")]
    Status(u16),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid source: {0}")]
    Invalid(String),
}

pub struct HttpDownloadEngine {
    client: reqwest::Client,
    meta_dir: PathBuf,
    per_host_limit: usize,
    host_semaphores: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl HttpDownloadEngine {
    pub fn new(meta_dir: PathBuf, per_host_limit: usize) -> Result<Self, HttpDownloadError> {
        let client = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(Duration::from_secs(12))
            .timeout(Duration::from_secs(60))
            .build()?;
        std::fs::create_dir_all(&meta_dir)?;
        Ok(Self {
            client,
            meta_dir,
            per_host_limit: per_host_limit.clamp(1, 8),
            host_semaphores: Mutex::new(HashMap::new()),
        })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    fn host_semaphore(&self, host: &str) -> Arc<Semaphore> {
        let mut guard = self
            .host_semaphores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(host.to_owned())
                .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_limit))),
        )
    }

    pub async fn probe(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
    ) -> Result<ProbeInfo, HttpDownloadError> {
        let mut head = self.client.head(url);
        for (k, v) in headers {
            head = head.header(k.as_str(), v.as_str());
        }
        if let Ok(resp) = head.send().await
            && resp.status().is_success()
        {
            let size = resp
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            let accepts_ranges = resp
                .headers()
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.eq_ignore_ascii_case("bytes"));
            let etag = header_str(resp.headers(), ETAG);
            let last_modified = header_str(resp.headers(), LAST_MODIFIED);
            if size.is_some() {
                return Ok(ProbeInfo {
                    size,
                    accepts_ranges,
                    etag,
                    last_modified,
                });
            }
        }

        // Fallback probe with `Range: bytes=0-0` when HEAD omits Content-Length.
        let mut get = self.client.get(url).header(RANGE, "bytes=0-0");
        for (k, v) in headers {
            get = get.header(k.as_str(), v.as_str());
        }
        let resp = get.send().await?;
        let status = resp.status();
        if status.as_u16() != 206 && !status.is_success() {
            return Err(HttpDownloadError::Status(status.as_u16()));
        }
        let etag = header_str(resp.headers(), ETAG);
        let last_modified = header_str(resp.headers(), LAST_MODIFIED);
        if status.as_u16() == 206 {
            let total = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rsplit_once('/'))
                .and_then(|(_, tot)| tot.parse::<u64>().ok());
            return Ok(ProbeInfo {
                size: total,
                accepts_ranges: true,
                etag,
                last_modified,
            });
        }
        let size = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        Ok(ProbeInfo {
            size,
            accepts_ranges: false,
            etag,
            last_modified,
        })
    }

    fn validator_path(&self, download_id: i64) -> PathBuf {
        self.meta_dir.join(format!("http_{download_id}.json"))
    }

    fn load_validator(&self, download_id: i64) -> Option<RemoteValidator> {
        let raw = std::fs::read(self.validator_path(download_id)).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    fn save_validator(&self, download_id: i64, val: &RemoteValidator) -> std::io::Result<()> {
        let json = serde_json::to_vec(val)?;
        std::fs::write(self.validator_path(download_id), json)
    }

    async fn execute_job(&self, job: Job) -> Result<(), String> {
        let spec: HttpSourceSpec =
            serde_json::from_str(&job.download.source_json).map_err(|e| e.to_string())?;
        let parsed = Url::parse(&spec.url).map_err(|e| e.to_string())?;
        let host = parsed.host_str().unwrap_or("localhost").to_owned();
        let probe = self
            .probe(&spec.url, &spec.headers)
            .await
            .map_err(|e| e.to_string())?;

        let total_size = probe
            .size
            .ok_or_else(|| "remote server did not provide Content-Length".to_owned())?;
        let validator = RemoteValidator {
            url: spec.url.clone(),
            size: total_size,
            etag: probe.etag.clone(),
            last_modified: probe.last_modified.clone(),
        };

        let existing_valid = self.load_validator(job.download.id).is_some_and(|prev| {
            (spec.url_may_change || prev.url == validator.url)
                && prev.size == validator.size
                && prev.etag == validator.etag
                && prev.last_modified == validator.last_modified
        });

        let blob = if existing_valid {
            job.blob(total_size).map_err(|e| e.to_string())?
        } else {
            // Validator changed (or first run): allocate fresh blob if size/etag mismatched.
            if let Some(prev_blob) = job.download.blob_id.as_deref()
                && let Ok(id) = BlobId::parse(prev_blob)
                && let Ok(existing) = job.blob(total_size)
                && existing.available().covered() > 0
                && self.load_validator(job.download.id).is_some()
            {
                // Keep safe: cannot reuse stale ranges if validator changed.
                let _ = id;
                return Err("upstream ETag/Last-Modified changed; restart download".into());
            }
            let b = job.blob(total_size).map_err(|e| e.to_string())?;
            let _ = self.save_validator(job.download.id, &validator);
            b
        };

        if blob.is_complete() {
            let _ = job.report_progress(&blob);
            return Ok(());
        }

        if !probe.accepts_ranges || total_size <= CHUNK_SIZE {
            return self
                .download_single(&job, &blob, &spec, total_size)
                .await
                .map_err(|e| e.to_string());
        }

        self.download_segmented(&job, &blob, &spec, &host, total_size)
            .await
    }

    async fn download_single(
        &self,
        job: &Job,
        blob: &Blob,
        spec: &HttpSourceSpec,
        total_size: u64,
    ) -> Result<(), HttpDownloadError> {
        use std::pin::pin;
        let mut req = self.client.get(&spec.url);
        for (k, v) in &spec.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(HttpDownloadError::Status(resp.status().as_u16()));
        }
        let mut offset = 0u64;
        let mut last_cp = Instant::now();
        let mut resp = resp;
        loop {
            let chunk_fut = pin!(resp.chunk());
            let maybe_chunk = tokio::select! {
                biased;
                () = job.cancel.cancelled() => {
                    let _ = job.report_progress(blob);
                    return Ok(());
                }
                res = chunk_fut => res?,
            };
            let Some(bytes) = maybe_chunk else { break };
            if offset + bytes.len() as u64 <= total_size {
                blob.write_available(offset, &bytes)?;
            }
            offset += bytes.len() as u64;
            if last_cp.elapsed() >= CHECKPOINT_INTERVAL {
                let _ = job.report_progress(blob);
                last_cp = Instant::now();
            }
        }
        let _ = job.report_progress(blob);
        Ok(())
    }

    async fn download_segmented(
        &self,
        job: &Job,
        blob: &Arc<Blob>,
        spec: &HttpSourceSpec,
        host: &str,
        total_size: u64,
    ) -> Result<(), String> {
        let scheduler = Arc::new(SegmentScheduler::new(blob.available(), total_size));
        let listener = Arc::new(BlobCursorListener {
            blob_id: blob.id().clone(),
            scheduler: Arc::clone(&scheduler),
        });
        blob.set_priority_listener(Some(listener));

        let worker_count = self.per_host_limit.clamp(1, 4);
        let sem = self.host_semaphore(host);
        let first_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let abort_all = CancellationToken::new();

        // Background checkpointer runs until all workers finish or cancel.
        let cp_cancel = job.cancel.clone();
        let cp_abort = abort_all.clone();
        let cp_blob = Arc::clone(blob);

        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let client = self.client.clone();
            let spec = spec.clone();
            let blob = Arc::clone(blob);
            let sched = Arc::clone(&scheduler);
            let sem = Arc::clone(&sem);
            let cancel = job.cancel.clone();
            let abort = abort_all.clone();
            let err_slot = Arc::clone(&first_error);

            handles.push(tokio::spawn(async move {
                while !cancel.is_cancelled() && !abort.is_cancelled() {
                    let Some(range) = sched.next_range() else {
                        break;
                    };
                    let Ok(_permit) = tokio::select! (
                        biased;
                        () = cancel.cancelled() => return,
                        () = abort.cancelled() => return,
                        p = sem.acquire() => p,
                    ) else {
                        return;
                    };

                    if let Err(e) = fetch_range(
                        &client,
                        &spec,
                        &blob,
                        range.clone(),
                        &sched,
                        &cancel,
                        &abort,
                    )
                    .await
                    {
                        sched.release_unfinished(range);
                        if !cancel.is_cancelled()
                            && let Ok(mut g) = err_slot.lock()
                        {
                            g.get_or_insert(e.to_string());
                        }
                        abort.cancel();
                        break;
                    }
                }
            }));
        }

        // Periodic checkpoint loop (event/timer bounded to active download only).
        let cp_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = cp_cancel.cancelled() => break,
                    () = cp_abort.cancelled() => break,
                    () = tokio::time::sleep(CHECKPOINT_INTERVAL) => {
                        let _ = cp_blob.checkpoint();
                    }
                }
            }
        });

        for h in handles {
            let _ = h.await;
        }
        abort_all.cancel();
        let _ = cp_task.await;
        blob.set_priority_listener(None);
        let _ = job.report_progress(blob);

        if job.cancel.is_cancelled() {
            return Ok(());
        }
        if let Ok(mut g) = first_error.lock()
            && let Some(err) = g.take()
        {
            return Err(err);
        }
        Ok(())
    }
}

impl DownloadEngine for HttpDownloadEngine {
    fn run(&self, job: Job) -> BoxFuture<Result<(), String>> {
        let client = self.client.clone();
        let meta_dir = self.meta_dir.clone();
        let per_host_limit = self.per_host_limit;
        let semaphores = Arc::new(
            self.host_semaphores
                .lock()
                .map(|m| m.clone())
                .unwrap_or_default(),
        );
        Box::pin(async move {
            let engine = HttpDownloadEngine {
                client,
                meta_dir,
                per_host_limit,
                host_semaphores: Mutex::new((*semaphores).clone()),
            };
            engine.execute_job(job).await
        })
    }
}

async fn fetch_range(
    client: &reqwest::Client,
    spec: &HttpSourceSpec,
    blob: &Blob,
    range: Range<u64>,
    scheduler: &SegmentScheduler,
    cancel: &CancellationToken,
    abort: &CancellationToken,
) -> Result<(), HttpDownloadError> {
    let end_inclusive = range.end - 1;
    let mut req = client
        .get(&spec.url)
        .header(RANGE, format!("bytes={}-{}", range.start, end_inclusive));
    for (k, v) in &spec.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let mut resp = tokio::select! {
        biased;
        () = cancel.cancelled() => return Ok(()),
        () = abort.cancelled() => return Ok(()),
        r = req.send() => r?,
    };
    let status = resp.status();
    if status.as_u16() != 206 && !status.is_success() {
        return Err(HttpDownloadError::Status(status.as_u16()));
    }

    let mut cursor = range.start;
    while cursor < range.end {
        let maybe_chunk = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(()),
            () = abort.cancelled() => return Ok(()),
            c = resp.chunk() => c?,
        };
        let Some(chunk) = maybe_chunk else { break };
        let remaining = (range.end - cursor) as usize;
        let slice = &chunk[..chunk.len().min(remaining)];
        if !slice.is_empty() {
            blob.write_available(cursor, slice)?;
            let next = cursor + slice.len() as u64;
            scheduler.mark_done(cursor..next);
            cursor = next;
        }
    }
    Ok(())
}

/// Tracks remaining gaps, urgent cursor requests from `PriorityListener`, and
/// dynamic gap splitting when workers become idle.
struct SegmentScheduler {
    state: Mutex<SchedState>,
    notify: Notify,
}

struct SchedState {
    total: u64,
    done: RangeSet,
    claimed: RangeSet,
    urgent: VecDeque<Range<u64>>,
}

impl SegmentScheduler {
    fn new(done: RangeSet, total: u64) -> Self {
        Self {
            state: Mutex::new(SchedState {
                total,
                done: done.clone(),
                claimed: done,
                urgent: VecDeque::new(),
            }),
            notify: Notify::new(),
        }
    }

    fn prioritize_cursor(&self, offset: u64) {
        let Ok(mut s) = self.state.lock() else {
            return;
        };
        let target = offset.min(s.total)..(offset.saturating_add(CURSOR_WINDOW)).min(s.total);
        if target.start < target.end {
            for missing in s.claimed.missing(target) {
                s.urgent.push_back(missing);
            }
            self.notify.notify_one();
        }
    }

    fn next_range(&self) -> Option<Range<u64>> {
        let mut s = self.state.lock().ok()?;
        while let Some(u) = s.urgent.pop_front() {
            if let Some(gap) = s.claimed.missing(u).into_iter().next() {
                let slice = gap.start..gap.end.min(gap.start + CHUNK_SIZE);
                s.claimed.insert(slice.clone());
                return Some(slice);
            }
        }
        // Pick the largest remaining gap and split if wide so parallel workers stay busy.
        let gap = s
            .claimed
            .missing(0..s.total)
            .into_iter()
            .max_by_key(|g| g.end - g.start)?;
        let len = gap.end - gap.start;
        // Dynamic split: take at most `CHUNK_SIZE`, or half of a big gap so another
        // free worker can concurrently grab the rest.
        let take = if len > CHUNK_SIZE * 2 {
            (len / 2).min(CHUNK_SIZE * 2)
        } else {
            len.min(CHUNK_SIZE)
        };
        let slice = gap.start..gap.start + take;
        s.claimed.insert(slice.clone());
        Some(slice)
    }

    fn mark_done(&self, range: Range<u64>) {
        if let Ok(mut s) = self.state.lock() {
            s.done.insert(range);
        }
    }

    fn release_unfinished(&self, range: Range<u64>) {
        if let Ok(mut s) = self.state.lock() {
            // Recompute `claimed` safely as `done` so unfinished chunks can be retried.
            let _ = range;
            s.claimed = s.done.clone();
        }
    }
}

struct BlobCursorListener {
    blob_id: BlobId,
    scheduler: Arc<SegmentScheduler>,
}

impl PriorityListener for BlobCursorListener {
    fn on_read(&self, blob: &BlobId, hint: ReadHint) {
        if *blob == self.blob_id {
            self.scheduler.prioritize_cursor(hint.offset);
        }
    }
}

fn header_str(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}
