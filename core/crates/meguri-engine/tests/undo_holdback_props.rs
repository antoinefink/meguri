//! The held-back run and sync (`specs/engine/undo.md` M8, M9; KTD6, KTD7):
//! the run a normal sync keeps local, upload-pending and uploaded steps, the
//! merge rule, and a state-machine property against a model history and a
//! model server. The real answer op (U44) and sessions (U43) build on this
//! unit, so the steps here come from the test ops in `common::steps`.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::Harness;
use common::steps::{self, ANSWER_CARD, BURY, UPDATE_NOTE};
use meguri_engine::{
    Collection, DisabledReason, EngineError, OpResult, OpSpec, SessionId, StepKind, SyncHold, SyncMerge, UndoScope,
    UndoStatus,
};
use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, RowKey, StampMode, Table};

const S1: SessionId = SessionId(1);
const S2: SessionId = SessionId(2);

fn note_with_card(c: &Collection, front: &str) -> (NoteId, CardId) {
    steps::setup_note(c, front)
}

fn peek(c: &Collection) -> UndoStatus {
    c.peek_undo(UndoScope::Global).unwrap()
}

fn refused(result: Result<impl std::fmt::Debug, EngineError>) -> DisabledReason {
    match result {
        Err(EngineError::UndoUnavailable { reason }) => reason,
        other => panic!("expected an undo refusal, got {other:?}"),
    }
}

fn local_revlog(c: &Collection) -> BTreeSet<i64> {
    let reader = c.reader(ReaderRole::Query).unwrap();
    reader
        .execute_raw("SELECT id FROM revlog", &[])
        .unwrap()
        .rows
        .iter()
        .map(|row| match row[0] {
            SqlValue::Integer(id) => id,
            _ => unreachable!(),
        })
        .collect()
}

/// A model sync server: the review-log rows it received.
#[derive(Default, Debug)]
struct Server {
    revlog: BTreeSet<i64>,
}

/// A normal sync's barrier (M9.1), as U107 takes it: an op on the writer
/// that writes nothing, captures the run and reads the outgoing review-log
/// rows: the pending ones, without those the run created.
fn sync_begin(c: &Collection, session: Option<SessionId>) -> (SyncHold, BTreeSet<i64>) {
    let (value, _) = sync_begin_op(c, session);
    value
}

/// The barrier op, with its own seq (it commits, writing nothing).
fn sync_begin_op(c: &Collection, session: Option<SessionId>) -> ((SyncHold, BTreeSet<i64>), u64) {
    let result = c
        .run(OpSpec::named("sync_snapshot", OpSource::SyncIncoming), move |ctx| {
            let hold = ctx.begin_sync_hold(session)?;
            let pending: BTreeSet<i64> = ctx
                .scope()
                .revlog_after(None, usize::MAX)?
                .into_iter()
                .filter(|row| row.usn.0 == -1)
                .map(|row| row.id.0)
                .collect();
            let held: BTreeSet<i64> = hold
                .run
                .rows
                .iter()
                .filter_map(|row| match (row.table, &row.key, &row.before) {
                    (Table::Revlog, RowKey::Id(id), None) => Some(*id),
                    _ => None,
                })
                .collect();
            Ok((hold, pending.difference(&held).copied().collect()))
        })
        .unwrap();
    (result.value, result.seq.0)
}

/// What the other side changed, staged by the sync.
#[derive(Clone, Debug)]
enum Incoming {
    CardDue(CardId, i32),
    NoteField(NoteId, String),
    /// A deck rename that loses the merge to the newer local row: staged,
    /// not applied.
    LostDeckRename(DeckId),
}

impl Incoming {
    fn target(&self) -> (Table, RowKey) {
        match self {
            Incoming::CardDue(id, _) => (Table::Cards, RowKey::Id(id.0)),
            Incoming::NoteField(id, _) => (Table::Notes, RowKey::Id(id.0)),
            Incoming::LostDeckRename(id) => (Table::Decks, RowKey::Id(id.0)),
        }
    }
}

/// The merge commit (M9.4): incoming rows written with the server's usn.
fn sync_merge(c: &Collection, snapshot: meguri_engine::Seq, incoming: Vec<Incoming>) -> OpResult<()> {
    let staged = incoming.iter().map(Incoming::target).collect();
    c.run(
        OpSpec::named("sync_merge", OpSource::SyncIncoming).stamp(StampMode::CallerSupplied),
        move |ctx| {
            ctx.report_sync_merge(SyncMerge { snapshot, staged })?;
            for change in incoming {
                match change {
                    Incoming::CardDue(id, due) => {
                        if let Some(mut card) = ctx.scope().card(id)? {
                            card.due = due;
                            card.usn = meguri_model::usn::Usn(100);
                            ctx.scope().put(&card)?;
                        }
                    }
                    Incoming::NoteField(id, value) => {
                        if ctx.scope().note(id)?.is_some() {
                            steps::set_first_field(ctx, id, value)?;
                        }
                    }
                    Incoming::LostDeckRename(_) => {}
                }
            }
            Ok(())
        },
    )
    .unwrap()
}

