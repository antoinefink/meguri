//! Keys-only engine contracts for U113/U114: undo M8.1, M9.6–M9.10.
//! These enabled tests intentionally require the future Cloud APIs; no service
//! or record application is simulated here. Frozen sync is not involved.
//!
//! Proposed API: OpContext::begin_cloud_hold(session, &[key]) -> CloudHold
//! { snapshot, run, flagged: BTreeMap<EntryId, BTreeSet<(Table, RowKey)>> };
//! Collection::{cloud_held_run(session), mark_cloud_uploaded(&[EntryId])};
//! OpResult::cloud_detached_run: Option<HeldBackRun>. Existing Result/error
//! conventions and HeldBackRun's lazy journal-backed images are retained.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::time::Duration;

use common::Harness;
use common::steps;
use meguri_engine::undo::holdback::CloudHold;
use meguri_engine::{
    Collection, DisabledReason, EngineError, EntryId, HeldBackRun, OpResult, OpSpec, SessionId, StepKind, UndoScope,
    UndoStatus,
};
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, DeckId, NoteId, RevlogId};
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, ReaderRole, Record, RowKey, Table};

const S1: SessionId = SessionId(1);
const S2: SessionId = SessionId(2);
type Key = (Table, RowKey);

fn key(table: Table, id: i64) -> Key {
    (table, RowKey::Id(id))
}

fn ids(run: &HeldBackRun) -> Vec<u64> {
    run.steps.iter().map(|step| step.id.0).collect()
}

fn refused(result: Result<impl std::fmt::Debug, EngineError>) -> DisabledReason {
    match result {
        Err(EngineError::UndoUnavailable { reason }) => reason,
        other => panic!("expected an undo refusal, got {other:?}"),
    }
}

fn barrier(c: &Collection, session: Option<SessionId>, keys: Vec<Key>) -> OpResult<CloudHold> {
    c.run(OpSpec::named("cloud_barrier", OpSource::CloudIncoming), move |ctx| {
        ctx.begin_cloud_hold(session, &keys)
    })
    .unwrap()
}

/// A real journaled row change: targets come from the journal, not a test
/// report or a helper that constructs the expected undo state.
fn incoming_card(c: &Collection, session: Option<SessionId>, id: CardId) -> OpResult<()> {
    let mut spec = OpSpec::named("cloud_card", OpSource::CloudIncoming);
    if let Some(session) = session {
        spec = spec.session(session);
    }
    c.run(spec, move |ctx| {
        let mut card = ctx.scope().card(id)?.unwrap();
        card.due += 100;
        card.mtime = EpochSecs(123);
        card.usn = Usn::PENDING;
        ctx.scope().put(&card)?;
        Ok(())
    })
    .unwrap()
}

#[test]
fn only_named_steps_outside_the_newest_answer_run_are_pending_then_acked() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, c1) = steps::setup_note(c, "first");
    let (_, c2) = steps::setup_note(c, "second");
    let a1 = steps::answer(c, Some(S1), c1).unwrap();
    let a2 = steps::answer(c, Some(S1), c2).unwrap();
    let held = barrier(
        c,
        Some(S1),
        vec![
            key(Table::Cards, c1.0),
            key(Table::Revlog, a1.value.0),
            key(Table::Cards, c2.0),
            key(Table::Notes, -77),
        ],
    );
    assert_eq!(held.value.snapshot, a2.seq);
    assert_eq!(ids(&held.value.run), [a2.seq.0]);
    assert_eq!(
        held.value.flagged,
        BTreeMap::from([(
            EntryId(a1.seq.0),
            BTreeSet::from([key(Table::Cards, c1.0), key(Table::Revlog, a1.value.0)]),
        )])
    );
    assert_eq!(c.undo(UndoScope::Session(S1)).unwrap().value.id, EntryId(a2.seq.0));
    assert_eq!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    );
    let before = steps::dump(c);
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::UploadPending);
    assert_eq!(steps::dump(c), before, "a pending refusal writes nothing");

    // Empty ACKs, unknown ids, and an id no longer on undo do not ACK A1.
    c.mark_cloud_uploaded(&[]).unwrap();
    c.mark_cloud_uploaded(&[EntryId(u64::MAX), EntryId(a2.seq.0)]).unwrap();
    assert_eq!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    );
    // A second barrier cannot implicitly ACK an earlier Cloud hold.
    let repeated = barrier(c, None, vec![key(Table::Cards, c1.0)]);
    assert!(repeated.value.flagged.is_empty());
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::UploadPending);
    c.mark_cloud_uploaded(&[EntryId(a1.seq.0)]).unwrap();
    c.mark_cloud_uploaded(&[EntryId(a1.seq.0)]).unwrap();
    assert_eq!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::AlreadyUploaded)
    );
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::AlreadyUploaded);
    let reader = c.reader(ReaderRole::Query).unwrap();
    assert!(reader.revlog_entry(a1.value).unwrap().is_some());
    assert!(reader.revlog_entry(a2.value).unwrap().is_none());
}

