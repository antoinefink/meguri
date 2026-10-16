//! Ops as callers see them (`engine/op-model` O1, O3 to O5): the spec an op
//! is submitted with, the context its closure writes through, its result,
//! and the pending result a submit returns.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use meguri_model::changes::{ChangeSet, OpSource};
use meguri_model::time::EpochMillis;
use meguri_storage::{StampMode, WriteScope};

use crate::cancel::CancelToken;
use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::executor::catalog::OpDef;
use crate::executor::preconditions::Precondition;
use crate::lifecycle::Generation;
use crate::progress::ProgressSink;
use crate::undo::entry::{SessionId, StepKind};
use crate::undo::{Restore, UndoManager, UndoNotes, UndoSpec};

/// How an op is to run.
#[derive(Clone, Debug)]
pub struct OpSpec {
    pub(crate) def: OpDef,
    pub(crate) source: OpSource,
    pub(crate) stamp: Option<StampMode>,
    pub(crate) preconditions: Vec<Precondition>,
    pub(crate) cancel: Option<Arc<CancelToken>>,
    pub(crate) progress: Option<ProgressSink>,
    pub(crate) park: bool,
    pub(crate) declared: Option<Duration>,
    pub(crate) plan_time: Option<Duration>,
    pub(crate) any_generation: bool,
    pub(crate) undo: UndoSpec,
}

impl OpSpec {
    pub fn new(def: OpDef, source: OpSource) -> OpSpec {
        OpSpec {
            def,
            source,
            stamp: None,
            preconditions: Vec::new(),
            cancel: None,
            progress: None,
            park: false,
            declared: None,
            plan_time: None,
            any_generation: false,
            undo: UndoSpec::default(),
        }
    }

    /// A plain op (no catalog entry).
    pub fn named(name: &'static str, source: OpSource) -> OpSpec {
        OpSpec::new(OpDef::new(name), source)
    }

    pub fn def(&self) -> OpDef {
        self.def
    }

    pub fn source(&self) -> OpSource {
        self.source
    }

    /// Writes with `mode` instead of the source's default (O1).
    pub fn stamp(mut self, mode: StampMode) -> OpSpec {
        self.stamp = Some(mode);
        self
    }

    pub fn precondition(mut self, precondition: Precondition) -> OpSpec {
        self.preconditions.push(precondition);
        self
    }

    pub fn preconditions(mut self, preconditions: impl IntoIterator<Item = Precondition>) -> OpSpec {
        self.preconditions.extend(preconditions);
        self
    }

    pub fn cancel(mut self, token: Arc<CancelToken>) -> OpSpec {
        self.cancel = Some(token);
        self
    }

    pub fn progress(mut self, sink: ProgressSink) -> OpSpec {
        self.progress = Some(sink);
        self
    }

    /// Waits outside the queues while the collection is unavailable instead
    /// of being refused (`engine/lifecycle` L4).
    pub fn park_when_unavailable(mut self) -> OpSpec {
        self.park = true;
        self
    }

    /// The op read nothing before it was submitted, so it is not bound to the
    /// generation of the handle it is submitted through: parked across a
    /// collection swap, it runs on the new generation instead of failing
    /// with `StaleGeneration` (`engine/lifecycle` L4).
    pub fn run_on_any_generation(mut self) -> OpSpec {
        self.any_generation = true;
        self
    }

    /// Declares a write phase of about `expected` (O8). Only catalog ops
    /// that may declare a long write are accepted; others are refused with
    /// `InvalidArgument` at submit.
    pub fn declare_long_write(mut self, expected: Duration) -> OpSpec {
        self.declared = Some(expected);
        self
    }

    /// The name and kind of the undo step a `UserUndoable` op records
    /// (`engine/undo` M1): Anki's op name ("Answer Card", R370). Without it
    /// the step takes the op's name and kind `Other`.
    pub fn undo_step(mut self, name: &'static str, kind: StepKind) -> OpSpec {
        self.undo.step = Some((name, kind));
        self
    }

    /// The review session the op belongs to (`engine/undo` M3, M8).
    pub fn session(mut self, session: SessionId) -> OpSpec {
        self.undo.session = Some(session);
        self
    }

    /// Neither recorded nor clearing the history (`engine/undo` M2): only for
    /// `UserNonUndoable` ops such as selecting a deck (R42). Other sources are
    /// refused with `InvalidArgument` at submit.
    pub fn skip_undo(mut self) -> OpSpec {
        self.undo.skip = true;
        self
    }

    pub(crate) fn stamp_mode(&self) -> StampMode {
        self.stamp
            .unwrap_or_else(|| crate::executor::source::default_stamp(self.source))
    }
}

