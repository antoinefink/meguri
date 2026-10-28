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
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=3;")
        .unwrap();
    let main = fs::read(directory.path().join(TRACKING_FILE)).unwrap();
    // The old version is still in the main file; only a private WAL-aware
    // probe can see that this is a newer database.
    assert_eq!(&main[60..64], &2_u32.to_be_bytes());
    assert!(directory.path().join("meguri-cloud.db-wal").exists());
    assert!(directory.path().join("meguri-cloud.db-shm").exists());
    let before = files(directory.path());
    for _ in 0..2 {
        assert!(matches!(
            TrackingStore::open(directory.path(), scope()).unwrap(),
            OpenOutcome::NewerSchema { found: 3 }
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

// V8.1 storage-only recovery. These tests exercise actual SQLite files and
// directory transitions; they are not the F3 full power-loss shim/U250 lane.
fn copy(id: u8) -> ConflictCopy {
    ConflictCopy {
        id: CopyId([id; 16]),
        kind: ConflictKind::Note,
        key: OpaqueBytes(vec![id]),
        reason: ConflictReason::FieldConflict,
        losing_envelope: OpaqueBytes(vec![0, 255, id]),
        losing_payload_digest: PayloadDigest([id; 32]),
        winning_payload_digest: PayloadDigest([99; 32]),
        field_names: Some(vec!["Question".into(), "Answer".into()]),
        template_names: Some(vec!["Reverse".into()]),
        note_deck: Some(77),
        staged_path: Some("cloud-staging/preserved".into()),
        created_at_ms: 100,
        resolved_at_ms: Some(200),
    }
}

fn recover(directory: &Path, random: &SeededRandom) -> TrackingStore {
    match TrackingStore::recover(directory, scope(), &clock(1_793_145_600_000), random).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
        _ => panic!("expected completed recovery"),
    }
}

fn damage_required_table(directory: &Path) {
    let db = rusqlite::Connection::open(directory.join(TRACKING_FILE)).unwrap();
    db.execute_batch("DROP TABLE inbox").unwrap();
}

#[test]
fn salvage_all_scopes_bindings_resolved_rows_and_never_server_facts() {
    let dir = tempfile::tempdir().unwrap();
    let random = SeededRandom::new(412);
    let mut a = open(dir.path());
    let old_a = binding(&mut a);
    a.store_conflict(old_a.id, &copy(1)).unwrap();
    a.commit_fetched(old_a.id, &[record("never-salvaged")], &OpaqueBytes(vec![42]), &clock(5))
        .unwrap();
    let mut prior_account = old_a.facts.clone();
    prior_account.account = OpaqueBytes(vec![9]);
    prior_account.epoch = EpochId([45; 16]);
    prior_account.state = BindingState::Isolated(IsolationReason::AccountChanged);
    let old_c = a.create_binding(prior_account, &clock(4), &random).unwrap();
    a.store_conflict(old_c.id, &copy(3)).unwrap();
    drop(a);
    let other_scope = BuildScope {
        container: ContainerId::new("other.container").unwrap(),
        environment: Environment::Production,
    };
    let mut b = match TrackingStore::open(dir.path(), other_scope.clone()).unwrap() {
        OpenOutcome::Opened(s) => s,
        _ => panic!(),
    };
    let mut facts = old_a.facts.clone();
    facts.account = OpaqueBytes(vec![8]);
    facts.epoch = EpochId([44; 16]);
    let old_b = b.create_binding(facts, &clock(6), &random).unwrap();
    b.store_conflict(old_b.id, &copy(2)).unwrap();
    drop(b);
    damage_required_table(dir.path());
    let store = recover(dir.path(), &random);
    assert_eq!(store.selected_binding().unwrap(), None);
    let groups = store.salvage_bindings().unwrap();
    assert_eq!(groups.len(), 3);
    for old in [&old_a, &old_b, &old_c] {
        let group = groups.iter().find(|g| g.original_binding == Some(old.id)).unwrap();
        assert_eq!(group.container.as_ref(), Some(&old.scope.container));
        assert_eq!(group.environment, Some(old.scope.environment));
        assert_eq!(group.account.as_ref(), Some(&old.facts.account));
        assert_eq!(group.epoch, Some(old.facts.epoch));
        assert_eq!(store.binding(group.id).unwrap(), None);
        assert_eq!(store.engine_state(group.id), Err(Error::BindingNotFound));
    }
    let copies = store.conflict_copies().unwrap();
    assert_eq!(copies.len(), 3);
    for expected in [copy(1), copy(2), copy(3)] {
        assert!(copies.iter().any(|r| r.copy == expected));
    }
    assert!(store.retained_damaged_sources().unwrap().is_empty());
    drop(store);
    assert_eq!(recover(dir.path(), &random).conflict_copies().unwrap(), copies);
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    for table in ["bindings", "engine_state", "inbox"] {
        let count: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn partial_unreadable_rows_keep_source_and_attention_forever_without_resalvage() {
    let dir = tempfile::tempdir().unwrap();
    let random = SeededRandom::new(500);
    let mut store = open(dir.path());
    let old = binding(&mut store);
    for id in 1..=3 {
        store.store_conflict(old.id, &copy(id)).unwrap();
    }
    drop(store);
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch("PRAGMA ignore_check_constraints=ON;").unwrap();
    assert_eq!(
        db.execute(
            "UPDATE conflicts SET content='unreadable' WHERE id=?1",
            [copy(2).id.0.as_slice()],
        )
        .unwrap(),
        1
    );
    let content: String = db
        .query_row(
            "SELECT content FROM conflicts WHERE id=?1",
            [copy(2).id.0.as_slice()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(content, "unreadable");
    db.execute_batch("DROP TABLE inbox;").unwrap();
    drop(db);
    let store = recover(dir.path(), &random);
    assert_eq!(store.conflict_copies().unwrap().len(), 2);
    let kept = store.retained_damaged_sources().unwrap();
    assert_eq!(kept.len(), 1);
    assert!(!kept[0].ends_with(".salvaging"));
    drop(store);
    let before = fs::read(dir.path().join(&kept[0])).unwrap();
    for _ in 0..2 {
        let store = recover(dir.path(), &random);
        assert_eq!(store.retained_damaged_sources().unwrap(), kept);
        assert_eq!(store.conflict_copies().unwrap().len(), 2);
        assert_eq!(store.salvage_bindings().unwrap().len(), 1);
        drop(store);
        assert_eq!(fs::read(dir.path().join(&kept[0])).unwrap(), before);
    }
}

#[test]
fn unknown_binding_metadata_stays_absent_and_unknown_groups_stay_per_source() {
    let dir = tempfile::tempdir().unwrap();
    let random = SeededRandom::new(600);
    let mut store = open(dir.path());
    let old = binding(&mut store);
    for id in 1..=3 {
        store.store_conflict(old.id, &copy(id)).unwrap();
    }
    drop(store);
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE conflicts SET binding=x'00' WHERE id<>x'01010101010101010101010101010101'; UPDATE bindings SET facts='{}'; DROP TABLE inbox;").unwrap();
    drop(db);
    let store = recover(dir.path(), &random);
    assert_eq!(store.conflict_copies().unwrap().len(), 3);
    let groups = store.salvage_bindings().unwrap();
    assert_eq!(groups.len(), 2);
    let known = groups.iter().find(|g| g.original_binding == Some(old.id)).unwrap();
    assert_eq!(known.container.as_ref(), Some(&old.scope.container));
    assert_eq!(known.epoch, None);
    assert_eq!(known.account, None);
    let unknown = groups.iter().find(|g| g.original_binding.is_none()).unwrap();
    assert_eq!(unknown.container, None);
    assert_eq!(unknown.environment, None);
    assert_eq!(unknown.account, None);
    assert_eq!(unknown.epoch, None);
}

#[test]
fn destination_commit_failure_preserves_source_and_rolls_back_groups() {
    let dir = tempfile::tempdir().unwrap();
    let random = SeededRandom::new(700);
    let mut store = open(dir.path());
    let old = binding(&mut store);
    store.store_conflict(old.id, &copy(1)).unwrap();
    drop(store);
    // Completed source beside a healthy destination, with a real SQLite
    // trigger failing the destination transaction after its group insert.
    let source = "meguri-cloud.db.damaged-2026-10-01-00.00.00.salvaging";
    fs::rename(dir.path().join(TRACKING_FILE), dir.path().join(source)).unwrap();
    let empty = tempfile::tempdir().unwrap();
    drop(open(empty.path()));
    fs::copy(empty.path().join(TRACKING_FILE), dir.path().join(TRACKING_FILE)).unwrap();
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_copy BEFORE INSERT ON conflicts BEGIN SELECT RAISE(FAIL,'copy refused'); END;",
    )
    .unwrap();
    drop(db);
    let before = fs::read(dir.path().join(source)).unwrap();
    assert!(matches!(
        TrackingStore::recover(dir.path(), scope(), &clock(3), &random),
        Err(Error::Constraint)
    ));
    assert_eq!(fs::read(dir.path().join(source)).unwrap(), before);
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    let groups: i64 = db
        .query_row("SELECT count(*) FROM salvage_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(groups, 0);
    db.execute_batch("DROP TRIGGER fail_copy").unwrap();
    drop(db);
    assert_eq!(recover(dir.path(), &random).conflict_copies().unwrap().len(), 1);
}

#[test]
fn v1_migration_preserves_original_binding_inbox_and_engine_serialization() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let old = binding(&mut store);
    store
        .commit_fetched(old.id, &[record("old-inbox")], &OpaqueBytes(vec![6, 7]), &clock(88))
        .unwrap();
    let inbox = store.inbox(old.id).unwrap();
    let state = store.engine_state(old.id).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    // The v2 migration is additive; these are exactly the original v1 tables.
    db.execute_batch("DROP TABLE conflicts; DROP TABLE salvage_bindings; PRAGMA user_version=1;")
        .unwrap();
    drop(db);
    let store = open(dir.path());
    assert_eq!(store.binding(old.id).unwrap(), Some(old));
    assert_eq!(store.inbox(inbox_binding(&store)).unwrap(), inbox);
    assert_eq!(store.engine_state(inbox_binding(&store)).unwrap(), state);
    assert!(store.conflict_copies().unwrap().is_empty());
}

fn inbox_binding(store: &TrackingStore) -> BindingId {
    store.selected_binding().unwrap().unwrap().id
}

#[test]
fn oldest_source_wins_global_id_dedup_and_empty_sources_make_no_groups() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let random = SeededRandom::new(800);
    let mut store = open(fixture.path());
    let old = binding(&mut store);
    store.store_conflict(old.id, &copy(1)).unwrap();
    drop(store);
    let oldest = "meguri-cloud.db.damaged-2026-01-01-00.00.00.salvaging";
    let newer = "meguri-cloud.db.damaged-2026-02-01-00.00.00.salvaging";
    fs::copy(fixture.path().join(TRACKING_FILE), dir.path().join(newer)).unwrap();
    fs::copy(fixture.path().join(TRACKING_FILE), dir.path().join(oldest)).unwrap();
    let empty = tempfile::tempdir().unwrap();
    drop(open(empty.path()));
    fs::copy(
        empty.path().join(TRACKING_FILE),
        dir.path().join("meguri-cloud.db.damaged-2026-03-01-00.00.00.salvaging"),
    )
    .unwrap();
    let store = recover(dir.path(), &random);
    let groups = store.salvage_bindings().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].source, oldest);
    assert_eq!(store.conflict_copies().unwrap()[0].copy, copy(1));
    for name in [oldest, newer] {
        assert!(!dir.path().join(name).exists());
    }
    drop(store);
    // A healthy destination's existing row wins even against older sources.
    let mut store = open(dir.path());
    let current = binding(&mut store);
    let mut distinct = copy(2);
    distinct.resolved_at_ms = None;
    store.store_conflict(current.id, &distinct).unwrap();
    let before = store.conflict_copies().unwrap();
    drop(store);
    let db = rusqlite::Connection::open(fixture.path().join(TRACKING_FILE)).unwrap();
    db.execute(
        "UPDATE conflicts SET id=?1,content=?2",
        rusqlite::params![distinct.id.0.as_slice(), serde_json::to_string(&distinct).unwrap()],
    )
    .unwrap();
    drop(db);
    fs::copy(fixture.path().join(TRACKING_FILE), dir.path().join(oldest)).unwrap();
    let store = recover(dir.path(), &random);
    assert_eq!(store.conflict_copies().unwrap(), before);
    assert_eq!(store.salvage_bindings().unwrap().len(), 1);
}

fn wal_damage_fixture(directory: &Path) {
    // Copy a quiescent WAL-aware image while its owner keeps WAL deletion off
    // the copied paths. No connection ever opens those paths before recovery.
    let fixture = tempfile::tempdir().unwrap();
    let mut store = open(fixture.path());
    let old = binding(&mut store);
    for id in 1..=3 {
        let mut c = copy(id);
        if id != 1 {
            c.resolved_at_ms = None;
        }
        store.store_conflict(old.id, &c).unwrap();
    }
    drop(store);
    let db = rusqlite::Connection::open(fixture.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; DROP TABLE inbox;")
        .unwrap();
    for suffix in ["", "-wal"] {
        fs::copy(
            fixture.path().join(format!("{TRACKING_FILE}{suffix}")),
            directory.join(format!("{TRACKING_FILE}{suffix}")),
        )
        .unwrap();
    }
    assert!(fs::metadata(directory.join("meguri-cloud.db-wal")).unwrap().len() > 32);
}

#[test]
fn recovery_sigabrt_child() {
    let Ok(directory) = std::env::var("MEGURI_CLOUD_RECOVERY_CHILD") else {
        return;
    };
    let point = std::env::var("MEGURI_CLOUD_RECOVERY_POINT").unwrap();
    faultpoint::arm(&point, Fault::Abort);
    let _ = TrackingStore::recover(
        Path::new(&directory),
        scope(),
        &clock(1_793_145_600_000),
        &SeededRandom::new(901),
    );
    panic!("armed recovery boundary was not reached");
}

#[test]
fn actual_sigabrt_at_each_v8_boundary_preserves_three_copies_once_with_real_wal() {
    use std::{os::unix::process::ExitStatusExt, process::Command};
    for point in [
        "cloud.tracking.move.after_journal",
        "cloud.tracking.move.after_wal",
        "cloud.tracking.move.after_main",
        "cloud.tracking.after_move_aside",
        "cloud.tracking.after_salvage",
    ] {
        let dir = tempfile::tempdir().unwrap();
        wal_damage_fixture(dir.path());
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery_sigabrt_child", "--nocapture"])
            .env("MEGURI_CLOUD_RECOVERY_CHILD", dir.path())
            .env("MEGURI_CLOUD_RECOVERY_POINT", point)
            .status()
            .unwrap();
        assert_eq!(
            status.signal(),
            Some(6),
            "must witness SIGABRT at {point}, not any nonzero exit"
        );
        let random = SeededRandom::new(902);
        let store = recover(dir.path(), &random);
        assert_eq!(store.conflict_copies().unwrap().len(), 3, "{point}");
        assert_eq!(store.salvage_bindings().unwrap().len(), 1, "{point}");
        assert_eq!(store.selected_binding().unwrap(), None);
        drop(store);
        assert_eq!(recover(dir.path(), &random).conflict_copies().unwrap().len(), 3);
    }
}

#[test]
fn recovery_of_missing_main_with_unjournaled_current_wal_refuses_without_guessing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("meguri-cloud.db-wal"),
        b"initial WAL is not a completed salvage",
    )
    .unwrap();
    let before = files(dir.path());
    assert!(matches!(
        TrackingStore::recover(dir.path(), scope(), &clock(0), &SeededRandom::new(1)),
        Err(Error::RecoveryRefused)
    ));
    assert_eq!(files(dir.path()), before);
}

#[test]
fn recovery_preserves_newer_original_main_wal_and_shm_bytes() {
    let dir = tempfile::tempdir().unwrap();
    drop(open(dir.path()));
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=3;")
        .unwrap();
    let before = files(dir.path());
    let random = SeededRandom::new(99);
    for _ in 0..2 {
        assert!(matches!(
            TrackingStore::recover(dir.path(), scope(), &clock(0), &random).unwrap(),
            OpenOutcome::NewerSchema { found: 3 }
        ));
        assert_eq!(files(dir.path()), before);
    }
    drop(db);
}

#[test]
fn fully_unreadable_main_and_wal_are_retained_with_no_empty_salvage_group() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(TRACKING_FILE), b"damaged-main").unwrap();
    fs::write(dir.path().join("meguri-cloud.db-wal"), b"damaged-wal").unwrap();
    let random = SeededRandom::new(101);
    let store = recover(dir.path(), &random);
    assert!(store.conflict_copies().unwrap().is_empty());
    assert!(store.salvage_bindings().unwrap().is_empty());
    let kept = store.retained_damaged_sources().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read(dir.path().join(&kept[0])).unwrap(), b"damaged-main");
    assert_eq!(
        fs::read(dir.path().join(format!("{}-wal", kept[0]))).unwrap(),
        b"damaged-wal"
    );
}

