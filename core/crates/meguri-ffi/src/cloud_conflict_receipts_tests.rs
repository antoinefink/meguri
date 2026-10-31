//! Product conflict receipts over actual Host, Service and original FFI batches.
use super::*;
use crate::cloud_asset_reads::CloudAssetReadKind;

fn metadata(record: &Record, sdk: &CloudEngineRegistration) -> CloudRecord {
    let value = publication_control_record(record).unwrap();
    CloudRecord {
        name: record.name.clone(),
        envelope: value.encrypted.unwrap(),
        system_fields: Some(value.system_fields.0),
        record_type: Some("MGEntity".into()),
        zone: Some(sdk.collection_zone.clone()),
        // Error records never own downloaded assets, even when Service has pa.
        asset_path: None,
    }
}

fn input(record: &CloudRecord) -> RecordInput {
    let mut input = RecordInput::encrypted(&record.name, "MGEntity", record.envelope.clone());
    if let Some(path) = &record.asset_path {
        input.fields.insert(
            "pa".into(),
            Field::Asset(Asset::synthetic(std::fs::read(path).unwrap())),
        );
    }
    input
}

fn build(fixture: &PublishedFixture, sdk: &CloudEngineRegistration, names: &[String]) -> CloudEngineBatch {
    fixture
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            names
                .iter()
                .map(|name| (name.clone(), CloudPendingChange::Save))
                .collect(),
        )
        .unwrap()
        .unwrap()
}

// Every positive receipt comes from the actual conditional Modify results.
fn results(
    fixture: &PublishedFixture,
    sdk: &CloudEngineRegistration,
    batch: &CloudEngineBatch,
) -> (Vec<CloudRecord>, Vec<CloudRecordFailure>) {
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
                changes: batch
                    .saves
                    .iter()
                    .map(|record| Mutation::Save {
                        record: input(record),
                        if_unchanged: record.system_fields.clone().map(SystemFields::from_bytes),
                    })
                    .collect(),
            },
        )
        .unwrap()
        .result
        .unwrap();
    let Reply::Modified(items) = reply else {
        panic!("actual Service Modify")
    };
    let mut saved = Vec::new();
    let mut failed = Vec::new();
    for item in items {
        match item.result {
            Ok(MutationSuccess::Saved(record)) => saved.push(metadata(&record, sdk)),
            Err(error) => {
                assert_eq!(error.code, ErrorCode::ServerRecordChanged);
                let server = error.server_record.unwrap();
                assert_eq!(server.name, item.name);
                failed.push(CloudRecordFailure {
                    name: item.name,
                    code: CloudEngineErrorCode::ServerRecordChanged,
                    server_record: Some(metadata(&server, sdk)),
                });
            }
            _ => panic!("save result"),
        }
    }
    (saved, failed)
}

fn deliver(
    fixture: &PublishedFixture,
    sdk: &CloudEngineRegistration,
    batch: &CloudEngineBatch,
    saved: Vec<CloudRecord>,
    failed: Vec<CloudRecordFailure>,
) -> u64 {
    fixture
        .workflow
        .sent(
            sdk.instance_id.clone(),
            batch.id,
            saved,
            failed,
            batch.saves.len() as u32,
        )
        .unwrap()
}

fn edit(fixture: &PublishedFixture, id: NoteId, front: String, back: String) {
    let collection = fixture.workflow.host.collection().unwrap();
    let edited = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id,
            fields: vec![front, back],
            tags: vec![],
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, edited.seq.0)
        .unwrap();
}

fn ack_base(fixture: &PublishedFixture, sdk: &CloudEngineRegistration, name: &str) -> Record {
    let batch = build(fixture, sdk, &[name.into()]);
    let (saved, failed) = results(fixture, sdk, &batch);
    assert!(failed.is_empty());
    assert_eq!(deliver(fixture, sdk, &batch, saved, failed), 1);
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    read(fixture, sdk, name)
}

