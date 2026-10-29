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
pub mod names;
pub mod reasons;
pub mod restorers;
pub(crate) mod stack;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_model::changes::{ChangeSet, OpSource};
use meguri_storage::{Journal, ReadRows, RowKey, StampMode, Table};

use crate::changes::stream::Seq;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::catalog;
use crate::executor::op::{OpContext, OpResult, OpSpec, Pending};
use entry::{Entry, EntryId, RestorerState, SessionId, StepInfo, StepKind, journal_targets};
use holdback::{CloudHold, HeldBackRun, SyncHold, SyncMerge};
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

    pub(crate) fn direction(self) -> Direction {
        match self {
            UndoAction::Undo(_) => Direction::Undo,
            UndoAction::Redo(_) => Direction::Redo,
        }
    }

    /// The catalog op it runs as: the long-write variant for a step whose
    /// op declared a long write (M4).
    fn def(self, long_write: bool) -> catalog::OpDef {
        match (self, long_write) {
            (UndoAction::Undo(_), false) => catalog::UNDO,
            (UndoAction::Redo(_), false) => catalog::REDO,
            (UndoAction::Undo(_), true) => catalog::UNDO_LONG_WRITE,
            (UndoAction::Redo(_), true) => catalog::REDO_LONG_WRITE,
        }
    }
}

/// An undo or redo op on the writer's queue (M4.1). The writer resolves its
/// step when it takes the op, before the op's write phase starts, so the op
/// declares the write of the step it restores even when that step was
/// recorded after the op was submitted.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Restore {
    pub action: UndoAction,
    /// The step on top of the action's scope when the writer took the op,
    /// or why there was none; `None` until then.
    pub target: Option<Result<EntryId, DisabledReason>>,
}

/// What the writer resolved for an undo or redo op it takes.
pub(crate) struct Resolved {
    pub target: Result<EntryId, DisabledReason>,
    pub def: catalog::OpDef,
    pub declared: Option<std::time::Duration>,
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
    /// `col.mod` when that undo or redo ran (A7).
    pub col_mod: Option<i64>,
    /// Clear the whole history when the op commits (M6): the day clock's
    /// rollover op (`sched/day-clock` D9).
    pub clear: Option<ClearCause>,
}

impl UndoNotes {
    /// Takes in the notes of a substep that committed (`OpContext::step`):
    /// what it set replaces what the op had set before it.
    pub(crate) fn absorb(&mut self, step: UndoNotes) {
        let UndoNotes {
            state,
            merge,
            restored,
            col_mod,
            clear,
        } = step;
        self.state = state.or(self.state.take());
        self.merge = merge.or(self.merge.take());
        self.restored = restored.or(self.restored);
        self.col_mod = col_mod.or(self.col_mod);
        self.clear = clear.or(self.clear);
    }
}

