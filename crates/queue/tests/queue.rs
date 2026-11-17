#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cinma_blobstore::{BlobStore, RangeSet, fetch_order};
use cinma_db::{Database, DownloadKind, DownloadState, NewDownload};
use cinma_queue::{BoxFuture, DownloadEngine, Job, Queue};

pub const SIZE: u64 = 4 * 1024 * 1024;
const CHUNK: u64 = 64 * 1024;

pub fn byte_at(offset: u64) -> u8 {
    (offset.wrapping_mul(2_654_435_761) >> 11) as u8
}

/// Writes a synthetic file in head/tail/cursor order, resuming from what the
/// blob already has; reports progress every `report_every` chunks.
pub struct FakeEngine {
    pub delay: Duration,
    pub report_every: u64,
    pub fail: bool,
}

impl DownloadEngine for FakeEngine {
    fn run(&self, job: Job) -> BoxFuture<Result<(), String>> {
        let (delay, every, fail) = (self.delay, self.report_every, self.fail);
        Box::pin(async move {
            if fail {
                return Err("boom".into());
            }
            let blob = job.blob(SIZE).map_err(|e| e.to_string())?;
            let mut n = 0u64;
            for range in fetch_order(SIZE, &blob.available(), 0) {
                let mut offset = range.start;
                while offset < range.end {
                    if job.cancel.is_cancelled() {
                        job.report_progress(&blob).map_err(|e| e.to_string())?;
                        return Ok(());
                    }
                    let len = CHUNK.min(range.end - offset);
                    let data: Vec<u8> = (offset..offset + len).map(byte_at).collect();
                    blob.write_available(offset, &data).map_err(|e| e.to_string())?;
                    offset += len;
                    n += 1;
                    if n % every == 0 {
                        job.report_progress(&blob).map_err(|e| e.to_string())?;
                    }
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
            job.report_progress(&blob).map_err(|e| e.to_string())?;
            Ok(())
        })
    }
}

fn new_download(profile: i64, title: &str, priority: i64) -> NewDownload {
    NewDownload {
        profile_id: profile,
        kind: DownloadKind::Http,
        title: title.into(),
        source_json: "{}".into(),
        priority,
    }
}

async fn wait_for(queue: &Queue, id: i64, state: DownloadState) {
    for _ in 0..500 {
        if queue.downloads(1).unwrap().iter().any(|d| d.id == id && d.state == state) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("download {id} never reached {state:?}: {:?}", queue.downloads(1).unwrap());
}

fn setup(dir: &std::path::Path, max_active: usize) -> (Queue, Arc<Mutex<Database>>, Arc<BlobStore>) {
    let db = Arc::new(Mutex::new(Database::open(&dir.join("db.sqlite3")).unwrap()));
    let store = Arc::new(BlobStore::open(&dir.join("blobs")).unwrap());
    let queue = Queue::new(Arc::clone(&db), Arc::clone(&store), tokio::runtime::Handle::current(), max_active).unwrap();
    (queue, db, store)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runs_to_completion_and_respects_max_active() {
    let dir = tempfile::tempdir().unwrap();
    let (queue, _db, store) = setup(dir.path(), 1);
    queue
        .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::from_millis(1), report_every: 8, fail: false }))
        .unwrap();
    let low = queue.add(&new_download(1, "low", 0)).unwrap();
    let high = queue.add(&new_download(1, "high", 9)).unwrap();
    assert_eq!(queue.active_count(), 1, "max_active = 1");
    wait_for(&queue, low, DownloadState::Completed).await;
    wait_for(&queue, high, DownloadState::Completed).await;
    let row = queue.downloads(1).unwrap().into_iter().find(|d| d.id == high).unwrap();
    assert_eq!(row.bytes_done, SIZE);
    let blob = store.open_blob(&cinma_blobstore::BlobId::parse(row.blob_id.as_deref().unwrap()).unwrap()).unwrap();
    assert!(blob.is_complete());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_resume_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (queue, _db, _store) = setup(dir.path(), 2);
    queue
        .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::from_millis(5), report_every: 2, fail: false }))
        .unwrap();
    let id = queue.add(&new_download(1, "a", 0)).unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    queue.pause(1, id).unwrap();
    // Resume right away while the old job may still be winding down
    // (regression: its finish() must not clobber the new job).
    queue.resume(1, id).unwrap();
    queue.pause(1, id).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let paused = queue.downloads(1).unwrap()[0].clone();
    assert_eq!(paused.state, DownloadState::Paused);
    assert!(paused.bytes_done > 0 && paused.bytes_done < SIZE);
    queue.resume(1, id).unwrap();
    wait_for(&queue, id, DownloadState::Completed).await;

    queue
        .register_engine(DownloadKind::Torrent, Arc::new(FakeEngine { delay: Duration::ZERO, report_every: 1, fail: true }))
        .unwrap();
    let mut failing = new_download(1, "bad", 0);
    failing.kind = DownloadKind::Torrent;
    let bad = queue.add(&failing).unwrap();
    wait_for(&queue, bad, DownloadState::Failed).await;
    let row = queue.downloads(1).unwrap().into_iter().find(|d| d.id == bad).unwrap();
    assert_eq!(row.error.as_deref(), Some("boom"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_kinds_without_engine_and_other_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let (queue, db, _store) = setup(dir.path(), 1);
    assert!(queue.add(&new_download(1, "x", 0)).is_err(), "no engine registered");
    queue
        .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::from_millis(50), report_every: 1, fail: false }))
        .unwrap();
    let other = db.lock().unwrap().create_profile("Other").unwrap();
    let id = queue.add(&new_download(1, "mine", 0)).unwrap();
    assert!(queue.pause(other, id).is_err(), "can't touch another profile's download");
    assert!(queue.downloads(other).unwrap().is_empty());
    queue.remove(1, id, true).unwrap();
    assert!(queue.downloads(1).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_an_active_download_deletes_all_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let (queue, _db, _store) = setup(dir.path(), 1);
    queue
        .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::from_millis(5), report_every: 1, fail: false }))
        .unwrap();
    // A listener that calls back into the queue must not deadlock.
    let reentrant = queue.clone();
    queue.set_change_listener(Some(Arc::new(move |_| { let _ = reentrant.active_count(); }))).unwrap();
    let id = queue.add(&new_download(1, "a", 0)).unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    queue.remove(1, id, true).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await; // engine notices cancel and checkpoints
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("blobs")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(leftovers.is_empty(), "orphaned files: {leftovers:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_requeues_active_and_resumes_from_blob() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let (queue, db, _store) = setup(dir.path(), 1);
        queue
            .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::from_millis(5), report_every: 2, fail: false }))
            .unwrap();
        let id = queue.add(&new_download(1, "a", 0)).unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        // Simulate a crash: the row is still 'active' when we walk away.
        assert_eq!(db.lock().unwrap().download(1, id).unwrap().unwrap().state, DownloadState::Active);
        queue.pause(1, id).unwrap(); // stop the task so it doesn't keep writing
        tokio::time::sleep(Duration::from_millis(100)).await; // let it finish
        db.lock().unwrap().set_download_state(id, DownloadState::Active, None).unwrap();
        id
    };
    let (queue, _db, _store) = setup(dir.path(), 1);
    let row = queue.downloads(1).unwrap()[0].clone();
    assert_eq!(row.state, DownloadState::Queued, "active rows are requeued on start-up");
    let before = row.bytes_done;
    assert!(before > 0);
    queue
        .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::ZERO, report_every: 4, fail: false }))
        .unwrap();
    wait_for(&queue, id, DownloadState::Completed).await;
    assert_eq!(queue.downloads(1).unwrap()[0].bytes_done, SIZE);
}