// One scenario body exercises both inline inbox/apply and pa first-read grants.
fn concurrent_conflict(large: bool) {
    let (fixture, sdk) = published();
    let (id, original) = add_note(&fixture, "Base back".into());
    let baseline = ack_base(&fixture, &sdk, &original.name);
    let acked = note_facts(&fixture, id);
    let local_front = if large {
        "local".repeat(160_000)
    } else {
        "Local front".into()
    };
    let remote_back = if large {
        "remote".repeat(160_000)
    } else {
        "Remote back".into()
    };
    edit(&fixture, id, local_front.clone(), "Base back".into());
    let mut names = vec![original.name.clone()];
    if !large {
        let (_, sibling) = add_note(&fixture, "Normal Saved sibling".into());
        names.push(sibling.name);
    }
    let batch = build(&fixture, &sdk, &names);
    let local = note_facts(&fixture, id);
    let codec::DecodeOutcome::Supported(base) = codec::Entity::decode(&metadata(&baseline, &sdk).envelope).unwrap()
    else {
        panic!("baseline entity")
    };
    let codec::Payload::Note(mut remote) = original.payload.clone() else {
        panic!("note")
    };
    remote.flds = format!("Question\u{1f}{remote_back}").into_bytes();
    let bytes = codec::Payload::Note(remote).encode().unwrap();
    let mut fields = base.fields;
    fields.writer = [84; 16];
    fields.erev += 1;
    fields.base_sha256 = Some(acked.acked_payload_digest);
    let remote = codec::Entity::build(fields, &bytes).unwrap();
    assert_eq!(remote.payload.is_none(), large);
    let mut concurrent = RecordInput::encrypted(&original.name, "MGEntity", remote.encode().unwrap());
    if large {
        concurrent
            .fields
            .insert("pa".into(), Field::Asset(Asset::synthetic(bytes.clone())));
    }
    let concurrent = save(
        &fixture,
        &sdk,
        concurrent,
        Some(baseline.system_fields.bytes().to_vec()),
    );
    let (saved, failed) = results(&fixture, &sdk, &batch);
    assert_eq!(saved.len(), usize::from(!large));
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0].server_record.as_ref().unwrap().system_fields.as_deref(),
        Some(concurrent.system_fields.bytes())
    );
    let runs = fixture.workflow.core.incoming_runs().len();
    assert_eq!(deliver(&fixture, &sdk, &batch, saved, failed), u64::from(!large));
    let held = note_facts(&fixture, id);
    assert_eq!(held.acked_rev, acked.acked_rev);
    assert_eq!(held.server_erev, acked.server_erev);
    assert_eq!(held.acked_system_fields, acked.acked_system_fields);
    assert_eq!(held.acked_payload_digest, acked.acked_payload_digest);
    assert!(held.base_payload == local.base_payload && held.payload == local.payload);
    assert_eq!(fixture.workflow.core.incoming_runs().len(), runs);
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    let entries = fixture.workflow.core.inbox_entries(binding).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.entry.record_name == original.name)
        .unwrap();
    assert!(matches!(entry.entry.source, c::InboxSource::SaveConflict));
    assert!(matches!(entry.entry.reason, c::InboxReason::Asset) == large);
    assert_eq!(entry.entry.retry_count, 0);
    assert!(
        !fixture
            .workflow
            .tracking_sendables()
            .unwrap()
            .contains_key(&original.name)
    );
    assert!(
        fixture
            .workflow
            .begin_first_asset_read_pass(sdk.instance_id.clone())
            .is_err()
    );
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    if large {
        let pass = fixture
            .workflow
            .begin_first_asset_read_pass(sdk.instance_id.clone())
            .unwrap();
        let work = pass.clone().next().unwrap().unwrap();
        assert_eq!(work.name, original.name);
        assert_eq!(work.kind, CloudAssetReadKind::SaveConflictFirst);
        struct Queue;
        impl crate::cloud_asset_reads::CloudAssetReadQueue for Queue {
            fn enqueue(&self) -> bool {
                true
            }
        }
        assert!(pass.submit(work.handle.clone(), Arc::new(Queue)).unwrap());
        let entries = fixture.workflow.core.inbox_entries(binding).unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.entry.record_name == original.name)
                .unwrap()
                .entry
                .retry_count,
            0
        );
        let download = tempfile::tempdir().unwrap();
        let callback = callback_record(
            &read(&fixture, &sdk, &original.name),
            &sdk.collection_zone,
            download.path(),
        );
        assert!(pass.complete(work.handle, Some(callback)).unwrap().intake_accepted);
        pass.finish().unwrap();
        assert!(
            fixture
                .workflow
                .begin_first_asset_read_pass(sdk.instance_id.clone())
                .unwrap()
                .next()
                .unwrap()
                .is_none()
        );
    } else {
        // The existing host apply path owns the write/WAL/ACK boundary.
        fixture
            .workflow
            .host
            .apply_native_cloud_incoming(&sdk.instance_id, Duration::from_secs(1))
            .unwrap();
    }
    let merged = note_facts(&fixture, id);
    assert_eq!(merged.acked_system_fields, concurrent.system_fields.bytes());
    assert_eq!(merged.acked_payload_digest, codec::sha256(&bytes));
    assert_eq!(merged.server_erev, remote.fields.erev);
    let note = fixture
        .workflow
        .host
        .collection()
        .unwrap()
        .reader(ReaderRole::Query)
        .unwrap()
        .note(id)
        .unwrap()
        .unwrap();
    assert_eq!(note.fields, format!("{local_front}\u{1f}{remote_back}"));
    assert!(merged.mark.is_some());
    assert!(
        !fixture
            .workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&original.name)
    );
    assert!(!fixture.workflow.core.incoming_wal_receipts().is_empty());
}

