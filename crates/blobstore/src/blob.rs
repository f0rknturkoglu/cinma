//! On-disk blobs: a sparse data file plus a sidecar recording which byte
//! ranges are durable. SQLite stays the source of truth for *what* a blob is
//! (download rows reference it); the sidecar only says which bytes to trust.
//!
//! Crash model (docs/adr/0009): `write_at` and `mark_available` are separate
//! so an engine can verify (e.g. a hash) before exposing bytes. `checkpoint`
//! snapshots the marked ranges, fsyncs the data file, then atomically replaces
//! the sidecar, so after a process kill the sidecar never claims bytes that
//! weren't written. Power loss behaviour depends on the disk honouring fsync.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::wait::wait_until;
use crate::{ByteSource, RangeSet, SourceReader};

const DATA_EXT: &str = "blob";
const META_EXT: &str = "ranges";
const META_VERSION: u32 = 1;
/// Read-ahead hint sent with every read.
pub const DEFAULT_READAHEAD: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobId(String);

impl BlobId {
    pub fn parse(s: &str) -> io::Result<Self> {
        if !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(s.to_ascii_lowercase()))
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid blob id {s:?}")))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn generate() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!("{nanos:x}{:08x}{count:04x}", std::process::id()))
    }
}

/// What a reader is about to need; sent to the owning engine on every read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadHint {
    pub offset: u64,
    pub len: u64,
    pub readahead: u64,
}

pub trait PriorityListener: Send + Sync {
    fn on_read(&self, blob: &BlobId, hint: ReadHint);
}

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    size: u64,
    ranges: RangeSet,
}

pub struct Blob {
    id: BlobId,
    size: u64,
    file: File,
    meta_path: PathBuf,
    dir: PathBuf,
    ranges: Mutex<RangeSet>,
    notify: Notify,
    listener: Mutex<Option<Arc<dyn PriorityListener>>>,
    checkpoint_lock: Mutex<()>,
}

impl Blob {
    pub fn id(&self) -> &BlobId {
        &self.id
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// Writes bytes without exposing them to readers (see `mark_available`).
    pub fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let end = offset
            .checked_add(data.len() as u64)
            .filter(|&end| end <= self.size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "write past end of blob"))?;
        let _ = end;
        self.file.write_all_at(data, offset)
    }

    /// Exposes `range` to readers. Call only after its bytes were written.
    pub fn mark_available(&self, range: std::ops::Range<u64>) {
        let range = range.start.min(self.size)..range.end.min(self.size);
        if let Ok(mut ranges) = self.ranges.lock() {
            ranges.insert(range);
        }
        self.notify.notify_waiters();
    }

    /// `write_at` + `mark_available` for engines that need no verification.
    pub fn write_available(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.write_at(offset, data)?;
        self.mark_available(offset..offset + data.len() as u64);
        Ok(())
    }

    pub fn available(&self) -> RangeSet {
        self.ranges.lock().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn is_complete(&self) -> bool {
        self.available().contains_range(0..self.size)
    }

    pub fn set_priority_listener(&self, listener: Option<Arc<dyn PriorityListener>>) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = listener;
        }
    }

    /// Makes everything marked so far durable. Returns the durable byte count,
    /// which is what may be recorded elsewhere (e.g. a download row).
    pub fn checkpoint(&self) -> io::Result<u64> {
        let _guard = self.checkpoint_lock.lock().map_err(|_| io::Error::other("poisoned"))?;
        // Snapshot first: everything in it was written before being marked.
        let snapshot = self.available();
        self.file.sync_data()?;
        write_meta_atomically(&self.dir, &self.meta_path, &Meta {
            version: META_VERSION,
            size: self.size,
            ranges: snapshot.clone(),
        })?;
        Ok(snapshot.covered())
    }

    /// A `ByteSource` reading this blob (blocks on missing ranges).
    pub fn source(self: &Arc<Self>) -> Arc<dyn ByteSource> {
        Arc::new(BlobSource(Arc::clone(self)))
    }
}

