//! Actual tracking schema/storage invariants, without a replacement cleanup model.
use super::*;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};

fn open(root: &Path) -> TrackingStore {
    let scope = BuildScope {
        container: ContainerId::new("test.expiry").unwrap(),
        environment: Environment::Development,
    };
    match TrackingStore::open(root, scope).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
        _ => panic!("expected synthetic supported store"),
    }
}

#[test]
fn expiry_work_inventory_covers_every_binding_column_in_actual_tracking_schema() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    store.install_runtime().unwrap();
    store.adapter_install().unwrap();
    let tables: Vec<String> = store
        .connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for table in tables {
        let columns: Vec<String> = store
            .connection
            .prepare("SELECT name FROM pragma_table_info(?1)")
            .unwrap()
            .query_map([&table], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        if table != "conflicts" && columns.iter().any(|c| c == "binding") {
            assert!(
                WORK_TABLES.contains(&table.as_str()) || EXTRA_WORK_TABLES.contains(&table.as_str()),
                "expiry must retain owners with work in actual table {table}"
            );
        }
    }
}

#[test]
fn isolated_owner_with_only_real_policy_evidence_keeps_it_when_last_copy_expires() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    store.install_cloud_policy().unwrap();
    let binding = store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![7]),
                epoch: EpochId([8; 16]),
                record_key: RecordKey([9; 32]),
                publication: serde_json::from_str("[1,2]").unwrap(),
                state: BindingState::Isolated(IsolationReason::SignedOut),
            },
            &FixedClock::new(1, UtcOffset::UTC),
            &SeededRandom::new(111),
        )
        .unwrap();
    let evidence = store.record_cloud_quiet(binding.id, 123).unwrap();
    let copy = ConflictCopy {
        id: CopyId([1; 16]),
        kind: ConflictKind::Note,
        key: OpaqueBytes(vec![1]),
        reason: ConflictReason::FieldConflict,
        losing_envelope: OpaqueBytes(vec![2]),
        losing_payload_digest: PayloadDigest([2; 32]),
        winning_payload_digest: PayloadDigest([3; 32]),
        field_names: None,
        template_names: None,
        note_deck: None,
        staged_path: None,
        created_at_ms: 1,
        resolved_at_ms: Some(2),
    };
    store.store_conflict(binding.id, &copy).unwrap();
    assert_eq!(store.expire_resolved_conflicts(2 + RETENTION_MS).unwrap(), 1);
    assert!(store.binding(binding.id).unwrap().is_some());
    assert_eq!(store.cloud_policy_evidence(binding.id).unwrap(), evidence);
    drop(store);
    let store = open(root.path());
    assert!(store.binding(binding.id).unwrap().is_some());
    assert_eq!(store.cloud_policy_evidence(binding.id).unwrap(), evidence);
}

#[test]
fn staging_sweep_retains_resolved_copy_until_its_expiry_commit() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let binding = store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![1]),
                epoch: EpochId([2; 16]),
                record_key: RecordKey([3; 32]),
                publication: serde_json::from_str("[4,5]").unwrap(),
                state: BindingState::Active,
            },
            &FixedClock::new(1, UtcOffset::UTC),
            &SeededRandom::new(112),
        )
        .unwrap();
    let staging = root.path().join(STAGING);
    fs::create_dir(&staging).unwrap();
    let retained = staging.join("asset-retained");
    let orphan = staging.join("asset-orphan");
    fs::write(&retained, b"synthetic saved conflict").unwrap();
    fs::write(&orphan, b"synthetic interrupted download").unwrap();
    store
        .store_conflict(
            binding.id,
            &ConflictCopy {
                id: CopyId([1; 16]),
                kind: ConflictKind::MediaName,
                key: OpaqueBytes(vec![1]),
                reason: ConflictReason::MediaContent,
                losing_envelope: OpaqueBytes(vec![2]),
                losing_payload_digest: PayloadDigest([2; 32]),
                winning_payload_digest: PayloadDigest([3; 32]),
                field_names: None,
                template_names: None,
                note_deck: None,
                staged_path: Some(retained.to_str().unwrap().into()),
                created_at_ms: 1,
                resolved_at_ms: Some(2),
            },
        )
        .unwrap();
    store.sweep_conflict_staging().unwrap();
    assert!(retained.exists());
    assert!(!orphan.exists());
    assert_eq!(store.expire_resolved_conflicts(2 + RETENTION_MS - 1).unwrap(), 0);
    assert!(retained.exists());
    assert_eq!(store.expire_resolved_conflicts(2 + RETENTION_MS).unwrap(), 1);
    assert!(retained.exists());
    store.finish_conflict_expiry().unwrap();
    assert!(!retained.exists());
}

fn pending_asset_fixture(root: &Path) -> (TrackingStore, BindingId, PathBuf, media::blob::Blob) {
    let mut store = open(root);
    store.install_runtime().unwrap();
    let binding = store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![1]),
                epoch: EpochId([2; 16]),
                record_key: RecordKey([3; 32]),
                publication: serde_json::from_str("[4,5]").unwrap(),
                state: BindingState::Active,
            },
            &FixedClock::new(1, UtcOffset::UTC),
            &SeededRandom::new(113),
        )
        .unwrap();
    let source = root.join("synthetic-source");
    fs::write(&source, b"synthetic pending media").unwrap();
    let (path, metadata) = media::io::stage(fs::File::open(source).unwrap(), &root.join(STAGING)).unwrap();
    store
        .store_conflict(
            binding.id,
            &ConflictCopy {
                id: CopyId([1; 16]),
                kind: ConflictKind::MediaName,
                key: OpaqueBytes(vec![1]),
                reason: ConflictReason::MediaContent,
                losing_envelope: OpaqueBytes(vec![2]),
                losing_payload_digest: PayloadDigest([2; 32]),
                winning_payload_digest: PayloadDigest([3; 32]),
                field_names: None,
                template_names: None,
                note_deck: None,
                staged_path: Some(path.to_str().unwrap().into()),
                created_at_ms: 1,
                resolved_at_ms: Some(2),
            },
        )
        .unwrap();
    (store, binding.id, path, metadata)
}

