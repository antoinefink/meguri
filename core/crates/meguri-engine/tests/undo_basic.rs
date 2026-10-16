//! Undo and redo of recorded steps (`specs/engine/undo.md` M1 to M6, M10):
//! restores, redo, the 30-step limit, what each op source does to the
//! history, session scopes, `col.mod`, graves and restorers.

mod common;

use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::steps::{self, ANSWER_CARD, BURY, SUSPEND, UPDATE_NOTE};
use common::{Harness, col_mod, drain};
use meguri_engine::executor::catalog;
use meguri_engine::{
    ClearCause, Direction, DisabledReason, EngineError, EventKind, LongOp, OpSpec, RestoredStep, SessionId, StepKind,
    UndoRestorer, UndoScope, UndoStatus,
};
use meguri_model::card::CardQueue;
use meguri_model::changes::OpSource;
use meguri_model::deck::Deck;
use meguri_model::grave::GraveKind;
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::usn::Usn;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, StampMode};

const S1: SessionId = SessionId(1);
const S2: SessionId = SessionId(2);

fn peek(h: &Harness) -> UndoStatus {
    h.collection.peek_undo(UndoScope::Global).unwrap()
}

fn peek_name(h: &Harness) -> Option<&'static str> {
    peek(h).name()
}

fn refused(result: Result<impl std::fmt::Debug, EngineError>) -> DisabledReason {
    match result {
        Err(EngineError::UndoUnavailable { reason }) => reason,
        other => panic!("expected an undo refusal, got {other:?}"),
    }
}

/// A note with one card, added by a non-undoable op so it is no step.
fn note_with_card(h: &Harness, front: &str) -> (NoteId, CardId) {
    steps::setup_note(&h.collection, front)
}

fn card_queue(h: &Harness, card: CardId) -> CardQueue {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    reader.card(card).unwrap().unwrap().queue
}

#[test]
fn step_names_are_the_names_undo_ops_v4_lists() {
    // R370, A1, A11.5: undo_status maps names to V4's ids, so the engine's
    // names are V4's, in its order.
    let spec = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../specs/conformance/undo-ops.md"),
    )
    .unwrap();
    let section = spec
        .split("\n## V4.")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("undo-ops.md has a V4 section");
    let listed: Vec<&str> = section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| line.split('|').nth(2).unwrap_or_default().trim())
        .collect();
    assert_eq!(listed, meguri_engine::undo::names::ALL);
}

#[test]
fn undo_restores_the_dump_before_the_op_and_redo_the_dump_after_it() {
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "front");
    h.clock.step_ms(5_000);
    let before = steps::dump(&h.collection);
    h.clock.step_ms(5_000);
    let answered = steps::answer(&h.collection, Some(S1), cid).unwrap();
    h.clock.step_ms(5_000);
    steps::edit(&h.collection, Some(S1), nid, "edited").unwrap();
    let after = steps::dump(&h.collection);
    assert_ne!(before, after);

    h.clock.step_ms(5_000);
    let undone = h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(undone.value.name, UPDATE_NOTE);
    assert_eq!(undone.undo, None, "an undo records no step");
    assert!(undone.changes.note(nid).is_some());
    let undone = h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(undone.value.name, ANSWER_CARD);
    assert_eq!(undone.value.id.seq(), answered.seq);
    assert_eq!(undone.changes.revlog_removed, [answered.value]);
    assert_eq!(
        steps::dump(&h.collection),
        before,
        "every row, usn, mtime and col.mod as before"
    );

    h.clock.step_ms(5_000);
    let redone = h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(redone.value.name, ANSWER_CARD);
    assert_eq!(redone.undo.map(|u| u.name), Some(ANSWER_CARD), "a redo records a step");
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(
        steps::dump(&h.collection),
        after,
        "redo leaves what the ops left, col.mod included"
    );
    assert_eq!(peek_name(&h), Some(UPDATE_NOTE));
    assert_eq!(
        h.collection.peek_redo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None)
    );
}

fn peek_redo(h: &Harness) -> UndoStatus {
    h.collection.peek_redo(UndoScope::Global).unwrap()
}