/// A whole sync: barrier, upload, `finish`, and the merge when something
/// came in.
fn sync(c: &Collection, server: &mut Server, session: Option<SessionId>, incoming: Vec<Incoming>) -> SyncHold {
    let (hold, outgoing) = sync_begin(c, session);
    server.revlog.extend(outgoing);
    c.sync_finish_sent().unwrap();
    if !incoming.is_empty() {
        sync_merge(c, hold.snapshot, incoming);
    }
    hold
}

#[test]
fn the_run_reaches_down_to_the_newest_answer_and_a_sync_uploads_the_rest() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "a");
    let (_, other) = note_with_card(c, "b");
    let fixture_rows = local_revlog(c);
    let answer1 = steps::answer(c, Some(S1), card).unwrap();
    let answer2 = steps::answer(c, Some(S1), other).unwrap();
    let bury = steps::bury(c, Some(S1), other).unwrap();

    let run = c.held_back_run(Some(S1)).unwrap();
    let ids: Vec<u64> = run.steps.iter().map(|s| s.id.0).collect();
    assert_eq!(ids, [bury.seq.0, answer2.seq.0], "[bury, answer2]");
    assert_eq!(
        run.steps.iter().map(|s| s.name).collect::<Vec<_>>(),
        [BURY, ANSWER_CARD]
    );

    let mut server = Server::default();
    let hold = sync(c, &mut server, Some(S1), Vec::new());
    assert_eq!(hold.run, run);
    let sent: BTreeSet<i64> = server.revlog.difference(&fixture_rows).copied().collect();
    assert_eq!(sent, BTreeSet::from([answer1.value.0]), "answer2's row held back");

    assert_eq!(c.undo(UndoScope::Session(S1)).unwrap().value.name, BURY);
    assert_eq!(c.undo(UndoScope::Session(S1)).unwrap().value.name, ANSWER_CARD);
    assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::AlreadyUploaded));
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::AlreadyUploaded);
    assert!(local_revlog(c).contains(&answer1.value.0));
}

#[test]
fn after_undoing_the_held_answer_the_next_sync_holds_nothing_back() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "a");
    let fixture_rows = local_revlog(c);
    let answer1 = steps::answer(c, Some(S1), card).unwrap();
    let answer2 = steps::answer(c, Some(S1), card).unwrap();
    let mut server = Server::default();
    sync(c, &mut server, Some(S1), Vec::new());
    let sent: BTreeSet<i64> = server.revlog.difference(&fixture_rows).copied().collect();
    assert_eq!(sent, BTreeSet::from([answer1.value.0]));
    c.undo(UndoScope::Session(S1)).unwrap();
    assert!(!local_revlog(c).contains(&answer2.value.0));

    let hold = sync(c, &mut server, Some(S1), Vec::new());
    assert!(hold.run.is_empty(), "the top step is uploaded");
    assert!(hold.run.rows.is_empty(), "nothing substituted");
    // sanityCheck2's review-log count (U107 leg): both sides hold the same
    // rows, answer1's among them, and nothing was substituted.
    assert_eq!(server.revlog, local_revlog(c));
    assert!(server.revlog.contains(&answer1.value.0));
}

#[test]
fn an_incoming_change_to_the_held_answers_card_clears_the_run() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "a");
    let (_, other) = note_with_card(c, "b");
    steps::answer(c, Some(S1), other).unwrap();
    steps::answer(c, Some(S1), card).unwrap();
    let mut server = Server::default();
    sync(c, &mut server, Some(S1), vec![Incoming::CardDue(card, 7)]);
    assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::ClearedBySync));
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::ClearedBySync);
}

#[test]
fn a_staged_deck_rename_that_loses_the_merge_still_clears_the_run() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "a");
    // The answer's step touches its deck's row (today counters).
    c.run(
        OpSpec::named("answer_card", OpSource::UserUndoable)
            .undo_step(ANSWER_CARD, StepKind::Answer)
            .session(S1),
        move |ctx| {
            let mut stored = ctx.scope().card(card)?.unwrap();
            stored.reps += 1;
            ctx.scope().put(&stored)?;
            let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
            deck.name = "Default today".to_owned();
            ctx.scope().put(&deck)?;
            Ok(())
        },
    )
    .unwrap();
    let mut server = Server::default();
    sync(c, &mut server, Some(S1), vec![Incoming::LostDeckRename(DeckId(1))]);
    assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::ClearedBySync));
}

