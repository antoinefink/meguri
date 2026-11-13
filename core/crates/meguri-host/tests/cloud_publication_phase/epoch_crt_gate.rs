//! Test-first P6/P12/K20 admission, from real publication and its legacy-schema migration.
//! Canonical source base: df657044 (the filtered runner's Git identity is not the base).
use super::*;
use super::media_protocol as protocol;
use meguri_storage::{ReadRows, ReaderRole};
use cloud::policy::{Attention, ChangeCause, DeviceEvent, DeviceSettings, Unavailable};

fn local_crt(host: &CoreHost) -> i64 {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().col().unwrap().created.0
}

// Change collection data through the engine's real committed/undoable writer.
// This never writes tracking, policy facts, or the qualified epoch ledger.
fn change_local_crt(host: &CoreHost, crt: i64) {
    let result = host.collection().unwrap().run(
        meguri_engine::OpSpec::named("synthetic collection creation time", meguri_engine::OpSource::UserUndoable),
        move |ctx| {
            let mut col = ctx.scope().col()?;
            col.created = meguri_model::time::EpochSecs(crt);
            ctx.scope().put_col(&col)?;
            Ok(())
        },
    ).unwrap();
    assert!(result.changes.collection_row);
    assert_eq!(local_crt(host), crt);
}

fn refresh(host: &CoreHost) -> cloud::policy::State {
    host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
    host.cloud_policy_driver().unwrap().state()
}

fn connect_policy(host: &CoreHost, account: &str) {
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    })).unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(cloud::OpaqueBytes(account.as_bytes().to_vec())),
        origin_sdk: None,
    }).unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
}

fn published(host: &CoreHost, service: &mut Service<SeededRandom>) -> String {
    own_until_scan(host, service);
    let runtime = host.cloud_runtime().unwrap();
    connect_policy(host, &scope().account);
    runtime.reconcile_binding(&host.collection().unwrap(), &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    let (sdk, _, _) = runtime.register_sdk(false).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(local_crt(host)));
    let state = refresh(host);
    assert!(state.gate_open() && state.engine_runs(), "actual publication admission: {state:#?}");
    sdk
}

fn assert_mismatch(host: &CoreHost, sdk: &str, mismatch: bool) {
    let runtime = host.cloud_runtime().unwrap();
    runtime.validate_sdk(sdk).unwrap();
    let state = refresh(host);
    let binding = state.facts.binding.as_ref().unwrap();
    assert_eq!(binding.state, cloud::BindingState::Active);
    assert!(binding.usable && binding.verified && state.engine_runs());
    assert_eq!(state.facts.summary.crt_mismatch, mismatch);
    assert_eq!(state.status().attention.contains_key(&Attention::CrtMismatch), mismatch);
    assert!(!state.facts.summary.attention_known);
    assert_eq!(state.sync_now_availability(), if mismatch { Err(Unavailable::RecoveryRequired) } else { Ok(()) });
    assert_eq!(state.gate_open(), !mismatch);
}

