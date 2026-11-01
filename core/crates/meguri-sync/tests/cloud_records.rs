//! K10.1-K10.3 stored facts and build ownership. No account-policy assertions.
use meguri_engine::device_state::PublicationId;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::{RandomSource, SeededRandom},
};
use meguri_sync::cloud::*;
use std::{fs, path::Path};

fn scope(container: &str, environment: Environment) -> BuildScope {
    BuildScope {
        container: ContainerId::new(container).unwrap(),
        environment,
    }
}

fn clock(ms: i64) -> FixedClock {
    FixedClock::new(ms, UtcOffset::UTC)
}

fn facts(state: BindingState) -> BindingFacts {
    // Synthetic fixture of the engine's existing identity type, not a new
    // production identity generator. Only the engine creates publication ids.
    let random = SeededRandom::new(77);
    let publication: PublicationId =
        serde_json::from_value(serde_json::json!([random.next_u64(), random.next_u64()])).unwrap();
    BindingFacts {
        account: OpaqueBytes(b"private-account-fixture".to_vec()),
        epoch: EpochId([31; 16]),
        record_key: RecordKey([91; 32]),
        publication,
        state,
    }
}

fn open(directory: &Path, scope: BuildScope) -> TrackingStore {
    match TrackingStore::open(directory, scope).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
        _ => panic!("expected supported tracking file"),
    }
}

fn record() -> InboxEntry {
    InboxEntry {
        record_type: RecordType::Entity,
        record_name: "e-synthetic".into(),
        value: InboxValue::Record {
            erev: u64::MAX,
            system_fields: OpaqueBytes(vec![0, 255, 4]),
            envelope: OpaqueBytes(b"opaque-private-envelope".to_vec()),
            staged_asset: Some("staging/synthetic-asset".into()),
        },
        reason: InboxReason::Asset,
        source: InboxSource::Fetch,
        absence_pass: Some([17; 16]),
        retry_count: 3,
        fetched_at_ms: 800,
    }
}

#[test]
fn exact_binding_inbox_and_sdk_state_survive_reopen_without_collection_writes() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("collection.anki2");
    let sentinel = b"collection bytes owned exclusively by the engine";
    fs::write(&collection, sentinel).unwrap();
    let build = scope("test.container", Environment::Development);
    let random = SeededRandom::new(123);
    let expected_random = SeededRandom::new(123);
    let mut expected_id = [0; 16];
    expected_id[..8].copy_from_slice(&expected_random.next_u64().to_be_bytes());
    expected_id[8..].copy_from_slice(&expected_random.next_u64().to_be_bytes());
    let mut store = open(directory.path(), build.clone());
    let binding = store
        .create_binding(facts(BindingState::Active), &clock(700), &random)
        .unwrap();
    assert_eq!(binding.id, BindingId(expected_id));
    assert_eq!(binding.created_at_ms, 700);
    assert_eq!(store.engine_state(binding.id).unwrap(), None);
    assert!(store.inbox(binding.id).unwrap().is_empty());
    let entry = record();
    let mut deletion = entry.clone();
    deletion.value = InboxValue::Deletion;
    deletion.reason = InboxReason::Ready;
    deletion.absence_pass = None;
    deletion.retry_count = 0;
    let cursor = OpaqueBytes(vec![255, 0, 31, 99]);
    let entries = [entry, deletion];
    store
        .commit_fetched(binding.id, &entries, &cursor, &clock(900))
        .unwrap();
    let committed_inbox = store.inbox(binding.id).unwrap();
    // Exact event replay retains ids and never doubles the durable entries.
    store
        .commit_fetched(binding.id, &entries, &cursor, &clock(900))
        .unwrap();
    assert_eq!(store.inbox(binding.id).unwrap(), committed_inbox);
    drop(store);

    let store = open(directory.path(), build);
    assert_eq!(store.binding(binding.id).unwrap(), Some(binding.clone()));
    assert_eq!(store.selected_binding().unwrap(), Some(binding.clone()));
    assert_eq!(store.inbox(binding.id).unwrap(), committed_inbox);
    assert_eq!(
        committed_inbox.iter().map(|row| row.entry.clone()).collect::<Vec<_>>(),
        entries
    );
    assert_eq!(
        store.engine_state(binding.id).unwrap(),
        Some(EngineState {
            serialization: cursor,
            saved_at_ms: 900,
        })
    );
    assert_eq!(fs::read(&collection).unwrap(), sentinel);
    assert!(!directory.path().join("collection.anki2-wal").exists());
    assert!(!directory.path().join("collection.anki2-shm").exists());
    assert!(!format!("{binding:?}").contains("private-account-fixture"));
    assert!(!format!("{:?}", entries[0]).contains("opaque-private-envelope"));
}

