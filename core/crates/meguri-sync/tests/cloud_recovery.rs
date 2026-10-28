//! V2.1, V3 durable transactions and V8.1 nondestructive open outcomes.
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    faultpoint::{self, Fault},
    rng::SeededRandom,
};
use meguri_sync::cloud::*;
use std::{fs, io, path::Path};

fn scope() -> BuildScope {
    BuildScope {
        container: ContainerId::new("test.container").unwrap(),
        environment: Environment::Development,
    }
}

fn clock(ms: i64) -> FixedClock {
    FixedClock::new(ms, UtcOffset::UTC)
}

fn open(directory: &Path) -> TrackingStore {
    match TrackingStore::open(directory, scope()).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
        _ => panic!("expected supported tracking file"),
    }
}

fn binding(store: &mut TrackingStore) -> Binding {
    store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(b"private-fixture-account".to_vec()),
                epoch: EpochId([13; 16]),
                record_key: RecordKey([27; 32]),
                publication: serde_json::from_str("[17,29]").unwrap(),
                state: BindingState::Active,
            },
            &clock(100),
            &SeededRandom::new(9),
        )
        .unwrap()
}

fn record(name: &str) -> InboxEntry {
    InboxEntry {
        record_type: RecordType::Entity,
        record_name: name.into(),
        value: InboxValue::Record {
            erev: 4,
            system_fields: OpaqueBytes(vec![0, 255]),
            envelope: OpaqueBytes(b"private-envelope-fixture".to_vec()),
            staged_asset: None,
        },
        reason: InboxReason::Ready,
        source: InboxSource::Fetch,
        absence_pass: None,
        retry_count: 0,
        fetched_at_ms: 200,
    }
}

fn files(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn missing_tracking_creates_unbound_store_and_never_recovers_server_facts() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("collection.anki2"), b"unchanged-local-collection").unwrap();
    let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
        OpenOutcome::Missing(store) => store,
        _ => panic!("expected missing outcome"),
    };
    assert_eq!(store.selected_binding().unwrap(), None);
    drop(store);
    let store = open(directory.path());
    assert_eq!(store.selected_binding().unwrap(), None);
    assert_eq!(store.engine_state(BindingId([0; 16])), Err(Error::BindingNotFound));
    assert_eq!(
        fs::read(directory.path().join("collection.anki2")).unwrap(),
        b"unchanged-local-collection"
    );
    drop(store);
    let connection = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    for table in ["bindings", "inbox", "engine_state"] {
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn before_commit_fault_rolls_back_inbox_and_updated_cursor_on_reopened_file() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = open(directory.path());
    let binding = binding(&mut store);
    let prior = OpaqueBytes(vec![1, 2, 3]);
    store
        .commit_fetched(binding.id, &[record("e-before")], &prior, &clock(300))
        .unwrap();
    let inbox_before = store.inbox(binding.id).unwrap();
    faultpoint::arm(
        "cloud.fetched.before_commit",
        Fault::IoError(io::ErrorKind::StorageFull),
    );
    let result = store.commit_fetched(
        binding.id,
        &[record("e-uncommitted")],
        &OpaqueBytes(vec![99]),
        &clock(400),
    );
    faultpoint::disarm("cloud.fetched.before_commit");
    assert_eq!(result, Err(Error::Io(io::ErrorKind::StorageFull)));
    drop(store);
    let mut store = open(directory.path());
    assert_eq!(store.inbox(binding.id).unwrap(), inbox_before);
    assert_eq!(
        store.engine_state(binding.id).unwrap(),
        Some(EngineState {
            serialization: prior,
            saved_at_ms: 300
        })
    );
    store
        .commit_fetched(binding.id, &[record("e-later")], &OpaqueBytes(vec![4]), &clock(500))
        .unwrap();
    drop(store);
    assert_eq!(open(directory.path()).inbox(binding.id).unwrap().len(), 2);
}

#[test]
fn sqlite_constraint_after_inbox_write_rolls_back_whole_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = open(directory.path());
    let binding = binding(&mut store);
    let prior = OpaqueBytes(vec![6]);
    store
        .commit_fetched(binding.id, &[record("e-before")], &prior, &clock(300))
        .unwrap();
    let inbox_before = store.inbox(binding.id).unwrap();
    let database = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    // RAISE(FAIL) leaves earlier statements intact inside the SQL transaction.
    // Only the store's real transaction rollback removes the preceding inbox
    // insert. SQLite's error message intentionally contains private fixtures.
    database
        .execute_batch(
            "CREATE TRIGGER reject_state BEFORE UPDATE ON engine_state BEGIN
        SELECT RAISE(FAIL,'private-fixture-account private-envelope-fixture'); END;",
        )
        .unwrap();
    let error = store
        .commit_fetched(binding.id, &[record("e-failed")], &OpaqueBytes(vec![7]), &clock(400))
        .unwrap_err();
    assert_eq!(error, Error::Constraint);
    assert!(!format!("{error} {error:?}").contains("private-fixture"));
    database.execute_batch("DROP TRIGGER reject_state").unwrap();
    drop((database, store));
    let store = open(directory.path());
    assert_eq!(store.inbox(binding.id).unwrap(), inbox_before);
    assert_eq!(
        store.engine_state(binding.id).unwrap(),
        Some(EngineState {
            serialization: prior,
            saved_at_ms: 300
        })
    );
}

#[test]
fn newer_schema_committed_only_in_wal_preserves_every_original_byte_and_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = open(directory.path());
    let binding = binding(&mut store);
    store
        .commit_fetched(
            binding.id,
            &[record("e-preserved")],
            &OpaqueBytes(vec![81]),
            &clock(300),
        )
        .unwrap();
    drop(store);
    let database = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    database
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=2;")
        .unwrap();
    let main = fs::read(directory.path().join(TRACKING_FILE)).unwrap();
    // The old version is still in the main file; only a private WAL-aware
    // probe can see that this is a newer database.
    assert_eq!(&main[60..64], &1_u32.to_be_bytes());
    assert!(directory.path().join("meguri-cloud.db-wal").exists());
    assert!(directory.path().join("meguri-cloud.db-shm").exists());
    let before = files(directory.path());
    for _ in 0..2 {
        assert!(matches!(
            TrackingStore::open(directory.path(), scope()).unwrap(),
            OpenOutcome::NewerSchema { found: 2 }
        ));
        assert_eq!(files(directory.path()), before);
    }
    // Keep the source connection alive until after the byte comparisons: its
    // own close may checkpoint, which is not a tracking-store action.
    drop(database);
}

#[test]
fn damaged_file_and_sidecars_are_not_moved_deleted_or_repaired() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join(TRACKING_FILE), b"not a sqlite database").unwrap();
    fs::write(directory.path().join("meguri-cloud.db-wal"), b"damaged-wal").unwrap();
    fs::write(directory.path().join("meguri-cloud.db-shm"), b"preserved-shm").unwrap();
    let before = files(directory.path());
    assert!(matches!(
        TrackingStore::open(directory.path(), scope()).unwrap(),
        OpenOutcome::Damaged
    ));
    assert_eq!(files(directory.path()), before);
}

#[test]
fn unreadable_required_table_is_damaged_and_original_stays_untouched() {
    let directory = tempfile::tempdir().unwrap();
    drop(open(directory.path()));
    let database = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    database.execute_batch("DROP TABLE inbox").unwrap();
    drop(database);
    let before = files(directory.path());
    assert!(matches!(
        TrackingStore::open(directory.path(), scope()).unwrap(),
        OpenOutcome::Damaged
    ));
    assert_eq!(files(directory.path()), before);
}
