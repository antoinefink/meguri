//! WAL coverage: storage C9.2.
//! Lock order: `commit` -> `coverage`; waiters omit `commit`.
//! Release locks before xSync or callbacks.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use meguri_storage::Writer;

use crate::{EngineError, Generation, Seq, executor::ExecShared, monotonic};

/// The collection incarnation and physical file a snapshot/receipt describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalIdentity {
    generation: Generation,
    native_file: (u64, u64),
}
impl WalIdentity {
    pub(crate) fn new(generation: Generation, native_file: (u64, u64)) -> Self {
        Self {
            generation,
            native_file,
        }
    }
    pub fn generation(self) -> Generation {
        self.generation
    }
    pub fn native_file(self) -> (u64, u64) {
        self.native_file
    }
}

/// Constructed only by the engine after real WAL sync/checkpoint coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalReceipt {
    identity: WalIdentity,
    through: Seq,
}
impl WalReceipt {
    pub fn identity(self) -> WalIdentity {
        self.identity
    }
    pub fn through(self) -> Seq {
        self.through
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalCoverage {
    Covered(WalReceipt),
    Timeout,
    /// Closed without coverage, failed sync, stale incarnation or wrong file.
    Unavailable,
}

#[derive(Default)]
pub(crate) struct Coverage {
    state: Mutex<State>,
    changed: Condvar,
}
#[derive(Default)]
struct State {
    identity: Option<WalIdentity>,
    incarnation: u64,
    through: Seq,
    version: Option<u32>,
    open: bool,
    failed: bool,
    failures: u64,
    #[cfg(feature = "test-hooks")]
    on_wait: Option<std::sync::mpsc::Sender<()>>,
    // Maintenance can write without taking an op seq. Until its token is
    // checked/synced, even a snapshot at the old seq must await coverage.
    unsequenced: bool,
    busy: bool,
    committing: bool,
    dirty: bool,
}

impl Coverage {
    pub(crate) fn opened(&self, shared: &ExecShared, writer: &Writer) -> Result<(), EngineError> {
        let identity = WalIdentity::new(shared.generation(), writer.validate_native_identity()?);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let incarnation = state.incarnation.wrapping_add(1);
        *state = State {
            identity: Some(identity),
            incarnation,
            open: true,
            unsequenced: true,
            ..State::default()
        };
        self.changed.notify_all();
        Ok(())
    }

    pub(crate) fn commit_started(&self) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).committing = true;
    }

    /// Called under commit after SQLite finishes, before the new seq can be
    /// treated as covered. The wait path never needs the writer's commit lock.
    pub(crate) fn commit_finished(
        &self,
        shared: &ExecShared,
        writer: &Writer,
        seq: Option<Seq>,
        pre_commit_version: Option<u32>,
        committed: Option<&meguri_storage::Committed>,
    ) {
        let version = writer.data_version().ok();
        let native = writer.validate_native_identity().ok();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.committing = false;
        if native.is_none() || version.is_none() || state.identity.map(|i| i.native_file) != native {
            state.failed = true;
            state.failures = state.failures.wrapping_add(1);
        } else {
            // Committed::changed proves no write only against clean prior coverage.
            if seq.is_some()
                && committed.is_some_and(|c| !c.changed && c.post_commit_error.is_none())
                && pre_commit_version.is_some()
                && state.version == pre_commit_version
                && state.open
                && !state.failed
                && !state.unsequenced
                && !state.busy
                && !state.dirty
                && state.identity == native.map(|native| WalIdentity::new(shared.generation(), native))
            {
                state.version = version;
            }
            state.dirty |= version != state.version;
            if seq.is_none() && state.dirty {
                state.unsequenced = true;
            }
            if let Some(seq) = seq
                && state.open
                && !state.failed
                && !state.unsequenced
                && !state.dirty
            {
                state.through = state.through.max(seq);
            }
        }
        self.changed.notify_all();
    }

    pub(crate) fn maintenance_started(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.busy = true;
    }
    pub(crate) fn maintenance_finished(&self, writer: &Writer) {
        let version = writer.data_version().ok();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.busy = false;
        state.unsequenced = version.is_none() || version != state.version;
        state.dirty = state.unsequenced;
        self.changed.notify_all();
    }

