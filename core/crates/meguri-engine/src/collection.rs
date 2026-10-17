//! The collection handle: how callers submit ops, read through role readers
//! and take snapshots. A handle belongs to one generation (`engine/lifecycle`
//! L3); through an older one every op, reader and snapshot fails with
//! `StaleGeneration`.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use meguri_storage::{ReaderRole, Writer};

use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::executor::ExecShared;
use crate::executor::catalog::OpDef;
use crate::executor::exclusive::Privilege;
use crate::executor::op::{OpContext, OpResult, OpSpec, Pending, WriteDelay};
use crate::lifecycle::Generation;
use crate::readers::{ReaderLease, Readers};

/// What every handle of one open collection shares.
pub(crate) struct CollectionCore {
    pub(crate) exec: Arc<ExecShared>,
    pub(crate) readers: Readers,
    pub(crate) space: Arc<dyn crate::DiskSpaceProvider>,
    /// Attaches the derived store to readers (`storage/derived-store` D9),
    /// once the store's open step ran.
    pub(crate) derived: OnceLock<Arc<crate::derived_host::Attachment>>,
}

/// A handle to the open collection, for one generation. Cheap to clone.
#[derive(Clone)]
pub struct Collection {
    pub(crate) core: Arc<CollectionCore>,
    pub(crate) generation: Generation,
    pub(crate) privilege: Option<Privilege>,
}

impl fmt::Debug for Collection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Collection")
            .field("path", &self.core.exec.path)
            .field("generation", &self.generation)
            .field("privileged", &self.privilege.is_some())
            .finish()
    }
}

impl Collection {
    /// Starts recording the writer's WAL sync attempts and op write phases
    /// (storage C9.2). Nothing is recorded before this call.
    #[cfg(feature = "test-hooks")]
    pub fn record_durability_events(&self) {
        self.core.exec.start_durability_recording();
    }

    /// What was recorded since [`Collection::record_durability_events`],
    /// read without queueing writer work.
    #[cfg(feature = "test-hooks")]
    pub fn durability_events(&self) -> crate::executor::durability_events::DurabilityEvents {
        self.core.exec.durability_events()
    }

    /// The generation this handle belongs to.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// The engine's current generation.
    pub fn current_generation(&self) -> Generation {
        self.core.exec.generation()
    }

    /// Whether this handle is of the current generation.
    pub fn is_current(&self) -> bool {
        self.generation == self.current_generation()
    }

    pub fn path(&self) -> &Path {
        &self.core.exec.path
    }

    /// Capacity supplied by Swift (important usage), or statvfs in headless runs.
    /// Large-operation callers supply their complete temporary-space budget.
    pub fn require_space(&self, required: u64) -> Result<(), EngineError> {
        self.check_current()?;
        let directory = self
            .path()
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        meguri_storage::space::require_space(self.core.space.as_ref(), directory, required)?;
        Ok(())
    }

    /// The seq of the last committed op.
    pub fn last_seq(&self) -> Seq {
        self.core.exec.last_seq()
    }

    /// The declared long write or maintenance task the writer runs now
    /// (`engine/op-model` O8): U43 keeps answers in its overlay meanwhile.
    pub fn write_delay(&self) -> Option<WriteDelay> {
        self.core.exec.write_delay()
    }

    /// Submits an op and returns at once (`engine/op-model` O2).
    pub fn submit<T, F>(&self, spec: OpSpec, op: F) -> Pending<OpResult<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
    {
        self.core.exec.submit_op(spec, self.generation, self.privilege, op)
    }

    /// Submits an op and waits for its result.
    pub fn run<T, F>(&self, spec: OpSpec, op: F) -> Result<OpResult<T>, EngineError>
    where
        T: Send + 'static,
        F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
    {
        self.submit(spec, op).wait()
    }

    /// Submits a maintenance task of the op catalog (`engine/op-model` O8):
    /// `work` gets the writer connection outside any transaction.
    pub fn submit_maintenance<T, F>(&self, def: OpDef, declared: Duration, work: F) -> Pending<T>
    where
        T: Send + 'static,
        F: FnOnce(&Writer) -> Result<T, EngineError> + Send + 'static,
    {
        self.core
            .exec
            .submit_maintenance(def, declared, self.generation, self.privilege, work)
    }

