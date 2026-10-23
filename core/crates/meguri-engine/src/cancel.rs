//! The cancel token (`engine/jobs` J4, KTD9): an atomic stop cause that blocking
//! work polls between steps, plus a future that async work (sync, on tokio)
//! awaits. Swift task cancellation never reaches Rust: meguri-ffi exports
//! this token and Swift cancels only through it.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use crate::error::EngineError;

/// The first stop request wins, including races with lifecycle shutdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopCause {
    User,
    System,
}

#[derive(Debug, Default)]
pub struct CancelToken {
    cause: AtomicU8,
    wakers: Mutex<Vec<Waker>>,
}

impl CancelToken {
    pub fn new() -> Arc<CancelToken> {
        Arc::new(CancelToken::default())
    }

    /// Requests cancellation and wakes every task waiting in
    /// [`CancelToken::cancelled`]. Idempotent; never blocks on the work.
    pub fn cancel(&self) {
        self.stop(1);
    }

    /// Records a system stop when no earlier request has won. Waiters wake
    /// immediately; the job owner remains responsible for draining its work.
    pub fn interrupt(&self) {
        self.stop(2);
    }

    pub fn stop_cause(&self) -> Option<StopCause> {
        match self.cause.load(Ordering::SeqCst) {
            0 => None,
            1 => Some(StopCause::User),
            _ => Some(StopCause::System),
        }
    }

    fn stop(&self, cause: u8) {
        let _ = self
            .cause
            .compare_exchange(0, cause, Ordering::SeqCst, Ordering::SeqCst);
        let wakers = std::mem::take(&mut *self.wakers.lock().unwrap_or_else(PoisonError::into_inner));
        for waker in wakers {
            waker.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.stop_cause().is_some()
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_stop_cause_is_retained_across_user_and_system_requests() {
        for system_first in [false, true] {
            let token = CancelToken::new();
            assert_eq!(token.stop_cause(), None);
            if system_first {
                token.interrupt();
                token.cancel();
            } else {
                token.cancel();
                token.interrupt();
            }
            assert!(token.is_cancelled());
            assert_eq!(
                token.stop_cause(),
                Some(if system_first {
                    StopCause::System
                } else {
                    StopCause::User
                })
            );
            assert_eq!(token.check(), Err(EngineError::Cancelled));
        }
    }
}
