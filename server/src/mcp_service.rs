// SPDX-License-Identifier: MIT
//! OAuth-to-collection bridge (M3–M6, C7), mounted by httpapi.
//!
//! Handle keys are owned by Service and survive every request/router for it. Parse raw arguments with Backend::parse at the transport.
//! Checkpoint maintenance belongs outside these bounded point operations.
mod transport;
pub(crate) mod attachments;
#[cfg(test)]
#[path = "../tests/support/attachment_formats.rs"]
pub(crate) mod attachment_formats;
#[cfg(test)]
mod authority_tests;

use crate::{
    Error, Result, Service, accounts::{PublicationBarrier, oauth},
    collection::{mcp::{self, Backend, CatalogInput, CatalogResult, Context, CreateInput,
        EditInput, SearchInput, SearchResult, WriteResult, WriteRuntime}, session::Sessions},
    crypto, storage::{self, DataRoot, collection::ReadPin},
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::{path::{Path, PathBuf}, sync::Arc, time::Duration};
use tokio::sync::OwnedMutexGuard;

pub(crate) struct McpService {
    service: Arc<Service>,
    backend: Arc<Backend>,
    #[cfg(any(test, feature = "test-hooks"))]
    probe: Option<Arc<dyn Fn(TestBoundary) + Send + Sync>>,
    #[cfg(test)]
    direct_fixture: Option<Arc<dyn Fn(&str) -> Result<Vec<u8>> + Send + Sync>>,
}

impl McpService {
    pub(crate) fn new(service: Arc<Service>) -> Self {
        Self {
            backend: Arc::new(Backend::new(service.mcp_key)), service,
            #[cfg(any(test, feature = "test-hooks"))]
            probe: None,
            #[cfg(test)]
            direct_fixture: None,
        }
    }

    /// Synchronous worker boundaries, for cancellation/invalidation ordering.
    /// Test callbacks must release their own waits, including after cancellation.
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn with_test_probe(mut self, probe: Arc<dyn Fn(TestBoundary) + Send + Sync>) -> Self {
        self.probe = Some(probe);
        self
    }

    pub(crate) async fn search_notes(&self, authority: oauth::Authority, input: SearchInput) -> Result<SearchResult> {
        self.execute(authority, false, move |backend, path, a| backend.search_notes(path, a, input)).await
    }

    pub(crate) async fn list_decks_and_note_types(&self, authority: oauth::Authority, input: CatalogInput) -> Result<CatalogResult> {
        self.execute(authority, false, move |backend, path, a| backend.list_decks_and_note_types(path, a, input)).await
    }

    pub(crate) async fn create_note(&self, authority: oauth::Authority, input: CreateInput) -> Result<WriteResult> {
        self.execute(authority, true, move |backend, path, a| {
            backend.create_note(path, a, input, &Runtime)
        }).await
    }

    pub(crate) async fn edit_note(&self, authority: oauth::Authority, input: EditInput) -> Result<WriteResult> {
        self.execute(authority, true, move |backend, path, a| {
            backend.edit_note(path, a, input, &Runtime)
        }).await
    }

    async fn execute<T: Send + 'static>(&self, original: oauth::Authority, write: bool,
        operation: impl FnOnce(&Backend, &Path, &CollectionAuthority) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.execute_pinned(original, write, None, operation).await
    }
    async fn execute_pinned<T: Send + 'static>(&self, original: oauth::Authority, write: bool, pinned: Option<attachments::Pinned>,
        operation: impl FnOnce(&Backend, &Path, &CollectionAuthority) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        // OAuth owns account resolution and barrier identity. Do not manufacture
        // a NativeAuthority or use a tool's note/deck IDs to select storage.
        let barrier = self.service.mcp_publication_barrier(&original).await?;
        let guard = barrier.try_enter().ok_or_else(mcp::busy)?;
        let root = self.service.metadata.owner();
        let sessions = self.service.normal_sessions.clone();
        let backend = self.backend.clone();
        #[cfg(any(test, feature = "test-hooks"))]
        let probe = self.probe.clone();
        self.service.metadata.files(move |_| {
            // All ownership moves into this worker before its first await can
            // be cancelled. It cannot release the gate before SQLite finishes.
            let _publication = (barrier, guard);
            check_oauth(&root, &original, write)?;
            let path = root.collection_path(original.account_id())?;
            if pinned.as_ref().is_some_and(|p|p.path != path) { return Err(Error::new("conflict")); }
            // Move the acquisition pin into the durable worker through completion.
            let _acquisition_pin = pinned;
            let pin = root.pin_collection(&path)?;
            let publication = path.parent().and_then(Path::file_name)
                .and_then(|name| name.to_str()).ok_or(Error::new("storage_unavailable"))?.to_owned();
            let context = Context { account: original.account_id().into(), publication };
            let held = Arc::new(HeldPublication { root, path: path.clone(), _pin: pin,
                _barrier: _publication.0, _guard: _publication.1 });
            let authority = CollectionAuthority { original, context, held, sessions, write,
                #[cfg(any(test, feature = "test-hooks"))]
                probe,
            };
            // Also covers refusals made before Backend::transaction is reached.
            authority.check()?;
            operation(&backend, &path, &authority)
            // Never convert an error after a successful commit into busy.
        }).await
    }
}