#[test]
fn empty_and_unrelated_keys_do_not_flag_but_a_later_matching_barrier_does() {
    let h = Harness::open();
    let c = &h.collection;
    let (nid, cid) = steps::setup_note(c, "original");
    let edit = steps::edit(c, None, nid, "edited").unwrap();
    let empty = barrier(c, None, Vec::new());
    assert!(empty.value.flagged.is_empty());
    assert!(empty.value.run.is_empty());
    // Card and note keys remain distinct for Cloud matching.
    let unrelated = barrier(c, None, vec![key(Table::Cards, cid.0)]);
    assert!(unrelated.value.flagged.is_empty());
    c.mark_cloud_uploaded(&[EntryId(edit.seq.0)]).unwrap(); // local is not pending
    match c.peek_undo(UndoScope::Global).unwrap() {
        UndoStatus::Available(step) => assert_eq!(step.id, EntryId(edit.seq.0)),
        other => panic!("unnamed edit must stay undoable: {other:?}"),
    }
    assert_eq!(c.undo(UndoScope::Global).unwrap().value.id, EntryId(edit.seq.0));
    assert_eq!(common::note_fields(c, nid), "original\u{1f}back");
    let redone = c.redo(UndoScope::Global).unwrap();
    let held = barrier(c, None, vec![key(Table::Notes, nid.0)]);
    assert_eq!(
        held.value.flagged,
        BTreeMap::from([(EntryId(redone.seq.0), BTreeSet::from([key(Table::Notes, nid.0)]),)])
    );
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::UploadPending);
}

#[test]
fn browse_steps_above_the_answer_are_held_and_remain_undoable() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, c1) = steps::setup_note(c, "first");
    let (n2, c2) = steps::setup_note(c, "second");
    let a1 = steps::answer(c, Some(S1), c1).unwrap();
    let a2 = steps::answer(c, Some(S1), c2).unwrap();
    let browse = steps::edit(c, None, n2, "browse").unwrap();
    let held = barrier(
        c,
        Some(S1),
        vec![
            key(Table::Cards, c1.0),
            key(Table::Cards, c2.0),
            key(Table::Notes, n2.0),
        ],
    );
    assert_eq!(ids(&held.value.run), [browse.seq.0, a2.seq.0]);
    assert_eq!(
        held.value.flagged,
        BTreeMap::from([(EntryId(a1.seq.0), BTreeSet::from([key(Table::Cards, c1.0)]),)])
    );
    assert_eq!(ids(&c.cloud_held_run(Some(S1)).unwrap()), [browse.seq.0, a2.seq.0]);
    assert_eq!(c.undo(UndoScope::Global).unwrap().value.id, EntryId(browse.seq.0));
    assert_eq!(c.undo(UndoScope::Session(S1)).unwrap().value.id, EntryId(a2.seq.0));
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::UploadPending);
}

#[test]
fn a_different_session_above_the_answer_or_no_answer_makes_the_cloud_run_empty() {
    let h = Harness::open();
    let c = &h.collection;
    let (nid, cid) = steps::setup_note(c, "a");
    assert!(c.cloud_held_run(Some(S1)).unwrap().is_empty());
    steps::edit(c, None, nid, "before answer").unwrap();
    assert!(c.cloud_held_run(Some(S1)).unwrap().is_empty());
    let answer = steps::answer(c, Some(S1), cid).unwrap();
    assert!(c.cloud_held_run(None).unwrap().is_empty());
    let other = steps::answer(c, Some(S2), cid).unwrap();
    assert!(c.cloud_held_run(Some(S1)).unwrap().is_empty());
    assert_eq!(ids(&c.cloud_held_run(Some(S2)).unwrap()), [other.seq.0]);
    let held = barrier(c, Some(S1), vec![key(Table::Cards, cid.0)]);
    assert!(held.value.run.is_empty());
    assert_eq!(
        held.value.flagged.keys().copied().collect::<Vec<_>>(),
        [EntryId(answer.seq.0), EntryId(other.seq.0)]
    );
    assert!(c.cloud_held_run(Some(S2)).unwrap().is_empty(), "pending stops the run");
    c.mark_cloud_uploaded(&[EntryId(other.seq.0)]).unwrap();
    assert!(c.cloud_held_run(Some(S2)).unwrap().is_empty(), "uploaded stops it too");
}

