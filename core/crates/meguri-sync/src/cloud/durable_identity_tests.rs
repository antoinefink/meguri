use super::{durable_identity::DurableFileIdentity, runtime::*, *};
use meguri_storage::persistent_identity::PersistentIdentity;

fn native() -> NativeFileIdentity {
    NativeFileIdentity {
        device: 17,
        inode: 29,
        created_seconds: 41,
        created_nanos: 53,
    }
}
fn durable(file: NativeFileIdentity) -> DurableFileIdentity {
    DurableFileIdentity {
        version: 1,
        identity: if cfg!(target_vendor = "apple") {
            PersistentIdentity::Darwin {
                volume_uuid: [3; 16],
                inode: file.inode,
            }
        } else {
            PersistentIdentity::NonDarwin {
                device: file.device,
                inode: file.inode,
            }
        },
        created_seconds: file.created_seconds,
        created_nanos: file.created_nanos,
    }
}
fn stored() -> StoredRuntimeState {
    StoredRuntimeState {
        file: Some(native()),
        durable_file: Some(durable(native())),
        change_counter: 7,
        open_counter: 11,
        note_scan: Some(NoteScanCursor {
            seq: 2,
            generation: 3,
            covered_through: 2,
            native_file: native(),
            modified: 5,
            schema_modified: 6,
        }),
        ..Default::default()
    }
}

#[test]
fn durable_identity_legacy_exact_upgrade_mismatch_and_absent_are_distinct() {
    let mut old = stored();
    old.durable_file = None;
    assert!(old.reopen_matches(native(), durable(native())).unwrap());
    let changed = NativeFileIdentity { device: 99, ..native() };
    assert!(!old.reopen_matches(changed, durable(changed)).unwrap());
    old.record_identity(native(), durable(native())).unwrap();
    assert_eq!(old.durable_file, Some(durable(native())));
    assert!(
        StoredRuntimeState::default()
            .reopen_matches(native(), durable(native()))
            .unwrap()
    );
}

#[test]
fn durable_identity_independent_component_changes_refuse() {
    let old = stored();
    for file in [
        NativeFileIdentity { inode: 99, ..native() },
        NativeFileIdentity {
            created_seconds: 99,
            ..native()
        },
        NativeFileIdentity {
            created_nanos: 99,
            ..native()
        },
    ] {
        assert!(!old.reopen_matches(file, durable(file)).unwrap());
    }
    let mut volume = durable(native());
    if let PersistentIdentity::Darwin { volume_uuid, .. } = &mut volume.identity {
        *volume_uuid = [4; 16];
        assert!(!old.reopen_matches(native(), volume).unwrap());
    }
}

#[test]
fn durable_identity_malformed_unknown_zero_and_foreign_platform_refuse() {
    let original = serde_json::to_value(stored()).unwrap();
    for bad in [
        serde_json::json!(null),
        serde_json::json!({}),
        serde_json::json!({"version": 99}),
    ] {
        let mut value = original.clone();
        value["durable_file"] = bad;
        assert!(serde_json::from_value::<StoredRuntimeState>(value).is_err());
    }
    let mut unknown = durable(native());
    unknown.version = 99;
    assert!(unknown.validate().is_err());
    unknown = durable(native());
    unknown.created_nanos = 1_000_000_000;
    assert!(unknown.validate().is_err());
    unknown = durable(native());
    unknown.identity = PersistentIdentity::Darwin {
        volume_uuid: [0; 16],
        inode: native().inode,
    };
    assert!(unknown.validate().is_err());
    unknown.identity = if cfg!(target_vendor = "apple") {
        PersistentIdentity::NonDarwin { device: 17, inode: 29 }
    } else {
        PersistentIdentity::Darwin {
            volume_uuid: [3; 16],
            inode: 29,
        }
    };
    assert!(unknown.validate().is_err());
}

#[cfg(not(target_vendor = "apple"))]
#[test]
fn durable_identity_non_darwin_keeps_exact_native_comparison() {
    let changed = NativeFileIdentity { device: 99, ..native() };
    assert!(!stored().reopen_matches(changed, durable(changed)).unwrap());
}

#[test]
fn durable_identity_birthtime_bind_race_refuses_before_tracking() {
    let f = native();
    let evidence = meguri_storage::persistent_identity::CloudFileEvidence {
        native: (f.device, f.inode, f.created_seconds, f.created_nanos),
        identity: durable(f).identity,
    };
    assert!(DurableFileIdentity::from_evidence(Some(evidence), f).is_ok());
    for changed in [
        NativeFileIdentity { device: 99, ..f },
        NativeFileIdentity { inode: 99, ..f },
        NativeFileIdentity {
            created_seconds: 99,
            ..f
        },
        NativeFileIdentity { created_nanos: 99, ..f },
    ] {
        assert!(DurableFileIdentity::from_evidence(Some(evidence), changed).is_err());
    }
    assert!(DurableFileIdentity::from_evidence(None, f).is_err());
}

#[cfg(target_vendor = "apple")]
#[test]
fn durable_identity_device_only_drift_invalidates_authorities_without_relabeling_cursor() {
    let mut old = stored();
    let cursor = old.note_scan.clone().unwrap();
    let current = NativeFileIdentity { device: 99, ..native() };
    assert!(old.reopen_matches(current, durable(current)).unwrap());
    old.record_identity(current, durable(current)).unwrap();
    assert_eq!(old.file, Some(current));
    assert_eq!(old.change_counter, 7);
    assert_eq!(old.open_counter, 11);
    assert!(old.note_scan.is_none());
    assert!(old.coverage.is_none());
    assert!(!old.whole_scan && !old.drained);
    assert_eq!(cursor.native_file, native());
}