#[test]
fn an_answer_made_after_the_snapshot_survives_unrelated_incoming_changes_but_not_one_to_its_card() {
    for touch_its_card in [false, true] {
        let h = Harness::open();
        let c = &h.collection;
        let (_, held_card) = note_with_card(c, "held");
        let (_, late_card) = note_with_card(c, "late");
        let (unrelated, _) = note_with_card(c, "unrelated");
        steps::answer(c, Some(S1), held_card).unwrap();
        let (hold, _) = sync_begin(c, Some(S1));
        // Answered while the sync runs: not in its snapshot.
        let late = steps::answer(c, Some(S1), late_card).unwrap();
        assert!(late.seq > hold.snapshot);
        c.sync_finish_sent().unwrap();
        let mut incoming = vec![Incoming::NoteField(unrelated, "remote".to_owned())];
        if touch_its_card {
            incoming.push(Incoming::CardDue(late_card, 3));
        }
        sync_merge(c, hold.snapshot, incoming);
        if !touch_its_card {
            assert_eq!(c.undo(UndoScope::Global).unwrap().value.id.seq(), late.seq);
        }
        // The late answer went alone when its card was targeted; the held
        // run below stays either way.
        assert_eq!(c.undo(UndoScope::Global).unwrap().value.id, hold.run.steps[0].id);
        assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::None));
    }
}

#[test]
fn a_pending_step_is_refused_during_the_sync_and_settled_by_its_end() {
    for lost_response in [false, true] {
        let h = Harness::open();
        let c = &h.collection;
        let (_, card) = note_with_card(c, "a");
        steps::answer(c, Some(S1), card).unwrap();
        // Another session's step on top: S1's run is empty.
        steps::bury(c, Some(S2), card).unwrap();
        let (hold, _) = sync_begin(c, Some(S1));
        assert!(hold.run.is_empty());
        assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::UploadPending));
        assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::UploadPending);
        if lost_response {
            // `finish` went out and its response was lost.
            c.sync_finish_sent().unwrap();
            assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::AlreadyUploaded));
        } else {
            c.sync_failed_before_finish().unwrap();
            assert_eq!(c.undo(UndoScope::Global).unwrap().value.name, BURY);
            assert_eq!(c.undo(UndoScope::Global).unwrap().value.name, ANSWER_CARD);
        }
    }
}

#[test]
fn the_run_is_empty_without_a_session_or_an_answer_or_when_the_top_is_uploaded() {
    let h = Harness::open();
    let c = &h.collection;
    let (nid, card) = note_with_card(c, "a");
    assert!(c.held_back_run(Some(S1)).unwrap().is_empty(), "empty history");
    steps::edit(c, Some(S1), nid, "x").unwrap();
    assert!(c.held_back_run(Some(S1)).unwrap().is_empty(), "no answer");
    steps::answer(c, Some(S1), card).unwrap();
    steps::bury(c, Some(S1), card).unwrap();
    assert!(c.held_back_run(None).unwrap().is_empty(), "no session");
    assert!(c.held_back_run(Some(S2)).unwrap().is_empty(), "another session's top");
    assert_eq!(c.held_back_run(Some(S1)).unwrap().steps.len(), 2);
    steps::edit(c, Some(S2), nid, "y").unwrap();
    assert!(c.held_back_run(Some(S1)).unwrap().is_empty(), "S2's step on top");
}

#[test]
fn the_run_gives_each_row_its_image_from_before_the_run() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "a");
    let first = steps::answer(c, Some(S1), card).unwrap();
    let second = steps::answer(c, Some(S1), card).unwrap();
    let run = c.held_back_run(Some(S1)).unwrap();
    assert_eq!(run.steps.len(), 1, "down to the newest answer only");
    assert!(run.touches(Table::Revlog, &RowKey::Id(second.value.0)));
    assert!(!run.touches(Table::Revlog, &RowKey::Id(first.value.0)));

    // A run of an answer and a bury of the same card: the card's image is
    // the one before both.
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "b");
    let original = c.reader(ReaderRole::Query).unwrap().card(card).unwrap().unwrap();
    let answered = steps::answer(c, Some(S1), card).unwrap();
    steps::bury(c, Some(S1), card).unwrap();
    let run = c.held_back_run(Some(S1)).unwrap();
    assert_eq!(run.steps.len(), 2);
    let card_row = run
        .rows
        .iter()
        .find(|row| row.table == Table::Cards && row.key == RowKey::Id(card.0))
        .unwrap();
    let before = <meguri_model::card::Card as meguri_storage::Record>::from_cells(card_row.before.as_deref().unwrap());
    assert_eq!(
        format!("{:?}", before.unwrap()),
        format!("{original:?}"),
        "the card before the answer"
    );
    let revlog_row = run.rows.iter().find(|row| row.table == Table::Revlog).unwrap();
    assert_eq!(revlog_row.key, RowKey::Id(answered.value.0));
    assert_eq!(revlog_row.before, None, "the run created it");
    assert!(run.rows.iter().all(|row| row.table != Table::Col));
    assert_eq!(run.rows.len(), 2);
}

