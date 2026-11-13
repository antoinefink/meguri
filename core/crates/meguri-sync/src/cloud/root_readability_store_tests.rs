//! Logical-schema/transaction checks. Runtime producer coverage lives in Host.
use super::*;
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};

fn fixture() -> (tempfile::TempDir, TrackingStore, Binding) {
    let dir = tempfile::tempdir().unwrap();
    let OpenOutcome::Missing(mut store) = TrackingStore::open(dir.path(), BuildScope {
        container: ContainerId::new("synthetic-root-readability").unwrap(), environment: Environment::Development,
    }).unwrap() else { panic!("new store required"); };
    store.install_runtime().unwrap();
    let binding = store.create_binding(BindingFacts {
        account: OpaqueBytes(vec![1]), epoch: EpochId([2; 16]), record_key: RecordKey([3; 32]),
        publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]), state: BindingState::Active,
    }, &FixedClock::new(1000, UtcOffset::UTC), &SeededRandom::new(1936)).unwrap();
    (dir, store, binding)
}

#[test]
fn root_unreadable_schema_migration_retains_unknown_not_proven_clean() {
    let (_dir, mut store, b) = fixture();
    store.connection.execute_batch("DROP TABLE cloud_root_readability; UPDATE cloud_publication_schema SET version=3;").unwrap();
    store.install_runtime().unwrap();
    let count: i64 = store.connection.query_row("SELECT count(*) FROM cloud_root_readability", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0, "neither legacy crt nor Active backfills readability");
    assert_eq!(store.root_unreadable(b.id).unwrap(), None);
    let version: i64 = store.connection.query_row("SELECT version FROM cloud_publication_schema", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 4);
    store.scope.environment = Environment::Production;
    assert!(matches!(store.root_unreadable(b.id), Err(Error::BindingNotFound)));
}

fn apply_active(store: &mut TrackingStore, binding: BindingId, active: workflow::Saved) {
    let durable = adapter::Durable { active: Some(active), ..store.adapter_durable(binding).unwrap() };
    store.adapter_commit(binding, &[], &[], None, &durable, adapter_store::AdapterCommitOptions {
        deleted_collection_zones: Vec::new(), at_ms: 1000, fail: false, start_absence: false,
        resolved_epoch: None, fetched_epoch: None, root_intake: true, epoch_readability: None,
    }).unwrap();
}

fn confirm_retained_active(store: &mut TrackingStore, binding: BindingId) {
    let active = store.adapter_durable(binding).unwrap().active.unwrap();
    let epoch = store.binding(binding).unwrap().unwrap().facts.epoch;
    let open = store.runtime_binding(binding).unwrap().open_counter;
    store.save_root_ack(binding, workflow::RootAcknowledgement {
        open, request_id: 1, epoch,
        envelope: OpaqueBytes(active.envelope), system_fields: OpaqueBytes(active.system_fields),
    }).unwrap();
}