#[test]
fn epoch_crt_gate_local_commit_blocks_next_barrier_and_real_undo_reopens_it() {
    let (_temp, _dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let sdk = published(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let crt = local_crt(&host);
    let binding = runtime.status().unwrap().binding.unwrap().id;
    let baseline = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert!(baseline.id > 0 && !baseline.saves.is_empty());
    assert!(send_actual_batch_with_instance(&host, &mut service, &baseline, sdk.clone()).is_empty());

    // Create one dirty deck to ensure a nonempty queue.
    let deck = meguri_engine::decks::add(&collection, "Synthetic epoch gate work".into()).unwrap().value.id;
    let name = runtime.entity_facts_page(cloud::entity_codec::Kind::Deck, None).unwrap().into_iter()
        .find(|e| e.key == cloud::entity_codec::id_key(deck.0)).unwrap().name;
    let names = BTreeSet::from([name]);
    change_local_crt(&host, crt + 1);
    runtime.validate_sdk(&sdk).unwrap();
    let blocked = runtime.build_collection_batch(&sdk, &collection, &names).unwrap();
    assert_eq!(blocked.id, 0, "P6: the barrier must reject known unequal creation times");
    assert!(blocked.saves.is_empty());
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(crt));
    assert_mismatch(&host, &sdk, true);
    assert_eq!(runtime.status().unwrap().binding.unwrap().id, binding);

    collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
    assert_eq!(local_crt(&host), crt);
    assert_mismatch(&host, &sdk, false);
    let resumed = runtime.build_collection_batch(&sdk, &collection, &names).unwrap();
    assert!(resumed.id > 0 && !resumed.saves.is_empty());
    assert!(send_actual_batch_with_instance(&host, &mut service, &resumed, sdk).is_empty());
    assert_eq!(runtime.status().unwrap().binding.unwrap().id, binding);
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_fetched_known_mismatch_projects_attention_and_equal_fetch_clears_it() {
    let (_temp, _dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let sdk = published(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let crt = local_crt(&host);
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let changed = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt + 1);
    runtime.adapter_fetched(&sdk, &zone, &[changed], &[]).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(crt + 1));
    assert_mismatch(&host, &sdk, true);
    let blocked = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert_eq!(blocked.id, 0);
    assert!(blocked.saves.is_empty());

    let equal = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt);
    runtime.adapter_fetched(&sdk, &zone, &[equal], &[]).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(crt));
    assert_mismatch(&host, &sdk, false);
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert!(batch.id > 0 && !batch.saves.is_empty());
    assert!(send_actual_batch_with_instance(&host, &mut service, &batch, sdk).is_empty());
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_legacy_unknown_blocks_send_until_actual_complete_epoch_is_accepted() {
    use meguri_storage::raw::PlainDatabase;

    let (_temp, dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let binding = {
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let request = until_active(&host, &mut service);
        let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
        assert!(matches!(&result, OwnResult::Saved(_)));
        runtime.publication_result(&request, result, &collection).unwrap();
        assert!(runtime.publication_op().unwrap().is_none());
        assert!(matches!(runtime.next_publication_work(&collection).unwrap(), PublicationWork::Idle));
        assert!(runtime.sdk_root_confirmed());
        assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(local_crt(&host)));
        runtime.status().unwrap().binding.unwrap()
    };
    host.close().unwrap();
    drop(host);

    // Recreate publication V2 only after every live Host owner has closed.
    // Keep the account decision, binding, epoch and actual Saved receipts intact.
    cloud::runtime::with_tracking_access(|| {
        let db = PlainDatabase::open(&dir.join(cloud::TRACKING_FILE)).unwrap();
        db.execute_batch("DROP TABLE cloud_epoch_crt; UPDATE cloud_publication_schema SET version=2;").unwrap();
    });
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    connect_policy(&host, &scope().account);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), binding);
    let root = service.request(&scope(), Caller::default(), Operation::FetchZone {
        zone: "meguri-root".into(), token: None,
    }).unwrap();
    let Ok(Reply::ZoneChanges(page)) = root.result else { panic!("actual root fetch required"); };
    assert!(!page.more);
    let active = page.changes.into_iter().find_map(|change| match change {
        meguri_testkit::cloud::RecordChange::Saved(record) if record.name == "active" => {
            let control = publication_control_record(&record).unwrap();
            Some(Saved { name: record.name, envelope: control.encrypted.unwrap(), system_fields: record.system_fields.bytes().to_vec() })
        }
        _ => None,
    }).unwrap();
    assert!(runtime.accept_root_fetch(&runtime.instance().unwrap(), Some(&active), true, root.request_id).unwrap());
    let (sdk, _, zone) = runtime.register_sdk(false).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), None);
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: zone.clone(), name: "epoch".into(), fields: RequestedFields::All,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual historical epoch required"); };
    let control = publication_control_record(&record).unwrap();
    let DecodeOutcome::Supported(epoch) = Epoch::decode(control.encrypted.as_deref().unwrap()).unwrap() else {
        panic!("supported historical epoch required");
    };
    assert_eq!(epoch.state, EpochState::Complete);
    assert_eq!(epoch.crt, local_crt(&host), "a matching local fallback grants no authority");
    let state = refresh(&host);
    assert!(state.engine_allowed() && state.engine_runs() && state.root_confirmed);
    assert!(state.facts.summary.collection_known);
    assert_eq!(state.facts.summary.scan, cloud::policy::Scan::None);
    assert_eq!(state.facts.summary.unsettled_intents, 0);
    assert!(state.facts.binding.as_ref().unwrap().usable);
    assert!(!state.facts.summary.crt_mismatch && !state.facts.summary.attention_known);
    assert!(!state.status().attention.contains_key(&Attention::CrtMismatch));
    assert_eq!(state.sync_now_availability(), Ok(()), "unknown alone is not recoveryRequired");
    let entity = runtime.entity_facts_page(cloud::entity_codec::Kind::Note, None).unwrap().remove(0);
    let cloud::entity_codec::Payload::Note(note) = &entity.payload else { panic!("synthetic published note required"); };
    meguri_engine::notes::update(&collection, meguri_engine::notes::UpdateNote {
        id: meguri_model::ids::NoteId(note.id),
        fields: vec!["Synthetic legacy edit".into(), "Synthetic answer".into()],
        tags: Vec::new(),
    }, meguri_engine::OpSource::UserUndoable).unwrap();
    let names = BTreeSet::from([entity.name.clone()]);
    assert!(runtime.entity_facts(&entity.name).unwrap().unwrap().mark.is_some());
    runtime.validate_sdk(&sdk).unwrap();
    let blocked = runtime.build_collection_batch(&sdk, &collection, &names).unwrap();
    assert_eq!(blocked.id, 0, "P6: absent legacy authority cannot equal local crt");
    assert!(blocked.saves.is_empty());
    assert!(!refresh(&host).gate_open());

    // The original real Service record enters through production SDK intake.
    let accepted = Saved { name: record.name, envelope: control.encrypted.unwrap(), system_fields: record.system_fields.bytes().to_vec() };
    runtime.adapter_fetched(&sdk, &zone, &[accepted], &[]).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(epoch.crt));
    assert_mismatch(&host, &sdk, false);
    let admitted = runtime.build_collection_batch(&sdk, &collection, &names).unwrap();
    assert!(admitted.id > 0 && !admitted.saves.is_empty());
    assert_eq!(admitted.saves[0].name, entity.name);
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_held_snapshot_cannot_send_after_both_current_values_change() {
    let (_temp, _dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let sdk = published(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let crt = local_crt(&host);
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let epoch = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt + 1);
    let writer = collection.clone();
    let caller = runtime.instance().unwrap();
    runtime.set_cloud_capture_hook_for_test(move |runtime| {
        writer.run(meguri_engine::OpSpec::named("synthetic creation time after capture", meguri_engine::OpSource::UserUndoable), move |ctx| {
            let mut col = ctx.scope().col()?;
            col.created = meguri_model::time::EpochSecs(crt + 1);
            ctx.scope().put_col(&col)?;
            Ok(())
        }).unwrap();
        runtime.receive_collection_records(&caller, &[epoch]).unwrap();
    });
    let names = entity_names(runtime);
    let blocked = runtime.build_collection_batch(&sdk, &collection, &names).unwrap();
    assert_eq!(blocked.id, 0, "the held snapshot still has the old crt");
    assert!(blocked.saves.is_empty());
    assert_mismatch(&host, &sdk, false);
    let fresh = runtime.build_collection_batch(&sdk, &collection, &names).unwrap();
    assert!(fresh.id > 0 && !fresh.saves.is_empty());
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_scan_and_reopen_rederive_mismatch_from_durable_values() {
    let (_temp, dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let sdk = published(&host, &mut service);
    let crt = local_crt(&host);
    change_local_crt(&host, crt + 1);
    let runtime = host.cloud_runtime().unwrap();
    runtime.reconcile_binding(&host.collection().unwrap(), &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert_mismatch(&host, &sdk, true);
    assert!(!runtime.status().unwrap().tracking_stopped);
    host.close().unwrap();
    drop(host);
    let host = open_host(&dir, keys());
    connect_policy(&host, &scope().account);
    let state = refresh(&host);
    assert!(state.facts.summary.crt_mismatch, "open must compare before the scan");
    let runtime = host.cloud_runtime().unwrap();
    runtime.reconcile_binding(&host.collection().unwrap(), &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    let (sdk, _, _) = runtime.register_sdk(false).unwrap();
    assert_mismatch(&host, &sdk, true);
    change_local_crt(&host, crt);
    assert_mismatch(&host, &sdk, false);
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_prepared_incoming_is_rechecked_on_the_actual_writer() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (_temp, _dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let sdk = published(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let crt = local_crt(&host);
    let key = b"synthetic-epoch-writer".to_vec();
    let payload = cloud::entity_codec::Payload::Config(cloud::entity_codec::Config {
        key: key.clone(), mtime_secs: NOW / 1000, val: b"true".to_vec(),
    });
    let binding = runtime.status().unwrap().binding.unwrap();
    let name = cloud::entity_codec::entity_name(&binding.facts.record_key.0, payload.kind(), &key).unwrap();
    let record = Saved {
        name: name.clone(),
        envelope: cloud::entity_codec::Entity::build(cloud::entity_codec::EntityFields {
            kind: payload.kind(), key: key.clone(), origin: [21; 16], erev: 1, generation: None,
            tombstone: false, deleted_at_ms: 0, writer: [22; 16], base_sha256: None,
        }, &payload.encode().unwrap()).unwrap().encode().unwrap(),
        system_fields: b"synthetic-epoch-writer-receipt".to_vec(),
    };
    runtime.receive_collection_records(&runtime.instance().unwrap(), &[record]).unwrap();
    let reached = Arc::new(AtomicBool::new(false));
    let observed = reached.clone();
    let writer = collection.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == cloud::testing::IncomingStage::ApplyBeforeWriter && !observed.swap(true, Ordering::SeqCst) {
            writer.run(meguri_engine::OpSpec::named("synthetic creation time after selection", meguri_engine::OpSource::UserUndoable), move |ctx| {
                let mut col = ctx.scope().col()?;
                col.created = meguri_model::time::EpochSecs(crt + 1);
                ctx.scope().put_col(&col)?;
                Ok(())
            }).unwrap();
        }
    });
    assert_eq!(runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap(), 0);
    assert!(reached.load(Ordering::SeqCst));
    assert!(collection.reader(ReaderRole::Query).unwrap().row_image(meguri_storage::Table::Config, &meguri_storage::RowKey::Text(key.clone())).unwrap().is_none());
    let inbox = runtime.inbox_entries(binding.id).unwrap();
    assert!(inbox.iter().any(|entry| entry.entry.record_name == name && entry.entry.reason == cloud::InboxReason::Ready && entry.entry.retry_count == 0));
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert_mismatch(&host, &sdk, true);
    collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
    assert_mismatch(&host, &sdk, false);
    assert!(runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap() > 0);
    assert!(collection.reader(ReaderRole::Query).unwrap().row_image(meguri_storage::Table::Config, &meguri_storage::RowKey::Text(key)).unwrap().is_some());
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_already_issued_native_send_is_rechecked_without_policy_refresh() {
    use cloud::policy::{ActionKind, NativeDisposition};
    let (_temp, _dir, host, mut service) = super::epoch_crt_knowledge::setup();
    let sdk = published(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let crt = local_crt(&host);
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: true, media: false,
    })).unwrap();
    let commands = host.cloud_policy_input(DeviceEvent::Foreground).unwrap();
    let send = commands.into_iter().find(|command| matches!(command.action.kind, ActionKind::RequestSend { .. })).expect("published dirty rows issue a send");
    let driver = host.cloud_policy_driver().unwrap();
    assert_eq!(driver.native_disposition(&send).unwrap(), NativeDisposition::Enqueue);
    driver.validate_native_sdk(&send, &sdk).unwrap();
    change_local_crt(&host, crt + 1);
    // Driver has not consumed Refresh; compact Runtime authority must close
    // both the prepared enqueue and final SDK validation independently.
    assert!(driver.native_disposition(&send).is_err());
    assert!(driver.validate_native_sdk(&send, &sdk).is_err());
    runtime.validate_sdk(&sdk).unwrap();
    assert!(!runtime.status().unwrap().tracking_stopped);
    host.collection().unwrap().undo(meguri_engine::undo::UndoScope::Global).unwrap();
    assert_eq!(driver.native_disposition(&send).unwrap(), NativeDisposition::Enqueue);
    driver.validate_native_sdk(&send, &sdk).unwrap();
    host.close().unwrap();
}

fn complete_publication() -> (tempfile::TempDir, CoreHost, Service<SeededRandom>, String) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("p6-repair");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(1147)).unwrap();
    publish_actual_collection(&host, &mut service);
    connect_policy(&host, &scope().account);
    let (sdk, _, _) = host.cloud_runtime().unwrap().register_sdk(false).unwrap();
    (temp, host, service, sdk)
}

fn save_entity(service: &mut Service<SeededRandom>, zone: &str, name: &str, envelope: Vec<u8>) -> Saved {
    let response = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: zone.into(), atomic: false, changes: vec![Mutation::Save {
            record: RecordInput::encrypted(name, "MGEntity", envelope.clone()), if_unchanged: None,
        }],
    }).unwrap();
    let Ok(Reply::Modified(mut items)) = response.result else { panic!("actual entity save required"); };
    let Ok(MutationSuccess::Saved(record)) = items.remove(0).result else { panic!("accepted entity required"); };
    Saved { name: record.name, envelope, system_fields: record.system_fields.bytes().to_vec() }
}