#[test]
fn undo_then_a_new_step_empties_redo() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    steps::bury(&h.collection, None, cid).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(peek_redo(&h).name(), Some(BURY));

    steps::suspend(&h.collection, None, cid).unwrap();
    assert_eq!(peek_redo(&h), UndoStatus::Unavailable(DisabledReason::None));
    assert_eq!(refused(h.collection.redo(UndoScope::Global)), DisabledReason::None);
    assert_eq!(peek_name(&h), Some(SUSPEND));
}

#[test]
fn an_undoable_op_that_changes_nothing_records_nothing_but_empties_redo() {
    // A2, A11.1: every operation empties the redo history, whether or not
    // it changed a row.
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    steps::bury(&h.collection, None, cid).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(peek_redo(&h).name(), Some(BURY));

    let nothing = h
        .collection
        .run(OpSpec::named("touch_nothing", OpSource::UserUndoable), |_| Ok(()))
        .unwrap();
    assert_eq!(nothing.undo, None, "nothing recorded");
    assert_eq!(peek_redo(&h), UndoStatus::Unavailable(DisabledReason::None));
    assert_eq!(peek_name(&h), Some(ANSWER_CARD), "the steps below stay");
}

#[test]
fn an_undo_or_redo_that_is_refused_leaves_both_histories() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(refused(h.collection.undo(UndoScope::Global)), DisabledReason::None);
    assert_eq!(peek_redo(&h).name(), Some(ANSWER_CARD), "a refused undo keeps redo");
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(refused(h.collection.redo(UndoScope::Global)), DisabledReason::None);
    assert_eq!(peek_name(&h), Some(ANSWER_CARD), "a refused redo keeps the steps");
}

#[test]
fn thirty_one_steps_drop_the_oldest_and_peek_names_each_as_recorded() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    let mut names = Vec::new();
    for i in 0..31 {
        match i % 3 {
            0 => {
                steps::answer(&h.collection, Some(S1), cid).unwrap();
                names.push(ANSWER_CARD);
            }
            1 => {
                steps::bury(&h.collection, Some(S1), cid).unwrap();
                names.push(BURY);
            }
            _ => {
                steps::suspend(&h.collection, Some(S1), cid).unwrap();
                names.push(SUSPEND);
            }
        }
    }
    for expected in names.iter().rev().take(30) {
        let status = peek(&h);
        assert_eq!(status.name(), Some(*expected));
        let UndoStatus::Available(step) = status else {
            unreachable!()
        };
        assert_eq!(step.session, Some(S1));
        assert_eq!(step.kind == StepKind::Answer, *expected == ANSWER_CARD);
        assert_eq!(h.collection.undo(UndoScope::Global).unwrap().value.name, *expected);
    }
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::None), "the 31st went");
    assert_eq!(refused(h.collection.undo(UndoScope::Global)), DisabledReason::None);
    // The card is back to its state after the first answer, which is gone.
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.card(cid).unwrap().unwrap().reps, 1);
}

#[test]
fn rollover_clears_the_history_with_its_reason() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    h.collection.clear_undo(ClearCause::Rollover).unwrap();
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ClearedByRollover));
    assert_eq!(
        refused(h.collection.undo(UndoScope::Global)),
        DisabledReason::ClearedByRollover
    );

    // A rollover op that writes clears too.
    steps::answer(&h.collection, None, cid).unwrap();
    h.collection
        .run(OpSpec::named("rollover_unbury", OpSource::Rollover), move |ctx| {
            ctx.scope().set_config("lastUnburied", &ConfigValue::Integer(7))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ClearedByRollover));
}

