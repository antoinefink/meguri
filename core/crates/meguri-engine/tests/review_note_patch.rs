//! `review/edit-current` E2: the review's Edit Note saves as a step of the
//! review session, so the session's Undo covers it, admitted only for the
//! open session.
mod session_support;
use meguri_engine::{EngineError, SessionId, StepKind, UndoScope, UndoStatus, notes};
use meguri_model::{clock::Clock, time::EpochMillis};
use meguri_sched::answer::Rating;
use meguri_model::ids::NoteId;
use meguri_storage::{ReadRows, ReaderRole};

fn patch(h: &session_support::Harness, note: NoteId, back: &str) -> notes::NotePatch {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let stored = r.note(note).unwrap().unwrap();
    let layout = r
        .notetype(stored.notetype_id)
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|field| notes::AddFieldIdentity {
            ord: field.ord,
            id: field.config.id,
            name: field.name.clone(),
        })
        .collect();
    notes::NotePatch {
        id: note,
        notetype_id: stored.notetype_id,
        field_layout: layout,
        fields: vec![(1, back.to_owned())],
        tags: None,
    }
}

fn back(h: &session_support::Harness, note: NoteId) -> String {
    let stored = h.collection.reader(ReaderRole::Query).unwrap().note(note).unwrap().unwrap();
    stored.field_values().nth(1).unwrap().to_owned()
}

#[test]
fn the_reviews_edit_is_a_step_of_its_session_and_its_undo_restores_the_note() {
    let h = session_support::Harness::new(3);
    let (id, head) = h.start();
    let note = head.items[0].note_id;
    let before = back(&h, note);
    let saved = h.review.patch_note(patch(&h, note, "edited"), id, None).unwrap();
    assert!(saved.value.written);
    assert_eq!(back(&h, note), "edited");
    match h.collection.peek_undo(UndoScope::Session(id)).unwrap() {
        UndoStatus::Available(step) => {
            assert_eq!(step.session, Some(id));
            assert_eq!(step.kind, StepKind::Other);
            assert_eq!(step.name, "Update Note");
        }
        other => panic!("the session's undo offers the edit: {other:?}"),
    }
    h.review.undo(UndoScope::Session(id)).unwrap();
    assert_eq!(back(&h, note), before, "Undo restores the note");
    h.review.end(id).unwrap();
}

#[test]
fn an_edit_naming_no_open_session_is_refused_and_writes_nothing() {
    let h = session_support::Harness::new(2);
    let (id, head) = h.start();
    let note = head.items[0].note_id;
    let before = back(&h, note);
    let refused = h.review.patch_note(patch(&h, note, "edited"), SessionId(id.0 + 100), None);
    assert!(matches!(refused, Err(EngineError::InvalidArgument { .. })), "{refused:?}");
    assert_eq!(back(&h, note), before);
    // A patch loaded in another generation is refused on the review's own collection.
    let current = h.collection.generation();
    let stale = h.review.patch_note(patch(&h, note, "edited"), id, Some(meguri_engine::Generation(current.0 + 1)));
    assert!(matches!(stale, Err(EngineError::StaleGeneration { .. })), "{stale:?}");
    assert_eq!(back(&h, note), before);
    h.review.end(id).unwrap();
    // An ended session admits no edit either.
    let ended = h.review.patch_note(patch(&h, note, "edited"), id, None);
    assert!(ended.is_err(), "{ended:?}");
    assert_eq!(back(&h, note), before);
}

#[test]
fn the_edit_is_a_new_step_so_an_undone_answer_can_no_longer_be_redone() {
    let h = session_support::Harness::new(3);
    let (id, head) = h.start();
    let answered = &head.items[0];
    h.review
        .answer(id, answered.card_id, Rating::Good, answered.state_token, EpochMillis(h.clock.now_ms()), 0)
        .unwrap();
    h.review.undo(UndoScope::Session(id)).unwrap();
    assert!(matches!(h.review.peek_redo(UndoScope::Session(id)).unwrap(), UndoStatus::Available(_)));
    let note = h.review.head(id, 3).unwrap().items[0].note_id;
    h.review.patch_note(patch(&h, note, "edited"), id, None).unwrap();
    let redo = h.review.peek_redo(UndoScope::Session(id)).unwrap();
    assert!(!matches!(redo, UndoStatus::Available(_)), "{redo:?}");
    h.review.end(id).unwrap();
}
