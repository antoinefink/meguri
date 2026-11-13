use super::*;
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{
    media::{Reply as MediaReply, Request as MediaRequest, Work},
    runtime::{CloudInputs, IdentityProvider, IdentityStatus},
    workflow::Saved,
};
use meguri_testkit::cloud::devices::{NativeFiles, PreboundDevice, options};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

#[test]
fn paused_media_name_ack_without_root_survives_open_and_rechecks_delayed_deletion() {
    rootless_ack_rechecks_delayed_deletion(true);
}

#[test]
fn qualified_false_drain_rechecks_rootless_ack_without_completing_absence() {
    rootless_ack_rechecks_delayed_deletion(false);
}

fn rootless_ack_rechecks_delayed_deletion(fetched: bool) {
    struct Account;
    impl IdentityProvider for Account {
        fn read_identity(&self, _: &meguri_sync::cloud::BuildScope) -> IdentityStatus {
            IdentityStatus::Available(meguri_sync::cloud::OpaqueBytes(b"synthetic-account-a".to_vec()))
        }
    }
    let run = tempfile::tempdir().unwrap();
    let directory = generated(run.path(), "rootless");
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(809)).unwrap();
    let files = PreboundDevice::arrange(&directory, &mut service, 809).unwrap();
    let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    let open = || {
        let mut config = options(&directory, NOW, 809);
        config.clock = clock.clone();
        config.cloud = Some(CloudInputs {
            scope: files.binding.scope.clone(),
            identity: Arc::new(Account),
            files: Arc::new(NativeFiles),
        });
        meguri_host::CoreHost::open(config).unwrap()
    };
    let saved = |record: &Record| {
        let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
            panic!("actual encrypted record")
        };
        Saved {
            name: record.name.clone(),
            envelope: envelope.clone(),
            system_fields: record.system_fields.bytes().to_vec(),
        }
    };
    let confirm = |host: &meguri_host::CoreHost, service: &mut Service<SeededRandom>| {
        let runtime = host.cloud_runtime().unwrap();
        runtime.refresh_identity().unwrap();
        runtime
            .reconcile_binding(
                &host.collection().unwrap(),
                &host.media().unwrap(),
                Duration::from_secs(30),
            )
            .unwrap();
        let (sdk, serialization, zone) = runtime.register_sdk(false).unwrap();
        assert_eq!(zone, files.collection_zone);
        assert_eq!(serialization, runtime.adapter_snapshot().unwrap().serialization);
        runtime.sdk_fetch_completed_without_root(&sdk).unwrap();
        let response = service
            .request(
                &files.scope,
                Caller::default(),
                Operation::Read {
                    zone: "meguri-root".into(),
                    name: "active".into(),
                    fields: RequestedFields::All,
                },
            )
            .unwrap();
        let Reply::Record(record) = response.result.unwrap() else {
            panic!("actual active read")
        };
        let ack = saved(&record);
        assert!(
            runtime
                .accept_root_read(&sdk, response.request_id, &ack.envelope, &ack.system_fields, false)
                .unwrap()
        );
        sdk
    };
    let read = |host: &meguri_host::CoreHost, work: &Work, service: &mut Service<SeededRandom>| {
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let name = match &work.request {
            MediaRequest::Root => "root",
            MediaRequest::Read { name } => name,
            _ => panic!("expected original single metadata work"),
        };
        runtime.validate_cloud_media_work(&collection, work).unwrap();
        let response = service
            .request(
                &files.scope,
                Caller::default(),
                Operation::Read {
                    zone: MEDIA.into(),
                    name: name.into(),
                    fields: RequestedFields::Named(vec!["e".into()]),
                },
            )
            .unwrap();
        let present = response.result.is_ok();
        let reply = match response.result {
            Ok(Reply::Record(record)) => {
                let ack = saved(&record);
                MediaReply::Record {
                    name: record.name,
                    record_type: record.record_type,
                    envelope: ack.envelope,
                    asset: None,
                }
            }
            Err(error) if error.code == ErrorCode::UnknownItem => MediaReply::UnknownItem,
            _ => panic!("unexpected actual metadata response"),
        };
        runtime
            .cloud_media_reply(&collection, &host.media().unwrap(), work, reply)
            .unwrap();
        present
    };
    let upload = |host: &meguri_host::CoreHost, sdk: &str, work: Work, service: &mut Service<SeededRandom>| {
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let MediaRequest::Upload { name, .. } = &work.request else {
            panic!("actual upload work")
        };
        runtime.queue_cloud_media_upload(sdk, &collection, &work).unwrap();
        let (id, original) = runtime
            .build_cloud_media_batch(sdk, &collection, &BTreeMap::from([(name.clone(), PendingChange::Save)]))
            .unwrap()
            .unwrap();
        assert_eq!(
            runtime.validate_cloud_media_batch(sdk, &collection, id).unwrap(),
            original
        );
        let MediaRequest::Upload { name, envelope, staged } = &original.request else {
            panic!("original upload")
        };
        let bytes = std::fs::read(staged).unwrap();
        assert_eq!(bytes, LOCAL);
        let mut record = RecordInput::encrypted(name, "MGBlob", envelope.clone());
        record.fields.insert("a".into(), Field::Asset(Asset::synthetic(bytes)));
        let response = service
            .request(
                &files.scope,
                Caller::default(),
                Operation::Modify {
                    zone: MEDIA.into(),
                    atomic: false,
                    changes: vec![Mutation::Save {
                        record,
                        if_unchanged: None,
                    }],
                },
            )
            .unwrap();
        let Reply::Modified(mut items) = response.result.unwrap() else {
            panic!("actual blob Save")
        };
        assert_eq!(items.len(), 1);
        let MutationSuccess::Saved(record) = items.remove(0).result.unwrap() else {
            panic!("actual blob Saved")
        };
        runtime.enter_sent_handler(sdk, id).unwrap();
        assert!(
            runtime
                .cloud_media_sdk_sent(sdk, &collection, id, &[saved(&record)], &[], &[], &[], &[], &[])
                .unwrap()
        );
        assert_eq!(runtime.return_sent_handler(sdk).unwrap(), id);
        runtime
            .sdk_saved_removed(sdk, id, std::slice::from_ref(&record.name))
            .unwrap();
        record
    };
    let durable = |host: &meguri_host::CoreHost| {
        let snapshot = host.cloud_runtime().unwrap().adapter_snapshot().unwrap();
        let content: serde_json::Value = serde_json::from_slice(&snapshot.tracking_content).unwrap();
        // Snapshot tuple member 4 is the persisted adapter::Durable.
        let tuple = content.as_array().filter(|v| v.len() == 6).unwrap();
        tuple[4].clone()
    };
    let fetch_collection = |host: &meguri_host::CoreHost, sdk: &str, service: &mut Service<SeededRandom>| {
        let runtime = host.cloud_runtime().unwrap();
        runtime
            .adapter_fetch_zone(sdk, files.collection_zone.clone(), true, false)
            .unwrap();
        let response = service
            .request(
                &files.scope,
                Caller::default(),
                Operation::FetchZone {
                    zone: files.collection_zone.clone(),
                    token: None,
                },
            )
            .unwrap();
        let Reply::ZoneChanges(page) = response.result.unwrap() else {
            panic!("actual collection page")
        };
        assert!(!page.more);
        let serialization = serde_json::to_vec(&serde_json::json!({
            "collection_zone": files.collection_zone,
            "collection_cursor": page.token,
        }))
        .unwrap();
        let mut records = Vec::new();
        let mut deleted = Vec::new();
        for change in page.changes {
            match change {
                RecordChange::Saved(record) if record.record_type == "MGEntity" => records.push(saved(&record)),
                RecordChange::Saved(record) => assert_eq!(record.record_type, "MGEpoch"),
                RecordChange::Deleted { name, .. } => deleted.push(name),
            }
        }
        runtime
            .adapter_fetched(sdk, &files.collection_zone, &records, &deleted)
            .unwrap();
        runtime
            .adapter_fetch_zone(sdk, files.collection_zone.clone(), false, false)
            .unwrap();
        serialization
    };
    let drain = |host: &meguri_host::CoreHost, sdk: &str, stale_sdk: &str, service: &mut Service<SeededRandom>| {
        let runtime = host.cloud_runtime().unwrap();
        let before = durable(host);
        let absence_completed = runtime.adapter_snapshot().unwrap().absence_pass_completed;
        if !fetched {
            assert!(runtime.adapter_snapshot().unwrap().serialization.is_some());
            assert!(before["absence"].is_object());
            assert_eq!(before["absence"]["completed"], false);
            assert!(!absence_completed);
        }
        let id = runtime
            .adapter_begin_fetch(sdk, vec![files.collection_zone.clone()])
            .unwrap();
        runtime.adapter_will_fetch(sdk, "rootless-drain".into(), true).unwrap();
        if fetched {
            fetch_collection(host, sdk, service);
        }
        // A retired opener cannot consume the current SDK's matching boundary.
        let pending = runtime.adapter_snapshot().unwrap();
        let media = runtime.media_blobs_for_testing().unwrap();
        assert_ne!(sdk, stale_sdk);
        assert!(runtime.adapter_did_fetch(stale_sdk, "rootless-drain", true).is_err());
        assert!(runtime.adapter_snapshot().unwrap() == pending);
        assert!(runtime.media_blobs_for_testing().unwrap() == media);
        let pending_durable = durable(host);
        runtime.adapter_did_fetch(sdk, "rootless-drain", true).unwrap();
        // didFetch pairs callbacks; only successful API return commits completion.
        assert_eq!(durable(host), pending_durable);
        assert_eq!(
            runtime.adapter_snapshot().unwrap().absence_pass_completed,
            pending.absence_pass_completed
        );
        assert!(runtime.media_blobs_for_testing().unwrap() == media);
        assert!(runtime.adapter_finish_fetch(sdk, id).unwrap());
        let after = durable(host);
        let old_drains = before["drains"].as_array().unwrap();
        let new_drains = after["drains"].as_array().unwrap();
        assert_eq!(new_drains.len(), old_drains.len() + 1);
        assert_eq!(&new_drains[..old_drains.len()], old_drains.as_slice());
        assert_eq!(
            new_drains.last().unwrap(),
            &serde_json::json!({
                "zone": files.collection_zone, "fetched": fetched,
            })
        );
        if !fetched {
            assert_eq!(after["absence"], before["absence"]);
            assert_eq!(after["absence_sequence"], before["absence_sequence"]);
            assert_eq!(
                runtime.adapter_snapshot().unwrap().absence_pass_completed,
                absence_completed
            );
        } else {
            assert!(runtime.adapter_snapshot().unwrap().absence_pass_completed);
        }
    };
    let entity_name;
    let stale_sdk;
    {
        let host = open();
        let sdk = confirm(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        if !fetched {
            // No-state calls with no zone callback cannot qualify a false drain.
            assert!(runtime.adapter_snapshot().unwrap().serialization.is_none());
            let before = runtime.adapter_snapshot().unwrap();
            let id = runtime
                .adapter_begin_fetch(&sdk, vec![files.collection_zone.clone()])
                .unwrap();
            runtime
                .adapter_will_fetch(&sdk, "rootless-no-state".into(), true)
                .unwrap();
            runtime.adapter_did_fetch(&sdk, "rootless-no-state", true).unwrap();
            assert!(!runtime.adapter_finish_fetch(&sdk, id).unwrap());
            assert!(runtime.adapter_snapshot().unwrap() == before);
            // Persist actual service progress, but interrupt before did-fetch.
            // The incomplete absence pass must survive the next opener's false drains.
            let id = runtime
                .adapter_begin_fetch(&sdk, vec![files.collection_zone.clone()])
                .unwrap();
            runtime
                .adapter_will_fetch(&sdk, "rootless-interrupted".into(), true)
                .unwrap();
            let serialization = fetch_collection(&host, &sdk, &mut service);
            runtime.adapter_state_update(&sdk, &serialization).unwrap();
            assert!(!runtime.adapter_finish_fetch(&sdk, id).unwrap());
            assert_eq!(runtime.adapter_snapshot().unwrap().serialization, Some(serialization));
            assert_eq!(durable(&host)["absence"]["completed"], false);
        }
        // This fixture never created a cloud_publication_bindings row.
        assert!(runtime.publication_media_state().unwrap().is_none());
        runtime.pause_cloud_media();
        let media = host.media().unwrap();
        assert_eq!(media.write_exact("photo.jpg", LOCAL).unwrap(), "photo.jpg");
        runtime.consume_media_feed(&host.collection().unwrap(), &media).unwrap();
        assert!(host.next_cloud_media_work().unwrap().is_none());
        entity_name =
            codec::entity_name(&files.binding.facts.record_key.0, codec::Kind::MediaName, b"photo.jpg").unwrap();
        let collection = host.collection().unwrap();
        let batch = runtime
            .build_collection_batch(&sdk, &collection, &BTreeSet::from([entity_name.clone()]))
            .unwrap();
        assert_eq!(batch.saves.len(), 1);
        assert_eq!(batch.saves[0].name, entity_name);
        let response = service
            .request(
                &files.scope,
                Caller::default(),
                Operation::Modify {
                    zone: files.collection_zone.clone(),
                    atomic: false,
                    changes: batch
                        .saves
                        .iter()
                        .map(|record| Mutation::Save {
                            record: RecordInput::encrypted(&record.name, "MGEntity", record.envelope.clone()),
                            if_unchanged: record.system_fields.clone().map(SystemFields::from_bytes),
                        })
                        .collect(),
                },
            )
            .unwrap();
        let Reply::Modified(mut items) = response.result.unwrap() else {
            panic!("actual MediaName Save")
        };
        assert_eq!(items.len(), 1);
        let MutationSuccess::Saved(record) = items.remove(0).result.unwrap() else {
            panic!("actual MediaName Saved")
        };
        runtime.enter_sent_handler(&sdk, batch.id).unwrap();
        runtime
            .sent_results(&sdk, &collection, batch.id, &[saved(&record)], &[], 1)
            .unwrap();
        assert_eq!(runtime.return_sent_handler(&sdk).unwrap(), batch.id);
        runtime
            .sdk_saved_removed(&sdk, batch.id, std::slice::from_ref(&entity_name))
            .unwrap();
        let ack = runtime.entity_facts(&entity_name).unwrap().unwrap();
        assert_eq!(ack.acked_rev, ack.local_rev);
        assert!(ack.mark.is_none() && !ack.acked_envelope.0.is_empty());
        assert!(runtime.media_blobs_for_testing().unwrap().is_empty());
        assert!(runtime.publication_media_state().unwrap().is_none());
        assert!(
            service.inspect(&files.scope).unwrap().unwrap().zones[MEDIA]
                .records
                .values()
                .all(|r| r.record_type != "MGBlob")
        );
        stale_sdk = sdk;
        host.close().unwrap();
    }
    clock.step_ms(23 * 60 * 60_000);
    let host = open();
    let sdk = confirm(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    assert!(runtime.publication_media_state().unwrap().is_none());
    assert!(runtime.media_blobs_for_testing().unwrap().is_empty());
    assert!(runtime.entity_facts(&entity_name).unwrap().unwrap().mark.is_none());
    runtime.resume_cloud_media();
    let root = host.next_cloud_media_work().unwrap().unwrap();
    assert!(matches!(root.request, MediaRequest::Root));
    assert!(read(&host, &root, &mut service));
    let (key, acked, _) = runtime.publication_media_state().unwrap().unwrap();
    assert!(acked);
    let states = runtime.media_blobs_for_testing().unwrap();
    assert_eq!(states.len(), 1);
    let (blob, state) = states.into_iter().next().unwrap();
    assert_eq!(state.metadata.name(&key.0), blob);
    assert_eq!(state.server, ServerState::Unknown);
    // The persisted instant is the original collection ACK, not discovery time.
    assert_eq!(state.recheck_at, Some(NOW));
    assert!(!state.rechecked);
    let work = host.next_cloud_media_work().unwrap().unwrap();
    assert!(matches!(&work.request, MediaRequest::Read { name } if name == &blob));
    assert!(!read(&host, &work, &mut service));
    let original = upload(
        &host,
        &sdk,
        host.next_cloud_media_work().unwrap().unwrap(),
        &mut service,
    );
    assert_eq!(
        runtime.media_blobs_for_testing().unwrap()[&blob].server,
        ServerState::Present
    );
    drain(&host, &sdk, &stale_sdk, &mut service);
    // Only durable inspections above and below precede the next policy/media call.
    let state = &runtime.media_blobs_for_testing().unwrap()[&blob];
    assert_eq!(state.server, ServerState::Unknown);
    assert!(state.rechecked && state.recheck_at == Some(NOW));
    runtime.resume_cloud_media();
    assert!(read(
        &host,
        &host.next_cloud_media_work().unwrap().unwrap(),
        &mut service
    ));
    assert!(read(
        &host,
        &host.next_cloud_media_work().unwrap().unwrap(),
        &mut service
    ));
    // A real server deletion lands after the first ACK drain and metadata read.
    let response = service
        .request(
            &files.scope,
            Caller::default(),
            Operation::Modify {
                zone: MEDIA.into(),
                atomic: false,
                changes: vec![Mutation::Delete { name: blob.clone() }],
            },
        )
        .unwrap();
    let Reply::Modified(items) = response.result.unwrap() else {
        panic!("actual delayed deletion")
    };
    assert!(matches!(items[0].result, Ok(MutationSuccess::Deleted)));
    clock.step_ms(60 * 60_000 - 1);
    drain(&host, &sdk, &stale_sdk, &mut service);
    assert_eq!(
        runtime.media_blobs_for_testing().unwrap()[&blob].server,
        ServerState::Present
    );
    clock.step_ms(1);
    drain(&host, &sdk, &stale_sdk, &mut service);
    let state = &runtime.media_blobs_for_testing().unwrap()[&blob];
    assert_eq!(state.server, ServerState::Unknown);
    assert_eq!(state.recheck_at, None);
    runtime.resume_cloud_media();
    assert!(read(
        &host,
        &host.next_cloud_media_work().unwrap().unwrap(),
        &mut service
    ));
    assert!(!read(
        &host,
        &host.next_cloud_media_work().unwrap().unwrap(),
        &mut service
    ));
    let restored = upload(
        &host,
        &sdk,
        host.next_cloud_media_work().unwrap().unwrap(),
        &mut service,
    );
    assert_eq!(restored.name, original.name);
    assert!(matches!(&restored.fields["a"], Field::Asset(asset) if asset.bytes() == LOCAL));
    assert_eq!(
        runtime.media_blobs_for_testing().unwrap()[&blob].server,
        ServerState::Present
    );
    assert!(runtime.entity_facts(&entity_name).unwrap().unwrap().mark.is_none());
    host.close().unwrap();
}