#[test]
fn durable_identity_native_and_durable_pair_rollback_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let scope = BuildScope {
        container: ContainerId::new("durable.identity").unwrap(),
        environment: Environment::Development,
    };
    let open = || match TrackingStore::open(root.path(), scope.clone()).unwrap() {
        OpenOutcome::Missing(s) | OpenOutcome::Opened(s) => s,
        _ => panic!("fixture store refused"),
    };
    let mut store = open();
    store.install_runtime().unwrap();
    let binding = store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![1]),
                epoch: EpochId([2; 16]),
                record_key: RecordKey([3; 32]),
                publication: serde_json::from_str("[17,29]").unwrap(),
                state: BindingState::Active,
            },
            &meguri_model::clock::FixedClock::new(88, meguri_model::clock::UtcOffset::UTC),
            &meguri_model::rng::SeededRandom::new(19),
        )
        .unwrap()
        .id;
    let mut legacy = stored();
    legacy.durable_file = None;
    let tx = store.connection.transaction().unwrap();
    super::runtime_store::put_binding(&tx, binding, &legacy).unwrap();
    tx.commit().unwrap();
    store.connection.execute_batch("CREATE TEMP TRIGGER fail_pair BEFORE UPDATE ON cloud_runtime_bindings BEGIN SELECT RAISE(ABORT,'injected identity write failure'); END;").unwrap();
    assert!(
        store
            .open_runtime_binding(binding, native(), durable(native()))
            .is_err()
    );
    assert_eq!(store.runtime_binding(binding).unwrap(), legacy);
    drop(store);
    let mut store = open();
    store.install_runtime().unwrap();
    assert_eq!(store.runtime_binding(binding).unwrap(), legacy);
    let upgraded = store
        .open_runtime_binding(binding, native(), durable(native()))
        .unwrap();
    assert_eq!(upgraded.durable_file, Some(durable(native())));
    drop(store);
    let mut store = open();
    store.install_runtime().unwrap();
    assert_eq!(store.runtime_binding(binding).unwrap(), upgraded);
    store
        .connection
        .execute("UPDATE cloud_runtime_schema SET version=6", [])
        .unwrap();
    assert!(store.stored_runtime_state(binding).is_err());
    let changes = store.connection.total_changes();
    let tx = store.connection.transaction().unwrap();
    assert!(matches!(
        super::tracking_payload::migrate(&tx, &scope),
        Err(Error::InvalidData)
    ));
    tx.commit().unwrap();
    assert_eq!(store.connection.total_changes(), changes);
    store
        .connection
        .execute("UPDATE cloud_runtime_schema SET version=8", [])
        .unwrap();
    store
        .isolate_runtime_binding(binding, IsolationReason::SignedOut)
        .unwrap();
    let isolated = store.binding(binding).unwrap().unwrap();
    let resumed = store
        .resume_isolated_binding(&isolated, native(), durable(native()), &None, &None)
        .unwrap();
    assert_eq!(resumed.facts.state, BindingState::Active);
    assert_eq!(
        store.runtime_binding(binding).unwrap().durable_file,
        Some(durable(native()))
    );
}

#[test]
fn durable_identity_new_publication_binding_records_pair_in_create_commit() {
    use super::publication::*;
    let root = tempfile::tempdir().unwrap();
    let scope = BuildScope {
        container: ContainerId::new("durable.publication").unwrap(),
        environment: Environment::Development,
    };
    let mut store = match TrackingStore::open(root.path(), scope).unwrap() {
        OpenOutcome::Missing(s) => s,
        _ => panic!("new store"),
    };
    store.install_runtime().unwrap();
    let publication = meguri_engine::device_state::Publication {
        identity: serde_json::from_str("[17,29]").unwrap(),
        binding: meguri_engine::device_state::AccountBinding::LocalOnly,
        decision: meguri_engine::device_state::PublicationDecision::Clear,
    };
    let mut op = PublishOp {
        id: [1; 16],
        step: PublishStep::Confirmed,
        binding_before: None,
        binding_after: None,
        account: OpaqueBytes(vec![1]),
        confirmed_publication: publication.identity,
        confirmed_file: native(),
        fence_before: DecisionSnapshot {
            publication,
            pending_reason: None,
        },
        epoch: EpochId([2; 16]),
        record_key: RecordKey([3; 32]),
        media_key: RecordKey([4; 32]),
        active: None,
        active_epoch: None,
        epoch_system_fields: None,
        complete_system_fields: None,
        root_acked: false,
        switch_sent: false,
        switch_conflict: None,
        active_attempt: None,
        started_at_ms: 88,
        retry_after_ms: None,
        crt: 77,
        reason: 1,
    };
    store.confirm_publication(&op).unwrap();
    let binding = store
        .bind_publication(&mut op, BindingId([8; 16]), 88, durable(native()), None)
        .unwrap();
    let saved = store.runtime_binding(binding.id).unwrap();
    assert_eq!(saved.file, Some(native()));
    assert_eq!(saved.durable_file, Some(durable(native())));
    assert_eq!(store.publication_op().unwrap().unwrap().binding_after, Some(binding.id));
}
