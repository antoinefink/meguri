//! P11 live snapshots use the existing Maintenance reader and U32 staging.
#![cfg(feature = "test-hooks")]

mod common;

use std::{fs, path::Path};

use common::{card, clock, empty_collection, listing, note, revlog};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{
    OpenOptions, ReadRows, ReaderRole, Record, StampMode, StorageError, leftovers::staging_directory, open_collection,
};

fn managed(path: &Path) -> meguri_storage::OpenedCollection {
    open_collection(path, &OpenOptions::new(&clock(), &SeededRandom::new(7)).managed()).unwrap()
}

fn assert_finalised(stage: &Path, snapshot: &Path) {
    assert_eq!(snapshot, stage.join("snapshot.anki2"));
    assert_eq!(&fs::read(snapshot).unwrap()[18..20], &[2, 2]);
    assert_eq!(listing(stage), ["snapshot.anki2".to_owned()].into());
}

#[test]
fn live_snapshot_includes_committed_wal_answers_while_sync_keeps_its_old_view() {
    let (directory, path) = empty_collection();
    let collection = managed(&path);
    let writer = collection.writer();
    let clock = StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let first = scope.add_note(&note(1, &["first", "answer"])).unwrap();
    scope.commit().unwrap();

    let sync_reader = collection.open_reader(ReaderRole::Sync).unwrap();
    let sync = sync_reader.begin_snapshot().unwrap();
    assert!(sync.note(first).unwrap().is_some());
    let maintenance = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let second = scope.add_note(&note(1, &["second", "committed in WAL"])).unwrap();
    let card_id = scope.add_card(&card(second, 0)).unwrap();
    let review_id = scope.add_revlog(&revlog(card_id, 1200)).unwrap();
    scope.commit().unwrap();
    assert!(sync.note(second).unwrap().is_none());

    let wal_path = directory.path().join("collection.anki2-wal");
    let sidecar_path = directory.path().join("meguri-sidecar.json");
    let database_before = fs::read(&path).unwrap();
    let wal_before = fs::read(&wal_path).unwrap();
    assert!(wal_before.len() > 32, "test must back up committed WAL frames");
    let sidecar_before = fs::read(&sidecar_path).unwrap();
    let stage = staging_directory(directory.path()).unwrap();
    let snapshot = maintenance.snapshot_live(stage.path()).unwrap();
    assert_finalised(stage.path(), &snapshot);
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&wal_path).unwrap(), wal_before);
    assert_eq!(fs::read(&sidecar_path).unwrap(), sidecar_before);
    assert!(sync.note(second).unwrap().is_none(), "sync still owns its earlier view");

    // The same live writer and readers remain usable after the snapshot.
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let third = scope.add_note(&note(1, &["after backup", "still writable"])).unwrap();
    scope.commit().unwrap();
    assert!(writer.note(third).unwrap().is_some());
    assert!(maintenance.note(third).unwrap().is_some());
    assert!(sync.note(third).unwrap().is_none());

    let backup = common::open(&snapshot).unwrap();
    assert_eq!(backup.writer().note(first).unwrap(), writer.note(first).unwrap());
    assert_eq!(backup.writer().note(second).unwrap(), writer.note(second).unwrap());
    assert_eq!(
        backup.writer().card(card_id).unwrap().map(|card| card.to_cells()),
        writer.card(card_id).unwrap().map(|card| card.to_cells())
    );
    assert_eq!(
        backup.writer().revlog_entry(review_id).unwrap(),
        writer.revlog_entry(review_id).unwrap()
    );
    assert!(backup.writer().note(third).unwrap().is_none());
    backup.close().unwrap();
    assert_finalised(stage.path(), &snapshot);
    drop(sync);
    assert!(sync_reader.note(third).unwrap().is_some());
    drop(sync_reader);
    drop(maintenance);
    collection.close().unwrap();
}