#[test]
fn undo_submitted_while_the_barrier_holds_the_writer_resolves_after_its_flags() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, c1) = steps::setup_note(c, "first");
    let (_, c2) = steps::setup_note(c, "second");
    let a1 = steps::answer(c, Some(S1), c1).unwrap();
    let a2 = steps::answer(c, Some(S1), c2).unwrap();
    let first = barrier(c, Some(S1), vec![key(Table::Cards, c1.0)]);
    assert_eq!(ids(&first.value.run), [a2.seq.0]);
    c.undo(UndoScope::Session(S1)).unwrap();
    c.mark_cloud_uploaded(&[EntryId(a1.seq.0)]).unwrap();
    // A2 is now the local top step again, with the redo's new step id.
    let redone = c.redo(UndoScope::Global).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let hold = c.submit(OpSpec::named("cloud_barrier", OpSource::CloudIncoming), move |ctx| {
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        // The session has ended. The top that was available at submit is
        // pending by the time the writer takes the queued undo.
        ctx.begin_cloud_hold(None, &[key(Table::Cards, c2.0)])
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let visible = c.peek_undo(UndoScope::Global).unwrap();
    let undo = c.submit_undo(UndoScope::Global);
    release_tx.send(()).unwrap();
    let held = hold.wait().unwrap();
    assert!(matches!(visible, UndoStatus::Available(ref step) if step.id == EntryId(redone.seq.0)));
    assert_eq!(
        held.value.flagged,
        BTreeMap::from([(EntryId(redone.seq.0), BTreeSet::from([key(Table::Cards, c2.0)]),)])
    );
    let before = steps::dump(c);
    assert_eq!(refused(undo.wait()), DisabledReason::UploadPending);
    assert_eq!(steps::dump(c), before);
    assert!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .revlog_entry(a2.value)
            .unwrap()
            .is_some()
    );
}

#[test]
fn barrier_drops_only_named_redos_and_keeps_the_others_in_order() {
    let h = Harness::open();
    let c = &h.collection;
    let notes: Vec<_> = ["a", "b", "c"].iter().map(|s| steps::setup_note(c, s).0).collect();
    for nid in &notes {
        steps::edit(c, None, *nid, "edited").unwrap();
    }
    for _ in 0..3 {
        c.undo(UndoScope::Global).unwrap();
    }
    let held = barrier(c, None, vec![key(Table::Notes, notes[1].0)]);
    assert!(held.value.flagged.is_empty(), "redo is dropped, never flagged");
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(common::note_fields(c, notes[0]), "edited\u{1f}back");
    assert_eq!(common::note_fields(c, notes[1]), "b\u{1f}back");
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(common::note_fields(c, notes[2]), "edited\u{1f}back");
    assert_eq!(refused(c.redo(UndoScope::Global)), DisabledReason::None);
}

#[test]
fn a_barrier_naming_an_undone_answers_deleted_revlog_prevents_its_reinsertion() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, cid) = steps::setup_note(c, "a");
    let answer = steps::answer(c, Some(S1), cid).unwrap();
    c.undo(UndoScope::Global).unwrap();
    let held = barrier(c, Some(S1), vec![key(Table::Revlog, answer.value.0)]);
    assert!(held.value.flagged.is_empty());
    assert_eq!(refused(c.redo(UndoScope::Global)), DisabledReason::None);
    assert!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .revlog_entry(answer.value)
            .unwrap()
            .is_none()
    );
}

#[test]
fn incoming_below_the_run_keeps_the_intervening_step_and_the_whole_run() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, old_card) = steps::setup_note(c, "old");
    let (middle_note, _) = steps::setup_note(c, "middle");
    let (browse_note, active_card) = steps::setup_note(c, "active");
    steps::bury(c, None, old_card).unwrap();
    let middle = steps::edit(c, None, middle_note, "middle edited").unwrap();
    let answer = steps::answer(c, Some(S1), active_card).unwrap();
    let browse = steps::edit(c, None, browse_note, "browse").unwrap();
    let applied = incoming_card(c, Some(S1), old_card);
    assert!(applied.cloud_detached_run.is_none());
    assert_eq!(ids(&c.cloud_held_run(Some(S1)).unwrap()), [browse.seq.0, answer.seq.0]);
    for expected in [browse.seq, answer.seq, middle.seq] {
        assert_eq!(c.undo(UndoScope::Global).unwrap().value.id, EntryId(expected.0));
    }
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::ClearedBySync);
}

