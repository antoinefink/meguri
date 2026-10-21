//! U37 cancellation and U28 subtree declarations through the existing writer.
#![cfg(feature = "test-hooks")]
mod common;
use common::{Gate, block_writer};
use meguri_engine::{
    CancelToken, Collection, EngineError, OpSource, OpSpec, ProgressSink, UndoScope, UndoStatus, decks, notes,
};
use meguri_storage::{ReadRows, ReaderRole};
use std::{sync::Arc, time::Duration};

struct Release(Gate);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.open();
    }
}
fn queued(c: &Collection, name: &str) {
    let until = meguri_engine::monotonic::now() + Duration::from_secs(5);
    while !c.has_queued_op_for_testing(name) {
        assert!(meguri_engine::monotonic::now() < until, "never queued {name}");
        std::thread::yield_now();
    }
}
fn history(c: &Collection) -> (UndoStatus, UndoStatus) {
    (
        c.peek_undo(UndoScope::Global).unwrap(),
        c.peek_redo(UndoScope::Global).unwrap(),
    )
}

#[test]
fn queued_controlled_note_delete_keeps_rows_log_and_both_stacks() {
    let h = common::Harness::open();
    let (nid, cid) = common::steps::setup_note(&h.collection, "kept");
    common::steps::suspend(&h.collection, None, cid).unwrap();
    h.collection.undo(UndoScope::Global).unwrap(); // Include an actual redo step.
    let before = common::steps::dump(&h.collection);
    let stacks = history(&h.collection);
    let log = h.collection.path().with_file_name("deleted.txt");
    let log_before = std::fs::read(&log).ok();
    let gate = Gate::new();
    let _release = Release(gate.clone());
    let blocker = block_writer(&h.collection, OpSource::Rollover, &gate);
    let cancel = CancelToken::new();
    let c = h.collection.clone();
    let token = cancel.clone();
    let work = h
        .engine
        .threads()
        .spawn("test-controlled-note-delete", meguri_engine::Qos::Default, move |_| {
            notes::delete_controlled(&c, vec![nid], OpSource::UserUndoable, token, ProgressSink::new())
        })
        .unwrap();
    queued(&h.collection, meguri_engine::undo::names::DELETE_NOTE);
    cancel.cancel();
    gate.open();
    blocker.wait().unwrap();
    assert!(matches!(work.join().unwrap(), Err(EngineError::Cancelled)));
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(history(&h.collection), stacks);
    assert_eq!(std::fs::read(&log).ok(), log_before);
    // Missing-note deletion is still no operation and retains redo.
    assert!(
        notes::delete(
            &h.collection,
            vec![meguri_model::ids::NoteId(-1)],
            OpSource::UserUndoable
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(history(&h.collection), stacks);
}

struct CancelBackup(Arc<CancelToken>);
impl meguri_engine::ports::BackupService for CancelBackup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        self.0.cancel();
        Ok(())
    }
}
struct NoBackup;
impl meguri_engine::ports::BackupService for NoBackup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        panic!("empty deck needs no backup")
    }
}
#[test]
fn controlled_deck_delete_cancels_during_backup_and_when_queued() {
    let h = common::Harness::open();
    let empty = decks::add(&h.collection, "Empty cancel".into()).unwrap().value.id;
    common::steps::setup_note(&h.collection, "backup subject");
    let before = common::steps::dump(&h.collection);
    let stacks = history(&h.collection);
    let cancel = CancelToken::new();
    assert!(matches!(
        decks::delete_controlled(
            &h.collection,
            vec![meguri_model::ids::DeckId(1)],
            Arc::new(CancelBackup(cancel.clone())),
            cancel,
            ProgressSink::new()
        ),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(history(&h.collection), stacks);
    let gate = Gate::new();
    let _release = Release(gate.clone());
    let blocker = block_writer(&h.collection, OpSource::Rollover, &gate);
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let c = h.collection.clone();
    let work = h
        .engine
        .threads()
        .spawn("test-controlled-deck-delete", meguri_engine::Qos::Default, move |_| {
            decks::delete_controlled(&c, vec![empty], Arc::new(NoBackup), token, ProgressSink::new())
        })
        .unwrap();
    queued(&h.collection, "delete_decks");
    cancel.cancel();
    gate.open();
    blocker.wait().unwrap();
    assert!(matches!(work.join().unwrap(), Err(EngineError::Cancelled)));
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(history(&h.collection), stacks);
}

#[test]
fn queued_controlled_undo_and_redo_keep_rows_and_both_stacks() {
    let h = common::Harness::open();
    let (_, cid) = common::steps::setup_note(&h.collection, "restore subject");
    common::steps::suspend(&h.collection, None, cid).unwrap();
    for redo in [false, true] {
        if redo {
            h.collection.undo(UndoScope::Global).unwrap();
        }
        let before = common::steps::dump(&h.collection);
        let stacks = history(&h.collection);
        let gate = Gate::new();
        let _release = Release(gate.clone());
        let blocker = block_writer(&h.collection, OpSource::Rollover, &gate);
        let token = CancelToken::new();
        let pending = if redo {
            h.collection
                .submit_redo_controlled(UndoScope::Global, token.clone(), ProgressSink::new())
        } else {
            h.collection
                .submit_undo_controlled(UndoScope::Global, token.clone(), ProgressSink::new())
        };
        queued(&h.collection, if redo { "redo" } else { "undo" });
        token.cancel();
        gate.open();
        blocker.wait().unwrap();
        assert!(matches!(pending.wait(), Err(EngineError::Cancelled)));
        assert_eq!(common::steps::dump(&h.collection), before);
        assert_eq!(history(&h.collection), stacks);
    }
}

#[test]
fn one_root_subtree_edits_declare_bulk_and_keep_business_results() {
    let h = common::fresh();
    let root = decks::add(&h.collection, "Root".into()).unwrap().value.id;
    let target = decks::add(&h.collection, "Target".into()).unwrap().value.id;
    h.collection
        .run(OpSpec::named("seed descendants", OpSource::Maintenance), move |ctx| {
            for i in 0..2_001 {
                ctx.scope().add_deck(&decks::new_deck(format!("Root\u{1f}Child {i}")))?;
            }
            Ok(())
        })
        .unwrap();
    let renamed = decks::rename(&h.collection, root, "Renamed".into()).unwrap();
    assert_eq!(renamed.changes.decks.len(), 2_002);
    let UndoStatus::Available(step) = h.collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("rename missing")
    };
    assert!(step.long_write.is_some());
    let moved = decks::move_decks(&h.collection, vec![root], Some(target)).unwrap();
    assert_eq!(moved.value, 1);
    assert_eq!(moved.changes.decks.len(), 2_002);
    // A malformed stored root normalizes and renames descendants through save_in.
    h.collection
        .run(OpSpec::named("legacy deck name", OpSource::Maintenance), move |ctx| {
            for mut deck in ctx.scope().deck_rows()? {
                if deck.id == root || decks::names::descendant(&deck.name, "Target\u{1f}Renamed") {
                    deck.name = deck.name.replacen("Target\u{1f}Renamed", "Target\u{1f} Renamed ", 1);
                    ctx.scope().put(&deck)?;
                }
            }
            Ok(())
        })
        .unwrap();
    let collapsed = decks::collapse(&h.collection, vec![root], decks::CollapseScope::Study, false).unwrap();
    assert_eq!(collapsed.changes.decks.len(), 2_002);
    let UndoStatus::Available(step) = h.collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("collapse missing")
    };
    assert!(step.long_write.is_some());
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck(root)
            .unwrap()
            .unwrap()
            .name
            .ends_with("Renamed")
    );
}