#[test]
fn finish_drops_redo_steps_up_to_the_snapshot_and_keeps_later_ones() {
    let h = Harness::open();
    let c = &h.collection;
    let (nid, card) = note_with_card(c, "a");
    let (_, other) = note_with_card(c, "b");
    steps::answer(c, Some(S2), other).unwrap();
    steps::answer(c, Some(S1), card).unwrap();
    steps::edit(c, None, nid, "1").unwrap();
    // A redo step made before the snapshot.
    c.undo(UndoScope::Global).unwrap();
    let (hold, _) = sync_begin(c, Some(S1));
    assert_eq!(hold.run.steps.len(), 1);
    // The held answer, undone during the sync: its redo step is made after.
    c.undo(UndoScope::Session(S1)).unwrap();
    c.sync_finish_sent().unwrap();
    assert_eq!(c.peek_redo(UndoScope::Global).unwrap().name(), Some(ANSWER_CARD));
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(
        c.peek_redo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None),
        "the edit's redo step was made before the snapshot"
    );
    assert_eq!(c.undo(UndoScope::Global).unwrap().value.name, ANSWER_CARD);
    assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::AlreadyUploaded));
    assert_eq!(common::note_fields(c, nid), "a\u{1f}back");
}

#[test]
fn a_merge_report_is_refused_from_any_other_source() {
    let h = Harness::open();
    let result = h
        .collection
        .run(OpSpec::named("not_a_sync", OpSource::UserNonUndoable), |ctx| {
            ctx.report_sync_merge(SyncMerge {
                snapshot: meguri_engine::Seq(0),
                staged: Vec::new(),
            })
        });
    assert!(matches!(result, Err(EngineError::InvalidArgument { .. })));
}

#[test]
fn a_sync_incoming_op_without_a_report_clears_all_but_the_run_by_its_journal() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = note_with_card(c, "a");
    let (other_note, _) = note_with_card(c, "b");
    steps::answer(c, Some(S1), card).unwrap();
    steps::answer(c, Some(S1), card).unwrap();
    let mut server = Server::default();
    let (hold, outgoing) = sync_begin(c, Some(S1));
    server.revlog.extend(outgoing);
    c.sync_finish_sent().unwrap();
    // A merge that reports nothing: its journal's keys are its targets.
    c.run(
        OpSpec::named("sync_merge", OpSource::SyncIncoming).stamp(StampMode::CallerSupplied),
        move |ctx| steps::set_first_field(ctx, other_note, "remote".to_owned()),
    )
    .unwrap();
    assert_eq!(c.undo(UndoScope::Global).unwrap().value.id, hold.run.steps[0].id);
    assert_eq!(peek(c), UndoStatus::Unavailable(DisabledReason::ClearedBySync));
}

// ---------------------------------------------------------------------------
// The state-machine property (M8, M9): random answers, edits, undo, redo,
// syncs against a model server, API writes and rollovers, checked against a
// model history after every action. 64 cases run with every test run; the
// ignored `ten_thousand_random_histories` is the unit's verification run.

const CARDS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Upload {
    Local,
    Pending,
    Uploaded,
}

#[derive(Clone, Debug)]
struct MStep {
    id: u64,
    name: &'static str,
    kind: StepKind,
    session: Option<SessionId>,
    keys: BTreeSet<(Table, RowKey)>,
    notes: BTreeSet<i64>,
    upload: Upload,
    /// The epoch second its first op (or the redo that put it back) began.
    began: i64,
    /// The note, when the step changed that one note row and nothing else
    /// but `col` (A3).
    note_only: Option<i64>,
}

impl MStep {
    fn reaches(&self, target: &(Table, RowKey), expand_notes: bool) -> bool {
        self.keys.contains(target)
            || (expand_notes
                && target.0 == Table::Notes
                && matches!(target.1, RowKey::Id(n) if self.notes.contains(&n)))
    }
}

#[derive(Debug)]
struct Model {
    undo: Vec<MStep>,
    redo: Vec<MStep>,
    floor: DisabledReason,
    sync: Option<(u64, BTreeSet<u64>)>,
    /// How many steps the top step took in (A3).
    merges: usize,
}

impl Model {
    fn new() -> Model {
        Model {
            undo: Vec::new(),
            redo: Vec::new(),
            floor: DisabledReason::None,
            sync: None,
            merges: 0,
        }
    }

