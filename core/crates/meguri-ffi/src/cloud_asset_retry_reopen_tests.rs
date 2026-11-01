//! K22.2 retryable fetch assets: real Service, drained policy actions and host reopen.
//! This is the Rust/FFI native delivery port, not Swift or Apple SDK acceptance.
use super::*;
use meguri_testkit::cloud::{RecordChange, Token};

fn refresh(fixture: &PublishedFixture) -> Vec<CloudPolicyIssuedCommand> {
    policy_commands(
        fixture
            .policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
    )
}

fn no_retry(commands: &[CloudPolicyIssuedCommand]) {
    assert!(
        commands
            .iter()
            .all(|command| !matches!(command.action, CloudPolicyAction::RetryAssetEntries))
    );
}

fn entry(fixture: &PublishedFixture, name: &str) -> c::StoredInboxEntry {
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    fixture
        .workflow
        .core
        .inbox_entries(binding)
        .unwrap()
        .into_iter()
        .find(|entry| entry.entry.record_name == name)
        .expect("original asset inbox entry")
}

fn physical_fields(fixture: &PublishedFixture, id: NoteId) -> Vec<u8> {
    fixture
        .workflow
        .host
        .collection()
        .unwrap()
        .reader(ReaderRole::Query)
        .unwrap()
        .note(id)
        .unwrap()
        .unwrap()
        .fields
        .as_bytes()
        .to_vec()
}

// Complete every Service page between the real SDK callbacks. Only the SDK's
// disposable asset file is unavailable; the Service envelope/version is intact.
fn fetch_zone_pages(
    fixture: &PublishedFixture,
    sdk: &CloudEngineRegistration,
    zone: &str,
    token: &mut Option<Token>,
    missing_asset: &str,
) {
    fixture
        .workflow
        .fetch_zone(sdk.instance_id.clone(), zone.into(), true, false)
        .unwrap();
    loop {
        let Reply::ZoneChanges(page) = fixture
            .server
            .lock()
            .unwrap()
            .request(
                &fixture.scope,
                Caller::default(),
                Operation::FetchZone {
                    zone: zone.into(),
                    token: token.clone(),
                },
            )
            .unwrap()
            .result
            .unwrap()
        else {
            panic!("actual Service FetchZone page");
        };
        let download = tempfile::tempdir().unwrap();
        let mut records = Vec::new();
        let mut deleted = Vec::new();
        for change in page.changes {
            match change {
                RecordChange::Saved(record) => {
                    let value = publication_control_record(&record).unwrap();
                    if record.name == missing_asset {
                        let mut callback = callback_record(&record, zone, download.path());
                        std::fs::remove_file(callback.asset_path.as_ref().unwrap()).unwrap();
                        // The native delivery port reports its missing download.
                        callback.asset_path = None;
                        records.push(callback);
                    } else {
                        records.push(CloudRecord {
                            name: record.name,
                            envelope: value.encrypted.unwrap(),
                            system_fields: Some(value.system_fields.0),
                            zone: Some(zone.into()),
                            ..Default::default()
                        });
                    }
                }
                RecordChange::Deleted { name, .. } => deleted.push(name),
            }
        }
        fixture
            .workflow
            .fetched(sdk.instance_id.clone(), zone.into(), records, deleted)
            .unwrap();
        *token = Some(page.token);
        if !page.more {
            break;
        }
    }
    fixture
        .workflow
        .fetch_zone(sdk.instance_id.clone(), zone.into(), false, false)
        .unwrap();
}