#[test]
fn builds_share_a_file_but_cannot_read_or_write_each_others_facts() {
    let directory = tempfile::tempdir().unwrap();
    let build_a = scope("test.container", Environment::Development);
    let build_b = scope("test.container", Environment::Production);
    let build_c = scope("other.container", Environment::Development);
    let random = SeededRandom::new(9);
    let mut a = open(directory.path(), build_a.clone());
    let mut b = open(directory.path(), build_b.clone());
    let mut c = open(directory.path(), build_c.clone());
    let a_binding = a
        .create_binding(facts(BindingState::Active), &clock(10), &random)
        .unwrap();
    let b_binding = b
        .create_binding(facts(BindingState::Active), &clock(20), &random)
        .unwrap();
    let c_binding = c
        .create_binding(facts(BindingState::Active), &clock(30), &random)
        .unwrap();
    for (store, own, foreign) in [
        (&mut a, &a_binding, &b_binding),
        (&mut b, &b_binding, &a_binding),
        (&mut c, &c_binding, &a_binding),
    ] {
        assert_eq!(store.binding(foreign.id).unwrap(), None);
        assert_eq!(store.engine_state(foreign.id), Err(Error::BindingNotFound));
        assert_eq!(store.inbox(foreign.id), Err(Error::BindingNotFound));
        assert_eq!(
            store.commit_fetched(foreign.id, &[record()], &OpaqueBytes(vec![8]), &clock(40)),
            Err(Error::BindingNotFound)
        );
        store
            .commit_fetched(own.id, &[record()], &OpaqueBytes(vec![own.id.0[0]]), &clock(50))
            .unwrap();
        assert_eq!(store.selected_binding().unwrap(), Some(own.clone()));
    }
    drop((a, b, c));
    for (scope, binding) in [(build_a, a_binding), (build_b, b_binding), (build_c, c_binding)] {
        let store = open(directory.path(), scope);
        assert_eq!(store.binding(binding.id).unwrap(), Some(binding.clone()));
        assert_eq!(store.inbox(binding.id).unwrap().len(), 1);
        assert_eq!(
            store.engine_state(binding.id).unwrap().unwrap().serialization,
            OpaqueBytes(vec![binding.id.0[0]])
        );
    }
}

#[test]
fn database_enforces_one_active_and_selection_prefers_it_over_newer_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Development);
    let mut first = open(directory.path(), build.clone());
    let mut second = open(directory.path(), build.clone());
    let random = SeededRandom::new(19);
    let active = first
        .create_binding(facts(BindingState::Active), &clock(10), &random)
        .unwrap();
    assert_eq!(
        second.create_binding(facts(BindingState::Active), &clock(20), &random),
        Err(Error::Constraint)
    );
    let isolated = second
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::AccountChanged)),
            &clock(30),
            &random,
        )
        .unwrap();
    assert_eq!(first.selected_binding().unwrap(), Some(active.clone()));
    assert_eq!(
        second.commit_fetched(isolated.id, &[record()], &OpaqueBytes(vec![9]), &clock(40)),
        Err(Error::IsolatedBinding)
    );
    drop((first, second));
    let store = open(directory.path(), build);
    assert_eq!(store.selected_binding().unwrap(), Some(active));
    assert_eq!(store.binding(isolated.id).unwrap(), Some(isolated.clone()));
    assert_eq!(store.engine_state(isolated.id).unwrap(), None);
    assert!(store.inbox(isolated.id).unwrap().is_empty());
}