/// What an op's closure writes through: its one write scope, plus what the
/// change set needs that the journal does not hold.
pub struct OpContext<'s, 'c> {
    pub(crate) scope: &'s mut WriteScope<'c>,
    pub(crate) source: OpSource,
    pub(crate) generation: Generation,
    pub(crate) cancel: Option<Arc<CancelToken>>,
    pub(crate) cancellable_write: bool,
    pub(crate) progress: Option<ProgressSink>,
    pub(crate) media_added: Vec<String>,
    pub(crate) media_removed: Vec<String>,
    pub(crate) undo: &'c UndoManager,
    pub(crate) undo_notes: UndoNotes,
    /// For an undo or redo op, the step the writer resolved for it (M4.1).
    pub(crate) restore: Option<Restore>,
    /// The seq of the last committed op: what a snapshot begun now sees.
    pub(crate) last_seq: Seq,
}

impl fmt::Debug for OpContext<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpContext")
            .field("source", &self.source)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl<'c> OpContext<'_, 'c> {
    /// The op's write scope: every write and every read inside the op.
    pub fn scope(&mut self) -> &mut WriteScope<'c> {
        self.scope
    }

    pub fn source(&self) -> OpSource {
        self.source
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// The clock's current instant.
    pub fn now(&self) -> EpochMillis {
        self.scope.now()
    }

    pub fn progress(&self) -> Option<&ProgressSink> {
        self.progress.as_ref()
    }

    /// Runs one step of a composite op in a savepoint (O3): when the step
    /// returns an error, its writes, journal entries, media notes and undo
    /// notes (restorer state, sync merge) are rolled back and the error is
    /// returned for the op to handle. The op keeps one transaction and one
    /// journal.
    pub fn step<T>(&mut self, step: impl FnOnce(&mut Self) -> Result<T, EngineError>) -> Result<T, EngineError> {
        let savepoint = self.scope.savepoint()?;
        let media = (self.media_added.len(), self.media_removed.len());
        // The step notes into an empty set, kept only when it commits.
        let before = std::mem::take(&mut self.undo_notes);
        let outcome = step(self);
        let noted = std::mem::replace(&mut self.undo_notes, before);
        match outcome {
            Ok(value) => {
                self.scope.release_savepoint(savepoint)?;
                self.undo_notes.absorb(noted);
                Ok(value)
            }
            Err(error) => {
                self.scope.rollback_to_savepoint(savepoint)?;
                self.media_added.truncate(media.0);
                self.media_removed.truncate(media.1);
                Err(error)
            }
        }
    }

    /// Reports a media file the op added (U31 stages media inside the op).
    pub fn media_added(&mut self, name: impl Into<String>) {
        self.media_added.push(name.into());
    }

    /// Reports a media file the op removed.
    pub fn media_removed(&mut self, name: impl Into<String>) {
        self.media_removed.push(name.into());
    }

    /// Between batches of a write phase the catalog declares cancellable
    /// (O5): `Err(Cancelled)` once cancel was requested, which rolls the op
    /// back. For every other op the write phase is past the cancel point and
    /// this is always `Ok`.
    pub fn check_cancel(&self) -> Result<(), EngineError> {
        match &self.cancel {
            Some(token) if self.cancellable_write => token.check(),
            _ => Ok(()),
        }
    }
}

/// The undo step an op recorded (`engine/undo` M1): set for a `UserUndoable`
/// op that changed something and for a redo. The step's id is the op's seq.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoMeta {
    pub name: &'static str,
    pub source: OpSource,
}

/// Where the op's time went (O10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpTiming {
    /// From submit to the writer taking the op.
    pub queued: Duration,
    /// Planning on a snapshot, for ops that came through plan/commit.
    pub plan: Option<Duration>,
    /// The write phase: from the writer taking the op to its publication.
    pub write: Duration,
    /// Rows the journal holds (`col` included).
    pub rows: usize,
}

/// A committed op.
#[derive(Clone, Debug)]
pub struct OpResult<T> {
    pub value: T,
    pub seq: Seq,
    pub changes: Arc<ChangeSet>,
    pub undo: Option<UndoMeta>,
    pub generation: Generation,
    /// Cancel was requested after the write phase started; the op completed
    /// anyway (O5).
    pub past_cancel_point: bool,
    pub timing: OpTiming,
}

/// The result of a submitted op or task, delivered once.
#[derive(Debug)]
pub struct Pending<R> {
    pub(crate) rx: Receiver<Result<R, EngineError>>,
}

impl<R> Pending<R> {
    /// Waits for the result.
    pub fn wait(self) -> Result<R, EngineError> {
        self.rx
            .recv()
            .unwrap_or_else(|_| Err(EngineError::internal("the writer dropped the op without a result")))
    }

    /// Waits at most `timeout`; `None` when the result is not there yet.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Result<R, EngineError>> {
        match self.rx.recv_timeout(timeout) {
            Ok(result) => Some(result),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                Some(Err(EngineError::internal("the writer dropped the op without a result")))
            }
        }
    }

    /// The result if it is there.
    pub fn try_take(&self) -> Option<Result<R, EngineError>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(EngineError::internal("the writer dropped the op without a result")))
            }
        }
    }
}

/// A declared long write or maintenance task on the writer now (O8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteDelay {
    pub op: &'static str,
    pub declared: Duration,
}