    /// A new step on top, unless the top step takes it in (A3).
    fn push(&mut self, step: MStep) {
        if let Some(top) = self.undo.last()
            && top.upload == Upload::Local
            && top.name == UPDATE_NOTE
            && step.name == UPDATE_NOTE
            && top.session == step.session
            && (step.began - top.began).max(0) < 60
            && top.note_only.is_some()
            && top.note_only == step.note_only
        {
            self.merges += 1;
            return;
        }
        self.undo.push(step);
        if self.undo.len() > 30 {
            self.undo.remove(0);
            self.floor = DisabledReason::None;
        }
    }

    fn record(&mut self, step: MStep) {
        self.push(step);
        self.redo.clear();
    }

    fn clear(&mut self, reason: DisabledReason) {
        self.undo.clear();
        self.redo.clear();
        self.floor = reason;
    }

    fn target(&self, redo: bool, scope: UndoScope) -> Result<&MStep, DisabledReason> {
        let stack = if redo { &self.redo } else { &self.undo };
        let Some(top) = stack.last() else {
            return Err(if redo { DisabledReason::None } else { self.floor });
        };
        if let UndoScope::Session(s) = scope
            && top.session != Some(s)
        {
            return Err(DisabledReason::None);
        }
        match top.upload {
            Upload::Local => Ok(top),
            Upload::Pending => Err(DisabledReason::UploadPending),
            Upload::Uploaded => Err(DisabledReason::AlreadyUploaded),
        }
    }

    fn status(&self, redo: bool, scope: UndoScope) -> Result<(u64, &'static str), DisabledReason> {
        self.target(redo, scope).map(|step| (step.id, step.name))
    }

    fn truncate(&mut self, target: &(Table, RowKey)) {
        if let Some(at) = self.undo.iter().rposition(|s| s.reaches(target, true)) {
            self.undo.drain(..=at);
            self.floor = DisabledReason::ChangedExternally;
        }
        if let Some(at) = self.redo.iter().rposition(|s| s.reaches(target, true)) {
            self.redo.drain(..=at);
        }
    }

    fn run(&self, session: Option<SessionId>) -> Vec<u64> {
        let Some(session) = session else { return Vec::new() };
        let mut run = Vec::new();
        for step in self.undo.iter().rev() {
            if step.session != Some(session) || step.upload != Upload::Local {
                break;
            }
            run.push(step.id);
            if step.kind == StepKind::Answer {
                return run;
            }
        }
        Vec::new()
    }

    fn begin_sync(&mut self, session: Option<SessionId>, snapshot: u64) -> Vec<u64> {
        for step in &mut self.undo {
            if step.upload == Upload::Pending {
                step.upload = Upload::Uploaded;
            }
        }
        let run = self.run(session);
        for step in &mut self.undo {
            if step.id <= snapshot && step.upload == Upload::Local && !run.contains(&step.id) {
                step.upload = Upload::Pending;
            }
        }
        self.sync = Some((snapshot, run.iter().copied().collect()));
        run
    }

    fn finish(&mut self) {
        for step in &mut self.undo {
            if step.upload == Upload::Pending {
                step.upload = Upload::Uploaded;
            }
        }
        if let Some((snapshot, _)) = &self.sync {
            let snapshot = *snapshot;
            self.redo.retain(|s| s.id > snapshot);
        }
    }

    fn fail(&mut self) {
        for step in &mut self.undo {
            if step.upload == Upload::Pending {
                step.upload = Upload::Local;
            }
        }
        self.sync = None;
    }

    fn merge(&mut self, targets: &[(Table, RowKey)]) {
        if targets.is_empty() {
            return;
        }
        let (snapshot, held) = self.sync.clone().expect("a sync began");
        let reaches = |s: &MStep| targets.iter().any(|t| s.reaches(t, false));
        let run_hit = self.undo.iter().any(|s| held.contains(&s.id) && reaches(s));
        let keep = |s: &MStep| {
            if s.id <= snapshot {
                held.contains(&s.id) && !run_hit
            } else {
                !reaches(s)
            }
        };
        let bottom = self.undo.first().is_some_and(|s| !keep(s));
        self.undo.retain(|s| keep(s));
        self.redo.retain(|s| s.id > snapshot && !reaches(s));
        if bottom {
            self.floor = DisabledReason::ClearedBySync;
        }
    }
}

/// Where a sync is.
#[derive(Debug)]
enum SyncPhase {
    Idle,
    /// Began: the snapshot and the outgoing review-log rows.
    Began(meguri_engine::Seq, BTreeSet<i64>),
    /// `finish` sent and answered; a merge may follow.
    Finished(meguri_engine::Seq),
}

struct Walk {
    rng: SeededRandom,
    h: Harness,
    cards: Vec<(NoteId, CardId)>,
    model: Model,
    server: Server,
    phase: SyncPhase,
    last_seq: u64,
    tokens: u64,
    log: Vec<String>,
}

