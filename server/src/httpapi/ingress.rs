// SPDX-License-Identifier: MIT
//! Browser-only work admission. Native sync never takes these slots.
use crate::{Error, Result, Service};
use axum::{
    body::{Body, Bytes},
    response::Response,
};
use http_body::{Body as HttpBody, Frame, SizeHint};
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    time::Instant,
};

pub(super) const CAPACITY: usize = 16;
pub(super) const BODY_BYTES: usize = 16 * 1024;
pub(super) const BODY_SECONDS: u64 = 10;

pub(crate) struct Ingress {
    slots: Arc<Semaphore>,
    stopping: watch::Sender<bool>,
}
impl Default for Ingress {
    fn default() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(CAPACITY)),
            stopping: watch::channel(false).0,
        }
    }
}
impl Ingress {
    pub(super) fn enter(&self) -> Result<OwnedSemaphorePermit> {
        if *self.stopping.borrow() {
            return Err(Error::new("admission_closed"));
        }
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::limited(1))
    }
    pub(super) fn close(&self) {
        self.stopping.send_replace(true);
    }
    pub(super) async fn idle(&self) {
        // No semaphore close: shutdown waits for every already accepted worker
        // and response guard, even when its HTTP caller disappeared.
        let _all = self.slots.clone().acquire_many_owned(CAPACITY as u32).await;
    }
    pub(super) async fn read(&self, body: &mut Body, deadline: Instant) -> Result<Vec<u8>> {
        let mut stopping = self.stopping.subscribe();
        if *stopping.borrow_and_update() {
            return Err(Error::new("admission_closed"));
        }
        if body.size_hint().lower() > BODY_BYTES as u64 {
            return Err(Error::new("request_too_large"));
        }
        bounded(collect(body), deadline, async move {
            let _ = stopping.changed().await;
        })
        .await
    }
}

// A deadline bounds the entire read, rather than restarting on each frame.
// Both futures stay in this task: timeout/cancellation creates no read worker.
async fn bounded<T>(
    work: impl Future<Output = Result<T>>,
    deadline: Instant,
    stopping: impl Future<Output = ()>,
) -> Result<T> {
    bounded_until(work, tokio::time::sleep_until(deadline), stopping).await
}
async fn bounded_until<T>(
    work: impl Future<Output = Result<T>>,
    expired: impl Future<Output = ()>,
    stopping: impl Future<Output = ()>,
) -> Result<T> {
    tokio::select! {
        biased;
        _ = stopping => Err(Error::new("admission_closed")),
        _ = expired => Err(Error::new("invalid_request")),
        result = work => result,
    }
}
async fn collect(body: &mut Body) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)).await {
        let frame = frame.map_err(|_| Error::new("invalid_request"))?;
        if let Ok(data) = frame.into_data() {
            if data.len() > BODY_BYTES - bytes.len() {
                return Err(Error::new("request_too_large"));
            }
            bytes.extend_from_slice(&data);
        }
        // Give the outer deadline/shutdown select a polling boundary even when
        // a body provides several immediately ready frames without byte growth.
        tokio::task::yield_now().await;
    }
    Ok(bytes)
}