#[test]
fn without_active_selection_uses_newest_and_never_a_salvage_binding() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Production);
    let random = SeededRandom::new(29);
    let mut store = open(directory.path(), build.clone());
    store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::SignedOut)),
            &clock(10),
            &random,
        )
        .unwrap();
    let newest = store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::Stopped)),
            &clock(20),
            &random,
        )
        .unwrap();
    drop(store);
    // Fixture of a future salvage's output; this slice performs no salvage.
    let connection = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    connection
        .execute(
            "INSERT INTO bindings(id,container,environment,created_at_ms,facts) VALUES (?1,?2,1,999,?3)",
            rusqlite::params![
                [55_u8; 16].as_slice(),
                build.container.as_str(),
                serde_json::to_string(&facts(BindingState::Isolated(IsolationReason::TrackingDamaged))).unwrap()
            ],
        )
        .unwrap();
    drop(connection);
    let mut store = open(directory.path(), build);
    assert_eq!(store.selected_binding().unwrap(), Some(newest));
    assert_eq!(
        store.create_binding(
            facts(BindingState::Isolated(IsolationReason::TrackingDamaged)),
            &clock(1000),
            &random
        ),
        Err(Error::InvalidData)
    );
}

fn conflict(id: u8) -> ConflictCopy {
    ConflictCopy {
        id: CopyId([id; 16]),
        kind: ConflictKind::Note,
        key: OpaqueBytes(vec![0, 255, id]),
        reason: ConflictReason::FieldConflict,
        losing_envelope: OpaqueBytes(b"opaque storage input; not a validated record".to_vec()),
        losing_payload_digest: PayloadDigest([id; 32]),
        winning_payload_digest: PayloadDigest([91; 32]),
        field_names: Some(vec!["Front".into(), "Back".into()]),
        template_names: Some(vec!["Forward".into(), "Reverse".into()]),
        note_deck: Some(42),
        staged_path: Some("cloud-stage/retained-copy".into()),
        created_at_ms: -123,
        resolved_at_ms: Some(456),
    }
}

#[test]
fn typed_conflict_storage_preserves_all_copy_fields_and_made_once_resolved_rows() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Development);
    let random = SeededRandom::new(1234);
    let mut store = open(directory.path(), build.clone());
    let binding = store
        .create_binding(facts(BindingState::Active), &clock(1), &random)
        .unwrap();
    let copy = conflict(1);
    assert_eq!(store.store_conflict(binding.id, &copy).unwrap(), copy.id);
    let mut repeated = copy.clone();
    repeated.id = CopyId([2; 16]);
    repeated.field_names = None;
    repeated.resolved_at_ms = None;
    assert_eq!(store.store_conflict(binding.id, &repeated).unwrap(), copy.id);
    drop(store);
    let mut foreign = open(directory.path(), scope("other.container", Environment::Production));
    assert_eq!(
        foreign.store_conflict(binding.id, &conflict(3)),
        Err(Error::BindingNotFound)
    );
    drop(foreign);
    let store = open(directory.path(), build);
    assert_eq!(
        store.conflict_copies().unwrap(),
        vec![StoredConflict {
            binding: binding.id,
            copy
        }]
    );
    assert!(store.salvage_bindings().unwrap().is_empty());
}

const COPY_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

