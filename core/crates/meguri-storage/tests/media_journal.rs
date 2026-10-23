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

#[test]
fn tracked_names_preserve_journal_bytes_filter_nonlive_and_never_scan() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
    // Write journal rows directly to prove the query returns stored keys,
    // including canonically equivalent but byte-distinct spellings.
    let db = Connection::open(dir.path().join("meguri-media.db")).unwrap();
    for (name, key, live, pending) in [
        ("É.PNG", "é.png", true, false),
        ("E\u{301}.PNG", "e\u{301}.png", true, false),
        ("removed.png", "removed.png", false, false),
        ("waiting.png", "waiting.png", true, true),
        ("missing.png", "stored-key", true, false),
    ] {
        db.execute(
            "INSERT INTO media(name,case_key,sha1,size,mtime_secs,mtime_nanos,live,dirty,change_counter,pending_download,sync_state) VALUES(?1,?2,NULL,0,0,0,?3,0,1,?4,CASE WHEN ?4 THEN 3 ELSE 0 END)",
            rusqlite::params![name, key, live, pending],
        )
        .unwrap();
    }
    let before = store.diagnostics();
    let names = store.tracked_names().unwrap();
    assert_eq!(names.len(), 3);
    for (name, key) in [
        ("É.PNG", "é.png"),
        ("E\u{301}.PNG", "e\u{301}.png"),
        ("missing.png", "stored-key"),
    ] {
        assert!(names.iter().any(|entry| entry.name == name && entry.case_key == key));
    }
    // Missing on disk remains a safe resolver miss; querying never repairs,
    // enumerates, hashes or admits pending downloads as available names.
    assert_eq!(store.diagnostics(), before);
    assert!(store.try_close().unwrap());
    assert!(store.tracked_names().is_err());
}
