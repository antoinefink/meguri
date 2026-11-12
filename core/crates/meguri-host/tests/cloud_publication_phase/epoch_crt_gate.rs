//! Test-first P6/P12/K20 admission, from real publication and its legacy-schema migration.
//! Canonical source base: df657044 (the filtered runner's Git identity is not the base).
use super::*;
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
