//! Owned native read ports over the existing PublishedFixture and real selection.
use super::*;
use crate::cloud_asset_reads::*;
use crate::cloud_policy::*;

#[path = "cloud_asset_read_activity_tests.rs"]
mod activity;

fn waiting_note(fixture: &PublishedFixture, sdk: &CloudEngineRegistration) -> (Record, NoteId, Vec<u8>) {
    let (input, bytes, id) = remote_note(fixture, true);
    let server = save(fixture, sdk, input, None);
    let download = tempfile::tempdir().unwrap();
    let mut record = callback_record(&server, &sdk.collection_zone, download.path());
    record.asset_path = None;
    fixture
        .workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            vec![record],
            vec![],
        )
        .unwrap();
    (server, id, bytes)
}

fn attempts(fixture: &PublishedFixture, name: &str) -> u32 {
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    fixture
        .workflow
        .core
        .inbox_entries(binding)
        .unwrap()
        .into_iter()
        .find(|entry| entry.entry.record_name == name)
        .unwrap()
        .entry
        .retry_count
}

fn retry_pass(fixture: &PublishedFixture, sdk: &CloudEngineRegistration) -> Arc<CloudAssetReadPass> {
    let issued = retry_command(fixture, sdk);
    fixture
        .policy
        .clone()
        .prepare_execution(issued.handle)
        .unwrap()
        .unwrap()
        .begin_asset_read_pass()
        .unwrap()
}

struct Queue {
    calls: AtomicUsize,
    accept: bool,
}
impl Queue {
    fn new(accept: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            accept,
        })
    }
}
impl CloudAssetReadQueue for Queue {
    fn enqueue(&self) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.accept
    }
}

fn retry_command(fixture: &PublishedFixture, sdk: &CloudEngineRegistration) -> CloudPolicyIssuedCommand {
    let workflow = &fixture.workflow;
    let fetch = workflow
        .begin_fetch(sdk.instance_id.clone(), vec![sdk.collection_zone.clone()])
        .unwrap();
    workflow
        .will_fetch(sdk.instance_id.clone(), "asset-read-drain".into(), true)
        .unwrap();
    workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), true, false)
        .unwrap();
    workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), false, false)
        .unwrap();
    workflow
        .did_fetch(sdk.instance_id.clone(), "asset-read-drain".into(), true)
        .unwrap();
    assert!(workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
    policy_commands(
        fixture
            .policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
    )
    .into_iter()
    .find(|issued| matches!(issued.action, CloudPolicyAction::RetryAssetEntries))
    .expect("actual drained collection fetch must issue RetryAssetEntries")
}

// Retry reads use the dedicated execution port; generic submission stays denied.
#[test]
fn retry_command_prepares_read_execution_while_generic_submit_is_denied() {
    let (fixture, sdk) = published();
    waiting_note(&fixture, &sdk);
    let issued = retry_command(&fixture, &sdk);
    let queue = Arc::new(CancelTimerQueue {
        timers: fixture.outstanding_timers.clone(),
        calls: AtomicUsize::new(0),
    });
    assert!(matches!(
        fixture
            .policy
            .clone()
            .submit_native(issued.handle.clone(), queue.clone()),
        Err(CoreError::CloudPolicy {
            failure: CloudPolicyFailure::CoreCapabilityUnavailable
        })
    ));
    assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
    let execution = fixture
        .policy
        .clone()
        .prepare_execution(issued.handle)
        .unwrap()
        .unwrap();
    assert_eq!(execution.original_sdk().unwrap(), Some(sdk.instance_id));
}