const CHILD_ENV: &str = "CINMA_QUEUE_CRASH_CHILD";

/// Child for the kill -9 test: downloads slowly until killed.
#[test]
fn queue_crash_child() {
    let Ok(dir) = std::env::var(CHILD_ENV) else { return };
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    runtime.block_on(async {
        let (queue, _db, _store) = setup(std::path::Path::new(&dir), 1);
        queue
            .register_engine(DownloadKind::Http, Arc::new(FakeEngine { delay: Duration::from_millis(2), report_every: 3, fail: false }))
            .unwrap();
        if queue.downloads(1).unwrap().is_empty() {
            queue.add(&new_download(1, "a", 0)).unwrap();
        }
        println!("\nready");
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
}

#[test]
fn kill_9_never_leaves_a_row_ahead_of_its_blob() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    for round in 0..3 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "queue_crash_child", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // The child owns schema creation; wait until it's running.
        let stdout = child.stdout.take().unwrap();
        let mut lines = std::io::BufRead::lines(std::io::BufReader::new(stdout));
        assert!(lines.any(|l| l.is_ok_and(|l| l.trim() == "ready")), "child started");
        // Wait (reading through a separate connection) until progress moved
        // past where the previous round left it.
        let last = Database::open(&dir.path().join("db.sqlite3"))
            .ok()
            .and_then(|db| db.downloads(1).ok())
            .and_then(|rows| rows.first().map(|r| r.bytes_done))
            .unwrap_or(0);
        for _ in 0..500 {
            std::thread::sleep(Duration::from_millis(10));
            if let Ok(db) = Database::open(&dir.path().join("db.sqlite3"))
                && let Ok(rows) = db.downloads(1)
                && let Some(row) = rows.first()
                && row.bytes_done > last + 256 * 1024
            {
                break;
            }
        }
        child.kill().unwrap();
        child.wait().unwrap();

        let db = Database::open(&dir.path().join("db.sqlite3")).unwrap();
        let ok: String = db.connection().query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
        assert_eq!(ok, "ok", "round {round}");
        let row = db.downloads(1).unwrap()[0].clone();
        let store = BlobStore::open(&dir.path().join("blobs")).unwrap();
        let blob = store.open_blob(&cinma_blobstore::BlobId::parse(row.blob_id.as_deref().unwrap()).unwrap()).unwrap();
        let durable = blob.available().covered();
        assert!(row.bytes_done <= durable, "round {round}: row says {} but blob has {durable}", row.bytes_done);
        assert!(row.bytes_done > 0 && durable < SIZE, "round {round}: killed mid-download");
        let _ = RangeSet::new();
    }
}