// Typed storage fixtures, including resolved timestamps, prove retention only;
// they do not stand in for U161's actual resolve/unresolve operations.
#[test]
fn conflict_expiry_cutoff_unresolved_backward_and_overflow_are_conservative() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Development);
    let mut store = open(directory.path(), build.clone());
    let binding = store
        .create_binding(facts(BindingState::Active), &clock(1), &SeededRandom::new(39))
        .unwrap();
    for (id, resolved) in [
        (1, Some(100)),
        (2, None),
        (3, Some(101)),
        (4, Some(i64::MIN)),
        (5, Some(i64::MAX)),
    ] {
        let mut copy = conflict(id);
        copy.staged_path = None;
        copy.resolved_at_ms = resolved;
        store.store_conflict(binding.id, &copy).unwrap();
    }
    assert_eq!(store.expire_resolved_conflicts(99).unwrap(), 0);
    assert_eq!(store.expire_resolved_conflicts(100 + COPY_RETENTION_MS - 1).unwrap(), 0);
    assert_eq!(store.expire_resolved_conflicts(100 + COPY_RETENTION_MS).unwrap(), 1);
    assert_eq!(store.expire_resolved_conflicts(101 + COPY_RETENTION_MS).unwrap(), 1);
    let remaining: Vec<_> = store
        .conflict_copies()
        .unwrap()
        .into_iter()
        .map(|c| c.copy.id)
        .collect();
    assert_eq!(remaining, vec![CopyId([2; 16]), CopyId([4; 16]), CopyId([5; 16])]);
    assert!(
        store.binding(binding.id).unwrap().is_some(),
        "active owner keeps its row"
    );
    drop(store);
    let mut reopened = open(directory.path(), build);
    assert_eq!(reopened.expire_resolved_conflicts(101 + COPY_RETENTION_MS).unwrap(), 0);
    assert_eq!(reopened.conflict_copies().unwrap().len(), 3);
}

#[test]
fn conflict_expiry_all_scopes_remove_only_last_copy_empty_isolated_owners() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Development);
    let mut store = open(directory.path(), build.clone());
    let random = SeededRandom::new(40);
    let active = store
        .create_binding(facts(BindingState::Active), &clock(1), &random)
        .unwrap();
    let empty = store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::SignedOut)),
            &clock(2),
            &random,
        )
        .unwrap();
    let keeps_copy = store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::SignedOut)),
            &clock(3),
            &random,
        )
        .unwrap();
    for (index, owner) in [active.id, empty.id, keeps_copy.id].into_iter().enumerate() {
        let mut copy = conflict(index as u8 + 1);
        copy.staged_path = None;
        store.store_conflict(owner, &copy).unwrap();
    }
    let mut unresolved = conflict(4);
    unresolved.staged_path = None;
    unresolved.resolved_at_ms = None;
    store.store_conflict(keeps_copy.id, &unresolved).unwrap();
    drop(store);
    let foreign_build = scope("another.build", Environment::Production);
    let mut foreign = open(directory.path(), foreign_build.clone());
    let owner = foreign
        .create_binding(facts(BindingState::Active), &clock(5), &random)
        .unwrap();
    let mut copy = conflict(5);
    copy.staged_path = None;
    foreign.store_conflict(owner.id, &copy).unwrap();
    // Real inbox/cursor work, followed by the existing test-only isolation seam.
    foreign
        .commit_fetched(owner.id, &[record()], &OpaqueBytes(vec![7]), &clock(6))
        .unwrap();
    foreign
        .isolate_runtime_binding_for_testing(owner.id, IsolationReason::SignedOut)
        .unwrap();
    let inbox = foreign.inbox(owner.id).unwrap();
    let cursor = foreign.engine_state(owner.id).unwrap();
    drop(foreign);
    let mut store = open(directory.path(), build.clone());
    assert_eq!(store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).unwrap(), 4);
    assert!(store.binding(active.id).unwrap().is_some());
    assert!(store.binding(empty.id).unwrap().is_none());
    assert!(store.binding(keeps_copy.id).unwrap().is_some());
    assert_eq!(store.conflict_copies().unwrap().len(), 1);
    drop(store);
    let foreign = open(directory.path(), foreign_build);
    assert!(foreign.binding(owner.id).unwrap().is_some());
    assert_eq!(foreign.inbox(owner.id).unwrap(), inbox);
    assert_eq!(foreign.engine_state(owner.id).unwrap(), cursor);
    drop(foreign);
    assert!(open(directory.path(), build).binding(empty.id).unwrap().is_none());
}