#[test]
fn owned_retry_read_intake_drives_existing_guarded_writer_and_wal() {
    let (fixture, sdk) = published();
    let (server, id, bytes) = waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    let issued = pass.clone().next().unwrap().unwrap();
    assert_eq!(issued.name, server.name);
    assert_eq!(issued.zone, sdk.collection_zone);
    assert_eq!(issued.kind, CloudAssetReadKind::Retry);
    assert!(pass.clone().next().is_err());
    assert_eq!(attempts(&fixture, &issued.name), 0);
    let queue = Queue::new(true);
    assert!(pass.submit(issued.handle.clone(), queue.clone()).unwrap());
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    assert_eq!(attempts(&fixture, &issued.name), 1);
    assert!(!pass.cancel(issued.handle.clone()).unwrap());
    // Ending the pass must retain the accepted operation and its charge.
    pass.finish().unwrap();
    let download = tempfile::tempdir().unwrap();
    let callback = callback_record(
        &read(&fixture, &sdk, &server.name),
        &sdk.collection_zone,
        download.path(),
    );
    let source = callback.asset_path.clone().unwrap();
    let original = Arc::downgrade(&fixture.workflow.core);
    let name = server.name.clone();
    let prepared = Arc::new(AtomicUsize::new(0));
    let observed = prepared.clone();
    fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::AssetReadPrepared {
            // The owned copy already exists; no collection apply/ACK yet.
            std::fs::remove_file(&source).unwrap();
            assert!(original.upgrade().unwrap().entity_facts(&name).unwrap().is_none());
            observed.fetch_add(1, Ordering::SeqCst);
        }
    });
    let done = pass.complete(issued.handle.clone(), Some(callback)).unwrap();
    assert!(done.intake_accepted);
    assert_eq!(prepared.load(Ordering::SeqCst), 1);
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
            .contains(&server.name)
    );
    assert!(!fixture.workflow.core.incoming_wal_receipts().is_empty());
    assert!(
        fixture
            .workflow
            .core
            .incoming_runs()
            .iter()
            .any(|run| run.changed_keys.contains(&server.name))
    );
    let tracking = fixture.workflow.durable_snapshot().unwrap().tracking_content;
    let runs = fixture.workflow.core.incoming_runs();
    let wal_count = fixture.workflow.core.incoming_wal_receipts().len();
    assert!(
        !fixture
            .workflow
            .core
            .asset_read_owned_for_testing(issued.handle.retained_for_test())
    );
    assert!(matches!(
        pass.complete(issued.handle.clone(), None),
        Err(CoreError::Unavailable { reason }) if reason == "cloud policy command is stale"
    ));
    assert!(
        !fixture
            .workflow
            .core
            .asset_read_owned_for_testing(issued.handle.retained_for_test())
    );
    assert_eq!(fixture.workflow.core.incoming_runs(), runs);
    assert_eq!(fixture.workflow.core.incoming_wal_receipts().len(), wal_count);
    assert!(note_facts(&fixture, id) == facts);
    assert_eq!(fixture.workflow.durable_snapshot().unwrap().tracking_content, tracking);
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    assert_eq!(prepared.load(Ordering::SeqCst), 1);
}

