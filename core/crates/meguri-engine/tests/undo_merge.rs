//! Merged note edits (`specs/engine/undo.md` A3, M1): an "Update Note" op
//! that changes only one note row records no step of its own when the top
//! step is an "Update Note" step of that same row whose first op began less
//! than 60 seconds earlier. The expected rows and values follow the
//! `undo/edit-note-merge` golden's shape.

mod common;

use common::steps::{self, ANSWER_CARD, UPDATE_NOTE};
use common::{Harness, col_mod, note_fields};
use meguri_engine::{DisabledReason, OpResult, OpSpec, SessionId, StepKind, UndoScope, UndoStatus};
use meguri_model::changes::OpSource;
use meguri_model::ids::NoteId;

const MINUTE: i64 = 60_000;

fn note(h: &Harness, front: &str) -> NoteId {
    steps::setup_note(&h.collection, front).0
}

fn edit(h: &Harness, nid: NoteId, value: &str) -> OpResult<()> {
    steps::edit(&h.collection, None, nid, value).unwrap()
}

/// The id of the step `undo` would undo now.
fn top(h: &Harness) -> Option<u64> {
    match h.collection.peek_undo(UndoScope::Global).unwrap() {
        UndoStatus::Available(step) => Some(step.id.0),
        UndoStatus::Unavailable(_) => None,
    }
}

fn undo(h: &Harness) -> u64 {
    h.collection.undo(UndoScope::Global).unwrap().value.id.0
}

fn redo(h: &Harness) -> u64 {
    h.collection.redo(UndoScope::Global).unwrap().value.id.0
}

#[test]
fn edits_within_sixty_seconds_of_the_steps_first_one_merge_and_one_at_sixty_does_not() {
    let h = Harness::open();
    let cat = note(&h, "cat");
    let dog = note(&h, "dog");
    h.clock.step_ms(60 * MINUTE);
    let before = steps::dump(&h.collection);

    // 0 s, 30 s and 59 s after the step began: one step.
    let first = edit(&h, cat, "gata");
    h.clock.step_ms(30_000);
    let second = edit(&h, cat, "gatita");
    assert_eq!(
        second.undo.map(|u| u.name),
        Some(UPDATE_NOTE),
        "the merged edit is undoable through the step"
    );
    h.clock.step_ms(29_000);
    edit(&h, cat, "gatito");
    let merged = steps::dump(&h.collection);
    assert_eq!(top(&h), Some(first.seq.0), "still the first edit's step");

    // 60 s after the step began, 1 s after the last edit: a step of its own.
    h.clock.step_ms(1_000);
    let own = edit(&h, cat, "michi");
    assert_eq!(top(&h), Some(own.seq.0));
    let michi = steps::dump(&h.collection);
    // Another note: a step of its own.
    h.clock.step_ms(5_000);
    let other = edit(&h, dog, "perra");
    assert_eq!(top(&h), Some(other.seq.0));

    assert_eq!(undo(&h), other.seq.0);
    assert_eq!(steps::dump(&h.collection), michi);
    assert_eq!(undo(&h), own.seq.0);
    assert_eq!(
        steps::dump(&h.collection),
        merged,
        "the last merged edit, its mod and col.mod"
    );
    assert_eq!(undo(&h), first.seq.0);
    assert_eq!(
        steps::dump(&h.collection),
        before,
        "one undo puts the note and col.mod back as before the first edit"
    );
    assert_eq!(top(&h), None);

    redo(&h);
    assert_eq!(
        steps::dump(&h.collection),
        merged,
        "redo puts the last merged edit back"
    );
}

#[test]
fn a_redone_step_counts_from_the_redo_and_a_redo_merges_as_an_edit_does() {
    // The golden of undo/edit-note-merge, steps 21 to 31: two redos of the
    // same note at one instant, then an edit 30 s later, are one step.
    let h = Harness::open();
    let cat = note(&h, "cat");
    let dog = note(&h, "dog");
    h.clock.step_ms(60 * MINUTE);
    let before = steps::dump(&h.collection);
    let original = col_mod(&h.collection);
    edit(&h, cat, "gata");
    h.clock.step_ms(MINUTE);
    edit(&h, cat, "michi");
    h.clock.step_ms(5_000);
    edit(&h, dog, "perra");
    for _ in 0..3 {
        undo(&h);
    }
    assert_eq!(steps::dump(&h.collection), before);

    h.clock.step_ms(60 * MINUTE);
    let first_redo = h.collection.redo(UndoScope::Global).unwrap();
    let second_redo = h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(note_fields(&h.collection, cat), "michi\u{1f}back");
    assert_eq!(
        top(&h),
        Some(first_redo.seq.0),
        "the second redo merged into the step the first one recorded"
    );
    assert_eq!(second_redo.undo.map(|u| u.name), Some(UPDATE_NOTE));
    assert_eq!(
        h.collection.peek_redo(UndoScope::Global).unwrap().name(),
        Some(UPDATE_NOTE),
        "the edit of the other note is still to redo"
    );

    h.clock.step_ms(30_000);
    edit(&h, cat, "neko");
    assert_eq!(top(&h), Some(first_redo.seq.0), "30 s after the redo: merged");
    undo(&h);
    assert_eq!(steps::dump(&h.collection), before, "all three reverted");
    assert_eq!(col_mod(&h.collection), original);
    assert_eq!(top(&h), None);
    redo(&h);
    assert_eq!(note_fields(&h.collection, cat), "neko\u{1f}back");
    assert_eq!(
        h.collection.peek_redo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None),
        "the edit emptied redo"
    );
}