#[test]
fn an_optimizer_maintenance_or_settings_write_clears_and_one_that_changes_nothing_does_not() {
    for source in [OpSource::Optimizer, OpSource::Maintenance, OpSource::UserNonUndoable] {
        let h = Harness::open();
        let (_, cid) = note_with_card(&h, "a");
        steps::answer(&h.collection, None, cid).unwrap();
        h.collection
            .run(OpSpec::named("reads_only", source), move |ctx| {
                ctx.scope().card(cid)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(peek_name(&h), Some(ANSWER_CARD), "{source}: nothing changed");
        h.collection
            .run(OpSpec::named("writes", source), move |ctx| {
                let mut card = ctx.scope().card(cid)?.unwrap();
                card.flags = 1;
                ctx.scope().put(&card)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            peek(&h),
            UndoStatus::Unavailable(DisabledReason::ClearedByNonUndoableOp),
            "{source}"
        );
    }
}

#[test]
fn check_database_and_an_outside_change_clear_with_their_reasons() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    h.collection.clear_undo(ClearCause::CheckDatabase).unwrap();
    assert_eq!(
        peek(&h),
        UndoStatus::Unavailable(DisabledReason::ClearedByNonUndoableOp)
    );
    steps::answer(&h.collection, None, cid).unwrap();
    h.collection.clear_undo(ClearCause::ExternalChange).unwrap();
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ChangedExternally));
}

/// The current-deck selection (R42): a skip-undo op.
fn select_deck(h: &Harness, deck: i128) -> meguri_engine::OpResult<()> {
    h.collection
        .run(
            OpSpec::named("set_current_deck", OpSource::UserNonUndoable).skip_undo(),
            move |ctx| {
                ctx.scope().set_config("curDeck", &ConfigValue::Integer(deck))?;
                ctx.scope()
                    .set_config("activeDecks", &ConfigValue::parse(b"[1]").unwrap())?;
                Ok(())
            },
        )
        .unwrap()
}

#[test]
fn selecting_a_deck_keeps_the_answer_undoable_and_a_settings_write_clears_it() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, Some(S1), cid).unwrap();
    steps::bury(&h.collection, Some(S1), cid).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(peek_redo(&h).name(), Some(BURY));
    let select = select_deck(&h, 1);
    assert!(!select.changes.config_keys.is_empty(), "the selection wrote");
    assert_eq!(select.undo, None, "not recorded");
    assert_eq!(peek_name(&h), Some(ANSWER_CARD), "the steps stay (R42)");
    assert_eq!(
        peek_redo(&h),
        UndoStatus::Unavailable(DisabledReason::None),
        "A11.1: a skip-undo op empties redo"
    );
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(peek_redo(&h).name(), Some(ANSWER_CARD));
    let again = select_deck(&h, 1);
    assert!(again.changes.is_empty(), "the same selection writes nothing");
    assert_eq!(
        peek_redo(&h),
        UndoStatus::Unavailable(DisabledReason::None),
        "and still empties redo"
    );
    steps::answer(&h.collection, Some(S1), cid).unwrap();

    let settings = h
        .collection
        .run(OpSpec::named("set_preference", OpSource::UserNonUndoable), |ctx| {
            ctx.scope().set_config("collapseTime", &ConfigValue::Integer(1234))?;
            Ok(())
        })
        .unwrap();
    assert!(!settings.changes.is_empty(), "the settings write changed a key");
    assert_eq!(
        peek(&h),
        UndoStatus::Unavailable(DisabledReason::ClearedByNonUndoableOp)
    );
}

#[test]
fn skip_undo_is_refused_for_any_source_but_user_non_undoable() {
    let h = Harness::open();
    for source in OpSource::ALL {
        let result = h.collection.run(OpSpec::named("skip", source).skip_undo(), |_| Ok(()));
        if source == OpSource::UserNonUndoable {
            assert!(result.is_ok());
        } else {
            assert!(
                matches!(result, Err(EngineError::InvalidArgument { .. })),
                "{source}: {result:?}"
            );
        }
    }
}

#[test]
fn a_session_scope_acts_only_on_a_top_step_of_its_own_session() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, Some(S1), cid).unwrap();
    steps::bury(&h.collection, Some(S2), cid).unwrap();
    let s1 = UndoScope::Session(S1);
    assert_eq!(
        h.collection.peek_undo(s1).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None)
    );
    assert_eq!(peek_name(&h), Some(BURY));
    assert_eq!(refused(h.collection.undo(s1)), DisabledReason::None);
    assert_eq!(card_queue(&h, cid), CardQueue::BuriedByUser, "nothing was undone");

    assert_eq!(h.collection.undo(UndoScope::Session(S2)).unwrap().value.name, BURY);
    assert_eq!(h.collection.peek_undo(s1).unwrap().name(), Some(ANSWER_CARD));
    assert_eq!(
        h.collection.peek_redo(s1).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None),
        "the redo step is S2's"
    );
    assert_eq!(
        h.collection.peek_redo(UndoScope::Session(S2)).unwrap().name(),
        Some(BURY)
    );

    // A step with no session is no session's.
    steps::suspend(&h.collection, None, cid).unwrap();
    assert_eq!(
        h.collection.peek_undo(s1).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None)
    );
    assert_eq!(peek_name(&h), Some(SUSPEND));
}

