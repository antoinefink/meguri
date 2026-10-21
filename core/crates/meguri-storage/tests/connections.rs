//! The writer and the role readers (`storage/open-and-connections` C4 to C7,
//! C9, C10; R1451, R1452, R1454).

mod common;

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{copy_fixture, exec, int, listing, open, query, sha256, text};
use meguri_model::faultpoint::{self, Fault};
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::SqlValue;
use meguri_storage::{Initializers, OpenError, OpenOptions, ReaderRole, StorageError, open_collection};

/// `col.mod`, `scm`, `usn` and `ls`, read with a plain connection that
/// changes nothing (`immutable=1`).
fn col_stamps(path: &std::path::Path) -> Vec<Vec<SqlValue>> {
    use meguri_storage::rusqlite::{Connection, OpenFlags};
    let uri = format!("file:{}?mode=ro&immutable=1", path.display());
    let conn = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).unwrap();
    let row: [i64; 4] = conn
        .query_row("SELECT mod, scm, usn, ls FROM col", [], |r| {
            Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?])
        })
        .unwrap();
    vec![row.iter().map(|v| SqlValue::Integer(*v)).collect()]
}

#[test]
fn the_writer_has_the_settings_of_c4_and_c9() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let col_before = col_stamps(&path);
    let opened = open(&path).unwrap();
    let w = opened.writer();
    assert_eq!(text(&exec(w, "SELECT * FROM pragma_journal_mode")), "wal");
    assert_eq!(text(&exec(w, "PRAGMA locking_mode")), "normal");
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_busy_timeout")), 1000);
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_page_size")), 4096);
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_cache_size")), -40960);
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_temp_store")), 2, "MEMORY");
    // C9: NORMAL, with full syncs on Apple whatever `synchronous` is (the
    // checkpoint and WAL-restart syncs SQLite still issues).
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_synchronous")), 1, "NORMAL");
    assert_eq!(int(&exec(w, "PRAGMA wal_autocheckpoint")), 1000);
    let apple = cfg!(target_vendor = "apple") as i64;
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_fullfsync")), apple);
    assert_eq!(int(&exec(w, "SELECT * FROM pragma_checkpoint_fullfsync")), apple);
    // The open changed nothing in col (C2.6).
    assert_eq!(exec(w, "SELECT mod, scm, usn, ls FROM col").rows, col_before);
    opened.close().unwrap();
}

#[cfg(unix)]
#[test]
fn counting_vfs_records_a_real_full_wal_sync_through_a_directory_symlink() {
    use meguri_storage::counting_vfs::{CountingVfs, clock};
    use meguri_storage::rusqlite::ffi;
    use std::os::unix::fs::symlink;

    let (directory, path) = copy_fixture("schema-18.anki2");
    let physical = path.canonicalize().unwrap();
    let alias = directory.path().join("alias");
    symlink(physical.parent().unwrap(), &alias).unwrap();
    let alias_path = alias.join("collection.anki2");
    assert_ne!(alias_path, physical);
    assert_eq!(alias_path.canonicalize().unwrap(), physical);
    let probe = CountingVfs::for_path(&physical);

    // A different database in the same aliased directory stays unobserved.
    let unrelated = alias.join("other.anki2");
    fs::copy(&physical, &unrelated).unwrap();
    let other = open(&unrelated).unwrap();
    exec(other.writer(), "UPDATE cards SET flags = 2");
    assert!(other.writer().sync_wal().unwrap());
    other.close().unwrap();
    assert!(probe.records().is_empty());

    let opened = open(&alias_path).unwrap();
    exec(opened.writer(), "UPDATE cards SET flags = 3");
    let started = clock::now();
    assert!(opened.writer().sync_wal().unwrap());
    let records = probe.records();
    let syncs: Vec<_> = records.iter().filter(|record| record.started >= started).collect();
    assert_eq!(syncs.len(), 1, "{records:?}");
    let record = syncs[0];
    // Resolve the observed filename only for this comparison; the observer
    // must retain exactly the filename supplied by SQLite, including aliases.
    assert_eq!(
        std::path::Path::new(&record.filename).canonicalize().unwrap(),
        physical.with_extension("anki2-wal"),
        "{record:?}"
    );
    assert_ne!(record.open_flags & ffi::SQLITE_OPEN_WAL, 0, "{record:?}");
    assert_eq!(record.flags & 0x0f, ffi::SQLITE_SYNC_FULL, "{record:?}");
    assert_eq!(record.result, ffi::SQLITE_OK, "{record:?}");
    assert!(record.completed >= record.started, "{record:?}");
    opened.close().unwrap();
}