// Using the standard typed body trait is necessary to tie the original incoming
// receiver, admission and root owner to Hyper's actual response-body lifetime.
// No helper stream, drain task, protocol parser or detached cleanup is involved.
struct Retained {
    response: Body,
    _incoming: Body,
    _permit: Option<OwnedSemaphorePermit>,
    _owner: Arc<Service>,
}
impl HttpBody for Retained {
    type Data = Bytes;
    type Error = axum::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
        Pin::new(&mut self.response).poll_frame(cx)
    }
    fn size_hint(&self) -> SizeHint {
        self.response.size_hint()
    }
}
pub(super) fn retain(
    mut response: Response,
    incoming: Body,
    permit: Option<OwnedSemaphorePermit>,
    owner: Arc<Service>,
) -> Response {
    // Axum is enabled for HTTP/1 only. Close is deliberate on every browser
    // response (including status and pre-admission errors), so Hyper never needs
    // to drain an unread body to make that connection reusable. Keep its receiver
    // alive through response transmission, not just through dispatch's return.
    response
        .headers_mut()
        .insert("connection", "close".parse().expect("static header"));
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::new(Retained {
            response: body,
            _incoming: incoming,
            _permit: permit,
            _owner: owner,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::pending,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::sync::oneshot;

    #[test]
    fn fixed_capacity_has_no_waiter_and_success_error_cancellation_reuse() {
        let ingress = Ingress::default();
        let mut held = (0..CAPACITY)
            .map(|_| ingress.enter().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ingress.enter().unwrap_err(), Error::limited(1));
        assert_eq!(ingress.slots.available_permits(), 0);
        drop(held.pop());
        // No queued worker can take the released slot ahead of this admission.
        drop(ingress.enter().unwrap());
        drop(held);
        assert_eq!(ingress.slots.available_permits(), CAPACITY);
        ingress.close();
        assert_eq!(ingress.enter().unwrap_err().code, "admission_closed");
    }

    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[tokio::test]
    async fn finite_pending_future_timeout_error_success_and_cancellation_cleanup() {
        let ingress = Arc::new(Ingress::default());
        let drops = Arc::new(AtomicUsize::new(0));
        let guard = Dropped(drops.clone());
        let permit = ingress.enter().unwrap();
        let (entered, observed) = oneshot::channel();
        let work = async move {
            let _held = (permit, guard);
            entered.send(()).unwrap();
            pending::<Result<()>>().await
        };
        let mut read = Box::pin(bounded(
            work,
            Instant::now() + Duration::from_secs(60),
            pending(),
        ));
        // Explicitly poll until the controlled work reports entry. No sleeps or
        // timing contention inference; dropping this future proves cancellation.
        poll_fn(|cx| {
            assert!(read.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        observed.await.unwrap();
        assert_eq!(ingress.slots.available_permits(), CAPACITY - 1);
        drop(read);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(ingress.slots.available_permits(), CAPACITY);

        // Inject a finite deadline signal into the private select helper. The
        // production caller always supplies sleep_until(absolute_deadline).
        let guard = Dropped(drops.clone());
        let permit = ingress.enter().unwrap();
        let (entered, observed) = oneshot::channel();
        let (expire, expired) = oneshot::channel();
        let mut timeout = Box::pin(bounded_until(
            async move {
                let _held = (guard, permit);
                entered.send(()).unwrap();
                pending::<Result<()>>().await
            },
            async {
                let _ = expired.await;
            },
            pending(),
        ));
        poll_fn(|cx| {
            assert!(timeout.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        observed.await.unwrap();
        expire.send(()).unwrap();
        assert_eq!(timeout.await.unwrap_err().code, "invalid_request");
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert_eq!(ingress.slots.available_permits(), CAPACITY);
        assert_eq!(
            bounded(pending::<Result<()>>(), Instant::now(), pending())
                .await
                .unwrap_err()
                .code,
            "invalid_request"
        );
        for result in [Ok(()), Err(Error::new("invalid_request"))] {
            let permit = ingress.enter().unwrap();
            let expected = result.clone();
            assert_eq!(
                bounded(
                    async move {
                        let _held = permit;
                        result
                    },
                    Instant::now() + Duration::from_secs(60),
                    pending()
                )
                .await,
                expected
            );
            assert_eq!(ingress.slots.available_permits(), CAPACITY);
        }
    }

    #[tokio::test]
    async fn collect_exact_limit_error_and_shutdown_pending_body() {
        let ingress = Ingress::default();
        let deadline = Instant::now() + Duration::from_secs(60);
        for n in [BODY_BYTES - 1, BODY_BYTES, BODY_BYTES + 1] {
            let mut body = Body::from(vec![b' '; n]);
            let result = ingress.read(&mut body, deadline).await;
            if n <= BODY_BYTES {
                assert_eq!(result.unwrap().len(), n);
            } else {
                assert_eq!(result.unwrap_err().code, "request_too_large");
            }
        }
        let (entered, observed) = oneshot::channel();
        struct PendingBody(Option<oneshot::Sender<()>>);
        impl HttpBody for PendingBody {
            type Data = Bytes;
            type Error = std::io::Error;
            fn poll_frame(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
                if let Some(signal) = self.0.take() {
                    signal.send(()).unwrap();
                }
                Poll::Pending
            }
        }
        let mut body = Body::new(PendingBody(Some(entered)));
        let mut read = Box::pin(ingress.read(&mut body, deadline));
        poll_fn(|cx| {
            assert!(read.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        observed.await.unwrap();
        ingress.close();
        assert_eq!(read.await.unwrap_err().code, "admission_closed");

        struct Failed;
        impl HttpBody for Failed {
            type Data = Bytes;
            type Error = std::io::Error;
            fn poll_frame(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
                Poll::Ready(Some(Err(std::io::Error::other("synthetic body failure"))))
            }
        }
        assert_eq!(
            collect(&mut Body::new(Failed)).await.unwrap_err().code,
            "invalid_request"
        );
    }
}
