//! Byte storage and the sources the player reads from.
//!
//! * [`ByteSource`] / [`SourceReader`]: what the player bridge consumes. A
//!   source can wrap another source (slot for archive playback, §5.1).
//! * [`BlobStore`] / [`Blob`]: sparse files with durable availability ranges
//!   that engines write into (docs/adr/0009).
//! * [`fetch_order`]: head/tail-first fetch planning for engines.
//!
//! Every blocking read is cancellable and wakes on data or cancel, never by
//! polling.

#![forbid(unsafe_code)]

pub mod archive;
mod blob;
mod files;
mod hash;
mod plan;
mod ranges;
mod wait;

use std::io;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
pub use tokio_util::sync::CancellationToken;

pub use archive::{ArchiveByteSource, ArchiveEntry, ArchiveKind, detect_archive_kind};

pub use blob::{Blob, BlobId, BlobStore, DEFAULT_READAHEAD, PriorityListener, ReadHint};
pub use files::{FileSource, move_file_no_overwrite, sanitize_file_name, unique_destination};
pub use hash::{
    OPENSUBTITLES_CHUNK_SIZE, compute_opensubtitles_hash_file,
    compute_opensubtitles_hash_nonblocking, opensubtitles_hash_from_chunks,
};
pub use plan::{HEAD_BYTES, TAIL_BYTES, fetch_order};
pub use ranges::RangeSet;
use wait::wait_until;

/// Live transfer metrics for a [`ByteSource`] (e.g. active BitTorrent stream).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceStats {
    pub is_torrent: bool,
    pub info_hash: Option<String>,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub peers_connected: u32,
    pub peers_queued: u32,
    pub peers_seen: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub finished: bool,
}

/// Something with a known size that can be read at arbitrary offsets.
pub trait ByteSource: Send + Sync + 'static {
    fn size(&self) -> u64;
    /// Opens an independent reader (one per player stream).
    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>>;
    /// Short human-readable label for logs.
    fn describe(&self) -> String;
    /// Original file name, when known (used for subtitle queries & matching).
    fn filename(&self) -> Option<&str> {
        None
    }
    /// Snapshot of byte ranges currently available without network I/O.
    fn available_ranges(&self) -> Option<RangeSet> {
        None
    }
    /// Non-blocking live transfer & peer counters when supported.
    fn stats(&self) -> Option<SourceStats> {
        None
    }
    /// Non-blocking read of already-available bytes only. Must NEVER block
    /// waiting for peers/network or send priority hints (safe for thumbnails
    /// and OpenSubtitles hashing). Returns `None` if `offset` is unavailable.
    fn try_read_at(&self, _offset: u64, _buf: &mut [u8]) -> Option<usize> {
        None
    }
}

pub trait SourceReader: Send {
    /// Reads up to `buf.len()` bytes at `offset`, blocking until at least one
    /// byte is available. Returns `Ok(0)` only at end of source and
    /// `ErrorKind::Interrupted` once `cancel` fires.
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cancel: &CancellationToken,
    ) -> io::Result<usize>;
}

pub fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "read cancelled")
}

/// In-memory source whose bytes become readable progressively. Reads past the
/// available prefix block, like a torrent piece that hasn't arrived. Used in
/// tests.
pub struct MemorySource {
    data: Arc<Vec<u8>>,
    available: Arc<(Mutex<u64>, Notify)>,
}

impl MemorySource {
    pub fn new(data: Vec<u8>, available: u64) -> Self {
        Self {
            data: Arc::new(data),
            available: Arc::new((Mutex::new(available), Notify::new())),
        }
    }

    pub fn set_available(&self, available: u64) {
        let (lock, notify) = &*self.available;
        if let Ok(mut guard) = lock.lock() {
            *guard = available;
        }
        notify.notify_waiters();
    }
}

impl ByteSource for MemorySource {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }

    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>> {
        Ok(Box::new(MemoryReader {
            data: Arc::clone(&self.data),
            available: Arc::clone(&self.available),
        }))
    }

    fn describe(&self) -> String {
        format!("memory({} bytes)", self.data.len())
    }

    fn available_ranges(&self) -> Option<RangeSet> {
        let avail = self.available.0.lock().ok().map(|a| *a).unwrap_or(0);
        let mut set = RangeSet::new();
        if avail > 0 {
            set.insert(0..avail.min(self.data.len() as u64));
        }
        Some(set)
    }

    fn try_read_at(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        let size = self.data.len() as u64;
        if offset >= size {
            return Some(0);
        }
        if buf.is_empty() {
            return Some(0);
        }
        let avail = self.available.0.lock().ok().map(|a| *a)?;
        if avail <= offset {
            return None;
        }
        let end = avail.min(size).min(offset + buf.len() as u64);
        let n = (end - offset) as usize;
        let start = offset as usize;
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Some(n)
    }
}

struct MemoryReader {
    data: Arc<Vec<u8>>,
    available: Arc<(Mutex<u64>, Notify)>,
}

impl SourceReader for MemoryReader {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        let size = self.data.len() as u64;
        if offset >= size || buf.is_empty() {
            return Ok(0);
        }
        let (lock, notify) = &*self.available;
        let available = wait_until(notify, cancel, || {
            lock.lock().ok().map(|a| *a).filter(|&a| a > offset)
        })?;
        let end = available.min(size).min(offset + buf.len() as u64);
        let n = (end - offset) as usize;
        let start = offset as usize;
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn reads_available_range() {
        let source = MemorySource::new(bytes(1000), 1000);
        let mut reader = source.open_reader().unwrap();
        let mut buf = [0u8; 10];
        let n = reader
            .read_at(500, &mut buf, &CancellationToken::new())
            .unwrap();
        assert_eq!(n, 10);
        assert_eq!(buf[0], (500 % 251) as u8);
    }

    #[test]
    fn short_read_at_availability_edge_and_eof() {
        let source = MemorySource::new(bytes(100), 100);
        let mut reader = source.open_reader().unwrap();
        let mut buf = [0u8; 64];
        assert_eq!(
            reader
                .read_at(80, &mut buf, &CancellationToken::new())
                .unwrap(),
            20
        );
        assert_eq!(
            reader
                .read_at(100, &mut buf, &CancellationToken::new())
                .unwrap(),
            0
        );
    }

    #[test]
    fn blocked_read_resumes_when_data_arrives() {
        let source = Arc::new(MemorySource::new(bytes(1000), 0));
        let mut reader = source.open_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 8];
            tx.send(reader.read_at(0, &mut buf, &CancellationToken::new()))
                .unwrap();
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "must block"
        );
        source.set_available(4);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(), 4);
        handle.join().unwrap();
    }

    #[test]
    fn cancel_unblocks_pending_read() {
        let source = MemorySource::new(bytes(1000), 0);
        let mut reader = source.open_reader().unwrap();
        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 8];
            tx.send(reader.read_at(0, &mut buf, &cancel2)).unwrap();
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "must block"
        );
        cancel.cancel();
        let err = rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        handle.join().unwrap();
    }
}