#[test]
fn epoch_crt_gate_existence_only_read_retains_verification_with_known_equal_crt() {
    let (_temp, host, mut service, sdk) = complete_publication();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let crt = local_crt(&host);
    let binding = runtime.status().unwrap().binding.unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    assert!(runtime.sdk_root_confirmed(), "publication already confirmed the root");
    runtime.sdk_fetch_completed_without_root(&sdk).unwrap();
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: "meguri-root".into(), name: "active".into(), fields: RequestedFields::All,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual active read required"); };
    let active = publication_control_record(&record).unwrap();
    // The return value reports first confirmation; this read refreshes an
    // already-confirmed root. Observe its actual persisted proof instead.
    assert!(!runtime.accept_root_read(&sdk, response.request_id, active.encrypted.as_deref().unwrap(), &active.system_fields.0, false).unwrap());
    runtime.validate_sdk(&sdk).unwrap();
    assert!(runtime.sdk_root_confirmed());
    let status = runtime.status().unwrap();
    assert!(status.stored_usable && status.identity_verified);
    assert_eq!(status.binding.as_ref(), Some(&binding));
    let root = runtime.adapter_snapshot().unwrap().root.unwrap();
    assert_eq!(root.request_id, response.request_id);
    assert_eq!(root.epoch, binding.facts.epoch);
    assert_eq!(root.envelope.0.as_slice(), active.encrypted.as_deref().unwrap());
    assert_eq!(root.system_fields, active.system_fields);
    let deck = meguri_engine::decks::add(&collection, "Synthetic known epoch work".into()).unwrap().value.id;
    let pending = runtime.entity_facts_page(cloud::entity_codec::Kind::Deck, None).unwrap().into_iter()
        .find(|entity| entity.key == cloud::entity_codec::id_key(deck.0)).unwrap();
    assert!(pending.mark.is_some() && pending.local_rev > pending.acked_rev);
    let drains = runtime.adapter_fetch_drains();
    let fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    let mut reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert_eq!(reads.len(), 1);
    let read = reads.remove(0);
    let issued: serde_json::Value = serde_json::from_slice(&runtime.adapter_snapshot().unwrap().tracking_content).unwrap();
    assert!(!issued[4]["epoch_verification"].is_null());
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: zone.clone(), name: "epoch".into(), fields: RequestedFields::Named(Vec::new()),
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual typed epoch existence required"); };
    assert_eq!(record.record_type, "MGEpoch");
    let control = publication_control_record(&record).unwrap();
    assert!(control.encrypted.is_none());
    assert!(runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Found(control), false, None, None).unwrap().is_none());
    assert!(runtime.adapter_finish_fetch(&sdk, fetch).unwrap());
    let after: serde_json::Value = serde_json::from_slice(&runtime.adapter_snapshot().unwrap().tracking_content).unwrap();
    assert_eq!(after[4]["epoch_verification"], issued[4]["epoch_verification"]);
    assert_eq!(after[4]["absence"], issued[4]["absence"]);
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(crt));
    assert_eq!(&runtime.adapter_fetch_drains()[drains.len()..], &[cloud::adapter::FetchDrain { zone, fetched: false }]);
    runtime.validate_sdk(&sdk).unwrap();
    assert!(runtime.sdk_root_confirmed());
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert_eq!(batch.id, 0, "known equal crt cannot bypass pending verification");
    assert!(batch.saves.is_empty());
    assert_eq!(runtime.status().unwrap().binding.unwrap().facts.state, cloud::BindingState::Active);
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_existence_only_reads_leave_unknown_authority_and_writer_closed() {
    use meguri_testkit::cloud::devices::PreboundDevice;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("existence-only");
    generated(&dir);
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(1148)).unwrap();
    let files = PreboundDevice::arrange(&dir, &mut service, 1148).unwrap();
    let identity = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(files.binding.facts.account.clone()))));
    let host = open_host_with(&dir, keys(), identity, files.scope.clone());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    let (sdk, serialization, zone) = runtime.register_sdk(false).unwrap();
    assert!(serialization.is_none());
    runtime.sdk_fetch_completed_without_root(&sdk).unwrap();
    let response = service.request(&files.scope, Caller::default(), Operation::Read {
        zone: "meguri-root".into(), name: "active".into(), fields: RequestedFields::All,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual active read required"); };
    let active = publication_control_record(&record).unwrap();
    assert!(runtime.accept_root_read(&sdk, response.request_id, active.encrypted.as_deref().unwrap(), &active.system_fields.0, false).unwrap());
    assert!(!runtime.adapter_epoch_crt_known());
    meguri_engine::decks::add(&collection, "Synthetic pending existence work".into()).unwrap();

    // A real Service save supplies pending incoming work; no summary or cache
    // is seeded. Its writer must stay closed until the epoch body qualifies.
    let key = b"synthetic-existence-writer".to_vec();
    let payload = cloud::entity_codec::Payload::Config(cloud::entity_codec::Config {
        key: key.clone(), mtime_secs: NOW / 1000, val: b"true".to_vec(),
    });
    let name = cloud::entity_codec::entity_name(&files.binding.facts.record_key.0, payload.kind(), &key).unwrap();
    let envelope = cloud::entity_codec::Entity::build(cloud::entity_codec::EntityFields {
        kind: payload.kind(), key: key.clone(), origin: [23; 16], erev: 1, generation: None,
        tombstone: false, deleted_at_ms: 0, writer: [24; 16], base_sha256: None,
    }, &payload.encode().unwrap()).unwrap().encode().unwrap();
    let response = service.request(&files.scope, Caller::default(), Operation::Modify {
        zone: zone.clone(), atomic: false, changes: vec![Mutation::Save {
            record: RecordInput::encrypted(&name, "MGEntity", envelope.clone()), if_unchanged: None,
        }],
    }).unwrap();
    let Ok(Reply::Modified(mut results)) = response.result else { panic!("actual incoming save required"); };
    let Ok(MutationSuccess::Saved(record)) = results.remove(0).result else { panic!("actual incoming receipt required"); };
    runtime.receive_collection_records(&runtime.instance().unwrap(), &[Saved {
        name: record.name, envelope, system_fields: record.system_fields.bytes().to_vec(),
    }]).unwrap();

    let response = service.request(&files.scope, Caller::default(), Operation::Read {
        zone: zone.clone(), name: "epoch".into(), fields: RequestedFields::All,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual complete epoch required"); };
    let complete = publication_control_record(&record).unwrap();
    let DecodeOutcome::Supported(mut publishing) = Epoch::decode(complete.encrypted.as_deref().unwrap()).unwrap() else { panic!("supported epoch required"); };
    publishing.state = EpochState::Publishing;
    publishing.counts.clear();
    // Native-shaped typed replies differ only in their encrypted body. The
    // nil-e case comes from an actual Service read with no requested fields.
    let response = service.request(&files.scope, Caller::default(), Operation::Read {
        zone: zone.clone(), name: "epoch".into(), fields: RequestedFields::Named(Vec::new()),
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual typed epoch existence required"); };
    assert_eq!(record.record_type, "MGEpoch");
    let nil = publication_control_record(&record).unwrap();
    assert!(nil.encrypted.is_none());
    let mut unreadable = complete.clone();
    unreadable.encrypted = Some(vec![255]);
    let mut unsupported = complete.clone();
    unsupported.encrypted = Some(vec![8, 2]); // field 1: future format 2
    let mut noncomplete = complete.clone();
    noncomplete.encrypted = Some(publishing.encode().unwrap());
    let mut wrong_epoch = publishing.clone();
    wrong_epoch.state = EpochState::Complete;
    wrong_epoch.epoch_id = [99; 16];
    let mut mismatched = complete.clone();
    mismatched.encrypted = Some(wrong_epoch.encode().unwrap());
    let mut no_system_fields = complete.clone();
    no_system_fields.system_fields.0.clear();
    let drains_before = runtime.adapter_fetch_drains();
    let mut pending = serde_json::Value::Null;
    for (index, control) in [nil, unreadable, unsupported, noncomplete, mismatched, no_system_fields, complete].into_iter().enumerate() {
        let valid = index == 6;
        let fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
        let group = format!("meguri-{fetch}");
        runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
        runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
        let mut reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
        assert_eq!(reads.len(), 1, "unresolved body must be checked by a later call");
        let read = reads.remove(0);
        assert!(!read.is_active() && read.zone() == zone);
        let issued: serde_json::Value = serde_json::from_slice(&runtime.adapter_snapshot().unwrap().tracking_content).unwrap();
        if index == 0 { pending = issued[4]["epoch_verification"].clone(); }
        assert!(!pending.is_null());
        assert_eq!(issued[4]["epoch_verification"], pending);
        assert!(runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Found(control), false, None, None).unwrap().is_none());
        assert!(runtime.adapter_finish_fetch(&sdk, fetch).unwrap());
        let snapshot = runtime.adapter_snapshot().unwrap();
        let after: serde_json::Value = serde_json::from_slice(&snapshot.tracking_content).unwrap();
        assert_eq!(after[4]["epoch_verification"].is_null(), valid);
        if !valid { assert_eq!(after[4]["epoch_verification"], pending); }
        assert_eq!(after[4]["absence"], issued[4]["absence"]);
        assert!(!snapshot.absence_pass_completed);
        assert_eq!(runtime.epoch_crt_for_testing().unwrap(), valid.then_some(local_crt(&host)));
        let drains = runtime.adapter_fetch_drains();
        assert_eq!(drains.len(), drains_before.len() + index + 1);
        assert_eq!(drains.last(), Some(&cloud::adapter::FetchDrain { zone: zone.clone(), fetched: false }));
        let diagnostic = runtime.adapter_root_read_diagnostics().pop().unwrap();
        assert!(diagnostic.accepted && diagnostic.a7_failure && !diagnostic.active && diagnostic.outcome == "found");
        runtime.validate_sdk(&sdk).unwrap();
        assert_eq!(runtime.status().unwrap().binding.unwrap().facts.state, cloud::BindingState::Active);
        let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
        assert_eq!(batch.id > 0 && !batch.saves.is_empty(), valid);
        if !valid {
            assert_eq!(runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap(), 0);
            assert!(collection.reader(ReaderRole::Query).unwrap().row_image(meguri_storage::Table::Config, &meguri_storage::RowKey::Text(key.clone())).unwrap().is_none());
        } else {
            // Building the valid batch already drains incoming work. Check
            // the Service config's actual value, then prove no second apply.
            let applied = collection.reader(ReaderRole::Query).unwrap().row_image(meguri_storage::Table::Config, &meguri_storage::RowKey::Text(key.clone())).unwrap().expect("valid batch applies the Service config");
            assert_eq!(applied[0], meguri_storage::Cell::Text(key.clone()));
            assert_eq!(applied[2], meguri_storage::Cell::Integer(NOW / 1000));
            assert_eq!(applied[3], meguri_storage::Cell::Blob(b"true".to_vec()));
            assert_eq!(runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap(), 0);
            assert_eq!(collection.reader(ReaderRole::Query).unwrap().row_image(meguri_storage::Table::Config, &meguri_storage::RowKey::Text(key.clone())).unwrap(), Some(applied));
        }
    }
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_existing_epoch_read_persists_complete_unequal_body_atomically() {
    let (_temp, host, mut service, sdk) = complete_publication();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let crt = local_crt(&host);
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let response = service.request(&scope(), Caller::default(), Operation::FetchZone { zone: zone.clone(), token: None }).unwrap();
    let Ok(Reply::ZoneChanges(page)) = response.result else { panic!("actual fetch cursor required"); };
    assert!(!page.more);
    let response = service.request(&scope(), Caller::default(), Operation::FetchZone { zone: zone.clone(), token: Some(page.token) }).unwrap();
    let Ok(Reply::ZoneChanges(page)) = response.result else { panic!("actual empty fetch required"); };
    assert!(!page.more && page.changes.is_empty());
    // Start the normal cursorless fetch so its owner requests epoch metadata.
    // Fetch completion below uses the SDK callbacks and the Service response.
    let fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    let mut reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert_eq!(reads.len(), 1);
    let read = reads.remove(0);
    assert!(!read.is_active() && read.zone() == zone);
    // The server changes after the fetch; only the issued record read can
    // deliver this unequal complete body to tracking.
    let changed = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt + 1);
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: zone.clone(), name: "epoch".into(), fields: RequestedFields::All,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual existing epoch read required"); };
    let control = publication_control_record(&record).unwrap();
    assert_eq!(control.encrypted.as_deref(), Some(changed.envelope.as_slice()));
    assert!(runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Found(control), false, None, None).unwrap().is_none());
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(crt + 1));
    let durable: serde_json::Value = serde_json::from_slice(&runtime.adapter_snapshot().unwrap().tracking_content).unwrap();
    assert!(durable[4]["epoch_verification"].is_null());
    assert_mismatch(&host, &sdk, true);
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert_eq!(batch.id, 0);
    assert!(batch.saves.is_empty());
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_prepared_batch_cannot_handoff_after_current_pair_changes_together() {
    let (_temp, host, mut service, sdk) = complete_publication();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    meguri_engine::decks::add(&collection, "Synthetic prepared batch work".into()).unwrap();
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert!(!batch.saves.is_empty());
    runtime.validate_prepared_batch(&sdk, &zone, batch.id).unwrap();
    let before = runtime.adapter_snapshot().unwrap();
    let crt = local_crt(&host);
    change_local_crt(&host, crt + 1);
    assert!(runtime.validate_prepared_batch(&sdk, &zone, batch.id).is_err());
    runtime.validate_sdk(&sdk).unwrap();
    runtime.validate_network_sdk(&sdk).unwrap();
    let changed = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt + 1);
    runtime.adapter_fetched(&sdk, &zone, &[changed], &[]).unwrap();
    assert_mismatch(&host, &sdk, false);
    assert!(runtime.validate_prepared_batch(&sdk, &zone, batch.id).is_err(), "the prepared snapshot still has the original crt");
    let after = runtime.adapter_snapshot().unwrap();
    assert_eq!(before.entities, after.entities, "denied handoff cannot ACK or replace original flights");
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_committed_ordinary_intent_settles_after_mismatch_without_new_writer() {
    use meguri_model::faultpoint::{self, Fault};
    use meguri_storage::{RowKey, Table};
    let (_temp, host, mut service, sdk) = complete_publication();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let key = b"synthetic-p6-committed".to_vec();
    let payload = cloud::entity_codec::Payload::Config(cloud::entity_codec::Config {
        key: key.clone(), mtime_secs: NOW / 1000, val: b"true".to_vec(),
    });
    let name = cloud::entity_codec::entity_name(&binding.facts.record_key.0, payload.kind(), &key).unwrap();
    let envelope = cloud::entity_codec::Entity::build(cloud::entity_codec::EntityFields {
        kind: payload.kind(), key: key.clone(), origin: [31; 16], erev: 1, generation: None,
        tombstone: false, deleted_at_ms: 0, writer: [32; 16], base_sha256: None,
    }, &payload.encode().unwrap()).unwrap().encode().unwrap();
    let saved = save_entity(&mut service, &zone, &name, envelope);
    runtime.adapter_fetched(&sdk, &zone, &[saved], &[]).unwrap();
    faultpoint::arm("cloud.apply.after_commit", Fault::IoError(std::io::ErrorKind::Other));
    let result = runtime.drain_incoming(&collection, Duration::from_secs(30));
    faultpoint::disarm("cloud.apply.after_commit");
    assert!(result.is_err());
    let retained = runtime.apply_intents(binding.id).unwrap();
    assert_eq!(retained.len(), 1);
    assert!(retained[0].result_seq.is_some() && !retained[0].keys.is_empty() && !retained[0].inbox_ids.is_empty());
    let row = collection.reader(ReaderRole::Query).unwrap().row_image(Table::Config, &RowKey::Text(key.clone())).unwrap().unwrap();
    change_local_crt(&host, local_crt(&host) + 1);
    // Settle local day-clock maintenance before measuring replay alone.
    collection.check_rollover().unwrap();
    let seq = collection.last_seq();
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    assert_eq!(collection.last_seq(), seq, "replay must preserve the committed sequence");
    assert_eq!(collection.reader(ReaderRole::Query).unwrap().row_image(Table::Config, &RowKey::Text(key)).unwrap(), Some(row));
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(!runtime.incoming_wal_receipts().is_empty());
    assert!(runtime.inbox_entries(binding.id).unwrap().iter().all(|entry| entry.entry.record_name != name));
    assert_mismatch(&host, &sdk, true);
    host.close().unwrap();
}

#[test]
fn epoch_crt_gate_real_download_and_collision_recheck_before_file_mutation() {
    use cloud::{entity_codec as codec, media::{blob::Blob, Request}};
    use meguri_testkit::cloud::Asset;
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicBool, Ordering};
    for collision in [false, true] {
        let (temp, host, mut service, sdk) = complete_publication();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let media = host.media().unwrap();
        host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
            enabled: true, paused: false, automatic: false, media: true,
        })).unwrap();
        runtime.resume_cloud_media();
        let binding = runtime.status().unwrap().binding.unwrap();
        let zone = cloud::adapter::collection_zone(binding.facts.epoch);
        let mut remaining = 16;
        protocol::flush_collection(&host, &mut service, &sdk, &mut remaining);
        let file = temp.path().join("p6-repair/collection.media/photo.jpg");
        if collision {
            let baseline_file = file.with_file_name("uploaded-baseline.jpg");
            std::fs::write(&baseline_file, b"synthetic local collision").unwrap();
            media.store().register_full(|_, _| true).unwrap();
            runtime.consume_media_feed(&collection, &media).unwrap();
            let (id, work) = protocol::prepare_upload(&host, &mut service, &sdk, &mut remaining);
            protocol::send_batch(&host, &mut service, &sdk, id, &work);
            protocol::flush_collection(&host, &mut service, &sdk, &mut remaining);
            let baseline = runtime.entity_facts_page(codec::Kind::MediaName, None).unwrap().remove(0);
            assert!(baseline.server_erev > 0 && baseline.mark.is_none());
            let note = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
            let codec::Payload::Note(note) = note.payload else { panic!("note required"); };
            meguri_engine::notes::update(&collection, meguri_engine::notes::UpdateNote {
                id: meguri_model::ids::NoteId(note.id), fields: vec!["<img src=\"photo.jpg\">".into(), "local".into()], tags: vec![],
            }, meguri_engine::OpSource::UserUndoable).unwrap();
            protocol::flush_collection(&host, &mut service, &sdk, &mut remaining);
            // The blob is already uploaded; this distinct name is a real local creation.
            std::fs::write(&file, b"synthetic local collision").unwrap();
            media.store().register_full(|_, _| true).unwrap();
            runtime.consume_media_feed(&collection, &media).unwrap();
            let local = runtime.entity_facts_page(codec::Kind::MediaName, None).unwrap().into_iter()
                .find(|entity| entity.key == b"photo.jpg").unwrap();
            assert_eq!(local.server_erev, 0);
            assert!(local.mark.is_some());
        }
        // Public known hash vector; Service owns both MGBlob and MGEntity.
        let bytes = b"abc";
        let blob = Blob { sha256: Sha256::digest(bytes).into(), sha1: [0xa9,0x99,0x3e,0x36,0x47,0x06,0x81,0x6a,0xba,0x3e,0x25,0x71,0x78,0x50,0xc2,0x6c,0x9c,0xd0,0xd8,0x9d], size: 3 };
        let media_key = runtime.publication_media_state().unwrap().unwrap().0;
        let blob_name = blob.name(&media_key.0);
        let mut input = RecordInput::encrypted(&blob_name, "MGBlob", blob.encode().unwrap());
        input.fields.insert("a".into(), Field::Asset(Asset::synthetic(bytes.to_vec())));
        let response = service.request(&scope(), Caller::default(), Operation::Modify {
            zone: "meguri-media".into(), atomic: false, changes: vec![Mutation::Save { record: input, if_unchanged: None }],
        }).unwrap();
        let Ok(Reply::Modified(items)) = response.result else { panic!("actual blob save required"); };
        assert!(matches!(items[0].result, Ok(MutationSuccess::Saved(_))));
        let payload = codec::Payload::MediaName(blob.media("photo.jpg"));
        let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::MediaName, b"photo.jpg").unwrap();
        let envelope = codec::Entity::build(codec::EntityFields {
            kind: codec::Kind::MediaName, key: b"photo.jpg".to_vec(), origin: [71; 16], erev: 1,
            generation: None, tombstone: false, deleted_at_ms: 0, writer: [72; 16], base_sha256: None,
        }, &payload.encode().unwrap()).unwrap().encode().unwrap();
        let saved = save_entity(&mut service, &zone, &name, envelope);
        runtime.adapter_fetched(&sdk, &zone, &[saved], &[]).unwrap();
        let crt = local_crt(&host);
        let reached = Arc::new(AtomicBool::new(false));
        let observed = reached.clone();
        let writer = collection.clone();
        let target = if collision { cloud::testing::IncomingStage::ResolutionBeforeMove } else { cloud::testing::IncomingStage::MediaReplyBeforeApply };
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == target && !observed.swap(true, Ordering::SeqCst) {
                writer.run(meguri_engine::OpSpec::named("p6 media boundary creation time", meguri_engine::OpSource::UserUndoable), move |ctx| {
                    let mut col = ctx.scope().col()?;
                    col.created = meguri_model::time::EpochSecs(crt + 1);
                    ctx.scope().put_col(&col)?;
                    Ok(())
                }).unwrap();
            }
        });
        let before_notes = runtime.entity_facts_page(codec::Kind::Note, None).unwrap();
        let mut denied = false;
        for _ in 0..16 {
            if remaining == 0 { break; }
            remaining -= 1;
            let work = runtime.cloud_media_work(&collection, &media).unwrap().expect("actual queued media work");
            if matches!(work.request, Request::Upload { .. } | Request::Delete { .. }) {
                protocol::flush_collection(&host, &mut service, &sdk, &mut remaining);
            }
            let result = protocol::deliver(&host, &mut service, &sdk, &work);
            if reached.load(Ordering::SeqCst) {
                assert!(result.is_err());
                denied = true;
                break;
            }
            result.unwrap();
        }
        assert!(denied && reached.load(Ordering::SeqCst));
        if collision {
            assert_eq!(std::fs::read(&file).unwrap(), b"synthetic local collision");
            let intents = runtime.apply_intents(binding.id).unwrap();
            assert_eq!(intents.len(), 1);
            let apply = intents[0].media.as_ref().unwrap();
            assert!(apply.moved.is_some() && apply.move_evidence.is_none() && !apply.installed);
            let moved = apply.moved.as_ref().unwrap();
            assert!(!file.parent().unwrap().join(std::str::from_utf8(&moved.name).unwrap()).exists());
        } else {
            assert!(!file.exists());
            assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
        }
        assert_eq!(runtime.entity_facts_page(codec::Kind::Note, None).unwrap(), before_notes);
        assert_mismatch(&host, &sdk, true);
        host.close().unwrap();
    }
}