#[test]
fn incoming_in_the_answer_detaches_browse_too_and_keeps_journal_before_images_alive() {
    let h = Harness::open();
    let c = &h.collection;
    let (old_note, _) = steps::setup_note(c, "old");
    let (nid, cid) = steps::setup_note(c, "active");
    steps::edit(c, None, old_note, "older step").unwrap();
    let (original_card, original_note) = {
        let reader = c.reader(ReaderRole::Query).unwrap();
        (
            reader.card(cid).unwrap().unwrap().to_cells(),
            reader.note(nid).unwrap().unwrap().to_cells(),
        )
    };
    let answer = steps::answer(c, Some(S1), cid).unwrap();
    steps::bury(c, Some(S1), cid).unwrap();
    let browse = steps::edit(c, None, nid, "browse above answer").unwrap();
    let applied = incoming_card(c, Some(S1), cid);
    let detached = applied.cloud_detached_run.expect("the targeted run is returned");
    assert_eq!(detached.steps.len(), 3);
    assert_eq!(detached.steps[0].id, EntryId(browse.seq.0));
    assert_eq!(detached.steps[2].id, EntryId(answer.seq.0));
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::ClearedBySync);
    assert!(c.cloud_held_run(Some(S1)).unwrap().is_empty());
    // Read only after after_commit dropped the steps. Both repeated card
    // touches use the oldest image; the newly inserted revlog was absent.
    assert_eq!(
        detached
            .before(Table::Cards, &RowKey::Id(cid.0))
            .unwrap()
            .unwrap()
            .before,
        Some(original_card)
    );
    assert_eq!(
        detached
            .before(Table::Notes, &RowKey::Id(nid.0))
            .unwrap()
            .unwrap()
            .before,
        Some(original_note)
    );
    assert_eq!(
        detached
            .before(Table::Revlog, &RowKey::Id(answer.value.0))
            .unwrap()
            .unwrap()
            .before,
        None
    );
    let rows = detached.rows().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.table != Table::Col));
    assert!(detached.before(Table::Col, &RowKey::Id(1)).unwrap().is_none());
    assert_eq!(common::note_fields(c, nid), "browse above answer\u{1f}back");
}

#[test]
fn targeting_a_card_move_drops_its_ancestor_deck_creation_too() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, cid) = steps::setup_note(c, "a");
    let created = c
        .run(
            OpSpec::named("create_deck", OpSource::UserUndoable).undo_step("Create Deck", StepKind::Other),
            |ctx| {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                deck.name = "d2".to_owned();
                Ok(ctx.scope().add_deck(&deck)?)
            },
        )
        .unwrap();
    let d2 = created.value;
    let moved = c
        .run(
            OpSpec::named("move_card", OpSource::UserUndoable).undo_step("Move Card", StepKind::Other),
            move |ctx| {
                let mut card = ctx.scope().card(cid)?.unwrap();
                card.deck_id = d2;
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    let held = barrier(c, None, vec![key(Table::Cards, cid.0)]);
    assert_eq!(
        held.value.flagged,
        BTreeMap::from([(EntryId(moved.seq.0), BTreeSet::from([key(Table::Cards, cid.0)]),)])
    );
    let applied = incoming_card(c, None, cid);
    assert!(applied.cloud_detached_run.is_none());
    assert_eq!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::ClearedBySync)
    );
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::ClearedBySync);
    c.mark_cloud_uploaded(&[EntryId(created.seq.0), EntryId(moved.seq.0)])
        .unwrap();
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::ClearedBySync);
    let reader = c.reader(ReaderRole::Query).unwrap();
    assert!(reader.deck(d2).unwrap().is_some(), "no surviving W may delete d2");
    assert_eq!(reader.card(cid).unwrap().unwrap().deck_id, d2);
}

#[test]
fn incoming_cuts_redo_below_its_nearest_target_and_keeps_redos_above() {
    let h = Harness::open();
    let c = &h.collection;
    let notes: Vec<_> = ["a", "b", "c"].iter().map(|s| steps::setup_note(c, s).0).collect();
    for nid in &notes {
        steps::edit(c, None, *nid, "edited").unwrap();
    }
    for _ in 0..3 {
        c.undo(UndoScope::Global).unwrap();
    }
    let target = notes[1];
    c.run(OpSpec::named("cloud_note", OpSource::CloudIncoming), move |ctx| {
        steps::set_first_field(ctx, target, "incoming".to_owned())
    })
    .unwrap();
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(common::note_fields(c, notes[0]), "edited\u{1f}back");
    assert_eq!(common::note_fields(c, notes[1]), "incoming\u{1f}back");
    assert_eq!(common::note_fields(c, notes[2]), "c\u{1f}back");
    assert_eq!(refused(c.redo(UndoScope::Global)), DisabledReason::None);
}

#[test]
fn a_grave_targets_the_deleted_object_even_when_that_object_is_already_absent() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, cid) = steps::setup_note(c, "a");
    c.run(OpSpec::named("delete_card", OpSource::UserUndoable), move |ctx| {
        ctx.scope().remove_card(cid)?;
        Ok(())
    })
    .unwrap();
    c.run(OpSpec::named("cloud_grave", OpSource::CloudIncoming), move |ctx| {
        ctx.scope()
            .add_grave(cid.0, meguri_model::grave::GraveKind::Card, Usn::PENDING)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::ClearedBySync);
    assert!(c.reader(ReaderRole::Query).unwrap().card(cid).unwrap().is_none());
}

