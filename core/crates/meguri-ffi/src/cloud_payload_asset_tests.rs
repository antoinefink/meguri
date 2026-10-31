//! Ordinary pa prerequisites over the published host and original FFI callbacks.
//! Source baseline: 8d7355e7; integration batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
use super::*;
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_sync::cloud::entity_codec as codec;
use meguri_testkit::cloud::{Asset, Record};

fn note_facts(fixture: &PublishedFixture, id: NoteId) -> w::EntityFacts {
    fixture
        .workflow
        .core
        .adapter_snapshot()
        .unwrap()
        .entities
        .into_iter()
        .find(|entity| matches!(&entity.payload, codec::Payload::Note(note) if note.id == id.0))
        .expect("actual note capture")
}

fn add_note(fixture: &PublishedFixture, back: String) -> (NoteId, w::EntityFacts) {
    let collection = fixture.workflow.host.collection().unwrap();
    let notetype = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        let Some(ConfigValue::Integer(id)) = reader.config_value("curModel").unwrap() else {
            panic!("stock current notetype")
        };
        NotetypeId(i64::try_from(id).unwrap())
    };
    let added = meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: notetype,
            fields: vec!["Question".into(), back],
            tags: vec![],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, added.seq.0)
        .unwrap();
    (added.value.id, note_facts(fixture, added.value.id))
}

fn save(
    fixture: &PublishedFixture,
    sdk: &CloudEngineRegistration,
    input: RecordInput,
    fields: Option<Vec<u8>>,
) -> Record {
    let reply = fixture
        .server
        .lock()
        .unwrap()
        .request(
            &fixture.scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: input,
                    if_unchanged: fields.map(SystemFields::from_bytes),
                }],
            },
        )
        .unwrap()
        .result
        .unwrap();
    let Reply::Modified(mut items) = reply else {
        panic!("actual Service modify")
    };
    assert_eq!(items.len(), 1);
    let MutationSuccess::Saved(record) = items.pop().unwrap().result.unwrap() else {
        panic!("actual Saved")
    };
    record
}

// Model the SDK's disposable download file, using bytes actually held by Service.
fn callback_record(record: &Record, zone: &str, directory: &std::path::Path) -> CloudRecord {
    let value = publication_control_record(record).unwrap();
    let asset_path = record.fields.get("pa").map(|field| {
        let Field::Asset(asset) = field else {
            panic!("ordinary payload asset")
        };
        let path = directory.join("sdk-pa");
        std::fs::write(&path, asset.bytes()).unwrap();
        path.to_string_lossy().into_owned()
    });
    CloudRecord {
        name: record.name.clone(),
        envelope: value.encrypted.unwrap(),
        system_fields: Some(value.system_fields.0),
        record_type: Some("MGEntity".into()),
        zone: Some(zone.into()),
        asset_path,
    }
}

fn remote_note(fixture: &PublishedFixture, large: bool) -> (RecordInput, Vec<u8>, NoteId) {
    let (_, seed) = add_note(fixture, "Seed".into());
    let codec::Payload::Note(mut note) = seed.payload else {
        panic!("note")
    };
    note.id += 1_000_000;
    note.guid = b"synthetic-remote-pa".to_vec();
    note.flds = format!(
        "Question\u{1f}{}",
        if large { "r".repeat(800_000) } else { "Remote".into() }
    )
    .into_bytes();
    let id = NoteId(note.id);
    let payload = codec::Payload::Note(note);
    let bytes = payload.encode().unwrap();
    let key = fixture
        .workflow
        .core
        .status()
        .unwrap()
        .binding
        .unwrap()
        .facts
        .record_key
        .0;
    let entity_key = c::row_payload::payload_key(&payload).unwrap();
    let name = codec::entity_name(&key, codec::Kind::Note, &entity_key).unwrap();
    let entity = codec::Entity::build(
        codec::EntityFields {
            kind: codec::Kind::Note,
            key: entity_key,
            origin: [83; 16],
            erev: 1,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer: [84; 16],
            base_sha256: None,
        },
        &bytes,
    )
    .unwrap();
    let mut input = RecordInput::encrypted(name, "MGEntity", entity.encode().unwrap());
    if entity.payload.is_none() {
        input
            .fields
            .insert("pa".into(), Field::Asset(Asset::synthetic(bytes.clone())));
    }
    (input, bytes, id)
}

