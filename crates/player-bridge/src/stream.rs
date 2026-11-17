//! `cinma://<id>` protocol: mpv reads bytes from our `ByteSource`s through
//! `mpv_stream_cb_add_ro`, so no localhost HTTP server is involved.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use cinma_blobstore::{ByteSource, CancellationToken, SourceReader};

use crate::sys;

pub const PROTOCOL: &str = "cinma";

#[derive(Default)]
pub struct Registry {
    sources: Mutex<HashMap<u64, Arc<dyn ByteSource>>>,
    next_id: Mutex<u64>,
}

impl Registry {
    /// Replaces all registered sources with `source`; returns its URI.
    pub fn register_exclusive(&self, source: Arc<dyn ByteSource>) -> String {
        let id = {
            let mut next = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
            *next += 1;
            *next
        };
        let mut sources = self.sources.lock().unwrap_or_else(|e| e.into_inner());
        sources.clear();
        sources.insert(id, source);
        format!("{PROTOCOL}://{id}")
    }

    pub fn clear(&self) {
        self.sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    fn get(&self, uri: &str) -> Option<Arc<dyn ByteSource>> {
        let id: u64 = uri
            .strip_prefix(PROTOCOL)?
            .strip_prefix("://")?
            .parse()
            .ok()?;
        self.sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }
}

struct Cookie {
    size: u64,
    cancel: CancellationToken,
    state: Mutex<ReaderState>,
}

struct ReaderState {
    reader: Box<dyn SourceReader>,
    position: u64,
}

/// # Safety
/// `user_data` must be a pointer obtained from `Arc::<Registry>::as_ptr` that
/// stays alive while the mpv handle exists.
pub unsafe extern "C" fn open_fn(
    user_data: *mut c_void,
    uri: *mut c_char,
    info: *mut sys::mpv_stream_cb_info,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: see function contract; mpv passes a valid NUL-terminated URI
        // and a writable info struct.
        let registry = unsafe { &*(user_data as *const Registry) };
        let uri = unsafe { CStr::from_ptr(uri) }.to_string_lossy();
        let Some(source) = registry.get(&uri) else {
            eprintln!("[cinma] stream: unknown source {uri}");
            return sys::ERROR_LOADING_FAILED;
        };
        let reader = match source.open_reader() {
            Ok(reader) => reader,
            Err(e) => {
                eprintln!("[cinma] stream: open {} failed: {e}", source.describe());
                return sys::ERROR_LOADING_FAILED;
            }
        };
        let cookie = Box::new(Cookie {
            size: source.size(),
            cancel: CancellationToken::new(),
            state: Mutex::new(ReaderState {
                reader,
                position: 0,
            }),
        });
        let info = unsafe { &mut *info };
        info.cookie = Box::into_raw(cookie).cast();
        info.read_fn = Some(read_fn);
        info.seek_fn = Some(seek_fn);
        info.size_fn = Some(size_fn);
        info.close_fn = Some(close_fn);
        info.cancel_fn = Some(cancel_fn);
        0
    }));
    result.unwrap_or(sys::ERROR_LOADING_FAILED)
}

/// # Safety
/// `cookie` was produced by [`open_fn`] and not yet closed.
unsafe fn cookie<'a>(cookie: *mut c_void) -> &'a Cookie {
    unsafe { &*(cookie as *const Cookie) }
}

unsafe extern "C" fn read_fn(cookie_ptr: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let cookie = unsafe { cookie(cookie_ptr) };
        if cookie.cancel.is_cancelled() {
            return -1;
        }
        let Ok(mut state) = cookie.state.lock() else {
            return -1;
        };
        if state.position >= cookie.size {
            return 0;
        }
        let len = nbytes.min(cookie.size - state.position) as usize;
        // SAFETY: mpv hands us a writable buffer of `nbytes` bytes.
        let slice = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), len) };
        let position = state.position;
        match state.reader.read_at(position, slice, &cookie.cancel) {
            Ok(n) => {
                state.position += n as u64;
                n as i64
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => -1,
            Err(e) => {
                eprintln!("[cinma] stream: read at {position} failed: {e}");
                -1
            }
        }
    }));
    result.unwrap_or(-1)
}

