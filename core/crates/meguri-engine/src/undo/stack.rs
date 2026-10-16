//! The history and redo stacks of one open collection
//! (`specs/engine/undo.md` M1 to M3, M6, M7, M9): which step is on top for a
//! scope, what each committed op does to them, and the sync flags.

use std::collections::HashSet;

use meguri_storage::{RowKey, Table};

use crate::changes::stream::Seq;
use crate::undo::entry::{Entry, EntryId, SessionId, StepInfo, StepKind, UploadState};
use crate::undo::reasons::DisabledReason;
use crate::undo::{UndoScope, UndoStatus};

/// The most steps kept (R719).
pub(crate) const CAPACITY: usize = 30;

/// Which stack an undo-like op works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Undo,
    Redo,
}

/// What a running normal sync captured at its barrier (M9.1).
#[derive(Clone, Debug)]
struct SyncState {
    snapshot: Seq,
    held: HashSet<EntryId>,
}

#[derive(Debug)]
pub(crate) struct UndoStack {
    /// Oldest first; the top is the last.
    undo: Vec<Entry>,
    /// The top (next to redo) is the last.
    redo: Vec<Entry>,
    /// What peek reports once the history is empty (M3).
    floor: DisabledReason,
    sync: Option<SyncState>,
}

impl Default for UndoStack {
    fn default() -> UndoStack {
        UndoStack {
            undo: Vec::new(),
            redo: Vec::new(),
            floor: DisabledReason::None,
            sync: None,
        }
    }
}

impl UndoStack {
    fn stack(&self, side: Side) -> &Vec<Entry> {
        match side {
            Side::Undo => &self.undo,
            Side::Redo => &self.redo,
        }
    }

    /// M3: the step `scope` would act on, or why there is none.
    pub(crate) fn target(&self, side: Side, scope: UndoScope) -> Result<&Entry, DisabledReason> {
        let Some(top) = self.stack(side).last() else {
            return Err(match side {
                Side::Undo => self.floor,
                Side::Redo => DisabledReason::None,
            });
        };
        if let UndoScope::Session(session) = scope
            && top.info.session != Some(session)
        {
            return Err(DisabledReason::None);
        }
        match top.upload {
            UploadState::Local => Ok(top),
            UploadState::Pending => Err(DisabledReason::UploadPending),
            UploadState::Uploaded => Err(DisabledReason::AlreadyUploaded),
        }
    }

    pub(crate) fn peek(&self, side: Side, scope: UndoScope) -> UndoStatus {
        match self.target(side, scope) {
            Ok(entry) => UndoStatus::Available(entry.info.clone()),
            Err(reason) => UndoStatus::Unavailable(reason),
        }
    }

    /// M1, A2: a new step on top, or merged into the top step (A3); redo
    /// is cleared. Returns the step that now holds the op.
    pub(crate) fn record(&mut self, entry: Entry) -> StepInfo {
        let info = self.push(entry);
        self.redo.clear();
        info
    }

    /// A2: an op that records nothing still empties redo (A11.1).
    pub(crate) fn clear_redo(&mut self) {
        self.redo.clear();
    }

    /// Puts a step on top of the history, unless the top step takes it in
    /// (A3); the oldest beyond 30 goes. Returns the step that holds it.
    fn push(&mut self, entry: Entry) -> StepInfo {
        if let Some(top) = self.undo.last_mut()
            && top.absorb(&entry)
        {
            return top.info.clone();
        }
        let info = entry.info.clone();
        self.undo.push(entry);
        if self.undo.len() > CAPACITY {
            self.undo.remove(0);
            self.floor = DisabledReason::None;
        }
        info
    }

    /// M2, M6: drops every step and redo step.
    pub(crate) fn clear_all(&mut self, reason: DisabledReason) {
        self.undo.clear();
        self.redo.clear();
        self.floor = reason;
    }

    /// M6: a replacement or a close also forgets a running sync's capture.
    pub(crate) fn reset(&mut self, reason: DisabledReason) {
        self.clear_all(reason);
        self.sync = None;
    }

    /// M4.5, M5: the step `id` was undone (or redone) by an op whose journal
    /// made `inverse`, which goes on the other stack; on the history it may
    /// merge into the top step as a new step would (A3). Nothing when a
    /// clear dropped the step meanwhile.
    pub(crate) fn restored(&mut self, side: Side, id: EntryId, inverse: impl FnOnce(&Entry) -> Entry) {
        let stack = match side {
            Side::Undo => &mut self.undo,
            Side::Redo => &mut self.redo,
        };
        let Some(at) = stack.iter().rposition(|entry| entry.info.id == id) else {
            return;
        };
        let entry = stack.remove(at);
        let made = inverse(&entry);
        match side {
            Side::Undo => self.redo.push(made),
            Side::Redo => {
                self.push(made);
            }
        }
    }