#[test]
fn each_reader_role_has_its_settings() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    for role in ReaderRole::ALL {
        let reader = opened.open_reader(role).unwrap();
        assert_eq!(reader.role(), role);
        let query_only = int(&query(&reader, "SELECT * FROM pragma_query_only"));
        let expected = if matches!(role, ReaderRole::Sync | ReaderRole::Maintenance) {
            0
        } else {
            1
        };
        assert_eq!(query_only, expected, "{role:?}");
        assert_eq!(
            int(&query(&reader, "SELECT * FROM pragma_busy_timeout")),
            250,
            "{role:?}"
        );
        assert_eq!(
            int(&query(&reader, "SELECT * FROM pragma_cache_size")),
            -40960,
            "{role:?}"
        );
        assert_eq!(int(&query(&reader, "SELECT * FROM pragma_temp_store")), 2, "{role:?}");
        // The collation is registered on every reader.
        assert_eq!(int(&query(&reader, "SELECT 'Straße' = 'STRASSE' COLLATE unicase")), 1);
    }
    opened.close().unwrap();
}

#[test]
fn a_reader_keeps_its_snapshot_while_the_writer_commits() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    let reader = opened.open_reader(ReaderRole::Interactive).unwrap();
    let count = "SELECT count(*) FROM graves";
    let before = int(&query(&reader, count));
    query(&reader, "BEGIN");
    assert_eq!(int(&query(&reader, count)), before);
    exec(opened.writer(), "INSERT INTO graves VALUES (424242, 0, -1)");
    assert_eq!(int(&query(&reader, count)), before, "the reader's snapshot moved");
    query(&reader, "COMMIT");
    assert_eq!(int(&query(&reader, count)), before + 1);
    drop(reader);
    opened.close().unwrap();
}

#[test]
fn readers_never_see_busy_while_the_writer_commits() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    let readers: Vec<_> = [ReaderRole::Query, ReaderRole::Search, ReaderRole::Sync]
        .into_iter()
        .map(|role| opened.open_reader(role).unwrap())
        .collect();
    let done = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = readers
        .into_iter()
        .map(|reader| {
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                let mut reads = 0;
                while done.load(Ordering::SeqCst) == 0 || reads < 50 {
                    reader
                        .execute_raw("SELECT count(*), sum(oid) FROM graves", &[])
                        .unwrap_or_else(|e| panic!("{:?}: {e}", reader.role()));
                    reads += 1;
                }
                reads
            })
        })
        .collect();
    for i in 0..300_i64 {
        opened
            .writer()
            .execute_raw(
                "INSERT INTO graves VALUES (?1, 1, -1)",
                &[SqlValue::Integer(1_000_000 + i)],
            )
            .unwrap();
    }
    done.store(1, Ordering::SeqCst);
    for thread in threads {
        assert!(thread.join().unwrap() >= 50);
    }
    opened.close().unwrap();
}