impl Walk {
    fn pick(&self, n: usize) -> usize {
        (self.rng.next_u64() % n as u64) as usize
    }

    fn session(&self) -> Option<SessionId> {
        [None, Some(S1), Some(S1), Some(S2)][self.pick(4)]
    }

    fn scope(&self) -> UndoScope {
        [
            UndoScope::Global,
            UndoScope::Global,
            UndoScope::Session(S1),
            UndoScope::Session(S2),
        ][self.pick(4)]
    }

    fn c(&self) -> &Collection {
        &self.h.collection
    }

    fn seen<T>(&mut self, result: &OpResult<T>) {
        self.last_seq = self.last_seq.max(result.seq.0);
    }

    fn fail(&self, what: String) -> ! {
        panic!("{what}\nactions:\n{}\nmodel: {:#?}", self.log.join("\n"), self.model)
    }

    fn check(&self, ok: bool, what: impl FnOnce() -> String) {
        if !ok {
            self.fail(what());
        }
    }

    /// The clock's epoch second.
    fn now(&self) -> i64 {
        self.h.clock.now_ms().div_euclid(1000)
    }

    fn step(&mut self) {
        // Some actions follow within the 60 s of a note-edit merge (A3),
        // some after it.
        let wait = [0, 5_000, 30_000, 61_000][self.pick(4)];
        self.h.clock.step_ms(wait);
        let (nid, cid) = self.cards[self.pick(CARDS)];
        let roll = self.pick(100);
        match roll {
            0..=19 => {
                let session = self.session();
                self.log.push(format!("answer {cid:?} in {session:?}"));
                let result = steps::answer(self.c(), session, cid).unwrap();
                self.seen(&result);
                self.model.record(MStep {
                    id: result.seq.0,
                    name: ANSWER_CARD,
                    kind: StepKind::Answer,
                    session,
                    keys: BTreeSet::from([
                        (Table::Cards, RowKey::Id(cid.0)),
                        (Table::Revlog, RowKey::Id(result.value.0)),
                    ]),
                    notes: BTreeSet::from([nid.0]),
                    upload: Upload::Local,
                    began: self.now(),
                    note_only: None,
                });
            }
            20..=27 => {
                let session = self.session();
                self.log.push(format!("bury {cid:?} in {session:?}"));
                let result = steps::bury(self.c(), session, cid).unwrap();
                self.seen(&result);
                if result.changes.is_empty() {
                    // Records nothing, empties redo (A2).
                    self.model.redo.clear();
                } else {
                    self.model.record(MStep {
                        id: result.seq.0,
                        name: BURY,
                        kind: StepKind::Other,
                        session,
                        keys: BTreeSet::from([(Table::Cards, RowKey::Id(cid.0))]),
                        notes: BTreeSet::from([nid.0]),
                        upload: Upload::Local,
                        began: self.now(),
                        note_only: None,
                    });
                }
            }
            28..=35 => {
                let session = self.session();
                self.tokens += 1;
                self.log.push(format!("edit {nid:?} in {session:?}"));
                let result = steps::edit(self.c(), session, nid, &format!("t{}", self.tokens)).unwrap();
                self.seen(&result);
                self.model.record(MStep {
                    id: result.seq.0,
                    name: UPDATE_NOTE,
                    kind: StepKind::Other,
                    session,
                    keys: BTreeSet::from([(Table::Notes, RowKey::Id(nid.0))]),
                    notes: BTreeSet::from([nid.0]),
                    upload: Upload::Local,
                    began: self.now(),
                    note_only: Some(nid.0),
                });
            }
            36..=53 => self.undo_or_redo(false),
            54..=61 => self.undo_or_redo(true),
            62..=66 => {
                self.tokens += 1;
                self.log.push(format!("api edit {nid:?}"));
                let result =
                    common::set_first_field(self.c(), OpSource::LocalApi, nid, &format!("api{}", self.tokens)).unwrap();
                self.seen(&result);
                self.model.truncate(&(Table::Notes, RowKey::Id(nid.0)));
            }
            67..=68 => {
                self.log.push("rollover".to_owned());
                self.c().clear_undo(meguri_engine::ClearCause::Rollover).unwrap();
                self.model.clear(DisabledReason::ClearedByRollover);
            }
            69 => {
                self.log.push(format!("optimizer writes {cid:?}"));
                self.tokens += 1;
                let token = self.tokens;
                let result = self
                    .c()
                    .run(OpSpec::named("optimizer_commit", OpSource::Optimizer), move |ctx| {
                        let mut card = ctx.scope().card(cid)?.unwrap();
                        card.original_due = token as i32;
                        ctx.scope().put(&card)?;
                        Ok(())
                    })
                    .unwrap();
                self.seen(&result);
                self.model.clear(DisabledReason::ClearedByNonUndoableOp);
            }
            70..=72 => {
                self.log.push("select deck".to_owned());
                self.tokens += 1;
                let token = i128::from(self.tokens);
                let result = self
                    .c()
                    .run(
                        OpSpec::named("select_deck", OpSource::UserNonUndoable).skip_undo(),
                        move |ctx| {
                            ctx.scope().set_config("curDeck", &ConfigValue::Integer(token))?;
                            Ok(())
                        },
                    )
                    .unwrap();
                self.seen(&result);
                // Keeps the steps, empties redo (A11.1).
                self.model.redo.clear();
            }
            _ => self.sync_step(),
        }
    }