fn assert_knowledge(store: &TrackingStore, binding: BindingId, knowledge: &str, status: &str) {
    let actual: (String, String) = store.connection.query_row(
        "SELECT epoch_knowledge,status FROM cloud_root_readability WHERE binding=?1",
        [binding.0.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(actual, (knowledge.into(), status.into()));
    root_readability_store::validate(&store.connection).unwrap();
}

#[test]
fn root_unreadable_migrated_active_only_and_retained_confirmation_keep_epoch_unknown() {
    for retained in [false, true] {
        let (_dir, mut store, b) = fixture();
        let readable = workflow::Saved {
            name: "active".into(), system_fields: vec![7], envelope: control_codec::Active {
                epoch: b.facts.epoch.0, previous: None, activated_at_ms: 1000, replaced_through_ms: None,
            }.encode().unwrap(),
        };
        let complete = control_codec::Epoch {
            epoch_id: b.facts.epoch.0, record_key: b.facts.record_key.0, crt: 1234, ver: 18,
            state: control_codec::EpochState::Complete, reason: control_codec::EpochReason::Initial,
            published_at_ms: 1000, counts: Vec::new(),
        };
        // V3 can retain qualified crt and Active while having no readability ledger.
        store.receive_records(b.id, &[], &[], Some(&complete), Some(true)).unwrap();
        if retained { apply_active(&mut store, b.id, readable.clone()); }
        store.connection.execute_batch("DROP TABLE cloud_root_readability; UPDATE cloud_publication_schema SET version=3;").unwrap();
        store.install_runtime().unwrap();
        assert_eq!(store.epoch_crt(b.id).unwrap(), Some(1234));
        if retained { confirm_retained_active(&mut store, b.id); }
        else { apply_active(&mut store, b.id, readable.clone()); }
        assert_knowledge(&store, b.id, "unknown", "unknown");
        assert_eq!(store.root_unreadable(b.id).unwrap(), None);
        confirm_retained_active(&mut store, b.id);
        assert_knowledge(&store, b.id, "unknown", "unknown");

        let bad = workflow::Saved { envelope: vec![255], system_fields: vec![8], ..readable.clone() };
        apply_active(&mut store, b.id, bad.clone());
        assert_knowledge(&store, b.id, "unknown", "unreadable");
        apply_active(&mut store, b.id, readable.clone());
        confirm_retained_active(&mut store, b.id);
        assert_knowledge(&store, b.id, "unknown", "unreadable");
        assert_eq!(store.root_unreadable(b.id).unwrap(), Some(OpaqueBytes(bad.system_fields)));

        // Observe real bodies through the normal decoder and store intake;
        // the older qualified crt cannot stand in for the latest Epoch body.
        let mut epoch = workflow::Saved { name: "epoch".into(), envelope: vec![255], system_fields: vec![9] };
        let observed = root_readability_store::epoch_observation(&b, &[epoch.clone()], &[]).unwrap();
        store.receive_records(b.id, &[], &[], None, observed).unwrap();
        assert_knowledge(&store, b.id, "incomplete", "unreadable");
        assert_eq!(store.epoch_crt(b.id).unwrap(), Some(1234));
        confirm_retained_active(&mut store, b.id);
        assert_eq!(store.root_unreadable(b.id).unwrap(), Some(OpaqueBytes(readable.system_fields.clone())));
        epoch.envelope = complete.encode().unwrap();
        let observed = root_readability_store::epoch_observation(&b, &[epoch.clone()], &[]).unwrap();
        let fetched = epoch_crt_store::fetched(&b, &[epoch.clone()]);
        store.receive_records(b.id, &[], &[], fetched.as_ref(), observed).unwrap();
        assert_knowledge(&store, b.id, "complete", "readable");
        assert_eq!(store.root_unreadable(b.id).unwrap(), None);
        epoch.envelope = vec![255];
        let observed = root_readability_store::epoch_observation(&b, &[epoch], &[]).unwrap();
        store.receive_records(b.id, &[], &[], None, observed).unwrap();
        confirm_retained_active(&mut store, b.id);
        assert_knowledge(&store, b.id, "incomplete", "unreadable");
        assert_eq!(store.epoch_crt(b.id).unwrap(), Some(1234));
    }
}

#[test]
fn root_unreadable_schema_checks_and_validation_refuse_unproven_readable_status() {
    let (_dir, store, b) = fixture();
    for knowledge in [None, Some("unknown"), Some("incomplete"), Some("invalid")] {
        assert!(store.connection.execute(
            "INSERT INTO cloud_root_readability VALUES(?1,?2,'readable',NULL)",
            rusqlite::params![b.id.0.as_slice(), knowledge],
        ).is_err());
    }
    store.connection.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
    for knowledge in ["unknown", "incomplete", "invalid"] {
        store.connection.execute("INSERT INTO cloud_root_readability VALUES(?1,?2,'readable',NULL)",
            rusqlite::params![b.id.0.as_slice(), knowledge]).unwrap();
        assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::InvalidData)));
        store.connection.execute("DELETE FROM cloud_root_readability", []).unwrap();
    }
}

#[test]
fn root_unreadable_schema_refuses_inconsistent_future_or_malformed_metadata() {
    let (_dir, store, b) = fixture();
    store.connection.execute("UPDATE cloud_publication_schema SET version=5", []).unwrap();
    assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::RuntimeSchemaUnsupported)));
    store.connection.execute("UPDATE cloud_publication_schema SET version=3", []).unwrap();
    assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::InvalidData)));
    store.connection.execute("UPDATE cloud_publication_schema SET version=4", []).unwrap();
    store.connection.execute_batch("PRAGMA ignore_check_constraints=ON;").unwrap();
    store.connection.execute("INSERT INTO cloud_root_readability VALUES(?1,'complete','unreadable',X'')", [b.id.0.as_slice()]).unwrap();
    assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::InvalidData)));
    store.connection.execute("DELETE FROM cloud_root_readability", []).unwrap();
    store.connection.execute_batch("DROP TABLE cloud_root_readability;").unwrap();
    assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::InvalidData)));
}

