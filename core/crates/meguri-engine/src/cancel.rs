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

#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    cause: Arc<AtomicU8>,
    wakers: Arc<Mutex<Vec<Waker>>>,
    deadline: Arc<Mutex<Option<std::time::Duration>>>,
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

    pub(crate) fn limit_until(&self, deadline: std::time::Duration) {
        *self.deadline.lock().unwrap_or_else(PoisonError::into_inner) = Some(deadline);
    }

    pub fn is_cancelled(&self) -> bool {
        if self
            .deadline
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|deadline| crate::monotonic::now() >= deadline)
        {
            self.interrupt();
        }
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

/// A serialization lane whose owner is retained until its guard drops. Waiting
/// callers share the token's cancellation wakeups and absolute deadline.
#[derive(Default)]
pub struct SerializationLane {
    inner: Arc<LaneState>,
}
#[derive(Default)]
struct LaneState {
    occupied: Mutex<bool>,
    changed: std::sync::Condvar,
}
struct LaneWake(std::sync::Weak<LaneState>);
impl std::task::Wake for LaneWake {
    fn wake(self: Arc<Self>) {
        if let Some(lane) = self.0.upgrade() {
            let _state = lane.occupied.lock().unwrap_or_else(PoisonError::into_inner);
            lane.changed.notify_all();
        }
    }
}
pub struct SerializationGuard<'a>(&'a SerializationLane);
impl SerializationLane {
    pub fn lock(&self, cancel: Option<&CancelToken>) -> Result<SerializationGuard<'_>, EngineError> {
        let wake = Waker::from(Arc::new(LaneWake(Arc::downgrade(&self.inner))));
        let mut occupied = self.inner.occupied.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(token) = cancel {
            token
                .wakers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(wake.clone());
        }
        let result = loop {
            if let Some(token) = cancel {
                let expired = token
                    .deadline
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some_and(|end| crate::monotonic::now() >= end);
                if token.stop_cause().is_some() || expired {
                    break Err(EngineError::Cancelled);
                }
            }
            if !*occupied {
                *occupied = true;
                break Ok(SerializationGuard(self));
            }
            let deadline = cancel.and_then(|token| *token.deadline.lock().unwrap_or_else(PoisonError::into_inner));
            occupied = match deadline {
                Some(end) => {
                    self.inner
                        .changed
                        .wait_timeout(occupied, end.saturating_sub(crate::monotonic::now()))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .inner
                    .changed
                    .wait(occupied)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        };
        drop(occupied);
        if let Some(token) = cancel {
            token
                .wakers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|w| !w.will_wake(&wake));
            if result.is_err() {
                token.check()?;
            }
        }
        result
    }
}
impl Drop for SerializationGuard<'_> {
    fn drop(&mut self) {
        *self.0.inner.occupied.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.0.inner.changed.notify_all();
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
        if !wakers.iter().any(|w| w.will_wake(cx.waker())) {
            wakers.push(cx.waker().clone());
        }
        drop(wakers);
        // A deadline check can itself interrupt the token and wake listeners.
        // Registration precedes this check, so cancellation cannot be lost.
        if self.token.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
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