fn expiry_media_fixture(directory: &Path) -> (TrackingStore, BindingId, std::path::PathBuf) {
    let mut store = open(directory, scope("test.container", Environment::Development));
    let owner = store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::SignedOut)),
            &clock(1),
            &SeededRandom::new(41),
        )
        .unwrap();
    let staging = directory.join("meguri-cloud-staging");
    fs::create_dir(&staging).unwrap();
    let path = staging.join("asset-synthetic-copy");
    fs::write(&path, b"synthetic staged losing media").unwrap();
    let mut copy = conflict(1);
    copy.kind = ConflictKind::MediaName;
    copy.staged_path = Some(path.to_str().unwrap().into());
    store.store_conflict(owner.id, &copy).unwrap();
    (store, owner.id, path)
}

#[test]
fn conflict_expiry_transaction_failure_keeps_rows_owner_and_file() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, owner, path) = expiry_media_fixture(directory.path());
    let copies = store.conflict_copies().unwrap();
    let db = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_expiry BEFORE DELETE ON bindings BEGIN SELECT RAISE(ABORT, 'synthetic transaction failure'); END;").unwrap();
    assert!(store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).is_err());
    assert_eq!(store.conflict_copies().unwrap(), copies);
    assert!(store.binding(owner).unwrap().is_some());
    store.finish_conflict_expiry().unwrap();
    assert_eq!(fs::read(path).unwrap(), b"synthetic staged losing media");
    db.execute_batch("DROP TRIGGER reject_expiry").unwrap();
}

#[test]
fn conflict_expiry_commit_then_reopen_replays_owned_unlink_durably() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, owner, path) = expiry_media_fixture(directory.path());
    assert_eq!(store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).unwrap(), 1);
    assert!(path.exists(), "transaction never unlinks before COMMIT");
    assert!(store.binding(owner).unwrap().is_none());
    drop(store); // Explicitly simulate the committed/unlinked boundary, not a kill.
    let mut store = open(directory.path(), scope("test.container", Environment::Development));
    assert!(store.conflict_copies().unwrap().is_empty());
    store.finish_conflict_expiry().unwrap();
    assert!(!path.exists());
    drop(store);
    let mut store = open(directory.path(), scope("test.container", Environment::Development));
    store.finish_conflict_expiry().unwrap();
    assert!(store.conflict_copies().unwrap().is_empty());
    assert!(store.binding(owner).unwrap().is_none());
}

#[test]
fn conflict_expiry_failed_unlink_retains_evidence_and_refuses_replaced_file() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _, path) = expiry_media_fixture(directory.path());
    store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).unwrap();
    let kept = directory.path().join("kept-original");
    fs::rename(&path, &kept).unwrap();
    fs::write(&path, b"replacement file must survive").unwrap();
    assert!(store.finish_conflict_expiry().is_err());
    assert_eq!(fs::read(&path).unwrap(), b"replacement file must survive");
    drop(store);
    let mut store = open(directory.path(), scope("test.container", Environment::Development));
    assert!(
        store.finish_conflict_expiry().is_err(),
        "failed evidence survives reopen"
    );
    fs::remove_file(&path).unwrap();
    fs::rename(kept, &path).unwrap();
    store.finish_conflict_expiry().unwrap();
    assert!(!path.exists());
}

