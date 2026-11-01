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
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=4;")
        .unwrap();
    let main = fs::read(directory.path().join(TRACKING_FILE)).unwrap();
    // The old version is still in the main file; only a private WAL-aware
    // probe can see that this is a newer database.
    assert_eq!(&main[60..64], &3_u32.to_be_bytes());
    assert!(directory.path().join("meguri-cloud.db-wal").exists());
    assert!(directory.path().join("meguri-cloud.db-shm").exists());
    let before = files(directory.path());
    for _ in 0..2 {
        assert!(matches!(
            TrackingStore::open(directory.path(), scope()).unwrap(),
            OpenOutcome::NewerSchema { found: 4 }
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

fn assert_current_incoming_damage_stays_untouched(table: &str, unreadable_content: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut store = open(directory.path());
    let binding = binding(&mut store);
    drop(store);
    let database = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    // Explicit target-version fixture, including while the pre-migration
    // implementation still writes version 2. Keep all corruption in the WAL.
    database
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=3;")
        .unwrap();
    if unreadable_content {
        let sql = match table {
            "cloud_apply_intents" => "INSERT INTO cloud_apply_intents(binding,content) VALUES(?1,'{}')",
            "cloud_refused" => "INSERT INTO cloud_refused(binding,name,content) VALUES(?1,'corrupt-entry','{}')",
            _ => panic!("unexpected incoming table"),
        };
        // Valid SQLite/JSON, but not a readable intent/refusal. These rows are
        // deliberate corruption, never fixtures for proving incoming applies.
        database.execute(sql, [binding.id.0.as_slice()]).unwrap();
    } else {
        database.execute_batch(&format!("DROP TABLE {table}")).unwrap();
    }
    assert!(directory.path().join("meguri-cloud.db-wal").exists());
    assert!(directory.path().join("meguri-cloud.db-shm").exists());
    let tables: i64 = database
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, if unreadable_content { 1 } else { 0 });
    if unreadable_content {
        let content: String = database
            .query_row(&format!("SELECT content FROM {table}"), [], |row| row.get(0))
            .unwrap();
        assert_eq!(content, "{}");
    }
    let before = files(directory.path());
    for _ in 0..2 {
        assert!(matches!(
            TrackingStore::open(directory.path(), scope()).unwrap(),
            OpenOutcome::Damaged
        ));
        // Byte identity also proves missing tables were not recreated and
        // malformed rows were not repaired. Avoid source reads here: even a
        // SELECT on this live connection can update its WAL-index read marks.
        assert_eq!(files(directory.path()), before);
    }
    // Closing this connection can checkpoint; compare before its own close.
    drop(database);
}

#[test]
fn current_schema_missing_apply_intents_is_damaged_without_recreating_or_changing_bytes() {
    assert_current_incoming_damage_stays_untouched("cloud_apply_intents", false);
}

#[test]
fn current_schema_missing_refused_is_damaged_without_recreating_or_changing_bytes() {
    assert_current_incoming_damage_stays_untouched("cloud_refused", false);
}

#[test]
fn current_schema_unreadable_apply_intents_is_damaged_without_changing_bytes() {
    assert_current_incoming_damage_stays_untouched("cloud_apply_intents", true);
}

#[test]
fn current_schema_unreadable_refused_is_damaged_without_changing_bytes() {
    assert_current_incoming_damage_stays_untouched("cloud_refused", true);
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
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=4;")
        .unwrap();
    let before = files(dir.path());
    let random = SeededRandom::new(99);
    for _ in 0..2 {
        assert!(matches!(
            TrackingStore::recover(dir.path(), scope(), &clock(0), &random).unwrap(),
            OpenOutcome::NewerSchema { found: 4 }
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

// U113 test-first RED: P24 account-check/root_read and Q11. The shared Service
// exists; cloud::devices/coordinator intentionally do not yet exist. These are
// enabled regressions, not acceptance evidence. No publication/adoption lane
// is claimed by arranging an already healthy, bound component fixture.
mod account_and_root_regressions {
    use meguri_model::rng::SeededRandom;
    use meguri_sync::cloud::{BindingState, IsolationReason, entity_codec as codec};
    use meguri_testkit::{
        cloud::{
            self, Config, Field, LogOutcome, Mutation, Record, ReplyReference, RequestKind, Scope, Service,
            coordinator::{Barrier, Coordinator, ReadKind, TraceEvent},
            devices::{DeviceFacts, DirtySource, IdentityReads, PreboundDevice},
        },
        generate::recipe::{Recipe, generate_recipe},
    };

    const SEED: u64 = 0x2411_0051;
    const EDIT: [&str; 2] = ["generic update", "back update"];

    fn fixture() -> (tempfile::TempDir, Service<SeededRandom>, PreboundDevice) {
        let run = tempfile::tempdir().unwrap();
        let collection = run.path().join("device");
        let recipe = Recipe::parse(
            r#"recipe: p24-bound-note
description: Generic test
now: "2025-01-01T00:00:00Z"
decks:
  - name: Generic deck
    notetype: basic
    new: 1
    fields: ["generic", "back"]
"#,
        )
        .unwrap();
        let manifest = generate_recipe(&recipe, SEED, &collection).unwrap();
        assert_eq!(manifest.counts.notes, 1);
        assert_eq!(manifest.counts.cards, 1);
        assert_eq!(manifest.counts.revlog, 0);
        let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(SEED)).unwrap();
        // Arrange an already healthy layout: local and acked notetype struct_gen,
        // Note/Card payloads and their acknowledgements must all agree on 1.
        // This seeds the baseline; it must not publish a live generation.
        let files = PreboundDevice::arrange(&collection, &mut service, SEED).unwrap();
        let baseline = files.read_durable().unwrap();
        assert_eq!(baseline.note, files.note);
        assert_eq!(baseline.note.nt_struct_gen, 1);
        let payload = codec::Payload::Note(baseline.note.clone());
        assert_ne!(payload.row_digest().unwrap(), payload.payload_digest().unwrap());
        assert_eq!(baseline.mark, None);
        assert!(baseline.pending_names.is_empty());
        assert_eq!(baseline.binding_state, BindingState::Active);
        assert!(baseline.scan_drained);
        assert!(baseline.sidecar_clean);
        assert_eq!(decode(&files, &record(&service, &files)).1, files.note);
        (run, service, files)
    }

    fn record(service: &Service<SeededRandom>, files: &PreboundDevice) -> Record {
        service.inspect(&files.scope).unwrap().unwrap().zones[&files.collection_zone].records[&files.entity_name]
            .clone()
    }

    fn decode(files: &PreboundDevice, record: &Record) -> (codec::Entity, codec::Note) {
        assert_eq!(record.name, files.entity_name);
        assert_eq!(record.record_type, "MGEntity");
        let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
            panic!("expected the actual encrypted entity envelope");
        };
        let received = codec::receive(
            &files.binding.facts.record_key.0,
            &record.name,
            bytes,
            codec::AssetInput::Absent,
        )
        .unwrap();
        let Some(codec::Payload::Note(note)) = received.payload else {
            panic!("expected the actual canonical Note payload");
        };
        assert_eq!(received.entity.fields.key, codec::id_key(files.note.id));
        (received.entity, note)
    }

    fn cursor(service: &Service<SeededRandom>) -> u64 {
        service.request_log().unwrap().last().map_or(0, |r| r.request_id)
    }

    fn assert_no_writes_since(service: &Service<SeededRandom>, after: u64) {
        // Global log includes every account/container/environment partition.
        for request in service.request_log().unwrap().iter().filter(|r| r.request_id > after) {
            assert!(!matches!(
                request.kind,
                RequestKind::Modify | RequestKind::CreateZone | RequestKind::DeleteZone
            ));
        }
    }

    fn assert_no_transfer_or_replacement(trace: &[TraceEvent]) {
        assert!(!trace.iter().any(|e| matches!(
            e,
            TraceEvent::PublishStarted
                | TraceEvent::AdoptStarted
                | TraceEvent::CollectionReplaced
                | TraceEvent::ManualTransferStarted
        )));
    }

    fn assert_closed(session: &mut Coordinator, service: &mut Service<SeededRandom>) {
        let before = cursor(service);
        let attempt = session.request_collection_batch(service).unwrap();
        assert!(attempt.mutations.is_empty());
        assert!(attempt.request_ids.is_empty());
        assert_no_writes_since(service, before);
    }

    fn assert_service_read(
        service: &Service<SeededRandom>,
        files: &PreboundDevice,
        request_id: u64,
        instance: &str,
        record_type: &str,
    ) {
        let log = service.request_log().unwrap();
        let request = log.iter().find(|r| r.request_id == request_id).unwrap();
        assert_eq!(request.kind, RequestKind::Read);
        assert_eq!(request.scope, files.scope);
        assert_eq!(request.caller.instance.as_deref(), Some(instance));
        assert_eq!(request.outcome, LogOutcome::Success);
        let outcomes = service.outcomes().unwrap();
        let outcome = outcomes.iter().find(|r| r.request_id == request_id).unwrap();
        let Ok(ReplyReference::Record(reference)) = &outcome.result else {
            panic!("expected a persisted successful Service read");
        };
        let record = service.resolve_record(&files.scope, reference).unwrap();
        assert_eq!(record.record_type, record_type);
        assert_eq!(request.records.len(), 1);
        assert_eq!(request.records[0].name, record.name);
    }

    fn assert_tracking_unchanged(before: &DeviceFacts, after: &DeviceFacts) {
        assert_eq!(after.change_counter, before.change_counter);
        assert_eq!(after.local_rev, before.local_rev);
        assert_eq!(after.last_observed_digest, before.last_observed_digest);
        assert_eq!(after.mark, before.mark);
        assert_eq!(after.pending_names, before.pending_names);
    }

    fn assert_observer_capture(session: &Coordinator, edit: &cloud::coordinator::Edit) {
        let proof = session.capture_proof(edit).unwrap();
        assert_eq!(proof.observed_seq, edit.seq);
        assert_eq!(proof.generation, edit.generation);
        assert!(proof.attempted);
        assert!(proof.persisted);
    }

    fn assert_saved(
        session: &mut Coordinator,
        service: &mut Service<SeededRandom>,
        files: &PreboundDevice,
        baseline: &Record,
        expected: &codec::Note,
    ) {
        let (old_entity, _) = decode(files, baseline);
        let before = cursor(service);
        let attempt = session.request_collection_batch(service).unwrap();
        assert_eq!(attempt.mutations.len(), 1);
        {
            let mutation: &cloud::Mutation = &attempt.mutations[0];
            let Mutation::Save { record, if_unchanged } = mutation else {
                panic!("expected a Save mutation");
            };
            assert_eq!(record.name, files.entity_name);
            assert_eq!(if_unchanged.as_ref(), Some(&baseline.system_fields));
        }
        assert!(!attempt.request_ids.is_empty());
        let saved = record(service, files);
        let (entity, note) = decode(files, &saved);
        assert_eq!(&note, expected);
        assert_eq!(note.flds, EDIT.join("\u{1f}").as_bytes());
        assert_eq!(entity.fields.erev, old_entity.fields.erev.checked_add(1).unwrap());
        assert_ne!(saved.system_fields, baseline.system_fields);
        let log = service.request_log().unwrap();
        let outcomes = service.outcomes().unwrap();
        let mut accepted = 0;
        for request in log
            .iter()
            .filter(|r| r.request_id > before && r.kind == RequestKind::Modify)
        {
            assert!(attempt.request_ids.contains(&request.request_id));
            assert_eq!(request.scope, files.scope);
            assert_eq!(request.zone.as_deref(), Some(files.collection_zone.as_str()));
            assert_eq!(request.caller.instance.as_deref(), Some(session.instance().as_str()));
            assert_eq!(request.outcome, LogOutcome::Success);
            let outcome = outcomes.iter().find(|r| r.request_id == request.request_id).unwrap();
            let Ok(ReplyReference::Modified(items)) = &outcome.result else {
                panic!("expected a persisted Service modify outcome");
            };
            for item in items {
                assert_eq!(item.name, files.entity_name);
                let reference = item.result.as_ref().unwrap().as_ref().unwrap();
                assert_eq!(service.resolve_record(&files.scope, reference).unwrap(), saved);
                accepted += 1;
            }
        }
        assert_eq!(accepted, 1);
        assert_no_transfer_or_replacement(session.trace());
    }

    #[test]
    fn dropped_dirty_mark_offline_clean_close_next_verified_open_scans_and_saves() {
        let (_run, mut service, files) = fixture();
        let baseline = record(&service, &files);
        let before = cursor(&service);
        let mut first = Coordinator::open(&files, IdentityReads::Available(files.scope.account.clone())).unwrap();
        first.reach(Barrier::BeforeRootConfirmation).unwrap();
        let tracking_before = first.facts().unwrap();
        assert_eq!(tracking_before.mark, None);
        assert!(tracking_before.pending_names.is_empty());
        // P24: roll back the whole observer capture; remain paused until SIGKILL.
        let before_loss = first.trace().len();
        first.arm_capture_loss_for_test(files.note.id).unwrap();
        let edit = first.edit_note_fields(files.note.id, &EDIT).unwrap();
        assert_ne!(edit.note.flds, tracking_before.note.flds);
        let capture = first.capture_proof(&edit).unwrap();
        assert_eq!(capture.observed_seq, edit.seq);
        assert_eq!(capture.generation, edit.generation);
        assert!(capture.attempted);
        assert!(!capture.persisted);
        let captured = first.facts().unwrap();
        assert_eq!(captured.note, edit.note);
        assert_tracking_unchanged(&tracking_before, &captured);
        let proof = first.wal_proof(&edit).unwrap();
        assert!(proof.through >= edit.seq);
        assert_eq!(proof.generation, edit.generation);
        assert_eq!(proof.native_file, edit.native_file);
        assert_eq!(first.facts().unwrap().mark, None);
        assert_eq!(record(&service, &files), baseline);
        assert!(
            !first.trace()[before_loss..]
                .iter()
                .any(|e| matches!(e, TraceEvent::ScanCommitted { .. }))
        );
        let killed = first.kill_sigkill().unwrap();
        assert_eq!(killed.signal, Some(9));
        assert!(!killed.clean_close);
        let after_kill = files.read_durable().unwrap();
        assert_eq!(after_kill.note, edit.note);
        assert_eq!(after_kill.mark, None);
        assert_tracking_unchanged(&tracking_before, &after_kill);
        assert_no_writes_since(&service, before);

        let mut offline = Coordinator::open(&files, IdentityReads::FailAllSession).unwrap();
        offline.reach(Barrier::IdentityBlocked).unwrap();
        assert_closed(&mut offline, &mut service);
        let offline_facts = offline.facts().unwrap();
        assert_eq!(offline_facts.note, edit.note);
        assert_tracking_unchanged(&tracking_before, &offline_facts);
        assert!(!offline_facts.scan_drained);
        offline.close_clean().unwrap();
        assert!(!offline.trace().iter().any(|e| matches!(
            e,
            TraceEvent::IdentityVerified { .. }
                | TraceEvent::ScanCommitted { .. }
                | TraceEvent::EngineStarted { .. }
                | TraceEvent::CollectionBatchBuilt { .. }
        )));
        assert_no_transfer_or_replacement(offline.trace());
        // These are fresh reads of persisted scan_state, collection and sidecar.
        let closed = files.read_durable().unwrap();
        assert!(closed.sidecar_clean);
        assert!(!closed.scan_drained);
        assert_eq!(closed.note, edit.note);
        assert_eq!(closed.mark, None);
        assert_tracking_unchanged(&tracking_before, &closed);
        assert_no_writes_since(&service, before);
        assert_eq!(record(&service, &files), baseline);

        let mut online = Coordinator::open(&files, IdentityReads::Available(files.scope.account.clone())).unwrap();
        online.reach(Barrier::ScanCommitted).unwrap();
        let scan = online.scan_proof().unwrap();
        assert_eq!(scan.snapshot_native_file, proof.native_file);
        assert_eq!(scan.receipt.native_file, scan.snapshot_native_file);
        assert_eq!(scan.receipt.generation, scan.snapshot_generation);
        assert!(scan.receipt.through >= scan.snapshot_seq);
        let recovered = online.facts().unwrap();
        // P24: persisted drain waits for clean close; this open has finished scanning.
        assert!(!recovered.scan_drained);
        let scan_state = online.scan_state().unwrap();
        assert!(!scan_state.scan_required);
        assert!(!scan_state.scan_running);
        assert!(scan_state.scan_completed_this_open);
        assert_eq!(recovered.note, edit.note);
        let mark = recovered.mark.unwrap();
        assert_eq!(mark.binding, files.binding.id);
        assert_eq!(mark.source, DirtySource::Snapshot);
        assert!(mark.change_seq > tracking_before.change_counter);
        assert!(mark.change_seq <= recovered.change_counter);
        assert!(recovered.local_rev > tracking_before.local_rev);
        assert_eq!(edit.note.nt_struct_gen, files.note.nt_struct_gen);
        let payload = codec::Payload::Note(edit.note.clone());
        assert_eq!(recovered.last_observed_digest, Some(payload.row_digest().unwrap()));
        assert_ne!(payload.row_digest().unwrap(), payload.payload_digest().unwrap());
        assert_eq!(
            mark.payload_digest,
            codec::Payload::Note(edit.note.clone()).payload_digest().unwrap()
        );
        assert!(recovered.pending_names.contains(&files.entity_name));
        assert!(!online.trace().iter().any(|e| matches!(
            e,
            TraceEvent::EngineStarted { .. } | TraceEvent::CollectionBatchBuilt { .. }
        )));
        let instance = online.instance();
        let verified = online
            .trace()
            .iter()
            .position(|e| {
                matches!(e,
            TraceEvent::IdentityVerified { instance: i, account }
                if i == &instance && account == &files.scope.account)
            })
            .unwrap();
        let scanned = online
            .trace()
            .iter()
            .position(|e| {
                matches!(e,
            TraceEvent::ScanCommitted { instance: i, native_file, generation, covered_seq, recovered_names }
                if i == &instance && native_file == &scan.snapshot_native_file
                    && generation == &scan.snapshot_generation && covered_seq == &scan.receipt.through
                    && recovered_names.contains(&files.entity_name))
            })
            .unwrap();
        assert!(verified < scanned);
        online.reach(Barrier::BeforeRootConfirmation).unwrap();
        let started = online
            .trace()
            .iter()
            .position(|e| {
                matches!(e,
            TraceEvent::EngineStarted { instance: i, account }
                if i == &instance && account == &files.scope.account)
            })
            .unwrap();
        assert!(scanned < started);
        assert_closed(&mut online, &mut service);
        let root = online.read(&mut service, ReadKind::ActiveNoState).unwrap();
        assert_eq!(root.instance, instance);
        assert_eq!(root.active_epoch, Some(files.binding.facts.epoch));
        assert_service_read(&service, &files, root.request_id, &instance, "MGActive");
        online.deliver_read(&root).unwrap();
        assert_saved(&mut online, &mut service, &files, &baseline, &edit.note);
        online.close_clean().unwrap();
        let drained = files.read_durable().unwrap();
        assert!(drained.scan_drained);
        assert!(drained.sidecar_clean);
        assert_eq!(drained.note, edit.note);
    }

    #[test]
    fn preverification_edits_then_different_identity_never_send() {
        let (_run, mut service, files) = fixture();
        let baseline_a = service.inspect(&files.scope).unwrap();
        let b = Scope {
            account: "synthetic-account-b".into(),
            ..files.scope.clone()
        };
        assert_ne!(b.account, files.scope.account);
        let baseline_b = service.inspect(&b).unwrap();
        let before = cursor(&service);
        let mut session = Coordinator::open(&files, IdentityReads::FailAllSession).unwrap();
        session.reach(Barrier::IdentityBlocked).unwrap();
        let before_edit = session.facts().unwrap();
        let edit = session.edit_note_fields(files.note.id, &EDIT).unwrap();
        assert_observer_capture(&session, &edit);
        let local = session.facts().unwrap();
        assert_eq!(local.binding_state, BindingState::Active);
        assert_eq!(local.note, edit.note);
        let mark = local.mark.unwrap();
        assert_eq!(mark.binding, files.binding.id);
        assert_eq!(mark.source, DirtySource::Observer);
        assert!(mark.change_seq > before_edit.change_counter);
        assert!(mark.change_seq <= local.change_counter);
        assert_eq!(
            mark.payload_digest,
            codec::Payload::Note(edit.note.clone()).payload_digest().unwrap()
        );
        assert!(local.pending_names.contains(&files.entity_name));
        assert_closed(&mut session, &mut service);
        // A successful external account event; automatic reads still fail.
        session.verify_identity(b.account.clone()).unwrap();
        assert_closed(&mut session, &mut service);
        assert_eq!(
            session.facts().unwrap().binding_state,
            BindingState::Isolated(IsolationReason::AccountChanged)
        );
        session.close_clean().unwrap();
        let durable = files.read_durable().unwrap();
        assert_eq!(durable.note, edit.note);
        assert_eq!(durable.mark, Some(mark));
        assert_eq!(
            durable.binding_state,
            BindingState::Isolated(IsolationReason::AccountChanged)
        );
        assert!(durable.pending_names.contains(&files.entity_name));
        assert!(session.trace().iter().any(|e| matches!(e,
            TraceEvent::IdentityVerified { account, .. } if account == &b.account)));
        assert!(!session.trace().iter().any(|e| matches!(
            e,
            TraceEvent::EngineStarted { .. } | TraceEvent::CollectionBatchBuilt { .. }
        )));
        assert_no_transfer_or_replacement(session.trace());
        assert_no_writes_since(&service, before);
        assert_eq!(service.inspect(&files.scope).unwrap(), baseline_a);
        assert_eq!(service.inspect(&b).unwrap(), baseline_b);
    }

    #[test]
    fn root_confirmation_safety_finite_permutations_require_current_instance_proof() {
        // P24 root_read / K21.20 / V22.14. Finite deterministic property:
        // all six orders of epoch-only, stale read and stale drain, followed by
        // each valid current-instance proof. No publisher is present here;
        // accepted publication `done` remains an allowed production proof.
        const ORDERS: [[u8; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
        for order in ORDERS {
            for own_drain in [false, true] {
                let (_run, mut service, files) = fixture();
                let baseline = record(&service, &files);
                let mut prior =
                    Coordinator::open(&files, IdentityReads::Available(files.scope.account.clone())).unwrap();
                prior.reach(Barrier::BeforeRootConfirmation).unwrap();
                let before_edit = prior.facts().unwrap();
                let edit = prior.edit_note_fields(files.note.id, &EDIT).unwrap();
                assert_observer_capture(&prior, &edit);
                let local = prior.facts().unwrap();
                let mark = local.mark.unwrap();
                assert_eq!(mark.binding, files.binding.id);
                assert_eq!(mark.source, DirtySource::Observer);
                assert!(mark.change_seq > before_edit.change_counter);
                assert!(mark.change_seq <= local.change_counter);
                assert!(prior.facts().unwrap().pending_names.contains(&files.entity_name));
                assert_closed(&mut prior, &mut service);
                let stale_read = prior.read(&mut service, ReadKind::ActiveNoState).unwrap();
                assert_service_read(&service, &files, stale_read.request_id, &prior.instance(), "MGActive");
                let stale_fetch = prior.fetch_root(&mut service).unwrap();
                prior.deliver_read(&stale_read).unwrap();
                assert!(prior.trace().iter().any(|e| matches!(e,
                    TraceEvent::RootConfirmed { instance } if instance == &prior.instance())));
                // Close without sending: a new instance must discard this flag.
                prior.close_clean().unwrap();
                let mut current =
                    Coordinator::open(&files, IdentityReads::Available(files.scope.account.clone())).unwrap();
                current.reach(Barrier::BeforeRootConfirmation).unwrap();
                assert_ne!(current.instance(), prior.instance());
                assert!(current.facts().unwrap().pending_names.contains(&files.entity_name));
                assert_closed(&mut current, &mut service);
                for event in order {
                    match event {
                        0 => {
                            let epoch = current.read(&mut service, ReadKind::Epoch).unwrap();
                            assert_service_read(&service, &files, epoch.request_id, &current.instance(), "MGEpoch");
                            current.deliver_read(&epoch).unwrap();
                        }
                        1 => current.deliver_read(&stale_read).unwrap(),
                        2 => current.deliver_root_fetch(&stale_fetch).unwrap(),
                        _ => unreachable!(),
                    }
                    assert!(current.facts().unwrap().pending_names.contains(&files.entity_name));
                    assert!(!current.trace().iter().any(|e| matches!(e,
                        TraceEvent::RootConfirmed { instance } if instance == &current.instance())));
                    assert_closed(&mut current, &mut service);
                    assert_eq!(record(&service, &files), baseline);
                }
                if own_drain {
                    let own = current.fetch_root(&mut service).unwrap();
                    assert_eq!(own.instance, current.instance());
                    assert!(!own.responses.is_empty());
                    assert!(matches!(&own.responses.last().unwrap().result,
                        Ok(cloud::Reply::ZoneChanges(page)) if !page.more));
                    let log = service.request_log().unwrap();
                    for response in &own.responses {
                        let request = log.iter().find(|r| r.request_id == response.request_id).unwrap();
                        assert_eq!(request.kind, RequestKind::FetchZone);
                        assert_eq!(request.scope, files.scope);
                        assert_eq!(request.zone.as_deref(), Some("meguri-root"));
                        assert_eq!(request.caller.instance.as_deref(), Some(current.instance().as_str()));
                        assert_eq!(request.outcome, LogOutcome::Success);
                    }
                    current.deliver_root_fetch(&own).unwrap();
                } else {
                    let own = current.read(&mut service, ReadKind::ActiveNoState).unwrap();
                    assert_eq!(own.instance, current.instance());
                    assert_eq!(own.active_epoch, Some(files.binding.facts.epoch));
                    assert_service_read(&service, &files, own.request_id, &current.instance(), "MGActive");
                    current.deliver_read(&own).unwrap();
                }
                assert!(current.trace().iter().any(|e| matches!(e,
                    TraceEvent::RootConfirmed { instance } if instance == &current.instance())));
                assert_saved(&mut current, &mut service, &files, &baseline, &edit.note);
                current.close_clean().unwrap();
            }
        }

        let (_run, mut service, files) = fixture();
        let mut lost = Coordinator::open(&files, IdentityReads::Available(files.scope.account.clone())).unwrap();
        lost.reach(Barrier::BeforeRootConfirmation).unwrap();
        let before_edit = lost.facts().unwrap();
        let edit = lost.edit_note_fields(files.note.id, &EDIT).unwrap();
        assert_observer_capture(&lost, &edit);
        let local = lost.facts().unwrap();
        let mark = local.mark.unwrap();
        assert_eq!(mark.binding, files.binding.id);
        assert_eq!(mark.source, DirtySource::Observer);
        assert!(mark.change_seq > before_edit.change_counter);
        assert!(mark.change_seq <= local.change_counter);
        assert!(lost.facts().unwrap().pending_names.contains(&files.entity_name));
        assert_closed(&mut lost, &mut service);
        // Delete only the synthetic bound zone using the existing Service.
        service
            .request(
                &files.scope,
                cloud::Caller::default(),
                cloud::Operation::DeleteZone {
                    zone: files.collection_zone.clone(),
                    cause: cloud::RemovalCause::Deleted,
                },
            )
            .unwrap()
            .result
            .unwrap();
        let before = cursor(&service);
        let read = lost.read(&mut service, ReadKind::ActiveAfterLostZone).unwrap();
        assert_eq!(read.instance, lost.instance());
        assert_eq!(read.active_epoch, Some(files.binding.facts.epoch));
        assert_service_read(&service, &files, read.request_id, &lost.instance(), "MGActive");
        lost.deliver_read(&read).unwrap();
        assert!(lost.trace().iter().any(|e| matches!(e,
            TraceEvent::RootReadRecorded { instance, request_id }
                if instance == &lost.instance() && request_id == &read.request_id)));
        assert!(
            !lost
                .trace()
                .iter()
                .any(|e| matches!(e, TraceEvent::RootConfirmed { .. }))
        );
        assert_eq!(
            lost.facts().unwrap().binding_state,
            BindingState::Isolated(IsolationReason::ZoneMissing)
        );
        assert_eq!(lost.facts().unwrap().note, edit.note);
        assert_eq!(lost.facts().unwrap().mark, Some(mark.clone()));
        assert_closed(&mut lost, &mut service);
        assert_no_writes_since(&service, before);
        assert_no_transfer_or_replacement(lost.trace());
        lost.close_clean().unwrap();
        let durable = files.read_durable().unwrap();
        assert_eq!(
            durable.binding_state,
            BindingState::Isolated(IsolationReason::ZoneMissing)
        );
        assert_eq!(durable.note, edit.note);
        assert_eq!(durable.mark, Some(mark));
    }
}

// Infrastructure entry for the shared CoreHost subprocess protocol.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[test]
fn recovery_salvage_owner_row_disappears_with_its_last_expired_copy() {
    let directory = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let mut store = open(source.path());
    let owner = binding(&mut store);
    let mut first = copy(1);
    first.staged_path = None;
    let mut last = copy(2);
    last.staged_path = None;
    last.resolved_at_ms = Some(201);
    store.store_conflict(owner.id, &first).unwrap();
    store.store_conflict(owner.id, &last).unwrap();
    drop(store);
    let salvaging = "meguri-cloud.db.damaged-2026-01-01-00.00.00.salvaging";
    fs::copy(source.path().join(TRACKING_FILE), directory.path().join(salvaging)).unwrap();
    let mut recovered = recover(directory.path(), &SeededRandom::new(944));
    let owners = recovered.salvage_bindings().unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].source, salvaging);
    assert!(
        recovered.binding(owners[0].id).unwrap().is_none(),
        "salvage is a distinct owner table"
    );
    let retention = 30 * 24 * 60 * 60 * 1_000;
    assert_eq!(recovered.expire_resolved_conflicts(200 + retention).unwrap(), 1);
    assert_eq!(recovered.salvage_bindings().unwrap(), owners);
    assert_eq!(recovered.expire_resolved_conflicts(201 + retention).unwrap(), 1);
    assert!(recovered.salvage_bindings().unwrap().is_empty());
    drop(recovered);
    let recovered = open(directory.path());
    assert!(recovered.conflict_copies().unwrap().is_empty());
    assert!(recovered.salvage_bindings().unwrap().is_empty());
}