#[test]
fn whole_seconds_count_and_a_clock_moved_back_counts_as_zero() {
    let h = Harness::open();
    let cat = note(&h, "cat");
    // The step begins 0.9 s into a second: second s.
    h.clock.step_ms(900);
    let first = edit(&h, cat, "a");
    // s + 59.95 s is second s + 59: merged.
    h.clock.step_ms(59_050);
    edit(&h, cat, "b");
    assert_eq!(top(&h), Some(first.seq.0));
    // Back by ten minutes: counts as 0 s.
    h.clock.step_ms(-10 * MINUTE);
    edit(&h, cat, "c");
    assert_eq!(top(&h), Some(first.seq.0));
    // Second s + 60: a new step.
    h.clock.step_ms(10 * MINUTE + 50);
    let own = edit(&h, cat, "d");
    assert_eq!(top(&h), Some(own.seq.0));
}

#[test]
fn an_edit_that_changes_another_row_is_a_step_and_no_edit_merges_into_it() {
    let h = Harness::open();
    let cat = note(&h, "cat");
    let edit_and_add_card = |nid: NoteId| {
        h.collection
            .run(
                OpSpec::named("update_note", OpSource::UserUndoable).undo_step(UPDATE_NOTE, StepKind::Other),
                move |ctx| {
                    steps::set_first_field(ctx, nid, "generated".to_owned())?;
                    ctx.scope().add_card(&common::card(nid, 1))?;
                    Ok(())
                },
            )
            .unwrap()
    };
    let plain = edit(&h, cat, "a");
    h.clock.step_ms(10_000);
    let generating = edit_and_add_card(cat);
    assert_eq!(top(&h), Some(generating.seq.0), "a card generated: not merged");
    h.clock.step_ms(10_000);
    let later = edit(&h, cat, "b");
    assert_eq!(
        top(&h),
        Some(later.seq.0),
        "nothing merges into a step that changed a card"
    );
    assert_eq!(undo(&h), later.seq.0);
    assert_eq!(undo(&h), generating.seq.0);
    assert_eq!(undo(&h), plain.seq.0);
}

#[test]
fn only_update_note_steps_of_the_same_note_merge() {
    let h = Harness::open();
    let (cat, card) = steps::setup_note(&h.collection, "cat");
    let dog = note(&h, "dog");
    steps::answer(&h.collection, None, card).unwrap();
    let after_answer = edit(&h, cat, "a");
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some(UPDATE_NOTE),
        "an edit after an answer is a step of its own"
    );
    let dog_edit = edit(&h, dog, "b");
    assert_eq!(top(&h), Some(dog_edit.seq.0));
    // An op of another name that changes only the note row.
    let renamed = h
        .collection
        .run(
            OpSpec::named("find_replace", OpSource::UserUndoable).undo_step("Find and Replace", StepKind::Other),
            move |ctx| steps::set_first_field(ctx, dog, "c".to_owned()),
        )
        .unwrap();
    assert_eq!(top(&h), Some(renamed.seq.0));
    let after = edit(&h, dog, "d");
    assert_eq!(top(&h), Some(after.seq.0), "not into a step of another name");
    for expected in [after.seq.0, renamed.seq.0, dog_edit.seq.0, after_answer.seq.0] {
        assert_eq!(undo(&h), expected);
    }
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some(ANSWER_CARD)
    );
}

#[test]
fn an_edit_in_another_session_is_a_step_of_its_own() {
    let h = Harness::open();
    let cat = note(&h, "cat");
    let first = steps::edit(&h.collection, Some(SessionId(1)), cat, "a").unwrap();
    let outside = steps::edit(&h.collection, None, cat, "b").unwrap();
    assert_eq!(top(&h), Some(outside.seq.0));
    let again = steps::edit(&h.collection, None, cat, "c").unwrap();
    assert_eq!(top(&h), Some(outside.seq.0), "merged into the step of no session");
    assert_ne!(again.seq, first.seq);
    assert_eq!(undo(&h), outside.seq.0);
    assert_eq!(undo(&h), first.seq.0);
}

#[test]
fn nothing_merges_into_an_uploaded_or_pending_step() {
    let h = Harness::open();
    let cat = note(&h, "cat");
    edit(&h, cat, "a");
    // A sync's barrier flags the step pending (M9.1).
    h.collection
        .run(OpSpec::named("sync_snapshot", OpSource::SyncIncoming), |ctx| {
            ctx.begin_sync_hold(None)
        })
        .unwrap();
    let during = edit(&h, cat, "b");
    assert_eq!(top(&h), Some(during.seq.0), "a step of its own, undoable");
    h.collection.sync_finish_sent().unwrap();
    assert_eq!(undo(&h), during.seq.0);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::AlreadyUploaded)
    );
    let after = edit(&h, cat, "c");
    assert_eq!(top(&h), Some(after.seq.0), "a step of its own, undoable");
    assert_eq!(undo(&h), after.seq.0);
    assert_eq!(note_fields(&h.collection, cat), "a\u{1f}back");
}
