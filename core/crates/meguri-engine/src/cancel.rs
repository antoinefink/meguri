//! The cancel token (`engine/jobs` J4, KTD9): an atomic flag that blocking
//! work polls between steps, plus a future that async work (sync, on tokio)
//! awaits. Swift task cancellation never reaches Rust: meguri-ffi exports
//! this token and Swift cancels only through it.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use crate::error::EngineError;

#[derive(Debug, Default)]
pub struct CancelToken {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}

impl CancelToken {
    pub fn new() -> Arc<CancelToken> {
        Arc::new(CancelToken::default())
    }

    /// Requests cancellation and wakes every task waiting in
    /// [`CancelToken::cancelled`]. Idempotent; never blocks on the work.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let wakers = std::mem::take(&mut *self.wakers.lock().unwrap_or_else(PoisonError::into_inner));
        for waker in wakers {
            waker.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// `Err(Cancelled)` once cancellation was requested.
    pub fn check(&self) -> Result<(), EngineError> {
        if self.is_cancelled() {
            Err(EngineError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Completes once cancellation is requested.
    pub fn cancelled(&self) -> Cancelled<'_> {
        Cancelled { token: self }
    }
}

/// The future returned by [`CancelToken::cancelled`].
#[derive(Debug)]
pub struct Cancelled<'a> {
    token: &'a CancelToken,
}

impl Future for Cancelled<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.token.is_cancelled() {
            return Poll::Ready(());
        }
        let mut wakers = self.token.wakers.lock().unwrap_or_else(PoisonError::into_inner);
        // cancel() sets the flag before it takes the wakers under this lock,
        // so checking again here cannot miss a cancel.
        if self.token.is_cancelled() {
            return Poll::Ready(());
        }
        if !wakers.iter().any(|w| w.will_wake(cx.waker())) {
            wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}