#[test]
fn one_actual_retry_pass_completes_two_entries_through_normal_writer_and_wal() {
    let (fixture, sdk) = published();
    let first = waiting_note(&fixture, &sdk);
    let second = waiting_note(&fixture, &sdk);
    assert_ne!(first.0.name, second.0.name);
    assert_ne!(first.1, second.1);
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    let entries = fixture.workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 2);
    for (server, _, _) in [&first, &second] {
        let entry = entries
            .iter()
            .find(|entry| entry.entry.record_name == server.name)
            .unwrap();
        assert_eq!(entry.entry.reason, c::InboxReason::Asset);
        assert_eq!(entry.entry.source, c::InboxSource::Fetch);
        assert_eq!(entry.entry.retry_count, 0);
        let c::InboxValue::Record {
            envelope, staged_asset, ..
        } = &entry.entry.value
        else {
            panic!("actual asset entry");
        };
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&envelope.0).unwrap() else {
            panic!("actual pa entity");
        };
        assert!(entity.payload.is_none() && entity.fields.generation.is_none());
        assert!(staged_asset.is_none());
    }
    // Keep this one drain-issued execution/pass across both normal writer calls.
    let pass = retry_pass(&fixture, &sdk);
    let queue = Queue::new(true);
    let mut completed = std::collections::BTreeSet::new();
    for count in 1..=2 {
        let work = pass.clone().next().unwrap().unwrap();
        let (server, id, bytes) = [&first, &second]
            .into_iter()
            .find(|(server, _, _)| server.name == work.name)
            .expect("selected one of the two original entries");
        assert!(completed.insert(work.name.clone()));
        assert_eq!(work.kind, CloudAssetReadKind::Retry);
        assert_eq!(work.zone, sdk.collection_zone);
        assert!(pass.submit(work.handle.clone(), queue.clone()).unwrap());
        assert_eq!(queue.calls.load(Ordering::SeqCst), count);
        assert_eq!(attempts(&fixture, &server.name), 1);
        let download = tempfile::tempdir().unwrap();
        let callback = callback_record(
            &read(&fixture, &sdk, &server.name),
            &sdk.collection_zone,
            download.path(),
        );
        assert!(pass.complete(work.handle, Some(callback)).unwrap().intake_accepted);
        let note = fixture
            .workflow
            .host
            .collection()
            .unwrap()
            .reader(ReaderRole::Query)
            .unwrap()
            .note(*id)
            .unwrap()
            .unwrap();
        let codec::Payload::Note(payload) = codec::Payload::decode(codec::Kind::Note, bytes).unwrap() else {
            panic!("note");
        };
        assert!(note.fields.as_bytes() == payload.flds);
        let facts = note_facts(&fixture, *id);
        assert_eq!(facts.acked_payload_digest, codec::sha256(bytes));
        assert_eq!(facts.acked_system_fields, server.system_fields.bytes());
        assert!(facts.acked_payload.as_ref() == Some(&codec::Payload::Note(payload)));
        assert!(
            !fixture
                .workflow
                .durable_snapshot()
                .unwrap()
                .inbox_names
                .contains(&server.name)
        );
        let runs = fixture.workflow.core.incoming_runs();
        let run = runs.iter().find(|run| run.changed_keys.contains(&server.name)).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.entry.record_name == server.name)
            .unwrap();
        assert!(run.entry_ids.contains(&entry.id));
        assert!(
            fixture.workflow.core.incoming_wal_receipts().iter().any(|receipt| {
                receipt.identity().generation().0 == run.generation && receipt.through().0 >= run.seq
            })
        );
    }
    assert_eq!(completed.len(), 2);
    assert!(pass.clone().next().unwrap().is_none());
    pass.finish().unwrap();
    assert!(fixture.workflow.core.inbox_entries(binding).unwrap().is_empty());
    assert_eq!(queue.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn owned_read_refusal_and_unstarted_cancel_are_uncharged() {
    let (fixture, sdk) = published();
    let (server, _, _) = waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    let work = pass.clone().next().unwrap().unwrap().handle;
    let queue = Queue::new(false);
    assert!(!pass.submit(work.clone(), queue.clone()).unwrap());
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    assert_eq!(attempts(&fixture, &server.name), 0);
    assert!(pass.clone().next().unwrap().is_none());
    let tracking = fixture.workflow.durable_snapshot().unwrap().tracking_content;
    let facts = fixture.workflow.core.entity_facts(&server.name).unwrap();
    let runs = fixture.workflow.core.incoming_runs();
    let wal_count = fixture.workflow.core.incoming_wal_receipts().len();
    assert!(
        !fixture
            .workflow
            .core
            .asset_read_owned_for_testing(work.retained_for_test())
    );
    assert!(matches!(
        pass.complete(work.clone(), None),
        Err(CoreError::Unavailable { reason }) if reason == "cloud policy command is stale"
    ));
    assert!(
        !fixture
            .workflow
            .core
            .asset_read_owned_for_testing(work.retained_for_test())
    );
    assert_eq!(attempts(&fixture, &server.name), 0);
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.workflow.core.incoming_runs(), runs);
    assert_eq!(fixture.workflow.core.incoming_wal_receipts().len(), wal_count);
    assert!(fixture.workflow.core.entity_facts(&server.name).unwrap() == facts);
    assert_eq!(fixture.workflow.durable_snapshot().unwrap().tracking_content, tracking);
    let next = retry_pass(&fixture, &sdk);
    let work = next.clone().next().unwrap().unwrap().handle;
    assert!(next.cancel(work.clone()).unwrap());
    assert_eq!(attempts(&fixture, &server.name), 0);
    assert!(next.submit(work, Queue::new(true)).is_err());
}

