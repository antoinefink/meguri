//! `decks/decks` D8 step 6: Delete Deck's confirmed losses, counted again on
//! the planning snapshot and inside the write transaction. Each change that
//! must land between the planning and the commit is made by a progress
//! listener on the planning pass's report, which the op makes on its
//! snapshot (as `notetype_removal.rs` does), so no case depends on timing.
#![cfg(feature = "test-hooks")]
mod common;

use common::{Harness, card, note, query_int};
use meguri_engine::decks::{self, ConfirmedLosses, DeletionSummary, Refusal};
use meguri_engine::{
    CancelToken, Collection, EngineError, OpSource, OpSpec, ProgressSink, UndoScope, UndoStatus, ports::BackupService,
};
use meguri_model::ids::{DeckId, NoteId};
use meguri_storage::{ReadRows, ReaderRole};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Backups(AtomicUsize);
impl BackupService for Backups {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
impl Backups {
    fn calls(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

type Change = Box<dyn Fn(&Collection, usize) + Send + Sync>;

/// Runs `change` at each planning pass's report, on that pass's snapshot.
struct DuringPlanning {
    collection: Collection,
    change: Change,
    passes: AtomicUsize,
}
impl meguri_engine::ProgressListener for DuringPlanning {
    fn on_progress(&self, progress: &meguri_engine::Progress) {
        // The phase change reports no total; the pass's own report has one.
        if progress.phase == meguri_engine::ProgressPhase::Planning && progress.completed == 0 && progress.total.is_some() {
            let pass = self.passes.fetch_add(1, Ordering::SeqCst);
            (self.change)(&self.collection, pass);
        }
    }
}

/// A sink whose listener makes `change` on the first planning pass only.
fn on_first_plan(h: &Harness, change: impl Fn(&Collection) + Send + Sync + 'static) -> ProgressSink {
    static TICK: AtomicU64 = AtomicU64::new(0);
    fn next_second() -> std::time::Duration {
        // Every report a second after the last, so the rate limit holds none back.
        std::time::Duration::from_secs(TICK.fetch_add(1, Ordering::SeqCst))
    }
    let sink = ProgressSink::with_clock(next_second);
    sink.set_listener(Arc::new(DuringPlanning {
        collection: h.collection.clone(),
        change: Box::new(move |c, pass| {
            if pass == 0 {
                change(c)
            }
        }),
        passes: AtomicUsize::new(0),
    }));
    sink
}

fn deck(h: &Harness, name: &str) -> DeckId {
    decks::add(&h.collection, name.into()).unwrap().value.id
}

/// One note with one card in `deck`, through a raw undoable op (writer only,
/// so it can run while a plan holds the maintenance reader).
fn add_card(c: &Collection, deck: DeckId) -> NoteId {
    c.run(OpSpec::named("concurrent_add", OpSource::UserUndoable), move |ctx| {
        let nid = ctx.scope().add_note(&note(&["extra", "back"]))?;
        let mut card = card(nid, 0);
        card.deck_id = deck;
        ctx.scope().add_card(&card)?;
        Ok(nid)
    })
    .unwrap()
    .value
}

fn remove_note(c: &Collection, nid: NoteId) {
    c.run(OpSpec::named("concurrent_remove", OpSource::UserUndoable), move |ctx| {
        ctx.scope().remove_note_and_cards(nid)?;
        Ok(())
    })
    .unwrap();
}

fn counts(decks: usize, cards: usize) -> ConfirmedLosses {
    ConfirmedLosses::Counts(DeletionSummary { decks, cards })
}

fn run(
    h: &Harness,
    ids: &[DeckId],
    confirmed: ConfirmedLosses,
    backups: &Arc<Backups>,
    progress: ProgressSink,
) -> Result<DeletionSummary, EngineError> {
    decks::delete_controlled(
        &h.collection,
        ids.to_vec(),
        confirmed,
        backups.clone(),
        CancelToken::new(),
        progress,
    )
    .map(|r| r.value)
}

fn losses(decks: u64, cards: u64) -> EngineError {
    EngineError::DeckRefused(Refusal::LossesChanged { decks, cards })
}

fn exists(h: &Harness, id: DeckId) -> bool {
    h.collection.reader(ReaderRole::Query).unwrap().deck(id).unwrap().is_some()
}

fn history(c: &Collection) -> (UndoStatus, UndoStatus) {
    (c.peek_undo(UndoScope::Global).unwrap(), c.peek_redo(UndoScope::Global).unwrap())
}

#[test]
fn a_card_added_to_the_deck_before_the_commit_is_losses_changed() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    add_card(&h.collection, home);
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, move |c| {
        add_card(c, home);
    });
    let error = run(&h, &[home], counts(1, 1), &backups, progress).unwrap_err();
    assert_eq!(error, losses(1, 2));
    assert!(exists(&h, home));
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 2);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
    // The first pass had backed up; the replan refuses before another.
    assert_eq!(backups.calls(), 1);
    // The fresh counts, confirmed, delete both cards.
    let deleted = run(&h, &[home], counts(1, 2), &backups, ProgressSink::new()).unwrap();
    assert_eq!(deleted, DeletionSummary { decks: 1, cards: 2 });
    assert!(!exists(&h, home));
}

#[test]
fn a_card_added_to_a_subdeck_before_the_commit_is_losses_changed() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    let sub = deck(&h, "Home::Sub");
    add_card(&h.collection, home);
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, move |c| {
        add_card(c, sub);
    });
    assert_eq!(
        run(&h, &[home], counts(2, 1), &backups, progress).unwrap_err(),
        losses(2, 2)
    );
    assert!(exists(&h, home) && exists(&h, sub));
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 2);
}