fn collection_drain(
    fixture: &PublishedFixture,
    sdk: &CloudEngineRegistration,
    token: &mut Option<Token>,
    missing_asset: &str,
) -> Vec<CloudPolicyIssuedCommand> {
    let fetch = fixture
        .workflow
        .begin_fetch(sdk.instance_id.clone(), vec![sdk.collection_zone.clone()])
        .unwrap();
    let group = format!("asset-reopen-{fetch}");
    fixture
        .workflow
        .will_fetch(sdk.instance_id.clone(), group.clone(), true)
        .unwrap();
    fetch_zone_pages(fixture, sdk, &sdk.collection_zone, token, missing_asset);
    fixture
        .workflow
        .did_fetch(sdk.instance_id.clone(), group, true)
        .unwrap();
    assert!(fixture.workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
    refresh(fixture)
}

fn issued_retry(commands: Vec<CloudPolicyIssuedCommand>) -> CloudPolicyIssuedCommand {
    let mut retry = commands
        .into_iter()
        .filter(|command| matches!(command.action, CloudPolicyAction::RetryAssetEntries));
    let issued = retry
        .next()
        .expect("first qualifying collection drain automatically issues retry");
    assert!(retry.next().is_none(), "at most one RetryAssetEntries per drain");
    issued
}

// This sends only a real engine-captured note. ACK is exclusively the actual
// Service conditional Saved callback for the original handed batch.
fn save_pending_note(fixture: &PublishedFixture, sdk: &CloudEngineRegistration, name: &str) -> Record {
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
    let pending = fixture
        .workflow
        .tracking_sendables()
        .unwrap()
        .into_iter()
        .filter(|(key, _)| key == name)
        .collect();
    let batch = fixture
        .workflow
        .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), pending)
        .unwrap()
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    let handed = &batch.saves[0];
    let mut input = RecordInput::encrypted(&handed.name, "MGEntity", handed.envelope.clone());
    if let Some(path) = &handed.asset_path {
        input.fields.insert(
            "pa".into(),
            Field::Asset(Asset::synthetic(std::fs::read(path).unwrap())),
        );
    }
    let saved = save(fixture, sdk, input, handed.system_fields.clone());
    let download = tempfile::tempdir().unwrap();
    fixture
        .workflow
        .sent(
            sdk.instance_id.clone(),
            batch.id,
            vec![callback_record(&saved, &sdk.collection_zone, download.path())],
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
        .sdk_saved_removed(&sdk.instance_id, batch.id, std::slice::from_ref(&saved.name))
        .unwrap();
    saved
}

// Compare durable note semantics across opens, without treating another engine's
// observer sequence/open counter as authority for this engine's writer.
fn same_pending_note(actual: &w::EntityFacts, expected: &w::EntityFacts) {
    assert_eq!(actual.name, expected.name);
    assert_eq!(actual.origin, expected.origin);
    assert_eq!(actual.local_rev, expected.local_rev);
    assert_eq!(actual.acked_rev, expected.acked_rev);
    assert!(actual.payload == expected.payload);
    assert!(actual.base_payload == expected.base_payload);
    assert!(actual.mark == expected.mark);
    assert_eq!(actual.server_erev, expected.server_erev);
    assert_eq!(actual.acked_row_digest, expected.acked_row_digest);
    assert_eq!(actual.acked_payload_digest, expected.acked_payload_digest);
    assert_eq!(actual.acked_system_fields, expected.acked_system_fields);
    assert!(actual.acked_payload == expected.acked_payload);
    assert!(actual.acked_envelope == expected.acked_envelope);
    assert!(actual.in_flight == expected.in_flight);
}

// Reuse the directories and Service owned by this exact fixture. No publication,
// bootstrap, fresh key provider, adoption or baseline fixture runs here.
fn reopen_host(mut fixture: PublishedFixture) -> PublishedFixture {
    fixture.workflow.host.close().unwrap();
    let mut config = HostConfig::new(
        fixture._directory.path().join("collection"),
        fixture._directory.path().join("backups"),
        fixture._directory.path().join("widget"),
        fixture.clock.clone(),
        fixture.random.clone(),
        Arc::new(meguri_storage::space::StatvfsSpace),
    );
    config.cloud = Some(r::CloudInputs {
        scope: c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
        identity: Arc::new(Account),
        files: Arc::new(NativeFiles),
    });
    let host = Arc::new(CoreHost::open(config).unwrap());
    let core = host.cloud_runtime().unwrap().clone();
    core.retain_adapter_inspection();
    fixture.fixture.workflow = CloudWorkflow {
        host,
        core,
        identity: None,
    };
    fixture.policy = fixture.workflow.policy().unwrap();
    fixture.outstanding_collection.lock().unwrap().clear();
    fixture.outstanding_timers.lock().unwrap().clear();
    fixture
}

