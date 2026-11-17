//! Blocking waits for synchronous callers (mpv's demuxer thread) that wake on
//! data or cancellation — never by polling.

use std::future::Future;
use std::io;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::cancelled_error;

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Minimal executor: drives one future on the calling thread, parking between
/// polls. Only for runtime-free futures (`Notify`, `CancellationToken`).
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

/// Blocks until `ready` yields a value or `cancel` fires. `notify` must get
/// `notify_waiters()` whenever `ready` may have changed.
pub fn wait_until<T>(
    notify: &Notify,
    cancel: &CancellationToken,
    mut ready: impl FnMut() -> Option<T>,
) -> io::Result<T> {
    loop {
        let mut notified = pin!(notify.notified());
        // Register before checking so a notification in between isn't lost.
        notified.as_mut().enable();
        if let Some(value) = ready() {
            return Ok(value);
        }
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        block_on(async {
            tokio::select! {
                () = notified.as_mut() => {}
                () = cancel.cancelled() => {}
            }
        });
    }
}