    /// M4.2, M5: the rows of step `id` changed. For undo the step and every
    /// older one go, with reason `ChangedExternally`; for redo, that redo step
    /// and every one below it.
    pub(crate) fn drop_changed(&mut self, side: Side, id: EntryId) {
        match side {
            Side::Undo => {
                if let Some(at) = self.undo.iter().position(|entry| entry.info.id == id) {
                    self.undo.drain(..=at);
                    self.floor = DisabledReason::ChangedExternally;
                }
            }
            Side::Redo => {
                if let Some(at) = self.redo.iter().position(|entry| entry.info.id == id) {
                    self.redo.drain(..=at);
                }
            }
        }
    }

    /// M7: a local-server write that changed `targets` drops the newest
    /// step it intersects and every older one, so no step that would restore
    /// what it changed stays; the same in redo.
    pub(crate) fn truncate_intersecting(&mut self, targets: &[(Table, RowKey)]) {
        if targets.is_empty() {
            return;
        }
        let reaches = |entry: &Entry| targets.iter().any(|target| entry.intersects(target, true));
        if let Some(at) = self.undo.iter().rposition(reaches) {
            self.undo.drain(..=at);
            self.floor = DisabledReason::ChangedExternally;
        }
        if let Some(at) = self.redo.iter().rposition(reaches) {
            self.redo.drain(..=at);
        }
    }

    /// M8: the indexes of the held-back run of `session`, newest first.
    pub(crate) fn run_indexes(&self, session: Option<SessionId>) -> Vec<usize> {
        let Some(session) = session else {
            return Vec::new();
        };
        let mut run = Vec::new();
        for (at, entry) in self.undo.iter().enumerate().rev() {
            if entry.info.session != Some(session) || entry.upload != UploadState::Local {
                break;
            }
            run.push(at);
            if entry.info.kind == StepKind::Answer {
                return run;
            }
        }
        Vec::new()
    }

    /// The steps of the held-back run, newest first.
    pub(crate) fn run(&self, session: Option<SessionId>) -> Vec<Entry> {
        self.run_indexes(session)
            .into_iter()
            .map(|at| self.undo[at].clone())
            .collect()
    }

    /// M9.1: captures the run and flags every other step up to `snapshot`
    /// upload-pending. Steps a sync left pending (its `finish` may have been
    /// sent) become uploaded first.
    pub(crate) fn begin_sync(&mut self, session: Option<SessionId>, snapshot: Seq) -> Vec<Entry> {
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending {
                entry.upload = UploadState::Uploaded;
            }
        }
        let run = self.run_indexes(session);
        let held: HashSet<EntryId> = run.iter().map(|at| self.undo[*at].info.id).collect();
        for entry in &mut self.undo {
            if entry.info.id.seq() <= snapshot && entry.upload == UploadState::Local && !held.contains(&entry.info.id) {
                entry.upload = UploadState::Pending;
            }
        }
        self.sync = Some(SyncState { snapshot, held });
        run.into_iter().map(|at| self.undo[at].clone()).collect()
    }

    /// M9.2: the `finish` request was sent.
    pub(crate) fn finish_sent(&mut self) {
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending {
                entry.upload = UploadState::Uploaded;
            }
        }
        if let Some(sync) = &self.sync {
            let snapshot = sync.snapshot;
            self.redo.retain(|entry| entry.info.id.seq() > snapshot);
        }
    }

    /// M9.3: the sync failed before `finish` was sent.
    pub(crate) fn failed_before_finish(&mut self) {
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending {
                entry.upload = UploadState::Local;
            }
        }
        self.sync = None;
    }

    /// M9.4: a `SyncIncoming` op applied (or staged) changes to `targets`.
    pub(crate) fn merge(&mut self, snapshot: Option<Seq>, targets: &[(Table, RowKey)]) {
        if targets.is_empty() {
            return;
        }
        let (snapshot, held) = match (&self.sync, snapshot) {
            (Some(sync), None) => (sync.snapshot, sync.held.clone()),
            (Some(sync), Some(given)) if sync.snapshot == given => (given, sync.held.clone()),
            (_, Some(given)) => (given, HashSet::new()),
            (None, None) => (Seq(u64::MAX), HashSet::new()),
        };
        let reaches = |entry: &Entry| targets.iter().any(|target| entry.intersects(target, false));
        let run_hit = self
            .undo
            .iter()
            .any(|entry| held.contains(&entry.info.id) && reaches(entry));
        let keep = |entry: &Entry| {
            if entry.info.id.seq() <= snapshot {
                held.contains(&entry.info.id) && !run_hit
            } else {
                !reaches(entry)
            }
        };
        let bottom_dropped = self.undo.first().is_some_and(|entry| !keep(entry));
        self.undo.retain(keep);
        self.redo
            .retain(|entry| entry.info.id.seq() > snapshot && !reaches(entry));
        if bottom_dropped {
            self.floor = DisabledReason::ClearedBySync;
        }
    }
}
