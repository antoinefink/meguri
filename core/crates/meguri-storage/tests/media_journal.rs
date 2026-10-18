mod common;

use meguri_storage::media::{MediaError, MediaStore};
use rusqlite::Connection;
use std::{fs, sync::Arc};

#[test]
fn media_journal_refuses_a_foreign_database_without_changing_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meguri-media.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE private(value TEXT); INSERT INTO private VALUES('preserve');")
        .unwrap();
    drop(db);
    let bytes = fs::read(&path).unwrap();
    let result = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock()));
    assert!(matches!(result, Err(MediaError::Database(_))));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!dir.path().join("meguri-media.db-wal").exists());
}

#[test]
fn media_journal_refuses_a_future_version_without_changing_it() {
    let dir = tempfile::tempdir().unwrap();
    let collection = dir.path().join("collection.anki2");
    drop(MediaStore::open(&collection, Arc::new(common::clock())).unwrap());
    let path = dir.path().join("meguri-media.db");
    let db = Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    let bytes = fs::read(&path).unwrap();
    assert!(matches!(
        MediaStore::open(&collection, Arc::new(common::clock())),
        Err(MediaError::Database(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn media_journal_acknowledgement_cannot_hide_a_newer_local_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
    store.ensure_static_media("asset.png", b"old").unwrap();
    let old = store.pending().unwrap().pop().unwrap();
    store.ensure_static_media("asset.png", b"new").unwrap();
    assert!(!store.acknowledge(&old.name, old.change_counter).unwrap());
    let new = store.pending().unwrap().pop().unwrap();
    assert!(new.change_counter > old.change_counter);
    assert!(store.acknowledge(&new.name, new.change_counter).unwrap());
    assert!(store.pending().unwrap().is_empty());
}