#[test]
fn rekey_targets_old_new_and_rewritten_reference_keys_independently() {
    for selection in ["previous", "replacement", "link"] {
        let h = Harness::open();
        let c = &h.collection;
        let (previous, card_id) = steps::setup_note(c, "item");
        let (earlier_note, _) = steps::setup_note(c, "prior");
        let (later_note, _) = steps::setup_note(c, "remaining");
        let replacement = NoteId(previous.0 + 100);
        if selection == "replacement" {
            c.run(
                OpSpec::named("prepare_spare_row", OpSource::UserNonUndoable).skip_undo(),
                move |ctx| {
                    let mut note = common::note(&["scratch", "back"]);
                    note.id = replacement;
                    ctx.scope().put(&note)?;
                    Ok(())
                },
            )
            .unwrap();
        }
        steps::edit(c, None, earlier_note, "prior edit").unwrap();
        match selection {
            "previous" => {
                steps::edit(c, None, previous, "item changed").unwrap();
            }
            "replacement" => {
                c.run(OpSpec::named("remove_spare_row", OpSource::UserUndoable), move |ctx| {
                    ctx.scope().remove_note(replacement)?;
                    Ok(())
                })
                .unwrap();
            }
            "link" => {
                steps::bury(c, None, card_id).unwrap();
            }
            _ => unreachable!(),
        }
        let remaining = steps::edit(c, None, later_note, "remaining changed").unwrap();
        let result = c
            .run(OpSpec::named("shift_note_key", OpSource::CloudRekey), move |ctx| {
                let mut note = ctx.scope().note(previous)?.unwrap();
                ctx.scope().remove_note(previous)?;
                note.id = replacement;
                ctx.scope().put(&note)?;
                let mut card = ctx.scope().card(card_id)?.unwrap();
                card.note_id = replacement;
                ctx.scope().put(&card)?;
                Ok(())
            })
            .unwrap();
        assert!(result.cloud_detached_run.is_none());
        assert_eq!(
            c.undo(UndoScope::Global).unwrap().value.id,
            EntryId(remaining.seq.0),
            "{selection}"
        );
        assert_eq!(
            refused(c.undo(UndoScope::Global)),
            DisabledReason::ClearedBySync,
            "{selection}"
        );
        let query = c.reader(ReaderRole::Query).unwrap();
        assert!(query.note(previous).unwrap().is_none());
        assert!(query.note(replacement).unwrap().is_some());
        assert_eq!(query.card(card_id).unwrap().unwrap().note_id, replacement);
    }
}

#[test]
fn no_row_barriers_and_capture_take_new_sequences_without_mod_or_events() {
    let h = Harness::open();
    let c = &h.collection;
    let (nid, _) = steps::setup_note(c, "a");
    let (other, _) = steps::setup_note(c, "unnamed");
    let edit = steps::edit(c, None, nid, "edited").unwrap();
    let unnamed = steps::edit(c, None, other, "unnamed edited").unwrap();
    // Settle any queued open-time rollover before measuring O4's no-op seqs.
    c.run(OpSpec::named("settle", OpSource::Rollover), |_| Ok(())).unwrap();
    let before = steps::dump(c);
    let seq = c.last_seq();
    let events = h.engine.stream().subscribe();
    h.clock.step_ms(2_000);
    let held = barrier(c, None, vec![key(Table::Notes, nid.0)]);
    assert_eq!(held.value.snapshot, seq);
    assert_eq!(held.seq.0, seq.0 + 1);
    assert_eq!(
        held.value.flagged,
        BTreeMap::from([(EntryId(edit.seq.0), BTreeSet::from([key(Table::Notes, nid.0)]),)])
    );
    assert!(held.changes.is_empty());
    assert!(held.undo.is_none());
    let capture = c
        .run(OpSpec::named("cloud_capture", OpSource::CloudIncoming), |_| Ok(()))
        .unwrap();
    assert_eq!(capture.seq.0, held.seq.0 + 1);
    assert!(capture.changes.is_empty());
    assert!(capture.cloud_detached_run.is_none());
    assert_eq!(steps::dump(c), before);
    assert!(events.try_recv().is_none());
    assert_eq!(
        c.undo(UndoScope::Global).unwrap().value.id,
        EntryId(unnamed.seq.0),
        "capture must not flag the unnamed step"
    );
    assert_eq!(refused(c.undo(UndoScope::Global)), DisabledReason::UploadPending);
}

#[derive(Clone, Debug)]
struct CloudModelStep {
    id: u64,
    origin: u64,
    session: Option<SessionId>,
    answer: bool,
    keys: BTreeSet<Key>,
    // 0 = local, 1 = pending, 2 = uploaded. No tracking/server is modeled.
    upload: u8,
    review: Option<(RevlogId, bool)>,
}

#[derive(Debug)]
enum CloudAction {
    Answer(usize, Option<SessionId>),
    Edit(usize, Option<SessionId>),
    Restore(bool),
    Barrier(Option<SessionId>, Vec<Key>),
    Ack(Vec<EntryId>),
    Incoming(usize, Option<SessionId>),
    Rekey(usize, Option<SessionId>),
}