#[test]
fn owned_read_rejects_foreign_pass_host_sdk_and_nonretry_command() {
    let (fixture, sdk) = published();
    waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    let work = pass.clone().next().unwrap().unwrap().handle;
    let other_pass = retry_pass(&fixture, &sdk);
    let (other_host, other_sdk) = published();
    waiting_note(&other_host, &other_sdk);
    let foreign_pass = retry_pass(&other_host, &other_sdk);
    let queue = Queue::new(true);
    for owner in [&other_pass, &foreign_pass] {
        assert!(owner.submit(work.clone(), queue.clone()).is_err());
        assert!(owner.complete(work.clone(), None).is_err());
        assert!(owner.cancel(work.clone()).is_err());
    }
    assert!(
        fixture
            .workflow
            .begin_first_asset_read_pass("wrong-sdk".into())
            .is_err()
    );
    let nonretry = fixture
        .outstanding_collection
        .lock()
        .unwrap()
        .iter()
        .find(|issued| matches!(issued.action, CloudPolicyAction::RequestFetch { .. }))
        .unwrap()
        .handle
        .clone();
    let execution = fixture.policy.clone().prepare_execution(nonretry).unwrap().unwrap();
    assert!(execution.begin_asset_read_pass().is_err());
    assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
    assert!(pass.submit(work.clone(), queue).unwrap());
    pass.complete(work, None).unwrap();
}

#[test]
fn paused_policy_blocks_selected_read_before_queue_admission() {
    let (fixture, sdk) = published();
    let (server, _, _) = waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    let work = pass.clone().next().unwrap().unwrap().handle;
    fixture
        .policy
        .clone()
        .input(CloudPolicyEvent::Settings {
            settings: CloudPolicyDeviceSettings {
                enabled: true,
                paused: true,
                automatic: false,
                media: false,
            },
        })
        .unwrap();
    let queue = Queue::new(true);
    assert!(pass.submit(work, queue.clone()).is_err());
    assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
    assert_eq!(attempts(&fixture, &server.name), 0);
}

#[test]
fn accepted_read_accounting_fault_keeps_original_owner_until_terminal() {
    let (fixture, sdk) = published();
    let (server, _, _) = waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    let work = pass.clone().next().unwrap().unwrap().handle;
    c::testing::arm_tracking_fault("cloud.tracking.commit.asset_read", c::testing::TrackingFault::Io).unwrap();
    let queue = Queue::new(true);
    assert!(pass.submit(work.clone(), queue.clone()).is_err());
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    assert!(
        fixture
            .workflow
            .core
            .asset_read_owned_for_testing(work.retained_for_test())
    );
    assert_eq!(attempts(&fixture, &server.name), 0);
    assert!(!pass.cancel(work.clone()).unwrap());
    pass.finish().unwrap();
    assert!(
        fixture
            .workflow
            .core
            .asset_read_owned_for_testing(work.retained_for_test())
    );
    // Outstanding accepted accounting is an authority fence, not a refused read.
    assert!(fixture.workflow.begin_first_asset_read_pass(sdk.instance_id).is_err());
    assert!(!pass.complete(work.clone(), None).unwrap().intake_accepted);
    assert!(
        !fixture
            .workflow
            .core
            .asset_read_owned_for_testing(work.retained_for_test())
    );
}