fn read(fixture: &PublishedFixture, sdk: &CloudEngineRegistration, name: &str) -> Record {
    let Reply::Record(record) = fixture
        .server
        .lock()
        .unwrap()
        .request(
            &fixture.scope,
            Caller::default(),
            Operation::Read {
                zone: sdk.collection_zone.clone(),
                name: name.into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!("actual all-fields Service read")
    };
    record
}

#[test]
fn ordinary_pa_large_note_real_save_ack_preserves_later_local_revision() {
    let (fixture, sdk) = published();
    let (id, original) = add_note(&fixture, "a".repeat(800_000));
    let bytes = original.payload.encode().unwrap();
    assert!(bytes.len() > 786_432 && bytes.len() <= 104_857_600);
    let pending = fixture
        .workflow
        .tracking_sendables()
        .unwrap()
        .into_iter()
        .filter(|(name, _)| name == &original.name)
        .collect();
    let batch = fixture
        .workflow
        .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), pending)
        .expect("ordinary large note must build a pa-bearing batch")
        .expect("selected note");
    assert_eq!(batch.saves.len(), 1);
    let handed = &batch.saves[0];
    assert_eq!(handed.name, original.name);
    let path = handed.asset_path.as_ref().expect("owned ordinary upload asset");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    let codec::DecodeOutcome::Supported(envelope) = codec::Entity::decode(&handed.envelope).unwrap() else {
        panic!("entity")
    };
    assert!(envelope.payload.is_none());
    assert_eq!(envelope.payload_sha256, codec::sha256(&bytes));
    let mut input = RecordInput::encrypted(&handed.name, "MGEntity", handed.envelope.clone());
    input.fields.insert(
        "pa".into(),
        Field::Asset(Asset::synthetic(std::fs::read(path).unwrap())),
    );
    let saved = save(&fixture, &sdk, input, handed.system_fields.clone());
    let Some(Field::Asset(landed)) = saved.fields.get("pa") else {
        panic!("Service must hold pa")
    };
    assert_eq!(landed.bytes(), bytes);

    let collection = fixture.workflow.host.collection().unwrap();
    let edited = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id,
            fields: vec!["Question".into(), "b".repeat(800_001)],
            tags: vec![],
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, edited.seq.0)
        .unwrap();
    let later = note_facts(&fixture, id);
    assert!(later.local_rev > original.local_rev);
    let download = tempfile::tempdir().unwrap();
    let callback = callback_record(&saved, &sdk.collection_zone, download.path());
    fixture
        .workflow
        .sent(sdk.instance_id.clone(), batch.id, vec![callback], vec![], 1)
        .expect("actual pa-bearing Saved must ACK the handed revision");
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    let acked = note_facts(&fixture, id);
    assert_eq!(
        (acked.local_rev, acked.acked_rev, acked.server_erev),
        (later.local_rev, original.local_rev, 1)
    );
    assert_eq!(acked.origin, original.origin);
    assert!(acked.payload == later.payload);
    assert!(acked.in_flight.is_empty());
    assert!(acked.acked_payload == Some(original.payload.clone()) && acked.base_payload == Some(original.payload));
    assert_eq!(acked.acked_payload_digest, codec::sha256(&bytes));
    assert_eq!(acked.acked_system_fields, saved.system_fields.bytes());
    assert!(fixture.workflow.tracking_sendables().unwrap().contains_key(&acked.name));
    assert!(
        fixture
            .workflow
            .take_pending_updates(sdk.instance_id)
            .unwrap()
            .iter()
            .any(|update| update.adds.contains_key(&acked.name))
    );
}