fn expire_pending_asset_copy(store: &mut TrackingStore, path: &Path) {
    assert_eq!(store.expire_resolved_conflicts(2 + RETENTION_MS).unwrap(), 1);
    assert!(store.conflict_copies().unwrap().is_empty());
    assert_eq!(fs::read(path).unwrap(), b"synthetic pending media");
}

fn check_pending_asset_retention(store: &mut TrackingStore, path: &Path) {
    assert!(store.conflict_copies().unwrap().is_empty());
    store.finish_conflict_expiry().unwrap();
    assert_eq!(fs::read(path).unwrap(), b"synthetic pending media");
    let orphan = path.parent().unwrap().join("asset-unreferenced");
    fs::write(&orphan, b"synthetic orphan").unwrap();
    store.sweep_conflict_staging().unwrap();
    assert!(!orphan.exists());
    assert_eq!(fs::read(path).unwrap(), b"synthetic pending media");
}

#[test]
fn staging_ledger_reference_survives_copy_expiry_finish_sweep_and_reopen() {
    for reference_first in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, binding, path, metadata) = pending_asset_fixture(root.path());
        if !reference_first {
            expire_pending_asset_copy(&mut store, &path);
        }
        let name = metadata.name(&[3; 32]);
        let blob = media::BlobState {
            metadata,
            download: media::Transfer::Staged,
            staged: Some(path.clone()),
            ..Default::default()
        };
        let mut ledger = store.media_ledger(binding).unwrap();
        ledger.blobs.insert(name.clone(), blob.clone());
        store.put_media(binding, &ledger).unwrap();
        if reference_first {
            expire_pending_asset_copy(&mut store, &path);
        }
        check_pending_asset_retention(&mut store, &path);
        assert_eq!(store.media_ledger(binding).unwrap().blobs.get(&name), Some(&blob));
        drop(store);
        let mut store = open(root.path());
        assert_eq!(store.media_ledger(binding).unwrap().blobs.get(&name), Some(&blob));
        check_pending_asset_retention(&mut store, &path);
    }
}

#[test]
fn staging_inbox_reference_survives_copy_expiry_finish_sweep_and_reopen() {
    for reference_first in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, binding, path, _) = pending_asset_fixture(root.path());
        if !reference_first {
            expire_pending_asset_copy(&mut store, &path);
        }
        let entry = InboxEntry {
            record_type: RecordType::Entity,
            record_name: "synthetic-pending-record".into(),
            value: InboxValue::Record {
                erev: 1,
                system_fields: OpaqueBytes(vec![1]),
                envelope: OpaqueBytes(vec![2]),
                staged_asset: Some(path.to_str().unwrap().into()),
            },
            reason: InboxReason::Asset,
            source: InboxSource::Fetch,
            absence_pass: None,
            retry_count: 0,
            fetched_at_ms: 1,
        };
        store
            .receive_records(binding, std::slice::from_ref(&entry), &[], None, None)
            .unwrap();
        if reference_first {
            expire_pending_asset_copy(&mut store, &path);
        }
        check_pending_asset_retention(&mut store, &path);
        let entries = store.inbox(binding).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry, entry);
        drop(store);
        let mut store = open(root.path());
        let entries = store.inbox(binding).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry, entry);
        check_pending_asset_retention(&mut store, &path);
    }
}

#[test]
fn staging_apply_reference_survives_copy_expiry_finish_sweep_and_reopen() {
    for reference_first in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, binding, path, metadata) = pending_asset_fixture(root.path());
        if !reference_first {
            expire_pending_asset_copy(&mut store, &path);
        }
        let mut intent = incoming::ApplyIntent {
            id: 0,
            binding,
            inbox_ids: Vec::new(),
            keys: Vec::new(),
            result_seq: None,
            originating_open: Some(1),
            planned_at_ms: 1,
            native_file: runtime::NativeFileIdentity {
                device: 1,
                inode: 2,
                created_seconds: 3,
                created_nanos: 0,
            },
            generation: 1,
            media: Some(media::Apply {
                incoming: workflow::Saved {
                    name: "synthetic-pending-record".into(),
                    envelope: vec![2],
                    system_fields: vec![1],
                },
                server: Some(metadata.media("pending.bin")),
                old_name: "pending.bin".into(),
                before: None,
                moved: None,
                moved_before: None,
                move_evidence: None,
                install_absent: false,
                staged: Some(path.clone()),
                before_referenced_new: Default::default(),
                local_wins: false,
                delete_wins: false,
                installed: false,
                abandoned: false,
            }),
        };
        intent.id = store.persist_apply_intent(&intent, None).unwrap();
        if reference_first {
            expire_pending_asset_copy(&mut store, &path);
        }
        check_pending_asset_retention(&mut store, &path);
        assert_eq!(store.apply_intents(binding).unwrap(), vec![intent.clone()]);
        drop(store);
        let mut store = open(root.path());
        assert_eq!(store.apply_intents(binding).unwrap(), vec![intent]);
        check_pending_asset_retention(&mut store, &path);
    }
}
