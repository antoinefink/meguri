//! The collection handle: how callers submit ops, read through role readers
//! and take snapshots. A handle belongs to one generation (`engine/lifecycle`
//! L3); through an older one every op, reader and snapshot fails with
//! `StaleGeneration`.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use meguri_storage::{OpenedCollection, ReadRows, ReaderRole, Writer};

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
    pub(crate) backup: Option<Arc<dyn crate::ports::BackupService>>,
    /// Attaches the derived store to readers (`storage/derived-store` D9),
    /// once the store's open step ran.
    pub(crate) derived: OnceLock<Arc<crate::derived_host::Attachment>>,
    pub(crate) render_cache: crate::rendering::ParseCache,
    pub(crate) random: Arc<dyn meguri_model::rng::RandomSource>,
    pub(crate) media: std::sync::Mutex<Option<Arc<crate::media::MediaState>>>,
    pub(crate) cloud_publication_authority: Option<Arc<dyn crate::ports::CloudPublicationAuthority>>,
    pub(crate) publication_binding: Arc<std::sync::Mutex<crate::device_state::AccountBinding>>,
}

/// A handle to the open collection, for one generation. Cheap to clone.
#[derive(Clone)]
pub struct Collection {
    pub(crate) core: Arc<CollectionCore>,
    pub(crate) generation: Generation,
    pub(crate) privilege: Option<Privilege>,
    pub(crate) undo_session: Option<crate::SessionId>,
}

/// Facts captured once before engine writes in one SQLite open incarnation.
/// `native_file` is C9's device/inode pair, not a birthtime identity.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenEvidence {
    pub generation: Generation,
    pub previous_clean_shutdown: bool,
    pub opened_crt: i64,
    pub opened_mod: i64,
    pub opened_scm: i64,
    pub native_file: (u64, u64),
    /// Bounded cloud-only failure: unavailable identity never prevents local use.
    pub cloud_file: Option<meguri_storage::persistent_identity::CloudFileEvidence>,
}

impl fmt::Debug for OpenEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenEvidence")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl OpenEvidence {
    pub(crate) fn capture(opened: &OpenedCollection, generation: Generation) -> Result<Self, EngineError> {
        let col = opened.writer().col()?;
        Ok(Self {
            generation,
            previous_clean_shutdown: opened.previous_clean_shutdown,
            opened_crt: col.created.0,
            opened_mod: col.modified.0,
            opened_scm: col.schema_modified.0,
            native_file: opened.writer().native_identity()?,
            cloud_file: opened.writer().cloud_file_evidence().ok(),
        })
    }
}