#[test]
fn ordinary_pa_remote_note_owns_download_before_apply_and_source_deletion() {
    let (fixture, sdk) = published();
    let (input, bytes, id) = remote_note(&fixture, true);
    let name = input.name.clone();
    save(&fixture, &sdk, input, None);
    let server = read(&fixture, &sdk, &name);
    let download = tempfile::tempdir().unwrap();
    let callback = callback_record(&server, &sdk.collection_zone, download.path());
    let source = callback.asset_path.clone().unwrap();
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    let core = Arc::downgrade(&fixture.workflow.core);
    let expected = bytes.clone();
    let observed = Arc::new(AtomicUsize::new(0));
    let count = observed.clone();
    fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::FetchAfterInbox {
            let entries = core.upgrade().unwrap().inbox_entries(binding).unwrap();
            let entry = entries.iter().find(|entry| entry.entry.record_name == name).unwrap();
            let c::InboxValue::Record {
                staged_asset: Some(staged),
                ..
            } = &entry.entry.value
            else {
                panic!("durable owned pa")
            };
            assert_ne!(staged, &source);
            std::fs::remove_file(&source).unwrap();
            assert_eq!(std::fs::read(staged).unwrap(), expected);
            count.fetch_add(1, Ordering::SeqCst);
        }
    });
    fixture
        .workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            vec![callback.clone()],
            vec![],
        )
        .expect("ordinary pa intake must own the bytes durably");
    assert_eq!(observed.load(Ordering::SeqCst), 1);
    let collection = fixture.workflow.host.collection().unwrap();
    let note = collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap();
    let codec::Payload::Note(payload) = codec::Payload::decode(codec::Kind::Note, &bytes).unwrap() else {
        panic!("note")
    };
    assert_eq!(note.fields.as_bytes(), payload.flds);
    let facts = note_facts(&fixture, id);
    assert_eq!(facts.acked_payload_digest, codec::sha256(&bytes));
    assert_eq!(facts.acked_system_fields, server.system_fields.bytes());
    assert!(
        !fixture
            .workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&callback.name)
    );
    assert!(!fixture.workflow.core.incoming_wal_receipts().is_empty());
    assert!(
        fixture
            .workflow
            .core
            .incoming_runs()
            .iter()
            .any(|run| run.changed_keys.contains(&callback.name))
    );
    fixture
        .workflow
        .state_update(sdk.instance_id, b"after-owned-pa".to_vec())
        .unwrap();
    assert_eq!(
        fixture.workflow.durable_snapshot().unwrap().serialization,
        Some(b"after-owned-pa".to_vec())
    );
}

