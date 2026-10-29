//! The history and redo stacks of one open collection
//! (`specs/engine/undo.md` M1 to M3, M6, M7, M9): which step is on top for a
//! scope, what each committed op does to them, and the sync flags.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use meguri_storage::{RowKey, Table};

use crate::changes::stream::Seq;
use crate::undo::entry::{Entry, EntryId, SessionId, StepInfo, StepKind, UploadState};
use crate::undo::holdback::{self, CloudHold, HeldBackRun};
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
    pub(super) safety_revision: u64,
    pub(super) safety_pending_commit: bool,
    pub(super) safety_backup: Option<(i64, crate::ports::BackupReceipt)>,
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
            safety_revision: 0,
            safety_pending_commit: false,
            safety_backup: None,
            undo: Vec::new(),
            redo: Vec::new(),
            floor: DisabledReason::None,
            sync: None,
        }
    }
}

impl UndoStack {
    #[cfg(feature = "test-hooks")]
    pub(super) fn depth(&self) -> usize {
        self.undo.len()
    }

    pub(super) fn invalidate_safety(&mut self) {
        self.safety_revision = self.safety_revision.wrapping_add(1);
        self.safety_backup = None;
    }

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

    /// M4.1: step `id`, which the writer resolved for an undo or redo op,
    /// while it is still the step `scope` acts on; otherwise why not.
    /// Only the writer adds steps, so between the resolve and the op another
    /// thread can only drop the step (a clear) or change its upload state.
    pub(crate) fn resolved_target(&self, side: Side, scope: UndoScope, id: EntryId) -> Result<&Entry, DisabledReason> {
        let entry = self.target(side, scope)?;
        if entry.info.id == id {
            Ok(entry)
        } else {
            Err(DisabledReason::None)
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
        self.clear_redo();
        info
    }

    /// A2: an op that records nothing still empties redo (A11.1).
    pub(crate) fn clear_redo(&mut self) {
        if !self.redo.is_empty() {
            self.invalidate_safety();
        }
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
            self.invalidate_safety();
            self.undo.remove(0);
            self.floor = DisabledReason::None;
        }
        info
    }

    /// M2, M6: drops every step and redo step.
    pub(crate) fn clear_all(&mut self, reason: DisabledReason) {
        self.invalidate_safety();
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
        self.invalidate_safety();
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
        self.invalidate_safety();
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
        self.invalidate_safety();
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

    /// M8.1: the CloudKit run also includes sessionless edits above the
    /// newest answer of this session. The manual run above stays unchanged.
    fn cloud_run_indexes(&self, session: Option<SessionId>) -> Vec<usize> {
        let Some(session) = session else {
            return Vec::new();
        };
        let mut run = Vec::new();
        for (at, entry) in self.undo.iter().enumerate().rev() {
            if entry.upload != UploadState::Local || entry.info.session.is_some_and(|s| s != session) {
                break;
            }
            run.push(at);
            if entry.info.kind == StepKind::Answer && entry.info.session == Some(session) {
                return run;
            }
        }
        Vec::new()
    }

    pub(crate) fn cloud_run(&self, session: Option<SessionId>) -> Vec<Entry> {
        self.cloud_run_indexes(session)
            .into_iter()
            .map(|at| self.undo[at].clone())
            .collect()
    }

    /// M9.6: named Local steps only; a later barrier never acknowledges an
    /// earlier flag. Retain keys with the entry, not the manual sync capture.
    pub(crate) fn begin_cloud(
        &mut self,
        session: Option<SessionId>,
        snapshot: Seq,
        keys: &[(Table, RowKey)],
    ) -> CloudHold {
        let run = self.cloud_run(session);
        let held: HashSet<EntryId> = run.iter().map(|entry| entry.info.id).collect();
        let mut flagged = BTreeMap::new();
        for entry in &mut self.undo {
            if entry.info.id.seq() > snapshot || entry.upload != UploadState::Local || held.contains(&entry.info.id) {
                continue;
            }
            let overlap: BTreeSet<_> = keys
                .iter()
                .filter(|key| entry.intersects(key, false))
                .cloned()
                .collect();
            if !overlap.is_empty() {
                entry.upload = UploadState::Pending;
                entry.cloud_flagged = Some(overlap.clone());
                flagged.insert(entry.info.id, overlap);
            }
        }
        let before = self.redo.len();
        self.redo
            .retain(|entry| entry.info.id.seq() > snapshot || !keys.iter().any(|key| entry.intersects(key, false)));
        if !flagged.is_empty() || before != self.redo.len() {
            self.invalidate_safety();
        }
        CloudHold {
            snapshot,
            run: holdback::run_value(&run),
            flagged,
        }
    }

    /// M9.7: the retained named overlap, without journal image reads.
    pub(crate) fn cloud_pending_keys(&self) -> BTreeMap<EntryId, BTreeSet<(Table, RowKey)>> {
        self.undo
            .iter()
            .filter(|entry| entry.upload == UploadState::Pending)
            .filter_map(|entry| entry.cloud_flagged.clone().map(|keys| (entry.info.id, keys)))
            .collect()
    }

    /// M9.7: only CloudKit-owned pending flags can be acknowledged here.
    /// Unknown ids and Local/Uploaded steps have no effect.
    pub(crate) fn mark_cloud_uploaded(&mut self, ids: &[EntryId]) {
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending && entry.cloud_flagged.is_some() && ids.contains(&entry.info.id) {
                entry.upload = UploadState::Uploaded;
            }
        }
    }