struct CloudWalk {
    h: Harness,
    objects: Vec<(NoteId, CardId)>,
    undo: Vec<CloudModelStep>,
    redo: Vec<CloudModelStep>,
    floor: DisabledReason,
    cut_origins: BTreeSet<u64>,
    reviews: Vec<RevlogId>,
    present: BTreeSet<RevlogId>,
    protected_deletions: BTreeSet<RevlogId>,
    log: Vec<String>,
    // Actions; pending/uploaded refusals; protected held steps; unrelated
    // ancestors cut; deletion redos dropped; detached runs for each source.
    coverage: [usize; 15],
}

impl CloudWalk {
    // Pure history model: no engine policy, peek, or held-run helper supplies
    // any expected flag, cut, refusal, or row-presence decision below.
    fn held(&self, session: Option<SessionId>) -> Vec<u64> {
        let Some(session) = session else { return Vec::new() };
        let mut run = Vec::new();
        for step in self.undo.iter().rev() {
            if step.upload != 0 || step.session.is_some_and(|s| s != session) {
                break;
            }
            run.push(step.id);
            if step.answer && step.session == Some(session) {
                return run;
            }
        }
        Vec::new()
    }

    fn status(&self, redo: bool, scope: UndoScope) -> Result<u64, DisabledReason> {
        let stack = if redo { &self.redo } else { &self.undo };
        let Some(step) = stack.last() else {
            return Err(if redo { DisabledReason::None } else { self.floor });
        };
        if let UndoScope::Session(session) = scope
            && step.session != Some(session)
        {
            return Err(DisabledReason::None);
        }
        match step.upload {
            0 => Ok(step.id),
            1 => Err(DisabledReason::UploadPending),
            2 => Err(DisabledReason::AlreadyUploaded),
            _ => unreachable!(),
        }
    }

    fn record(&mut self, id: u64, session: Option<SessionId>, keys: BTreeSet<Key>, review: Option<RevlogId>) {
        self.redo.clear();
        self.undo.push(CloudModelStep {
            id,
            origin: id,
            session,
            answer: review.is_some(),
            keys,
            upload: 0,
            review: review.map(|id| (id, true)),
        });
        if self.undo.len() > 30 {
            self.undo.remove(0);
            self.floor = DisabledReason::None;
        }
    }