    fn undo_or_redo(&mut self, redo: bool) {
        let scope = self.scope();
        self.log
            .push(format!("{} {scope:?}", if redo { "redo" } else { "undo" }));
        let expected = self.model.status(redo, scope);
        let peeked = if redo {
            self.c().peek_redo(scope)
        } else {
            self.c().peek_undo(scope)
        }
        .unwrap();
        let peeked = match peeked {
            UndoStatus::Available(step) => Ok((step.id.0, step.name)),
            UndoStatus::Unavailable(reason) => Err(reason),
        };
        self.check(peeked == expected, || {
            format!("peek: engine {peeked:?}, model {expected:?}")
        });
        let before = steps::dump(self.c());
        let result = if redo {
            self.c().redo(scope)
        } else {
            self.c().undo(scope)
        };
        match (result, expected) {
            (Ok(done), Ok((id, _))) => {
                self.check(done.value.id.0 == id, || {
                    format!("restored {:?}, model {id}", done.value.id)
                });
                self.seen(&done);
                let popped = if redo {
                    self.model.redo.pop()
                } else {
                    self.model.undo.pop()
                };
                let mut step = popped.expect("the model's top");
                // Undo never changes a row outside the step's touched set.
                let after = steps::dump(self.c());
                for (table, key) in steps::changed_rows(&before, &after) {
                    let row = match (table, key.as_slice()) {
                        ("cards", [SqlValue::Integer(id)]) => Some((Table::Cards, RowKey::Id(*id))),
                        ("notes", [SqlValue::Integer(id)]) => Some((Table::Notes, RowKey::Id(*id))),
                        ("revlog", [SqlValue::Integer(id)]) => Some((Table::Revlog, RowKey::Id(*id))),
                        _ => None,
                    };
                    let inside = table == "col" || row.is_some_and(|row| step.keys.contains(&row));
                    self.check(inside, || format!("{table} {key:?} changed outside step {}", step.id));
                }
                // The step the undo or redo leaves is its own journal, made
                // now; a note row it did not change takes it out of A3.
                step.id = done.seq.0;
                step.began = self.now();
                step.note_only = step.note_only.filter(|note| done.changes.note(NoteId(*note)).is_some());
                if redo {
                    self.model.push(step);
                } else {
                    self.model.redo.push(step);
                }
            }
            (Err(EngineError::UndoUnavailable { reason }), Err(expected)) => {
                self.check(reason == expected, || {
                    format!("refused with {reason:?}, model {expected:?}")
                });
                self.check(steps::dump(self.c()) == before, || "a refusal wrote".to_owned());
            }
            (got, want) => self.fail(format!("engine {got:?}, model {want:?}")),
        }
    }

    fn sync_step(&mut self) {
        match std::mem::replace(&mut self.phase, SyncPhase::Idle) {
            SyncPhase::Idle => {
                let session = [None, Some(S1), Some(S1)][self.pick(3)];
                self.log.push(format!("sync begins for {session:?}"));
                let ((hold, outgoing), barrier) = sync_begin_op(self.c(), session);
                self.check(hold.snapshot.0 == self.last_seq, || {
                    format!("snapshot {:?}, last op {}", hold.snapshot, self.last_seq)
                });
                let run = self.model.begin_sync(session, self.last_seq);
                self.last_seq = barrier;
                let got: Vec<u64> = hold.run.steps.iter().map(|s| s.id.0).collect();
                self.check(got == run, || format!("run: engine {got:?}, model {run:?}"));
                self.phase = SyncPhase::Began(hold.snapshot, outgoing);
            }
            SyncPhase::Began(snapshot, outgoing) => match self.pick(4) {
                0 => {
                    self.log.push("sync fails before finish".to_owned());
                    self.c().sync_failed_before_finish().unwrap();
                    self.model.fail();
                }
                1 => {
                    self.log.push("finish sent, response lost".to_owned());
                    self.server.revlog.extend(outgoing);
                    self.c().sync_finish_sent().unwrap();
                    self.model.finish();
                }
                _ => {
                    self.log.push("finish sent".to_owned());
                    self.server.revlog.extend(outgoing);
                    self.c().sync_finish_sent().unwrap();
                    self.model.finish();
                    self.phase = SyncPhase::Finished(snapshot);
                }
            },
            SyncPhase::Finished(snapshot) => {
                let incoming: Vec<Incoming> = (0..self.pick(3))
                    .map(|_| {
                        let (nid, cid) = self.cards[self.pick(CARDS)];
                        match self.pick(3) {
                            0 => Incoming::CardDue(cid, 1000 + self.pick(100) as i32),
                            1 => Incoming::NoteField(nid, "remote".to_owned()),
                            _ => Incoming::LostDeckRename(DeckId(1)),
                        }
                    })
                    .collect();
                self.log.push(format!("merge {incoming:?}"));
                let targets: Vec<(Table, RowKey)> = incoming.iter().map(Incoming::target).collect();
                let result = sync_merge(self.c(), snapshot, incoming);
                self.seen(&result);
                self.model.merge(&targets);
            }
        }
    }