#[test]
fn other_roles_and_an_active_maintenance_transaction_refuse_before_writing() {
    let (directory, path) = empty_collection();
    let collection = managed(&path);
    let stage = staging_directory(directory.path()).unwrap();
    fs::write(stage.path().join("caller-owned"), b"preserve").unwrap();
    let before = listing(stage.path());
    let source_before = fs::read(&path).unwrap();
    for role in ReaderRole::ALL {
        if role == ReaderRole::Maintenance {
            continue;
        }
        let reader = collection.open_reader(role).unwrap();
        assert!(
            matches!(reader.snapshot_live(stage.path()), Err(StorageError::Mode { .. })),
            "{role:?}"
        );
        assert_eq!(listing(stage.path()), before);
    }
    let maintenance = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let transaction = maintenance.begin_snapshot().unwrap();
    assert_eq!(
        maintenance.snapshot_live(stage.path()),
        Err(StorageError::TransactionOpen)
    );
    assert!(
        transaction.col().is_ok(),
        "the refused call leaves the transaction usable"
    );
    assert_eq!(listing(stage.path()), before);
    assert_eq!(fs::read(stage.path().join("caller-owned")).unwrap(), b"preserve");
    assert_eq!(fs::read(&path).unwrap(), source_before);
    drop(transaction);
    // Refusal must not leave this shared reader inside another transaction.
    let snapshot = maintenance.snapshot_live(stage.path()).unwrap();
    assert_eq!(&fs::read(snapshot).unwrap()[18..20], &[2, 2]);
    drop(maintenance);
    collection.close().unwrap();
}

#[test]
fn an_existing_destination_even_when_empty_is_preserved() {
    let (directory, path) = empty_collection();
    let collection = managed(&path);
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    for bytes in [b"".as_slice(), b"existing user bytes".as_slice()] {
        let stage = staging_directory(directory.path()).unwrap();
        let destination = stage.path().join("snapshot.anki2");
        fs::write(&destination, bytes).unwrap();
        assert_eq!(reader.snapshot_live(stage.path()), Err(StorageError::DestinationExists));
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        assert_eq!(listing(stage.path()), ["snapshot.anki2".to_owned()].into());
    }
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn snapshot_refuses_existing_sidecars() {
    let (directory, path) = empty_collection();
    let collection = managed(&path);
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    for entry_name in ["snapshot.anki2-wal", "snapshot.anki2-shm", "snapshot.anki2-journal"] {
        let stage = staging_directory(directory.path()).unwrap();
        fs::write(stage.path().join(entry_name), [1_u8, 2, 3]).unwrap();
        assert_eq!(reader.snapshot_live(stage.path()), Err(StorageError::DestinationExists));
        assert_eq!(fs::read(stage.path().join(entry_name)).unwrap(), [1_u8, 2, 3]);
        assert_eq!(listing(stage.path()), [entry_name.to_owned()].into());
    }
    drop(reader);
    collection.close().unwrap();
}

#[cfg(unix)]
#[test]
fn snapshot_refuses_symlink_destinations() {
    use std::os::unix::fs::symlink;

    let (directory, path) = empty_collection();
    let collection = managed(&path);
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let original = fs::read(&path).unwrap();
    let missing = directory.path().join("not-created.anki2");
    for target in [&path, &missing] {
        for name in [
            "snapshot.anki2",
            "snapshot.anki2-wal",
            "snapshot.anki2-shm",
            "snapshot.anki2-journal",
        ] {
            let stage = staging_directory(directory.path()).unwrap();
            let link = stage.path().join(name);
            symlink(target, &link).unwrap();
            assert_eq!(reader.snapshot_live(stage.path()), Err(StorageError::DestinationExists));
            assert_eq!(fs::read_link(&link).unwrap(), *target);
            assert_eq!(listing(stage.path()), [name.to_owned()].into());
            assert_eq!(fs::read(&path).unwrap(), original);
            assert!(!missing.exists());
        }
    }
    drop(reader);
    collection.close().unwrap();
}
