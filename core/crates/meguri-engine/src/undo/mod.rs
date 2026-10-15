//! Undo and redo (C3, `specs/engine/undo.md`): steps recorded from the
//! journals of `UserUndoable` ops, the effect every other op has on them,
//! undo and redo as ops, the held-back run a normal sync keeps local, and
//! the core rule that no uploaded step is ever undone.
//!
//! The history of an open collection lives in its executor and is changed on
//! the writer, right after each commit and before the op's result is
//! returned; callers read it with `peek` from any thread.

pub mod entry;
pub mod holdback;
pub mod reasons;
pub mod restorers;
pub(crate) mod stack;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_model::changes::{ChangeSet, OpSource};
use meguri_storage::{Journal, StampMode};

use crate::changes::stream::Seq;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::catalog;
use crate::executor::op::{OpContext, OpResult, OpSpec, Pending};
use entry::{Entry, EntryId, RestorerState, SessionId, StepInfo, StepKind, journal_targets};
use holdback::{HeldBackRun, SyncHold, SyncMerge};
use reasons::{ClearCause, DisabledReason, SourceEffect, source_effect};
use restorers::{Direction, RestoredStep, UndoRestorer};
use stack::{Side, UndoStack};

/// Which steps a call may act on (M3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UndoScope {
    /// The top step, whatever its session.
    Global,
    /// The top step, only when it belongs to this session.
    Session(SessionId),
}

/// What `peek` reports (M3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoStatus {
    Available(StepInfo),
    Unavailable(DisabledReason),
}

impl UndoStatus {
    /// The available step's name.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            UndoStatus::Available(step) => Some(step.name),
            UndoStatus::Unavailable(_) => None,
        }
    }

    /// Why nothing is available; `None` when a step is.
    pub fn reason(&self) -> Option<DisabledReason> {
        match self {
            UndoStatus::Available(_) => None,
            UndoStatus::Unavailable(reason) => Some(*reason),
        }
    }
}

/// An op that undoes or redoes the top step of its scope (M4, M5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UndoAction {
    Undo(UndoScope),
    Redo(UndoScope),
}

impl UndoAction {
    fn side(self) -> Side {
        match self {
            UndoAction::Undo(_) => Side::Undo,
            UndoAction::Redo(_) => Side::Redo,
        }
    }

    fn scope(self) -> UndoScope {
        match self {
            UndoAction::Undo(scope) | UndoAction::Redo(scope) => scope,
        }
    }

    fn direction(self) -> Direction {
        match self {
            UndoAction::Undo(_) => Direction::Undo,
            UndoAction::Redo(_) => Direction::Redo,
        }
    }
}

/// What the op spec says about undo: the step it records and how.
#[derive(Clone, Debug, Default)]
pub(crate) struct UndoSpec {
    pub step: Option<(&'static str, StepKind)>,
    pub session: Option<SessionId>,
    pub skip: bool,
    pub action: Option<UndoAction>,
}

/// What an op's closure left for the undo manager.
#[derive(Default)]
pub(crate) struct UndoNotes {
    pub state: Option<RestorerState>,
    pub merge: Option<SyncMerge>,
    /// The step an undo or redo op restored.
    pub restored: Option<EntryId>,
}

/// A committed op, as the undo manager sees it.
pub(crate) struct CommitInfo<'a> {
    pub seq: Seq,
    pub name: &'static str,
    pub source: OpSource,
    pub spec: &'a UndoSpec,
    pub declared: Option<std::time::Duration>,
    pub journal: Option<Arc<Journal>>,
    pub changes: &'a ChangeSet,
    pub notes: UndoNotes,
}

/// What the commit did to the history.
pub(crate) enum Applied {
    Nothing,
    Recorded(StepInfo),
    Restored {
        direction: Direction,
        step: StepInfo,
        state: Option<RestorerState>,
    },
}

/// The undo manager of one open collection.
#[derive(Default)]
pub(crate) struct UndoManager {
    stack: Mutex<UndoStack>,
    restorers: Mutex<Vec<Arc<dyn UndoRestorer>>>,
}

