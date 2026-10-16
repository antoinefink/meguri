//! Local-server writes and the history (`specs/engine/undo.md` M7, R1795,
//! R1796, AE80): never recorded, never clearing it, but truncating it at the
//! deepest step they intersect.

mod common;

use common::steps::{self, ANSWER_CARD, BURY, UPDATE_NOTE};
use common::{Harness, set_first_field};
use meguri_engine::{DisabledReason, EngineError, OpSpec, SessionId, UndoScope, UndoStatus};
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, NoteId};
use meguri_storage::{ReadRows, ReaderRole};

fn peek(h: &Harness) -> UndoStatus {
    h.collection.peek_undo(UndoScope::Global).unwrap()
}

fn note_with_card(h: &Harness, front: &str) -> (NoteId, CardId) {
    steps::setup_note(&h.collection, front)
}

fn refused(result: Result<impl std::fmt::Debug, EngineError>) -> DisabledReason {
    match result {
        Err(EngineError::UndoUnavailable { reason }) => reason,
        other => panic!("expected an undo refusal, got {other:?}"),
    }
}

#[test]
fn an_api_edit_of_the_note_of_an_answered_card_truncates_the_answer() {
    // AE80: the top step answers a card of note N; a script edits N's fields.
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "N");
    steps::answer(&h.collection, Some(SessionId(1)), cid).unwrap();
    assert_eq!(peek(&h).name(), Some(ANSWER_CARD));

    let api = set_first_field(&h.collection, OpSource::LocalApi, nid, "from the script").unwrap();
    assert_eq!(api.undo, None, "API writes are never recorded (R1795)");
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ChangedExternally));
    assert_eq!(
        refused(h.collection.undo(UndoScope::Global)),
        DisabledReason::ChangedExternally
    );
    assert_eq!(common::note_fields(&h.collection, nid), "from the script\u{1f}back");
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.card(cid).unwrap().unwrap().reps, 1, "the answer stands");
}

#[test]
fn an_api_change_of_the_third_of_five_steps_keeps_the_two_newer_ones() {
    let h = Harness::open();
    let notes: Vec<(NoteId, CardId)> = (0..5).map(|i| note_with_card(&h, &format!("n{i}"))).collect();
    // Steps 5 (oldest) to 1 (newest), each on its own note.
    for (nid, _) in &notes {
        steps::edit(&h.collection, None, *nid, "user").unwrap();
    }
    let third = notes[2].0;
    set_first_field(&h.collection, OpSource::LocalApi, third, "script").unwrap();

    for _ in 0..2 {
        assert_eq!(peek(&h).name(), Some(UPDATE_NOTE));
        h.collection.undo(UndoScope::Global).unwrap();
    }
    assert_eq!(common::note_fields(&h.collection, notes[4].0), "n4\u{1f}back");
    assert_eq!(common::note_fields(&h.collection, notes[3].0), "n3\u{1f}back");
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ChangedExternally));
    assert_eq!(
        refused(h.collection.undo(UndoScope::Global)),
        DisabledReason::ChangedExternally
    );
    // Steps 3 to 5 are gone, their edits stay.
    assert_eq!(common::note_fields(&h.collection, third), "script\u{1f}back");
    assert_eq!(common::note_fields(&h.collection, notes[0].0), "user\u{1f}back");
}

#[test]
fn when_several_steps_intersect_the_cut_is_below_the_newest_of_them() {
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "N");
    let (other, _) = note_with_card(&h, "O");
    steps::edit(&h.collection, None, nid, "older edit").unwrap();
    // An answer on a card of N: its rows are not N's, but it reaches N (AE80).
    steps::answer(&h.collection, None, cid).unwrap();
    steps::edit(&h.collection, None, other, "newest").unwrap();
    set_first_field(&h.collection, OpSource::LocalApi, nid, "script").unwrap();

    assert_eq!(h.collection.undo(UndoScope::Global).unwrap().value.name, UPDATE_NOTE);
    assert_eq!(common::note_fields(&h.collection, other), "O\u{1f}back");
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ChangedExternally));
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.card(cid).unwrap().unwrap().reps, 1, "the answer was not undone");
}

#[test]
fn an_api_change_touching_no_step_leaves_the_history_whole() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "mine");
    let (other, _) = note_with_card(&h, "theirs");
    steps::answer(&h.collection, None, cid).unwrap();
    steps::bury(&h.collection, None, cid).unwrap();
    set_first_field(&h.collection, OpSource::LocalApi, other, "script").unwrap();
    assert_eq!(h.collection.undo(UndoScope::Global).unwrap().value.name, BURY);
    assert_eq!(h.collection.undo(UndoScope::Global).unwrap().value.name, ANSWER_CARD);
}

#[test]
fn an_api_change_of_a_card_reaches_the_steps_of_that_card_only() {
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "a");
    let (_, other_card) = note_with_card(&h, "b");
    steps::edit(&h.collection, None, nid, "edited").unwrap();
    steps::bury(&h.collection, None, other_card).unwrap();
    // A card change does not reach a step that edited only the card's note.
    h.collection
        .run(OpSpec::named("api_set_due", OpSource::LocalApi), move |ctx| {
            let mut card = ctx.scope().card(cid)?.unwrap();
            card.due = 99;
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(h.collection.undo(UndoScope::Global).unwrap().value.name, BURY);
    assert_eq!(h.collection.undo(UndoScope::Global).unwrap().value.name, UPDATE_NOTE);
}

#[test]
fn an_api_change_truncates_redo_at_the_deepest_step_it_reaches() {
    let h = Harness::open();
    let (a, _) = note_with_card(&h, "a");
    let (b, _) = note_with_card(&h, "b");
    let (c, _) = note_with_card(&h, "c");
    steps::edit(&h.collection, None, a, "1").unwrap();
    steps::edit(&h.collection, None, b, "2").unwrap();
    steps::edit(&h.collection, None, c, "3").unwrap();
    for _ in 0..3 {
        h.collection.undo(UndoScope::Global).unwrap();
    }
    // Redo order: a, then b, then c. The script touches b.
    set_first_field(&h.collection, OpSource::LocalApi, b, "script").unwrap();
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(common::note_fields(&h.collection, a), "1\u{1f}back");
    assert_eq!(
        h.collection.peek_redo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None)
    );
    assert_eq!(refused(h.collection.redo(UndoScope::Global)), DisabledReason::None);
    assert_eq!(common::note_fields(&h.collection, c), "c\u{1f}back");
}

#[test]
fn an_api_write_that_changes_nothing_truncates_nothing() {
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "same");
    steps::answer(&h.collection, None, cid).unwrap();
    let unchanged = set_first_field(&h.collection, OpSource::LocalApi, nid, "same").unwrap();
    assert!(unchanged.changes.is_empty());
    assert_eq!(peek(&h).name(), Some(ANSWER_CARD));
}
