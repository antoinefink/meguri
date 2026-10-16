//! The explicit cancellation token (KTD9): the engine's
//! [`meguri_engine::CancelToken`] exported to Swift. Swift task cancellation
//! does not reach Rust, so every cancellable call takes a `CancelToken` and
//! Swift calls `cancel()` on it. Blocking work polls [`CancelToken::check`];
//! async work (sync, on tokio) awaits [`CancelToken::cancelled`].

use std::sync::Arc;

use meguri_engine::cancel::Cancelled;

use crate::callback;
use crate::error::CoreError;

#[derive(Debug, uniffi::Object)]
pub struct CancelToken {
    inner: Arc<meguri_engine::CancelToken>,
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
        CancelToken::wrap(meguri_engine::CancelToken::new())
    }

    /// The exported face of an engine token (a job's, an op's).
    pub fn wrap(inner: Arc<meguri_engine::CancelToken>) -> Arc<CancelToken> {
        Arc::new(CancelToken { inner })
    }

    /// The engine token, for op specs and jobs.
    pub fn engine_token(&self) -> &Arc<meguri_engine::CancelToken> {
        &self.inner
    }

    /// Sets the flag and wakes every task waiting in [`CancelToken::cancelled`].
    pub fn trigger(&self) {
        self.inner.cancel();
    }

    pub fn requested(&self) -> bool {
        self.inner.is_cancelled()
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
        self.inner.cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::mpsc;
    use std::task::{Context, Poll, Wake, Waker};
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
    fn an_exported_token_is_the_engine_token_it_wraps() {
        let engine = meguri_engine::CancelToken::new();
        let exported = CancelToken::wrap(Arc::clone(&engine));
        exported.cancel().unwrap();
        assert!(engine.is_cancelled());
        assert!(Arc::ptr_eq(exported.engine_token(), &engine));
    }
}
