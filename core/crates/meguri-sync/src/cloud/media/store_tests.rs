//! Transaction-local identical-feed admission; no Runtime or provider substitutes.
use super::*;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use rusqlite::{Connection, types::Value};

const NOW: i64 = 1_735_689_600_000;
const FEED: [u8; 16] = [7; 16];

fn fixture(ledger: Ledger) -> (tempfile::TempDir, TrackingStore, BindingId, Ledger) {
    let directory = tempfile::tempdir().unwrap();
    let scope = BuildScope {
        container: ContainerId::new("synthetic-identical-feed").unwrap(),
        environment: Environment::Development,
    };
    let OpenOutcome::Missing(mut store) = TrackingStore::open(directory.path(), scope).unwrap() else {
        panic!("new synthetic store")
    };
    store.install_runtime().unwrap();
    let binding = add_binding(&mut store, 1917);
    store
        .open_runtime_binding(
            binding,
            runtime::NativeFileIdentity {
                device: 1,
                inode: 2,
                created_seconds: 3,
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
                created_seconds: 3,
                created_nanos: 0,
            },
        )
        .unwrap();
    store.put_media(binding, &ledger).unwrap();
    let captured = store.media_ledger(binding).unwrap();
    store.put_media(binding, &captured).unwrap();
    (directory, store, binding, captured)
}

fn add_binding(store: &mut TrackingStore, seed: u64) -> BindingId {
    store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![1]),
                epoch: EpochId([2; 16]),
                record_key: RecordKey([3; 32]),
                publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                state: BindingState::Active,
            },
            &FixedClock::new(NOW, UtcOffset::UTC),
            &SeededRandom::new(seed),
        )
        .unwrap()
        .id
}

fn empty() -> Ledger {
    Ledger {
        feed: Some((FEED, 10)),
        ..Default::default()
    }
}

fn metadata() -> blob::Blob {
    blob::Blob {
        sha256: [8; 32],
        sha1: [9; 20],
        size: 3,
    }
}

fn local_media(store: &mut TrackingStore, binding: BindingId, name: &str) -> workflow::EntityFacts {
    let key = store.binding(binding).unwrap().unwrap().facts.record_key;
    let mut state = store.runtime_binding(binding).unwrap();
    let open = state.open_counter;
    let entity = crate::cloud::workflow_store::new_pending_entity(
        &key,
        c::Payload::MediaName(metadata().media(name)),
        [5; 16],
        runtime::DirtySource::Observer,
        &mut state.change_counter,
        (open, 2, 3),
    )
    .unwrap();
    let tx = store.connection.transaction().unwrap();
    crate::cloud::workflow_store::put(&tx, binding, &entity).unwrap();
    crate::cloud::runtime_store::put_binding(&tx, binding, &state).unwrap();
    tx.commit().unwrap();
    entity
}

#[test]
fn exchange_media_policy_distinguishes_blocked_names_from_nonpending_names() {
    for flag in [
        "ordinary",
        "unsyncable",
        "removal_held",
        "file_missing",
        "withheld",
        "superseded",
    ] {
        let (_directory, mut store, binding, _) = fixture(empty());
        let row = local_media(&mut store, binding, "public.bin");
        let mut ledger = store.media_ledger(binding).unwrap();
        let mut flags = NameState::default();
        match flag {
            "unsyncable" => flags.unsyncable = true,
            "removal_held" => flags.removal_held = true,
            "file_missing" => flags.file_missing = true,
            "withheld" => flags.withheld = Some([8; 32]),
            "superseded" => flags.superseded = Some([8; 32]),
            _ => {}
        }
        ledger.names.insert("public.bin".into(), flags);
        store.put_media(binding, &ledger).unwrap();
        let pending = store.policy_pending_names(binding).unwrap();
        let excluded = ["file_missing", "withheld", "superseded"].contains(&flag);
        assert_eq!(pending.contains(&row.name), !excluded, "{flag}");
        let blocked = store.blocked_exchange_names(binding, &pending).unwrap();
        assert_eq!(
            blocked.contains(&row.name),
            ["unsyncable", "removal_held"].contains(&flag),
            "{flag}"
        );
    }
}