struct HeldPublication {
    root: Arc<DataRoot>,
    path: PathBuf,
    _pin: ReadPin,
    _barrier: Arc<PublicationBarrier>,
    _guard: OwnedMutexGuard<()>,
}

struct CollectionAuthority {
    original: oauth::Authority,
    context: Context,
    held: Arc<HeldPublication>,
    sessions: Arc<Sessions>,
    write: bool,
    #[cfg(any(test, feature = "test-hooks"))]
    probe: Option<Arc<dyn Fn(TestBoundary) + Send + Sync>>,
}
impl CollectionAuthority {
    fn check(&self) -> Result<()> {
        check_oauth(&self.held.root, &self.original, self.write)?;
        if self.held.root.collection_path(self.original.account_id())? != self.held.path {
            return Err(Error::new("conflict"));
        }
        Ok(())
    }
}
impl mcp::Authority for CollectionAuthority {
    type Guard = Arc<HeldPublication>;
    fn context(&self) -> &Context { &self.context }
    fn admit(&self, write: bool) -> Result<Self::Guard> {
        if write != self.write { return Err(Error::new("internal_error")); }
        self.check()?;
        // A retained BEGIN IMMEDIATE writer remains live between native HTTP
        // requests. Inspect before even opening a writable SQLite connection.
        // Reads deliberately skip this inspection and see only committed WAL.
        if write && self.sessions.has_retained_writer(self.original.account_id())? {
            return Err(mcp::busy());
        }
        #[cfg(any(test, feature = "test-hooks"))]
        if let Some(probe) = &self.probe { probe(TestBoundary::Admitted); }
        Ok(self.held.clone())
    }
    fn recheck(&self, guard: &Self::Guard) -> Result<()> {
        if !Arc::ptr_eq(guard, &self.held) { return Err(Error::new("internal_error")); }
        // Each check gets a fresh, short metadata snapshot. Never reuse the
        // admission read transaction, which would hide a changed credential.
        #[cfg(any(test, feature = "test-hooks"))]
        if let Some(probe) = &self.probe { probe(TestBoundary::BeforeCompletion); }
        self.check()
    }
}

fn check_oauth(root: &DataRoot, original: &oauth::Authority, write: bool) -> Result<()> {
    let mut c = storage::inspect(root.database())?;
    c.busy_timeout(Duration::ZERO)?;
    let tx = c.transaction()?;
    oauth::check_authority(&tx, original, write)?;
    tx.commit()?;
    Ok(())
}

fn secret() -> Result<[u8; 32]> { crypto::bearer(&crypto::random_token()?) }
struct Runtime;
impl WriteRuntime for Runtime {
    fn now_millis(&self) -> Result<i64> {
        i64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos().div_euclid(1_000_000))
            .map_err(|_| Error::new("storage_unavailable"))
    }
    fn guid_random_u64(&self) -> Result<u64> {
        let bytes = secret()?;
        Ok(u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes")))
    }
    fn random_position(&self, counter: u32) -> Result<u32> {
        Ok(StdRng::seed_from_u64(u64::from(counter)).random_range(1..counter.max(1000)))
    }
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TestBoundary { Admitted, BeforeCompletion }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p4_uses_the_pinned_seeded_generator() {
        let runtime = Runtime;
        for (counter, expected) in [(0,801), (1,825), (2,82), (3,647), (998,779),
            (999,574), (1000,998), (1001,661), (5000,4542),
            (123456789,33597438), (u32::MAX,2331729548)] {
            assert_eq!(runtime.random_position(counter).unwrap(), expected);
        }
    }
}
