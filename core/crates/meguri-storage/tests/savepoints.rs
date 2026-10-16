//! Savepoints inside a write scope (`engine/op-model` O3): a composite op's
//! step that fails is rolled back to its savepoint, its rows and its journal
//! entries with it, while the op keeps one transaction and one journal.

mod common;

use common::{NOW_MS, card, empty_collection, note, open};
use meguri_model::card::CardQueue;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_storage::journal::After;
use meguri_storage::{JournalOptions, ReadRows, Record, RowKey, StampMode, StorageError, Table};

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

#[test]
fn a_step_rolled_back_to_its_savepoint_leaves_no_row_and_no_journal_entry() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let kept = scope.add_note(&note(1, &["kept", "x"])).unwrap();

    let step = scope.savepoint().unwrap();
    let dropped = scope.add_note(&note(1, &["dropped", "y"])).unwrap();
    assert!(scope.note(dropped).unwrap().is_some());
    scope.rollback_to_savepoint(step).unwrap();

    assert!(scope.note(dropped).unwrap().is_none(), "the step's row is gone");
    assert!(scope.note(kept).unwrap().is_some(), "rows before the step stay");
    let journal = scope.journal().unwrap();
    assert!(journal.entry(Table::Notes, &RowKey::Id(dropped.0)).is_none());
    assert!(journal.entry(Table::Notes, &RowKey::Id(kept.0)).is_some());
    assert_eq!(journal.len(), 1);

    let committed = scope.commit().unwrap();
    assert!(committed.changed);
    assert!(writer.note(dropped).unwrap().is_none());
    assert!(writer.note(kept).unwrap().is_some());
}

#[test]
fn a_row_changed_before_and_inside_a_rolled_back_step_keeps_its_earlier_after_image() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let nid = scope.add_note(&note(1, &["a", "b"])).unwrap();
    let cid = scope.add_card(&card(nid, 0)).unwrap();
    scope.commit().unwrap();
    let original = writer.card(cid).unwrap().unwrap();

    clock.step_ms(60_000);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let mut first = original.clone();
    first.queue = CardQueue::Suspended;
    scope.put(&first).unwrap();
    let after_first = scope.card(cid).unwrap().unwrap();

    let step = scope.savepoint().unwrap();
    let mut second = after_first.clone();
    second.flags = 3;
    scope.put(&second).unwrap();
    scope.rollback_to_savepoint(step).unwrap();

    assert_eq!(scope.card(cid).unwrap().unwrap().flags, 0);
    let journal = scope.journal().unwrap();
    let entry = journal.entry(Table::Cards, &RowKey::Id(cid.0)).unwrap();
    assert_eq!(journal.before(entry).unwrap(), Some(original.to_cells()));
    assert_eq!(entry.after(), &After::Row(after_first.to_cells()));
    scope.commit().unwrap();
    assert_eq!(writer.card(cid).unwrap().unwrap().queue, CardQueue::Suspended);
}

#[test]
fn a_released_step_keeps_its_rows_and_an_inner_rollback_spares_the_outer_step() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let outer = scope.savepoint().unwrap();
    let a = scope.add_note(&note(1, &["outer", "1"])).unwrap();
    let inner = scope.savepoint().unwrap();
    let b = scope.add_note(&note(1, &["inner", "2"])).unwrap();
    scope.rollback_to_savepoint(inner).unwrap();
    assert!(scope.journal().unwrap().entry(Table::Notes, &RowKey::Id(b.0)).is_none());
    // The rolled-back id is free again, so the next note may take it.
    let c = scope.add_note(&note(1, &["after", "3"])).unwrap();
    scope.release_savepoint(outer).unwrap();

    let journal = scope.journal().unwrap();
    assert_eq!(journal.len(), 2);
    assert!(journal.entry(Table::Notes, &RowKey::Id(a.0)).is_some());
    assert!(journal.entry(Table::Notes, &RowKey::Id(c.0)).is_some());
    scope.commit().unwrap();
    let fields: Vec<String> = writer
        .notes_after(None, 10)
        .unwrap()
        .into_iter()
        .map(|n| n.fields)
        .collect();
    assert_eq!(fields, ["outer\u{1f}1", "after\u{1f}3"]);
}

#[test]
fn rolling_back_the_only_change_leaves_col_mod_alone() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let col_mod = writer.col().unwrap().modified;
    clock.step_ms(5_000);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let step = scope.savepoint().unwrap();
    scope.add_note(&note(1, &["gone", "z"])).unwrap();
    assert!(scope.has_changes());
    scope.rollback_to_savepoint(step).unwrap();
    assert!(!scope.has_changes());
    let committed = scope.commit().unwrap();
    assert!(!committed.changed);
    assert_eq!(committed.col_mod, None);
    assert!(committed.journal.unwrap().is_empty());
    assert_eq!(writer.col().unwrap().modified, col_mod);
}

#[test]
fn spilled_before_images_of_a_rolled_back_step_are_dropped() {
    let (dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let ids: Vec<_> = (0..40)
        .map(|i| scope.add_note(&note(1, &[&"x".repeat(300), &i.to_string()])).unwrap())
        .collect();
    scope.commit().unwrap();

    clock.step_ms(60_000);
    let options = JournalOptions {
        spill_threshold: 2_000,
        spill_dir: Some(dir.path().to_path_buf()),
    };
    let mut scope = writer.begin_write(StampMode::Local, &clock, Some(options)).unwrap();
    let edit = |scope: &mut meguri_storage::WriteScope<'_>, id| {
        let mut n = scope.note(id).unwrap().unwrap();
        n.fields = format!("{}\u{1f}edited", "y".repeat(300));
        scope.put(&n).unwrap();
    };
    for id in &ids[..5] {
        edit(&mut scope, *id);
    }
    let held = scope.journal().unwrap().held_bytes();
    let spilled = scope.journal().unwrap().spilled_bytes();
    let step = scope.savepoint().unwrap();
    for id in &ids[5..] {
        edit(&mut scope, *id);
    }
    assert!(scope.journal().unwrap().spilled_bytes() > spilled, "the step spilled");
    scope.rollback_to_savepoint(step).unwrap();
    let journal = scope.journal().unwrap();
    assert_eq!(journal.len(), 5);
    assert_eq!(journal.held_bytes(), held);
    assert_eq!(journal.spilled_bytes(), spilled);
    // A later step reuses the spill file from where the kept entries end.
    edit(&mut scope, ids[39]);
    let entry = scope
        .journal()
        .unwrap()
        .entry(Table::Notes, &RowKey::Id(ids[39].0))
        .unwrap();
    let before = scope.journal().unwrap().before(entry).unwrap().unwrap();
    assert_eq!(
        before[6],
        meguri_storage::Cell::text(format!("{}\u{1f}39", "x".repeat(300)))
    );
    scope.commit().unwrap();
}

#[test]
fn savepoints_are_released_and_rolled_back_innermost_first() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let outer = scope.savepoint().unwrap();
    let _inner = scope.savepoint().unwrap();
    assert!(matches!(scope.release_savepoint(outer), Err(StorageError::Mode { .. })));
    assert!(matches!(
        scope.rollback_to_savepoint(outer),
        Err(StorageError::Mode { .. })
    ));
}