fn saved(binding: BindingId, entity: &workflow::EntityFacts) -> workflow::Saved {
    let envelope = c::Entity::build(
        c::EntityFields {
            kind: entity.kind,
            key: entity.key.clone(),
            origin: entity.origin,
            erev: 1,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer: binding.0,
            base_sha256: None,
        },
        &entity.payload.encode().unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    workflow::Saved {
        name: entity.name.clone(),
        envelope,
        system_fields: vec![1],
    }
}

// Include every persisted table, not just the ledger or Runtime counters.
fn persisted(connection: &Connection) -> Vec<(String, Vec<Vec<Value>>)> {
    let tables = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let quoted = table.replace('"', "\"\"");
            let mut statement = connection.prepare(&format!("SELECT * FROM \"{quoted}\"")).unwrap();
            let columns = statement.column_count();
            let mut rows = statement
                .query_map([], |row| (0..columns).map(|i| row.get::<_, Value>(i)).collect())
                .unwrap()
                .collect::<Result<Vec<Vec<Value>>, _>>()
                .unwrap();
            // Table order is immaterial; value encoding gives a stable total order.
            rows.sort_by_cached_key(|row| format!("{row:?}"));
            (table, rows)
        })
        .collect()
}

fn refused(store: &mut TrackingStore, binding: BindingId, proposal: &Ledger, now: i64) {
    let before = persisted(&store.connection);
    assert_eq!(
        store.commit_media_feed(binding, proposal, &[], (1, 2, 3), now, None),
        Err(Error::RecoveryRequired)
    );
    assert_eq!(persisted(&store.connection), before);
}

#[test]
fn identical_advanced_empty_feed_is_false_and_preserves_every_persisted_table() {
    let mut body = empty();
    body.names.insert(
        "photo.bin".into(),
        NameState {
            local: Some(metadata().media("photo.bin")),
            withheld: Some([8; 32]),
            ..Default::default()
        },
    );
    body.blobs.insert(
        "synthetic-blob".into(),
        BlobState {
            metadata: metadata(),
            server: ServerState::Present,
            recheck_at: Some(NOW),
            ..Default::default()
        },
    );
    body.deferred_rechecks.insert(
        "synthetic-hash".into(),
        DeferredRecheck {
            metadata: metadata(),
            acked_at_ms: NOW,
            rechecked: false,
            final_rechecked: false,
        },
    );
    body.cleanup_drain_ms = Some(NOW);
    let (_directory, mut store, binding, proposal) = fixture(body);
    let before = persisted(&store.connection);
    assert_eq!(store.media_ledger(binding).unwrap().revision, proposal.revision + 1);
    assert_eq!(
        store.commit_media_feed(binding, &proposal, &[], (1, 2, 3), NOW, None),
        Ok(false)
    );
    assert_eq!(persisted(&store.connection), before);
    // The exception belongs only to feed commits, never to generic ledger CAS.
    assert_eq!(store.put_media(binding, &proposal), Err(Error::RecoveryRequired));
    assert_eq!(persisted(&store.connection), before);
}

#[test]
fn advanced_feed_with_different_media_or_ack_obligations_still_rolls_back() {
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let mut current = store.media_ledger(binding).unwrap();
    current.names.insert(
        "new.bin".into(),
        NameState {
            local: Some(metadata().media("new.bin")),
            ..Default::default()
        },
    );
    store.put_media(binding, &current).unwrap();
    refused(&mut store, binding, &proposal, NOW);

    let mut proposal = store.media_ledger(binding).unwrap();
    let mut ack = proposal.clone();
    ack.deferred_rechecks.insert(
        "synthetic-ack".into(),
        DeferredRecheck {
            metadata: metadata(),
            acked_at_ms: NOW,
            rechecked: false,
            final_rechecked: false,
        },
    );
    store.put_media(binding, &ack).unwrap();
    refused(&mut store, binding, &proposal, NOW);
    // A later final-drain obligation is semantic even with the same cursor.
    proposal = store.media_ledger(binding).unwrap();
    let mut later = proposal.clone();
    later.cleanup_drain_ms = Some(NOW);
    store.put_media(binding, &later).unwrap();
    refused(&mut store, binding, &proposal, NOW);
}