/// Local work guard, independent of SQLite timestamps. `pending` includes
/// admitted/parked work that has not committed yet. Never a remote revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferAdmission {
    pub admitted: u64,
    pub committed: u64,
    pub pending: bool,
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
    /// R6 activation authority for this actual open native file. The engine
    /// measured the closed candidate's bytes before admitting its writer.
    pub fn cloud_adopt_activation(&self) -> Result<Option<crate::device_state::CloudAdoptActivation>, EngineError> {
        use std::os::unix::fs::MetadataExt;
        let evidence = self.open_evidence()?;
        let metadata = std::fs::symlink_metadata(self.path()).map_err(meguri_storage::StorageError::from)?;
        if !metadata.file_type().is_file() || (metadata.dev(), metadata.ino()) != evidence.native_file {
            return Ok(None);
        }
        Ok(crate::device_state::DeviceState::for_collection(self.path())
            .cloud_adopt_activation(evidence.native_file)?)
    }

    /// Immutable facts for the current open incarnation, including during an
    /// initial privileged OpenStep. Resume/reopen captures fresh facts.
    /// No writer work is queued, so Rust callbacks may read this directly.
    pub fn open_evidence(&self) -> Result<OpenEvidence, EngineError> {
        let evidence = self
            .core
            .exec
            .open_evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check_current()?;
        self.core.exec.require_admission(self.privilege)?;
        evidence
            .as_ref()
            .copied()
            .filter(|e| e.generation == self.generation)
            .ok_or_else(|| EngineError::unavailable("no open collection incarnation"))
    }

    pub fn transfer_admission(&self) -> Result<TransferAdmission, EngineError> {
        self.check_current()?;
        self.core.exec.transfer_admission(self.privilege)
    }
    /// Observe queue admission without taking the writer, for gated tests.
    #[cfg(feature = "test-hooks")]
    pub fn has_queued_op_for_testing(&self, name: &str) -> bool {
        self.core.exec.has_queued_op(name)
    }

    #[cfg(feature = "test-hooks")]
    pub fn wait_queued_op_for_testing(&self, name: &str) {
        self.core.exec.wait_queued_op(name);
    }

    /// Arms `fail_sync_after_commit` for the next commit.
    #[cfg(feature = "test-hooks")]
    pub fn fail_wal_sync_after_next_commit_for_testing(&self) {
        self.core
            .exec
            .fail_sync_after_commit
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Postpones only this collection's automatic WAL flushes. Explicit
    /// background flush and close still perform their real sync/checkpoint.
    #[cfg(feature = "test-hooks")]
    pub fn pause_automatic_wal_flush_for_testing(&self, paused: bool) {
        self.core.exec.pause_automatic_wal_flush(paused);
    }

    /// Signals once a coverage waiter is registered, without callbacks.
    #[cfg(feature = "test-hooks")]
    pub fn notify_next_wal_wait_for_testing(&self, sender: std::sync::mpsc::Sender<()>) {
        self.core.exec.coverage.notify_next_wait(sender);
    }

    /// Runs once after this collection's next op publishes its committed
    /// seq, with no commit lock held, before day invalidation or observers.
    #[cfg(feature = "test-hooks")]
    pub fn after_next_commit(&self, hook: impl FnOnce(Seq) + Send + 'static) {
        *self.core.exec.after_commit.lock().unwrap() = Some(Box::new(hook));
    }

    /// Observes one pending settled-seq wait on this collection's stream.
    /// The hook must only notify; it runs under the stream's state lock.
    #[cfg(feature = "test-hooks")]
    pub fn on_next_settled_wait(&self, hook: impl FnOnce(Seq) + Send + 'static) {
        self.core.exec.stream.on_next_settled_wait(hook);
    }

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

    /// Flush this generation's WAL on the writer without lifecycle participants.
    /// Call off the writer, with no reader lease or caller locks held. A timeout
    /// does not cancel the queued flush; callers still need a matching receipt
    /// from `wait_wal_covered` before persisting durability-dependent facts.
    pub fn flush_wal(&self, timeout: Duration) -> Result<(), EngineError> {
        if crate::executor::writer::on_writer_thread() {
            return Err(EngineError::InvalidArgument {
                message: "WAL flush waits must run off the writer".into(),
            });
        }
        self.check_current()?;
        self.core.exec.require_admission(self.privilege)?;
        let deadline = crate::monotonic::now().saturating_add(timeout);
        self.core
            .exec
            .flush_wal(self.generation)
            .wait_timeout(deadline.saturating_sub(crate::monotonic::now()))
            .ok_or_else(|| EngineError::SuspendDeadline {
                phase: "collection WAL".into(),
            })??;
        self.check_current()
    }

    /// Wait off the writer for actual durable coverage of a snapshot's seq.
    /// Compare the receipt identity with the snapshot before persisting facts.
    /// This observes the existing flush schedule; it queues no writer work.
    pub fn wait_wal_covered(
        &self,
        identity: crate::WalIdentity,
        seq: Seq,
        timeout: Duration,
    ) -> Result<crate::WalCoverage, EngineError> {
        self.core
            .exec
            .coverage
            .wait(&self.core.exec, self.generation, identity, seq, timeout)
    }

    /// The declared long write or maintenance task the writer runs now
    /// (`engine/op-model` O8): U43 keeps answers in its overlay meanwhile.
    pub fn write_delay(&self) -> Option<WriteDelay> {
        self.core.exec.write_delay()
    }

    /// Admits required safety work off the writer, then queues the op (O2, T6).
    pub fn submit<T, F>(&self, mut spec: OpSpec, op: F) -> Pending<OpResult<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
    {
        if !spec.safety_admitted && spec.needs_safety_backup() {
            match self.admit_safety(&spec) {
                Ok(backup) => {
                    spec.safety_backup = Some(backup);
                    spec.safety_admitted = true;
                }
                Err(error) => {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let _ = tx.send(Err(error));
                    return Pending { rx };
                }
            }
        }
        if spec.source == crate::OpSource::UserUndoable && spec.undo.session.is_none() {
            spec.undo.session = self.undo_session;
        }
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

    #[cfg(feature = "test-hooks")]
    pub fn observe_cloud_reader_wait_for_testing(&self, sender: std::sync::mpsc::Sender<bool>) {
        self.core.readers.observe_cloud_wait_for_testing(sender);
    }

    /// Probe the actual Cloud slot mutex, without taking a reader.
    #[cfg(feature = "test-hooks")]
    pub fn cloud_reader_slot_for_testing(&self) -> Option<bool> {
        self.core.readers.cloud_slot_for_testing()
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
        self.reader_checked(role, index, false)
    }

    /// Live backup admission is checked while its maintenance lease is held.
    pub fn backup_reader(&self) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_checked(ReaderRole::Maintenance, 0, true)
    }

    pub(crate) fn operation_reader(&self, role: ReaderRole) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_checked(role, 0, true)
    }

    fn reader_checked(
        &self,
        role: ReaderRole,
        index: usize,
        require_admission: bool,
    ) -> Result<ReaderLease<'_>, EngineError> {
        if role == ReaderRole::Cloud && crate::executor::writer::on_writer_thread() {
            return Err(EngineError::InvalidArgument {
                message: "the Cloud reader must be leased off the writer".into(),
            });
        }
        #[cfg(test)]
        tests::before_lease();
        let mut guard = self.core.readers.lease(role, index, || {
            self.check_current()?;
            if require_admission {
                self.core.exec.require_admission(self.privilege)?;
            }
            Ok(())
        })?;
        if let (Some(attachment), Some(reader)) = (self.core.derived.get(), guard.as_mut()) {
            attachment.ensure(reader, self.generation);
        }
        Ok(ReaderLease::new(guard, &self.core.exec, self.generation))
    }

    /// Exercise the writer-loop fallback independently of CloseTask.
    #[cfg(feature = "test-hooks")]
    pub fn stop_writer_for_testing(&self) {
        self.core.exec.wait_idle();
        self.core.readers.close_all();
        self.core.exec.stop();
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

impl Collection {
    /// Explicit local choice can only restrict publication. It grants no
    /// sending token and retains replacement evidence and publication identity.
    pub fn choose_cloud_local_only<T, E>(
        &self, expected: &crate::device_state::PublicationDecisionState, native_file: (u64, u64),
        with_choice: impl FnOnce(&mut dyn FnMut() -> Result<bool, EngineError>) -> Result<T, E>,
    ) -> Result<T, E>
    where E: From<EngineError> {
        let _lease = self.reader(ReaderRole::Cloud)?;
        if self.open_evidence()?.native_file != native_file {
            return Err(EngineError::unavailable("local choice file changed").into());
        }
        // Acquire the reader before the caller's state guard. Its checked
        // ownership remains held through this restrictive sidecar comparison.
        with_choice(&mut || {
            let mut binding = self.core.publication_binding.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut replacement = expected.clone();
            replacement.publication.binding = crate::device_state::AccountBinding::LocalOnly;
            replacement.publication.decision = crate::device_state::PublicationDecision::Clear;
            replacement.pending_reason = None;
            let changed = crate::device_state::DeviceState::for_collection(self.path())
                .compare_exchange_publication_decision(expected, &replacement)?;
            if changed { *binding = replacement.publication.binding; }
            Ok(changed)
        })
    }

    pub fn resolve_cloud_publication_decision(
        &self,
        operation: [u8; 16],
        expected: &crate::device_state::PublicationDecisionState,
    ) -> Result<bool, EngineError> {
        let _lease = self.reader(ReaderRole::Cloud)?;
        let provider =
            self.core
                .cloud_publication_authority
                .as_ref()
                .ok_or_else(|| EngineError::CollectionUnavailable {
                    reason: "durable cloud publication authority is unavailable".into(),
                })?;
        // Providers read platform inputs before their locks. Their apply callback
        // holds the durable row stable and never calls Runtime or the writer.
        provider.with_resolution(self, operation, expected, &mut |replacement| {
            let mut binding = self
                .core
                .publication_binding
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let changed = crate::device_state::DeviceState::for_collection(self.path())
                .compare_exchange_publication_decision(expected, replacement)?;
            if changed {
                *binding = replacement.publication.binding.clone();
            }
            Ok(changed)
        })
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

    #[test]
    fn a_transferred_reader_waiter_rechecks_generation_after_replacement() {
        let (_dir, engine, old) = crate::readers::tests::open();
        let lease = old.reader(ReaderRole::Cloud).unwrap();
        let pin = lease
            .into_lifetime_pin()
            .unwrap_or_else(|(_, error)| panic!("{error:?}"));
        let (waiting, reached) = mpsc::channel();
        let (woken, paused) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let current = old.clone();
        let requester = thread::spawn(move || {
            crate::readers::tests::on_waiting(move || waiting.send(()).unwrap());
            // Hold the actual transferred waiter after its condvar wake,
            // outside the slot mutex, until replacement has finished.
            crate::readers::tests::on_woken(move || {
                woken.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            current.reader(ReaderRole::Cloud).map(|_| ())
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(pin);
        paused.recv_timeout(Duration::from_secs(5)).unwrap();
        let replacement = old.path().with_file_name("replacement.anki2");
        std::fs::copy(fixture(), &replacement).unwrap();
        let slot = engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(false).unwrap();
        exclusive
            .replace(move |live| {
                std::fs::rename(&replacement, live).map_err(|error| EngineError::internal(error.to_string()))
            })
            .unwrap();
        drop(exclusive);
        drop(slot);
        release.send(()).unwrap();
        assert_eq!(
            requester.join().unwrap(),
            Err(EngineError::StaleGeneration {
                planned: Generation(1),
                current: Generation(2),
            })
        );
        engine.close().unwrap();
    }

    #[test]
    fn a_transferred_operation_reader_waiter_rechecks_admission_after_wake() {
        let (_dir, engine, collection) = crate::readers::tests::open();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let pin = lease
            .into_lifetime_pin()
            .unwrap_or_else(|(_, error)| panic!("{error:?}"));
        let (waiting, reached) = mpsc::channel();
        let current = collection.clone();
        let requester = thread::spawn(move || {
            crate::readers::tests::on_waiting(move || waiting.send(()).unwrap());
            current.operation_reader(ReaderRole::Cloud).map(|_| ())
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let slot = engine.acquire_slot(LongOp::Restore).unwrap();
        let exclusive = slot.enter_exclusive(false).unwrap();
        drop(pin);
        assert!(matches!(
            requester.join().unwrap(),
            Err(EngineError::CollectionUnavailable { .. })
        ));
        drop(exclusive);
        drop(slot);
        assert!(collection.operation_reader(ReaderRole::Cloud).is_ok());
        engine.close().unwrap();
    }
}