#[test]
fn vacuum_into_works_on_the_sync_and_maintenance_readers_and_writes_fail_on_every_reader() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    for role in ReaderRole::ALL {
        let reader = opened.open_reader(role).unwrap();
        for write in [
            "UPDATE cards SET flags = 1",
            "INSERT INTO graves VALUES (1, 0, 0)",
            "CREATE TABLE extra (x)",
        ] {
            let error = reader.execute_raw(write, &[]).unwrap_err();
            assert!(
                matches!(error, StorageError::Sqlite { .. }),
                "{role:?}: {write}: {error:?}"
            );
        }
        if role.query_only() {
            reader.execute_raw("CREATE TEMP TABLE scratch (x)", &[]).unwrap_err();
        }
        let target = dir.path().join(format!("{role:?}.vacuumed"));
        let result = reader.execute_raw("VACUUM INTO ?1", &[SqlValue::Text(target.display().to_string())]);
        if matches!(role, ReaderRole::Sync | ReaderRole::Maintenance) {
            result.unwrap_or_else(|e| panic!("{role:?}: {e}"));
            assert!(target.exists());
        } else {
            result.unwrap_err();
        }
    }
    assert_eq!(
        int(&exec(opened.writer(), "SELECT count(*) FROM cards WHERE flags = 1")),
        0
    );
    opened.close().unwrap();
}

#[test]
fn a_collation_registration_failure_fails_the_open() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let sha = sha256(&path);
    let before = listing(dir.path());
    faultpoint::arm("storage.conn_init.unicase", Fault::IoError(std::io::ErrorKind::Other));
    let error = open(&path).unwrap_err();
    faultpoint::disarm_all();
    assert!(matches!(error, OpenError::ConnectionInit { .. }), "{error:?}");
    assert_eq!(sha256(&path), sha);
    assert_eq!(listing(dir.path()), before);
}

#[test]
fn initializers_run_on_the_writer_and_every_reader_and_a_failing_one_fails_the_open() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let clock = common::clock();
    let random = SeededRandom::new(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut initializers = Initializers::new();
    let counter = Arc::clone(&calls);
    initializers.register(move |conn| {
        counter.fetch_add(1, Ordering::SeqCst);
        conn.create_scalar_function(
            "meguri_test_double",
            1,
            meguri_storage::rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | meguri_storage::rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
                | meguri_storage::rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
            |ctx| Ok(ctx.get::<i64>(0)? * 2),
        )
    });
    let opened = open_collection(&path, &OpenOptions::new(&clock, &random).initializers(initializers)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(int(&exec(opened.writer(), "SELECT meguri_test_double(21)")), 42);
    let reader = opened.open_reader(ReaderRole::Query).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(int(&query(&reader, "SELECT meguri_test_double(4)")), 8);
    drop(reader);
    opened.close().unwrap();

    let mut failing = Initializers::new();
    failing.register(|_| Err(meguri_storage::rusqlite::Error::InvalidQuery));
    let error = open_collection(&path, &OpenOptions::new(&clock, &random).initializers(failing)).unwrap_err();
    assert!(matches!(error, OpenError::ConnectionInit { .. }), "{error:?}");
}

#[test]
fn the_checkpoint_refuses_inside_a_transaction_and_reports_a_busy_reader() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    let w = opened.writer();
    exec(w, "BEGIN IMMEDIATE");
    assert_eq!(w.checkpoint(), Err(StorageError::TransactionOpen));
    exec(w, "COMMIT");

    let reader = opened.open_reader(ReaderRole::Maintenance).unwrap();
    query(&reader, "BEGIN");
    query(&reader, "SELECT count(*) FROM cards");
    exec(w, "UPDATE cards SET mod = mod + 1");
    let busy = w.checkpoint().unwrap();
    assert!(busy.busy && !busy.succeeded(), "{busy:?}");
    query(&reader, "COMMIT");
    let done = w.checkpoint().unwrap();
    assert!(done.succeeded(), "{done:?}");
    assert_eq!(fs::metadata(dir.path().join("collection.anki2-wal")).unwrap().len(), 0);
    drop(reader);
    opened.close().unwrap();
}