#[test]
fn undo_sets_col_mod_back_and_redo_sets_the_value_the_op_set() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    h.clock.step_ms(10_000);
    let before = col_mod(&h.collection);
    h.clock.step_ms(10_000);
    steps::answer(&h.collection, None, cid).unwrap();
    let set = col_mod(&h.collection);
    assert!(set > before);

    h.clock.step_ms(10_000);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col_mod(&h.collection), before, "storage/ids-usn-mtime S5: never now");
    h.clock.step_ms(10_000);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(col_mod(&h.collection), set);
    h.clock.step_ms(10_000);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col_mod(&h.collection), before);
}

#[test]
fn redo_sets_col_mod_to_its_value_when_the_undo_ran_and_the_next_undo_to_its_value_when_the_redo_ran() {
    // A7, A11.4: something that moves col.mod and keeps the history (a deck
    // selection, R42; a local-server write that reaches no step, M7) shows
    // which value each restore takes.
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    let (other, _) = note_with_card(&h, "b");
    h.clock.step_ms(10_000);
    let before = col_mod(&h.collection);
    h.clock.step_ms(10_000);
    steps::answer(&h.collection, None, cid).unwrap();
    let set = col_mod(&h.collection);
    h.clock.step_ms(10_000);
    select_deck(&h, 2);
    let selected = col_mod(&h.collection);
    assert!(before < set && set < selected);

    h.clock.step_ms(10_000);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col_mod(&h.collection), before, "the value before the answer");
    h.clock.step_ms(10_000);
    common::set_first_field(&h.collection, OpSource::LocalApi, other, "script").unwrap();
    let scripted = col_mod(&h.collection);
    assert!(scripted > selected);
    assert_eq!(peek_redo(&h).name(), Some(ANSWER_CARD), "the write reached no step");

    h.clock.step_ms(10_000);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(
        col_mod(&h.collection),
        selected,
        "the value when the undo ran, not the one the answer set"
    );
    h.clock.step_ms(10_000);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col_mod(&h.collection), scripted, "the value when the redo ran");
}

#[test]
fn undo_of_a_deletion_restores_rows_and_removes_its_graves_and_redo_adds_them_back() {
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "doomed");
    h.clock.step_ms(1_000);
    let before = steps::dump(&h.collection);
    h.clock.step_ms(1_000);
    let deleted = h
        .collection
        .run(
            OpSpec::named("remove_notes", OpSource::UserUndoable).undo_step("Delete Note", StepKind::Other),
            move |ctx| {
                ctx.scope().remove_note_and_cards(nid)?;
                Ok(())
            },
        )
        .unwrap();
    let after = steps::dump(&h.collection);
    assert_eq!(after["graves"].len(), before["graves"].len() + 2);

    let undone = h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(undone.value.name, "Delete Note");
    assert_eq!(steps::dump(&h.collection), before);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.grave(cid.0, GraveKind::Card).unwrap().is_none());
    assert!(reader.grave(nid.0, GraveKind::Note).unwrap().is_none());
    drop(reader);

    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(steps::dump(&h.collection), after);
    assert_eq!(peek_name(&h), Some("Delete Note"));
    assert!(deleted.undo.is_some());
}