#[test]
fn ffi_inline_conflict_keeps_ack_until_existing_apply_path() {
    concurrent_conflict(false);
}

#[test]
fn ffi_pa_conflict_grants_first_read_only_after_original_handler_return() {
    concurrent_conflict(true);
}

#[test]
fn ffi_remote_pa_conflict_preflight_and_intake_materialize_no_retained_bodies() {
    let (fixture, sdk) = published();
    let (id, original) = add_note(&fixture, "Base".into());
    let baseline = ack_base(&fixture, &sdk, &original.name);
    let codec::DecodeOutcome::Supported(base) = codec::Entity::decode(&metadata(&baseline, &sdk).envelope).unwrap()
    else {
        panic!("baseline entity")
    };
    // Real host edits and original backend handoffs retain eight distinct pa
    // flights. Each batch/body is dropped before the next revision. Full-size
    // 8 x 40 MiB peak coverage stays in cloud_payload_memory unchanged.
    for revision in 0..8 {
        edit(
            &fixture,
            id,
            format!("{revision}{}", "x".repeat(1024 * 1024)),
            "Local".into(),
        );
        let call = fixture.workflow.begin_send(sdk.instance_id.clone()).unwrap().unwrap();
        let handed = build(&fixture, &sdk, &[original.name.clone()]);
        fixture
            .workflow
            .core
            .send_call_request(&sdk.instance_id, call, handed.id)
            .unwrap();
        fixture
            .workflow
            .finish_send(
                sdk.instance_id.clone(),
                call,
                fixture.workflow.tracking_sendables().unwrap(),
            )
            .unwrap();
    }
    assert_eq!(
        fixture
            .workflow
            .core
            .payload_proof_for_testing(&original.name)
            .unwrap()
            .unwrap()
            .flights,
        8
    );
    edit(&fixture, id, "Current front".into(), "Current back".into());
    let retry = build(&fixture, &sdk, &[original.name.clone()]);
    let codec::Payload::Note(mut remote) = original.payload else {
        panic!("note")
    };
    remote.flds = format!("Question\u{1f}{}", "remote".repeat(180_000)).into_bytes();
    let bytes = codec::Payload::Note(remote).encode().unwrap();
    let mut fields = base.fields;
    fields.writer = [84; 16];
    fields.erev += 1;
    fields.base_sha256 = Some(base.payload_sha256);
    let remote = codec::Entity::build(fields, &bytes).unwrap();
    assert!(remote.payload.is_none());
    let mut record = RecordInput::encrypted(&original.name, "MGEntity", remote.encode().unwrap());
    record.fields.insert("pa".into(), Field::Asset(Asset::synthetic(bytes)));
    save(&fixture, &sdk, record, Some(baseline.system_fields.bytes().to_vec()));
    let (saved, failed) = results(&fixture, &sdk, &retry);
    assert_eq!((saved.len(), failed.len()), (0, 1));
    // Both observations and calls execute synchronously on this test thread:
    // the counter is thread-local. No diagnostic full-image read intervenes.
    let before = c::testing::entity_body_reads();
    fixture
        .workflow
        .core
        .preflight_sent_conflicts(
            &sdk.instance_id,
            &fixture.workflow.host.collection().unwrap(),
            retry.id,
            &[original.name.clone()],
        )
        .unwrap();
    assert_eq!(c::testing::entity_body_reads(), before);
    assert_eq!(deliver(&fixture, &sdk, &retry, saved, failed), 0);
    assert_eq!(c::testing::entity_body_reads(), before);
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
}