fn write_meta_atomically(dir: &Path, path: &Path, meta: &Meta) -> io::Result<()> {
    let tmp = path.with_extension(format!("{META_EXT}.tmp"));
    let json = serde_json::to_vec(meta).map_err(io::Error::other)?;
    {
        let mut file = File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    File::open(dir)?.sync_all()
}

struct BlobSource(Arc<Blob>);

impl ByteSource for BlobSource {
    fn size(&self) -> u64 {
        self.0.size
    }

    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>> {
        Ok(Box::new(BlobReader { blob: Arc::clone(&self.0) }))
    }

    fn describe(&self) -> String {
        format!("blob {}", self.0.id.as_str())
    }
}

struct BlobReader {
    blob: Arc<Blob>,
}

impl SourceReader for BlobReader {
    fn read_at(&mut self, offset: u64, buf: &mut [u8], cancel: &CancellationToken) -> io::Result<usize> {
        let blob = &self.blob;
        if offset >= blob.size || buf.is_empty() {
            return Ok(0);
        }
        let listener = blob.listener.lock().ok().and_then(|l| l.clone());
        if let Some(listener) = listener {
            listener.on_read(&blob.id, ReadHint {
                offset,
                len: buf.len() as u64,
                readahead: DEFAULT_READAHEAD,
            });
        }
        let run_end = wait_until(&blob.notify, cancel, || {
            blob.ranges.lock().ok().and_then(|r| r.run_end(offset))
        })?;
        let n = (run_end - offset).min(buf.len() as u64) as usize;
        blob.file.read_exact_at(&mut buf[..n], offset)?;
        Ok(n)
    }
}

/// Directory of blobs. One live `Blob` per id (shared range state).
pub struct BlobStore {
    root: PathBuf,
    open: Mutex<HashMap<BlobId, Weak<Blob>>>,
}

impl BlobStore {
    pub fn open(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        // Leftovers of interrupted checkpoints.
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "tmp") {
                let _ = fs::remove_file(path);
            }
        }
        Ok(Self { root: root.to_path_buf(), open: Mutex::new(HashMap::new()) })
    }

    fn paths(&self, id: &BlobId) -> (PathBuf, PathBuf) {
        (
            self.root.join(format!("{}.{DATA_EXT}", id.as_str())),
            self.root.join(format!("{}.{META_EXT}", id.as_str())),
        )
    }

    /// Creates an empty (sparse) blob of `size` bytes.
    pub fn create(&self, size: u64) -> io::Result<Arc<Blob>> {
        let id = BlobId::generate();
        let (data_path, meta_path) = self.paths(&id);
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&data_path)?;
        file.set_len(size)?;
        let blob = Arc::new(self.make(id.clone(), size, file, meta_path, RangeSet::new()));
        blob.checkpoint()?;
        self.registry()?.insert(id, Arc::downgrade(&blob));
        Ok(blob)
    }

    pub fn open_blob(&self, id: &BlobId) -> io::Result<Arc<Blob>> {
        let mut registry = self.registry()?;
        if let Some(blob) = registry.get(id).and_then(Weak::upgrade) {
            return Ok(blob);
        }
        let (data_path, meta_path) = self.paths(id);
        let meta: Meta = serde_json::from_slice(&fs::read(&meta_path)?).map_err(io::Error::other)?;
        if meta.version != META_VERSION || !meta.ranges.is_valid_for(meta.size) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "corrupt blob metadata"));
        }
        let file = OpenOptions::new().read(true).write(true).open(&data_path)?;
        if file.metadata()?.len() != meta.size {
            file.set_len(meta.size)?;
        }
        let blob = Arc::new(self.make(id.clone(), meta.size, file, meta_path, meta.ranges));
        registry.insert(id.clone(), Arc::downgrade(&blob));
        Ok(blob)
    }

    pub fn remove(&self, id: &BlobId) -> io::Result<()> {
        self.registry()?.remove(id);
        let (data_path, meta_path) = self.paths(id);
        for path in [meta_path, data_path] {
            match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn ids(&self) -> io::Result<Vec<BlobId>> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == META_EXT)
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(id) = BlobId::parse(stem)
            {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn make(&self, id: BlobId, size: u64, file: File, meta_path: PathBuf, ranges: RangeSet) -> Blob {
        Blob {
            id,
            size,
            file,
            meta_path,
            dir: self.root.clone(),
            ranges: Mutex::new(ranges),
            notify: Notify::new(),
            listener: Mutex::new(None),
            checkpoint_lock: Mutex::new(()),
        }
    }

    fn registry(&self) -> io::Result<std::sync::MutexGuard<'_, HashMap<BlobId, Weak<Blob>>>> {
        self.open.lock().map_err(|_| io::Error::other("poisoned"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn pattern(offset: u64, len: usize) -> Vec<u8> {
        (0..len as u64).map(|i| ((offset + i) * 31 % 251) as u8).collect()
    }

    #[test]
    fn write_mark_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let blob = store.create(1000).unwrap();
        blob.write_available(100, &pattern(100, 50)).unwrap();
        let mut reader = blob.source().open_reader().unwrap();
        let mut buf = [0u8; 80];
        let n = reader.read_at(120, &mut buf, &CancellationToken::new()).unwrap();
        assert_eq!(n, 30, "short read up to the end of the available run");
        assert_eq!(&buf[..30], &pattern(120, 30)[..]);
        assert_eq!(reader.read_at(1000, &mut buf, &CancellationToken::new()).unwrap(), 0);
        assert!(blob.write_at(990, &[0; 20]).is_err());
    }

    #[test]
    fn written_but_unmarked_bytes_are_not_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let blob = store.create(100).unwrap();
        blob.write_at(0, &[7; 10]).unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut reader = blob.source().open_reader().unwrap();
        assert!(reader.read_at(0, &mut [0; 4], &cancel).is_err());
        blob.mark_available(0..10);
        assert_eq!(reader.read_at(0, &mut [0; 4], &CancellationToken::new()).unwrap(), 4);
    }

    #[test]
    fn blocked_read_wakes_on_data_and_on_cancel_promptly() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let blob = store.create(100).unwrap();

        let source = blob.source();
        let (tx, rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || {
            let mut reader = source.open_reader().unwrap();
            let mut buf = [0u8; 8];
            tx.send(reader.read_at(40, &mut buf, &CancellationToken::new()).map(|n| (n, buf))).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err(), "must block");
        blob.write_available(40, &pattern(40, 8)).unwrap();
        let (n, buf) = rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!((n, &buf[..]), (8, &pattern(40, 8)[..]));
        reader_thread.join().unwrap();

        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        let source = blob.source();
        let (tx, rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || {
            let mut reader = source.open_reader().unwrap();
            tx.send(reader.read_at(90, &mut [0u8; 4], &cancel2).map_err(|e| e.kind())).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err(), "must block");
        let started = Instant::now();
        cancel.cancel();
        let result = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(result, Err(io::ErrorKind::Interrupted));
        assert!(started.elapsed() < Duration::from_millis(50), "cancel wakes immediately");
        reader_thread.join().unwrap();
    }

    #[test]
    fn every_read_sends_a_priority_hint() {
        struct Rec(Mutex<Vec<ReadHint>>);
        impl PriorityListener for Rec {
            fn on_read(&self, _: &BlobId, hint: ReadHint) {
                self.0.lock().unwrap().push(hint);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let blob = store.create(100).unwrap();
        blob.write_available(0, &[1; 100]).unwrap();
        let rec = Arc::new(Rec(Mutex::new(Vec::new())));
        blob.set_priority_listener(Some(rec.clone()));
        let mut reader = blob.source().open_reader().unwrap();
        reader.read_at(10, &mut [0; 5], &CancellationToken::new()).unwrap();
        reader.read_at(60, &mut [0; 7], &CancellationToken::new()).unwrap();
        let hints = rec.0.lock().unwrap().clone();
        assert_eq!(hints.iter().map(|h| (h.offset, h.len)).collect::<Vec<_>>(), vec![(10, 5), (60, 7)]);
    }

    #[test]
    fn reopen_trusts_only_checkpointed_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let store = BlobStore::open(dir.path()).unwrap();
            let blob = store.create(1000).unwrap();
            blob.write_available(0, &pattern(0, 100)).unwrap();
            assert_eq!(blob.checkpoint().unwrap(), 100);
            blob.write_available(500, &pattern(500, 100)).unwrap(); // not checkpointed
            blob.id().clone()
        };
        let store = BlobStore::open(dir.path()).unwrap();
        let blob = store.open_blob(&id).unwrap();
        assert_eq!(blob.available().iter().collect::<Vec<_>>(), vec![0..100]);
        assert_eq!(store.ids().unwrap(), vec![id.clone()]);
        assert!(Arc::ptr_eq(&blob, &store.open_blob(&id).unwrap()), "one live blob per id");
        store.remove(&id).unwrap();
        assert!(store.ids().unwrap().is_empty());
    }

    #[test]
    fn rejects_path_like_ids() {
        assert!(BlobId::parse("../etc").is_err());
        assert!(BlobId::parse("").is_err());
        assert!(BlobId::parse("abc123").is_ok());
    }
}