#[test]
fn undo_removes_an_earlier_grave_of_an_object_the_step_deleted_and_redo_puts_it_back() {
    // The note's grave exists before the deletion (an object re-created
    // under a deleted id), so the deletion adds none for it (graves G2); the
    // undo still removes it, as Anki does (graves G6).
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "again");
    h.collection
        .run(OpSpec::named("old_grave", OpSource::UserNonUndoable), move |ctx| {
            ctx.scope().add_grave(nid.0, GraveKind::Note, Usn(-1))?;
            Ok(())
        })
        .unwrap();
    let reader_grave = |h: &Harness| {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        (
            reader.grave(nid.0, GraveKind::Note).unwrap().is_some(),
            reader.grave(cid.0, GraveKind::Card).unwrap().is_some(),
        )
    };
    h.collection
        .run(
            OpSpec::named("remove_notes", OpSource::UserUndoable).undo_step("Delete Note", StepKind::Other),
            move |ctx| {
                ctx.scope().remove_note_and_cards(nid)?;
                Ok(())
            },
        )
        .unwrap();
    let after = steps::dump(&h.collection);
    assert_eq!(reader_grave(&h), (true, true));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(reader_grave(&h), (false, false), "both graves gone");
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(reader_grave(&h), (true, true));
    assert_eq!(steps::dump(&h.collection), after);
}

#[test]
fn a_redo_of_a_deletion_writes_every_grave_its_undo_removed_back_pending() {
    // A5, A11.3: the note's grave was synced (usn 5) before the deletion
    // kept it; after the undo and the redo every grave of the deletion is
    // pending, and the undo of the redone deletion removes them all again.
    let h = Harness::open();
    let (nid, cid) = note_with_card(&h, "again");
    h.collection
        .run(
            OpSpec::named("old_grave", OpSource::UserNonUndoable).stamp(StampMode::CallerSupplied),
            move |ctx| {
                ctx.scope().add_grave(nid.0, GraveKind::Note, Usn(5))?;
                Ok(())
            },
        )
        .unwrap();
    let graves = |h: &Harness| {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        (
            reader.grave(nid.0, GraveKind::Note).unwrap().map(|g| g.usn),
            reader.grave(cid.0, GraveKind::Card).unwrap().map(|g| g.usn),
        )
    };
    h.collection
        .run(
            OpSpec::named("remove_notes", OpSource::UserUndoable).undo_step("Delete Note", StepKind::Other),
            move |ctx| {
                ctx.scope().remove_note_and_cards(nid)?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        graves(&h),
        (Some(Usn(5)), Some(Usn(-1))),
        "graves G2: the old grave kept"
    );
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(graves(&h), (None, None));
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(graves(&h), (Some(Usn(-1)), Some(Usn(-1))), "every grave pending");
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(graves(&h), (None, None));
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(graves(&h), (Some(Usn(-1)), Some(Usn(-1))));
}

#[test]
fn a_row_changed_outside_the_history_disables_the_step_with_changed_externally() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    h.collection
        .run(
            OpSpec::named("set_deck_view", OpSource::UserUndoable).undo_step("Set Option", StepKind::Other),
            |ctx| {
                ctx.scope().set_config("curDeck", &ConfigValue::Integer(5))?;
                Ok(())
            },
        )
        .unwrap();
    // A skip-undo selection changes the step's row without telling the
    // history.
    h.collection
        .run(OpSpec::named("select", OpSource::UserNonUndoable).skip_undo(), |ctx| {
            ctx.scope().set_config("curDeck", &ConfigValue::Integer(1))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(peek_name(&h), Some("Set Option"), "peek reads no rows");
    let dump = steps::dump(&h.collection);
    assert_eq!(
        refused(h.collection.undo(UndoScope::Global)),
        DisabledReason::ChangedExternally
    );
    assert_eq!(steps::dump(&h.collection), dump, "nothing written");
    assert_eq!(peek(&h), UndoStatus::Unavailable(DisabledReason::ChangedExternally));
    assert_eq!(
        refused(h.collection.undo(UndoScope::Global)),
        DisabledReason::ChangedExternally
    );
}

#[test]
fn a_redo_whose_rows_changed_is_refused_and_dropped() {
    // A8: every op empties redo, so only a writer outside the history can
    // change a redo step's rows. Here an SQL trigger takes the review-log
    // id of the answer as the undo deletes its row; the redo's check (M5)
    // finds the row and refuses (deviation undo-revlog-taken-id-redo).
    let h = Harness::open();
    steps::answer(&h.collection, None, note_with_card(&h, "a").1).unwrap();
    let (_, card) = note_with_card(&h, "b");
    steps::answer(&h.collection, None, card).unwrap();
    // The maintenance lane is the only raw writer; it stands in for
    // another process here.
    h.collection
        .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), |writer| {
            writer.execute_raw(
                "CREATE TRIGGER occupy AFTER DELETE ON revlog WHEN old.usn = -1 BEGIN \
                 INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) \
                 VALUES (old.id, old.cid, 9, 4, 4, 0, 2500, 1500, 0); END",
                &[],
            )?;
            Ok(())
        })
        .wait()
        .unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(peek_redo(&h).name(), Some(ANSWER_CARD));
    let dump = steps::dump(&h.collection);
    assert_eq!(
        refused(h.collection.redo(UndoScope::Global)),
        DisabledReason::ChangedExternally
    );
    assert_eq!(steps::dump(&h.collection), dump, "nothing written");
    assert_eq!(peek_redo(&h), UndoStatus::Unavailable(DisabledReason::None));
    assert_eq!(peek_name(&h), Some(ANSWER_CARD), "the history below stays");
}