#[test]
fn after_close_the_main_file_alone_holds_the_collection() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    exec(opened.writer(), "UPDATE cards SET flags = 3");
    let reader = opened.open_reader(ReaderRole::Interactive).unwrap();
    query(&reader, "SELECT count(*) FROM cards");
    drop(reader);
    let outcome = opened.close().unwrap();
    assert!(outcome.succeeded(), "{outcome:?}");
    assert_eq!(
        listing(dir.path()).into_iter().collect::<Vec<_>>(),
        ["collection.anki2"]
    );

    // Only the main file, elsewhere: the write is in it.
    let elsewhere = tempfile::tempdir().unwrap();
    let moved = elsewhere.path().join("collection.anki2");
    fs::copy(&path, &moved).unwrap();
    let reopened = open(&moved).unwrap();
    assert_eq!(
        int(&exec(reopened.writer(), "SELECT count(*) FROM cards WHERE flags != 3")),
        0
    );
    reopened.close().unwrap();
}

#[test]
fn raw_rows_give_each_value_by_its_storage_class() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    let sql = "SELECT NULL, 7, 1.5, 'é', X'00ff', CAST(X'61ff' AS TEXT)";
    // TEXT that is not UTF-8 comes back with the replacement character.
    let expected = vec![vec![
        SqlValue::Null,
        SqlValue::Integer(7),
        SqlValue::Real(1.5),
        SqlValue::Text("é".to_owned()),
        SqlValue::Blob(vec![0x00, 0xff]),
        SqlValue::Text("a\u{fffd}".to_owned()),
    ]];
    assert_eq!(exec(opened.writer(), sql).rows, expected);
    let reader = opened.open_reader(ReaderRole::Query).unwrap();
    assert_eq!(query(&reader, sql).rows, expected);
    drop(reader);
    opened.close().unwrap();
}

#[test]
fn c12_open_rejects_persistent_and_restored_path_swaps_before_setup() {
    use meguri_storage::connections::{WriterOpenHook, WriterOpenStage};
    use std::os::unix::fs::MetadataExt;

    for put_original_back in [false, true] {
        let (directory, path) = copy_fixture("schema-18.anki2");
        let candidate = directory.path().join("candidate.anki2");
        let parked = directory.path().join("parked.anki2");
        fs::copy(&path, &candidate).unwrap();
        {
            let connection = meguri_storage::rusqlite::Connection::open(&candidate).unwrap();
            connection.execute_batch("UPDATE col SET mod = mod + 42").unwrap();
        }
        let file_id = |file: &std::path::Path| {
            let metadata = fs::metadata(file).unwrap();
            (metadata.dev(), metadata.ino())
        };
        let first_id = file_id(&path);
        let second_id = file_id(&candidate);
        assert_ne!(first_id, second_id);
        let hashes = (sha256(&path), sha256(&candidate));
        let live = path.clone();
        let saved = parked.clone();
        let second = candidate.clone();
        let barrier = WriterOpenHook::for_path(
            &path,
            Arc::new(move |stage| match stage {
                WriterOpenStage::Retained => {
                    fs::rename(&live, &saved).unwrap();
                    fs::rename(&second, &live).unwrap();
                }
                WriterOpenStage::SqliteOpened => {
                    assert_eq!(file_id(&live), second_id);
                    if put_original_back {
                        fs::rename(&live, &second).unwrap();
                        fs::rename(&saved, &live).unwrap();
                        assert_eq!(file_id(&live), first_id);
                    }
                }
            }),
        );
        let setup_count = Arc::new(AtomicUsize::new(0));
        let observed = setup_count.clone();
        let mut initializers = Initializers::new();
        initializers.register(move |connection| {
            observed.fetch_add(1, Ordering::SeqCst);
            connection.execute_batch("UPDATE col SET mod = mod + 1000")
        });
        let clock = common::clock();
        let random = SeededRandom::new(12);
        let failure =
            open_collection(&path, &OpenOptions::new(&clock, &random).initializers(initializers)).unwrap_err();
        drop(barrier);
        if !put_original_back {
            fs::rename(&path, &candidate).unwrap();
            fs::rename(&parked, &path).unwrap();
        }
        assert!(matches!(failure, OpenError::Io { .. }), "{failure:?}");
        if put_original_back {
            assert!(
                failure.to_string().contains("SQLite file identity check failed"),
                "the retained file and restored path agree, but SQLite opened the other file: {failure}"
            );
        }
        assert_eq!(setup_count.load(Ordering::SeqCst), 0);
        assert_eq!((sha256(&path), sha256(&candidate)), hashes);
        for file in [&path, &candidate] {
            for suffix in ["-journal", "-wal", "-shm"] {
                let mut name = file.as_os_str().to_owned();
                name.push(suffix);
                assert!(!std::path::PathBuf::from(name).exists());
            }
        }
    }
}