#[test]
fn epoch_crt_gate_committed_media_trash_settles_after_mismatch_with_real_intent_and_wal() {
    use cloud::entity_codec::{self as codec, Kind, Payload};
    use meguri_model::faultpoint::{self, Fault};
    let (temp, host, mut service, sdk) = complete_publication();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let media = host.media().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: true,
    })).unwrap();
    runtime.resume_cloud_media();
    let file = temp.path().join("p6-repair/collection.media/retired.jpg");
    std::fs::write(&file, b"synthetic p6 committed trash").unwrap();
    media.store().register_full(|_, _| true).unwrap();
    runtime.consume_media_feed(&collection, &media).unwrap();
    let before = runtime.entity_facts_page(Kind::MediaName, None).unwrap().remove(0);
    assert!(matches!(&before.payload, Payload::MediaName(value) if value.name == b"retired.jpg"));
    let envelope = codec::Entity::build(codec::EntityFields {
        kind: Kind::MediaName, key: before.key.clone(), origin: before.origin,
        erev: before.server_erev + 1, generation: None, tombstone: true,
        deleted_at_ms: NOW, writer: [81; 16], base_sha256: None,
    }, &before.payload.encode().unwrap()).unwrap().encode().unwrap();
    let saved = save_entity(&mut service, &zone, &before.name, envelope);
    runtime.adapter_fetched(&sdk, &zone, &[saved], &[]).unwrap();
    faultpoint::arm("cloud.media.after_resolution_install", Fault::IoError(std::io::ErrorKind::Other));
    let applied = runtime.drain_incoming(&collection, Duration::from_secs(30));
    faultpoint::disarm("cloud.media.after_resolution_install");
    assert!(applied.is_err() && !file.exists());
    let pending = runtime.apply_intents(binding.id).unwrap();
    assert_eq!(pending.len(), 1);
    let apply = pending[0].media.as_ref().unwrap();
    assert!(apply.installed && !apply.abandoned && apply.server.is_none());
    assert!(!pending[0].inbox_ids.is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    change_local_crt(&host, local_crt(&host) + 1);
    let seq = collection.last_seq();
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    assert_eq!(collection.last_seq(), seq);
    assert!(!file.exists());
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(!runtime.incoming_wal_receipts().is_empty());
    assert!(runtime.inbox_entries(binding.id).unwrap().iter().all(|entry| entry.entry.record_name != before.name));
    assert_mismatch(&host, &sdk, true);
    host.close().unwrap();
}