#[test]
fn repeated_deck_requests_declare_the_full_work_on_a_small_table() {
    let h = common::fresh();
    let root = decks::add(&h.collection, "Repeated".into()).unwrap().value.id;
    let rows = h.collection.reader(ReaderRole::Query).unwrap().deck_rows().unwrap();
    assert_eq!(rows.len(), 2);
    let requests = meguri_engine::tags::LONG_WRITE_ROWS / rows.len() + 1;
    let declared = Duration::from_micros(100).saturating_mul((rows.len() * requests) as u32);
    let moved = decks::move_decks(&h.collection, vec![root; requests], Some(meguri_model::ids::DeckId(1))).unwrap();
    assert_eq!(moved.value, 1);
    assert_eq!(moved.changes.decks.len(), 1);
    let UndoStatus::Available(step) = h.collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("move missing")
    };
    assert_eq!(step.name, meguri_engine::undo::names::RENAME_DECK);
    assert_eq!(step.long_write, Some(declared));
    let collapsed = !rows.iter().find(|d| d.id == root).unwrap().common.study_collapsed;
    let changed = decks::collapse(
        &h.collection,
        vec![root; requests],
        decks::CollapseScope::Study,
        collapsed,
    )
    .unwrap();
    assert_eq!(changed.changes.decks.len(), 1);
    let UndoStatus::Available(step) = h.collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("collapse missing")
    };
    assert_eq!(step.name, meguri_engine::undo::names::EXPAND_COLLAPSE);
    assert_eq!(step.long_write, Some(declared));
}