#[test]
fn actual_media_ack_after_capture_keeps_its_obligation_and_refuses_stale_feed() {
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let entity = local_media(&mut store, binding, "acked.bin");
    store
        .persist_flight(
            binding,
            &entity.name,
            workflow::Flight {
                local_rev: entity.local_rev,
                payload_digest: entity.payload.payload_digest().unwrap(),
                row_digest: entity.payload.row_digest().unwrap(),
                payload: entity.payload.clone().into(),
                erev: 1,
                image_seq: 2,
                held: false,
                tombstone: false,
                tombstone_envelope_digest: None,
            },
        )
        .unwrap();
    let saved = saved(binding, &entity);
    let received = c::receive(&[3; 32], &saved.name, &saved.envelope, c::AssetInput::Absent).unwrap();
    store.acknowledge(binding, &saved, received, NOW).unwrap();
    let acked = store.entity_facts(binding, &entity.name).unwrap().unwrap();
    assert_eq!(acked.server_erev, 1);
    assert!(acked.mark.is_none() && acked.acked_payload == Some(entity.payload));
    let ledger = store.media_ledger(binding).unwrap();
    assert!(ledger.revision > proposal.revision);
    assert_eq!(ledger.deferred_rechecks.len(), 1);
    refused(&mut store, binding, &proposal, NOW);
}

#[test]
fn future_revision_regressing_cursor_foreign_and_absent_feed_are_refused() {
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let mut future = proposal.clone();
    future.revision = store.media_ledger(binding).unwrap().revision + 1;
    refused(&mut store, binding, &future, NOW);
    let mut regressing = proposal.clone();
    regressing.feed = Some((FEED, 9));
    refused(&mut store, binding, &regressing, NOW);
    regressing.revision = store.media_ledger(binding).unwrap().revision;
    refused(&mut store, binding, &regressing, NOW);
    let mut future_cursor = proposal.clone();
    future_cursor.feed = Some((FEED, 11));
    refused(&mut store, binding, &future_cursor, NOW);
    let mut foreign = proposal.clone();
    foreign.feed = Some(([6; 16], 10));
    refused(&mut store, binding, &foreign, NOW);
    let (_directory, mut store, binding, absent) = fixture(Ledger::default());
    refused(&mut store, binding, &absent, NOW);
}

#[test]
fn identical_body_with_file_changes_never_coalesces() {
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let before = persisted(&store.connection);
    let changes = [(c::Payload::MediaName(metadata().media("new.bin")), [5; 16])];
    assert_eq!(
        store.commit_media_feed(binding, &proposal, &changes, (1, 2, 3), NOW, None),
        Err(Error::RecoveryRequired)
    );
    assert_eq!(persisted(&store.connection), before);
}

#[test]
fn identical_body_with_persisted_apply_intent_never_coalesces() {
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let entity = local_media(&mut store, binding, "new.bin");
    // Persist through the ordinary intent API. This test proves refusal/rollback,
    // never pretends a collection/media apply completed successfully.
    let mut intent = incoming::ApplyIntent {
        id: 0,
        binding,
        inbox_ids: vec![],
        keys: vec![incoming::ApplyKey {
            name: entity.name.clone(),
            kind: entity.kind,
            key: entity.key.clone(),
            role: incoming::ApplyKeyRole::Entry,
            before_digest: Some(entity.payload.row_digest().unwrap()),
            after_digest: Some(entity.payload.row_digest().unwrap()),
            before_payload: Some(entity.payload.clone()),
            after_payload: Some(entity.payload.clone()),
            own_flight: None,
        }],
        result_seq: None,
        originating_open: Some(1),
        planned_at_ms: NOW,
        native_file: runtime::NativeFileIdentity {
            device: 1,
            inode: 2,
            created_seconds: 3,
            created_nanos: 0,
        },
        generation: 3,
        media: Some(Apply {
            incoming: saved(binding, &entity),
            server: Some(metadata().media("new.bin")),
            old_name: "new.bin".into(),
            before: Some(metadata().media("new.bin")),
            moved: None,
            moved_before: None,
            move_evidence: None,
            install_absent: false,
            staged: None,
            before_referenced_new: BTreeMap::new(),
            local_wins: false,
            delete_wins: false,
            installed: false,
            abandoned: false,
        }),
    };
    intent.id = store.persist_apply_intent(&intent, None).unwrap();
    let proposals = [(Some(entity.clone()), entity)];
    let before = persisted(&store.connection);
    assert_eq!(
        store.commit_media_feed(
            binding,
            &proposal,
            &[],
            (1, 2, 3),
            NOW,
            Some((&intent, &proposals, true))
        ),
        Err(Error::RecoveryRequired)
    );
    assert_eq!(persisted(&store.connection), before);
    assert_eq!(store.apply_intents(binding).unwrap(), vec![intent]);
}

