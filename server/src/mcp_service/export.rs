// SPDX-License-Identifier: MIT
//! M9: bounded, single-use Bearer downloads of the committed collection only.
use super::*;
use crate::{accounts, collection::schema,
    limits::TempReservation, storage::collection::Generation};
use axum::{body::{Body, Bytes}, response::Response};
use http_body::{Body as HttpBody, Frame};
use std::{collections::BTreeMap, future::Future, io::{self, Read}, pin::Pin, sync::Mutex,
    task::{Context as TaskContext, Poll}};
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};

fn invalid() -> Error { Error::new("capability_invalid") }
fn key(token: &str) -> Result<[u8; 32]> {
    crypto::bearer(token).map_err(|_| invalid())?;
    Ok(crypto::digest(token.as_bytes()))
}
#[derive(Clone)]
struct Ticket { authority: oauth::Authority, expires: i64 }
pub(crate) struct Exports {
    tickets: Mutex<BTreeMap<[u8; 32], Arc<Ticket>>>,
    capacity: Arc<Semaphore>,
}
impl Default for Exports {
    fn default() -> Self {
        Self { tickets: Mutex::new(BTreeMap::new()), capacity: Arc::new(Semaphore::new(2)) }
    }
}
impl Exports {
    fn read(&self, key: &[u8; 32]) -> Result<Arc<Ticket>> {
        let mut tickets = self.tickets.lock().map_err(|_| Error::new("storage_unavailable"))?;
        tickets.retain(|_, t| t.expires > accounts::now());
        tickets.get(key).cloned().ok_or_else(invalid)
    }
    #[cfg(test)]
    pub(super) async fn idle_fixture(&self) {
        let _all = self.capacity.acquire_many(2).await.unwrap();
    }
    #[cfg(test)]
    pub(super) fn expire_fixture(&self, token: &str) {
        self.tickets.lock().unwrap().get_mut(&key(token).unwrap()).map(|ticket| Arc::make_mut(ticket).expires = 0).unwrap();
    }
}
impl McpService {
    pub(crate) async fn download_collection(&self, original: oauth::Authority) -> Result<(String, String, i64)> {
        let token = crypto::random_token()?;
        let key = key(&token)?;
        let expires = (accounts::now() + 600).min(original.access_expires_at());
        let store = self.service.mcp_exports.clone();
        self.execute(original, false, move |_, _, a| {
            a.check()?;
            let mut tickets = store.tickets.lock().map_err(|_| Error::new("storage_unavailable"))?;
            tickets.retain(|_, t| t.expires > accounts::now());
            if tickets.len() >= 64 || tickets.values().filter(|t| t.authority.account_id() == a.original.account_id()).count() >= 8 {
                return Err(mcp::busy());
            }
            if tickets.contains_key(&key) { return Err(Error::new("storage_unavailable")); }
            tickets.insert(key, Arc::new(Ticket { authority: a.original.clone(), expires }));
            Ok(())
        }).await?;
        Ok((format!("{}/api/mcp/collection-download", self.service.config.origin()), token, expires))
    }
}
// These fields drop in order: close SQLite before releasing its independent pin.
struct Source { connection: rusqlite::Connection, _pin: ReadPin, path: PathBuf, _root: Arc<DataRoot> }
// Close the response file before removing its stage, releasing byte/capacity
// reservations, then root ownership. Cancellation moves this entire owner.
struct Snapshot {
    file: std::fs::File,
    bytes: u64,
    _stage: Generation,
    _reservations: Vec<TempReservation>,
    _capacity: OwnedSemaphorePermit,
    _root: Arc<DataRoot>,
}
impl Service {
    pub(crate) async fn redeem_collection(self: &Arc<Self>, token: &str) -> Result<Response> {
        let key = key(token)?;
        let ticket = self.mcp_exports.read(&key)?;
        let capacity = self.mcp_exports.capacity.clone();
        let bridge = McpService::new(self.clone());
        let first_ticket = ticket.clone();
        // execute holds the one publication guard. Establish a real SQLite read
        // transaction here, then return a separate pin without retaining it.
        let budget = self.temp.clone();
        let (source, snapshot) = bridge.execute(ticket.authority.clone(), false, move |_, path, a| {
            if first_ticket.expires <= accounts::now() { return Err(invalid()); }
            let capacity = capacity.try_acquire_owned().map_err(|_| mcp::busy())?;
            let pin = a.held.root.pin_collection(path)?;
            let c = schema::open_mcp(path, false)?;
            c.execute_batch("BEGIN DEFERRED")?;
            c.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get::<_, i64>(0))?;
            let pages: i64 = c.pragma_query_value(None, "page_count", |r| r.get(0))?;
            let page_size: i64 = c.pragma_query_value(None, "page_size", |r| r.get(0))?;
            let bytes = u64::try_from(pages).ok().and_then(|n| n.checked_mul(u64::try_from(page_size).ok()?))
                .filter(|n| *n > 0).and_then(|n| n.checked_mul(2)).ok_or(Error::new("storage_unavailable"))?;
            let metadata = budget.reserve(4096)?;
            let reservation = budget.reserve(bytes)?;
            // Create only under the original account guard. Reset cannot make
            // this preparation create directories in a replacement account tree.
            let root = a.held.root.clone();
            let stage = root.stage_collection(a.original.account_id())?;
            let snapshot = Snapshot { file: stage.writer()?, bytes: 0, _stage: stage,
                _reservations: vec![metadata, reservation], _capacity: capacity, _root: root.clone() };
            Ok((Source { connection: c, _pin: pin, path: path.to_owned(), _root: root }, snapshot))
        }).await?;
        self.pause("collection_export_pinned").await;
        let path = source.path.clone();
        // Only the expensive backup runs off the guard and metadata slot. All
        // resources enter this owned worker before cancellation can detach it.
        let snapshot = tokio::task::spawn_blocking(move || -> Result<Snapshot> {
            let mut snapshot = snapshot;
            let result = schema::snapshot(&source.connection, &snapshot._stage.path);
            drop(source);
            result?;
            snapshot.bytes = snapshot.file.metadata()?.len();
            Ok(snapshot)
        }).await.map_err(|_| Error::new("storage_unavailable"))??;
        self.pause("collection_export_ready").await;
        let store = self.mcp_exports.clone();
        // No await between the final authority check and ticket removal. A copy
        // failure or lost publication leaves the ticket unconsumed.
        let snapshot = bridge.execute(ticket.authority.clone(), false, move |_, current, a| {
            let mut tickets = store.tickets.lock().map_err(|_| Error::new("storage_unavailable"))?;
            a.check()?;
            if path != current { return Err(Error::new("conflict")); }
            if tickets.get(&key).is_none_or(|t| t.expires <= accounts::now() || !Arc::ptr_eq(t, &ticket)) { return Err(invalid()); }
            tickets.remove(&key).ok_or_else(invalid)?;
            Ok(snapshot)
        }).await?;
        Ok(stream(snapshot))
    }
}
enum Chunk { Data(Bytes), End }
struct Stream {
    rx: mpsc::Receiver<io::Result<Chunk>>,
    deadline: Pin<Box<tokio::time::Sleep>>,
    completed: Option<oneshot::Sender<()>>,
    cleaned: Option<oneshot::Receiver<()>>,
    ended: bool,
}
impl Stream {
    fn finish(&mut self) {
        self.ended = true;
        self.rx.close();
        if let Some(done) = self.completed.take() { let _ = done.send(()); }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(done) = self.completed.take() { let _ = done.send(()); }
    }
}
impl HttpBody for Stream {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<io::Result<Frame<Bytes>>>> {
        if !self.ended {
            // Check before the channel: queued bytes cannot extend the worker's
            // absolute deadline, even if the client resumes polling much later.
            if self.deadline.as_mut().poll(cx).is_ready() {
                self.finish();
                return Poll::Ready(Some(Err(io::ErrorKind::TimedOut.into())));
            }
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(Ok(Chunk::Data(bytes)))) => return Poll::Ready(Some(Ok(Frame::data(bytes)))),
                Poll::Ready(Some(Ok(Chunk::End))) => {
                    self.finish();
                }
                Poll::Ready(Some(Err(error))) => {
                    self.finish();
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(None) => self.finish(),
                Poll::Pending => return Poll::Pending,
            }
        }
        // EOF follows cleanup, so byte reservations never outlive completion.
        if let Some(cleaned) = self.cleaned.as_mut() {
            if Pin::new(cleaned).poll(cx).is_pending() { return Poll::Pending; }
            self.cleaned = None;
        }
        Poll::Ready(None)
    }
}
fn stream(mut snapshot: Snapshot) -> Response {
    let size = snapshot.bytes;
    let (tx, rx) = mpsc::channel(2);
    let (completed, completion) = oneshot::channel();
    let (cleanup, cleaned) = oneshot::channel();
    let runtime = tokio::runtime::Handle::current();
    // Absolute five-minute response deadline bounds even a trickling client.
    // A closed receiver wakes the worker immediately; at most two chunks queue.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    tokio::task::spawn_blocking(move || {
        let result = (|| -> io::Result<()> {
            let mut buffer = [0; 64 * 1024];
            loop {
                if tokio::time::Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()); }
                let count = snapshot.file.read(&mut buffer)?;
                if count == 0 { return Ok(()); }
                runtime.block_on(async {
                    tokio::time::timeout_at(deadline, tx.send(Ok(Chunk::Data(Bytes::copy_from_slice(&buffer[..count]))))).await
                })
                    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
                    .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
            }
        })();
        runtime.block_on(async {
            let end = if result.is_ok() { Ok(Chunk::End) } else { Err(io::Error::other("collection download ended")) };
            let _ = tokio::time::timeout_at(deadline, tx.send(end)).await;
            // Retain the file, stage and capacity until consumption or drop,
            // even when the entire small file fitted in the bounded channel.
            let _ = tokio::time::timeout_at(deadline, completion).await;
        });
        drop(snapshot);
        let _ = cleanup.send(());
    });
    let mut response = Response::new(Body::new(Stream { rx, deadline: Box::pin(tokio::time::sleep_until(deadline)),
        completed: Some(completed), cleaned: Some(cleaned), ended: false }));
    response.headers_mut().insert("content-type", "application/vnd.sqlite3".parse().unwrap());
    response.headers_mut().insert("content-disposition", "attachment; filename=\"collection.anki2\"".parse().unwrap());
    response.headers_mut().insert("content-length", size.into());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn elapsed_response_deadline_refuses_queued_bytes_and_waits_for_cleanup() {
        let (tx, rx) = mpsc::channel(2);
        tx.try_send(Ok(Chunk::Data(Bytes::from_static(b"queued")))).unwrap();
        let (completed, mut completion) = oneshot::channel();
        let (cleanup, cleaned) = oneshot::channel();
        let deadline = tokio::time::Instant::now() - Duration::from_secs(1);
        let mut stream = Stream { rx, deadline: Box::pin(tokio::time::sleep_until(deadline)),
            completed: Some(completed), cleaned: Some(cleaned), ended: false };
        let frame = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_frame(cx)).await;
        assert_eq!(frame.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(tx.is_closed());
        assert_eq!(completion.try_recv(), Ok(()));
        std::future::poll_fn(|cx| {
            assert!(Pin::new(&mut stream).poll_frame(cx).is_pending());
            Poll::Ready(())
        }).await;
        cleanup.send(()).unwrap();
        assert!(std::future::poll_fn(|cx| Pin::new(&mut stream).poll_frame(cx)).await.is_none());
        assert!(std::future::poll_fn(|cx| Pin::new(&mut stream).poll_frame(cx)).await.is_none());
    }
}