unsafe extern "C" fn seek_fn(cookie_ptr: *mut c_void, offset: i64) -> i64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let cookie = unsafe { cookie(cookie_ptr) };
        if offset < 0 || cookie.cancel.is_cancelled() {
            return sys::ERROR_GENERIC;
        }
        let Ok(mut state) = cookie.state.lock() else {
            return sys::ERROR_GENERIC;
        };
        state.position = offset as u64;
        offset
    }));
    result.unwrap_or(sys::ERROR_GENERIC)
}

unsafe extern "C" fn size_fn(cookie_ptr: *mut c_void) -> i64 {
    let cookie = unsafe { cookie(cookie_ptr) };
    cookie.size as i64
}

unsafe extern "C" fn cancel_fn(cookie_ptr: *mut c_void) {
    // Called from another thread while a read may be blocked; must not block.
    let cookie = unsafe { cookie(cookie_ptr) };
    cookie.cancel.cancel();
}

unsafe extern "C" fn close_fn(cookie_ptr: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: mpv calls close exactly once, after all other callbacks.
        drop(unsafe { Box::from_raw(cookie_ptr as *mut Cookie) });
    }));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use cinma_blobstore::MemorySource;
    use std::ffi::CString;
    use std::ptr;
    use std::time::Duration;

    fn open(registry: &Arc<Registry>, uri: &str) -> (c_int, sys::mpv_stream_cb_info) {
        let mut info = sys::mpv_stream_cb_info {
            cookie: ptr::null_mut(),
            read_fn: None,
            seek_fn: None,
            size_fn: None,
            close_fn: None,
            cancel_fn: None,
        };
        let uri = CString::new(uri).unwrap();
        let rc = unsafe {
            open_fn(
                Arc::as_ptr(registry) as *mut c_void,
                uri.as_ptr() as *mut c_char,
                &mut info,
            )
        };
        (rc, info)
    }

    #[test]
    fn reads_seeks_and_closes_through_callbacks() {
        let registry = Arc::new(Registry::default());
        let data: Vec<u8> = (0..100u8).collect();
        let uri = registry.register_exclusive(Arc::new(MemorySource::new(data, 100)));
        let (rc, info) = open(&registry, &uri);
        assert_eq!(rc, 0);
        unsafe {
            assert_eq!(info.size_fn.unwrap()(info.cookie), 100);
            assert_eq!(info.seek_fn.unwrap()(info.cookie, 90), 90);
            let mut buf = [0 as c_char; 32];
            assert_eq!(info.read_fn.unwrap()(info.cookie, buf.as_mut_ptr(), 32), 10);
            assert_eq!(buf[0] as u8, 90);
            assert_eq!(
                info.read_fn.unwrap()(info.cookie, buf.as_mut_ptr(), 32),
                0,
                "EOF"
            );
            info.close_fn.unwrap()(info.cookie);
        }
    }

    #[test]
    fn unknown_uri_fails_to_open() {
        let registry = Arc::new(Registry::default());
        assert_eq!(open(&registry, "cinma://999").0, sys::ERROR_LOADING_FAILED);
        assert_eq!(open(&registry, "http://x").0, sys::ERROR_LOADING_FAILED);
    }

    #[test]
    fn cancel_unblocks_a_blocked_read() {
        let registry = Arc::new(Registry::default());
        let uri = registry.register_exclusive(Arc::new(MemorySource::new(vec![0; 100], 0)));
        let (rc, info) = open(&registry, &uri);
        assert_eq!(rc, 0);
        let cookie = info.cookie as usize;
        let read = info.read_fn.unwrap();
        let reader = std::thread::spawn(move || {
            let mut buf = [0 as c_char; 8];
            unsafe { read(cookie as *mut c_void, buf.as_mut_ptr(), 8) }
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !reader.is_finished(),
            "read must block until data or cancel"
        );
        unsafe { info.cancel_fn.unwrap()(info.cookie) };
        assert_eq!(reader.join().unwrap(), -1);
        unsafe {
            let mut buf = [0 as c_char; 8];
            assert_eq!(
                info.read_fn.unwrap()(info.cookie, buf.as_mut_ptr(), 8),
                -1,
                "stays cancelled"
            );
            info.close_fn.unwrap()(info.cookie);
        }
    }
}