#[test]
fn reference_age_uses_current_transaction_references_and_actual_time_before_equality() {
    let mut body = empty();
    body.blobs.insert(
        "synthetic-blob".into(),
        BlobState {
            metadata: metadata(),
            unreferenced_since_ms: Some(NOW),
            deletion_due_ms: Some(NOW + cleanup::RETENTION_MS),
            ..Default::default()
        },
    );
    let (_directory, mut store, binding, proposal) = fixture(body);
    let before = persisted(&store.connection);
    assert_eq!(
        store.commit_media_feed(binding, &proposal, &[], (1, 2, 3), NOW, None),
        Ok(false)
    );
    assert_eq!(persisted(&store.connection), before);
    // A backwards clock resets the age and deletion authority despite pre-age equality.
    refused(&mut store, binding, &proposal, NOW - 1);
    store
        .insert_workflow_entity(
            binding,
            c::Payload::Note(c::Note {
                id: 11,
                mid: 12,
                nt_struct_gen: 1,
                flds: b"<img src=\"live.bin\">\x1fback".to_vec(),
                ..Default::default()
            }),
            [6; 16],
            (1, 2, 3),
        )
        .unwrap();
    local_media(&mut store, binding, "live.bin");
    refused(&mut store, binding, &proposal, NOW);
}

#[test]
fn corrupt_or_out_of_range_current_ledger_never_qualifies_as_redundant() {
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let mut invalid = proposal.clone();
    invalid.revision = u64::MAX;
    store
        .connection
        .execute(
            "UPDATE cloud_media_state SET content=?2 WHERE binding=?1",
            rusqlite::params![binding.0.as_slice(), crate::cloud::store::encode(&invalid).unwrap()],
        )
        .unwrap();
    refused(&mut store, binding, &proposal, NOW);
    store
        .connection
        .execute(
            "UPDATE cloud_media_state SET content='[]' WHERE binding=?1",
            [binding.0.as_slice()],
        )
        .unwrap();
    let before = persisted(&store.connection);
    assert_eq!(
        store.commit_media_feed(binding, &proposal, &[], (1, 2, 3), NOW, None),
        Err(Error::InvalidData)
    );
    assert_eq!(persisted(&store.connection), before);
}

#[test]
fn final_completeness_effects_qualify_only_when_adoption_and_removal_agree() {
    // Completeness is a feed input, not a persisted certification: both input
    // values can yield this same final body when adoption is already finished.
    let (_directory, mut store, binding, proposal) = fixture(empty());
    let before = persisted(&store.connection);
    assert_eq!(
        store.commit_media_feed(binding, &proposal, &[], (1, 2, 3), NOW, None),
        Ok(false)
    );
    assert_eq!(persisted(&store.connection), before);
    let mut incomplete_adoption = proposal.clone();
    incomplete_adoption.adoption = true;
    refused(&mut store, binding, &incomplete_adoption, NOW);
    let mut held = empty();
    held.names.insert(
        "removed.bin".into(),
        NameState {
            removal_held: true,
            ..Default::default()
        },
    );
    let (_directory, mut store, binding, mut complete_removal) = fixture(held);
    let flags = complete_removal.names.get_mut("removed.bin").unwrap();
    flags.removal_held = false;
    flags.tombstone_at = Some(NOW);
    refused(&mut store, binding, &complete_removal, NOW);
}

#[test]
fn redundant_commit_retains_minimum_floor_across_active_builds() {
    let (directory, mut store, binding, proposal) = fixture(empty());
    let other_scope = BuildScope {
        container: ContainerId::new("synthetic-identical-feed-other").unwrap(),
        environment: Environment::Development,
    };
    let OpenOutcome::Opened(mut other) = TrackingStore::open(directory.path(), other_scope).unwrap() else {
        panic!("existing synthetic store")
    };
    other.install_runtime().unwrap();
    let second = add_binding(&mut other, 1918);
    other
        .put_media(
            second,
            &Ledger {
                feed: Some((FEED, 4)),
                ..Default::default()
            },
        )
        .unwrap();
    let before = persisted(&store.connection);
    assert_eq!(
        store.commit_media_feed(binding, &proposal, &[], (1, 2, 3), NOW, None),
        Ok(false)
    );
    assert_eq!(persisted(&store.connection), before);
    assert_eq!(store.media_floor(FEED).unwrap(), Some(4));
}
