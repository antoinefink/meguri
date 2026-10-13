//! The explicit cancellation token (KTD9). Swift task cancellation does not
//! reach Rust, so every cancellable call takes a `CancelToken` and Swift calls
//! `cancel()` on it. Blocking work polls [`CancelToken::check`]; async work
//! (sync, on tokio) awaits [`CancelToken::cancelled`].

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use crate::callback;
use crate::error::CoreError;

#[derive(Debug, Default, uniffi::Object)]
pub struct CancelToken {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}

#[uniffi::export]
impl CancelToken {
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, CoreError> {
        callback::enter("CancelToken::new");
        Ok(CancelToken::new_arc())
    }

    /// Requests cancellation. Idempotent; never blocks.
    pub fn cancel(&self) -> Result<(), CoreError> {
        callback::enter("CancelToken::cancel");
        self.trigger();
        Ok(())
    }

    pub fn is_cancelled(&self) -> Result<bool, CoreError> {
        callback::enter("CancelToken::is_cancelled");
        Ok(self.requested())
    }
}

impl CancelToken {
    pub fn new_arc() -> Arc<CancelToken> {
        Arc::new(CancelToken::default())
    }

    /// Sets the flag and wakes every task waiting in [`CancelToken::cancelled`].
    pub fn trigger(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let wakers = std::mem::take(&mut *self.wakers.lock().unwrap_or_else(PoisonError::into_inner));
        for waker in wakers {
            waker.wake();
        }
    }

    pub fn requested(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// `Err(CoreError::Cancelled)` once cancellation was requested.
    pub fn check(&self) -> Result<(), CoreError> {
        if self.requested() {
            Err(CoreError::Cancelled)
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
        if self.token.requested() {
            return Poll::Ready(());
        }
        let mut wakers = self.token.wakers.lock().unwrap_or_else(PoisonError::into_inner);
        // trigger() sets the flag before it takes the wakers under this lock,
        // so checking again here cannot miss a cancel.
        if self.token.requested() {
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
    use std::sync::mpsc;
    use std::task::Wake;
    use std::thread;
    use std::time::Duration;

    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
                return out;
            }
            thread::park();
        }
    }

    #[test]
    fn check_fails_with_cancelled_only_after_cancel() {
        let token = CancelToken::new().unwrap();
        assert_eq!(token.check(), Ok(()));
        assert_eq!(token.is_cancelled(), Ok(false));
        token.cancel().unwrap();
        assert_eq!(token.check(), Err(CoreError::Cancelled));
        assert_eq!(token.is_cancelled(), Ok(true));
        token.cancel().unwrap();
        assert_eq!(token.check(), Err(CoreError::Cancelled));
    }

    #[test]
    fn the_async_token_completes_when_another_thread_cancels() {
        let token = CancelToken::new_arc();
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = {
            let token = Arc::clone(&token);
            thread::spawn(move || {
                block_on(token.cancelled());
                done_tx.send(()).unwrap();
            })
        };
        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "completed before cancel"
        );
        token.cancel().unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the waiter was not woken by cancel");
        waiter.join().unwrap();
    }

    #[test]
    fn the_async_token_is_ready_at_once_when_already_cancelled() {
        let token = CancelToken::new_arc();
        token.trigger();
        block_on(token.cancelled());
    }
}