/// A committed op, as the undo manager sees it.
pub(crate) struct CommitInfo<'a> {
    pub seq: Seq,
    /// The epoch second at which the op began (A3).
    pub began: i64,
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
    CloudDetached(HeldBackRun),
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
    pub(crate) fn safety_state(&self, now: i64) -> (Option<u64>, Option<crate::ports::BackupReceipt>) {
        let stack = self.lock();
        if stack.safety_pending_commit {
            return (None, None);
        }
        let receipt = stack.safety_backup.as_ref().and_then(|(completed, receipt)| {
            now.checked_sub(*completed)
                .filter(|age| (0..600_000).contains(age))
                .map(|_| receipt.clone())
        });
        (Some(stack.safety_revision), receipt)
    }

    pub(crate) fn safety_commit_pending(&self) {
        self.lock().safety_pending_commit = true;
    }

    pub(crate) fn safety_completed(&self, revision: Option<u64>, completed: i64, receipt: crate::ports::BackupReceipt) {
        let mut stack = self.lock();
        if !stack.safety_pending_commit && revision == Some(stack.safety_revision) {
            stack.safety_backup = Some((completed, receipt));
        }
    }

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

    /// M4.1, on the writer as it takes an undo or redo op: the step the op
    /// acts on and the catalog op and write it runs with.
    pub(crate) fn resolve(&self, action: UndoAction) -> Resolved {
        let (target, declared) = match self.lock().target(action.side(), action.scope()) {
            Ok(entry) => (Ok(entry.info.id), entry.info.long_write),
            Err(reason) => (Err(reason), None),
        };
        Resolved {
            target,
            def: action.def(declared.is_some()),
            declared,
        }
    }

    /// M6: replacement and close.
    pub(crate) fn reset(&self, reason: DisabledReason) {
        self.lock().reset(reason);
    }

    /// M2, M4.5, M5: applies a committed op's effect, on the writer.
    pub(crate) fn after_commit(&self, commit: CommitInfo<'_>) -> Applied {
        let changed = commit.journal.as_ref().is_some_and(|journal| !journal.is_empty());
        let mut stack = self.lock();
        stack.safety_pending_commit = false;
        let safety_changed = changed || !commit.changes.is_empty();
        if safety_changed
            && (commit.source != OpSource::UserUndoable
                || commit.spec.skip
                || commit.spec.action.is_some()
                || commit.journal.is_none())
        {
            stack.invalidate_safety();
        }
        if let Some(cause) = commit.notes.clear {
            // M6, whether or not the op changed anything.
            stack.clear_all(cause.reason());
        }
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
                entry.inverse(
                    commit.seq.into(),
                    journal,
                    commit.changes,
                    commit.notes.col_mod,
                    commit.began,
                )
            });
            return applied;
        }
        if commit.spec.skip {
            // Keeps the steps; every op but undo and redo empties redo (A2).
            stack.clear_redo();
            return Applied::Nothing;
        }
        match source_effect(commit.source) {
            SourceEffect::Recorded => {
                let Some(journal) = commit.journal.filter(|_| changed) else {
                    stack.clear_redo();
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
                let holder = stack.record(Entry::recorded(
                    info,
                    journal,
                    commit.changes,
                    commit.notes.state,
                    commit.began,
                ));
                Applied::Recorded(holder)
            }
            SourceEffect::ClearAll(reason) => {
                if changed {
                    stack.clear_all(reason);
                }
                Applied::Nothing
            }
            SourceEffect::Kept => Applied::Nothing,
            SourceEffect::TruncateIntersecting => {
                if let Some(journal) = commit.journal.as_deref() {
                    stack.truncate_intersecting(&journal_targets(journal));
                }
                Applied::Nothing
            }
            SourceEffect::CloudTargetedDrop => commit
                .journal
                .as_deref()
                .and_then(|journal| stack.cloud_drop(commit.spec.session, &journal_targets(journal)))
                .map_or(Applied::Nothing, Applied::CloudDetached),
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

    /// Clears the whole history with `cause` when this op commits, whether
    /// or not it changed anything (M6): the day clock's rollover op
    /// (`sched/day-clock` D9).
    pub(crate) fn clear_undo_on_commit(&mut self, cause: ClearCause) {
        self.undo_notes.clear = Some(cause);
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
            run: holdback::run_value(&run),
        })
    }

    /// M9.6: a CloudKit barrier flags only named touched keys outside the
    /// active Cloud run. The snapshot is the last committed op's seq.
    /// This is a no-row barrier; it shares no manual-transfer capture state.
    pub fn begin_cloud_hold(
        &mut self,
        session: Option<SessionId>,
        keys: &[(Table, RowKey)],
    ) -> Result<CloudHold, EngineError> {
        if self.source != OpSource::CloudIncoming {
            return Err(EngineError::InvalidArgument {
                message: format!("a {} op cannot begin a CloudKit hold", self.source),
            });
        }
        Ok(self.undo.lock().begin_cloud(session, self.last_seq, keys))
    }

    /// The undo or redo itself (M4, M5), inside the op's transaction, on the
    /// step the writer resolved as it took the op (M4.1).
    fn restore_top(&mut self) -> Result<StepInfo, EngineError> {
        let Some(Restore {
            action,
            target: Some(target),
        }) = self.restore
        else {
            return Err(EngineError::internal("an undo or redo op ran without a resolved step"));
        };
        let unavailable = |reason| EngineError::UndoUnavailable { reason };
        let side = action.side();
        let id = target.map_err(unavailable)?;
        // A clear from another thread may have dropped the step since.
        let entry = self
            .undo
            .lock()
            .resolved_target(side, action.scope(), id)
            .cloned()
            .map_err(unavailable)?;
        if !entry.still_applied(self.scope)? {
            self.undo.lock().drop_changed(side, entry.info.id);
            return Err(EngineError::UndoUnavailable {
                reason: DisabledReason::ChangedExternally,
            });
        }
        let col_mod = self.scope.col()?.modified.0;
        entry.restore(self.scope, action.direction())?;
        self.undo_notes.restored = Some(entry.info.id);
        self.undo_notes.col_mod = Some(col_mod);
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

    /// Undoes the top step of `scope` as an op (M4): the step on top when
    /// the writer takes the op (M4.1). Refused with `UndoUnavailable` when
    /// there is none, it was uploaded or is pending upload, or its rows
    /// changed.
    pub fn submit_undo(&self, scope: UndoScope) -> Pending<OpResult<StepInfo>> {
        self.submit_undo_controlled(scope, crate::CancelToken::new(), crate::ProgressSink::new())
    }

    // The session may restore presentation before the writer is free. Keep
    // U25's selection authoritative and refuse if that selection changed.
    pub(crate) fn answer_undo_metadata(
        &self,
        scope: UndoScope,
    ) -> Result<Option<(Entry, crate::sched::answer::AnswerMetadata)>, EngineError> {
        let stack = self.undo_manager()?.lock();
        let UndoStatus::Available(info) = stack.peek(Side::Undo, scope) else {
            return Ok(None);
        };
        let entry = stack
            .resolved_target(Side::Undo, scope, info.id)
            .map_err(|reason| EngineError::UndoUnavailable { reason })?;
        Ok(entry
            .state
            .as_ref()
            .and_then(|s| s.downcast_ref::<crate::sched::answer::AnswerMetadata>())
            .cloned()
            .map(|meta| (entry.clone(), meta)))
    }

    pub(crate) fn submit_answer_undo(
        &self,
        scope: UndoScope,
        expected: EntryId,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Pending<OpResult<StepInfo>> {
        let action = UndoAction::Undo(scope);
        let mut spec = OpSpec::new(action.def(false), OpSource::UserUndoable)
            .stamp(StampMode::CallerSupplied)
            .cancel(cancel)
            .progress(progress);
        spec.undo.action = Some(action);
        self.submit(spec, move |ctx| {
            if ctx.restore.is_none_or(|r| r.target != Some(Ok(expected))) {
                return Err(EngineError::UndoUnavailable {
                    reason: DisabledReason::ChangedExternally,
                });
            }
            ctx.restore_top()
        })
    }

    pub fn undo(&self, scope: UndoScope) -> Result<OpResult<StepInfo>, EngineError> {
        self.submit_undo(scope).wait()
    }

    /// Redoes the top redo step of `scope` as an op (M5).
    pub fn submit_redo(&self, scope: UndoScope) -> Pending<OpResult<StepInfo>> {
        self.submit_redo_controlled(scope, crate::CancelToken::new(), crate::ProgressSink::new())
    }

    pub fn redo(&self, scope: UndoScope) -> Result<OpResult<StepInfo>, EngineError> {
        self.submit_redo(scope).wait()
    }

    pub fn submit_undo_controlled(
        &self,
        scope: UndoScope,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Pending<OpResult<StepInfo>> {
        self.submit_restore(UndoAction::Undo(scope), cancel, progress)
    }
    pub fn submit_redo_controlled(
        &self,
        scope: UndoScope,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Pending<OpResult<StepInfo>> {
        self.submit_restore(UndoAction::Redo(scope), cancel, progress)
    }
    pub fn undo_controlled(
        &self,
        scope: UndoScope,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<OpResult<StepInfo>, EngineError> {
        self.submit_undo_controlled(scope, cancel, progress).wait()
    }
    pub fn redo_controlled(
        &self,
        scope: UndoScope,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<OpResult<StepInfo>, EngineError> {
        self.submit_redo_controlled(scope, cancel, progress).wait()
    }
    fn submit_restore(
        &self,
        action: UndoAction,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Pending<OpResult<StepInfo>> {
        // Which step it restores, and so whether it runs as the long-write
        // variant with that step's declared write, is settled on the writer
        // as it takes the op (M4.1): a step recorded meanwhile, such as a
        // bulk op's that was running at submit, is the one it restores.
        let mut spec = OpSpec::new(action.def(false), OpSource::UserUndoable)
            .stamp(StampMode::CallerSupplied)
            .cancel(cancel)
            .progress(progress);
        spec.undo.action = Some(action);
        self.submit(spec, |ctx| ctx.restore_top())
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
        Ok(holdback::run_value(&run))
    }

    /// The CloudKit held run, including sessionless edits above the newest
    /// answer of this session (M8.1). Keeps journals without reading images.
    pub fn cloud_held_run(&self, session: Option<SessionId>) -> Result<HeldBackRun, EngineError> {
        let run = self.undo_manager()?.lock().cloud_run(session);
        Ok(holdback::run_value(&run))
    }

    /// M9.7: acknowledge named CloudKit Pending steps. Any thread may call
    /// this after tracking commits its acknowledgement; unknown ids are ignored.
    pub fn mark_cloud_uploaded(&self, ids: &[EntryId]) -> Result<(), EngineError> {
        self.undo_manager()?.lock().mark_cloud_uploaded(ids);
        Ok(())
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