#[test]
fn ordinary_pa_file_failures_are_retryable_but_inline_corruption_is_refused() {
    for failure in ["absent", "missing", "unreadable", "size", "digest", "inline"] {
        let (fixture, sdk) = published();
        let (mut input, _, id) = remote_note(&fixture, failure != "inline");
        if failure == "inline" {
            let Some(Field::EncryptedBytes(bytes)) = input.fields.get_mut("e") else {
                panic!("envelope")
            };
            let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(bytes).unwrap() else {
                panic!("entity")
            };
            entity.payload_sha256[0] ^= 1;
            *bytes = entity.encode().unwrap();
        }
        let server = save(&fixture, &sdk, input, None);
        let download = tempfile::tempdir().unwrap();
        let mut callback = callback_record(&server, &sdk.collection_zone, download.path());
        if let Some(source) = callback.asset_path.as_ref() {
            match failure {
                "absent" => {
                    std::fs::remove_file(source).unwrap();
                    callback.asset_path = None;
                }
                "missing" => std::fs::remove_file(source).unwrap(),
                "unreadable" => {
                    std::fs::remove_file(source).unwrap();
                    std::fs::create_dir(source).unwrap();
                }
                "size" => {
                    let mut bytes = std::fs::read(source).unwrap();
                    bytes.pop();
                    std::fs::write(source, bytes).unwrap();
                }
                "digest" => {
                    let mut bytes = std::fs::read(source).unwrap();
                    let last = bytes.last_mut().unwrap();
                    *last ^= 1;
                    std::fs::write(source, bytes).unwrap();
                }
                _ => unreachable!(),
            }
        }
        fixture
            .workflow
            .fetched(
                sdk.instance_id.clone(),
                sdk.collection_zone,
                vec![callback.clone()],
                vec![],
            )
            .expect("file failures must become durable asset entries, not callback rejection");
        let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
        let entries = fixture.workflow.core.inbox_entries(binding).unwrap();
        let refused = fixture.workflow.core.refused_entries(binding).unwrap();
        if failure == "inline" {
            assert!(!entries.iter().any(|entry| entry.entry.record_name == callback.name));
            assert!(refused.iter().any(|entry| entry.record_name == callback.name));
        } else {
            let entry = entries
                .iter()
                .find(|entry| entry.entry.record_name == callback.name)
                .unwrap();
            assert!(matches!(entry.entry.reason, c::InboxReason::Asset));
            assert_eq!(entry.entry.retry_count, 0);
            assert!(!refused.iter().any(|entry| entry.record_name == callback.name));
        }
        assert!(
            fixture
                .workflow
                .host
                .collection()
                .unwrap()
                .reader(ReaderRole::Query)
                .unwrap()
                .note(id)
                .unwrap()
                .is_none()
        );
        fixture
            .workflow
            .state_update(sdk.instance_id, b"after-classified-pa".to_vec())
            .unwrap();
        assert!(!fixture.workflow.durable_snapshot().unwrap().tracking_stopped);
    }
}

#[test]
fn ordinary_pa_failed_durable_intake_cannot_advance_cursor() {
    let (fixture, sdk) = published();
    let (input, _, id) = remote_note(&fixture, true);
    let server = save(&fixture, &sdk, input, None);
    let download = tempfile::tempdir().unwrap();
    let callback = callback_record(&server, &sdk.collection_zone, download.path());
    let before = fixture.workflow.durable_snapshot().unwrap();
    fixture.workflow.core.arm_adapter_faults_for_testing(true, false);
    assert!(
        fixture
            .workflow
            .fetched(
                sdk.instance_id.clone(),
                sdk.collection_zone,
                vec![callback.clone()],
                vec![]
            )
            .is_err()
    );
    assert!(
        fixture
            .workflow
            .state_update(sdk.instance_id, b"must-not-pass-pa".to_vec())
            .is_err()
    );
    let after = fixture.workflow.durable_snapshot().unwrap();
    assert_eq!(after.serialization, before.serialization);
    assert!(!after.inbox_names.contains(&callback.name));
    assert!(after.tracking_stopped);
    assert!(fixture.workflow.core.incoming_runs().is_empty());
    assert!(
        fixture
            .workflow
            .host
            .collection()
            .unwrap()
            .reader(ReaderRole::Query)
            .unwrap()
            .note(id)
            .unwrap()
            .is_none()
    );
}