#[test]
fn stale_sdk_or_policy_terminal_retires_only_original_read_with_actual_successor() {
    for policy_pause in [false, true] {
        let (fixture, sdk) = published();
        let (server, _, _) = waiting_note(&fixture, &sdk);
        let old = retry_pass(&fixture, &sdk);
        let original = old.clone().next().unwrap().unwrap().handle;
        assert!(old.submit(original.clone(), Queue::new(true)).unwrap());
        let root = fixture.workflow.core.adapter_snapshot().unwrap().root.unwrap();
        if policy_pause {
            for paused in [true, false] {
                fixture
                    .policy
                    .clone()
                    .input(CloudPolicyEvent::Settings {
                        settings: CloudPolicyDeviceSettings {
                            enabled: true,
                            paused,
                            automatic: false,
                            media: false,
                        },
                    })
                    .unwrap();
            }
        }
        fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
        fixture.workflow.release_sdk(sdk.instance_id).unwrap();
        let successor_sdk = fixture.workflow.register_sdk(false).unwrap();
        fixture
            .workflow
            .core
            .accept_root_fetch(
                &successor_sdk.instance_id,
                Some(&w::Saved {
                    name: "active".into(),
                    envelope: root.envelope.0,
                    system_fields: root.system_fields.0,
                }),
                true,
                1,
            )
            .unwrap();
        fixture
            .policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap();
        let successor = retry_pass(&fixture, &successor_sdk);
        let current = successor.clone().next().unwrap().unwrap().handle;
        assert!(successor.submit(current.clone(), Queue::new(true)).unwrap());
        assert_eq!(attempts(&fixture, &server.name), 2);
        assert!(old.complete(original.clone(), None).is_err());
        assert!(
            !fixture
                .workflow
                .core
                .asset_read_owned_for_testing(original.retained_for_test())
        );
        assert!(
            fixture
                .workflow
                .core
                .asset_read_owned_for_testing(current.retained_for_test())
        );
        // Late duplicate/cancel of the original must not retire the successor.
        assert!(!old.cancel(original.clone()).unwrap());
        assert!(old.complete(original, None).is_err());
        assert!(
            fixture
                .workflow
                .core
                .asset_read_owned_for_testing(current.retained_for_test())
        );
        let download = tempfile::tempdir().unwrap();
        let callback = callback_record(&server, &successor_sdk.collection_zone, download.path());
        assert!(successor.complete(current, Some(callback)).unwrap().intake_accepted);
        assert!(
            !fixture
                .workflow
                .durable_snapshot()
                .unwrap()
                .inbox_names
                .contains(&server.name)
        );
        assert!(!fixture.workflow.core.incoming_wal_receipts().is_empty());
    }
}

#[test]
fn accepted_original_terminal_cleans_up_after_account_generation_or_close() {
    for transition in 0..3 {
        let (fixture, sdk) = published();
        waiting_note(&fixture, &sdk);
        let pass = retry_pass(&fixture, &sdk);
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        match transition {
            0 => fixture
                .workflow
                .account_changed(None, CloudAccountStatus::NoAccount, None)
                .unwrap(),
            1 => {
                let engine = fixture.workflow.host.engine();
                let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive
                    .replace(|path| {
                        assert!(path.exists());
                        Ok(())
                    })
                    .unwrap();
            }
            _ => fixture.workflow.host.close().unwrap(),
        }
        assert!(pass.complete(work.clone(), None).is_err());
        assert!(
            !fixture
                .workflow
                .core
                .asset_read_owned_for_testing(work.retained_for_test())
        );
    }
}