impl UndoManager {
    fn lock(&self) -> MutexGuard<'_, UndoStack> {
        self.stack.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn restorers(&self) -> Vec<Arc<dyn UndoRestorer>> {
        self.restorers.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn add_restorer(&self, restorer: Arc<dyn UndoRestorer>) {
        self.restorers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(restorer);
    }

    pub(crate) fn peek(&self, side: Side, scope: UndoScope) -> UndoStatus {
        self.lock().peek(side, scope)
    }

    /// M6: replacement and close.
    pub(crate) fn reset(&self, reason: DisabledReason) {
        self.lock().reset(reason);
    }

    /// M2, M4.5, M5: applies a committed op's effect, on the writer.
    pub(crate) fn after_commit(&self, commit: CommitInfo<'_>) -> Applied {
        let changed = commit.journal.as_ref().is_some_and(|journal| !journal.is_empty());
        let mut stack = self.lock();
        if let Some(action) = commit.spec.action {
            let (Some(target), Some(journal)) = (commit.notes.restored, commit.journal) else {
                return Applied::Nothing;
            };
            let mut applied = Applied::Nothing;
            stack.restored(action.side(), target, |entry| {
                applied = Applied::Restored {
                    direction: action.direction(),
                    step: entry.info.clone(),
                    state: entry.state.clone(),
                };
                entry.inverse(commit.seq.into(), journal, commit.changes)
            });
            return applied;
        }
        if commit.spec.skip {
            return Applied::Nothing;
        }
        match source_effect(commit.source) {
            SourceEffect::Recorded => {
                let Some(journal) = commit.journal.filter(|_| changed) else {
                    return Applied::Nothing;
                };
                let (name, kind) = commit.spec.step.unwrap_or((commit.name, StepKind::Other));
                let info = StepInfo {
                    id: commit.seq.into(),
                    name,
                    kind,
                    session: commit.spec.session,
                    long_write: commit.declared,
                };
                stack.record(Entry::recorded(
                    info.clone(),
                    journal,
                    commit.changes,
                    commit.notes.state,
                ));
                Applied::Recorded(info)
            }
            SourceEffect::ClearAll(reason) => {
                if changed {
                    stack.clear_all(reason);
                }
                Applied::Nothing
            }
            SourceEffect::TruncateIntersecting => {
                if let Some(journal) = commit.journal.as_deref() {
                    stack.truncate_intersecting(&journal_targets(journal));
                }
                Applied::Nothing
            }
            SourceEffect::SyncMerge => {
                match commit.notes.merge {
                    Some(merge) => stack.merge(Some(merge.snapshot), &merge.staged),
                    None => {
                        if let Some(journal) = commit.journal.as_deref() {
                            stack.merge(None, &journal_targets(journal));
                        }
                    }
                }
                Applied::Nothing
            }
        }
    }

    /// M10: runs the restorers after an undo or redo committed.
    pub(crate) fn notify_restorers(&self, applied: &Applied, changes: &ChangeSet) {
        let Applied::Restored { direction, step, state } = applied else {
            return;
        };
        let restorers = self.restorers();
        if restorers.is_empty() {
            return;
        }
        restorers::notify(
            &restorers,
            &RestoredStep {
                direction: *direction,
                step,
                changes,
                state: state.as_deref(),
            },
        );
    }
}

impl<'c> OpContext<'_, 'c> {
    /// Attaches state for the restorers (M10) to the step this op records;
    /// its undo and redo hand it back.
    pub fn set_undo_state(&mut self, state: RestorerState) {
        self.undo_notes.state = Some(state);
    }

    /// Reports a `SyncIncoming` merge (M9.4). Refused for other sources.
    pub fn report_sync_merge(&mut self, merge: SyncMerge) -> Result<(), EngineError> {
        if self.source != OpSource::SyncIncoming {
            return Err(EngineError::InvalidArgument {
                message: format!("a {} op cannot report a sync merge", self.source),
            });
        }
        self.undo_notes.merge = Some(merge);
        Ok(())
    }

    /// M9.1: a normal sync's barrier. Returns the snapshot seq (the last
    /// committed op) and the held-back run of `session`, and flags every
    /// other step up to the snapshot upload-pending. Call it from an op that
    /// writes nothing; the snapshot is what readers see while it runs.
    pub fn begin_sync_hold(&mut self, session: Option<SessionId>) -> Result<SyncHold, EngineError> {
        let snapshot = self.last_seq;
        let run = self.undo.lock().begin_sync(session, snapshot);
        Ok(SyncHold {
            snapshot,
            run: holdback::run_value(&run)?,
        })
    }