#[test]
fn ffi_historical_own_live_conflict_acks_lost_revision_without_writing_newer_local_edit() {
    let (fixture, sdk) = published();
    let (id, original) = add_note(&fixture, "Base".into());
    ack_base(&fixture, &sdk, &original.name);
    edit(&fixture, id, "Lost save".into(), "Back".into());
    let lost = note_facts(&fixture, id);
    let call = fixture.workflow.begin_send(sdk.instance_id.clone()).unwrap().unwrap();
    let original_batch = build(&fixture, &sdk, &[original.name.clone()]);
    fixture
        .workflow
        .core
        .send_call_request(&sdk.instance_id, call, original_batch.id)
        .unwrap();
    let (landed, failed) = results(&fixture, &sdk, &original_batch);
    assert!(failed.is_empty());
    assert_eq!(landed.len(), 1);
    // Service committed Saved; its SDK reply is deliberately withheld. End
    // the original send, preserving the durable historical flight.
    fixture
        .workflow
        .finish_send(
            sdk.instance_id.clone(),
            call,
            fixture.workflow.tracking_sendables().unwrap(),
        )
        .unwrap();
    edit(&fixture, id, "Newer front".into(), "Newer back".into());
    let retry = build(&fixture, &sdk, &[original.name.clone()]);
    let current = note_facts(&fixture, id);
    assert!(current.local_rev > lost.local_rev);
    let (saved, failed) = results(&fixture, &sdk, &retry);
    assert!(saved.is_empty());
    assert_eq!(failed[0].server_record.as_ref().unwrap().envelope, landed[0].envelope);
    let writes = fixture.workflow.core.incoming_wal_receipts().len();
    let runs = fixture.workflow.core.incoming_runs().len();
    assert_eq!(deliver(&fixture, &sdk, &retry, saved, failed), 1);
    let ack = note_facts(&fixture, id);
    assert_eq!(ack.local_rev, current.local_rev);
    assert_eq!(ack.acked_rev, lost.local_rev);
    assert_eq!(ack.acked_system_fields, landed[0].system_fields.clone().unwrap());
    assert!(ack.payload == current.payload && ack.acked_payload == Some(lost.payload));
    assert!(ack.mark.is_some());
    let note = fixture
        .workflow
        .host
        .collection()
        .unwrap()
        .reader(ReaderRole::Query)
        .unwrap()
        .note(id)
        .unwrap()
        .unwrap();
    assert_eq!(note.fields, "Newer front\u{1f}Newer back");
    assert_eq!(fixture.workflow.core.incoming_wal_receipts().len(), writes);
    assert_eq!(fixture.workflow.core.incoming_runs().len(), runs);
    assert!(
        !fixture
            .workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&original.name)
    );
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    assert!(
        fixture
            .workflow
            .tracking_sendables()
            .unwrap()
            .contains_key(&original.name)
    );
}

