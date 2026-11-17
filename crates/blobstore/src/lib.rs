//! Byte sources the player reads from.
//!
//! Phase 1 keeps this deliberately small: a [`ByteSource`] hands out
//! independent [`SourceReader`]s, and every blocking read can be cancelled.
//! Sparse blobs, availability bitmaps and head/tail priority arrive in Phase 2;
//! composable sources (archive playback) wrap a `ByteSource` in another one.

#![forbid(unsafe_code)]

use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub use tokio_util::sync::CancellationToken;

/// Something with a known size that can be read at arbitrary offsets.
pub trait ByteSource: Send + Sync + 'static {
    fn size(&self) -> u64;
    /// Opens an independent reader (one per player stream).
    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>>;
    /// Short human-readable label for logs.
    fn describe(&self) -> String;
}

pub trait SourceReader: Send {
    /// Reads up to `buf.len()` bytes at `offset`, blocking until at least one
    /// byte is available. Returns `Ok(0)` only at end of source and
    /// `ErrorKind::Interrupted` once `cancel` fires.
    fn read_at(&mut self, offset: u64, buf: &mut [u8], cancel: &CancellationToken)
    -> io::Result<usize>;
}

pub fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "read cancelled")
}

/// In-memory source whose bytes become readable progressively. Reads past the
/// available prefix block, like a torrent piece that hasn't arrived. Used in
/// tests and as a reference implementation.
pub struct MemorySource {
    data: Arc<Vec<u8>>,
    available: Arc<(Mutex<u64>, Condvar)>,
}

impl MemorySource {
    pub fn new(data: Vec<u8>, available: u64) -> Self {
        Self {
            data: Arc::new(data),
            available: Arc::new((Mutex::new(available), Condvar::new())),
        }
    }

    pub fn set_available(&self, available: u64) {
        let (lock, cvar) = &*self.available;
        if let Ok(mut guard) = lock.lock() {
            *guard = available;
            cvar.notify_all();
        }
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
}

struct MemoryReader {
    data: Arc<Vec<u8>>,
    available: Arc<(Mutex<u64>, Condvar)>,
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
        let (lock, cvar) = &*self.available;
        let mut available = lock.lock().map_err(|_| io::Error::other("poisoned"))?;
        while *available <= offset {
            if cancel.is_cancelled() {
                return Err(cancelled_error());
            }
            // Condvar can't observe the token; wake periodically to check it.
            available = cvar
                .wait_timeout(available, Duration::from_millis(5))
                .map_err(|_| io::Error::other("poisoned"))?
                .0;
        }
        let end = (*available).min(size).min(offset + buf.len() as u64);
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

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn reads_available_range() {
        let source = MemorySource::new(bytes(1000), 1000);
        let mut reader = source.open_reader().unwrap();
        let mut buf = [0u8; 10];
        let n = reader.read_at(500, &mut buf, &CancellationToken::new()).unwrap();
        assert_eq!(n, 10);
        assert_eq!(buf[0], (500 % 251) as u8);
    }

    #[test]
    fn short_read_at_availability_edge_and_eof() {
        let source = MemorySource::new(bytes(100), 100);
        let mut reader = source.open_reader().unwrap();
        let mut buf = [0u8; 64];
        assert_eq!(reader.read_at(80, &mut buf, &CancellationToken::new()).unwrap(), 20);
        assert_eq!(reader.read_at(100, &mut buf, &CancellationToken::new()).unwrap(), 0);
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
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err(), "must block");
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
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err(), "must block");
        cancel.cancel();
        let err = rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        handle.join().unwrap();
    }
}