    /// The undo or redo itself (M4, M5), inside the op's transaction.
    fn restore_top(&mut self, action: UndoAction) -> Result<StepInfo, EngineError> {
        let side = action.side();
        let entry = self
            .undo
            .lock()
            .target(side, action.scope())
            .cloned()
            .map_err(|reason| EngineError::UndoUnavailable { reason })?;
        if !entry.still_applied(self.scope)? {
            self.undo.lock().drop_changed(side, entry.info.id);
            return Err(EngineError::UndoUnavailable {
                reason: DisabledReason::ChangedExternally,
            });
        }
        entry.restore(self.scope)?;
        self.undo_notes.restored = Some(entry.info.id);
        Ok(entry.info)
    }
}

impl Collection {
    fn undo_manager(&self) -> Result<&UndoManager, EngineError> {
        self.check_current()?;
        Ok(&self.core.exec.undo)
    }

    /// The step `undo(scope)` would undo, or why there is none (M3).
    pub fn peek_undo(&self, scope: UndoScope) -> Result<UndoStatus, EngineError> {
        Ok(self.undo_manager()?.peek(Side::Undo, scope))
    }

    /// The step `redo(scope)` would redo, or `None`.
    pub fn peek_redo(&self, scope: UndoScope) -> Result<UndoStatus, EngineError> {
        Ok(self.undo_manager()?.peek(Side::Redo, scope))
    }

    /// Undoes the top step of `scope` as an op (M4). Refused with
    /// `UndoUnavailable` when there is none, it was uploaded or is pending
    /// upload, or its rows changed.
    pub fn submit_undo(&self, scope: UndoScope) -> Pending<OpResult<StepInfo>> {
        self.submit_restore(UndoAction::Undo(scope))
    }

    pub fn undo(&self, scope: UndoScope) -> Result<OpResult<StepInfo>, EngineError> {
        self.submit_undo(scope).wait()
    }

    /// Redoes the top redo step of `scope` as an op (M5).
    pub fn submit_redo(&self, scope: UndoScope) -> Pending<OpResult<StepInfo>> {
        self.submit_restore(UndoAction::Redo(scope))
    }

    pub fn redo(&self, scope: UndoScope) -> Result<OpResult<StepInfo>, EngineError> {
        self.submit_redo(scope).wait()
    }

    fn submit_restore(&self, action: UndoAction) -> Pending<OpResult<StepInfo>> {
        // A step of a job-run op is undone with its declared write (M4);
        // only then is the op a catalog long write.
        let declared = match self.core.exec.undo.peek(action.side(), action.scope()) {
            UndoStatus::Available(step) => step.long_write,
            UndoStatus::Unavailable(_) => None,
        };
        let def = match (action, declared.is_some()) {
            (UndoAction::Undo(_), false) => catalog::UNDO,
            (UndoAction::Redo(_), false) => catalog::REDO,
            (UndoAction::Undo(_), true) => catalog::UNDO_LONG_WRITE,
            (UndoAction::Redo(_), true) => catalog::REDO_LONG_WRITE,
        };
        let mut spec = OpSpec::new(def, OpSource::UserUndoable).stamp(StampMode::CallerSupplied);
        if let Some(declared) = declared {
            spec = spec.declare_long_write(declared);
        }
        spec.undo.action = Some(action);
        self.submit(spec, move |ctx| ctx.restore_top(action))
    }

    /// Clears the whole history (M6): rollover, Check Database, an outside
    /// change.
    pub fn clear_undo(&self, cause: ClearCause) -> Result<(), EngineError> {
        self.undo_manager()?.lock().clear_all(cause.reason());
        Ok(())
    }

    /// The held-back run of `session` now (M8).
    pub fn held_back_run(&self, session: Option<SessionId>) -> Result<HeldBackRun, EngineError> {
        let run = self.undo_manager()?.lock().run(session);
        holdback::run_value(&run)
    }

    /// M9.2: the sync sent its `finish` request; pending steps are uploaded
    /// for good.
    pub fn sync_finish_sent(&self) -> Result<(), EngineError> {
        self.undo_manager()?.lock().finish_sent();
        Ok(())
    }

    /// M9.3: the sync failed before sending `finish`; pending steps are
    /// undoable again.
    pub fn sync_failed_before_finish(&self) -> Result<(), EngineError> {
        self.undo_manager()?.lock().failed_before_finish();
        Ok(())
    }

    /// Registers a restorer (M10) for this collection, until it closes.
    pub fn add_undo_restorer(&self, restorer: Arc<dyn UndoRestorer>) {
        self.core.exec.undo.add_restorer(restorer);
    }
}