#[test]
fn actual_handed_pa_conflict_grants_one_uncharged_first_read_after_handler_return() {
    let (fixture, sdk) = published();
    let (id, original) = add_note(&fixture, "Base back".into());
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
    let baseline_batch = fixture
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            fixture
                .workflow
                .tracking_sendables()
                .unwrap()
                .into_iter()
                .filter(|(name, _)| name == &original.name)
                .collect(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(baseline_batch.saves.len(), 1);
    let baseline_handed = &baseline_batch.saves[0];
    let baseline = save(
        &fixture,
        &sdk,
        RecordInput::encrypted(&baseline_handed.name, "MGEntity", baseline_handed.envelope.clone()),
        baseline_handed.system_fields.clone(),
    );
    let baseline_download = tempfile::tempdir().unwrap();
    fixture
        .workflow
        .sent(
            sdk.instance_id.clone(),
            baseline_batch.id,
            vec![callback_record(
                &baseline,
                &sdk.collection_zone,
                baseline_download.path(),
            )],
            vec![],
            vec![],
            vec![],
            1,
        )
        .unwrap();
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    fixture
        .workflow
        .core
        .sdk_saved_removed(
            &sdk.instance_id,
            baseline_batch.id,
            std::slice::from_ref(&baseline.name),
        )
        .unwrap();
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
    let acked = note_facts(&fixture, id);
    assert!(acked.acked_payload.as_ref() == Some(&original.payload));
    assert_eq!(
        acked.acked_payload_digest,
        codec::sha256(&original.payload.encode().unwrap())
    );
    assert_eq!(acked.acked_system_fields, baseline.system_fields.bytes());
    assert_eq!(acked.acked_rev, original.local_rev);
    assert!(acked.in_flight.is_empty());
    assert!(
        !fixture
            .workflow
            .tracking_sendables()
            .unwrap()
            .contains_key(&original.name)
    );

    // Disjoint edits from a real Saved base force local-front + remote-back.
    let local_front = "local".repeat(160_000);
    let remote_back = "remote".repeat(160_000);
    let collection = fixture.workflow.host.collection().unwrap();
    let edited = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id,
            fields: vec![local_front.clone(), "Base back".into()],
            tags: vec![],
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, edited.seq.0)
        .unwrap();
    let local = note_facts(&fixture, id);
    assert!(local.base_payload.as_ref() == Some(&original.payload));
    let batch = fixture
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            fixture
                .workflow
                .tracking_sendables()
                .unwrap()
                .into_iter()
                .filter(|(name, _)| name == &original.name)
                .collect(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    let handed = &batch.saves[0];
    assert!(std::fs::read(handed.asset_path.as_ref().unwrap()).unwrap() == local.payload.encode().unwrap());
    assert_eq!(handed.system_fields.as_deref(), Some(baseline.system_fields.bytes()));
    let baseline_value = publication_control_record(&baseline).unwrap();
    let codec::DecodeOutcome::Supported(envelope) = codec::Entity::decode(&baseline_value.encrypted.unwrap()).unwrap()
    else {
        panic!("actual Saved base entity")
    };
    let codec::Payload::Note(mut remote) = original.payload.clone() else {
        panic!("note")
    };
    remote.flds = format!("Question\u{1f}{remote_back}").into_bytes();
    let bytes = codec::Payload::Note(remote).encode().unwrap();
    let mut fields = envelope.fields;
    fields.writer = [84; 16];
    fields.erev += 1;
    fields.base_sha256 = Some(acked.acked_payload_digest);
    let remote = codec::Entity::build(fields, &bytes).unwrap();
    assert!(remote.payload.is_none());
    let mut input = RecordInput::encrypted(&handed.name, "MGEntity", remote.encode().unwrap());
    input
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(bytes.clone())));
    let concurrent = save(&fixture, &sdk, input, Some(baseline.system_fields.bytes().to_vec()));
    assert_ne!(concurrent.system_fields.bytes(), baseline.system_fields.bytes());
    let mut input = RecordInput::encrypted(&handed.name, "MGEntity", handed.envelope.clone());
    input.fields.insert(
        "pa".into(),
        Field::Asset(Asset::synthetic(
            std::fs::read(handed.asset_path.as_ref().unwrap()).unwrap(),
        )),
    );
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
                    if_unchanged: handed.system_fields.clone().map(SystemFields::from_bytes),
                }],
            },
        )
        .unwrap()
        .result
        .unwrap();
    let Reply::Modified(mut items) = reply else {
        panic!("actual native-style result")
    };
    let error = items.pop().unwrap().result.err().unwrap();
    assert_eq!(error.code, ErrorCode::ServerRecordChanged);
    let server = error.server_record.unwrap();
    assert_eq!(server.system_fields.bytes(), concurrent.system_fields.bytes());
    let value = publication_control_record(&server).unwrap();
    let collection = fixture.workflow.host.collection().unwrap();
    // Exercise the existing narrow core intake with the actual original handed
    // batch and actual server conflict. Generic FFI conflict routing is later.
    fixture
        .workflow
        .core
        .enter_sent_handler(&sdk.instance_id, batch.id)
        .unwrap();
    fixture
        .workflow
        .core
        .sent_asset_save_conflict(
            &sdk.instance_id,
            &collection,
            batch.id,
            &w::Saved {
                name: server.name.clone(),
                envelope: value.encrypted.unwrap(),
                system_fields: value.system_fields.0,
            },
        )
        .unwrap();
    assert!(
        fixture
            .workflow
            .begin_first_asset_read_pass(sdk.instance_id.clone())
            .is_err()
    );
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    let first = fixture
        .workflow
        .begin_first_asset_read_pass(sdk.instance_id.clone())
        .unwrap();
    let work = first.clone().next().unwrap().unwrap();
    assert_eq!(work.name, original.name);
    assert_eq!(work.kind, CloudAssetReadKind::SaveConflictFirst);
    assert_eq!(attempts(&fixture, &server.name), 0);
    let refused = Queue::new(false);
    assert!(!first.submit(work.handle, refused.clone()).unwrap());
    assert_eq!(refused.calls.load(Ordering::SeqCst), 1);
    assert_eq!(attempts(&fixture, &server.name), 0);
    first.finish().unwrap();
    let first = fixture
        .workflow
        .begin_first_asset_read_pass(sdk.instance_id.clone())
        .unwrap();
    let work = first.clone().next().unwrap().unwrap().handle;
    let accepted = Queue::new(true);
    assert!(first.submit(work.clone(), accepted.clone()).unwrap());
    assert_eq!(accepted.calls.load(Ordering::SeqCst), 1);
    assert_eq!(attempts(&fixture, &server.name), 0);
    assert!(!first.cancel(work.clone()).unwrap());
    first.finish().unwrap();
    let download = tempfile::tempdir().unwrap();
    let callback = callback_record(
        &read(&fixture, &sdk, &server.name),
        &sdk.collection_zone,
        download.path(),
    );
    assert!(first.complete(work, Some(callback)).unwrap().intake_accepted);
    assert_eq!(accepted.calls.load(Ordering::SeqCst), 1);
    let merged_fields = format!("{local_front}\u{1f}{remote_back}").into_bytes();
    let note = collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap();
    assert!(note.fields.as_bytes() == merged_fields);
    assert!(
        fixture
            .workflow
            .begin_first_asset_read_pass(sdk.instance_id.clone())
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
    let facts = note_facts(&fixture, id);
    assert_eq!(facts.acked_payload_digest, codec::sha256(&bytes));
    assert_eq!(facts.acked_system_fields, server.system_fields.bytes());
    assert_eq!(facts.server_erev, remote.fields.erev);
    assert!(facts.acked_payload.as_ref() == Some(&codec::Payload::decode(codec::Kind::Note, &bytes).unwrap()));
    let codec::Payload::Note(merged) = &facts.payload else {
        panic!("merged note");
    };
    assert!(merged.flds == merged_fields);
    assert!(facts.mark.is_some());
    assert!(facts.in_flight.is_empty());
    assert!(matches!(
        fixture.workflow.tracking_sendables().unwrap().get(&server.name),
        Some(CloudPendingChange::Save)
    ));
    assert!(
        fixture
            .workflow
            .take_pending_updates(sdk.instance_id.clone())
            .unwrap()
            .iter()
            .any(|update| matches!(update.adds.get(&server.name), Some(CloudPendingChange::Save)))
    );
    assert!(
        !fixture
            .workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&server.name)
    );
    assert!(
        fixture
            .workflow
            .core
            .incoming_runs()
            .iter()
            .any(|run| run.changed_keys.contains(&server.name))
    );
    assert!(!fixture.workflow.core.incoming_wal_receipts().is_empty());
}