#[test]
fn an_empty_subdeck_added_before_the_commit_is_losses_changed() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, |c| {
        decks::add(c, "Home::New".into()).unwrap();
    });
    assert_eq!(
        run(&h, &[home], counts(1, 0), &backups, progress).unwrap_err(),
        losses(2, 0)
    );
    assert!(exists(&h, home));
    assert_eq!(backups.calls(), 0);
}

#[test]
fn a_delete_confirmed_at_zero_cards_that_gains_one_is_refused_without_a_backup() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, move |c| {
        add_card(c, home);
    });
    assert_eq!(
        run(&h, &[home], counts(1, 0), &backups, progress).unwrap_err(),
        losses(1, 1)
    );
    assert!(exists(&h, home));
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 1);
    assert_eq!(backups.calls(), 0);
}

#[test]
fn a_count_that_shrinks_before_the_commit_still_deletes() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    add_card(&h.collection, home);
    let gone = add_card(&h.collection, home);
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, move |c| remove_note(c, gone));
    let deleted = run(&h, &[home], counts(1, 2), &backups, progress).unwrap();
    assert_eq!(deleted, DeletionSummary { decks: 1, cards: 1 });
    assert!(!exists(&h, home));
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 0);
}

#[test]
fn a_stale_confirmation_is_refused_on_the_planning_snapshot_without_a_backup() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    add_card(&h.collection, home);
    add_card(&h.collection, home);
    let backups = Arc::new(Backups::default());
    assert_eq!(
        run(&h, &[home], counts(1, 1), &backups, ProgressSink::new()).unwrap_err(),
        losses(1, 2)
    );
    assert_eq!(backups.calls(), 0);
    // `decks` alone refuses too: the question did not state this subdeck.
    deck(&h, "Home::Sub");
    assert_eq!(
        run(&h, &[home], counts(1, 2), &backups, ProgressSink::new()).unwrap_err(),
        losses(2, 2)
    );
    assert_eq!(backups.calls(), 0);
}

/// With the table preconditions skipped, the count in the write transaction
/// refuses on its own.
#[test]
fn the_count_in_the_write_transaction_refuses_alone() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    add_card(&h.collection, home);
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, move |c| {
        add_card(c, home);
    });
    let error = decks::without_table_preconditions_for_testing(|| run(&h, &[home], counts(1, 1), &backups, progress))
        .unwrap_err();
    assert_eq!(error, losses(1, 2));
    assert_eq!(backups.calls(), 1);
    assert!(exists(&h, home));
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 2);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
}

/// "Backup at commit": a commit that finds cards the plan did not, with no
/// backup taken, plans again; with counts the replan refuses, with `Any` it
/// backs up and then removes the card.
#[test]
fn cards_found_at_commit_without_a_backup_replan_first() {
    for confirmed in [counts(1, 0), ConfirmedLosses::Any] {
        let h = common::fresh();
        let home = deck(&h, "Home");
        let backups = Arc::new(Backups::default());
        let progress = on_first_plan(&h, move |c| {
            add_card(c, home);
        });
        let result =
            decks::without_table_preconditions_for_testing(|| run(&h, &[home], confirmed, &backups, progress));
        match confirmed {
            ConfirmedLosses::Any => {
                assert_eq!(result.unwrap(), DeletionSummary { decks: 1, cards: 1 });
                assert_eq!(backups.calls(), 1);
                assert!(!exists(&h, home));
            }
            ConfirmedLosses::Counts(_) => {
                assert_eq!(result.unwrap_err(), losses(1, 1));
                assert_eq!(backups.calls(), 0);
                assert!(exists(&h, home));
            }
        }
    }
}

/// `Any` with the table preconditions in place: the card added after the
/// planning goes after a replan and a backup.
#[test]
fn any_deletes_what_the_commit_finds() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    let backups = Arc::new(Backups::default());
    let progress = on_first_plan(&h, move |c| {
        add_card(c, home);
    });
    let deleted = run(&h, &[home], ConfirmedLosses::Any, &backups, progress).unwrap();
    assert_eq!(deleted, DeletionSummary { decks: 1, cards: 1 });
    assert_eq!(backups.calls(), 1);
    assert!(!exists(&h, home));
}

/// A refusal records no step and leaves the undo and redo history as it was,
/// both on the planning snapshot and in the write transaction.
#[test]
fn a_refusal_leaves_the_undo_and_redo_history() {
    let h = common::fresh();
    let home = deck(&h, "Home");
    add_card(&h.collection, home);
    deck(&h, "Spare");
    h.collection.undo(UndoScope::Global).unwrap();
    let before = history(&h.collection);
    assert!(matches!(before.1, UndoStatus::Available(_)));
    let backups = Arc::new(Backups::default());
    assert_eq!(
        run(&h, &[home], counts(1, 0), &backups, ProgressSink::new()).unwrap_err(),
        losses(1, 1)
    );
    assert_eq!(history(&h.collection), before);
    // In the write transaction: the history as the concurrent write left it.
    let seen = Arc::new(Mutex::new(None));
    let record = seen.clone();
    let progress = on_first_plan(&h, move |c| {
        add_card(c, home);
        *record.lock().unwrap() = Some(history(c));
    });
    let error = decks::without_table_preconditions_for_testing(|| run(&h, &[home], counts(1, 1), &backups, progress))
        .unwrap_err();
    assert_eq!(error, losses(1, 2));
    assert_eq!(Some(history(&h.collection)), seen.lock().unwrap().clone());
}
