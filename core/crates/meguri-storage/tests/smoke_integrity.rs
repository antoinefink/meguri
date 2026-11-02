//! Generated-only Q5 observer tests; no CloudKit/signed acceptance.
#![cfg(feature = "test-hooks")]
mod common;
use meguri_storage::ReaderRole;

#[test]
fn full_check_and_schema_observe_the_owned_wal_snapshot_without_writes() {
    let (_dir, path) = common::empty_collection();
    let collection = common::open(&path).unwrap();
    let reader = collection.open_reader(ReaderRole::Query).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    let main = std::fs::read(&path).unwrap();
    let wal_path = path.with_file_name("collection.anki2-wal");
    let wal = std::fs::read(&wal_path).ok();
    let first = snapshot.smoke_integrity().unwrap();
    assert!(first.integrity_ok && first.schema_ok);
    assert_eq!(
        first.missing_card_notes
            + first.missing_card_decks
            + first.missing_note_types
            + first.missing_field_types
            + first.wrong_field_counts,
        0
    );
    assert_eq!(std::fs::read(&path).unwrap(), main);
    assert_eq!(std::fs::read(&wal_path).ok(), wal);
    common::exec(collection.writer(), "CREATE TABLE smoke_extra (id INTEGER)");
    common::exec(collection.writer(), "DROP INDEX ix_cards_nid");
    assert_eq!(
        snapshot.smoke_integrity().unwrap(),
        first,
        "schema shares the pinned WAL view"
    );
    drop(snapshot);
    let later = reader.begin_snapshot().unwrap();
    let changed = later.smoke_integrity().unwrap();
    assert!(changed.integrity_ok);
    assert!(
        !changed.schema_ok,
        "quick/open success cannot substitute for exact schema"
    );
}

#[test]
fn structurally_sound_rows_still_report_missing_references_and_field_counts() {
    let (_dir, path) = common::empty_collection();
    let collection = common::open(&path).unwrap();
    let writer = collection.writer();
    common::exec(
        writer,
        "INSERT INTO notes VALUES(1,'generated',99,0,-1,'','one','one',0,0,'')",
    );
    common::exec(
        writer,
        "INSERT INTO cards VALUES(1,2,99,0,0,-1,0,0,0,0,0,0,0,0,0,0,0,'')",
    );
    common::exec(writer, "INSERT INTO fields VALUES(99,0,'Q',X'')");
    let reader = collection.open_reader(ReaderRole::Query).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    let report = snapshot.smoke_integrity().unwrap();
    assert!(report.integrity_ok && report.schema_ok);
    assert_eq!(report.missing_card_notes, 1);
    assert_eq!(report.missing_card_decks, 1);
    assert_eq!(report.missing_note_types, 1);
    assert_eq!(report.missing_field_types, 1);
    assert_eq!(report.wrong_field_counts, 0);
    drop(snapshot);
    common::exec(writer, "UPDATE notes SET flds='Q' || char(31) || 'A'");
    let changed = reader.begin_snapshot().unwrap().smoke_integrity().unwrap();
    assert_eq!(changed.wrong_field_counts, 1);
}