#[test]
fn ffi_historical_own_note_and_card_tombstones_require_the_actual_handed_envelope() {
    let (fixture, sdk) = published();
    let (id, note) = add_note(&fixture, "To delete".into());
    let card = fixture
        .workflow
        .core
        .adapter_snapshot()
        .unwrap()
        .entities
        .into_iter()
        .find(|entity| matches!(&entity.payload, codec::Payload::Card(card) if card.nid == id.0))
        .unwrap();
    let names = vec![note.name.clone(), card.name.clone()];
    // A live Card waits for its Note's actual ACK, even when both are pending.
    // Each baseline save returns its original handler before the next build.
    ack_base(&fixture, &sdk, &note.name);
    ack_base(&fixture, &sdk, &card.name);
    let collection = fixture.workflow.host.collection().unwrap();
    let deleted = meguri_engine::notes::delete(&collection, vec![id], meguri_engine::OpSource::UserUndoable)
        .unwrap()
        .unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, deleted.seq.0)
        .unwrap();
    assert!(
        collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(id)
            .unwrap()
            .is_none()
    );
    let codec::Payload::Card(card_payload) = &card.payload else {
        panic!("card")
    };
    assert!(
        collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(meguri_model::ids::CardId(card_payload.id))
            .unwrap()
            .is_none()
    );
    let call = fixture.workflow.begin_send(sdk.instance_id.clone()).unwrap().unwrap();
    let lost = build(&fixture, &sdk, &names);
    fixture
        .workflow
        .core
        .send_call_request(&sdk.instance_id, call, lost.id)
        .unwrap();
    let (landed, failed) = results(&fixture, &sdk, &lost);
    assert_eq!(landed.len(), 2);
    assert!(failed.is_empty());
    fixture
        .workflow
        .finish_send(
            sdk.instance_id.clone(),
            call,
            fixture.workflow.tracking_sendables().unwrap(),
        )
        .unwrap();
    let retry = build(&fixture, &sdk, &names);
    let (saved, failed) = results(&fixture, &sdk, &retry);
    assert!(saved.is_empty());
    assert_eq!(failed.len(), 2);
    let before = names
        .iter()
        .map(|name| fixture.workflow.core.entity_facts(name).unwrap().unwrap())
        .collect::<Vec<_>>();
    for receipt in &failed {
        let actual = receipt.server_record.as_ref().unwrap();
        let original = landed.iter().find(|record| record.name == actual.name).unwrap();
        assert_eq!(actual.envelope, original.envelope);
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&actual.envelope).unwrap() else {
            panic!("actual tombstone envelope")
        };
        assert!(entity.fields.tombstone);
        assert!(matches!(entity.fields.kind, codec::Kind::Note | codec::Kind::Card));
        let facts = before.iter().find(|facts| facts.name == actual.name).unwrap();
        assert_eq!(
            entity.fields.writer,
            fixture.workflow.core.status().unwrap().binding.unwrap().id.0
        );
        assert!(facts.in_flight.iter().any(|flight| flight.tombstone
            && flight.erev == entity.fields.erev
            && flight.local_rev == facts.local_rev
            && flight.payload_digest == codec::sha256(&[])
            && flight.tombstone_envelope_digest == Some(codec::sha256(&actual.envelope))));
    }
    let writes = fixture.workflow.core.incoming_wal_receipts().len();
    let runs = fixture.workflow.core.incoming_runs().len();
    assert_eq!(deliver(&fixture, &sdk, &retry, saved, failed), 2);
    for old in before {
        let ack = fixture.workflow.core.entity_facts(&old.name).unwrap().unwrap();
        let server = landed.iter().find(|record| record.name == old.name).unwrap();
        assert_eq!(ack.local_rev, old.local_rev);
        assert_eq!(ack.acked_rev, old.local_rev);
        assert_eq!(ack.acked_envelope.0, server.envelope);
        assert_eq!(ack.acked_system_fields, server.system_fields.clone().unwrap());
        assert!(ack.payload == old.payload && ack.tombstone_at == old.tombstone_at);
        assert!(ack.mark.is_none() && ack.in_flight.is_empty());
        assert!(
            !fixture
                .workflow
                .durable_snapshot()
                .unwrap()
                .inbox_names
                .contains(&old.name)
        );
    }
    assert!(
        collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(id)
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.workflow.core.incoming_wal_receipts().len(), writes);
    assert_eq!(fixture.workflow.core.incoming_runs().len(), runs);
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
}