// Start the reopened native port using only the real policy's issued action.
// Network delivery of later collection work is driven explicitly by the test.
fn start_reopened(fixture: &PublishedFixture) -> CloudEngineRegistration {
    let mut commands = Vec::new();
    commands.extend(policy_commands(
        fixture
            .policy
            .clone()
            .input(CloudPolicyEvent::Settings {
                settings: CloudPolicyDeviceSettings {
                    enabled: true,
                    paused: false,
                    automatic: true,
                    media: false,
                },
            })
            .unwrap(),
    ));
    commands.extend(policy_commands(fixture.policy.clone().refresh_account().unwrap()));
    commands.extend(policy_commands(
        fixture
            .policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    ));
    let mut start = commands
        .into_iter()
        .filter(|command| matches!(command.action, CloudPolicyAction::StartEngine { .. }));
    let issued = start
        .next()
        .expect("reopened host issues startup using its durable binding and live account");
    assert!(start.next().is_none());
    let execution = fixture
        .policy
        .clone()
        .prepare_execution(issued.handle)
        .unwrap()
        .unwrap();
    assert!(execution.original_sdk().unwrap().is_none());
    let sdk = execution.register_sdk().unwrap();
    execution.validate_execution().unwrap();
    no_retry(&refresh(fixture));

    // Establish this new instance's confirmed root from actual Service pages,
    // before its FIRST collection fetch. Never reconstruct root authority by id.
    let fetch = fixture
        .workflow
        .begin_fetch(sdk.instance_id.clone(), vec!["meguri-root".into()])
        .unwrap();
    let group = format!("asset-reopen-root-{fetch}");
    fixture
        .workflow
        .will_fetch(sdk.instance_id.clone(), group.clone(), true)
        .unwrap();
    fetch_zone_pages(fixture, &sdk, "meguri-root", &mut None, "");
    fixture
        .workflow
        .did_fetch(sdk.instance_id.clone(), group, true)
        .unwrap();
    assert!(fixture.workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
    no_retry(&refresh(fixture));
    sdk
}

#[test]
fn five_failed_drain_retries_then_actual_reopen_first_drain_merges_and_service_acks_local_edit() {
    let (fixture, sdk) = published();
    let (id, original) = add_note(&fixture, "Base back".into());
    let baseline = save_pending_note(&fixture, &sdk, &original.name);
    let acked = note_facts(&fixture, id);
    assert!(acked.acked_payload.as_ref() == Some(&original.payload));
    assert_eq!(
        acked.acked_payload_digest,
        codec::sha256(&original.payload.encode().unwrap())
    );
    assert_eq!(acked.acked_system_fields, baseline.system_fields.bytes());
    assert_eq!(acked.acked_rev, original.local_rev);
    assert!(acked.mark.is_none() && acked.in_flight.is_empty());

    let collection = fixture.workflow.host.collection().unwrap();
    let local_front = "local".repeat(160_000);
    let remote_back = "remote".repeat(160_000);
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
    assert!(local.mark.is_some());
    assert!(local.base_payload.as_ref() == Some(&original.payload));
    assert!(local.in_flight.is_empty());
    let local_fields = format!("{local_front}\u{1f}Base back").into_bytes();
    assert_eq!(physical_fields(&fixture, id), local_fields);

    let codec::Payload::Note(mut remote_note) = original.payload.clone() else {
        panic!("note");
    };
    remote_note.flds = format!("Question\u{1f}{remote_back}").into_bytes();
    let remote_payload = codec::Payload::Note(remote_note);
    let remote_bytes = remote_payload.encode().unwrap();
    let value = publication_control_record(&baseline).unwrap();
    let codec::DecodeOutcome::Supported(base_entity) = codec::Entity::decode(&value.encrypted.unwrap()).unwrap() else {
        panic!("actual Service Saved base");
    };
    let mut fields = base_entity.fields;
    fields.writer = [84; 16];
    fields.erev += 1;
    fields.base_sha256 = Some(acked.acked_payload_digest);
    let remote = codec::Entity::build(fields, &remote_bytes).unwrap();
    assert!(remote.payload.is_none(), "ordinary large payload must require pa");
    let mut input = RecordInput::encrypted(&original.name, "MGEntity", remote.encode().unwrap());
    input
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(remote_bytes.clone())));
    let server = save(&fixture, &sdk, input, Some(baseline.system_fields.bytes().to_vec()));
    assert_ne!(server.system_fields.bytes(), baseline.system_fields.bytes());

    let unrelated: Vec<_> = fixture
        .workflow
        .core
        .adapter_snapshot()
        .unwrap()
        .entities
        .into_iter()
        .filter(|row| row.name != original.name)
        .collect();
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap();
    let path = fixture._directory.path().join("collection/collection.anki2");
    let native_file = r::FileIdentityProvider::identity(&NativeFiles, &path).unwrap();
    let mut token = None;
    let mut first_entry = None;
    let queue = Queue::new(true);
    no_retry(&refresh(&fixture));
    for attempt in 1..=5 {
        let issued = issued_retry(collection_drain(&fixture, &sdk, &mut token, &server.name));
        let before = entry(&fixture, &server.name);
        if first_entry.is_none() {
            assert_eq!(before.entry.reason, c::InboxReason::Asset);
            assert_eq!(before.entry.source, c::InboxSource::Fetch);
            assert_eq!(before.entry.retry_count, 0);
            let c::InboxValue::Record {
                erev,
                envelope,
                system_fields,
                staged_asset,
            } = &before.entry.value
            else {
                panic!("fetched asset entry");
            };
            assert_eq!(*erev, remote.fields.erev);
            assert!(envelope.0 == remote.encode().unwrap());
            assert_eq!(system_fields.0, server.system_fields.bytes());
            assert!(staged_asset.is_none());
            first_entry = Some(before.clone());
        }
        assert_eq!(before.entry.retry_count, attempt - 1);
        let pass = fixture
            .policy
            .clone()
            .prepare_execution(issued.handle)
            .unwrap()
            .unwrap()
            .begin_asset_read_pass()
            .unwrap();
        let work = pass.clone().next().unwrap().unwrap();
        assert_eq!(work.name, server.name);
        assert_eq!(work.zone, sdk.collection_zone);
        assert_eq!(work.kind, CloudAssetReadKind::Retry);
        assert!(pass.submit(work.handle.clone(), queue.clone()).unwrap());
        assert_eq!(attempts(&fixture, &server.name), attempt);
        // Actual all-fields Service read, with failure confined to native SDK
        // materialization. The same remote version remains on the Service.
        let download = tempfile::tempdir().unwrap();
        let readback = read(&fixture, &sdk, &work.name);
        assert_eq!(readback.system_fields.bytes(), server.system_fields.bytes());
        let callback = callback_record(&readback, &work.zone, download.path());
        std::fs::remove_file(callback.asset_path.as_ref().unwrap()).unwrap();
        assert!(
            !pass
                .complete(work.handle.clone(), Some(callback))
                .unwrap()
                .intake_accepted
        );
        assert!(
            !fixture
                .workflow
                .core
                .asset_read_owned_for_testing(work.handle.retained_for_test())
        );
        assert!(
            pass.clone().next().unwrap().is_none(),
            "no second read of the entry in this pass"
        );
        pass.finish().unwrap();
        let mut after = entry(&fixture, &server.name);
        assert_eq!(after.entry.retry_count, attempt);
        after.entry.retry_count = 0;
        assert!(
            after == *first_entry.as_ref().unwrap(),
            "failed read retains the original inbox id and version"
        );
        same_pending_note(&note_facts(&fixture, id), &local);
        assert_eq!(physical_fields(&fixture, id), local_fields);
        assert!(fixture.workflow.core.incoming_wal_receipts().is_empty());
        no_retry(&refresh(&fixture));
        no_retry(&refresh(&fixture));
    }
    assert_eq!(queue.calls.load(Ordering::SeqCst), 5);
    assert!(
        fixture
            .policy
            .status()
            .unwrap()
            .attention
            .iter()
            .any(|attention| attention.kind == CloudPolicyAttention::AssetUnavailable && attention.count == 1)
    );
    // A sixth actual collection drain cannot admit a sixth read in this open.
    // Some policies emit an empty retry action; selection must still be capped.
    let capped = collection_drain(&fixture, &sdk, &mut token, &server.name);
    let mut capped_actions = 0;
    for command in capped
        .into_iter()
        .filter(|command| matches!(command.action, CloudPolicyAction::RetryAssetEntries))
    {
        capped_actions += 1;
        let pass = fixture
            .policy
            .clone()
            .prepare_execution(command.handle)
            .unwrap()
            .unwrap()
            .begin_asset_read_pass()
            .unwrap();
        assert!(pass.clone().next().unwrap().is_none());
        pass.finish().unwrap();
    }
    assert!(capped_actions <= 1);
    assert_eq!(attempts(&fixture, &server.name), 5);
    assert_eq!(queue.calls.load(Ordering::SeqCst), 5);
    same_pending_note(&note_facts(&fixture, id), &local);
    no_retry(&refresh(&fixture));
    drop(collection);

    let fixture = reopen_host(fixture);
    assert!(fixture.workflow.validate_sdk(sdk.instance_id.clone()).is_err());
    assert_eq!(fixture.workflow.core.status().unwrap().binding, Some(binding.clone()));
    assert_eq!(
        r::FileIdentityProvider::identity(&NativeFiles, &path).unwrap(),
        native_file
    );
    assert!(entry(&fixture, &server.name) == *first_entry.as_ref().unwrap());
    assert_eq!(attempts(&fixture, &server.name), 0);
    same_pending_note(&note_facts(&fixture, id), &local);
    assert_eq!(physical_fields(&fixture, id), local_fields);
    let reopened_sdk = start_reopened(&fixture);
    assert_ne!(reopened_sdk.instance_id, sdk.instance_id);
    assert_eq!(reopened_sdk.collection_zone, sdk.collection_zone);
    // This is the first collection drain of the new host/SDK, using a fresh
    // Service cursor. Its duplicate erev must not replace/reset the old entry.
    let issued = issued_retry(collection_drain(&fixture, &reopened_sdk, &mut None, &server.name));
    assert_eq!(attempts(&fixture, &server.name), 0);
    assert!(entry(&fixture, &server.name) == *first_entry.as_ref().unwrap());
    let pass = fixture
        .policy
        .clone()
        .prepare_execution(issued.handle)
        .unwrap()
        .unwrap()
        .begin_asset_read_pass()
        .unwrap();
    let work = pass.clone().next().unwrap().unwrap();
    assert_eq!(work.kind, CloudAssetReadKind::Retry);
    assert_eq!(work.name, server.name);
    assert_eq!(work.zone, reopened_sdk.collection_zone);
    let accepted = Queue::new(true);
    assert!(pass.submit(work.handle.clone(), accepted.clone()).unwrap());
    assert_eq!(attempts(&fixture, &server.name), 1);
    let download = tempfile::tempdir().unwrap();
    let readback = read(&fixture, &reopened_sdk, &work.name);
    assert_eq!(readback.system_fields.bytes(), server.system_fields.bytes());
    let callback = callback_record(&readback, &work.zone, download.path());
    let source = callback.asset_path.clone().unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), remote_bytes);
    assert!(
        pass.complete(work.handle.clone(), Some(callback))
            .unwrap()
            .intake_accepted
    );
    assert!(
        std::path::Path::new(&source).exists(),
        "SDK file retained through callback return"
    );
    assert!(pass.clone().next().unwrap().is_none());
    pass.finish().unwrap();
    assert_eq!(accepted.calls.load(Ordering::SeqCst), 1);
    assert!(
        !fixture
            .workflow
            .core
            .asset_read_owned_for_testing(work.handle.retained_for_test())
    );
    assert!(
        !fixture
            .workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&server.name)
    );

    let merged_fields = format!("{local_front}\u{1f}{remote_back}").into_bytes();
    assert_eq!(physical_fields(&fixture, id), merged_fields);
    let merged = note_facts(&fixture, id);
    let codec::Payload::Note(payload) = &merged.payload else {
        panic!("merged note");
    };
    assert_eq!(payload.flds, merged_fields);
    assert!(merged.mark.is_some() && merged.in_flight.is_empty());
    assert!(merged.acked_payload.as_ref() == Some(&remote_payload));
    assert_eq!(merged.acked_payload_digest, codec::sha256(&remote_bytes));
    assert_eq!(merged.acked_system_fields, server.system_fields.bytes());
    assert_eq!(merged.server_erev, remote.fields.erev);
    assert!(matches!(
        fixture.workflow.tracking_sendables().unwrap().get(&server.name),
        Some(CloudPendingChange::Save)
    ));
    let runs = fixture.workflow.core.incoming_runs();
    let run = runs
        .iter()
        .find(|run| run.changed_keys.contains(&server.name))
        .expect("normal incoming writer op");
    assert!(run.entry_ids.contains(&first_entry.as_ref().unwrap().id));
    let collection = fixture.workflow.host.collection().unwrap();
    assert_eq!(run.generation, collection.generation().0);
    assert_eq!(run.native_file, native_file);
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    assert_eq!(snapshot.wal_identity().generation().0, run.generation);
    assert_eq!(
        snapshot.wal_identity().native_file(),
        (native_file.device, native_file.inode)
    );
    assert!(snapshot.seq().0 >= run.seq);
    let receipts = fixture.workflow.core.incoming_wal_receipts();
    assert!(
        receipts
            .iter()
            .any(|receipt| receipt.identity() == snapshot.wal_identity() && receipt.through().0 >= run.seq)
    );
    drop(snapshot);
    drop(reader);

    let saved = save_pending_note(&fixture, &reopened_sdk, &server.name);
    assert_ne!(saved.system_fields.bytes(), server.system_fields.bytes());
    let acknowledged = note_facts(&fixture, id);
    assert!(acknowledged.mark.is_none() && acknowledged.in_flight.is_empty());
    assert!(acknowledged.payload == merged.payload);
    assert!(acknowledged.acked_payload.as_ref() == Some(&merged.payload));
    let merged_bytes = merged.payload.encode().unwrap();
    assert_eq!(acknowledged.acked_payload_digest, codec::sha256(&merged_bytes));
    assert_eq!(acknowledged.acked_system_fields, saved.system_fields.bytes());
    assert_eq!(acknowledged.acked_rev, acknowledged.local_rev);
    let saved_value = publication_control_record(&read(&fixture, &reopened_sdk, &server.name)).unwrap();
    let codec::DecodeOutcome::Supported(saved_entity) = codec::Entity::decode(&saved_value.encrypted.unwrap()).unwrap()
    else {
        panic!("Service retained merged entity");
    };
    assert_eq!(saved_entity.payload_sha256, codec::sha256(&merged_bytes));
    let Field::Asset(asset) = &saved.fields["pa"] else {
        panic!("merged Service asset");
    };
    assert_eq!(asset.bytes(), merged_bytes);
    assert!(
        !fixture
            .workflow
            .tracking_sendables()
            .unwrap()
            .contains_key(&server.name)
    );
    drop(collection);

    let fixture = reopen_host(fixture);
    assert_eq!(fixture.workflow.core.status().unwrap().binding, Some(binding));
    assert_eq!(
        r::FileIdentityProvider::identity(&NativeFiles, &path).unwrap(),
        native_file
    );
    same_pending_note(&note_facts(&fixture, id), &acknowledged);
    assert_eq!(physical_fields(&fixture, id), merged_fields);
    assert!(
        !fixture
            .workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&server.name)
    );
    let final_entities = fixture.workflow.core.adapter_snapshot().unwrap().entities;
    for expected in unrelated {
        let actual = final_entities
            .iter()
            .find(|row| row.name == expected.name)
            .expect("unrelated row survives");
        same_pending_note(actual, &expected);
    }
    fixture.workflow.host.close().unwrap();
}
