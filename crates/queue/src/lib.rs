//! Unified download queue (skeleton). Rows live in SQLite (`download`), bytes
//! in the BlobStore; engines (torrent / HTTP / yt-dlp, Phase 5) plug in
//! through [`DownloadEngine`]. The queue starts queued items up to
//! `max_active`, highest priority first, and recovers `active` rows to
//! `queued` on start-up (after a crash nothing is running).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use cinma_blobstore::{Blob, BlobId, BlobStore, CancellationToken};
use cinma_db::{
    Database, DbError, DownloadKind, DownloadRow, DownloadState, NewDownload, ProfileId,
};
use tokio::runtime::Handle;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("database: {0}")]
    Db(#[from] DbError),
    #[error("storage: {0}")]
    Io(#[from] io::Error),
    #[error("no engine for {0:?} downloads")]
    NoEngine(DownloadKind),
    #[error("download {0} not found")]
    NotFound(i64),
    #[error("internal state poisoned")]
    Poisoned,
}

/// What an engine gets for one download.
pub struct Job {
    pub download: DownloadRow,
    pub cancel: CancellationToken,
    db: Arc<Mutex<Database>>,
    store: Arc<BlobStore>,
}

impl Job {
    /// The download's blob: reopened if it exists (resume), else created
    /// with `size` bytes and recorded on the row.
    pub fn blob(&self, size: u64) -> Result<Arc<Blob>, QueueError> {
        if let Some(id) = &self.download.blob_id
            && let Ok(blob) = self.store.open_blob(&BlobId::parse(id)?)
        {
            return Ok(blob);
        }
        let blob = self.store.create(size)?;
        self.db
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .set_download_blob(self.download.id, blob.id().as_str(), Some(size))?;
        Ok(blob)
    }

    /// Checkpoints the blob, then records the durable byte count — in that
    /// order, so the row never claims more than the blob can prove.
    pub fn report_progress(&self, blob: &Blob) -> Result<u64, QueueError> {
        let durable = blob.checkpoint()?;
        self.db
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .set_download_progress(self.download.id, durable)?;
        Ok(durable)
    }
}

/// A `'static` handle for reporting a job's progress from spawned tasks.
#[derive(Clone)]
pub struct ProgressReporter {
    id: i64,
    db: Arc<Mutex<Database>>,
}

impl ProgressReporter {
    /// Same contract as [`Job::report_progress`]: checkpoint, then the row.
    pub fn report(&self, blob: &Blob) -> Result<u64, QueueError> {
        let durable = blob.checkpoint()?;
        self.db
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .set_download_progress(self.id, durable)?;
        Ok(durable)
    }
}

impl Job {
    pub fn progress_reporter(&self) -> ProgressReporter {
        ProgressReporter {
            id: self.download.id,
            db: Arc::clone(&self.db),
        }
    }

    /// The same job with a different source description (an engine that
    /// resolves its source, like yt-dlp, hands the result to another engine).
    pub fn with_source_json(mut self, source_json: String) -> Job {
        self.download.source_json = source_json;
        self
    }

    /// Records where the finished file lives (engines that write files directly).
    pub fn set_file_path(&self, path: &std::path::Path) -> Result<(), QueueError> {
        self.db
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .set_download_file_path(self.download.id, Some(&path.to_string_lossy()))?;
        Ok(())
    }

    /// Progress for engines without a blob (the bytes are durable in their file).
    pub fn report_bytes(&self, done: u64, total: Option<u64>) -> Result<(), QueueError> {
        self.db
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .set_download_bytes(self.download.id, done, total)?;
        Ok(())
    }
}

pub trait DownloadEngine: Send + Sync {
    /// Runs until the blob is complete, an error, or `job.cancel`.
    /// Returning `Ok` with the job cancelled means "stopped", not "done".
    fn run(&self, job: Job) -> BoxFuture<Result<(), String>>;
}

pub type ChangeListener = Arc<dyn Fn(i64) + Send + Sync>;

struct Inner {
    db: Arc<Mutex<Database>>,
    store: Arc<BlobStore>,
    runtime: Handle,
    max_active: usize,
    engines: Mutex<HashMap<DownloadKind, Arc<dyn DownloadEngine>>>,
    /// Running jobs: download id → (job generation, cancel token).
    running: Mutex<HashMap<i64, (u64, CancellationToken)>>,
    next_generation: std::sync::atomic::AtomicU64,
    on_change: Mutex<Option<ChangeListener>>,
}

#[derive(Clone)]
pub struct Queue {
    inner: Arc<Inner>,
}

impl Queue {
    pub fn new(
        db: Arc<Mutex<Database>>,
        store: Arc<BlobStore>,
        runtime: Handle,
        max_active: usize,
    ) -> Result<Self, QueueError> {
        db.lock()
            .map_err(|_| QueueError::Poisoned)?
            .requeue_active_downloads()?;
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                store,
                runtime,
                max_active: max_active.max(1),
                engines: Mutex::new(HashMap::new()),
                running: Mutex::new(HashMap::new()),
                next_generation: std::sync::atomic::AtomicU64::new(0),
                on_change: Mutex::new(None),
            }),
        })
    }

    pub fn register_engine(
        &self,
        kind: DownloadKind,
        engine: Arc<dyn DownloadEngine>,
    ) -> Result<(), QueueError> {
        self.inner
            .engines
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .insert(kind, engine);
        self.schedule()
    }

    pub fn set_change_listener(&self, listener: Option<ChangeListener>) -> Result<(), QueueError> {
        *self
            .inner
            .on_change
            .lock()
            .map_err(|_| QueueError::Poisoned)? = listener;
        Ok(())
    }

    pub fn add(&self, download: &NewDownload) -> Result<i64, QueueError> {
        if !self
            .inner
            .engines
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .contains_key(&download.kind)
        {
            return Err(QueueError::NoEngine(download.kind));
        }
        let id = self.db()?.insert_download(download)?;
        self.changed(id);
        self.schedule()?;
        Ok(id)
    }

    pub fn pause(&self, profile: ProfileId, id: i64) -> Result<(), QueueError> {
        self.require(profile, id)?;
        self.db()?
            .set_download_state(id, DownloadState::Paused, None)?;
        self.stop_job(id)?;
        self.changed(id);
        self.schedule()
    }

    pub fn resume(&self, profile: ProfileId, id: i64) -> Result<(), QueueError> {
        let row = self.require(profile, id)?;
        if matches!(row.state, DownloadState::Paused | DownloadState::Failed) {
            self.db()?
                .set_download_state(id, DownloadState::Queued, None)?;
            self.changed(id);
        }
        self.schedule()
    }

    /// Removes the row and, if `delete_data`, its blob.
    pub fn remove(&self, profile: ProfileId, id: i64, delete_data: bool) -> Result<(), QueueError> {
        let row = self.require(profile, id)?;
        self.stop_job(id)?;
        self.db()?.delete_download(profile, id)?;
        if delete_data && let Some(blob) = row.blob_id {
            self.inner.store.remove(&BlobId::parse(&blob)?)?;
        }
        self.changed(id);
        self.schedule()
    }

    pub fn downloads(&self, profile: ProfileId) -> Result<Vec<DownloadRow>, QueueError> {
        Ok(self.db()?.downloads(profile)?)
    }

    pub fn active_count(&self) -> usize {
        self.inner.running.lock().map(|r| r.len()).unwrap_or(0)
    }

    fn require(&self, profile: ProfileId, id: i64) -> Result<DownloadRow, QueueError> {
        self.db()?
            .download(profile, id)?
            .ok_or(QueueError::NotFound(id))
    }

    fn db(&self) -> Result<std::sync::MutexGuard<'_, Database>, QueueError> {
        self.inner.db.lock().map_err(|_| QueueError::Poisoned)
    }

    fn changed(&self, id: i64) {
        if let Ok(Some(listener)) = self.inner.on_change.lock().map(|l| l.clone()) {
            listener(id);
        }
    }

    fn stop_job(&self, id: i64) -> Result<(), QueueError> {
        if let Some((_, cancel)) = self
            .inner
            .running
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .remove(&id)
        {
            cancel.cancel();
        }
        Ok(())
    }

    /// Starts queued downloads while there is capacity and an engine for them.
    fn schedule(&self) -> Result<(), QueueError> {
        let started = self.start_ready()?;
        // Notify outside the lock: listeners may call back into the queue.
        for id in started {
            self.changed(id);
        }
        Ok(())
    }

    fn start_ready(&self) -> Result<Vec<i64>, QueueError> {
        let mut started = Vec::new();
        let mut running = self
            .inner
            .running
            .lock()
            .map_err(|_| QueueError::Poisoned)?;
        let free = self.inner.max_active.saturating_sub(running.len());
        if free == 0 {
            return Ok(started);
        }
        let engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .clone();
        let candidates = self.db()?.next_queued_downloads(free + running.len())?;
        for row in candidates {
            if running.contains_key(&row.id) {
                continue;
            }
            if running.len() >= self.inner.max_active {
                break;
            }
            let Some(engine) = engines.get(&row.kind).cloned() else {
                continue;
            };
            self.db()?
                .set_download_state(row.id, DownloadState::Active, None)?;
            let cancel = CancellationToken::new();
            let generation = self
                .inner
                .next_generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            running.insert(row.id, (generation, cancel.clone()));
            let id = row.id;
            let job = Job {
                download: row,
                cancel: cancel.clone(),
                db: Arc::clone(&self.inner.db),
                store: Arc::clone(&self.inner.store),
            };
            let queue = self.clone();
            self.inner.runtime.spawn(async move {
                let result = engine.run(job).await;
                queue.finish(id, generation, &cancel, result);
            });
            started.push(id);
        }
        Ok(started)
    }

    fn finish(
        &self,
        id: i64,
        generation: u64,
        cancel: &CancellationToken,
        result: Result<(), String>,
    ) {
        // Only remove our own entry: after pause+resume a newer job for the
        // same download may already be running.
        let was_running = self
            .inner
            .running
            .lock()
            .map(|mut r| match r.get(&id) {
                Some((g, _)) if *g == generation => r.remove(&id).is_some(),
                _ => false,
            })
            .unwrap_or(false);
        // Paused/removed jobs were already taken out of `running` and keep
        // whatever state pause/remove gave them.
        if was_running && !cancel.is_cancelled() {
            let (state, error) = match &result {
                Ok(()) => (DownloadState::Completed, None),
                Err(e) => (DownloadState::Failed, Some(e.as_str())),
            };
            if let Ok(db) = self.db() {
                let _ = db.set_download_state(id, state, error);
            }
        }
        self.changed(id);
        let _ = self.schedule();
    }
}