    pub(crate) fn is_synced_version(&self, version: u32) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.open
            && !state.failed
            && !state.unsequenced
            && !state.busy
            && !state.committing
            && state.version == Some(version)
    }

    pub(crate) fn failed(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.failed = true;
        state.committing = false;
        state.failures = state.failures.wrapping_add(1);
        self.changed.notify_all();
    }

    /// Actual storage sync, outside all coverage/commit locks. Capturing the
    /// last *taken* seq covers event-only positions too; commit order alone
    /// never calls this success path.
    pub(crate) fn sync(&self, shared: &ExecShared, writer: &Writer) -> Result<bool, EngineError> {
        let result = (|| {
            let native = writer.validate_native_identity()?;
            let version = writer.data_version()?;
            let identity = WalIdentity::new(shared.generation(), native);
            let through = {
                let _commit = shared.lock_commit();
                Seq(shared.stream.next_seq().0 - 1)
            };
            let synced = writer.sync_wal()?;
            if writer.validate_native_identity()? != native {
                return Err(EngineError::unavailable("collection file changed during WAL sync"));
            }
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if !state.open || state.identity != Some(identity) {
                return Err(EngineError::unavailable("WAL sync belongs to another incarnation"));
            }
            state.through = state.through.max(through);
            state.version = Some(version);
            state.failed = false;
            state.unsequenced = false;
            state.dirty = false;
            state.committing = false;
            self.changed.notify_all();
            Ok(synced)
        })();
        if result.is_err() {
            self.failed();
        }
        result
    }

    /// Called after a successful checkpoint, or after the connection has
    /// closed. A failed close retains the previous watermark, wakes waiters
    /// unavailable, and cannot transfer it to a subsequent writer.
    pub(crate) fn closed(&self, shared: &ExecShared, identity: Option<WalIdentity>, covered: bool) {
        let through = {
            let _commit = shared.lock_commit();
            Seq(shared.stream.next_seq().0 - 1)
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if covered && identity.is_some() && state.identity == identity {
            state.through = state.through.max(through);
            state.unsequenced = false;
            state.failed = false;
        } else {
            state.failed = true;
        }
        state.open = false;
        state.busy = false;
        self.changed.notify_all();
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn notify_next_wait(&self, sender: std::sync::mpsc::Sender<()>) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).on_wait = Some(sender);
    }

    pub(crate) fn wait(
        &self,
        shared: &ExecShared,
        handle_generation: Generation,
        identity: WalIdentity,
        seq: Seq,
        timeout: Duration,
    ) -> Result<WalCoverage, EngineError> {
        if crate::executor::writer::on_writer_thread() {
            return Err(EngineError::InvalidArgument {
                message: "WAL coverage waits must run off the writer".into(),
            });
        }
        let deadline = monotonic::now().saturating_add(timeout);
        let mut incarnation = None;
        let mut failures = None;
        loop {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if identity.generation != handle_generation
                || shared.generation() != handle_generation
                || state.identity.is_some_and(|current| current != identity)
                || incarnation.is_some_and(|i| i != state.incarnation)
                || failures.is_some_and(|count| count != state.failures)
            {
                return Ok(WalCoverage::Unavailable);
            }
            if state.identity.is_some() {
                incarnation = Some(state.incarnation);
                failures = Some(state.failures);
            }
            // Event-only seqs require no new I/O while every commit is covered.
            // Never infer this while maintenance has no associated op seq.
            let through = if state.open
                && !state.failed
                && !state.unsequenced
                && !state.busy
                && !state.committing
                && !state.dirty
            {
                state.through.max(Seq(shared.stream.next_seq().0 - 1))
            } else {
                state.through
            };
            if state.identity.is_some() && !state.unsequenced && !state.busy && seq <= through {
                return Ok(WalCoverage::Covered(WalReceipt { identity, through }));
            }
            if (state.identity.is_some() && !state.open) || state.failed {
                return Ok(WalCoverage::Unavailable);
            }
            let remaining = deadline.saturating_sub(monotonic::now());
            if remaining.is_zero() {
                return Ok(WalCoverage::Timeout);
            }
            #[cfg(feature = "test-hooks")]
            if let Some(sender) = state.on_wait.take() {
                let _ = sender.send(());
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            drop(state);
        }
    }
}
