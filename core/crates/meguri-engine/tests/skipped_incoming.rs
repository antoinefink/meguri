//! S1.1 diagnostics travel with committed results, without becoming mutations.
mod common;

use common::{Harness, add_note_with_card};
use meguri_engine::{ChangeObserver, EngineError, EventKind, ObservedCommit, OpSpec, Ports, UndoScope};
use meguri_model::{
    changes::{ChangeSet, OpSource},
    ids::{CardId, DeckId, NoteId},
};
use meguri_storage::{ReadRows, ReaderRole, Record, StampMode};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Recorder(Mutex<Vec<(meguri_engine::Seq, meguri_engine::Generation, ChangeSet)>>);
impl ChangeObserver for Recorder {
    fn name(&self) -> &str {
        "skipped incoming recorder"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        self.0
            .lock()
            .unwrap()
            .push((commit.seq, commit.generation, commit.changes.clone()));
        Ok(())
    }
}
fn report(ctx: &mut meguri_engine::OpContext<'_, '_>) {
    ctx.report_skipped_incoming(
        [CardId(90), CardId(-2), CardId(90), CardId(10)],
        [NoteId(10), NoteId(-2), NoteId(10)],
        [DeckId(90), DeckId(-2), DeckId(90)],
    );
    ctx.report_skipped_incoming([CardId(10)], [NoteId(-2)], [DeckId(-2)]);
}
fn exact(changes: &ChangeSet) {
    assert_eq!(changes.skipped_incoming_cards, [CardId(-2), CardId(10), CardId(90)]);
    assert_eq!(changes.skipped_incoming_notes, [NoteId(-2), NoteId(10)]);
    assert_eq!(changes.skipped_incoming_decks, [DeckId(-2), DeckId(90)]);
}
fn barrier(h: &Harness) -> meguri_engine::OpResult<()> {
    h.collection
        .run(
            OpSpec::named("settle rollover", OpSource::Rollover).stamp(StampMode::Raw),
            |_| Ok(()),
        )
        .unwrap()
}

#[test]
fn diagnostic_only_results_preserve_stamps_history_source_effects_and_one_seq() {
    for source in [
        OpSource::UserUndoable,
        OpSource::UserNonUndoable,
        OpSource::SyncIncoming,
        OpSource::LocalApi,
        OpSource::Optimizer,
        OpSource::Rollover,
        OpSource::Maintenance,
        OpSource::CloudIncoming,
        OpSource::CloudRekey,
    ] {
        let recorder = Arc::new(Recorder::default());
        let h = Harness::open_with(Ports::default().observer(recorder.clone()));
        add_note_with_card(&h.collection, OpSource::UserUndoable, &["redo", "back"]);
        h.collection.undo(UndoScope::Global).unwrap();
        let previous = barrier(&h);
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let redo = h.collection.peek_redo(UndoScope::Global).unwrap();
        let col = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
        recorder.0.lock().unwrap().clear();
        let events = h.engine.stream().subscribe();
        h.clock.step_ms(1000);
        let result = h
            .collection
            .run(OpSpec::named("diagnostics only", source), |ctx| {
                report(ctx);
                Ok(())
            })
            .unwrap();
        exact(&result.changes);
        assert!(result.changes.is_empty());
        assert_eq!(result.undo, None);
        assert_eq!(result.seq.0, previous.seq.0 + 1);
        assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().col().unwrap(), col);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        if source == OpSource::UserUndoable {
            assert!(h.collection.peek_redo(UndoScope::Global).unwrap().name().is_none());
        } else {
            assert_eq!(h.collection.peek_redo(UndoScope::Global).unwrap(), redo);
        }
        let next = barrier(&h);
        assert_eq!(next.seq.0, result.seq.0 + 1, "diagnostics reserve no extra sequence");
        assert!(events.try_recv().is_none());
        assert!(recorder.0.lock().unwrap().is_empty());
        assert!(next.changes.skipped_incoming_cards.is_empty());
        assert!(next.changes.skipped_incoming_notes.is_empty());
        assert!(next.changes.skipped_incoming_decks.is_empty());
    }
}

#[test]
fn mutation_observer_event_and_result_share_exact_diagnostics_seq_and_generation() {
    let recorder = Arc::new(Recorder::default());
    let h = Harness::open_with(Ports::default().observer(recorder.clone()));
    let (nid, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["before", "back"]).value;
    barrier(&h);
    recorder.0.lock().unwrap().clear();
    let events = h.engine.stream().subscribe();
    let result = h
        .collection
        .run(
            OpSpec::named("merge with diagnostics", OpSource::SyncIncoming),
            move |ctx| {
                report(ctx);
                let mut row = ctx.scope().card(cid)?.unwrap();
                row.flags = 3;
                ctx.scope().put(&row)?;
                Ok(())
            },
        )
        .unwrap();
    exact(&result.changes);
    assert_eq!(result.changes.cards.len(), 1);
    assert_eq!(result.changes.cards[0].id, cid);
    assert_eq!(result.changes.cards[0].note_id, nid);
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(cid)
            .unwrap()
            .unwrap()
            .flags,
        3
    );
    let event = events.try_recv().expect("one committed Changes event");
    assert_eq!((event.seq, event.generation), (result.seq, result.generation));
    let EventKind::Changes(changes) = &event.kind else {
        panic!("expected Changes")
    };
    assert!(Arc::ptr_eq(changes, &result.changes));
    assert_eq!(
        *recorder.0.lock().unwrap(),
        [(result.seq, result.generation, (*result.changes).clone())]
    );
    assert!(events.try_recv().is_none());
}

#[test]
fn failed_operation_and_failed_savepoint_expose_no_attempted_diagnostics() {
    let recorder = Arc::new(Recorder::default());
    let h = Harness::open_with(Ports::default().observer(recorder.clone()));
    let (_, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["rollback", "back"]).value;
    barrier(&h);
    recorder.0.lock().unwrap().clear();
    let events = h.engine.stream().subscribe();
    let before = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(cid)
        .unwrap()
        .unwrap()
        .to_cells();
    let col = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let fail = move |ctx: &mut meguri_engine::OpContext<'_, '_>| -> Result<(), EngineError> {
        report(ctx);
        let mut row = ctx.scope().card(cid)?.unwrap();
        row.flags = 7;
        ctx.scope().put(&row)?;
        Err(EngineError::InvalidArgument {
            message: "rollback witness".into(),
        })
    };
    assert!(
        h.collection
            .run(OpSpec::named("failed diagnostic merge", OpSource::SyncIncoming), fail)
            .is_err()
    );
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(cid)
            .unwrap()
            .unwrap()
            .to_cells(),
        before
    );
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().col().unwrap(), col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    let clean = h
        .collection
        .run(OpSpec::named("failed diagnostic step", OpSource::SyncIncoming), |ctx| {
            let failed: Result<(), EngineError> = ctx.step(|ctx| {
                report(ctx);
                Err(EngineError::InvalidArgument {
                    message: "savepoint witness".into(),
                })
            });
            assert!(failed.is_err());
            Ok(())
        })
        .unwrap();
    assert!(clean.changes.is_empty());
    assert!(clean.changes.skipped_incoming_cards.is_empty());
    assert!(clean.changes.skipped_incoming_notes.is_empty());
    assert!(clean.changes.skipped_incoming_decks.is_empty());
    assert!(events.try_recv().is_none());
    assert!(recorder.0.lock().unwrap().is_empty());
}