#[test]
fn prepared_asset_queue_callback_allows_actual_core_query_from_another_thread() {
    struct InspectQueue {
        host: Arc<meguri_host::CoreHost>,
        core: Arc<r::Runtime>,
    }
    impl CloudAssetReadQueue for InspectQueue {
        fn enqueue(&self) -> bool {
            let host = self.host.clone();
            let core = self.core.clone();
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            let query = std::thread::spawn(move || {
                host.cloud_policy_driver().unwrap().status();
                let binding = core.status().unwrap().binding.unwrap().id;
                core.inbox_entries(binding).unwrap();
                core.adapter_snapshot().unwrap();
                send.send(()).unwrap();
            });
            // Same five-second synchronization deadline used by existing FFI
            // provider tests. This is a lock-liveness probe, with no sleeps.
            receive
                .recv_timeout(Duration::from_secs(5))
                .expect("queue held a core lock");
            query.join().unwrap();
            true
        }
    }
    let (fixture, sdk) = published();
    waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    let work = pass.clone().next().unwrap().unwrap().handle;
    assert!(
        pass.submit(
            work.clone(),
            Arc::new(InspectQueue {
                host: fixture.workflow.host.clone(),
                core: fixture.workflow.core.clone(),
            })
        )
        .unwrap()
    );
    pass.complete(work, None).unwrap();
}