#[test]
fn malformed_conflict_tail_cannot_ack_real_saved_sibling_or_enter_handler() {
    for problem in [
        "missing",
        "code",
        "type",
        "zone",
        "name",
        "unknown",
        "unhanded-existing",
        "system",
        "empty-system",
        "empty-envelope",
        "asset",
        "absent-type",
        "absent-zone",
    ] {
        let (fixture, sdk) = published();
        let (first_id, first) = add_note(&fixture, "First".into());
        let (second_id, second) = add_note(&fixture, "Second".into());
        let batch = build(&fixture, &sdk, &[first.name.clone(), second.name.clone()]);
        let second_input = batch.saves.iter().find(|record| record.name == second.name).unwrap();
        save(&fixture, &sdk, input(second_input), second_input.system_fields.clone());
        let (saved, failed) = results(&fixture, &sdk, &batch);
        assert_eq!(saved.len(), 1);
        assert_eq!(failed.len(), 1);
        let mut malformed = failed.clone();
        match problem {
            "missing" => malformed[0].server_record = None,
            "code" => malformed[0].code = CloudEngineErrorCode::Other,
            "unknown" => {
                malformed[0].name = "unknown".into();
                malformed[0].server_record.as_mut().unwrap().name = "unknown".into();
            }
            "unhanded-existing" => {
                let (_, unhanded) = add_note(&fixture, "Never handed in this batch".into());
                malformed[0].name = unhanded.name.clone();
                malformed[0].server_record.as_mut().unwrap().name = unhanded.name;
            }
            other => {
                let record = malformed[0].server_record.as_mut().unwrap();
                match other {
                    "type" => record.record_type = Some("MGBlob".into()),
                    "zone" => record.zone = Some("meguri-media".into()),
                    "name" => record.name = first.name.clone(),
                    "system" => record.system_fields = None,
                    "empty-system" => record.system_fields = Some(vec![]),
                    "empty-envelope" => record.envelope.clear(),
                    "asset" => record.asset_path = Some("untrusted-error-file".into()),
                    "absent-type" => record.record_type = None,
                    "absent-zone" => record.zone = None,
                    _ => unreachable!(),
                }
            }
        }
        let before_first = note_facts(&fixture, first_id);
        let before_second = note_facts(&fixture, second_id);
        assert!(
            fixture
                .workflow
                .sent(sdk.instance_id.clone(), batch.id, saved.clone(), malformed, 2)
                .is_err()
        );
        assert!(fixture.workflow.handler_returned(sdk.instance_id.clone()).is_err());
        assert!(note_facts(&fixture, first_id) == before_first);
        assert!(note_facts(&fixture, second_id) == before_second);
        // Original batch and receipt still own the callback after rejection.
        assert_eq!(deliver(&fixture, &sdk, &batch, saved, failed), 2);
        fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
        assert_eq!(note_facts(&fixture, first_id).acked_rev, first.local_rev);
        assert_eq!(note_facts(&fixture, second_id).acked_rev, second.local_rev);
    }
}