#[test]
fn unknown_groups_from_different_sources_remain_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let random = SeededRandom::new(104);
    for (id, name) in [
        (1, "meguri-cloud.db.damaged-2026-01-01-00.00.00.salvaging"),
        (2, "meguri-cloud.db.damaged-2026-02-01-00.00.00.salvaging"),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let mut store = open(fixture.path());
        let old = binding(&mut store);
        store.store_conflict(old.id, &copy(id)).unwrap();
        drop(store);
        let db = rusqlite::Connection::open(fixture.path().join(TRACKING_FILE)).unwrap();
        db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE conflicts SET binding=x'00';")
            .unwrap();
        drop(db);
        fs::copy(fixture.path().join(TRACKING_FILE), dir.path().join(name)).unwrap();
    }
    let store = recover(dir.path(), &random);
    let groups = store.salvage_bindings().unwrap();
    assert_eq!(groups.len(), 2);
    assert_ne!(groups[0].source, groups[1].source);
    assert!(
        groups
            .iter()
            .all(|g| g.original_binding.is_none() && g.account.is_none() && g.epoch.is_none())
    );
    let copies = store.conflict_copies().unwrap();
    assert_eq!(copies.len(), 2);
    assert_ne!(copies[0].binding, copies[1].binding);
}

#[test]
fn deferred_foreign_key_failure_at_actual_destination_commit_keeps_main_and_wal() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    wal_damage_fixture(fixture.path());
    let source = "meguri-cloud.db.damaged-2026-01-01-00.00.00.salvaging";
    drop(open(dir.path()));
    fs::copy(fixture.path().join(TRACKING_FILE), dir.path().join(source)).unwrap();
    fs::copy(
        fixture.path().join("meguri-cloud.db-wal"),
        dir.path().join(format!("{source}-wal")),
    )
    .unwrap();
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch(
        "CREATE TABLE commit_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE commit_child(id INTEGER REFERENCES commit_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER reject_commit AFTER INSERT ON conflicts BEGIN INSERT INTO commit_child VALUES (1); END;",
    )
    .unwrap();
    drop(db);
    let main = fs::read(dir.path().join(source)).unwrap();
    let wal = fs::read(dir.path().join(format!("{source}-wal"))).unwrap();
    let random = SeededRandom::new(1200);
    assert!(matches!(
        TrackingStore::recover(dir.path(), scope(), &clock(0), &random),
        Err(Error::Constraint)
    ));
    assert_eq!(fs::read(dir.path().join(source)).unwrap(), main);
    assert_eq!(fs::read(dir.path().join(format!("{source}-wal"))).unwrap(), wal);
    let db = rusqlite::Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    for table in ["salvage_bindings", "conflicts", "commit_child"] {
        let count: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    db.execute_batch("DROP TRIGGER reject_commit").unwrap();
    drop(db);
    assert_eq!(recover(dir.path(), &random).conflict_copies().unwrap().len(), 3);
}