    /// Checks that hold after every action.
    fn invariants(&self) {
        // No review-log row the server holds is ever deleted by an undo.
        let local = local_revlog(self.c());
        let missing: Vec<&i64> = self.server.revlog.difference(&local).collect();
        self.check(missing.is_empty(), || {
            format!("uploaded review-log rows undone: {missing:?}")
        });
        for (redo, scope) in [
            (false, UndoScope::Global),
            (false, UndoScope::Session(S1)),
            (true, UndoScope::Global),
        ] {
            let peeked = if redo {
                self.c().peek_redo(scope)
            } else {
                self.c().peek_undo(scope)
            }
            .unwrap();
            let peeked = match peeked {
                UndoStatus::Available(step) => Ok((step.id.0, step.name)),
                UndoStatus::Unavailable(reason) => Err(reason),
            };
            let expected = self.model.status(redo, scope);
            self.check(peeked == expected, || {
                format!("peek {scope:?} redo={redo}: engine {peeked:?}, model {expected:?}")
            });
        }
        // The held-back run is never flagged: it is what the model says.
        let run: Vec<u64> = self
            .c()
            .held_back_run(Some(S1))
            .unwrap()
            .steps
            .iter()
            .map(|s| s.id.0)
            .collect();
        let expected = self.model.run(Some(S1));
        self.check(run == expected, || format!("run: engine {run:?}, model {expected:?}"));
    }
}

/// Runs `cases` seeded walks of 40 actions each; returns how often each kind
/// of action ran, and how many steps merged into the top step (A3).
fn walk_cases(cases: std::ops::RangeInclusive<u64>) -> (BTreeMap<String, usize>, usize) {
    let mut actions = BTreeMap::<String, usize>::new();
    let mut merges = 0;
    for seed in cases {
        let h = Harness::open();
        let cards: Vec<(NoteId, CardId)> = (0..CARDS)
            .map(|i| note_with_card(&h.collection, &format!("c{i}")))
            .collect();
        let last_seq = h.collection.last_seq().0;
        let mut walk = Walk {
            rng: SeededRandom::new(seed),
            h,
            cards,
            model: Model::new(),
            server: Server::default(),
            phase: SyncPhase::Idle,
            last_seq,
            tokens: 0,
            log: vec![format!("seed {seed}")],
        };
        for _ in 0..40 {
            walk.step();
            walk.invariants();
        }
        for line in &walk.log[1..] {
            *actions
                .entry(line.split(' ').next().unwrap_or("").to_owned())
                .or_default() += 1;
        }
        merges += walk.model.merges;
    }
    (actions, merges)
}

#[test]
fn random_histories_never_undo_an_uploaded_step_or_touch_rows_outside_a_step() {
    let (actions, merges) = walk_cases(1..=64);
    assert_eq!(actions.values().sum::<usize>(), 64 * 40);
    assert!(merges > 0, "no edit or redo merged into the top step");
    // Every kind of action ran.
    for kind in [
        "answer", "bury", "edit", "undo", "redo", "api", "rollover", "sync", "finish", "merge",
    ] {
        assert!(
            actions.get(kind).copied().unwrap_or(0) > 0,
            "{kind} never ran: {actions:?}"
        );
    }
}

/// The unit's verification run (10,000 cases): `tools/mg core-test --
/// --run-ignored only -E 'test(=ten_thousand_random_histories)'`.
#[test]
#[ignore = "about a minute; the verification run of U25"]
fn ten_thousand_random_histories() {
    let (actions, merges) = walk_cases(1..=10_000);
    assert_eq!(actions.values().sum::<usize>(), 10_000 * 40);
    assert!(merges > 0);
}