    /// M9.8: cut the targeted step and its ancestors on each stack. A hit
    /// anywhere in the pre-drop Cloud run detaches its journals and extends
    /// the undo cut to the top, including newer sessionless edits.
    pub(crate) fn cloud_drop(
        &mut self,
        session: Option<SessionId>,
        targets: &[(Table, RowKey)],
    ) -> Option<HeldBackRun> {
        if targets.is_empty() {
            return None;
        }
        let reaches = |entry: &Entry| targets.iter().any(|key| entry.intersects(key, false));
        let run = self.cloud_run_indexes(session);
        let detach = run.iter().any(|at| reaches(&self.undo[*at]));
        let detached = detach.then(|| {
            let entries: Vec<_> = run.iter().map(|at| self.undo[*at].clone()).collect();
            holdback::run_value(&entries)
        });
        let cut = if detach {
            self.undo.len().checked_sub(1)
        } else {
            self.undo.iter().rposition(reaches)
        };
        let redo_cut = self.redo.iter().rposition(reaches);
        if cut.is_some() || redo_cut.is_some() {
            self.invalidate_safety();
        }
        if let Some(at) = cut {
            self.undo.drain(..=at);
            self.floor = DisabledReason::ClearedBySync;
        }
        if let Some(at) = redo_cut {
            self.redo.drain(..=at);
        }
        detached
    }

    /// M9.1: captures the run and flags every other step up to `snapshot`
    /// upload-pending. Steps a sync left pending (its `finish` may have been
    /// sent) become uploaded first.
    pub(crate) fn begin_sync(&mut self, session: Option<SessionId>, snapshot: Seq) -> Vec<Entry> {
        self.invalidate_safety();
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending && entry.cloud_flagged.is_none() {
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
        self.invalidate_safety();
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending && entry.cloud_flagged.is_none() {
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
        let mut changed = false;
        for entry in &mut self.undo {
            if entry.upload == UploadState::Pending && entry.cloud_flagged.is_none() {
                entry.upload = UploadState::Local;
                changed = true;
            }
        }
        if changed {
            self.invalidate_safety();
        }
        self.sync = None;
    }

    /// M9.4: a `SyncIncoming` op applied (or staged) changes to `targets`.
    pub(crate) fn merge(&mut self, snapshot: Option<Seq>, targets: &[(Table, RowKey)]) {
        if targets.is_empty() {
            return;
        }
        self.invalidate_safety();
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