#[test]
fn conflict_expiry_never_unlinks_arbitrary_paths_symlinks_or_shared_live_files() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let (mut store, owner, path) = expiry_media_fixture(directory.path());
    let private = directory.path().join("private-sentinel");
    fs::write(&private, b"synthetic private sentinel").unwrap();
    let link = path.parent().unwrap().join("asset-link");
    symlink(&private, &link).unwrap();
    for (id, staged) in [(2, private.clone()), (3, link.clone()), (4, path.clone())] {
        let mut copy = conflict(id);
        copy.kind = ConflictKind::MediaName;
        copy.staged_path = Some(staged.to_str().unwrap().into());
        if id == 4 {
            copy.resolved_at_ms = None;
        }
        store.store_conflict(owner, &copy).unwrap();
    }
    assert_eq!(store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).unwrap(), 3);
    store.finish_conflict_expiry().unwrap();
    assert_eq!(fs::read(private).unwrap(), b"synthetic private sentinel");
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert!(path.exists(), "unresolved copy retains shared bytes");
}

#[test]
fn conflict_expiry_retries_pending_bookkeeping_on_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let (mut store, _, path) = expiry_media_fixture(directory.path());
    store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).unwrap();
    let db = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    db.execute_batch("CREATE TRIGGER hold_pending_bookkeeping BEFORE DELETE ON cloud_conflict_unlinks BEGIN SELECT RAISE(ABORT, 'synthetic bookkeeping failure'); END;").unwrap();
    assert!(store.finish_conflict_expiry().is_err());
    assert!(
        !path.exists(),
        "unlink and directory barrier completed before bookkeeping"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM cloud_conflict_unlinks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(store);
    db.execute_batch("DROP TRIGGER hold_pending_bookkeeping").unwrap();
    drop(db);
    let mut store = open(directory.path(), scope("test.container", Environment::Development));
    store.finish_conflict_expiry().unwrap();
    drop(store);
    let db = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM cloud_conflict_unlinks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn conflict_expiry_staging_directory_link_cannot_select_another_directory() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (mut store, _, path) = expiry_media_fixture(directory.path());
    let staging = path.parent().unwrap();
    fs::rename(staging, directory.path().join("original-staging")).unwrap();
    let sentinel = elsewhere.path().join(path.file_name().unwrap());
    fs::write(&sentinel, b"synthetic external bytes").unwrap();
    symlink(elsewhere.path(), staging).unwrap();
    assert!(store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).is_err());
    assert_eq!(store.conflict_copies().unwrap().len(), 1);
    assert_eq!(fs::read(sentinel).unwrap(), b"synthetic external bytes");
}

#[test]
fn conflict_expiry_sigabrt_child() {
    let Ok(directory) = std::env::var("MEGURI_CONFLICT_EXPIRY_CHILD") else {
        return;
    };
    let mut store = open(Path::new(&directory), scope("test.container", Environment::Development));
    assert_eq!(store.expire_resolved_conflicts(456 + COPY_RETENTION_MS).unwrap(), 1);
    // Actual process death at the public transaction/finish boundary. This is
    // not injection of an unlink, SQLite COMMIT or fsync syscall failure.
    std::process::abort();
}

#[test]
fn conflict_expiry_sigabrt_after_commit_recovers_only_owned_staged_file() {
    use std::{os::unix::process::ExitStatusExt, process::Command};
    let directory = tempfile::tempdir().unwrap();
    let (store, owner, path) = expiry_media_fixture(directory.path());
    drop(store);
    let sentinel = directory.path().join("synthetic-live-media");
    fs::write(&sentinel, b"live bytes remain").unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "conflict_expiry_sigabrt_child", "--nocapture"])
        .env("MEGURI_CONFLICT_EXPIRY_CHILD", directory.path())
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGABRT));
    assert!(path.exists());
    let mut store = open(directory.path(), scope("test.container", Environment::Development));
    assert!(store.conflict_copies().unwrap().is_empty());
    assert!(store.binding(owner).unwrap().is_none());
    store.finish_conflict_expiry().unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read(sentinel).unwrap(), b"live bytes remain");
}