#[test]
fn undo_and_redo_of_a_deck_name_swap_meet_no_unique_name_conflict() {
    let h = Harness::open();
    let (a, b) = h
        .collection
        .run(OpSpec::named("add_decks", OpSource::UserNonUndoable), |ctx| {
            let mut deck: Deck = ctx.scope().deck(DeckId(1))?.unwrap();
            deck.name = "Alpha".to_owned();
            let a = ctx.scope().add_deck(&deck)?;
            deck.name = "Beta".to_owned();
            let b = ctx.scope().add_deck(&deck)?;
            Ok((a, b))
        })
        .unwrap()
        .value;
    let rename = |ctx: &mut meguri_engine::OpContext<'_, '_>, id: DeckId, name: &str| -> Result<(), EngineError> {
        let mut deck = ctx.scope().deck(id)?.unwrap();
        deck.name = name.to_owned();
        ctx.scope().put(&deck)?;
        Ok(())
    };
    h.collection
        .run(
            OpSpec::named("swap", OpSource::UserUndoable).undo_step("Rename Deck", StepKind::Other),
            move |ctx| {
                rename(ctx, a, "Swap")?;
                rename(ctx, b, "Alpha")?;
                rename(ctx, a, "Beta")?;
                Ok(())
            },
        )
        .unwrap();
    let name = |h: &Harness, id: DeckId| {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        reader.deck(id).unwrap().unwrap().name
    };
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!((name(&h, a), name(&h, b)), ("Alpha".to_owned(), "Beta".to_owned()));
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!((name(&h, a), name(&h, b)), ("Beta".to_owned(), "Alpha".to_owned()));
}

#[test]
fn a_collection_replacement_clears_with_cleared_by_one_way() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    let replacement = h.dir.path().join("replacement.anki2");
    std::fs::copy(common::fixture("schema-18.anki2"), &replacement).unwrap();
    {
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(false).unwrap();
        exclusive
            .replace(move |live| {
                std::fs::rename(&replacement, live).map_err(|e| EngineError::Internal { message: e.to_string() })
            })
            .unwrap();
    }
    let current = h.engine.collection().unwrap();
    assert_eq!(
        current.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::ClearedByOneWay)
    );
    assert!(matches!(
        h.collection.peek_undo(UndoScope::Global),
        Err(EngineError::StaleGeneration { .. })
    ));
}

#[test]
fn closing_the_collection_drops_the_history() {
    let h = Harness::open();
    let (_, cid) = note_with_card(&h, "a");
    steps::answer(&h.collection, None, cid).unwrap();
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(
        reopened.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::None)
    );
}