#[test]
fn c12_open_refuses_unsupported_failed_or_nonzero_file_control_results() {
    use meguri_storage::{counting_vfs::CountingVfs, rusqlite::ffi};

    let refusals = [
        (ffi::SQLITE_NOTFOUND, 0),
        (ffi::SQLITE_IOERR, 0),
        (ffi::SQLITE_OK, 1),
        (ffi::SQLITE_OK, -1),
    ];
    for reply in refusals {
        let (_directory, path) = copy_fixture("schema-18.anki2");
        let original_hash = sha256(&path);
        let probe = CountingVfs::for_path(&path);
        probe.has_moved_result(reply.0, reply.1);
        let setup_count = Arc::new(AtomicUsize::new(0));
        let observed = setup_count.clone();
        let mut initializers = Initializers::new();
        initializers.register(move |connection| {
            observed.fetch_add(1, Ordering::SeqCst);
            connection.execute_batch("UPDATE col SET mod = mod + 1000")
        });
        let clock = common::clock();
        let random = SeededRandom::new(12);
        let failure =
            open_collection(&path, &OpenOptions::new(&clock, &random).initializers(initializers)).unwrap_err();
        assert!(matches!(failure, OpenError::Io { .. }), "{reply:?}: {failure:?}");
        assert!(
            failure.to_string().contains("SQLite file identity check failed"),
            "{reply:?}: {failure}"
        );
        assert_eq!(setup_count.load(Ordering::SeqCst), 0, "{reply:?}");
        assert_eq!(sha256(&path), original_hash, "{reply:?}");
        assert!(probe.records().is_empty(), "{reply:?}: unexpected sync");
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            assert!(!std::path::PathBuf::from(name).exists());
        }
    }
}

#[test]
fn c12_wal_and_two_reopens_keep_native_identity_config_and_schema() {
    use std::os::unix::fs::MetadataExt;

    let (_directory, path) = copy_fixture("schema-18.anki2");
    let metadata = fs::metadata(&path).unwrap();
    let expected_id = (metadata.dev(), metadata.ino());
    let mut expected_catalog = None;
    for opening in 0..3 {
        let collection = open(&path).unwrap();
        let writer = collection.writer();
        assert_eq!(writer.native_identity().unwrap(), expected_id);
        assert_eq!(text(&exec(writer, "PRAGMA journal_mode")), "wal");
        let catalog = (
            exec(writer, "SELECT * FROM config ORDER BY key").rows,
            exec(
                writer,
                "SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name",
            )
            .rows,
        );
        match &expected_catalog {
            Some(expected) => assert_eq!(&catalog, expected),
            None => expected_catalog = Some(catalog),
        }
        if opening == 0 {
            exec(writer, "UPDATE cards SET flags = 3");
            writer.sync_wal().unwrap();
            assert_eq!(writer.native_identity().unwrap(), expected_id);
        }
        assert_eq!(int(&exec(writer, "SELECT count(*) FROM cards WHERE flags != 3")), 0);
        collection.close().unwrap();
    }
}