#[test]
fn descendant_addition_replans_the_long_write_declaration() {
    let h = common::fresh();
    let root = decks::add(&h.collection, "Before".into()).unwrap().value.id;
    let gate = Gate::new();
    let _release = Release(gate.clone());
    let entered = gate.clone();
    let blocking = h.collection.submit(
        OpSpec::named("concurrent descendants", OpSource::Maintenance),
        move |ctx| {
            entered.enter_and_wait();
            for i in 0..2_001 {
                ctx.scope()
                    .add_deck(&decks::new_deck(format!("Before\u{1f}Concurrent {i}")))?;
            }
            Ok(())
        },
    );
    gate.wait_entered();
    let c = h.collection.clone();
    let work = h
        .engine
        .threads()
        .spawn("test-deck-bound-replan", meguri_engine::Qos::Default, move |_| {
            decks::rename_controlled(&c, root, "After".into(), CancelToken::new(), ProgressSink::new())
        })
        .unwrap();
    queued(&h.collection, "rename_deck");
    gate.open();
    blocking.wait().unwrap();
    let result = work.join().unwrap().unwrap();
    assert_eq!(result.changes.decks.len(), 2_002);
    let UndoStatus::Available(step) = h.collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("rename missing")
    };
    assert!(step.long_write.is_some());
}

#[test]
fn subtree_edits_cancel_in_planning_and_in_the_writer_queue_without_history_effects() {
    let h = common::fresh();
    let root = decks::add(&h.collection, "Kept".into()).unwrap().value.id;
    for kind in [0, 1, 2] {
        for planning in [true, false] {
            let before = common::steps::dump(&h.collection);
            let stacks = history(&h.collection);
            let gate = Gate::new();
            let _release = Release(gate.clone());
            let blocking = block_writer(&h.collection, OpSource::Rollover, &gate);
            let cancel = CancelToken::new();
            if planning {
                cancel.cancel();
            }
            let token = cancel.clone();
            let c = h.collection.clone();
            let work =
                h.engine
                    .threads()
                    .spawn(
                        "test-cancel-deck-edit",
                        meguri_engine::Qos::Default,
                        move |_| match kind {
                            0 => decks::rename_controlled(&c, root, "Changed".into(), token, ProgressSink::new())
                                .map(|_| ()),
                            1 => decks::move_decks_controlled(
                                &c,
                                vec![root],
                                Some(meguri_model::ids::DeckId(1)),
                                token,
                                ProgressSink::new(),
                            )
                            .map(|_| ()),
                            _ => decks::collapse_controlled(
                                &c,
                                vec![root],
                                decks::CollapseScope::Study,
                                false,
                                token,
                                ProgressSink::new(),
                            )
                            .map(|_| ()),
                        },
                    )
                    .unwrap();
            if !planning {
                queued(
                    &h.collection,
                    ["rename_deck", "reparent_decks", "set_deck_collapsed"][kind],
                );
                cancel.cancel();
            }
            gate.open();
            blocking.wait().unwrap();
            assert!(matches!(work.join().unwrap(), Err(EngineError::Cancelled)));
            assert_eq!(common::steps::dump(&h.collection), before);
            assert_eq!(history(&h.collection), stacks);
        }
    }
}