#[test]
fn root_unreadable_mark_and_actual_active_roll_back_with_inbox_cursor() {
    let (_dir, mut store, b) = fixture();
    let before = store.adapter_durable(b.id).unwrap();
    let active = workflow::Saved { name: "active".into(), envelope: vec![255], system_fields: vec![7,8] };
    let durable = adapter::Durable { active: Some(active), ..before.clone() };
    store.connection.execute_batch("CREATE TEMP TRIGGER reject_root_readability BEFORE INSERT ON cloud_root_readability BEGIN SELECT RAISE(ABORT,'root attention constraint'); END;").unwrap();
    assert!(store.adapter_commit(b.id, &[], &[], Some(b"synthetic-cursor"), &durable, adapter_store::AdapterCommitOptions {
        deleted_collection_zones: Vec::new(), at_ms: 1000, fail: false, start_absence: false,
        resolved_epoch: None, fetched_epoch: None, root_intake: true, epoch_readability: None,
    }).is_err());
    assert_eq!(store.root_unreadable(b.id).unwrap(), None);
    assert_eq!(store.adapter_durable(b.id).unwrap().active, before.active);
    let cursors: i64 = store.connection.query_row("SELECT count(*) FROM engine_state", [], |r| r.get(0)).unwrap();
    assert_eq!(cursors, 0);
    store.connection.execute_batch("DROP TRIGGER reject_root_readability;").unwrap();
    store.adapter_commit(b.id, &[], &[], None, &durable, adapter_store::AdapterCommitOptions {
        deleted_collection_zones: Vec::new(), at_ms: 1000, fail: false, start_absence: false,
        resolved_epoch: None, fetched_epoch: None, root_intake: true, epoch_readability: None,
    }).unwrap();
    assert_eq!(store.root_unreadable(b.id).unwrap(), Some(OpaqueBytes(vec![7,8])));
    // Clearing even a syntactically supported Active needs complete-Epoch proof.
    let readable = workflow::Saved { envelope: control_codec::Active {
        epoch: b.facts.epoch.0, previous: None, activated_at_ms: 1000, replaced_through_ms: None,
    }.encode().unwrap(), ..durable.active.clone().unwrap() };
    let tx = store.connection.transaction().unwrap();
    root_readability_store::observe(&tx, b.id, Some(&readable), Some(false)).unwrap();
    tx.commit().unwrap();
    assert!(store.root_unreadable(b.id).unwrap().is_some());
}

#[test]
fn root_unreadable_public_open_refuses_invalid_existing_home_without_source_changes() {
    use std::{fs, os::unix::fs::PermissionsExt};
    fn bytes(dir: &std::path::Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(dir).unwrap().map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), fs::read(entry.path()).unwrap())
        }).collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }
    for read_only in [false, true] {
        for damage in ["future", "old-marker", "unmarked", "missing-table", "malformed"] {
            let (dir, store, b) = fixture();
            match damage {
                "future" => store.connection.execute_batch("UPDATE cloud_publication_schema SET version=5").unwrap(),
                "old-marker" => store.connection.execute_batch("UPDATE cloud_publication_schema SET version=3").unwrap(),
                "unmarked" => store.connection.execute_batch("DROP TABLE cloud_publication_schema").unwrap(),
                "missing-table" => store.connection.execute_batch("DROP TABLE cloud_root_readability").unwrap(),
                "malformed" => {
                    store.connection.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
                    store.connection.execute("INSERT INTO cloud_root_readability VALUES(?1,'complete','unreadable',X'')", [b.id.0.as_slice()]).unwrap();
                }
                _ => unreachable!(),
            }
            // Keep the actual source writer alive: the extension change is in
            // its WAL. Only the private copy may recover/checkpoint that WAL.
            let before = bytes(dir.path());
            let permissions: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|entry| {
                let path = entry.unwrap().path();
                let permissions = fs::metadata(&path).unwrap().permissions();
                (path, permissions)
            }).collect();
            let dir_permissions = fs::metadata(dir.path()).unwrap().permissions();
            if read_only {
                for (path, _) in &permissions { fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap(); }
                fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
            }
            let outcome = TrackingStore::open(dir.path(), store.scope.clone());
            if damage == "future" {
                assert!(matches!(outcome, Err(Error::RuntimeSchemaUnsupported)));
            } else {
                assert!(matches!(outcome, Ok(OpenOutcome::Damaged)));
            }
            assert_eq!(bytes(dir.path()), before, "public probe must preserve every source byte and sidecar");
            fs::set_permissions(dir.path(), dir_permissions).unwrap();
            for (path, permissions) in permissions { fs::set_permissions(path, permissions).unwrap(); }
            drop(store);
        }
    }
}
