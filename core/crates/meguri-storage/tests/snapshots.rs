//! What the engine's executor and readers need from storage (U24): read
//! snapshots on a reader that keep one view until dropped, readers opened
//! from a factory that outlives the writer's thread, and row digests that
//! equal the journal's note digests (`engine/op-model` O6, O7).

mod common;

use std::thread;

use common::{NOW_MS, card, empty_collection, exec, int, note, open};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_storage::journal::{self, After};
use meguri_storage::{JournalOptions, ReadRows, ReaderRole, Record, RowKey, StampMode, Table};

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

#[test]
fn a_snapshot_keeps_its_view_until_dropped_and_then_lets_a_checkpoint_finish() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let first = scope.add_note(&note(1, &["first", "1"])).unwrap();
    scope.commit().unwrap();

    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    assert!(snapshot.note(first).unwrap().is_some());

    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let second = scope.add_note(&note(1, &["second", "2"])).unwrap();
    scope.commit().unwrap();

    assert!(
        snapshot.note(second).unwrap().is_none(),
        "the snapshot predates the commit"
    );
    assert!(writer.note(second).unwrap().is_some());
    assert!(writer.checkpoint().unwrap().busy, "the open snapshot holds the log");

    drop(snapshot);
    assert!(reader.note(second).unwrap().is_some(), "a fresh read sees the commit");
    assert!(!writer.checkpoint().unwrap().busy);
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn a_reader_factory_opens_readers_on_another_thread_with_their_role_settings() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let factory = collection.reader_factory();
    let roles = thread::spawn(move || {
        ReaderRole::ALL
            .into_iter()
            .map(|role| {
                let reader = factory.open(role).unwrap();
                (
                    role,
                    reader.role(),
                    int(&common::query(&reader, "SELECT * FROM pragma_query_only")),
                )
            })
            .collect::<Vec<_>>()
    })
    .join()
    .unwrap();
    for (asked, got, query_only) in roles {
        assert_eq!(asked, got);
        assert_eq!(query_only == 1, asked.query_only(), "{asked:?}");
    }
    collection.close().unwrap();
}

#[test]
fn a_row_digest_equals_the_journal_digest_of_the_stored_note() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let nid = scope.add_note(&note(1, &["digest", "me"])).unwrap();
    let cid = scope.add_card(&card(nid, 0)).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();

    let After::Note(added) = journal.entry(Table::Notes, &RowKey::Id(nid.0)).unwrap().after() else {
        panic!("a note's after-image is a digest");
    };
    let image = writer.row_image(Table::Notes, &RowKey::Id(nid.0)).unwrap().unwrap();
    assert_eq!(journal::digest(&image), added.sha256);
    assert_eq!(
        writer.row_digest(Table::Notes, &RowKey::Id(nid.0)).unwrap(),
        Some(added.sha256)
    );

    let stored_card = writer.card(cid).unwrap().unwrap();
    assert_eq!(
        writer.row_image(Table::Cards, &RowKey::Id(cid.0)).unwrap(),
        Some(stored_card.to_cells())
    );
    assert_eq!(writer.row_image(Table::Cards, &RowKey::Id(cid.0 + 99)).unwrap(), None);
    assert_eq!(writer.row_digest(Table::Cards, &RowKey::Id(cid.0 + 99)).unwrap(), None);

    // A change of one column changes the digest.
    let before = writer.row_digest(Table::Cards, &RowKey::Id(cid.0)).unwrap();
    exec(writer, &format!("UPDATE cards SET flags = 1 WHERE id = {}", cid.0));
    assert_ne!(writer.row_digest(Table::Cards, &RowKey::Id(cid.0)).unwrap(), before);
    collection.close().unwrap();
}