#[test]
fn a_step_of_a_long_write_reports_it_and_its_undo_and_redo_declare_it() {
    let h = Harness::open();
    let notes = common::add_notes(&h.collection, 20, "bulk");
    let declared = Duration::from_millis(400);
    h.collection
        .run(
            OpSpec::new(catalog::BULK_OP, OpSource::UserUndoable)
                .declare_long_write(declared)
                .undo_step("Find and Replace", StepKind::Other),
            move |ctx| {
                for nid in &notes {
                    steps::set_first_field(ctx, *nid, "replaced".to_owned())?;
                }
                Ok(())
            },
        )
        .unwrap();
    let UndoStatus::Available(step) = peek(&h) else {
        panic!("the bulk step is available")
    };
    assert_eq!(step.long_write, Some(declared));
    let events = h.engine.stream().subscribe();
    let undone = h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(undone.changes.notes.len(), 20);
    let redo_step = h.collection.peek_redo(UndoScope::Global).unwrap();
    let UndoStatus::Available(redo_step) = redo_step else {
        panic!("redo available")
    };
    assert_eq!(redo_step.long_write, Some(declared));
    h.collection.redo(UndoScope::Global).unwrap();
    let delays: Vec<&'static str> = drain(&events, Duration::from_millis(100))
        .iter()
        .filter_map(|event| match event.kind {
            EventKind::WriteDelay { op, declared: d } if d == declared => Some(op),
            _ => None,
        })
        .collect();
    assert_eq!(delays, ["undo_long_write", "redo_long_write"]);
}

/// One call a restorer got: direction, step name, state, cards changed.
type Seen = (Direction, &'static str, Option<String>, usize);

/// Records what it was told, with the state as a string.
#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<Seen>>,
}

impl UndoRestorer for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    fn restored(&self, step: &RestoredStep<'_>) {
        let state = step
            .state
            .and_then(|state| (state as &dyn Any).downcast_ref::<String>())
            .cloned();
        self.seen
            .lock()
            .unwrap()
            .push((step.direction, step.step.name, state, step.changes.cards.len()));
    }
}

#[test]
fn restorers_get_each_undo_and_redo_with_the_state_the_op_attached() {
    let h = Harness::open();
    let recorder = Arc::new(Recorder::default());
    h.collection.add_undo_restorer(recorder.clone());
    struct Panics;
    impl UndoRestorer for Panics {
        fn name(&self) -> &str {
            "panics"
        }
        fn restored(&self, _: &RestoredStep<'_>) {
            panic!("a broken restorer")
        }
    }
    h.collection.add_undo_restorer(Arc::new(Panics));
    let (_, cid) = note_with_card(&h, "a");
    h.collection
        .run(
            OpSpec::named("answer_card", OpSource::UserUndoable).undo_step(ANSWER_CARD, StepKind::Answer),
            move |ctx| {
                let mut card = ctx.scope().card(cid)?.unwrap();
                card.reps += 1;
                ctx.scope().put(&card)?;
                ctx.set_undo_state(Arc::new("cutoff 42".to_owned()));
                Ok(())
            },
        )
        .unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    h.collection.redo(UndoScope::Global).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    let state = Some("cutoff 42".to_owned());
    assert_eq!(
        *recorder.seen.lock().unwrap(),
        [
            (Direction::Undo, ANSWER_CARD, state.clone(), 1),
            (Direction::Redo, ANSWER_CARD, state.clone(), 1),
            (Direction::Undo, ANSWER_CARD, state, 1),
        ]
    );
}

#[test]
fn a_tag_whose_letter_case_the_step_changed_is_restored() {
    let h = Harness::open();
    h.collection
        .run(OpSpec::named("add_tag", OpSource::UserNonUndoable), |ctx| {
            ctx.scope().put(&meguri_model::tag::Tag {
                name: "Verbs".to_owned(),
                ..Default::default()
            })?;
            Ok(())
        })
        .unwrap();
    let before = steps::dump(&h.collection);
    h.collection
        .run(
            OpSpec::named("rename_tag", OpSource::UserUndoable).undo_step("Rename Tag", StepKind::Other),
            |ctx| {
                ctx.scope().remove_tag("Verbs")?;
                ctx.scope().put(&meguri_model::tag::Tag {
                    name: "verbs".to_owned(),
                    ..Default::default()
                })?;
                Ok(())
            },
        )
        .unwrap();
    let after = steps::dump(&h.collection);
    assert_ne!(before["tags"], after["tags"]);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(steps::dump(&h.collection)["tags"], before["tags"]);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(steps::dump(&h.collection)["tags"], after["tags"]);
}