    fn apply(&mut self, action: CloudAction) {
        self.log.push(format!("{action:?}"));
        // Keep edits and restored edits outside A3's merge window. The test
        // clock advances; no wall time, retries, or scheduling is involved.
        self.h.clock.step_ms(61_000);
        let c = self.h.collection.clone();
        match action {
            CloudAction::Answer(at, session) => {
                self.coverage[0] += 1;
                let card = self.objects[at].1;
                let done = steps::answer(&c, session, card).unwrap();
                self.reviews.push(done.value);
                self.present.insert(done.value);
                self.record(
                    done.seq.0,
                    session,
                    BTreeSet::from([key(Table::Cards, card.0), key(Table::Revlog, done.value.0)]),
                    Some(done.value),
                );
            }
            CloudAction::Edit(at, session) => {
                self.coverage[1] += 1;
                let note = self.objects[at].0;
                let done = steps::edit(&c, session, note, &format!("field-{}", self.log.len())).unwrap();
                self.record(done.seq.0, session, BTreeSet::from([key(Table::Notes, note.0)]), None);
            }
            CloudAction::Restore(redo) => {
                self.coverage[2 + usize::from(redo)] += 1;
                let expected = self.status(redo, UndoScope::Global);
                let before = steps::dump(&c);
                let actual = if redo {
                    c.redo(UndoScope::Global)
                } else {
                    c.undo(UndoScope::Global)
                };
                match expected {
                    Err(reason) => {
                        assert_eq!(refused(actual), reason, "{:?}", self.log);
                        assert_eq!(steps::dump(&c), before, "refusal wrote: {:?}", self.log);
                        if reason == DisabledReason::UploadPending {
                            self.coverage[8] += 1;
                        }
                        if reason == DisabledReason::AlreadyUploaded {
                            self.coverage[9] += 1;
                        }
                    }
                    Ok(id) => {
                        let done = actual.unwrap();
                        assert_eq!(done.value.id, EntryId(id), "{:?}", self.log);
                        let mut step = if redo { self.redo.pop() } else { self.undo.pop() }.unwrap();
                        assert_eq!(step.upload, 0);
                        assert!(!self.cut_origins.contains(&step.origin), "cut ancestor restored");
                        if let Some((review, present)) = &mut step.review {
                            *present = !*present;
                            if *present {
                                assert!(!self.protected_deletions.contains(review), "deletion restored");
                                self.present.insert(*review);
                            } else {
                                self.present.remove(review);
                            }
                        }
                        step.id = done.seq.0;
                        if redo {
                            self.undo.push(step);
                        } else {
                            self.redo.push(step);
                        }
                    }
                }
            }
            CloudAction::Barrier(session, keys) => {
                self.coverage[4] += 1;
                let run = self.held(session);
                let mut flagged = BTreeMap::new();
                for step in &mut self.undo {
                    let overlap: BTreeSet<_> = keys.iter().filter(|key| step.keys.contains(*key)).cloned().collect();
                    if step.upload == 0 && !run.contains(&step.id) && !overlap.is_empty() {
                        step.upload = 1;
                        flagged.insert(EntryId(step.id), overlap);
                    }
                }
                self.coverage[10] += run.len();
                for review in &self.reviews {
                    if !self.present.contains(review) && keys.contains(&key(Table::Revlog, review.0)) {
                        self.protected_deletions.insert(*review);
                    }
                }
                self.redo.retain(|step| {
                    let keep = !keys.iter().any(|key| step.keys.contains(key));
                    if !keep && step.review.is_some_and(|(_, present)| !present) {
                        self.coverage[12] += 1;
                    }
                    keep
                });
                let snapshot = c.last_seq();
                let before = steps::dump(&c);
                let actual = barrier(&c, session, keys);
                assert_eq!(actual.value.snapshot, snapshot, "{:?}", self.log);
                assert_eq!(ids(&actual.value.run), run, "{:?}", self.log);
                assert_eq!(actual.value.flagged, flagged, "{:?}", self.log);
                assert!(actual.value.flagged.keys().all(|id| !run.contains(&id.0)));
                assert_eq!(steps::dump(&c), before, "barrier wrote rows");
            }
            CloudAction::Ack(ids) => {
                self.coverage[5] += 1;
                let before = steps::dump(&c);
                c.mark_cloud_uploaded(&ids).unwrap();
                for step in &mut self.undo {
                    if step.upload == 1 && ids.contains(&EntryId(step.id)) {
                        step.upload = 2;
                    }
                }
                assert_eq!(steps::dump(&c), before, "ACK wrote rows");
            }
            CloudAction::Incoming(at, session) | CloudAction::Rekey(at, session) => {
                let rekey = matches!(action, CloudAction::Rekey(..));
                self.coverage[6 + usize::from(rekey)] += 1;
                let (note, card) = self.objects[at];
                let replacement = NoteId(note.0 + 1_000_000);
                let targets = if rekey {
                    BTreeSet::from([
                        key(Table::Notes, note.0),
                        key(Table::Notes, replacement.0),
                        key(Table::Cards, card.0),
                    ])
                } else {
                    BTreeSet::from([key(Table::Cards, card.0)])
                };
                let reaches = |step: &CloudModelStep| !step.keys.is_disjoint(&targets);
                let run = self.held(session);
                let detached = self.undo.iter().any(|step| run.contains(&step.id) && reaches(step));
                let cut = if detached {
                    self.undo.len().checked_sub(1)
                } else {
                    self.undo.iter().rposition(reaches)
                };
                if let Some(cut) = cut {
                    for step in self.undo.drain(..=cut) {
                        self.coverage[11] += usize::from(!reaches(&step));
                        self.cut_origins.insert(step.origin);
                    }
                    self.floor = DisabledReason::ClearedBySync;
                }
                if let Some(cut) = self.redo.iter().rposition(reaches) {
                    self.cut_origins.extend(self.redo.drain(..=cut).map(|step| step.origin));
                }
                let actual = if rekey {
                    let mut spec = OpSpec::named("cloud_rekey", OpSource::CloudRekey);
                    if let Some(session) = session {
                        spec = spec.session(session);
                    }
                    let done = c
                        .run(spec, move |ctx| {
                            let mut stored = ctx.scope().note(note)?.unwrap();
                            ctx.scope().remove_note(note)?;
                            stored.id = replacement;
                            ctx.scope().put(&stored)?;
                            let mut stored = ctx.scope().card(card)?.unwrap();
                            stored.note_id = replacement;
                            ctx.scope().put(&stored)?;
                            Ok(())
                        })
                        .unwrap();
                    self.objects[at].0 = replacement;
                    done
                } else {
                    incoming_card(&c, session, card)
                };
                assert_eq!(actual.cloud_detached_run.is_some(), detached, "{:?}", self.log);
                if let Some(value) = actual.cloud_detached_run {
                    self.coverage[13 + usize::from(rekey)] += 1;
                    assert_eq!(ids(&value), run, "{:?}", self.log);
                    // Force the retained journals to read after the cut.
                    let rows = value.rows().collect::<Result<Vec<_>, _>>().unwrap();
                    assert!(rows.iter().all(|row| row.table != Table::Col));
                    assert!(!rows.is_empty());
                }
            }
        }
        for redo in [false, true] {
            for scope in [UndoScope::Global, UndoScope::Session(S1), UndoScope::Session(S2)] {
                let status = if redo { c.peek_redo(scope) } else { c.peek_undo(scope) }.unwrap();
                let actual = match status {
                    UndoStatus::Available(step) => Ok(step.id.0),
                    UndoStatus::Unavailable(reason) => Err(reason),
                };
                assert_eq!(actual, self.status(redo, scope), "{:?}", self.log);
            }
        }
        for session in [None, Some(S1), Some(S2)] {
            assert_eq!(
                ids(&c.cloud_held_run(session).unwrap()),
                self.held(session),
                "{:?}",
                self.log
            );
        }
        let reader = c.reader(ReaderRole::Query).unwrap();
        for review in &self.reviews {
            assert_eq!(
                reader.revlog_entry(*review).unwrap().is_some(),
                self.present.contains(review),
                "{:?}",
                self.log
            );
        }
        for review in &self.protected_deletions {
            assert!(
                reader.revlog_entry(*review).unwrap().is_none(),
                "deletion restored: {:?}",
                self.log
            );
        }
    }
}