// The original native SDK operation is retained here, including its batch and
// callback; no SendCall is created for an automatic SDK operation.
fn held_automatic_pa_after_resume(disposition: u8) {
    let (fixture, sdk) = published();
    fixture.workflow.core.set_automatic(&sdk.instance_id, true).unwrap();
    let (id, original) = add_note(&fixture, "a".repeat(800_000));
    let pending = fixture
        .workflow
        .tracking_sendables()
        .unwrap()
        .into_iter()
        .filter(|(name, _)| name == &original.name)
        .collect();
    let batch = fixture
        .workflow
        .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), pending)
        .unwrap()
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    let handed = &batch.saves[0];
    let path = handed.asset_path.as_ref().unwrap();
    let bytes = original.payload.encode().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    // Hold the actual Service Saved callback across suspension. This is the
    // original automatic operation, with no explicit network-call counter.
    let mut input = RecordInput::encrypted(&handed.name, "MGEntity", handed.envelope.clone());
    input.fields.insert(
        "pa".into(),
        Field::Asset(Asset::synthetic(std::fs::read(path).unwrap())),
    );
    let saved = save(&fixture, &sdk, input, handed.system_fields.clone());
    let download = tempfile::tempdir().unwrap();
    let callback = callback_record(&saved, &sdk.collection_zone, download.path());
    let calls = fixture.workflow.core.send_calls().len();
    fixture.workflow.host.engine().flush_for_background().unwrap();
    assert_eq!(
        fixture.workflow.host.engine().suspend(Duration::from_secs(5)).unwrap(),
        meguri_engine::SuspendOutcome::Suspended
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    fixture.workflow.host.engine().resume().unwrap();
    assert!(fixture.workflow.validate_sdk(sdk.instance_id.clone()).is_err());
    fixture.workflow.core.refresh_identity().unwrap();
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);

    let collection = fixture.workflow.host.collection().unwrap();
    let edited = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id,
            fields: vec!["Question".into(), "b".repeat(800_001)],
            tags: vec![],
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, edited.seq.0)
        .unwrap();
    let later = note_facts(&fixture, id);
    assert!(later.local_rev > original.local_rev);
    assert_eq!(fixture.workflow.core.send_calls().len(), calls);

    if disposition == 0 {
        fixture
            .workflow
            .sent(sdk.instance_id.clone(), batch.id, vec![callback], vec![], 1)
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        let acked = note_facts(&fixture, id);
        assert_eq!(acked.acked_rev, original.local_rev);
        assert_eq!(acked.local_rev, later.local_rev);
        assert!(acked.payload == later.payload && acked.mark.is_some());
        assert!(acked.acked_payload == Some(original.payload));
        fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
        assert!(!std::path::Path::new(path).exists());
    } else {
        if disposition == 2 {
            fixture
                .workflow
                .account_changed(
                    None,
                    CloudAccountStatus::Available,
                    Some(b"synthetic-different-owner".to_vec()),
                )
                .unwrap();
            assert!(
                fixture
                    .workflow
                    .sent(sdk.instance_id.clone(), batch.id, vec![callback.clone()], vec![], 1)
                    .is_err()
            );
        }
        fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(fixture.workflow.register_sdk(true).is_err());
        assert!(
            fixture
                .workflow
                .sent(sdk.instance_id.clone(), batch.id, vec![callback.clone()], vec![], 1)
                .is_err()
        );
        assert_eq!(note_facts(&fixture, id).acked_rev, original.acked_rev);
        // Native SDK release confirms cancellation and completion of outstanding work.
        // Retiring that SDK must preserve the upload path while it still owns it.
        fixture.workflow.release_sdk(sdk.instance_id.clone()).unwrap();
        assert!(!std::path::Path::new(path).exists());
        if disposition == 1 {
            fixture
                .workflow
                .core
                .reconcile_binding(
                    &collection,
                    &fixture.workflow.host.media().unwrap(),
                    Duration::from_secs(5),
                )
                .unwrap();
            let successor = fixture.workflow.register_sdk(true).unwrap();
            assert_ne!(successor.instance_id, sdk.instance_id);
            assert!(
                fixture
                    .workflow
                    .sent(successor.instance_id, batch.id, vec![callback], vec![], 1)
                    .is_err()
            );
            assert_eq!(note_facts(&fixture, id).acked_rev, original.acked_rev);
        }
    }
}

#[test]
fn original_automatic_pa_sdk_survives_real_suspend_resume_and_acks_only_handed_revision() {
    held_automatic_pa_after_resume(0);
}

#[test]
fn retired_sdk_keeps_upload_until_release_and_rejects_replacement_callback() {
    held_automatic_pa_after_resume(1);
}

#[test]
fn original_automatic_pa_sdk_account_change_refuses_ack_and_retains_file_until_cancel_drain() {
    held_automatic_pa_after_resume(2);
}
