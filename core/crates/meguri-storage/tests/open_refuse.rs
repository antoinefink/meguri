//! Refused files stay untouched (`storage/open-and-connections` C1, C2.2,
//! C3; R1479, R1482, R2013): same SHA-256, and no file appears next to them,
//! for rollback-journal and WAL-mode files. The scenarios record only the
//! refusal kind; the bytes and the directory are checked here.

mod common;

use std::fs;

use common::{copy_fixture, listing, open, plain, sha256};
use meguri_storage::OpenError;

fn assert_refused_untouched(fixture: &str, expected: &OpenError) {
    let (dir, path) = copy_fixture(fixture);
    let before_sha = sha256(&path);
    let before_listing = listing(dir.path());
    let error = open(&path).expect_err(fixture);
    assert_eq!(&error, expected, "{fixture}");
    assert_eq!(sha256(&path), before_sha, "{fixture}: the file changed");
    assert_eq!(listing(dir.path()), before_listing, "{fixture}: a file appeared");
}

#[test]
fn refused_versions_leave_the_file_and_its_directory_untouched() {
    for mode in ["delete", "wal"] {
        assert_refused_untouched(&format!("ver-10-{mode}.anki2"), &OpenError::TooOld { version: 10 });
        for version in [12, 13, 19] {
            assert_refused_untouched(&format!("ver-{version}-{mode}.anki2"), &OpenError::TooNew { version });
        }
    }
}

#[test]
fn v1_scheduler_refused_and_untouched() {
    assert_refused_untouched("v1-scheduler-18.anki2", &OpenError::V1Scheduler);
    assert_refused_untouched("v1-scheduler-11.anki2", &OpenError::V1Scheduler);
}

#[test]
fn a_refused_file_with_a_wal_file_next_to_it_gains_at_most_a_shm_file() {
    let (dir, path) = copy_fixture("ver-19-wal.anki2");
    let wal = dir.path().join("collection.anki2-wal");
    fs::write(&wal, b"").unwrap();
    let before_sha = sha256(&path);
    let before = listing(dir.path());
    assert_eq!(open(&path).unwrap_err(), OpenError::TooNew { version: 19 });
    assert_eq!(sha256(&path), before_sha);
    let mut after = listing(dir.path());
    after.remove("collection.anki2-shm");
    assert_eq!(after, before);
}

#[test]
fn files_that_are_not_collections_are_refused_and_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_collection = OpenError::NotACollection {
        missing_tables: Vec::new(),
    };

    let text = dir.path().join("text.anki2");
    fs::write(
        &text,
        "this is not a database, just enough text to fill a header ....................",
    )
    .unwrap();
    let empty = dir.path().join("empty.anki2");
    fs::write(&empty, b"").unwrap();

    let no_col = dir.path().join("no-col.anki2");
    plain(&no_col).execute_batch("CREATE TABLE notes (id integer)").unwrap();
    let no_row = dir.path().join("no-row.anki2");
    plain(&no_row).execute_batch("CREATE TABLE col (ver integer)").unwrap();
    let text_ver = dir.path().join("text-ver.anki2");
    // No affinity, so the text stays text.
    plain(&text_ver)
        .execute_batch("CREATE TABLE col (ver); INSERT INTO col VALUES ('18')")
        .unwrap();
    let real_ver = dir.path().join("real-ver.anki2");
    plain(&real_ver)
        .execute_batch("CREATE TABLE col (ver integer); INSERT INTO col VALUES (18.5)")
        .unwrap();

    let before = listing(dir.path());
    let cases = [
        (&text, not_a_collection.clone()),
        (
            &empty,
            OpenError::NotACollection {
                missing_tables: vec!["col".to_owned()],
            },
        ),
        (
            &no_col,
            OpenError::NotACollection {
                missing_tables: vec!["col".to_owned()],
            },
        ),
        (&no_row, not_a_collection.clone()),
        (&text_ver, not_a_collection.clone()),
        (&real_ver, not_a_collection),
    ];
    for (path, expected) in cases {
        let sha = sha256(path);
        assert_eq!(open(path).unwrap_err(), expected, "{}", path.display());
        assert_eq!(sha256(path), sha, "{}", path.display());
    }
    assert_eq!(listing(dir.path()), before);
    assert_eq!(
        open(&dir.path().join("missing.anki2")).unwrap_err(),
        OpenError::NotFound
    );
}

#[test]
fn a_hot_rollback_journal_is_rolled_back_before_the_version_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let crashed = dir.path().join("crashed");
    fs::create_dir(&crashed).unwrap();
    let live = dir.path().join("live.anki2");
    let conn = plain(&live);
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         CREATE TABLE col (id integer PRIMARY KEY, ver integer NOT NULL);
         INSERT INTO col VALUES (1, 10);
         CREATE TABLE filler (data blob);",
    )
    .unwrap();
    // A writer that crashes in the middle of a transaction: `ver` changed and
    // enough pages written to spill the cache into the file, so the journal
    // is hot. Copying the file and its journal now is what a crash leaves.
    conn.execute_batch(
        "PRAGMA cache_size = 2;
         BEGIN;
         UPDATE col SET ver = 18;
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 400)
         INSERT INTO filler SELECT zeroblob(2000) FROM n;",
    )
    .unwrap();
    let path = crashed.join("collection.anki2");
    let journal = crashed.join("collection.anki2-journal");
    fs::copy(&live, &path).unwrap();
    fs::copy(dir.path().join("live.anki2-journal"), &journal).unwrap();
    conn.execute_batch("ROLLBACK").unwrap();
    assert!(fs::metadata(&journal).unwrap().len() > 0);

    // Read without the rollback, the file would claim version 18.
    assert_eq!(open(&path).unwrap_err(), OpenError::TooOld { version: 10 });
    assert!(!journal.exists(), "the rollback removes the journal");
}