#[test]
fn ffi_retained_media_other_stays_pending_and_saved_returns_validated_count() {
    for saved_reply in [false, true] {
        let (fixture, sdk) = published();
        let media = fixture.workflow.media().unwrap();
        accept_root(&media, &sdk);
        fixture
            .workflow
            .host
            .media()
            .unwrap()
            .store()
            .add("conflict.bin", b"real staged blob")
            .unwrap();
        let read = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        assert!(matches!(read.request, CloudMediaRequest::Read { .. }));
        media.reply(read.handle, CloudMediaReply::UnknownItem).unwrap();
        let upload = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        let CloudMediaRequest::Upload { name, staged_path, .. } = &upload.request else {
            panic!("original upload")
        };
        media.queue_upload(upload.handle.clone()).unwrap();
        for update in fixture.workflow.take_pending_updates(sdk.instance_id.clone()).unwrap() {
            fixture
                .workflow
                .update_applied(
                    sdk.instance_id.clone(),
                    update.id,
                    update.adds.keys().cloned().collect(),
                )
                .unwrap();
        }
        let batch = fixture
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                "meguri-media".into(),
                fixture.workflow.tracking_sendables().unwrap(),
            )
            .unwrap()
            .unwrap();
        let queue = Arc::new(ReadQueue(AtomicUsize::new(0)));
        assert!(
            fixture
                .workflow
                .submit_media_batch(sdk.instance_id.clone(), batch.id, queue)
                .unwrap()
        );
        // Missing/mismatched native blob proofs project to Other with no
        // server_record. The original retained media Work owns this callback.
        let (saved, failed) = if saved_reply {
            assert_eq!(batch.saves.len(), 1);
            let handed = &batch.saves[0];
            assert_eq!(&handed.name, name);
            assert_eq!(handed.record_type.as_deref(), Some("MGBlob"));
            assert_eq!(handed.asset_path.as_deref(), Some(staged_path.as_str()));
            let mut record = RecordInput::encrypted(name, "MGBlob", handed.envelope.clone());
            record.fields.insert(
                "a".into(),
                Field::Asset(Asset::synthetic(std::fs::read(staged_path).unwrap())),
            );
            let reply = fixture
                .server
                .lock()
                .unwrap()
                .request(
                    &fixture.scope,
                    Caller::default(),
                    Operation::Modify {
                        zone: "meguri-media".into(),
                        atomic: false,
                        changes: vec![Mutation::Save {
                            record,
                            if_unchanged: handed.system_fields.clone().map(SystemFields::from_bytes),
                        }],
                    },
                )
                .unwrap()
                .result
                .unwrap();
            let Reply::Modified(mut items) = reply else {
                panic!("actual media Service Modify")
            };
            assert_eq!(items.len(), 1);
            let MutationSuccess::Saved(record) = items.pop().unwrap().result.unwrap() else {
                panic!("actual media Saved")
            };
            assert_eq!(&record.name, name);
            assert_eq!(record.record_type, "MGBlob");
            let value = publication_control_record(&record).unwrap();
            assert!(!value.system_fields.0.is_empty());
            (
                vec![CloudRecord {
                    name: record.name,
                    envelope: value.encrypted.unwrap(),
                    system_fields: Some(value.system_fields.0),
                    record_type: Some("MGBlob".into()),
                    zone: Some("meguri-media".into()),
                    asset_path: None,
                }],
                vec![],
            )
        } else {
            (
                vec![],
                vec![CloudRecordFailure {
                    name: name.clone(),
                    code: CloudEngineErrorCode::Other,
                    server_record: None,
                }],
            )
        };
        assert_eq!(
            fixture
                .workflow
                .sent(sdk.instance_id.clone(), batch.id, saved, failed, 1)
                .unwrap(),
            u64::from(saved_reply)
        );
        fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
        fixture.workflow.core.validate_sdk(&sdk.instance_id).unwrap();
        let retained = fixture.workflow.core.validate_cloud_media_work(
            &fixture.workflow.host.collection().unwrap(),
            upload.handle.retained_for_test(),
        );
        assert_eq!(retained.is_ok(), !saved_reply);
        assert_eq!(std::path::Path::new(staged_path).exists(), !saved_reply);
        // A collection send still works after the parked media failure.
        let (_, note) = add_note(&fixture, "Collection stays alive".into());
        ack_base(&fixture, &sdk, &note.name);
    }
}
