// SPDX-License-Identifier: MIT
use super::*;
use std::{
    collections::HashMap,
    sync::{Mutex, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, RwLock, OwnedRwLockReadGuard, OwnedRwLockWriteGuard};
/// Must immediately roll back/drop retained round state without client waits.
/// Invoked under the target barrier and committing metadata writer, only after
/// current authority is validated. Never acquire a barrier or wait for clients.
pub trait RoundRetirer: Send + Sync {
    fn retire(&self) -> Result<()>;
}
pub struct PublicationBarrier {
    lock: Arc<AsyncMutex<()>>,
    backup: Arc<AsyncMutex<()>>,
    // History snapshots exclude only restores, not ordinary daily capture.
    restore: Arc<RwLock<()>>,
    round: Mutex<Option<Arc<dyn RoundRetirer>>>,
    // MCP collection jobs queue here before touching the publication lock.
    mcp: Arc<AsyncMutex<()>>,
    // Only browser collection writes and sign-out use this lease. Browse takes
    // it with try after publication; sign-out takes it without publication and
    // never acquires publication later. MCP/native admission stays unchanged.
    browser_commit: Arc<AsyncMutex<()>>,
    #[cfg(feature = "test-hooks")]
    browser_waiting: tokio::sync::Semaphore,
    #[cfg(feature = "test-hooks")]
    mcp_waiting: tokio::sync::Semaphore,
    #[cfg(feature = "test-hooks")]
    waiting: tokio::sync::Semaphore,
}
impl PublicationBarrier {
    pub(crate) fn try_browser_commit(&self) -> Option<OwnedMutexGuard<()>> {
        self.browser_commit.clone().try_lock_owned().ok()
    }
    pub(crate) async fn enter_browser_commit(&self) -> OwnedMutexGuard<()> {
        #[cfg(not(feature = "test-hooks"))]
        { self.browser_commit.clone().lock_owned().await }
        #[cfg(feature = "test-hooks")]
        {
            let mut lock = Box::pin(self.browser_commit.clone().lock_owned());
            let mut reported = false;
            std::future::poll_fn(|cx| {
                use std::future::Future;
                let result = lock.as_mut().poll(cx);
                if result.is_pending() && !reported {
                    reported = true; self.browser_waiting.add_permits(1);
                }
                result
            }).await
        }
    }
    #[cfg(feature = "test-hooks")]
    pub async fn browser_commit_waiter_pending(&self) {
        self.browser_waiting.acquire().await.unwrap().forget();
    }

    // Try only: never wait on restore I/O while holding metadata/publication.
    // Retain the barrier Arc alongside each owned guard (the registry is weak).
    pub(crate) fn try_restore(&self) -> Option<OwnedRwLockWriteGuard<()>> {
        self.restore.clone().try_write_owned().ok()
    }
    pub(crate) fn try_history(&self) -> Option<OwnedRwLockReadGuard<()>> {
        self.restore.clone().try_read_owned().ok()
    }
    pub(crate) fn try_backup(&self) -> Option<OwnedMutexGuard<()>> {
        self.backup.clone().try_lock_owned().ok()
    }
    /// Standalone writes refuse contention without joining the waiter queue.
    /// Bounded MCP reads use enter() with their asynchronous timeout instead.
    /// Keep this barrier's Arc alongside the returned guard until work ends.
    pub(crate) fn try_enter(&self) -> Option<OwnedMutexGuard<()>> {
        self.lock.clone().try_lock_owned().ok()
    }
    pub async fn enter(&self) -> OwnedMutexGuard<()> {
        #[cfg(not(feature = "test-hooks"))]
        {
            self.lock.clone().lock_owned().await
        }
        #[cfg(feature = "test-hooks")]
        {
            let mut lock = Box::pin(self.lock.clone().lock_owned());
            let mut reported = false;
            std::future::poll_fn(|cx| {
                use std::future::Future;
                let result = lock.as_mut().poll(cx);
                if result.is_pending() && !reported {
                    reported = true;
                    self.waiting.add_permits(1);
                }
                result
            })
            .await
        }
    }
    /// Acknowledges an actually polled, contending acquisition in fixtures.
    #[cfg(feature = "test-hooks")]
    pub async fn waiter_pending(&self) {
        self.waiting.acquire().await.unwrap().forget();
    }
    /// MCP-only FIFO admission. Call before enter/try_enter, never while owning
    /// publication, metadata or worker capacity. Keep both guards in the worker.
    pub(crate) async fn enter_mcp(&self) -> OwnedMutexGuard<()> {
        #[cfg(not(feature = "test-hooks"))]
        {
            self.mcp.clone().lock_owned().await
        }
        #[cfg(feature = "test-hooks")]
        {
            let mut lock = Box::pin(self.mcp.clone().lock_owned());
            let mut reported = false;
            std::future::poll_fn(|cx| {
                use std::future::Future;
                let result = lock.as_mut().poll(cx);
                if result.is_pending() && !reported {
                    reported = true;
                    self.mcp_waiting.add_permits(1);
                }
                result
            }).await
        }
    }
    /// Acknowledges an actually polled MCP queue waiter, once per acquisition.
    #[cfg(feature = "test-hooks")]
    pub(crate) async fn mcp_waiter_pending(&self) {
        self.mcp_waiting.acquire().await.unwrap().forget();
    }
    /// Call while owning this barrier. Retain this Arc for the round lifetime.
    pub fn set_round(&self, round: Option<Arc<dyn RoundRetirer>>) -> Result<()> {
        *self
            .round
            .lock()
            .map_err(|_| Error::new("storage_unavailable"))? = round;
        Ok(())
    }
    pub(crate) fn retire(&self) -> Result<()> {
        if let Some(round) = self
            .round
            .lock()
            .map_err(|_| Error::new("storage_unavailable"))?
            .take()
        {
            round.retire()?;
        }
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Barriers(Mutex<HashMap<String, Weak<PublicationBarrier>>>);
impl Barriers {
    /// Callers resolve an existing account first. Prune dead entries and bound
    /// active gates; arbitrary unauthenticated IDs never enter this map.
    pub fn get(&self, id: &str) -> Result<Arc<PublicationBarrier>> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| Error::new("storage_unavailable"))?;
        map.retain(|_, v| v.strong_count() > 0);
        if let Some(g) = map.get(id).and_then(Weak::upgrade) {
            return Ok(g);
        }
        if map.len() >= 1024 {
            return Err(Error::new("storage_unavailable"));
        }
        let gate = Arc::new(PublicationBarrier {
            lock: Arc::new(AsyncMutex::new(())),
            backup: Arc::new(AsyncMutex::new(())),
            restore: Arc::new(RwLock::new(())),
            round: Mutex::new(None),
            mcp: Arc::new(AsyncMutex::new(())),
            browser_commit: Arc::new(AsyncMutex::new(())),
            #[cfg(feature = "test-hooks")]
            browser_waiting: tokio::sync::Semaphore::new(0),
            #[cfg(feature = "test-hooks")]
            mcp_waiting: tokio::sync::Semaphore::new(0),
            #[cfg(feature = "test-hooks")]
            waiting: tokio::sync::Semaphore::new(0),
        });
        map.insert(id.into(), Arc::downgrade(&gate));
        Ok(gate)
    }
}
impl Service {
    /// Full-file publication owns the account barrier before resolving current
    /// collection state. The authority read takes no shared metadata writer.
    /// The dedicated blocking job retains the barrier across cancellation.
    pub(crate) async fn collection_operation<T: Send + 'static>(
        &self,
        authority: NativeAuthority,
        retire: bool,
        operation: impl FnOnce(&crate::storage::DataRoot) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.collection_operation_at(authority, retire, "collection_with_barrier", operation).await
    }
    /// Pre-transfer staging uses the same authority and barrier without retiring
    /// round state. Its test boundary is separate from final publication.
    pub(crate) async fn collection_staging<T: Send + 'static>(
        &self,
        authority: NativeAuthority,
        operation: impl FnOnce(&crate::storage::DataRoot) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.collection_operation_at(authority, false, "collection_staging_with_barrier", operation).await
    }
    async fn collection_operation_at<T: Send + 'static>(
        &self,
        authority: NativeAuthority,
        retire: bool,
        phase: &'static str,
        operation: impl FnOnce(&crate::storage::DataRoot) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let gate = self.publication_barrier(&authority).await?;
        let guard = gate.enter().await;
        self.pause(phase).await;
        self.metadata.run(move |c| {
            admission(c)?;
            check_native_generation(c, &authority)
        }).await?;
        self.metadata.files(move |root| {
            let _guard = guard;
            if retire { gate.retire()?; }
            operation(root)
        }).await
    }
    pub async fn publication_barrier(
        &self,
        authority: &NativeAuthority,
    ) -> Result<Arc<PublicationBarrier>> {
        let a = authority.clone();
        self.metadata
            .run(move |c| {
                admission(c)?;
                check_native_generation(c, &a)?;
                Ok(())
            })
            .await?;
        self.barriers.get(&authority.id)
    }
    /// Storage paths are resolved only after authority validation. Streaming may
    /// use these outside the barrier; publication must call `publish` again.
    pub async fn account_stores(
        &self,
        authority: &NativeAuthority,
    ) -> Result<crate::storage::AccountStores> {
        let a = authority.clone();
        self.metadata
            .run(move |c| {
                admission(c)?;
                check_native_generation(c, &a)
            })
            .await?;
        let id = authority.id.clone();
        self.metadata
            .files(move |root| {
                root.validate_stores(&id)?;
                root.stores(&id)
            })
            .await
    }
    /// Metadata publication seam: bytes/hash/validation prepared outside;
    /// callback is synchronous, bounded and runs off the async executor under
    /// the barrier and one short IMMEDIATE transaction. No network/hash waits.
    /// Commit durable collection state inside callback before returning success.
    pub async fn publish<T: Send + 'static>(
        &self,
        a: NativeAuthority,
        f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.publish_at(a,"publish_with_barrier",f).await
    }
    /// Same coordinator and transaction; a named test boundary distinguishes a
    /// media commit from its earlier metadata-only snapshot.
    pub(crate) async fn publish_at<T: Send + 'static>(
        &self, a:NativeAuthority, phase:&'static str,
        f:impl FnOnce(&Connection)->Result<T>+Send+'static,
    )->Result<T> {
        let gate = self.publication_barrier(&a).await?;
        let guard = gate.enter().await;
        self.pause(phase).await;
        self.metadata
            .run(move |c| {
                let _publication = (gate, guard);
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                check_native_generation(&tx, &a)?;
                let result = f(&tx)?;
                tx.commit()?;
                Ok(result)
            })
            .await
    }
}
pub(crate) fn check_native_generation(c: &Connection, a: &NativeAuthority) -> Result<()> {
    read_account(c, &a.id)?
        .filter(|r| r.usable() && r.generation == a.generation)
        .ok_or(Error::new("authentication_failed"))?;
    if crate::storage::reset::admitted_epoch(c, &a.id)? != a.data_epoch {
        return Err(Error::new("authentication_failed"));
    }
    Ok(())
}