#[test]
fn read_completion_rejects_metadata_and_retries_bad_source_bytes_without_ack() {
    for problem in [
        "name",
        "zone",
        "type",
        "absent-metadata",
        "missing",
        "symlink",
        "digest",
    ] {
        let (fixture, sdk) = published();
        let (server, id, _) = waiting_note(&fixture, &sdk);
        let pass = retry_pass(&fixture, &sdk);
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        let download = tempfile::tempdir().unwrap();
        let mut callback = callback_record(&server, &sdk.collection_zone, download.path());
        match problem {
            "name" => callback.name.push('x'),
            "zone" => callback.zone = Some("meguri-root".into()),
            "type" => callback.record_type = Some("MGGeneration".into()),
            "absent-metadata" => callback.record_type = None,
            "missing" => std::fs::remove_file(callback.asset_path.as_ref().unwrap()).unwrap(),
            "symlink" => {
                let source = callback.asset_path.as_ref().unwrap();
                let target = download.path().join("target");
                std::fs::rename(source, &target).unwrap();
                std::os::unix::fs::symlink(target, source).unwrap();
            }
            "digest" => {
                let source = callback.asset_path.as_ref().unwrap();
                let mut bytes = std::fs::read(source).unwrap();
                *bytes.last_mut().unwrap() ^= 1;
                std::fs::write(source, bytes).unwrap();
            }
            _ => unreachable!(),
        }
        let result = pass.complete(work.clone(), Some(callback));
        if ["name", "zone", "type", "absent-metadata"].contains(&problem) {
            assert!(result.is_err());
        } else {
            assert!(!result.unwrap().intake_accepted);
        }
        assert!(
            !fixture
                .workflow
                .core
                .asset_read_owned_for_testing(work.retained_for_test())
        );
        assert_eq!(attempts(&fixture, &server.name), 1);
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
        assert!(fixture.workflow.core.incoming_wal_receipts().is_empty());
    }
}
