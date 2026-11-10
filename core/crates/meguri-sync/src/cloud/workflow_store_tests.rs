use super::*;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};

fn fixture() -> (tempfile::TempDir, TrackingStore, BindingId, u64) {
    let directory = tempfile::tempdir().unwrap();
    let scope = BuildScope {
        container: ContainerId::new("synthetic-scan-provenance").unwrap(),
        environment: Environment::Development,
    };
    let OpenOutcome::Missing(mut store) = TrackingStore::open(directory.path(), scope).unwrap() else {
        panic!("new store");
    };
    store.install_runtime().unwrap();
    let binding = store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![1]),
                epoch: EpochId([2; 16]),
                record_key: RecordKey([3; 32]),
                publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                state: BindingState::Active,
            },
            &FixedClock::new(1000, UtcOffset::UTC),
            &SeededRandom::new(1916),
        )
        .unwrap();
    let state = store
        .open_runtime_binding(
            binding.id,
            runtime::NativeFileIdentity {
                device: 1,
                inode: 2,
                created_seconds: 1,
                created_nanos: 0,
            },
            runtime::DurableFileIdentity {
                version: 1,
                identity: if cfg!(target_vendor = "apple") {
                    meguri_storage::persistent_identity::PersistentIdentity::Darwin {
                        volume_uuid: [3; 16],
                        inode: 2,
                    }
                } else {
                    meguri_storage::persistent_identity::PersistentIdentity::NonDarwin { device: 1, inode: 2 }
                },
                created_seconds: 1,
                created_nanos: 0,
            },
        )
        .unwrap();
    (directory, store, binding.id, state.open_counter)
}

// Cover both the note adapter and generic entity storage with actual store APIs.
fn payloads() -> [codec::Payload; 2] {
    [
        codec::Payload::Note(codec::Note {
            id: 11,
            mid: 12,
            nt_struct_gen: 1,
            flds: b"front\x1fback".to_vec(),
            ..Default::default()
        }),
        codec::Payload::Config(codec::Config {
            key: b"scan-provenance".to_vec(),
            val: b"1".to_vec(),
            ..Default::default()
        }),
    ]
}

#[test]
fn guarded_scan_equal_payload_preserves_pending_facts_and_retained_flight() {
    let (_directory, mut store, binding, open) = fixture();
    for payload in payloads() {
        let row = store
            .insert_workflow_entity(binding, payload.clone(), [5; 16], (open, 10, 7))
            .unwrap();
        store
            .persist_flight(
                binding,
                &row.name,
                Flight {
                    local_rev: row.local_rev,
                    payload_digest: payload.payload_digest().unwrap(),
                    row_digest: payload.row_digest().unwrap(),
                    payload: payload.into(),
                    erev: 1,
                    image_seq: 10,
                    held: false,
                    tombstone: false,
                    tombstone_envelope_digest: None,
                },
            )
            .unwrap();
        let before = store.entity_facts(binding, &row.name).unwrap().unwrap();
        assert!(before.sent && !before.in_flight.is_empty() && before.mark.is_some());
        let counter = store.runtime_binding(binding).unwrap().change_counter;
        store.workflow_merge(binding, open, 20, 8, before.clone()).unwrap();
        let after = store.entity_facts(binding, &row.name).unwrap().unwrap();
        let mut expected = before;
        expected.observer_open = open;
        expected.observed_seq = 20;
        expected.observed_generation = 8;
        assert!(after == expected, "only actual scan provenance may change");
        assert_eq!(store.runtime_binding(binding).unwrap().change_counter, counter);
    }
}

#[test]
fn guarded_changed_scan_marks_snapshot_revision_and_rejects_older_or_invalid_open() {
    let (_directory, mut store, binding, open) = fixture();
    for payload in payloads() {
        let before = store
            .insert_workflow_entity(binding, payload, [6; 16], (open, 10, 7))
            .unwrap();
        let counter = store.runtime_binding(binding).unwrap().change_counter;
        let mut scanned = before.clone();
        match &mut scanned.payload {
            codec::Payload::Note(note) => note.flds = b"front\x1fchanged".to_vec(),
            codec::Payload::Config(config) => config.val = b"2".to_vec(),
            _ => unreachable!(),
        }
        store.workflow_merge(binding, open, 20, 7, scanned.clone()).unwrap();
        let committed = store.entity_facts(binding, &before.name).unwrap().unwrap();
        assert_eq!(committed.payload, scanned.payload);
        assert_eq!(committed.local_rev, before.local_rev + 1);
        let mark = committed.mark.as_ref().unwrap();
        assert_eq!(mark.source, DirtySource::Snapshot);
        assert_eq!(mark.change_seq, counter + 1);
        assert_eq!(mark.payload_digest, committed.payload.payload_digest().unwrap());
        assert_eq!(
            (
                committed.observer_open,
                committed.observed_seq,
                committed.observed_generation
            ),
            (open, 20, 7)
        );
        assert_eq!(store.runtime_binding(binding).unwrap().change_counter, counter + 1);
        // The old image must not replace a later commit from this open/generation.
        store.workflow_merge(binding, open, 19, 7, before.clone()).unwrap();
        assert!(store.entity_facts(binding, &before.name).unwrap().unwrap() == committed);
        for invalid_open in [open - 1, open + 1] {
            assert_eq!(
                store.workflow_merge(binding, invalid_open, 30, 8, before.clone()),
                Err(Error::InvalidData)
            );
            assert!(store.entity_facts(binding, &before.name).unwrap().unwrap() == committed);
        }
        assert_eq!(store.runtime_binding(binding).unwrap().change_counter, counter + 1);
    }
}