    /// The reader of `role` (the first of its kind for search and server
    /// readers), held until the lease drops.
    pub fn reader(&self, role: ReaderRole) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_at(role, 0)
    }

    /// The reader of search worker `worker`.
    pub fn search_reader(&self, worker: usize) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_at(ReaderRole::Search, worker)
    }

    /// Server reader `index` (macOS, while the local servers run).
    pub fn server_reader(&self, index: usize) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_at(ReaderRole::Server, index)
    }

    /// The generation is checked while the reader is held, never before:
    /// a replacement that completed in between would otherwise hand this
    /// handle a reader of the new file (L3).
    fn reader_at(&self, role: ReaderRole, index: usize) -> Result<ReaderLease<'_>, EngineError> {
        #[cfg(test)]
        tests::before_lease();
        let mut guard = self.core.readers.lease(role, index, || self.check_current())?;
        if let (Some(attachment), Some(reader)) = (self.core.derived.get(), guard.as_mut()) {
            attachment.ensure(reader, self.generation);
        }
        Ok(ReaderLease::new(guard, &self.core.exec, self.generation))
    }

    pub(crate) fn check_current(&self) -> Result<(), EngineError> {
        let current = self.current_generation();
        if current == self.generation {
            Ok(())
        } else {
            Err(EngineError::StaleGeneration {
                planned: self.generation,
                current,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;

    use meguri_model::clock::{StepClock, UtcOffset};
    use meguri_model::rng::SeededRandom;
    use meguri_storage::ReaderRole;

    use crate::error::EngineError;
    use crate::lifecycle::{Engine, EngineConfig, Generation, LongOp, OpenConfig};

    thread_local! {
        /// Runs on this thread just before a reader is leased, so a test can
        /// hold a lease request there while a replacement runs.
        static BEFORE_LEASE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
    }

    pub(super) fn before_lease() {
        BEFORE_LEASE.with_borrow_mut(|hook| {
            if let Some(hook) = hook {
                hook();
            }
        });
    }

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2")
    }

    /// A reader request through a handle of generation 1 that is held up
    /// until a replacement made generation 2 must not get a reader of the
    /// new file under the old handle (L3).
    #[test]
    fn a_reader_request_overtaken_by_a_replacement_fails_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        std::fs::copy(fixture(), &path).unwrap();
        let replacement = dir.path().join("replacement.anki2");
        std::fs::copy(fixture(), &replacement).unwrap();
        let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let old = engine
            .open_raw(&path, OpenConfig::new(clock, Arc::new(SeededRandom::new(7))))
            .unwrap();
        assert_eq!(old.generation(), Generation(1));

        let (paused_tx, paused_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel::<()>();
        let requester = {
            let old = old.clone();
            thread::spawn(move || {
                BEFORE_LEASE.with_borrow_mut(|hook| {
                    *hook = Some(Box::new(move || {
                        paused_tx.send(()).unwrap();
                        resume_rx.recv().unwrap();
                    }));
                });
                match old.reader(ReaderRole::Query) {
                    // While a lease is held the generation cannot change, so
                    // this is the generation the lease reads.
                    Ok(_lease) => Ok(old.current_generation()),
                    Err(error) => Err(error),
                }
            })
        };
        paused_rx.recv_timeout(Duration::from_secs(10)).unwrap();

        let slot = engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(false).unwrap();
        let source = replacement.clone();
        exclusive
            .replace(move |live| std::fs::rename(&source, live).map_err(|e| EngineError::internal(e.to_string())))
            .unwrap();
        drop(exclusive);
        drop(slot);
        assert_eq!(engine.generation(), Generation(2));

        resume_tx.send(()).unwrap();
        assert_eq!(
            requester.join().unwrap(),
            Err(EngineError::StaleGeneration {
                planned: Generation(1),
                current: Generation(2)
            })
        );
    }
}