#[test]
fn seeded_mixed_cloud_histories_preserve_undo_safety() {
    for seed in 1..=16 {
        let rng = SeededRandom::new(seed);
        let pick = |n: usize| (rng.next_u64() % n as u64) as usize;
        let h = Harness::open();
        let mut objects: Vec<_> = (0..3)
            .map(|i| steps::setup_note(&h.collection, &format!("row-{i}")))
            .collect();
        objects.rotate_left(pick(3));
        h.collection
            .run(OpSpec::named("settle", OpSource::Rollover), |_| Ok(()))
            .unwrap();
        let mut walk = CloudWalk {
            h,
            objects,
            undo: Vec::new(),
            redo: Vec::new(),
            floor: DisabledReason::None,
            cut_origins: BTreeSet::new(),
            reviews: Vec::new(),
            present: BTreeSet::new(),
            protected_deletions: BTreeSet::new(),
            log: vec![format!("seed {seed}")],
            coverage: [0; 15],
        };
        for action in [
            CloudAction::Edit(2, None),
            CloudAction::Answer(0, Some(S1)),
            CloudAction::Answer(1, Some(S1)),
            CloudAction::Edit(2, None),
            CloudAction::Barrier(
                Some(S1),
                vec![
                    key(Table::Cards, walk.objects[0].1.0),
                    key(Table::Cards, walk.objects[1].1.0),
                    key(Table::Notes, walk.objects[2].0.0),
                ],
            ),
            CloudAction::Restore(false),
            CloudAction::Restore(false),
            CloudAction::Restore(false),
        ] {
            walk.apply(action);
        }
        walk.apply(CloudAction::Ack(
            walk.undo.iter().map(|step| EntryId(step.id)).collect(),
        ));
        for action in [
            CloudAction::Restore(false),
            CloudAction::Incoming(0, Some(S1)),
            CloudAction::Restore(true),
            CloudAction::Restore(false),
        ] {
            walk.apply(action);
        }
        walk.apply(CloudAction::Barrier(
            Some(S1),
            vec![key(Table::Revlog, walk.reviews[1].0)],
        ));
        for action in [
            CloudAction::Restore(true),
            CloudAction::Rekey(2, Some(S1)),
            CloudAction::Answer(0, Some(S1)),
            CloudAction::Edit(2, None),
            CloudAction::Incoming(0, Some(S1)),
            CloudAction::Answer(1, Some(S1)),
            CloudAction::Edit(2, None),
            CloudAction::Rekey(1, Some(S1)),
        ] {
            walk.apply(action);
        }
        for _ in 0..64 {
            let at = pick(3);
            let session = [None, Some(S1), Some(S2)][pick(3)];
            let action = match pick(8) {
                0 => CloudAction::Answer(at, session),
                1 => CloudAction::Edit(at, session),
                2 => CloudAction::Restore(false),
                3 => CloudAction::Restore(true),
                4 => {
                    let (note, card) = walk.objects[at];
                    let keys = match pick(4) {
                        0 => Vec::new(),
                        1 => vec![key(Table::Notes, note.0)],
                        2 => vec![key(Table::Cards, card.0)],
                        _ => vec![key(Table::Revlog, walk.reviews[pick(walk.reviews.len())].0)],
                    };
                    CloudAction::Barrier(session, keys)
                }
                5 => CloudAction::Ack(
                    walk.undo
                        .iter()
                        .filter(|_| pick(2) == 0)
                        .map(|step| EntryId(step.id))
                        .chain([EntryId(u64::MAX)])
                        .collect(),
                ),
                6 => CloudAction::Incoming(at, session),
                _ => CloudAction::Rekey(at, session),
            };
            walk.apply(action);
        }
        assert_eq!(walk.coverage[..8].iter().sum::<usize>(), 86);
        assert!(
            walk.coverage.iter().all(|count| *count > 0),
            "seed {seed}: {:?}",
            walk.coverage
        );
    }
}
