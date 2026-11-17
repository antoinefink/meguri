//! Real Host/Runtime composition. Baseline preparation is the existing test hook;
//! policy projection, mutations, SDK drains, faults and evidence use production paths.
#![cfg(feature = "test-hooks")]
use meguri_host::CoreHost;
use meguri_model::clock::{Clock, StepClock, UtcOffset};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, Table};
use meguri_sync::cloud::{
    BuildScope, ContainerId, Environment, OpaqueBytes,
    entity_codec::{self as codec, Payload},
    policy::{
        ActionKind, ChangeCause, Command, DeviceEvent, DeviceSettings, DriverError, ExchangeOutcome, OperationError,
        Source, StatusState,
    },
    runtime::{CloudInputs, IdentityProvider, IdentityStatus, RuntimeError},
    testing::{ApplyInterruption, IncomingStage},
    workflow::Saved,
};
use meguri_testkit::cloud::devices::{NativeFiles, options};
use std::{sync::Arc, time::Duration};

#[path = "support/epoch_crt.rs"]
mod epoch_fixture;

#[path = "support/reference_waiting.rs"]
mod reference_waiting;

struct Account;
#[test]
fn epoch_crt_legacy_runtime_keeps_unknown_attention_conservative() {
    let rig = rig_with_epoch(true, 1914, false);
    assert_eq!(rig.host.cloud_runtime().unwrap().epoch_crt_for_testing().unwrap(), None);
    assert!(!rig.host.cloud_policy_driver().unwrap().state().facts.summary.attention_known);
    rig.host.close().unwrap();
}
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec()))
    }
}
struct Rig {
    _root: tempfile::TempDir,
    config: meguri_host::HostConfig,
    host: Arc<CoreHost>,
    clock: Arc<StepClock>,
    sdk: String,
    zone: String,
    records: Vec<Saved>,
    active: Saved,
    baseline_deck: meguri_model::ids::DeckId,
}
fn rig() -> Rig {
    rig_with_root(true)
}
fn rig_with_root(confirm_root: bool) -> Rig {
    rig_with_root_seed(confirm_root, 1914)
}
fn rig_with_root_seed(confirm_root: bool, seed: u64) -> Rig {
    rig_with_epoch(confirm_root, seed, true)
}
fn rig_with_epoch(confirm_root: bool, seed: u64, known_epoch: bool) -> Rig {
    rig_with_files(confirm_root, seed, known_epoch, Arc::new(NativeFiles))
}
fn rig_with_files(
    confirm_root: bool, seed: u64, known_epoch: bool,
    files: Arc<dyn meguri_sync::cloud::runtime::FileIdentityProvider>,
) -> Rig {
    rig_with_files_and_state(confirm_root, seed, known_epoch, files, confirm_root)
}
fn rig_with_files_and_state(
    confirm_root: bool, seed: u64, known_epoch: bool,
    files: Arc<dyn meguri_sync::cloud::runtime::FileIdentityProvider>, saved_state: bool,
) -> Rig {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("collection");
    std::fs::create_dir_all(&directory).unwrap();
    let mut config = options(&directory, 1_894_701_600_000, seed);
    // HostConfig defaults to LocalOnly, which correctly closes the policy gate.
    // Arrange the same connected account as the existing synthetic baseline.
    config.publication_binding = meguri_engine::device_state::AccountBinding::Connected {
        account: "synthetic-account-A".into(),
        configuration: "synthetic-u114".into(),
        environment: "development".into(),
    };
    let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
    config.clock = clock.clone();
    meguri_engine::bootstrap::create(
        &directory.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
    config.cloud = Some(CloudInputs {
        scope: BuildScope {
            container: ContainerId::new("synthetic-u114").unwrap(),
            environment: Environment::Development,
        },
        identity: Arc::new(Account),
        files,
    });
    CoreHost::migrate_provider_configuration(
        &config.manual_transfer_directory,
        meguri_sync::provider::LegacyInput::Settings(meguri_sync::provider::LegacySettings {
            enabled: true,
            paused: false,
            automatic: false,
            media: false,
        }),
    )
    .unwrap();
    let host = Arc::new(CoreHost::open(config.clone()).unwrap());
    let collection = host.collection().unwrap();
    let baseline_deck = meguri_engine::decks::add(&collection, "Policy baseline deck".into())
        .unwrap()
        .value
        .id;
    if !confirm_root {
        let notetype = meguri_engine::notetypes::add_stock(
            &collection,
            meguri_engine::notetypes::StockKind::Basic,
            Some("First manual review".into()),
        )
        .unwrap()
        .value
        .id;
        meguri_engine::notes::add(
            &collection,
            meguri_engine::notes::AddNote {
                notetype_id: notetype,
                fields: vec!["Public question".into(), "Public answer".into()],
                tags: Vec::new(),
                deck_id: baseline_deck,
                guid: None,
                update_add_defaults: false,
            },
            meguri_engine::OpSource::UserUndoable,
        )
        .unwrap();
        meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    }
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let (records, active, _) = runtime
        .prepare_adapter_baseline_for_testing(
            &collection,
            config.clock.as_ref(),
            config.random.as_ref(),
            saved_state.then(|| b"synthetic-sdk-state".to_vec()),
        )
        .unwrap();
    host.refresh_provider_admission();
    if known_epoch {
        let mut service = meguri_testkit::cloud::Service::open(
            root.path(), meguri_testkit::cloud::Config::default(), meguri_model::rng::SeededRandom::new(seed),
        ).unwrap();
        epoch_fixture::accept(&host, &mut service, &meguri_testkit::cloud::Scope {
            container: "synthetic-u114".into(), environment: "development".into(), account: "synthetic-account-A".into(),
        });
    }
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true,
        paused: false,
        automatic: false,
        media: false,
    }))
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
        origin_sdk: None,
    })
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true })
        .unwrap();
    runtime
        .reconcile_binding(&collection, &media, Duration::from_secs(10))
        .unwrap();
    if confirm_root {
        assert!(
            runtime
                .accept_root_fetch(&runtime.instance().unwrap(), Some(&active), true, 1)
                .unwrap()
        );
    }
    let (sdk, _, zone) = runtime.register_sdk(false).unwrap();
    host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let state = host.cloud_policy_driver().unwrap().state();
    assert!(
        state.gate_open() == known_epoch && state.engine_runs(),
        "fixture admission: {state:#?}"
    );
    assert_eq!(
        state.root_confirmed, confirm_root,
        "fixture root confirmation: {state:#?}"
    );
    Rig {
        _root: root,
        config,
        host,
        clock,
        sdk,
        zone,
        records,
        active,
        baseline_deck,
    }
}
#[track_caller]
fn command(mut commands: Vec<Command>, f: impl Fn(&ActionKind) -> bool) -> Command {
    let index = commands
        .iter()
        .position(|c| f(&c.action.kind))
        .unwrap_or_else(|| panic!("requested policy action absent; actual commands: {commands:#?}"));
    commands.remove(index)
}

fn deck_row(rig: &Rig, id: i64) -> Vec<Cell> {
    let collection = rig.host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    reader.row_image(Table::Decks, &RowKey::Id(id)).unwrap().unwrap()
}

#[test]
fn cloud_scan_unsent_deck_undo_reopen_drops_only_never_handed_tracking() {
    let mut fixture = rig();
    let collection = fixture.host.collection().unwrap();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let owner = cloud.status().unwrap().binding.unwrap().id;
    let original = cloud.entity_facts_page(codec::Kind::Deck, None).unwrap();
    let baseline = original.iter().find(|entity| entity.key == codec::id_key(fixture.baseline_deck.0)).unwrap().clone();
    let created = meguri_engine::decks::add(&collection, "Local deck removed before handoff".into()).unwrap().value.id;
    let pending = cloud.entity_facts_page(codec::Kind::Deck, None).unwrap().into_iter()
        .find(|entity| entity.key == codec::id_key(created.0)).unwrap();
    let never_handed = |entity: &meguri_sync::cloud::workflow::EntityFacts| {
        assert!(!entity.sent);
        assert_eq!(entity.server_erev, 0);
        assert_eq!(entity.acked_rev, 0);
        assert!(entity.in_flight.is_empty());
        assert!(entity.acked_system_fields.is_empty());
        assert!(entity.acked_payload.is_none());
        assert!(entity.acked_envelope.0.is_empty());
    };
    never_handed(&pending);
    {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        assert!(reader.row_image(Table::Decks, &RowKey::Id(created.0)).unwrap().is_some());
    }
    collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
    {
        let current = fixture.host.collection().unwrap();
        let reader = current.reader(ReaderRole::Query).unwrap();
        assert!(reader.row_image(Table::Decks, &RowKey::Id(created.0)).unwrap().is_none());
    }
    never_handed(&cloud.entity_facts(&pending.name).unwrap().unwrap());
    assert!(matches!(cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[], &[]), Err(RuntimeError::StaleIncarnation)));
    drop(collection);
    fixture.host.close().unwrap();
    fixture.host = Arc::new(CoreHost::open(fixture.config.clone()).unwrap());
    fixture.host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    })).unwrap();
    fixture.host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())), origin_sdk: None,
    }).unwrap();
    fixture.host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
    let collection = fixture.host.collection().unwrap();
    let cloud = fixture.host.cloud_runtime().unwrap();
    assert_eq!(cloud.status().unwrap().binding.unwrap().id, owner);
    never_handed(&cloud.entity_facts(&pending.name).unwrap().unwrap());
    {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        assert!(reader.row_image(Table::Decks, &RowKey::Id(created.0)).unwrap().is_none());
    }
    cloud.reconcile_binding(&collection, &fixture.host.media().unwrap(), Duration::from_secs(10)).unwrap();
    assert!(cloud.entity_facts(&pending.name).unwrap().is_none());
    assert_eq!(cloud.entity_facts_page(codec::Kind::Deck, None).unwrap().len(), original.len());
    let retained = cloud.entity_facts(&baseline.name).unwrap().unwrap();
    assert!(retained.payload == baseline.payload);
    assert_eq!(retained.sent, baseline.sent);
    assert_eq!(retained.local_rev, baseline.local_rev);
    assert_eq!(retained.acked_rev, baseline.acked_rev);
    assert_eq!(retained.server_erev, baseline.server_erev);
    assert_eq!(retained.mark, baseline.mark);
    assert!(retained.in_flight == baseline.in_flight);
    assert!(cloud.inbox_entries(owner).unwrap().is_empty());
    assert!(cloud.apply_intents(owner).unwrap().is_empty());
    assert!(cloud.accept_root_fetch(&cloud.instance().unwrap(), Some(&fixture.active), true, 1).unwrap());
    let (sdk, _, _) = cloud.register_sdk(false).unwrap();
    cloud.validate_sdk(&sdk).unwrap();
    assert!(!cloud.tracking_sendables().contains_key(&pending.name));
    let batch = cloud.build_collection_batch(&sdk, &collection, &[pending.name.clone()].into()).unwrap();
    assert!(batch.saves.is_empty());
    assert!(cloud.entity_facts(&pending.name).unwrap().is_none());
    let projection = cloud.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap();
    assert!(!projection.facts.summary.attention.contains_key(&meguri_sync::cloud::policy::Attention::ExchangeIncomplete));
    assert!(!projection.facts.summary.attention_known);
    fixture.host.close().unwrap();
}

#[test]
fn cloud_scan_handed_deck_keeps_tracking_and_uploaded_undo_refusal_after_reopen() {
    let mut fixture = rig();
    let collection = fixture.host.collection().unwrap();
    let created = meguri_engine::decks::add(&collection, "Deck handed to the SDK".into()).unwrap().value.id;
    let cloud = fixture.host.cloud_runtime().unwrap();
    let pending = cloud.entity_facts_page(codec::Kind::Deck, None).unwrap().into_iter()
        .find(|entity| entity.key == codec::id_key(created.0)).unwrap();
    let batch = cloud.build_collection_batch(&fixture.sdk, &collection, &[pending.name.clone()].into()).unwrap();
    cloud.validate_sdk(&fixture.sdk).unwrap();
    assert_eq!(batch.saves.len(), 1);
    assert_eq!(batch.saves[0].name, pending.name);
    let handed = cloud.entity_facts(&pending.name).unwrap().unwrap();
    assert!(handed.sent);
    assert!(!handed.in_flight.is_empty());
    assert_eq!(handed.server_erev, 0);
    assert_eq!(handed.acked_rev, 0);
    assert!(matches!(collection.undo(meguri_engine::undo::UndoScope::Global),
        Err(meguri_engine::EngineError::UndoUnavailable {
            reason: meguri_engine::undo::reasons::DisabledReason::UploadPending,
        })));
    incomplete_ack_batch(&fixture, batch);
    let acked = cloud.entity_facts(&pending.name).unwrap().unwrap();
    assert!(acked.sent);
    assert!(acked.server_erev > 0);
    assert_eq!(acked.acked_rev, acked.local_rev);
    assert!(acked.in_flight.is_empty());
    assert!(matches!(collection.undo(meguri_engine::undo::UndoScope::Global),
        Err(meguri_engine::EngineError::UndoUnavailable {
            reason: meguri_engine::undo::reasons::DisabledReason::AlreadyUploaded,
        })));
    let row = deck_row(&fixture, created.0);
    drop(collection);
    incomplete_reopen(&mut fixture);
    let cloud = fixture.host.cloud_runtime().unwrap();
    let retained = cloud.entity_facts(&pending.name).unwrap().unwrap();
    assert!(retained.sent);
    assert_eq!(retained.server_erev, acked.server_erev);
    assert_eq!(retained.local_rev, acked.local_rev);
    assert_eq!(retained.acked_rev, acked.acked_rev);
    assert!(retained.in_flight.is_empty());
    assert!(retained.mark.is_none());
    assert_eq!(deck_row(&fixture, created.0), row);
    assert_eq!(incomplete_count(&fixture.host), None);
    assert!(!cloud.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready,
        fixture.host.engine().generation().0).unwrap().facts.summary.attention_known);
    fixture.host.close().unwrap();
}

fn incoming_deck(rig: &Rig) -> (i64, Vec<Cell>, Command) {
    let (saved, entity, mut deck) = rig
        .records
        .iter()
        .find_map(|saved| {
            let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&saved.envelope).unwrap() else {
                return None;
            };
            if entity.fields.kind != codec::Kind::Deck {
                return None;
            }
            let Payload::Deck(deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap()
            else {
                unreachable!()
            };
            (deck.id != 1).then_some((saved, entity, deck))
        })
        .unwrap();
    let before = deck_row(rig, deck.id);
    let id = deck.id;
    deck.name = b"Policy remote deck".to_vec();
    deck.mtime_secs += 1;
    let mut fields = entity.fields;
    fields.erev += 1;
    let remote = Saved {
        name: saved.name.clone(),
        envelope: codec::Entity::build(fields, &Payload::Deck(deck).encode().unwrap())
            .unwrap()
            .encode()
            .unwrap(),
        system_fields: b"synthetic-policy-revision-2".to_vec(),
    };
    rig.host
        .cloud_runtime()
        .unwrap()
        .adapter_fetched(&rig.sdk, &rig.zone, &[remote], &[])
        .unwrap();
    let apply = command(
        rig.host
            .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
            .unwrap(),
        |kind| matches!(kind, ActionKind::ApplyReady { .. }),
    );
    (id, before, apply)
}

// Baseline-only regression: every observation below uses an existing public seam.
#[test]
fn exchange_incomplete_real_rename_produces_attention() {
    let rig = rig();
    meguri_engine::decks::rename(
        &rig.host.collection().unwrap(), rig.baseline_deck, "Incomplete selected deck".into(),
    ).unwrap();
    let action = incomplete_end(&rig);
    rig.host.execute_cloud_policy_core(&action, Duration::from_secs(10)).unwrap();
    assert_eq!(incomplete_count(&rig.host), Some(1));
    assert!(!rig.host.cloud_policy_driver().unwrap().state().facts.summary.attention_known);
    rig.host.close().unwrap();
}

fn incomplete_count(host: &CoreHost) -> Option<u64> {
    let collection = host.collection().unwrap();
    host.cloud_runtime().unwrap().policy_projection(
        meguri_sync::cloud::policy::Lifecycle::Ready, collection.generation().0,
    ).unwrap().facts.summary.attention.get(&meguri_sync::cloud::policy::Attention::ExchangeIncomplete).copied()
}

fn incomplete_end(rig: &Rig) -> Command {
    let action = policy_native_end(rig, true, None);
    assert!(matches!(&action.action.kind, ActionKind::RecordExchange(record)
        if matches!(record.outcome, ExchangeOutcome::Incomplete { .. })));
    action
}

fn policy_native_end(rig: &Rig, fetched: bool,
    terminal: Option<meguri_sync::cloud::adapter::CollectionOutcome>) -> Command {
    let driver = rig.host.cloud_policy_driver().unwrap();
    let run = command(rig.host.cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command)).unwrap(),
        |k| matches!(k, ActionKind::RunExchange { .. }));
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else { unreachable!() };
    let runtime = rig.host.cloud_runtime().unwrap();
    let native = driver.capture_exchange(&run, &rig.host.collection().unwrap()).unwrap();
    policy_fetch_snapshot(rig);
    runtime.adapter_complete_exchange(&rig.sdk, native, fetched, terminal).unwrap();
    command(driver.complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation()).unwrap(),
        |k| matches!(k, ActionKind::RecordExchange(_)))
}

fn policy_fetch_snapshot(rig: &Rig) {
    let runtime = rig.host.cloud_runtime().unwrap();
    let fetch = runtime.adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into(), rig.zone.clone()]).unwrap();
    let group = format!("incomplete:{fetch}");
    runtime.adapter_will_fetch(&rig.sdk, group.clone(), true).unwrap();
    for zone in ["meguri-root".to_owned(), rig.zone.clone()] {
        runtime.adapter_fetch_zone(&rig.sdk, zone.clone(), true, false).unwrap();
        if zone == rig.zone {
            runtime.adapter_fetched(&rig.sdk, &zone, &rig.records, &[]).unwrap();
        } else {
            runtime.adapter_fetched(&rig.sdk, &zone, std::slice::from_ref(&rig.active), &[]).unwrap();
        }
        runtime.adapter_fetch_zone(&rig.sdk, zone, false, false).unwrap();
    }
    runtime.adapter_did_fetch(&rig.sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&rig.sdk, fetch).unwrap());
    runtime.adapter_exchange_fetch(&rig.sdk, true).unwrap();
}

fn cancelled_exchange(rig: &Rig) -> Command {
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |kind| matches!(kind, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    command(
        rig.host
            .cloud_policy_driver()
            .unwrap()
            .end_before_capture(
                exchange,
                ExchangeOutcome::Cancelled,
                rig.host.engine().state(),
                rig.host.engine().generation(),
            )
            .unwrap(),
        |kind| matches!(kind, ActionKind::RecordExchange(_)),
    )
}

#[test]
fn media_off_applies_ordinary_collection_incoming_without_blob_work() {
    let rig = rig();
    let (id, before, apply) = incoming_deck(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap().id;
    assert_eq!(runtime.inbox_entries(binding).unwrap().len(), 1);
    rig.host
        .execute_cloud_policy_core(&apply, Duration::from_secs(10))
        .unwrap();
    assert_ne!(deck_row(&rig, id), before);
    assert!(runtime.inbox_entries(binding).unwrap().is_empty());
    assert!(runtime.apply_intents(binding).unwrap().is_empty());
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert!(
        runtime
            .cloud_media_work(&rig.host.collection().unwrap(), &rig.host.media().unwrap())
            .unwrap()
            .is_none()
    );
    rig.host.close().unwrap();
}

#[test]
fn native_pause_rejects_issued_apply_without_changing_collection_or_tracking() {
    let rig = rig();
    let (id, before, apply) = incoming_deck(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap().id;
    let inbox = runtime.inbox_entries(binding).unwrap();
    runtime.adapter_pause(true).unwrap();
    assert!(matches!(
        rig.host.execute_cloud_policy_core(&apply, Duration::from_secs(10)),
        Err(DriverError::StaleCommand)
    ));
    assert_eq!(deck_row(&rig, id), before);
    assert_eq!(runtime.inbox_entries(binding).unwrap(), inbox);
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert!(rig.host.cloud_policy_driver().unwrap().state().tracking_stop.is_none());
    rig.host.close().unwrap();
}

#[test]
fn native_pause_rejects_issued_exchange_evidence_without_writing_it() {
    let rig = rig();
    let record = cancelled_exchange(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let before = rig.host.cloud_policy_driver().unwrap().status().evidence;
    runtime.adapter_pause(true).unwrap();
    assert!(matches!(
        rig.host.execute_cloud_policy_core(&record, Duration::from_secs(10)),
        Err(DriverError::StaleCommand)
    ));
    let projection = runtime
        .policy_projection(
            meguri_sync::cloud::policy::Lifecycle::Ready,
            rig.host.engine().generation().0,
        )
        .unwrap();
    assert_eq!(projection.evidence, before);
    assert!(!runtime.status().unwrap().tracking_stopped);
    rig.host.close().unwrap();
}

#[test]
fn actual_apply_rollback_and_cancel_preserve_typed_deferral_without_tracking_stop() {
    for interruption in [ApplyInterruption::Cancel, ApplyInterruption::Error] {
        let rig = rig();
        let (id, before, apply) = incoming_deck(&rig);
        let runtime = rig.host.cloud_runtime().unwrap();
        let rolled_back = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = rolled_back.clone();
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == IncomingStage::IncomingRollbackComplete {
                observed.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
        runtime.interrupt_next_incoming_for_testing(interruption);
        let result = rig.host.execute_cloud_policy_core(&apply, Duration::from_secs(10));
        match (interruption, result) {
            (ApplyInterruption::Cancel, Err(DriverError::ApplyDeferred(meguri_engine::EngineError::Cancelled))) => {}
            (
                ApplyInterruption::Error,
                Err(DriverError::ApplyDeferred(meguri_engine::EngineError::Internal { .. })),
            ) => {}
            (_, other) => panic!("expected the actual rolled-back writer error: {other:?}"),
        }
        assert!(rolled_back.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(deck_row(&rig, id), before);
        let binding = runtime.status().unwrap().binding.unwrap().id;
        assert!(runtime.apply_intents(binding).unwrap().is_empty());
        assert!(runtime.incoming_runs().is_empty());
        let inbox = runtime.inbox_entries(binding).unwrap();
        assert_eq!(inbox.len(), 1);
        if interruption == ApplyInterruption::Cancel {
            assert_eq!(inbox[0].entry.reason, meguri_sync::cloud::InboxReason::Ready);
            assert_eq!(inbox[0].entry.retry_count, 0);
        } else {
            assert!(matches!(
                inbox[0].entry.reason,
                meguri_sync::cloud::InboxReason::ApplyFailed { .. }
            ));
            assert_eq!(inbox[0].entry.retry_count, 1);
        }
        assert!(!runtime.status().unwrap().tracking_stopped);
        assert!(rig.host.cloud_policy_driver().unwrap().state().tracking_stop.is_none());
        rig.host.close().unwrap();
    }
}

#[test]
fn provider_after_native_join_allows_fresh_preview_after_healthy_stop() {
    use meguri_sync::cloud::{BindingState, IsolationReason, publication::{PreviewWork, PublicationError}};

    let rig = rig();
    let runtime = rig.host.cloud_runtime().unwrap();
    let original = runtime.status().unwrap().binding.unwrap();
    assert!(!runtime.status().unwrap().tracking_stopped);
    runtime.validate_sdk(&rig.sdk).unwrap();
    assert!(matches!(
        runtime.provider_after_native_join(),
        Err(RuntimeError::Publication(PublicationError::Busy))
    ));
    // Retirement alone still leaves the actual registered SDK installed.
    runtime.retire_sdk(&rig.sdk, false).unwrap();
    assert!(matches!(runtime.validate_sdk(&rig.sdk), Err(RuntimeError::StaleIncarnation)));
    assert!(matches!(
        runtime.provider_after_native_join(),
        Err(RuntimeError::Publication(PublicationError::Busy))
    ));

    let preview = provider_preview_after_sdk_retirement(&rig);
    let status = runtime.status().unwrap();
    let stopped = status.binding.unwrap();
    assert_eq!(stopped.id, original.id);
    assert_eq!(stopped.scope, original.scope);
    assert_eq!(stopped.facts.account, original.facts.account);
    assert_eq!(stopped.facts.publication, original.facts.publication);
    assert_eq!(stopped.facts.state, BindingState::Isolated(IsolationReason::Stopped));
    assert!(!status.stored_usable && !status.identity_verified);
    assert!(status.scan_required && !status.scan_completed_this_open);
    assert!(matches!(runtime.validate_sdk(&rig.sdk), Err(RuntimeError::StaleIncarnation)));
    assert!(runtime.register_sdk(false).is_err());
    assert!(runtime.resume_binding_offer().is_none());
    rig.host.close().unwrap();
    assert!(matches!(
        preview.expect("healthy provider retirement must permit an explicit fresh preview"),
        PreviewWork::ReadActive { .. }
    ));
}

// Core exclusions only: these calls do not certify native SDK cancellation/drain.
fn provider_preview_after_sdk_retirement(
    rig: &Rig,
) -> Result<meguri_sync::cloud::publication::PreviewWork, RuntimeError> {
    let runtime = rig.host.cloud_runtime().unwrap();
    let account = runtime.status().unwrap().binding.unwrap().facts.account;
    runtime.retire_sdk(&rig.sdk, false).unwrap();
    runtime.release_sdk(&rig.sdk).unwrap();
    runtime.provider_after_native_join().unwrap();
    rig.host.stop_using_icloud().unwrap();
    let read = runtime.adapter_begin_identity_read(false, None).unwrap();
    assert!(runtime.adapter_complete_identity_read(read, IdentityStatus::Available(account)).unwrap());
    rig.host.begin_cloud_connection_preview()
}

#[test]
fn actual_apply_rollback_persistence_failure_stops_tracking() {
    let rig = rig();
    let (id, before, apply) = incoming_deck(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    runtime.interrupt_next_incoming_for_testing(ApplyInterruption::Error);
    meguri_sync::cloud::testing::arm_tracking_fault(
        "cloud.tracking.commit.rollback",
        meguri_sync::cloud::testing::TrackingFault::Constraint,
    )
    .unwrap();
    let result = rig.host.execute_cloud_policy_core(&apply, Duration::from_secs(10));
    assert!(matches!(result, Err(DriverError::CoreActionFailed { .. })));
    assert_eq!(deck_row(&rig, id), before);
    assert!(runtime.status().unwrap().tracking_stopped);
    assert!(rig.host.cloud_policy_driver().unwrap().state().tracking_stop.is_some());
    assert!(
        rig.host
            .cloud_policy_driver()
            .unwrap()
            .status()
            .attention
            .contains_key(&meguri_sync::cloud::policy::Attention::TrackingError)
    );
    assert!(matches!(
        provider_preview_after_sdk_retirement(&rig),
        Err(RuntimeError::Tracking(meguri_sync::cloud::Error::Database))
    ));
    assert!(runtime.status().unwrap().tracking_stopped);
    assert!(matches!(runtime.validate_sdk(&rig.sdk), Err(RuntimeError::StaleIncarnation)));
    rig.host.close().unwrap();
}

#[test]
fn failed_rollover_does_not_receipt_or_write_evidence_and_explicit_retry_checks_again() {
    let rig = rig();
    let collection = rig.host.collection().unwrap();
    collection.check_rollover().unwrap();
    let events = rig.host.engine().stream().subscribe();
    rig.clock.step_ms(86_400_000);
    let record = cancelled_exchange(&rig);
    // Calling from the writer makes the actual check_rollover reject its caller.
    // A second explicit call must fail too, rather than skipping the failed check.
    for _ in 0..2 {
        let host = rig.host.clone();
        let attempt = record.clone();
        let result = collection
            .run(
                meguri_engine::OpSpec::named("policy_rollover_caller", meguri_engine::OpSource::CloudIncoming),
                move |_| Ok(host.execute_cloud_policy_core(&attempt, Duration::from_secs(10))),
            )
            .unwrap()
            .value;
        assert!(matches!(
            result,
            Err(DriverError::Runtime(RuntimeError::Engine(
                meguri_engine::EngineError::InvalidArgument { .. }
            )))
        ));
        assert!(
            rig.host
                .cloud_policy_driver()
                .unwrap()
                .status()
                .evidence
                .last_attempt
                .is_none()
        );
        assert!(!rig.host.cloud_runtime().unwrap().status().unwrap().tracking_stopped);
    }
    // Authority is still valid; this explicit call runs outside the writer.
    rig.host.cloud_policy_driver().unwrap().validate(&record).unwrap();
    rig.host
        .execute_cloud_policy_core(&record, Duration::from_secs(10))
        .unwrap();
    let rollovers = std::iter::from_fn(|| events.try_recv())
        .filter(|event| matches!(event.kind, meguri_engine::changes::stream::EventKind::RolledOver { .. }))
        .count();
    assert_eq!(rollovers, 1);
    assert!(
        rig.host
            .cloud_policy_driver()
            .unwrap()
            .status()
            .evidence
            .last_attempt
            .is_some()
    );
    rig.host
        .execute_cloud_policy_core(&record, Duration::from_secs(10))
        .unwrap();
    assert!(
        std::iter::from_fn(|| events.try_recv())
            .all(|event| !matches!(event.kind, meguri_engine::changes::stream::EventKind::RolledOver { .. }))
    );
    rig.host.close().unwrap();
}

#[test]
fn real_pending_mutation_and_actual_settings_reach_retained_driver() {
    let rig = rig();
    let collection = rig.host.collection().unwrap();
    // Stable-layout capture supports edits to a tracked deck, not new structural rows.
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Policy pending deck".into()).unwrap();
    rig.host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Local))
        .unwrap();
    let status = rig.host.cloud_policy_driver().unwrap().status();
    assert!(status.pending > 0);
    assert!(status.evidence.as_of_ms().is_none());
    assert_eq!(
        status.state,
        StatusState::Paused(meguri_sync::cloud::policy::PausedReason::AutomaticOff)
    );
    rig.host
        .cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
            enabled: true,
            paused: false,
            automatic: true,
            media: false,
        }))
        .unwrap();
    assert_eq!(
        rig.host.cloud_policy_driver().unwrap().status().state,
        StatusState::ChangesPending
    );
    rig.host.close().unwrap();
}

#[test]
fn early_exchange_clock_and_receipt_survive_repeat_and_tracking_failure() {
    let rig = rig();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Key))
            .unwrap(),
        |k| matches!(k, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    rig.clock.step_ms(31_000);
    let driver = rig.host.cloud_policy_driver().unwrap();
    let commands = driver
        .end_before_capture(
            exchange,
            ExchangeOutcome::Waiting {
                class: Some(OperationError::NoAccount),
                retry_after_ms: None,
            },
            rig.host.engine().state(),
            rig.host.engine().generation(),
        )
        .unwrap();
    let record = command(commands, |k| matches!(k, ActionKind::RecordExchange(_)));
    let duplicate = driver
        .end_before_capture(
            exchange,
            ExchangeOutcome::Cancelled,
            rig.host.engine().state(),
            rig.host.engine().generation(),
        )
        .unwrap();
    assert!(
        !duplicate
            .iter()
            .any(|c| matches!(c.action.kind, ActionKind::RecordExchange(_)))
    );
    let runtime = rig.host.cloud_runtime().unwrap();
    runtime.policy_evidence_constraint_for_testing(true).unwrap();
    let failed = rig.host.execute_cloud_policy_core(&record, Duration::from_secs(10));
    runtime.policy_evidence_constraint_for_testing(false).unwrap();
    assert!(matches!(failed, Err(DriverError::CoreActionFailed { .. })));
    assert!(runtime.status().unwrap().tracking_stopped);
    let status = driver.status();
    assert!(status.evidence.last_attempt.is_none());
    assert!(
        status
            .attention
            .contains_key(&meguri_sync::cloud::policy::Attention::TrackingError)
    );
    // Evidence has not been fabricated by either the early end or the failed write.
    assert!(status.evidence.last_completed.is_none());
    rig.host.close().unwrap();
}

#[test]
fn early_exchange_is_durable_and_replaying_action_does_not_change_its_clock() {
    let rig = rig();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |k| matches!(k, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    rig.clock.step_ms(30_000);
    let driver = rig.host.cloud_policy_driver().unwrap();
    let record = command(
        driver
            .end_before_capture(
                exchange,
                ExchangeOutcome::Cancelled,
                rig.host.engine().state(),
                rig.host.engine().generation(),
            )
            .unwrap(),
        |k| matches!(k, ActionKind::RecordExchange(_)),
    );
    rig.host
        .execute_cloud_policy_core(&record, Duration::from_secs(10))
        .unwrap();
    let first = driver.status().evidence.last_attempt.unwrap();
    assert_eq!(first.ended_ms, 1_894_701_630_000);
    rig.clock.step_ms(4_000);
    rig.host
        .execute_cloud_policy_core(&record, Duration::from_secs(10))
        .unwrap();
    assert_eq!(driver.status().evidence.last_attempt, Some(first));
    assert!(driver.status().evidence.last_completed.is_none());
    rig.host.close().unwrap();
}

#[test]
fn actual_sdk_drains_qualify_quiet_only_after_both_zones_and_durable_evidence_write() {
    let rig = rig();
    let runtime = rig.host.cloud_runtime().unwrap();
    rig.host
        .cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
            enabled: true,
            paused: false,
            automatic: true,
            media: false,
        }))
        .unwrap();
    let call = runtime
        .adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into(), rig.zone.clone()])
        .unwrap();
    let group = format!("fetch:{call}");
    // Existing adapter contract uses the call's own group; no policy-created drains.
    runtime.adapter_will_fetch(&rig.sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&rig.sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&rig.sdk, call).unwrap());
    let commands = rig
        .host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let quiet = command(commands, |k| matches!(k, ActionKind::RecordQuietPoint { .. }));
    assert!(
        rig.host
            .cloud_policy_driver()
            .unwrap()
            .status()
            .evidence
            .quiet_ms
            .is_none()
    );
    rig.host
        .execute_cloud_policy_core(&quiet, Duration::from_secs(10))
        .unwrap();
    assert!(
        rig.host
            .cloud_policy_driver()
            .unwrap()
            .status()
            .evidence
            .quiet_ms
            .is_some()
    );
    // Full attention/media projection is explicitly unknown in this baseline Runtime.
    assert_eq!(
        rig.host.cloud_policy_driver().unwrap().status().state,
        StatusState::AwaitingEvidence
    );
    rig.host.close().unwrap();
}

fn quiet_rig() -> Rig {
    rig_with_files_and_state(false, 1914, true, Arc::new(NativeFiles), true)
}

// An SDK with saved state can drain unchanged zones before root confirmation.
// The existing baseline has no stored Active record; no root proof is supplied.
fn quiet_without_root(rig: &Rig) -> Command {
    let runtime = rig.host.cloud_runtime().unwrap();
    assert!(!runtime.sdk_root_confirmed());
    rig.host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: true, media: false,
    })).unwrap();
    let call = runtime.adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into(), rig.zone.clone()]).unwrap();
    let group = format!("quiet:{call}");
    runtime.adapter_will_fetch(&rig.sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&rig.sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&rig.sdk, call).unwrap());
    assert!(!runtime.sdk_root_confirmed());
    let commands = rig.host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
    let driver = rig.host.cloud_policy_driver().unwrap();
    assert!(driver.state().gate_open());
    assert!(!driver.state().root_confirmed);
    assert!(driver.status().evidence.quiet_ms.is_none());
    command(commands, |kind| matches!(kind, ActionKind::RecordQuietPoint { .. }))
}

#[test]
fn quiet_without_root_is_durable_but_never_authorizes_collection_sending() {
    use meguri_sync::cloud::policy::{Lifecycle, PolicyWork};
    let rig = quiet_rig();
    let quiet = quiet_without_root(&rig);
    let ActionKind::RecordQuietPoint { at_ms } = quiet.action.kind else { unreachable!() };
    let cancel = meguri_engine::CancelToken::new();
    let mut work = PolicyWork::new(&cancel);
    rig.host.execute_cloud_policy_core_with_work(&quiet, Duration::from_secs(10), &mut work).unwrap();
    // The actual write latched this job's receipt. Later cancellation cannot
    // replace its result; the candidate was not merely consumed as superseded.
    cancel.cancel();
    assert!(work.check().is_ok());
    let runtime = rig.host.cloud_runtime().unwrap();
    let evidence = rig.host.cloud_policy_driver().unwrap().status().evidence;
    assert_eq!(evidence.quiet_ms, Some(at_ms));
    assert!(evidence.last_completed.is_none());
    assert!(evidence.last_attempt.is_none());
    assert!(!runtime.sdk_root_confirmed());
    let collection = rig.host.collection().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Still awaiting root".into()).unwrap();
    let dirty = runtime.entity_facts_page(codec::Kind::Deck, None).unwrap().into_iter()
        .find(|entity| entity.key == codec::id_key(rig.baseline_deck.0)).unwrap();
    assert!(dirty.mark.is_some());
    runtime.validate_sdk(&rig.sdk).unwrap();
    assert!(runtime.start_manual_send_call(&rig.sdk).unwrap().is_none());
    let batch = runtime.build_collection_batch(&rig.sdk, &collection, &[dirty.name].into()).unwrap();
    assert_eq!(batch.id, 0);
    assert!(batch.saves.is_empty());
    assert!(!runtime.sdk_root_confirmed());
    assert!(!rig.host.cloud_policy_driver().unwrap().state().facts.summary.attention_known);
    rig.host.close().unwrap();
    let reopened = CoreHost::open(rig.config.clone()).unwrap();
    let projection = reopened.cloud_runtime().unwrap().policy_projection(
        Lifecycle::Ready, reopened.engine().generation().0,
    ).unwrap();
    assert_eq!(projection.evidence, evidence);
    assert!(!projection.facts.summary.attention_known);
    reopened.close().unwrap();
}

#[test]
fn quiet_without_root_cancelled_work_leaves_qualifying_candidate_available() {
    use meguri_sync::cloud::policy::PolicyWork;
    let rig = quiet_rig();
    let quiet = quiet_without_root(&rig);
    let cancel = meguri_engine::CancelToken::new();
    cancel.cancel();
    let mut work = PolicyWork::new(&cancel);
    assert!(rig.host.execute_cloud_policy_core_with_work(&quiet, Duration::from_secs(10), &mut work).is_err());
    assert!(work.check().is_err());
    assert!(rig.host.cloud_policy_driver().unwrap().status().evidence.quiet_ms.is_none());
    rig.host.execute_cloud_policy_core(&quiet, Duration::from_secs(10)).unwrap();
    assert!(rig.host.cloud_policy_driver().unwrap().status().evidence.quiet_ms.is_some());
    rig.host.close().unwrap();
}

#[test]
fn quiet_without_root_stale_candidate_cannot_stamp_after_pause() {
    use meguri_sync::cloud::policy::PolicyWork;
    let rig = quiet_rig();
    let quiet = quiet_without_root(&rig);
    rig.host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: true, automatic: true, media: false,
    })).unwrap();
    let cancel = meguri_engine::CancelToken::new();
    let mut work = PolicyWork::new(&cancel);
    assert!(matches!(rig.host.execute_cloud_policy_core_with_work(&quiet, Duration::from_secs(10), &mut work),
        Err(DriverError::StaleCommand)));
    cancel.cancel();
    assert!(work.check().is_err());
    assert!(rig.host.cloud_policy_driver().unwrap().status().evidence.quiet_ms.is_none());
    rig.host.close().unwrap();
}

#[test]
fn quiet_without_root_database_failure_preserves_error_and_required_stop() {
    use meguri_sync::cloud::policy::{Attention, PolicyWork};
    let rig = quiet_rig();
    let quiet = quiet_without_root(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    runtime.policy_evidence_constraint_for_testing(true).unwrap();
    let cancel = meguri_engine::CancelToken::new();
    let mut work = PolicyWork::new(&cancel);
    let failed = rig.host.execute_cloud_policy_core_with_work(&quiet, Duration::from_secs(10), &mut work);
    runtime.policy_evidence_constraint_for_testing(false).unwrap();
    let Err(DriverError::CoreActionFailed { error, commands }) = failed else { panic!("real evidence constraint must fail"); };
    assert!(matches!(error, RuntimeError::Tracking(meguri_sync::cloud::Error::Constraint)));
    assert!(commands.iter().any(|command| matches!(command.action.kind, ActionKind::StopEngine { .. })));
    cancel.cancel();
    assert!(work.check().is_ok());
    assert!(runtime.status().unwrap().tracking_stopped);
    let status = rig.host.cloud_policy_driver().unwrap().status();
    assert!(status.evidence.quiet_ms.is_none());
    assert!(status.evidence.last_completed.is_none());
    assert!(status.attention.contains_key(&Attention::TrackingError));
    rig.host.close().unwrap();
}

#[test]
fn closed_generation_cannot_write_or_roll_over_an_exchange() {
    let rig = rig();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |k| matches!(k, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    let driver = rig.host.cloud_policy_driver().unwrap();
    let record = command(
        driver
            .end_before_capture(
                exchange,
                ExchangeOutcome::Cancelled,
                rig.host.engine().state(),
                rig.host.engine().generation(),
            )
            .unwrap(),
        |k| matches!(k, ActionKind::RecordExchange(_)),
    );
    rig.host.close().unwrap();
    assert!(
        rig.host
            .execute_cloud_policy_core(&record, Duration::from_secs(10))
            .is_err()
    );
    assert!(driver.status().evidence.last_attempt.is_none());
}

#[test]
fn completed_runtime_exchange_uses_accepted_end_clock_not_projection_time() {
    let rig = rig();
    let driver = rig.host.cloud_policy_driver().unwrap();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |k| matches!(k, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    let native = driver.capture_exchange(&run, &rig.host.collection().unwrap()).unwrap();
    let runtime = rig.host.cloud_runtime().unwrap();
    let fetch = runtime
        .adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into(), rig.zone.clone()])
        .unwrap();
    runtime
        .adapter_will_fetch(&rig.sdk, "policy-exchange".into(), true)
        .unwrap();
    runtime.adapter_did_fetch(&rig.sdk, "policy-exchange", true).unwrap();
    assert!(runtime.adapter_finish_fetch(&rig.sdk, fetch).unwrap());
    runtime.adapter_exchange_fetch(&rig.sdk, true).unwrap();
    rig.clock.step_ms(10_000);
    let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
    assert_eq!(
        completed.collection,
        meguri_sync::cloud::adapter::CollectionOutcome::Succeeded
    );
    rig.clock.step_ms(90_000);
    let record = command(
        driver
            .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
            .unwrap(),
        |k| matches!(k, ActionKind::RecordExchange(_)),
    );
    rig.host
        .execute_cloud_policy_core(&record, Duration::from_secs(10))
        .unwrap();
    let evidence = driver.status().evidence;
    assert_eq!(evidence.last_completed.unwrap().ended_ms, 1_894_701_610_000);
    assert!(evidence.last_attempt.is_none());
    rig.host.close().unwrap();
}

// Reproduce P19 through real capture, owned SDK fetch outcomes and the core
// evidence command. This does not seed a policy summary or an exchange receipt.
fn k10_completed_exchange(rig: &Rig) {
    let driver = rig.host.cloud_policy_driver().unwrap();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |kind| matches!(kind, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    let runtime = rig.host.cloud_runtime().unwrap();
    let native = driver.capture_exchange(&run, &rig.host.collection().unwrap()).unwrap();
    let fetch = runtime
        .adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into(), rig.zone.clone()])
        .unwrap();
    let group = format!("fetch:{fetch}");
    runtime.adapter_will_fetch(&rig.sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&rig.sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&rig.sdk, fetch).unwrap());
    runtime.adapter_exchange_fetch(&rig.sdk, true).unwrap();
    rig.clock.step_ms(10_000);
    let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
    assert_eq!(
        completed.collection,
        meguri_sync::cloud::adapter::CollectionOutcome::Succeeded
    );
    let record = command(
        driver
            .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
            .unwrap(),
        |kind| matches!(kind, ActionKind::RecordExchange(_)),
    );
    rig.host
        .execute_cloud_policy_core(&record, Duration::from_secs(10))
        .unwrap();
    rig.host
        .cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
            enabled: true,
            paused: false,
            automatic: true,
            media: false,
        }))
        .unwrap();
}

#[test]
fn k10_projection_completed_scan_and_exchange_cannot_prove_missing_attention_facts() {
    use meguri_sync::cloud::policy::{Lifecycle, Scan};
    let rig = rig();
    k10_completed_exchange(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let projection = runtime
        .policy_projection(Lifecycle::Ready, rig.host.engine().generation().0)
        .unwrap();
    let binding = projection.facts.binding.as_ref().unwrap();
    assert!(binding.usable && binding.verified);
    assert!(projection.root_confirmed);
    let summary = &projection.facts.summary;
    assert!(summary.collection_known);
    assert_eq!(summary.scan, Scan::None);
    assert_eq!(
        (
            summary.pending_unheld,
            summary.sendable,
            summary.in_flight,
            summary.ready,
            summary.incoming_waiting,
            summary.unsettled_intents
        ),
        (0, 0, 0, 0, 0, 0)
    );
    assert!(!summary.apply_in_progress);
    assert!(summary.attention.is_empty());
    let completed = projection.evidence.last_completed.as_ref().unwrap();
    assert!(matches!(completed.outcome, ExchangeOutcome::Succeeded { .. }));
    assert_eq!(completed.ended_ms, 1_894_701_610_000);
    // Known-clean work and P19 success are insufficient: the production K10
    // flag/root/crt/generation producers documented in the handoff are absent.
    // Keep this conservative until those producers supply actual evidence.
    assert!(!summary.attention_known);
    let status = rig.host.cloud_policy_driver().unwrap().status();
    assert!(status.account_matches_binding);
    assert_eq!(status.evidence, projection.evidence);
    assert_eq!(status.state, StatusState::AwaitingEvidence);
    rig.host.close().unwrap();
}

#[test]
fn k10_projection_scan_without_p19_evidence_does_not_supply_a_zero_or_timestamp() {
    use meguri_sync::cloud::policy::Lifecycle;
    let rig = rig();
    let projection = rig
        .host
        .cloud_runtime()
        .unwrap()
        .policy_projection(Lifecycle::Ready, rig.host.engine().generation().0)
        .unwrap();
    assert!(projection.facts.summary.collection_known);
    assert!(!projection.facts.summary.attention_known);
    assert!(projection.evidence.last_completed.is_none());
    assert!(projection.evidence.quiet_ms.is_none());
    assert_eq!(projection.evidence.as_of_ms(), None);
    rig.host.close().unwrap();
}

// Canonical source base: 71e68bd5. Pending integration batch base remains
// 1416a520542a8c5daa38a23bfe398da704c3d6c9. P14.4 / V8.1 producer regression.
fn recover_policy_conflicts(rig: &Rig, unreadable: bool) -> Vec<String> {
    use meguri_sync::cloud::{self, runtime::with_tracking_access};
    let facts = rig.host.cloud_runtime().unwrap().status().unwrap().binding.unwrap().facts;
    rig.host.close().unwrap();
    let source = tempfile::tempdir().unwrap();
    // V8.1 salvage includes foreign scopes; ordinary runtime access still does not.
    let foreign = BuildScope {
        container: ContainerId::new("synthetic-foreign-recovery").unwrap(),
        environment: Environment::Production,
    };
    let cloud::OpenOutcome::Missing(mut store) = cloud::TrackingStore::open(source.path(), foreign).unwrap()
    else {
        panic!("expected new synthetic source");
    };
    let owner = store.create_binding(facts, rig.config.clock.as_ref(), rig.config.random.as_ref()).unwrap();
    for id in 1..=3 {
        store.store_conflict(owner.id, &cloud::ConflictCopy {
            id: cloud::CopyId([id; 16]),
            kind: cloud::ConflictKind::Note,
            key: OpaqueBytes(vec![id]),
            reason: cloud::ConflictReason::FieldConflict,
            losing_envelope: OpaqueBytes(b"synthetic-recovery-conflict".to_vec()),
            losing_payload_digest: cloud::PayloadDigest([id; 32]),
            winning_payload_digest: cloud::PayloadDigest([99; 32]),
            field_names: None,
            template_names: None,
            note_deck: None,
            staged_path: None,
            created_at_ms: rig.clock.now_ms(),
            resolved_at_ms: None,
        }).unwrap();
    }
    drop(store);
    let db = meguri_storage::rusqlite::Connection::open(source.path().join(cloud::TRACKING_FILE)).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
    if unreadable {
        // Valid JSON but not a ConflictCopy, as in the partial-recovery regression.
        for id in [2_u8, 3] {
            assert_eq!(db.execute("UPDATE conflicts SET content='{}' WHERE id=?1", [[id; 16].as_slice()]).unwrap(), 1);
        }
    }
    db.execute_batch("DROP TABLE inbox;").unwrap();
    let directory = &rig.config.collection_directory;
    // Copy the quiescent pair before its fixture connection can checkpoint it.
    for suffix in ["", "-wal"] {
        std::fs::copy(source.path().join(format!("{}{suffix}", cloud::TRACKING_FILE)),
            directory.join(format!("{}{suffix}", cloud::TRACKING_FILE))).unwrap();
    }
    drop(db);
    let scope = rig.config.cloud.as_ref().unwrap().scope.clone();
    with_tracking_access(|| {
        let outcome = cloud::TrackingStore::recover(directory, scope,
            rig.config.clock.as_ref(), rig.config.random.as_ref()).unwrap();
        let (cloud::OpenOutcome::Missing(store) | cloud::OpenOutcome::Opened(store)) = outcome else {
            panic!("expected completed synthetic recovery");
        };
        assert!(store.selected_binding().unwrap().is_none());
        assert_eq!(store.salvage_bindings().unwrap().len(), 1);
        assert_eq!(store.conflict_copies().unwrap().len(), if unreadable { 1 } else { 3 });
        store.retained_damaged_sources().unwrap()
    })
}

#[test]
fn k10_projection_retained_unreadable_conflicts_survive_binding_and_reopen() {
    use meguri_sync::cloud::{self, policy::{Attention, Lifecycle}};
    let rig = rig_with_epoch(true, 1931, false);
    let kept = recover_policy_conflicts(&rig, true);
    assert_eq!(kept.len(), 1);
    let path = rig.config.collection_directory.join(&kept[0]);
    let wal = rig.config.collection_directory.join(format!("{}-wal", kept[0]));
    let before = (std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap());
    // First reopen with only salvage owners, then install an ordinary synthetic
    // baseline and reopen again. No policy summary or attention is seeded.
    for ordinary in [false, true] {
        for reopened in [false, true] {
            let host = CoreHost::open(rig.config.clone()).unwrap();
            let runtime = host.cloud_runtime().unwrap();
            let collection = host.collection().unwrap();
            if ordinary && !reopened {
                runtime.prepare_adapter_baseline_for_testing(&collection,
                    rig.config.clock.as_ref(), rig.config.random.as_ref(), None).unwrap();
            }
            host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
                enabled: true, paused: false, automatic: true, media: false,
            })).unwrap();
            host.cloud_policy_input(DeviceEvent::Account {
                identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
                origin_sdk: None,
            }).unwrap();
            host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
            let projection = runtime.policy_projection(Lifecycle::Ready, collection.generation().0).unwrap();
            assert_eq!(projection.facts.binding.is_some(), ordinary);
            assert_eq!(projection.facts.summary.attention.get(&Attention::ConflictsUnreadable), Some(&1));
            assert_eq!(projection.facts.summary.attention.get(&Attention::Conflicts), Some(&1));
            assert!(!projection.facts.summary.attention_known);
            let status = host.cloud_policy_driver().unwrap().status();
            assert_eq!(status.state, StatusState::NeedsAttention);
            assert_eq!(status.attention.get(&Attention::ConflictsUnreadable), Some(&1));
            assert_eq!(status.attention.get(&Attention::Conflicts), Some(&1));
            assert_eq!(status.attention.contains_key(&Attention::ChooseCollection), !ordinary);
            assert!(!status.attention.contains_key(&Attention::Isolated));
            assert!(!format!("{:?} {status:?}", projection.facts).contains(&kept[0]));
            // An unsafe identity at an actual kept path must fail projection,
            // including without an ordinary binding; it cannot become zero.
            let saved = rig.config.collection_directory.join("retained-source-error-fixture");
            std::fs::rename(&path, &saved).unwrap();
            std::os::unix::fs::symlink(&saved, &path).unwrap();
            assert!(matches!(runtime.policy_projection(Lifecycle::Ready, collection.generation().0),
                Err(RuntimeError::Tracking(cloud::Error::RecoveryRefused))));
            std::fs::remove_file(&path).unwrap();
            std::fs::rename(&saved, &path).unwrap();
            host.close().unwrap();
            assert_eq!((std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap()), before);
        }
    }
}

#[test]
fn k10_projection_complete_salvage_has_no_unreadable_item_or_readiness_claim() {
    use meguri_sync::cloud::policy::{Attention, Lifecycle};
    let rig = rig_with_epoch(true, 1932, false);
    assert!(recover_policy_conflicts(&rig, false).is_empty());
    for _ in 0..2 {
        let host = CoreHost::open(rig.config.clone()).unwrap();
        let projection = host.cloud_runtime().unwrap()
            .policy_projection(Lifecycle::Ready, host.engine().generation().0).unwrap();
        assert!(projection.facts.binding.is_none());
        assert_eq!(projection.facts.summary.attention,
            [(Attention::Conflicts, 3)].into_iter().collect());
        assert!(!projection.facts.summary.attention_known);
        host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
            enabled: true, paused: false, automatic: true, media: false,
        })).unwrap();
        let status = host.cloud_policy_driver().unwrap().status();
        assert!(!status.attention.contains_key(&Attention::ConflictsUnreadable));
        assert_eq!(status.attention.get(&Attention::Conflicts), Some(&3));
        assert!(!matches!(status.state, StatusState::UpToDate { .. }));
        host.close().unwrap();
    }
}

fn preserved_policy_copy(id: u8, now_ms: i64, resolved: bool) -> meguri_sync::cloud::ConflictCopy {
    use meguri_sync::cloud::*;
    ConflictCopy {
        id: CopyId([id; 16]),
        kind: ConflictKind::Note,
        // Multiple versions of one object must still count once per copy.
        key: OpaqueBytes(b"synthetic-preserved-note".to_vec()),
        reason: ConflictReason::FieldConflict,
        losing_envelope: OpaqueBytes(b"synthetic-preserved-version".to_vec()),
        losing_payload_digest: PayloadDigest([id; 32]),
        winning_payload_digest: PayloadDigest([99; 32]),
        field_names: None,
        template_names: None,
        note_deck: None,
        staged_path: None,
        created_at_ms: now_ms,
        resolved_at_ms: resolved.then_some(now_ms),
    }
}

fn policy_conflict_store(rig: &Rig, scope: BuildScope) -> meguri_sync::cloud::TrackingStore {
    use meguri_sync::cloud::{OpenOutcome, TrackingStore};
    let (OpenOutcome::Missing(store) | OpenOutcome::Opened(store)) =
        TrackingStore::open(&rig.config.collection_directory, scope).unwrap()
    else {
        panic!("expected healthy synthetic tracking store");
    };
    store
}

fn assert_preserved_policy_count(host: &CoreHost, count: Option<u64>) {
    use meguri_sync::cloud::policy::{Attention, Lifecycle};
    let projection = host.cloud_runtime().unwrap()
        .policy_projection(Lifecycle::Ready, host.engine().generation().0).unwrap();
    assert_eq!(projection.facts.summary.attention.get(&Attention::Conflicts).copied(), count);
    assert!(!projection.facts.summary.attention_known);
}

#[test]
fn k10_projection_preserved_conflicts_counts_current_isolated_and_other_build_copies() {
    use meguri_sync::cloud::{BindingState, IsolationReason, runtime::with_tracking_access};
    let rig = rig_with_epoch(true, 1933, false);
    let binding = rig.host.cloud_runtime().unwrap().status().unwrap().binding.unwrap();
    rig.host.close().unwrap();
    let scope = rig.config.cloud.as_ref().unwrap().scope.clone();
    with_tracking_access(|| {
        let mut store = policy_conflict_store(&rig, scope);
        store.store_conflict(binding.id, &preserved_policy_copy(1, rig.clock.now_ms(), false)).unwrap();
        store.store_conflict(binding.id, &preserved_policy_copy(4, rig.clock.now_ms(), true)).unwrap();
        let mut facts = binding.facts.clone();
        facts.state = BindingState::Isolated(IsolationReason::ReplacedLocally);
        let earlier = StepClock::new(rig.clock.now_ms() - 1, UtcOffset::UTC, "UTC");
        let older = store.create_binding(facts, &earlier, rig.config.random.as_ref()).unwrap();
        for id in [2, 3] {
            store.store_conflict(older.id, &preserved_policy_copy(id, earlier.now_ms(), false)).unwrap();
        }
        drop(store);
        let foreign_scope = BuildScope {
            container: ContainerId::new("synthetic-preserved-other-build").unwrap(),
            environment: Environment::Production,
        };
        let mut foreign = policy_conflict_store(&rig, foreign_scope);
        let owner = foreign.create_binding(binding.facts.clone(), rig.clock.as_ref(), rig.config.random.as_ref()).unwrap();
        foreign.store_conflict(owner.id, &preserved_policy_copy(5, rig.clock.now_ms(), false)).unwrap();
        foreign.store_conflict(owner.id, &preserved_policy_copy(6, rig.clock.now_ms(), true)).unwrap();
    });
    // Only stored copies are inputs. Each reopen projects through the real Runtime.
    for (stage, expected) in [(0, Some(4)), (1, Some(3)), (2, Some(2)), (3, None)] {
        if stage > 0 {
            // U161 resolve/unresolve is not exposed by TrackingStore yet. Change
            // storage fixtures directly; this tests projection, not UI operations.
            with_tracking_access(|| {
                let db = meguri_storage::rusqlite::Connection::open(
                    rig.config.collection_directory.join(meguri_sync::cloud::TRACKING_FILE)).unwrap();
                match stage {
                    1 => assert_eq!(db.execute(
                        "UPDATE conflicts SET content=json_set(content,'$.resolved_at_ms',?1) WHERE id=?2",
                        meguri_storage::rusqlite::params![rig.clock.now_ms(), [1_u8; 16].as_slice()],
                    ).unwrap(), 1),
                    2 => assert_eq!(db.execute("DELETE FROM conflicts WHERE id=?1", [[2_u8; 16].as_slice()]).unwrap(), 1),
                    _ => assert_eq!(db.execute(
                        "UPDATE conflicts SET content=json_set(content,'$.resolved_at_ms',?1) WHERE id IN (?2,?3)",
                        meguri_storage::rusqlite::params![rig.clock.now_ms(), [3_u8; 16].as_slice(), [5_u8; 16].as_slice()],
                    ).unwrap(), 2),
                }
            });
        }
        let host = CoreHost::open(rig.config.clone()).unwrap();
        assert_eq!(host.cloud_runtime().unwrap().status().unwrap().binding.unwrap().id, binding.id);
        assert_preserved_policy_count(&host, expected);
        host.close().unwrap();
    }
}

#[test]
fn k10_projection_preserved_conflicts_without_binding_includes_salvage_and_other_build() {
    use meguri_sync::cloud::{self, runtime::with_tracking_access};
    let rig = rig_with_epoch(true, 1934, false);
    let facts = rig.host.cloud_runtime().unwrap().status().unwrap().binding.unwrap().facts;
    assert!(recover_policy_conflicts(&rig, false).is_empty());
    let scope = rig.config.cloud.as_ref().unwrap().scope.clone();
    with_tracking_access(|| {
        let foreign_scope = BuildScope {
            container: ContainerId::new("synthetic-preserved-other-build").unwrap(),
            environment: Environment::Production,
        };
        let mut foreign = policy_conflict_store(&rig, foreign_scope);
        let owner = foreign.create_binding(facts, rig.clock.as_ref(), rig.config.random.as_ref()).unwrap();
        foreign.store_conflict(owner.id, &preserved_policy_copy(4, rig.clock.now_ms(), false)).unwrap();
        foreign.store_conflict(owner.id, &preserved_policy_copy(5, rig.clock.now_ms(), true)).unwrap();
        drop(foreign);
        let mut store = policy_conflict_store(&rig, scope);
        assert!(store.selected_binding().unwrap().is_none());
        assert!(store.binding(owner.id).unwrap().is_none());
        let salvage = store.conflict_copies().unwrap().into_iter()
            .find(|c| c.copy.id == cloud::CopyId([1; 16])).unwrap().binding;
        assert!(store.binding(salvage).unwrap().is_none());
        for id in [owner.id, salvage] {
            assert_eq!(store.store_conflict(id, &preserved_policy_copy(9, rig.clock.now_ms(), false)),
                Err(cloud::Error::BindingNotFound));
        }
    });
    let host = CoreHost::open(rig.config.clone()).unwrap();
    assert!(host.cloud_runtime().unwrap().policy_projection(
        cloud::policy::Lifecycle::Ready, host.engine().generation().0).unwrap().facts.binding.is_none());
    assert_preserved_policy_count(&host, Some(4));
    host.close().unwrap();
}

#[test]
fn k10_projection_preserved_conflicts_tracking_errors_are_not_zero_without_binding() {
    use meguri_sync::cloud::{self, policy::Lifecycle, runtime::with_tracking_access};
    let rig = rig_with_epoch(true, 1935, false);
    assert!(recover_policy_conflicts(&rig, false).is_empty());
    let host = CoreHost::open(rig.config.clone()).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let generation = host.engine().generation().0;
    assert!(runtime.policy_projection(Lifecycle::Ready, generation).unwrap().facts.binding.is_none());
    assert_preserved_policy_count(&host, Some(3));
    let content: String = with_tracking_access(|| {
        let db = meguri_storage::rusqlite::Connection::open(
            rig.config.collection_directory.join(cloud::TRACKING_FILE)).unwrap();
        let content = db.query_row("SELECT content FROM conflicts WHERE id=?1", [[1_u8; 16].as_slice()],
            |row| row.get(0)).unwrap();
        assert_eq!(db.execute("UPDATE conflicts SET content='{}' WHERE id=?1", [[1_u8; 16].as_slice()]).unwrap(), 1);
        content
    });
    assert!(matches!(runtime.policy_projection(Lifecycle::Ready, generation),
        Err(RuntimeError::Tracking(cloud::Error::InvalidData))));
    with_tracking_access(|| {
        let db = meguri_storage::rusqlite::Connection::open(
            rig.config.collection_directory.join(cloud::TRACKING_FILE)).unwrap();
        db.execute("UPDATE conflicts SET content=?1 WHERE id=?2",
            meguri_storage::rusqlite::params![content, [1_u8; 16].as_slice()]).unwrap();
        db.execute_batch("ALTER TABLE conflicts RENAME TO synthetic_unreadable_conflicts;").unwrap();
    });
    assert!(matches!(runtime.policy_projection(Lifecycle::Ready, generation),
        Err(RuntimeError::Tracking(_))));
    with_tracking_access(|| {
        let db = meguri_storage::rusqlite::Connection::open(
            rig.config.collection_directory.join(cloud::TRACKING_FILE)).unwrap();
        db.execute_batch("ALTER TABLE synthetic_unreadable_conflicts RENAME TO conflicts;").unwrap();
    });
    host.close().unwrap();
}

#[test]
fn k10_projection_corrupt_records_count_refusals_despite_prior_success() {
    use meguri_sync::cloud::policy::{Attention, Lifecycle};
    let rig = rig();
    k10_completed_exchange(&rig);
    let driver = rig.host.cloud_policy_driver().unwrap();
    let evidence = driver.status().evidence;
    let runtime = rig.host.cloud_runtime().unwrap();
    let corrupt: Vec<_> = rig
        .records
        .iter()
        .take(2)
        .map(|saved| Saved {
            name: saved.name.clone(),
            envelope: b"synthetic-invalid-envelope".to_vec(),
            system_fields: b"synthetic-invalid-version".to_vec(),
        })
        .collect();
    assert_eq!(corrupt.len(), 2);
    runtime.adapter_fetched(&rig.sdk, &rig.zone, &corrupt, &[]).unwrap();
    let projection = runtime
        .policy_projection(Lifecycle::Ready, rig.host.engine().generation().0)
        .unwrap();
    assert_eq!(projection.facts.summary.attention.get(&Attention::Refused), Some(&2));
    assert!(!projection.facts.summary.attention_known);
    assert_eq!(projection.evidence, evidence);
    rig.host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let status = driver.status();
    assert_eq!(status.state, StatusState::NeedsAttention);
    assert_eq!(status.attention.get(&Attention::Refused), Some(&2));
    rig.host.close().unwrap();
}

#[test]
fn k10_projection_stale_account_callback_cannot_reuse_prior_success_as_readiness() {
    use meguri_sync::cloud::{
        BindingState, IsolationReason,
        policy::{Attention, Lifecycle},
    };
    let rig = rig();
    k10_completed_exchange(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let evidence = rig.host.cloud_policy_driver().unwrap().status().evidence;
    runtime
        .adapter_account(Some(&rig.sdk), IdentityStatus::SwitchedAccounts)
        .unwrap();
    assert!(matches!(
        runtime.adapter_fetched(&rig.sdk, &rig.zone, &rig.records, &[]),
        Err(RuntimeError::StaleIncarnation)
    ));
    let projection = runtime
        .policy_projection(Lifecycle::Ready, rig.host.engine().generation().0)
        .unwrap();
    let binding = projection.facts.binding.as_ref().unwrap();
    assert_eq!(binding.state, BindingState::Isolated(IsolationReason::AccountChanged));
    assert!(!binding.usable && !binding.verified);
    assert!(!projection.facts.summary.attention_known);
    assert_eq!(projection.evidence, evidence);
    rig.host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let status = rig.host.cloud_policy_driver().unwrap().status();
    assert_eq!(status.state, StatusState::NeedsAttention);
    assert_eq!(status.attention.get(&Attention::Isolated), Some(&1));
    rig.host.close().unwrap();
}

#[test]
fn completed_exchange_keeps_original_pending_and_media_work_after_later_edits() {
    use meguri_sync::cloud::{
        adapter::{CollectionOutcome, CompletedExchange},
        policy::Lifecycle,
    };
    for before_completion in [false, true] {
        let rig = rig();
        let collection = rig.host.collection().unwrap();
        let media = rig.host.media().unwrap();
        let runtime = rig.host.cloud_runtime().unwrap();
        let driver = rig.host.cloud_policy_driver().unwrap();
        let run = command(
            rig.host
                .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
                .unwrap(),
            |kind| matches!(kind, ActionKind::RunExchange { .. }),
        );
        let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
            unreachable!()
        };
        let native = driver.capture_exchange(&run, &collection).unwrap();
        let edit = || {
            meguri_engine::decks::rename(&collection, rig.baseline_deck, "Newer pending edit".into()).unwrap();
            media
                .write_exact("pending.bin", b"synthetic media awaiting upload")
                .unwrap();
            runtime.consume_media_feed(&collection, &media).unwrap();
        };
        if before_completion {
            edit();
        }
        let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
        assert_eq!(completed.collection, CollectionOutcome::Succeeded);
        assert!(completed.result.selected_names.is_empty());
        let frozen = completed.work.as_ref().unwrap();
        assert!(frozen.held_pending_names.is_empty());
        assert_eq!(frozen.pending_unheld_names.is_empty(), !before_completion);
        assert_eq!(frozen.media.upload_files, Some(u64::from(before_completion)));
        assert_eq!(frozen.media.upload_bytes, Some(if before_completion { 31 } else { 0 }));
        assert_eq!(frozen.media.uploads_running, None);
        assert_eq!(frozen.media.remaining_download_ms, None);
        let encoded = serde_json::to_value(&completed).unwrap();
        assert_eq!(
            serde_json::from_value::<CompletedExchange>(encoded.clone()).unwrap(),
            completed
        );
        let mut legacy = encoded;
        legacy.as_object_mut().unwrap().remove("work");
        assert_eq!(serde_json::from_value::<CompletedExchange>(legacy).unwrap().work, None);
        if !before_completion {
            edit();
        }
        let current = runtime
            .policy_projection(Lifecycle::Ready, collection.generation().0)
            .unwrap();
        assert!(current.facts.summary.pending_unheld > 0);
        assert_eq!(current.facts.summary.media.upload_files, Some(1));
        let action = command(
            driver
                .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
                .unwrap(),
            |kind| matches!(kind, ActionKind::RecordExchange(_)),
        );
        let ActionKind::RecordExchange(record) = &action.action.kind else {
            unreachable!()
        };
        let original = driver.original_exchange_work_for_testing(&action).unwrap();
        assert_eq!(&original.work, frozen);
        assert_eq!(original.result, completed.result);
        assert_eq!(original.accounting, completed.accounting.clone().unwrap());
        assert!(driver.original_exchange_work_for_testing(&run).is_err());
        let mut invalid = action.clone();
        let ActionKind::RecordExchange(invalid_record) = &mut invalid.action.kind else {
            unreachable!()
        };
        invalid_record.ended_ms += 1;
        assert!(driver.original_exchange_work_for_testing(&invalid).is_err());
        let ExchangeOutcome::Succeeded {
            held_back,
            newer_pending,
            media: reported,
            ..
        } = &record.outcome
        else {
            panic!("successful exchange required")
        };
        assert_eq!(*held_back, 0);
        assert_eq!(*newer_pending, frozen.pending_unheld_names.len() as u64);
        assert_eq!(reported, &frozen.media);
        rig.host
            .execute_cloud_policy_core(&action, Duration::from_secs(10))
            .unwrap();
        assert_eq!(runtime.adapter_exchange_result(native), Some(completed));
        assert_eq!(
            std::fs::read(media.store().media_dir().join("pending.bin")).unwrap(),
            b"synthetic media awaiting upload"
        );
        rig.host.close().unwrap();
        assert!(driver.original_exchange_work_for_testing(&action).is_err());
    }
}

#[test]
fn exchange_finishes_when_selected_new_note_and_card_are_deleted_before_handoff() {
    use meguri_sync::cloud::adapter::CollectionOutcome;
    for restore in [false, true] {
        let rig = rig_with_root(false);
        fetch_first_root(&rig);
        let collection = rig.host.collection().unwrap();
        let runtime = rig.host.cloud_runtime().unwrap();
        let driver = rig.host.cloud_policy_driver().unwrap();
        let notetype = rig
            .records
            .iter()
            .find_map(|saved| {
                let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&saved.envelope).ok()? else {
                    return None;
                };
                match Payload::decode(entity.fields.kind, entity.payload.as_deref()?).ok()? {
                    Payload::Notetype(value) => Some(meguri_model::ids::NotetypeId(value.id)),
                    _ => None,
                }
            })
            .unwrap();
        let added = meguri_engine::notes::add(
            &collection,
            meguri_engine::notes::AddNote {
                notetype_id: notetype,
                fields: vec!["Created during a manual test".into(), "Public answer".into()],
                tags: Vec::new(),
                deck_id: rig.baseline_deck,
                guid: None,
                update_add_defaults: false,
            },
            meguri_engine::OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        assert_eq!(added.cards.len(), 1);
        let run = command(
            rig.host
                .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
                .unwrap(),
            |kind| matches!(kind, ActionKind::RunExchange { .. }),
        );
        let native = driver.capture_exchange(&run, &collection).unwrap();
        let selected = runtime.exchange_result(native).unwrap().selected_names;
        fetch_first_root(&rig);
        let new_rows = selected
            .iter()
            .filter(|name| {
                matches!(
                    runtime.entity_facts(name).unwrap().unwrap().kind,
                    codec::Kind::Note | codec::Kind::Card
                )
            })
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(new_rows.len(), 2);
        let other = selected
            .difference(&new_rows)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(other.len(), 1);
        for name in &other {
            assert_eq!(runtime.entity_facts(name).unwrap().unwrap().kind, codec::Kind::Config);
        }
        for name in &new_rows {
            let entity = runtime.entity_facts(name).unwrap().unwrap();
            assert!(!entity.sent && entity.server_erev == 0 && entity.in_flight.is_empty());
        }
        meguri_engine::notes::delete(&collection, vec![added.id], meguri_engine::OpSource::UserUndoable)
            .unwrap()
            .unwrap();
        for name in &new_rows {
            assert!(runtime.entity_facts(name).unwrap().is_none());
        }
        // Note creation also updates an ordinary config key. Sync that selected
        // work normally without ever handing the deleted note or card to the SDK.
        let call = runtime.start_manual_send_call(&rig.sdk).unwrap().unwrap();
        let batch = runtime.build_collection_batch(&rig.sdk, &collection, &other).unwrap();
        assert_eq!(batch.saves.len(), other.len());
        let saved = batch
            .saves
            .into_iter()
            .map(|record| Saved {
                name: record.name,
                envelope: record.envelope,
                system_fields: b"synthetic-config-save".to_vec(),
            })
            .collect::<Vec<_>>();
        runtime.enter_sent_handler(&rig.sdk, batch.id).unwrap();
        runtime
            .sent_results(&rig.sdk, &collection, batch.id, &saved, &[], 1)
            .unwrap();
        runtime.return_sent_handler(&rig.sdk).unwrap();
        runtime.finish_send_call(&rig.sdk, call.id).unwrap();
        if restore {
            collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
            for name in &new_rows {
                assert!(runtime.entity_facts(name).unwrap().is_some());
            }
        }
        let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
        assert_eq!(completed.result.acked_names, other);
        assert!(completed.result.acked_names.is_disjoint(&new_rows));
        assert_eq!(completed.result.dropped_names, new_rows);
        assert!(completed.result.absent_names.is_empty());
        assert_eq!(completed.collection, CollectionOutcome::Succeeded);
        assert!(completed.result.outstanding_names.is_empty());
        let work = completed.work.as_ref().unwrap();
        assert_eq!(
            work.pending_unheld_names.intersection(&new_rows).count(),
            if restore { 2 } else { 0 }
        );
        let accounting = completed.accounting.as_ref().unwrap();
        assert!(accounting.clean_names.is_empty() && accounting.incoming.is_empty());
        rig.host.close().unwrap();
    }
}

#[test]
fn exchange_records_clean_selected_entities_without_claiming_a_new_ack() {
    use meguri_sync::cloud::adapter::{CollectionOutcome, CompletedExchange};
    let rig = rig();
    let collection = rig.host.collection().unwrap();
    let runtime = rig.host.cloud_runtime().unwrap();
    let driver = rig.host.cloud_policy_driver().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Temporary local name".into()).unwrap();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |kind| matches!(kind, ActionKind::RunExchange { .. }),
    );
    let native = driver.capture_exchange(&run, &collection).unwrap();
    let selected = runtime.exchange_result(native).unwrap().selected_names;
    assert_eq!(selected.len(), 1);
    collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
    for name in &selected {
        let entity = runtime.entity_facts(name).unwrap().unwrap();
        assert!(entity.mark.is_none() && entity.server_erev > 0 && entity.in_flight.is_empty());
    }
    let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
    assert_eq!(completed.collection, CollectionOutcome::Succeeded);
    assert!(completed.result.acked_names.is_empty() && completed.result.dropped_names.is_empty());
    assert!(completed.result.outstanding_names.is_empty());
    assert_eq!(completed.accounting.as_ref().unwrap().clean_names, selected);
    let mut legacy = serde_json::to_value(&completed).unwrap();
    legacy.as_object_mut().unwrap().remove("accounting");
    assert_eq!(
        serde_json::from_value::<CompletedExchange>(legacy).unwrap().accounting,
        None
    );
    rig.host.close().unwrap();
}

#[test]
fn exchange_incoming_reasons_keep_their_original_accounting_after_completion() {
    use meguri_sync::cloud::{
        ApplyFailure, InboxReason,
        adapter::{
            CollectionOutcome, CompletedExchange, ExchangeIncoming, ExchangeIncomingEntry, ExchangeIncomingReason,
        },
    };
    let reasons = [
        (InboxReason::Ready, false, 1, 0, 0),
        (InboxReason::Generation, true, 0, 1, 0),
        (InboxReason::Reference, true, 0, 1, 0),
        (InboxReason::Structure, true, 0, 1, 0),
        (InboxReason::Format, false, 0, 0, 1),
        (InboxReason::Asset, false, 0, 0, 1),
        (
            InboxReason::ApplyFailed {
                failure: ApplyFailure::Apply,
                error: OpaqueBytes(vec![1]),
            },
            false,
            0,
            0,
            1,
        ),
        (
            InboxReason::ApplyFailed {
                failure: ApplyFailure::Rekey,
                error: OpaqueBytes(vec![2]),
            },
            false,
            0,
            0,
            1,
        ),
    ];
    for (reason, succeeded, ready, waiting, blocked) in reasons {
        let rig = rig();
        let runtime = rig.host.cloud_runtime().unwrap();
        let driver = rig.host.cloud_policy_driver().unwrap();
        let run = command(
            rig.host
                .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
                .unwrap(),
            |kind| matches!(kind, ActionKind::RunExchange { .. }),
        );
        let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
            unreachable!()
        };
        let native = driver.capture_exchange(&run, &rig.host.collection().unwrap()).unwrap();
        let (deck, original, _) = incoming_deck(&rig);
        let binding = runtime.status().unwrap().binding.unwrap().id;
        let entries = runtime.inbox_entries(binding).unwrap();
        assert_eq!(entries.len(), 1);
        runtime
            .set_inbox_reason_for_testing(binding, entries[0].id, reason.clone())
            .unwrap();
        let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
        assert!(completed.result.selected_names.is_empty());
        assert_eq!(
            completed.accounting.as_ref().unwrap().incoming,
            vec![ExchangeIncomingEntry {
                record_name: entries[0].entry.record_name.clone(),
                reason: ExchangeIncomingReason::from(&reason),
            }]
        );
        assert_eq!(
            completed.incoming,
            Some(ExchangeIncoming {
                ready,
                waiting,
                blocked
            }),
            "{reason:?}"
        );
        assert_eq!(
            completed.collection,
            if succeeded {
                CollectionOutcome::Succeeded
            } else {
                CollectionOutcome::Incomplete
            },
            "{reason:?}"
        );
        assert_eq!(runtime.adapter_exchange_result(native), Some(completed.clone()));
        let encoded = serde_json::to_value(&completed).unwrap();
        assert_eq!(
            serde_json::from_value::<CompletedExchange>(encoded.clone()).unwrap(),
            completed
        );
        let mut legacy = encoded;
        legacy.as_object_mut().unwrap().remove("incoming");
        assert_eq!(
            serde_json::from_value::<CompletedExchange>(legacy).unwrap().incoming,
            None
        );

        // A later intake classification must not rewrite the ended exchange.
        runtime
            .set_inbox_reason_for_testing(
                binding,
                entries[0].id,
                if succeeded {
                    InboxReason::Format
                } else {
                    InboxReason::Generation
                },
            )
            .unwrap();
        let action = command(
            driver
                .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
                .unwrap(),
            |kind| matches!(kind, ActionKind::RecordExchange(_)),
        );
        let ActionKind::RecordExchange(record) = &action.action.kind else {
            unreachable!()
        };
        match &record.outcome {
            ExchangeOutcome::Succeeded {
                selected,
                incoming_waiting,
                ..
            } => {
                assert!(succeeded);
                assert_eq!(*selected, 0);
                assert_eq!(*incoming_waiting, waiting);
            }
            ExchangeOutcome::Incomplete {
                outstanding,
                blocked: actual,
            } => {
                assert!(!succeeded);
                assert_eq!(*outstanding, 0);
                assert_eq!(*actual, blocked);
            }
            other => panic!("unexpected exchange outcome: {other:?}"),
        }
        rig.host
            .execute_cloud_policy_core(&action, Duration::from_secs(10))
            .unwrap();
        assert_eq!(deck_row(&rig, deck), original);
        assert_eq!(runtime.inbox_entries(binding).unwrap().len(), 1);
        rig.host.close().unwrap();
    }
}

#[test]
fn exchange_selection_retains_local_edits_waiting_for_incoming_work() {
    use meguri_sync::cloud::{InboxReason, adapter::CollectionOutcome};
    for reason in [InboxReason::Generation, InboxReason::Reference, InboxReason::Structure] {
        let rig = rig();
        let collection = rig.host.collection().unwrap();
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Local waiting change".into()).unwrap();
        let local = deck_row(&rig, rig.baseline_deck.0);
        let runtime = rig.host.cloud_runtime().unwrap();
        let (deck, _, _) = incoming_deck(&rig);
        assert_eq!(deck, rig.baseline_deck.0);
        let binding = runtime.status().unwrap().binding.unwrap().id;
        let entries = runtime.inbox_entries(binding).unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        runtime
            .set_inbox_reason_for_testing(binding, entry.id, reason.clone())
            .unwrap();
        let driver = rig.host.cloud_policy_driver().unwrap();
        let run = command(
            rig.host
                .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
                .unwrap(),
            |kind| matches!(kind, ActionKind::RunExchange { .. }),
        );
        let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
            unreachable!()
        };
        let native = driver.capture_exchange(&run, &collection).unwrap();
        let captured = runtime.exchange_result(native).unwrap();
        assert!(!runtime.tracking_sendables().contains_key(&entry.entry.record_name));
        assert_eq!(captured.selected_names, [entry.entry.record_name.clone()].into());
        assert_eq!(captured.outstanding_names, captured.selected_names);
        let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
        assert_eq!(completed.collection, CollectionOutcome::Incomplete, "{reason:?}");
        assert_eq!(completed.result.outstanding_names, captured.selected_names);
        assert!(completed.result.acked_names.is_empty());
        let action = command(
            driver
                .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
                .unwrap(),
            |kind| matches!(kind, ActionKind::RecordExchange(_)),
        );
        let ActionKind::RecordExchange(record) = &action.action.kind else {
            unreachable!()
        };
        assert_eq!(
            record.outcome,
            ExchangeOutcome::Incomplete {
                outstanding: 1,
                blocked: 0
            }
        );
        assert_eq!(deck_row(&rig, deck), local);
        assert_eq!(runtime.inbox_entries(binding).unwrap()[0].entry.reason, reason);
        rig.host
            .execute_cloud_policy_core(&action, Duration::from_secs(10))
            .unwrap();
        rig.host.close().unwrap();
    }
}

#[test]
fn exchange_blocked_names_keep_acknowledgement_precedence_and_durable_counts() {
    use meguri_sync::cloud::{InboxReason, adapter::CollectionOutcome};
    for refused in [false, true] {
        for acknowledged in [false, true] {
            let rig = rig();
            let collection = rig.host.collection().unwrap();
            meguri_engine::decks::rename(&collection, rig.baseline_deck, "Local blocked change".into()).unwrap();
            let local = deck_row(&rig, rig.baseline_deck.0);
            let runtime = rig.host.cloud_runtime().unwrap();
            let driver = rig.host.cloud_policy_driver().unwrap();
            let run = command(
                rig.host
                    .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
                    .unwrap(),
                |kind| matches!(kind, ActionKind::RunExchange { .. }),
            );
            let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
                unreachable!()
            };
            let native = driver.capture_exchange(&run, &collection).unwrap();
            let selection = runtime.exchange_result(native).unwrap();
            assert_eq!(selection.selected_names.len(), 1);
            let name = selection.selected_names.iter().next().unwrap().clone();
            let batch = acknowledged.then(|| {
                runtime
                    .build_collection_batch(&rig.sdk, &collection, &selection.selected_names)
                    .unwrap()
            });
            let binding = runtime.status().unwrap().binding.unwrap().id;
            if refused {
                let invalid = Saved {
                    name: name.clone(),
                    envelope: vec![1],
                    system_fields: vec![2],
                };
                runtime.adapter_fetched(&rig.sdk, &rig.zone, &[invalid], &[]).unwrap();
                assert_eq!(runtime.refused_entries(binding).unwrap().len(), 1);
            } else {
                incoming_deck(&rig);
                let entries = runtime.inbox_entries(binding).unwrap();
                assert_eq!(entries.len(), 1);
                runtime
                    .set_inbox_reason_for_testing(binding, entries[0].id, InboxReason::Format)
                    .unwrap();
            }
            if let Some(batch) = batch {
                assert_eq!(batch.saves.len(), 1);
                let saved = Saved {
                    name: name.clone(),
                    envelope: batch.saves[0].envelope.clone(),
                    system_fields: vec![3],
                };
                runtime.enter_sent_handler(&rig.sdk, batch.id).unwrap();
                runtime
                    .sent_results(&rig.sdk, &collection, batch.id, &[saved], &[], 1)
                    .unwrap();
                runtime.return_sent_handler(&rig.sdk).unwrap();
            }
            let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
            assert!(completed.result.outstanding_names.is_empty());
            assert_eq!(completed.result.blocked_names.contains(&name), !acknowledged);
            assert_eq!(completed.result.acked_names.contains(&name), acknowledged);
            let mut legacy = serde_json::to_value(&completed).unwrap();
            legacy["result"].as_object_mut().unwrap().remove("blocked_names");
            let legacy: meguri_sync::cloud::adapter::CompletedExchange = serde_json::from_value(legacy).unwrap();
            assert!(legacy.result.blocked_names.is_empty());
            let action = command(
                driver
                    .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
                    .unwrap(),
                |kind| matches!(kind, ActionKind::RecordExchange(_)),
            );
            let ActionKind::RecordExchange(record) = &action.action.kind else {
                unreachable!()
            };
            let blocked = u64::from(!acknowledged) + u64::from(!refused);
            if blocked == 0 {
                assert_eq!(completed.collection, CollectionOutcome::Succeeded);
                assert!(matches!(record.outcome, ExchangeOutcome::Succeeded { selected: 1, .. }));
            } else {
                assert_eq!(completed.collection, CollectionOutcome::Incomplete);
                assert_eq!(
                    record.outcome,
                    ExchangeOutcome::Incomplete {
                        outstanding: 0,
                        blocked
                    }
                );
            }
            rig.host
                .execute_cloud_policy_core(&action, Duration::from_secs(10))
                .unwrap();
            assert_eq!(runtime.adapter_exchange_result(native), Some(completed));
            assert_eq!(deck_row(&rig, rig.baseline_deck.0), local);
            rig.host.close().unwrap();
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CompletionClockEffect {
    Read,
    Pause,
    Isolate,
}

#[derive(Debug)]
struct ReentrantCompletionClock {
    runtime: std::sync::Weak<meguri_sync::cloud::runtime::Runtime>,
    clock: Arc<StepClock>,
    effect: CompletionClockEffect,
    calls: std::sync::atomic::AtomicUsize,
}
impl Clock for ReentrantCompletionClock {
    fn now_ms(&self) -> i64 {
        let runtime = self.runtime.upgrade().unwrap();
        // Same-thread provider reentry takes both the state and tracking guards.
        runtime.status().unwrap();
        runtime.adapter_guard();
        match self.effect {
            CompletionClockEffect::Read => {}
            CompletionClockEffect::Pause => runtime.adapter_pause(true).unwrap(),
            CompletionClockEffect::Isolate => runtime.adapter_stop_syncing().unwrap(),
        }
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.clock.now_ms()
    }
    fn utc_offset_at(&self, at_ms: i64) -> UtcOffset {
        self.clock.utc_offset_at(at_ms)
    }
    fn zone_id(&self) -> String {
        self.clock.zone_id()
    }
}

#[test]
fn completion_clock_reenters_runtime_and_rechecks_live_completion_facts() {
    const CHILD: &str = "MEGURI_POLICY_COMPLETION_CLOCK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // A regressed mutex deadlock must fail this test without hanging the suite
        // or waiting for Runtime/Host destructors on the blocked child thread.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "completion_clock_reenters_runtime_and_rechecks_live_completion_facts",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        for _ in 0..300 {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "reentrant completion child failed: {status}");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("reentrant completion child did not finish within the 30-second watchdog");
    }
    for effect in [
        CompletionClockEffect::Read,
        CompletionClockEffect::Pause,
        CompletionClockEffect::Isolate,
    ] {
        let rig = rig();
        let driver = rig.host.cloud_policy_driver().unwrap();
        let run = command(
            rig.host
                .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
                .unwrap(),
            |k| matches!(k, ActionKind::RunExchange { .. }),
        );
        let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
            unreachable!()
        };
        let native = driver.capture_exchange(&run, &rig.host.collection().unwrap()).unwrap();
        let runtime = rig.host.cloud_runtime().unwrap();
        let fetch = runtime
            .adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into(), rig.zone.clone()])
            .unwrap();
        runtime
            .adapter_will_fetch(&rig.sdk, "reentrant-clock".into(), true)
            .unwrap();
        runtime.adapter_did_fetch(&rig.sdk, "reentrant-clock", true).unwrap();
        assert!(runtime.adapter_finish_fetch(&rig.sdk, fetch).unwrap());
        runtime.adapter_exchange_fetch(&rig.sdk, true).unwrap();
        let clock = Arc::new(ReentrantCompletionClock {
            runtime: Arc::downgrade(runtime),
            clock: rig.clock.clone(),
            effect,
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        runtime.attach_adapter_clock(clock.clone());
        rig.clock.step_ms(10_000);
        let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
        use meguri_sync::cloud::adapter::CollectionOutcome;
        assert_eq!(
            completed.collection,
            match effect {
                CompletionClockEffect::Read => CollectionOutcome::Succeeded,
                CompletionClockEffect::Pause => CollectionOutcome::Cancelled,
                CompletionClockEffect::Isolate => CollectionOutcome::Failed,
            }
        );
        assert_eq!(runtime.adapter_exchange_result(native), Some(completed.clone()));
        rig.clock.step_ms(90_000);
        assert_eq!(
            runtime
                .adapter_complete_exchange(&rig.sdk, native, false, Some(CollectionOutcome::Failed))
                .unwrap(),
            completed
        );
        assert_eq!(clock.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        if matches!(effect, CompletionClockEffect::Read) {
            let record = command(
                driver
                    .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
                    .unwrap(),
                |k| matches!(k, ActionKind::RecordExchange(_)),
            );
            rig.host
                .execute_cloud_policy_core(&record, Duration::from_secs(10))
                .unwrap();
            let evidence = driver.status().evidence;
            assert_eq!(evidence.last_completed.unwrap().ended_ms, 1_894_701_610_000);
            assert!(evidence.last_attempt.is_none());
        }
        rig.host.close().unwrap();
    }
}

#[test]
fn native_capture_uses_committed_sequence_after_apply_finishes() {
    let rig = rig();
    let (id, before, _) = incoming_deck(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let driver = rig.host.cloud_policy_driver().unwrap();
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |kind| matches!(kind, ActionKind::RunExchange { .. }),
    );
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else {
        unreachable!()
    };
    assert_eq!(
        driver.native_exchange_readiness(&run, false, true).unwrap(),
        (true, None)
    );
    let (arrived, arrival) = std::sync::mpsc::sync_channel(1);
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let released = std::sync::Mutex::new(released);
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == IncomingStage::ApplyAfterCommit {
            arrived.send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
        }
    });
    // Always unblock the real worker, including an assertion failure on this thread.
    struct ReleaseOnDrop(std::sync::mpsc::SyncSender<()>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            let _ = self.0.try_send(());
        }
    }
    std::thread::scope(|scope| {
        let collection = rig.host.collection().unwrap();
        let sdk = &rig.sdk;
        let applying = scope.spawn(move || runtime.drain_native_incoming(&collection, sdk, Duration::from_secs(10)));
        let release = ReleaseOnDrop(release);
        arrival.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_ne!(deck_row(&rig, id), before, "the actual collection commit occurred");
        let binding = runtime.status().unwrap().binding.unwrap().id;
        let intents = runtime.apply_intents(binding).unwrap();
        assert_eq!(intents.len(), 1);
        assert!(intents[0].result_seq.is_some(), "committed but not settled");
        assert!(!runtime.adapter_exchange_active());
        rig.host
            .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
            .unwrap();
        let state = driver.state();
        assert_eq!(state.facts.summary.unsettled_intents, 0);
        assert!(state.facts.summary.apply_in_progress);
        assert!(
            state.gate_open() && state.engine_runs(),
            "the separate apply exclusion is necessary"
        );
        assert_eq!(
            driver.native_exchange_readiness(&run, false, true).unwrap(),
            (false, None)
        );
        assert!(
            matches!(
                driver.complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation()),
                Err(DriverError::UnqualifiedCompletion)
            ),
            "no Runtime capture was installed while readiness waited"
        );
        drop(release);
        applying.join().unwrap().unwrap();
    });
    rig.host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    assert!(!driver.state().facts.summary.apply_in_progress);
    assert_eq!(
        driver.native_exchange_readiness(&run, false, true).unwrap(),
        (true, None)
    );
    assert!(!runtime.adapter_exchange_active());
    let collection = rig.host.collection().unwrap();
    let expected_sequence = collection.last_seq().0;
    let runtime_exchange = driver.capture_exchange(&run, &collection).unwrap();
    let selection = runtime.exchange_result(runtime_exchange).unwrap();
    assert_eq!(selection.boundary_seq, expected_sequence);
    assert!(runtime.adapter_exchange_active());
    assert_eq!(driver.capture_exchange(&run, &collection).unwrap(), runtime_exchange);
    assert_eq!(runtime.exchange_result(runtime_exchange), Some(selection.clone()));
    assert!(runtime.adapter_exchange_active());

    use meguri_sync::cloud::adapter::CollectionOutcome;
    let completed = runtime
        .adapter_complete_exchange(&rig.sdk, runtime_exchange, false, Some(CollectionOutcome::Cancelled))
        .unwrap();
    assert_eq!(completed.id, runtime_exchange);
    assert_eq!(completed.collection, CollectionOutcome::Cancelled);
    assert_eq!(completed.result, selection);
    assert!(!runtime.adapter_exchange_active());
    let record = command(
        driver
            .complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation())
            .unwrap(),
        |kind| matches!(kind, ActionKind::RecordExchange(_)),
    );
    let ActionKind::RecordExchange(record) = record.action.kind else {
        unreachable!()
    };
    assert_eq!(record.exchange, exchange);
    assert_eq!(record.outcome, ExchangeOutcome::Cancelled);
    rig.host.close().unwrap();
}

fn first_manual_capture(rig: &Rig) -> u64 {
    let run = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |kind| matches!(kind, ActionKind::RunExchange { .. }),
    );
    let driver = rig.host.cloud_policy_driver().unwrap();
    assert_eq!(
        driver.native_exchange_readiness(&run, false, true).unwrap(),
        (true, None)
    );
    driver.capture_exchange(&run, &rig.host.collection().unwrap()).unwrap()
}

fn fetch_first_root(rig: &Rig) {
    let runtime = rig.host.cloud_runtime().unwrap();
    let fetch = runtime
        .adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into()])
        .unwrap();
    runtime
        .adapter_will_fetch(&rig.sdk, "first-manual".into(), true)
        .unwrap();
    runtime
        .adapter_fetch_zone(&rig.sdk, "meguri-root".into(), true, false)
        .unwrap();
    runtime
        .adapter_fetched(&rig.sdk, "meguri-root", std::slice::from_ref(&rig.active), &[])
        .unwrap();
    runtime
        .adapter_fetch_zone(&rig.sdk, "meguri-root".into(), false, false)
        .unwrap();
    runtime.adapter_did_fetch(&rig.sdk, "first-manual", true).unwrap();
    assert!(runtime.adapter_finish_fetch(&rig.sdk, fetch).unwrap());
    runtime.adapter_exchange_fetch(&rig.sdk, true).unwrap();
}

#[test]
fn first_manual_capture_before_root_selects_pending_revisions_without_admitting_sends() {
    let rig = rig_with_root(false);
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    assert!(!runtime.sdk_root_confirmed());
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "First manual revision".into()).unwrap();
    let name = runtime
        .entity_facts_page(codec::Kind::Deck, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == codec::id_key(rig.baseline_deck.0))
        .unwrap()
        .name;
    let selected_rev = runtime.entity_facts(&name).unwrap().unwrap().local_rev;
    let boundary = collection.last_seq().0;
    let id = first_manual_capture(&rig);
    let selected = runtime.exchange_result(id).unwrap();
    assert_eq!(selected.boundary_seq, boundary);
    assert_eq!(selected.selected_names, [name.clone()].into());
    assert_eq!(selected.outstanding_names, selected.selected_names);
    assert!(selected.acked_names.is_empty());
    assert!(runtime.tracking_sendables().is_empty());
    assert!(
        runtime
            .take_pending_updates(&rig.sdk)
            .unwrap()
            .iter()
            .all(|u| u.adds.is_empty())
    );
    assert!(runtime.start_manual_send_call(&rig.sdk).unwrap().is_none());
    assert!(
        runtime
            .build_collection_batch(&rig.sdk, &collection, &selected.selected_names)
            .unwrap()
            .saves
            .is_empty()
    );
    runtime.enter_sent_handler(&rig.sdk, 991).unwrap();
    // A previously saved baseline record has no prepared flight for this edit.
    // Reject the actual record, rather than treating an empty receipt as an ACK.
    let no_flight = rig.records.iter().find(|saved| saved.name == name).unwrap();
    let before_refusal = runtime.entity_facts(&name).unwrap();
    assert!(
        runtime
            .sent_results(&rig.sdk, &collection, 991, std::slice::from_ref(no_flight), &[], 1)
            .is_err()
    );
    assert!(runtime.entity_facts(&name).unwrap() == before_refusal);
    assert_eq!(runtime.exchange_result(id), Some(selected.clone()));
    assert!(!runtime.sdk_root_confirmed());
    runtime.return_sent_handler(&rig.sdk).unwrap();

    // Starting review without an answer leaves no newest answer to hold later edits.
    let meguri_engine::session::StartResult::Started { session_id, .. } =
        rig.host.review().start(rig.baseline_deck, None).unwrap()
    else {
        panic!("real review must start");
    };
    // An active review still cannot create another capture before proof.
    assert!(runtime.sync_now(&rig.sdk, &collection).is_err());
    // A later edit keeps the captured membership and boundary while root is absent.
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Later preproof revision".into()).unwrap();
    assert!(runtime.entity_facts(&name).unwrap().unwrap().local_rev > selected_rev);
    assert_eq!(runtime.exchange_result(id), Some(selected.clone()));
    fetch_first_root(&rig);
    assert!(runtime.sdk_root_confirmed());
    assert_eq!(runtime.exchange_result(id), Some(selected));
    assert!(runtime.tracking_sendables().contains_key(&name));
    assert!(
        runtime
            .take_pending_updates(&rig.sdk)
            .unwrap()
            .iter()
            .any(|u| u.adds.contains_key(&name))
    );
    let call = runtime.start_manual_send_call(&rig.sdk).unwrap().unwrap();
    let batch = runtime
        .build_collection_batch(&rig.sdk, &collection, &[name.clone()].into())
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    let codec::DecodeOutcome::Supported(sent_entity) = codec::Entity::decode(&batch.saves[0].envelope).unwrap() else {
        panic!("production deck envelope required");
    };
    let Payload::Deck(sent_deck) =
        Payload::decode(sent_entity.fields.kind, sent_entity.payload.as_deref().unwrap()).unwrap()
    else {
        panic!("production deck payload required");
    };
    assert_eq!(sent_deck.name.as_slice(), b"Later preproof revision");
    let sent_rev = runtime.entity_facts(&name).unwrap().unwrap().local_rev;
    assert!(sent_rev >= selected_rev);
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Newer than the captured flight".into()).unwrap();
    let saved = Saved {
        name: name.clone(),
        envelope: batch.saves[0].envelope.clone(),
        system_fields: b"synthetic-first-manual-save".to_vec(),
    };
    runtime.enter_sent_handler(&rig.sdk, batch.id).unwrap();
    runtime
        .sent_results(&rig.sdk, &collection, batch.id, &[saved], &[], 1)
        .unwrap();
    runtime.return_sent_handler(&rig.sdk).unwrap();
    runtime.finish_send_call(&rig.sdk, call.id).unwrap();
    let facts = runtime.entity_facts(&name).unwrap().unwrap();
    assert_eq!(facts.acked_rev, sent_rev);
    assert!(facts.local_rev > facts.acked_rev && facts.mark.is_some());
    let result = runtime.exchange_result(id).unwrap();
    assert!(result.acked_names.contains(&name) && result.outstanding_names.is_empty());
    assert!(runtime.tracking_sendables().contains_key(&name));
    rig.host.review().end(session_id).unwrap();
    runtime.poll_review(&rig.sdk).unwrap();
    assert!(runtime.tracking_sendables().contains_key(&name));
    rig.host.close().unwrap();
}

#[test]
fn pre_capture_save_does_not_ack_an_identical_later_handoff() {
    use meguri_sync::cloud::adapter::CollectionOutcome;

    for newer_edit in [false, true] {
        let rig = rig();
        let runtime = rig.host.cloud_runtime().unwrap();
        let collection = rig.host.collection().unwrap();
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Before manual capture".into()).unwrap();
        let pending = runtime.tracking_sendables().into_keys().collect();
        let original = runtime.build_collection_batch(&rig.sdk, &collection, &pending).unwrap();
        assert_eq!(original.saves.len(), 1);
        let saved = &original.saves[0];
        let name = &saved.name;
        let earlier = runtime.entity_facts(name).unwrap().unwrap().in_flight[0].image_seq;

        // Selecting the deck advances the boundary without changing its image.
        meguri_engine::decks::select(&collection, rig.baseline_deck).unwrap();
        let id = first_manual_capture(&rig);
        let selection = runtime.exchange_result(id).unwrap();
        assert!(earlier < selection.boundary_seq);
        assert!(selection.selected_names.contains(name));
        assert_eq!(selection.selected_names.len(), 2);
        let later = runtime
            .build_collection_batch(&rig.sdk, &collection, &selection.selected_names)
            .unwrap();
        assert_eq!(later.saves.len(), 2);
        let repeated = later.saves.iter().find(|input| &input.name == name).unwrap();
        assert_eq!(repeated.envelope, saved.envelope);
        let first_image = runtime.smoke_batch_for_testing(&rig.sdk, original.id).unwrap();
        let later_image = runtime.smoke_batch_for_testing(&rig.sdk, later.id).unwrap();
        assert_eq!(first_image.save_images[0].image_seq, earlier);
        let repeated_image = later_image
            .save_images
            .iter()
            .find(|image| &image.name == name)
            .unwrap();
        assert_eq!(repeated_image.image_seq, selection.boundary_seq);
        assert!(!first_image.save_images[0].held && !repeated_image.held);
        if newer_edit {
            meguri_engine::decks::rename(&collection, rig.baseline_deck, "After manual capture".into()).unwrap();
        }

        let reply = Saved {
            name: name.clone(),
            envelope: saved.envelope.clone(),
            system_fields: b"original-save-reply".to_vec(),
        };
        runtime.enter_sent_handler(&rig.sdk, original.id).unwrap();
        runtime
            .sent_results(&rig.sdk, &collection, original.id, &[reply], &[], 1)
            .unwrap();
        runtime.return_sent_handler(&rig.sdk).unwrap();
        let result = runtime.exchange_result(id).unwrap();
        assert!(result.acked_names.is_empty());
        assert_eq!(result.outstanding_names, selection.selected_names);

        let other = later.saves.iter().find(|input| &input.name != name).unwrap();
        let reply = Saved {
            name: other.name.clone(),
            envelope: other.envelope.clone(),
            system_fields: b"later-save-reply".to_vec(),
        };
        runtime.enter_sent_handler(&rig.sdk, later.id).unwrap();
        runtime
            .sent_results(&rig.sdk, &collection, later.id, &[reply], &[], 2)
            .unwrap();
        runtime.return_sent_handler(&rig.sdk).unwrap();
        let completed = runtime.adapter_complete_exchange(&rig.sdk, id, true, None).unwrap();
        assert_eq!(completed.result.acked_names, [other.name.clone()].into());
        assert_eq!(completed.result.outstanding_names.contains(name), newer_edit);
        assert_eq!(
            completed.collection,
            if newer_edit {
                CollectionOutcome::Incomplete
            } else {
                CollectionOutcome::Succeeded
            }
        );
        rig.host.close().unwrap();
    }
}

#[test]
fn lost_save_credits_its_original_image_through_conflict_and_fetch() {
    for discover_by_fetch in [false, true] {
        for before_capture in [false, true] {
            let rig = rig();
            let runtime = rig.host.cloud_runtime().unwrap();
            let collection = rig.host.collection().unwrap();
            meguri_engine::decks::rename(&collection, rig.baseline_deck, "First upload".into()).unwrap();
            let names: std::collections::BTreeSet<_> = runtime.tracking_sendables().into_keys().collect();
            assert_eq!(names.len(), 1);
            let name = names.first().unwrap();
            let mut exchange = (!before_capture).then(|| first_manual_capture(&rig));
            let batch = runtime.build_collection_batch(&rig.sdk, &collection, &names).unwrap();
            let [input] = batch.saves.as_slice() else {
                panic!("expected one real upload")
            };
            let server = Saved {
                name: input.name.clone(),
                envelope: input.envelope.clone(),
                system_fields: b"landed-version".to_vec(),
            };
            let flight = runtime.entity_facts(name).unwrap().unwrap().in_flight.remove(0);
            if before_capture {
                meguri_engine::decks::select(&collection, rig.baseline_deck).unwrap();
                exchange = Some(first_manual_capture(&rig));
            }
            let id = exchange.unwrap();
            assert_eq!(
                flight.image_seq < runtime.exchange_result(id).unwrap().boundary_seq,
                before_capture
            );
            meguri_engine::decks::rename(&collection, rig.baseline_deck, "Local edit after capture".into()).unwrap();
            runtime.enable_smoke_acknowledgements_for_testing();
            let server_digest = codec::sha256(&server.envelope);
            let observations = if discover_by_fetch {
                runtime.adapter_fetched(&rig.sdk, &rig.zone, &[server], &[]).unwrap();
                runtime
                    .drain_native_incoming(&collection, &rig.sdk, Duration::from_secs(10))
                    .unwrap();
                let found = runtime.smoke_acknowledgements_for_testing(None).unwrap();
                assert!(runtime.smoke_acknowledgements_for_testing(None).unwrap().is_empty());
                found
            } else {
                let retry = runtime.build_collection_batch(&rig.sdk, &collection, &names).unwrap();
                assert_eq!(retry.saves.len(), 1);
                runtime.enter_sent_handler(&rig.sdk, retry.id).unwrap();
                assert_eq!(
                    runtime
                        .sent_results_with_conflicts(&rig.sdk, &collection, retry.id, &[], &[server], &[], 1)
                        .unwrap(),
                    1
                );
                let found = runtime
                    .smoke_acknowledgements_for_testing(Some((&rig.sdk, retry.id)))
                    .unwrap();
                assert!(
                    runtime
                        .smoke_acknowledgements_for_testing(Some((&rig.sdk, batch.id)))
                        .is_err()
                );
                runtime.return_sent_handler(&rig.sdk).unwrap();
                assert!(
                    runtime
                        .smoke_acknowledgements_for_testing(Some((&rig.sdk, retry.id)))
                        .is_err()
                );
                found
            };
            let [observation] = observations.as_slice() else {
                panic!("expected one original acknowledgement")
            };
            assert_eq!(&observation.name, name);
            assert_eq!(observation.sdk, rig.sdk);
            assert_eq!(observation.envelope_digest, server_digest);
            assert_eq!(observation.image_seq, flight.image_seq);
            assert_eq!(observation.held, flight.held);
            let tracked = runtime.entity_facts(name).unwrap().unwrap();
            assert_eq!(tracked.acked_rev, flight.local_rev);
            assert!(tracked.local_rev > tracked.acked_rev && tracked.mark.is_some());
            let result = runtime.exchange_result(id).unwrap();
            assert_eq!(result.acked_names.contains(name), !before_capture);
            assert_eq!(result.outstanding_names.contains(name), before_capture);
            rig.host.close().unwrap();
        }
    }
}

#[test]
fn saved_flight_during_real_reconcile_scan_preserves_newer_edit_and_capture_accounting() {
    use meguri_sync::cloud::runtime::ScanPausePoint;
    use std::sync::mpsc;

    let rig = rig_with_root(false);
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    let media = rig.host.media().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Captured before held scan".into()).unwrap();
    let id = first_manual_capture(&rig);
    let selection = runtime.exchange_result(id).unwrap();
    assert_eq!(selection.selected_names.len(), 1);
    let name = selection.selected_names.iter().next().unwrap().clone();
    fetch_first_root(&rig);
    let call = runtime.start_manual_send_call(&rig.sdk).unwrap().unwrap();
    let batch = runtime
        .build_collection_batch(&rig.sdk, &collection, &selection.selected_names)
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    assert_eq!(batch.saves[0].name, name);
    runtime.send_call_request(&rig.sdk, call.id, batch.id).unwrap();
    let prepared = runtime.entity_facts(&name).unwrap().unwrap();
    assert_eq!(prepared.in_flight.len(), 1);
    let flight = prepared.in_flight[0].clone();
    assert_eq!(flight.local_rev, prepared.local_rev);
    let saved = Saved {
        name: name.clone(),
        envelope: batch.saves[0].envelope.clone(),
        system_fields: b"synthetic-held-scan-save".to_vec(),
    };
    meguri_engine::decks::rename(
        &collection,
        rig.baseline_deck,
        "Newer local revision during receipt".into(),
    )
    .unwrap();
    let newer = runtime.entity_facts(&name).unwrap().unwrap();
    let newer_row = deck_row(&rig, rig.baseline_deck.0);
    assert!(newer.local_rev > flight.local_rev);
    assert_eq!(runtime.exchange_result(id), Some(selection.clone()));

    std::thread::scope(|threads| {
        // Keep the resume sender inside the scope body so unwinding disconnects
        // the existing bounded pause before scoped workers are joined.
        let (reached, wait) = mpsc::channel();
        let (resume, go) = mpsc::channel();
        runtime
            .arm_note_scan_pause_for_testing(ScanPausePoint::BeforeSnapshot, reached, go)
            .unwrap();
        let scanning = &collection;
        let scanning_media = &media;
        let scan = threads.spawn(move || runtime.reconcile_binding(scanning, scanning_media, Duration::from_secs(10)));
        wait.recv_timeout(Duration::from_secs(10)).unwrap();
        let status = runtime.status().unwrap();
        assert!(status.scan_running && !status.scan_completed_this_open);
        runtime.validate_caller(&rig.sdk).unwrap();
        assert!(runtime.sdk_root_confirmed());
        assert!(runtime.start_manual_send_call(&rig.sdk).unwrap().is_none());
        assert!(
            runtime
                .build_collection_batch(&rig.sdk, &collection, &selection.selected_names)
                .unwrap()
                .saves
                .is_empty()
        );
        runtime.enter_sent_handler(&rig.sdk, batch.id).unwrap();
        assert!(
            runtime
                .sent_results(
                    &rig.sdk,
                    &collection,
                    batch.id + 1,
                    std::slice::from_ref(&saved),
                    &[],
                    1
                )
                .is_err()
        );
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&saved.envelope).unwrap() else {
            panic!("production flight envelope required");
        };
        let mut fields = entity.fields;
        fields.writer[0] ^= 1;
        let foreign = Saved {
            name: name.clone(),
            envelope: codec::Entity::build(fields, entity.payload.as_deref().unwrap())
                .unwrap()
                .encode()
                .unwrap(),
            system_fields: saved.system_fields.clone(),
        };
        assert!(
            runtime
                .sent_results(&rig.sdk, &collection, batch.id, &[foreign], &[], 1)
                .is_err()
        );
        assert!(runtime.entity_facts(&name).unwrap().as_ref() == Some(&newer));
        assert_eq!(runtime.exchange_result(id), Some(selection.clone()));

        runtime
            .sent_results(&rig.sdk, &collection, batch.id, std::slice::from_ref(&saved), &[], 1)
            .unwrap();
        assert!(runtime.status().unwrap().scan_running);
        let acked = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(acked.acked_rev, flight.local_rev);
        assert_eq!(acked.server_erev, flight.erev);
        assert_eq!(acked.acked_payload_digest, flight.payload_digest);
        assert_eq!(acked.acked_system_fields, saved.system_fields);
        assert!(acked.in_flight.is_empty());
        assert_eq!(acked.local_rev, newer.local_rev);
        assert!(acked.payload == newer.payload && acked.mark.is_some());
        assert!(acked.base_payload.as_ref() == Some(flight.payload.canonical().expect("diagnostic flight payload")));
        let result = runtime.exchange_result(id).unwrap();
        assert_eq!(result.boundary_seq, selection.boundary_seq);
        assert_eq!(result.selected_names, selection.selected_names);
        assert_eq!(result.acked_names, [name.clone()].into());
        assert!(result.outstanding_names.is_empty());
        runtime.return_sent_handler(&rig.sdk).unwrap();
        runtime
            .sdk_saved_removed(&rig.sdk, batch.id, std::slice::from_ref(&name))
            .unwrap();
        runtime.finish_send_call(&rig.sdk, call.id).unwrap();
        runtime.validate_caller(&rig.sdk).unwrap();
        assert!(runtime.tracking_sendables().is_empty());
        resume.send(()).unwrap();
        scan.join().unwrap().unwrap();
    });
    assert!(!runtime.status().unwrap().scan_running);
    assert!(runtime.tracking_sendables().contains_key(&name));
    let after_scan = runtime.entity_facts(&name).unwrap().unwrap();
    assert_eq!(after_scan.local_rev, newer.local_rev);
    assert_eq!(after_scan.acked_rev, flight.local_rev);
    assert!(after_scan.payload == newer.payload && after_scan.mark.is_some());
    assert_eq!(deck_row(&rig, rig.baseline_deck.0), newer_row);
    rig.host.close().unwrap();
}

#[test]
fn preproof_capture_still_rejects_stale_paused_unverified_and_stopped_tracking() {
    for transition in 0..5 {
        let rig = rig_with_root(false);
        let runtime = rig.host.cloud_runtime().unwrap();
        let collection = rig.host.collection().unwrap();
        let mut caller = rig.sdk.clone();
        match transition {
            0 => caller.push_str(":stale"),
            1 => runtime.adapter_pause(true).unwrap(),
            2 => runtime.adapter_account(None, IdentityStatus::ReadFailed).unwrap(),
            3 => runtime.adapter_tracking_failure(&rig.sdk).unwrap(),
            _ => {
                let slot = rig.host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive
                    .replace(|path| {
                        assert!(path.exists());
                        Ok(())
                    })
                    .unwrap();
            }
        }
        assert!(
            runtime.sync_now(&caller, &collection).is_err(),
            "capture crossed transition {transition}"
        );
        assert!(!runtime.adapter_exchange_active());
        rig.host.close().unwrap();
    }
}

#[test]
fn first_manual_capture_cannot_send_after_changed_or_deleted_root() {
    for deleted in [false, true] {
        let rig = rig_with_root(false);
        let runtime = rig.host.cloud_runtime().unwrap();
        let collection = rig.host.collection().unwrap();
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Pending before root rejection".into()).unwrap();
        let id = first_manual_capture(&rig);
        let selection = runtime.exchange_result(id).unwrap();
        assert!(!selection.selected_names.is_empty());
        let fetch = runtime
            .adapter_begin_fetch(&rig.sdk, vec!["meguri-root".into()])
            .unwrap();
        runtime
            .adapter_will_fetch(&rig.sdk, "rejected-root".into(), true)
            .unwrap();
        runtime
            .adapter_fetch_zone(&rig.sdk, "meguri-root".into(), true, false)
            .unwrap();
        if deleted {
            runtime
                .adapter_fetched(&rig.sdk, "meguri-root", &[], &["active".into()])
                .unwrap();
            // Root deletion queues a known loss. V2.2 requires a fresh account
            // read before the account lane accepts that loss and isolates it.
            let identity_read = runtime.adapter_begin_identity_read(false, None).unwrap();
            assert!(
                runtime
                    .adapter_complete_identity_read(
                        identity_read,
                        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
                    )
                    .unwrap()
            );
            runtime.adapter_accept_zone_losses(identity_read, &collection).unwrap();
        } else {
            let mut changed = rig.active.clone();
            let mut active = meguri_sync::cloud::control_codec::Active::decode(&changed.envelope).unwrap();
            active.epoch[0] ^= 1;
            changed.envelope = active.encode().unwrap();
            runtime
                .adapter_fetched(&rig.sdk, "meguri-root", &[changed], &[])
                .unwrap();
            runtime
                .adapter_fetch_zone(&rig.sdk, "meguri-root".into(), false, false)
                .unwrap();
            runtime.adapter_did_fetch(&rig.sdk, "rejected-root", true).unwrap();
            assert!(runtime.adapter_finish_fetch(&rig.sdk, fetch).is_err());
        }
        assert!(!runtime.sdk_root_confirmed());
        let status = runtime.status().unwrap();
        assert!(!status.stored_usable && !status.identity_verified);
        let reason = if deleted {
            meguri_sync::cloud::IsolationReason::ActiveMissing
        } else {
            meguri_sync::cloud::IsolationReason::EpochSwitched
        };
        assert_eq!(
            status.binding.unwrap().facts.state,
            meguri_sync::cloud::BindingState::Isolated(reason)
        );
        assert!(runtime.start_manual_send_call(&rig.sdk).is_err());
        assert!(
            runtime
                .build_collection_batch(&rig.sdk, &collection, &selection.selected_names)
                .unwrap()
                .saves
                .is_empty()
        );
        assert!(runtime.sync_now(&rig.sdk, &collection).is_err());
        assert_eq!(runtime.exchange_result(id), Some(selection));
        rig.host.close().unwrap();
    }
}

#[test]
fn native_authority_matches_actual_runtime_lifecycle_sdk_and_noops() {
    for transition in 0..6 {
        let rig = rig();
        let runtime = rig.host.cloud_runtime().unwrap();
        let owner = runtime.instance().unwrap();
        let check = || {
            assert!(runtime.native_authority_matches_state_for_testing(&[&owner, &rig.sdk, "unknown", ""]));
        };
        check();
        runtime.retire_sdk("unknown", true).unwrap();
        runtime.release_sdk("unknown").unwrap();
        runtime.adapter_pause(false).unwrap();
        runtime
            .adapter_account(
                Some(&rig.sdk),
                IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
            )
            .unwrap();
        check();
        runtime.validate_native_sdk_for_testing(&rig.sdk).unwrap();
        match transition {
            0 => {
                rig.host.engine().suspend(Duration::from_secs(10)).unwrap();
                check();
                assert!(matches!(
                    runtime.validate_native_sdk_for_testing(&rig.sdk),
                    Err(RuntimeError::StaleIncarnation)
                ));
                rig.host.engine().resume().unwrap();
                check();
                assert!(matches!(
                    runtime.validate_sdk(&rig.sdk),
                    Err(RuntimeError::IdentityUnverified)
                ));
                assert!(matches!(
                    runtime.validate_native_sdk_for_testing(&rig.sdk),
                    Err(RuntimeError::IdentityUnverified)
                ));
                runtime.refresh_identity().unwrap();
                check();
                runtime
                    .adapter_account(
                        Some(&rig.sdk),
                        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
                    )
                    .unwrap();
                check();
                runtime.validate_native_sdk_for_testing(&rig.sdk).unwrap();
            }
            1 => {
                runtime.retire_sdk(&rig.sdk, false).unwrap();
                check();
                runtime.release_sdk(&rig.sdk).unwrap();
                check();
                let replacement = runtime.register_sdk(true).unwrap().0;
                assert!(runtime.native_authority_matches_state_for_testing(&[
                    &owner,
                    &rig.sdk,
                    &replacement,
                    "unknown"
                ]));
                assert!(runtime.validate_native_sdk_for_testing(&rig.sdk).is_err());
                runtime.validate_native_sdk_for_testing(&replacement).unwrap();
                runtime.retire_sdk(&replacement, false).unwrap();
                runtime.release_sdk(&replacement).unwrap();
            }
            2 => {
                runtime.adapter_pause(true).unwrap();
                check();
                runtime.adapter_pause(false).unwrap();
                check();
            }
            3 => {
                runtime
                    .adapter_account(None, IdentityStatus::TemporarilyUnavailable)
                    .unwrap();
                check();
                assert!(matches!(
                    runtime.validate_native_caller_for_testing(&owner),
                    Err(RuntimeError::IdentityUnverified)
                ));
                // SDK guard mismatch precedes the unverified binding error.
                assert!(matches!(
                    runtime.validate_native_sdk_for_testing(&rig.sdk),
                    Err(RuntimeError::StaleIncarnation)
                ));
                runtime
                    .adapter_account(
                        None,
                        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
                    )
                    .unwrap();
                check();
            }
            4 => {
                runtime.adapter_tracking_failure(&rig.sdk).unwrap();
                check();
            }
            5 => {
                runtime.adapter_stop_syncing().unwrap();
                check();
            }
            _ => unreachable!(),
        }
        runtime.retire_sdk(&rig.sdk, false).unwrap();
        runtime.release_sdk(&rig.sdk).unwrap();
        rig.host.close().unwrap();
        check();
    }
}

#[path = "common/native_authority.rs"]
mod native_authority;
use native_authority::hold_native_authority_edge;

#[test]
fn native_authority_refuses_original_command_while_account_isolation_is_held() {
    let rig = rig();
    let runtime = rig.host.cloud_runtime().unwrap();
    let command = command(
        rig.host
            .cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command))
            .unwrap(),
        |kind| matches!(kind, ActionKind::RunExchange { .. }),
    );
    let driver = rig.host.cloud_policy_driver().unwrap();
    driver.validate_native_sdk(&command, &rig.sdk).unwrap();
    let (observed, validation, result, released) = hold_native_authority_edge(
        runtime,
        IncomingStage::AccountIsolationPublished,
        || runtime.adapter_account(Some(&rig.sdk), IdentityStatus::SignedOut),
        || driver.validate_native_sdk(&command, &rig.sdk),
    );
    assert!(runtime.native_authority_matches_state_for_testing(&[&rig.sdk]));
    runtime.retire_sdk(&rig.sdk, false).unwrap();
    runtime.release_sdk(&rig.sdk).unwrap();
    rig.host.close().unwrap();
    assert!(observed && released);
    result.unwrap().unwrap();
    assert!(matches!(validation.unwrap().unwrap(), Err(DriverError::StaleCommand)));
}

#[test]
fn native_authority_retire_isolation_precedes_held_readback_and_survives_failure() {
    for fail_readback in [false, true] {
        let rig = rig();
        let runtime = rig.host.cloud_runtime().unwrap();
        let collection = rig.host.collection().unwrap();
        // An actual same-account reset makes the next owned epoch read due.
        runtime
            .adapter_account(
                Some(&rig.sdk),
                IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
            )
            .unwrap();
        let fetch = runtime.adapter_begin_fetch(&rig.sdk, vec![rig.zone.clone()]).unwrap();
        let group = format!("meguri-{fetch}");
        runtime.adapter_will_fetch(&rig.sdk, group.clone(), true).unwrap();
        runtime.adapter_did_fetch(&rig.sdk, &group, true).unwrap();
        let mut reads = runtime.adapter_fetch_reads(&rig.sdk, fetch, &collection).unwrap();
        assert_eq!(reads.len(), 1);
        let read = reads.remove(0);
        assert!(!read.is_active());
        let followup = runtime
            .adapter_complete_fetch_read(
                &read,
                &collection,
                meguri_sync::cloud::publication::RecordRead::Missing,
                false,
                None,
                None,
            )
            .unwrap()
            .unwrap();
        assert!(followup.is_active());
        // V2.2 requires a read started after this loss; the event is not a read.
        let identity_read = runtime.adapter_begin_identity_read(false, None).unwrap();
        assert!(
            runtime
                .adapter_complete_identity_read(
                    identity_read,
                    IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
                )
                .unwrap()
        );
        runtime.validate_sdk(&rig.sdk).unwrap();
        runtime.validate_native_sdk_for_testing(&rig.sdk).unwrap();
        if fail_readback {
            runtime.fail_retire_readback_for_testing();
        }
        let (observed, validation, result, released) = hold_native_authority_edge(
            runtime,
            IncomingStage::SDKIsolationPublished,
            || runtime.retire_sdk(&rig.sdk, false),
            || runtime.validate_native_sdk_for_testing(&rig.sdk),
        );
        let same = runtime.native_authority_matches_state_for_testing(&[&rig.sdk]);
        let unusable = !runtime.status().unwrap().stored_usable && !runtime.status().unwrap().identity_verified;
        let after_failure = runtime.validate_native_sdk_for_testing(&rig.sdk);
        // Readback failure leaves stopping untouched; the successfully isolated
        // binding alone must reject the still-installed original SDK.
        runtime.retire_sdk(&rig.sdk, false).unwrap();
        runtime.release_sdk(&rig.sdk).unwrap();
        rig.host.close().unwrap();
        assert!(observed && released && same && unusable);
        assert!(matches!(
            validation.unwrap().unwrap(),
            Err(RuntimeError::IdentityUnverified)
        ));
        let result = result.unwrap();
        if fail_readback {
            assert!(matches!(
                result,
                Err(RuntimeError::Tracking(meguri_sync::cloud::Error::Database))
            ));
            assert!(matches!(after_failure, Err(RuntimeError::IdentityUnverified)));
        } else {
            result.unwrap();
            assert!(matches!(after_failure, Err(RuntimeError::StaleIncarnation)));
        }
    }
}

#[test]
fn native_authority_unbound_preserves_original_stale_error_precedence() {
    let root = tempfile::tempdir().unwrap();
    let runtime = meguri_sync::cloud::runtime::Runtime::new(
        root.path().to_owned(),
        CloudInputs {
            scope: BuildScope {
                container: ContainerId::new("synthetic-u114").unwrap(),
                environment: Environment::Development,
            },
            identity: Arc::new(Account),
            files: Arc::new(NativeFiles),
        },
    );
    assert!(runtime.native_authority_matches_state_for_testing(&["", "unknown"]));
    assert!(matches!(
        runtime.validate_native_sdk_for_testing(""),
        Err(RuntimeError::StaleIncarnation)
    ));
    assert!(matches!(
        runtime.validate_native_caller_for_testing(""),
        Err(RuntimeError::StaleIncarnation)
    ));
}

// Original opaque issuance throughout: only real settings/runtime transitions
// invalidate it. No test reconstructs a guard or substitutes a result.
#[test]
fn original_refusal_attempt_distinguishes_policy_and_backend_supersession() {
    for media in [false, true] {
        for backend_only in [false, true] {
            let rig = rig();
            let driver = rig.host.cloud_policy_driver().unwrap();
            let issued = if media {
                command(
                    rig.host
                        .cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
                            enabled: true,
                            paused: false,
                            automatic: true,
                            media: true,
                        }))
                        .unwrap(),
                    |kind| matches!(kind, ActionKind::ResumeMedia),
                )
            } else {
                command(
                    rig.host
                        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Local))
                        .unwrap(),
                    |kind| matches!(kind, ActionKind::UpdatePending(_)),
                )
            };
            if backend_only {
                rig.host.cloud_runtime().unwrap().adapter_pause(true).unwrap();
            } else {
                rig.host
                    .cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
                        enabled: true,
                        paused: true,
                        automatic: media,
                        media,
                    }))
                    .unwrap();
            }
            let runtime = rig.host.cloud_runtime().unwrap();
            let before = runtime.emitted_update_ids();
            let cycles = driver.state().cycles;
            let failure = if media {
                driver
                    .native_disposition_attempt_with_work(&issued, &mut Default::default())
                    .unwrap_err()
            } else {
                rig.host
                    .execute_cloud_policy_core_attempt_with_work(
                        &issued,
                        Duration::from_secs(10),
                        &mut Default::default(),
                    )
                    .unwrap_err()
            };
            assert!(matches!(failure.error, DriverError::StaleCommand));
            assert!(failure.refusal.is_some());
            // Old public accessors keep exactly the original stale failure.
            if media {
                assert!(matches!(
                    driver.native_disposition(&issued),
                    Err(DriverError::StaleCommand)
                ));
            } else {
                assert!(matches!(
                    rig.host.execute_cloud_policy_core(&issued, Duration::from_secs(10)),
                    Err(DriverError::StaleCommand)
                ));
            }
            assert_eq!(runtime.emitted_update_ids(), before);
            assert_eq!(driver.state().cycles, cycles);
            // The already returned attempt is immutable across another settings turn.
            rig.host
                .cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
                    enabled: true,
                    paused: false,
                    automatic: media,
                    media,
                }))
                .unwrap();
            assert!(failure.refusal.is_some());
            rig.host.close().unwrap();
        }
    }
}

#[test]
fn original_refusal_attempt_rejects_foreign_owner_binding_and_generation_without_qualification() {
    let original = rig();
    let issued = command(
        original
            .host
            .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Local))
            .unwrap(),
        |kind| matches!(kind, ActionKind::UpdatePending(_)),
    );
    let foreign = rig_with_root_seed(true, 1915);
    assert_ne!(
        issued.action.guard.binding,
        foreign
            .host
            .cloud_policy_driver()
            .unwrap()
            .state()
            .facts
            .binding
            .as_ref()
            .map(|binding| binding.id)
    );
    foreign.host.cloud_runtime().unwrap().adapter_pause(true).unwrap();
    let error = foreign
        .host
        .execute_cloud_policy_core_attempt_with_work(&issued, Duration::from_secs(10), &mut Default::default())
        .unwrap_err();
    assert!(matches!(error.error, DriverError::StaleCommand));
    assert!(error.refusal.is_none());
    foreign.host.close().unwrap();
    let slot = original
        .host
        .engine()
        .acquire_slot(meguri_engine::LongOp::Restore)
        .unwrap();
    slot.enter_exclusive(true).unwrap().replace(|_| Ok(())).unwrap();
    drop(slot);
    original
        .host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let error = original
        .host
        .execute_cloud_policy_core_attempt_with_work(&issued, Duration::from_secs(10), &mut Default::default())
        .unwrap_err();
    assert!(error.refusal.is_none());
    original.host.close().unwrap();
}

fn incomplete_record(rig: &Rig, action: &Command) {
    rig.host.execute_cloud_policy_core(action, Duration::from_secs(10)).unwrap();
}
fn incomplete_pending_batch(rig: &Rig) -> meguri_sync::cloud::workflow::BuiltBatch {
    let runtime = rig.host.cloud_runtime().unwrap();
    let names = runtime.tracking_sendables().into_keys().collect();
    let batch = runtime.build_collection_batch(&rig.sdk, &rig.host.collection().unwrap(), &names).unwrap();
    assert!(!batch.saves.is_empty());
    batch
}
fn incomplete_ack_batch(rig: &Rig, batch: meguri_sync::cloud::workflow::BuiltBatch) {
    let runtime = rig.host.cloud_runtime().unwrap();
    let saved = batch.saves.into_iter().map(|r| Saved {
        name: r.name, envelope: r.envelope, system_fields: format!("incomplete-save:{}", batch.id).into_bytes(),
    }).collect::<Vec<_>>();
    runtime.enter_sent_handler(&rig.sdk, batch.id).unwrap();
    runtime.sent_results(&rig.sdk, &rig.host.collection().unwrap(), batch.id, &saved, &[], saved.len()).unwrap();
    runtime.return_sent_handler(&rig.sdk).unwrap();
}

#[test]
fn exchange_incomplete_accumulates_original_boundaries_through_other_results_and_duplicates() {
    use meguri_sync::cloud::adapter::CollectionOutcome;
    let rig = rig();
    let collection = rig.host.collection().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "First incomplete revision".into()).unwrap();
    let first = incomplete_end(&rig);
    incomplete_record(&rig, &first);
    let old_batch = incomplete_pending_batch(&rig);
    for (fetched, terminal) in [(false, None), (true, Some(CollectionOutcome::Failed)), (true, Some(CollectionOutcome::Cancelled))] {
        let result = policy_native_end(&rig, fetched, terminal);
        incomplete_record(&rig, &result);
        incomplete_record(&rig, &result);
        assert_eq!(incomplete_count(&rig.host), Some(1));
    }
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Second incomplete revision".into()).unwrap();
    let second = incomplete_end(&rig);
    incomplete_record(&rig, &second);
    incomplete_record(&rig, &second);
    incomplete_ack_batch(&rig, old_batch);
    assert_eq!(incomplete_count(&rig.host), Some(1));
    incomplete_ack_batch(&rig, incomplete_pending_batch(&rig));
    assert_eq!(incomplete_count(&rig.host), None);
    incomplete_record(&rig, &first);
    incomplete_record(&rig, &second);
    assert_eq!(incomplete_count(&rig.host), None);
    rig.host.close().unwrap();
}

#[test]
fn exchange_incomplete_late_ack_survives_newer_edit_and_reopen_before_or_after_recording() {
    for record_before_ack in [false, true] {
        let rig = rig();
        let collection = rig.host.collection().unwrap();
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Original obligation".into()).unwrap();
        let action = incomplete_end(&rig);
        let batch = incomplete_pending_batch(&rig);
        if record_before_ack { incomplete_record(&rig, &action); }
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Newer pending local edit".into()).unwrap();
        incomplete_ack_batch(&rig, batch);
        if !record_before_ack { incomplete_record(&rig, &action); }
        assert_eq!(incomplete_count(&rig.host), None);
        assert!(!rig.host.cloud_runtime().unwrap().tracking_sendables().is_empty());
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Even newer local edit".into()).unwrap();
        assert_eq!(incomplete_count(&rig.host), None);
        incomplete_record(&rig, &action);
        rig.host.close().unwrap();
        let reopened = CoreHost::open(rig.config.clone()).unwrap();
        assert_eq!(incomplete_count(&reopened), None);
        assert!(!reopened.cloud_runtime().unwrap().policy_projection(
            meguri_sync::cloud::policy::Lifecycle::Ready, reopened.collection().unwrap().generation().0,
        ).unwrap().facts.summary.attention_known);
        reopened.close().unwrap();
    }
}

#[test]
fn exchange_incomplete_reopen_retains_none_some_and_all_done() {
    use meguri_sync::cloud::InboxReason;
    for done in 0..=2 {
        let rig = rig();
        let collection = rig.host.collection().unwrap();
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Two directions pending".into()).unwrap();
        incoming_deck(&rig);
        let runtime = rig.host.cloud_runtime().unwrap();
        let binding = runtime.status().unwrap().binding.unwrap().id;
        let entry = runtime.inbox_entries(binding).unwrap().remove(0);
        let action = incomplete_end(&rig);
        incomplete_record(&rig, &action);
        assert_eq!(incomplete_count(&rig.host), Some(1));
        if done >= 1 {
            runtime.set_inbox_reason_for_testing(binding, entry.id, InboxReason::Reference).unwrap();
            assert_eq!(incomplete_count(&rig.host), Some(1)); // outgoing still blocked
        }
        if done == 2 {
            runtime.set_inbox_reason_for_testing(binding, entry.id, InboxReason::Ready).unwrap();
            let apply = command(rig.host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap(),
                |kind| matches!(kind, ActionKind::ApplyReady { .. }));
            rig.host.execute_cloud_policy_core(&apply, Duration::from_secs(10)).unwrap();
            if !runtime.tracking_sendables().is_empty() { incomplete_ack_batch(&rig, incomplete_pending_batch(&rig)); }
            assert_eq!(incomplete_count(&rig.host), None);
        }
        rig.host.close().unwrap();
        let reopened = CoreHost::open(rig.config.clone()).unwrap();
        assert_eq!(incomplete_count(&reopened), if done == 2 { None } else { Some(1) });
        reopened.close().unwrap();
    }
}

#[test]
fn exchange_incomplete_every_original_incoming_reason_can_become_nonblocking_only_for_itself() {
    use meguri_sync::cloud::{InboxReason, ApplyFailure};
    for original in [InboxReason::Ready, InboxReason::Format, InboxReason::Asset,
        InboxReason::ApplyFailed { failure: ApplyFailure::Apply, error: OpaqueBytes(vec![1]) }] {
        for later in [InboxReason::Generation, InboxReason::Reference, InboxReason::Structure] {
            let rig = rig();
            incoming_deck(&rig);
            let runtime = rig.host.cloud_runtime().unwrap();
            let binding = runtime.status().unwrap().binding.unwrap().id;
            let entry = runtime.inbox_entries(binding).unwrap().remove(0);
            runtime.set_inbox_reason_for_testing(binding, entry.id, original.clone()).unwrap();
            let action = incomplete_end(&rig);
            if original == InboxReason::Ready {
                assert!(matches!(&action.action.kind, ActionKind::RecordExchange(record)
                    if matches!(record.outcome, ExchangeOutcome::Incomplete { outstanding: 0, blocked: 0 })));
            }
            incomplete_record(&rig, &action);
            assert_eq!(incomplete_count(&rig.host), Some(1));
            runtime.set_inbox_reason_for_testing(binding, entry.id, later).unwrap();
            assert_eq!(incomplete_count(&rig.host), None);
            runtime.set_inbox_reason_for_testing(binding, entry.id, InboxReason::Format).unwrap();
            incomplete_record(&rig, &action);
            assert_eq!(incomplete_count(&rig.host), None);
            let projection = runtime.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, rig.host.engine().generation().0).unwrap();
            assert!(projection.facts.summary.attention.contains_key(&meguri_sync::cloud::policy::Attention::UnsupportedSchema));
            assert!(!projection.facts.summary.attention_known);
            rig.host.close().unwrap();
        }
    }
}

// P12.6.1 follows K13 acceptance of the captured version.
#[test]
fn exchange_incomplete_k13_proofs_exclude_unaccepted_deliveries() {
    use meguri_sync::cloud::{InboxReason, InboxValue};
    enum Evidence { Applied, Replaced, Removed, Unsupported, Malformed, Older, Unrelated }
    for evidence in [Evidence::Applied, Evidence::Replaced, Evidence::Removed,
        Evidence::Unsupported, Evidence::Malformed, Evidence::Older, Evidence::Unrelated] {
        let fixture = rig();
        let (_, _, worker) = incoming_deck(&fixture);
        let cloud = fixture.host.cloud_runtime().unwrap();
        let owner = cloud.status().unwrap().binding.unwrap().id;
        let captured = cloud.inbox_entries(owner).unwrap().remove(0);
        let name = &captured.entry.record_name;
        let end = incomplete_end(&fixture);
        incomplete_record(&fixture, &end);
        assert_eq!(incomplete_count(&fixture.host), Some(1));
        let settles = match evidence {
            Evidence::Applied => {
                fixture.host.execute_cloud_policy_core(&worker, Duration::from_secs(10)).unwrap();
                true
            }
            Evidence::Removed => {
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[], std::slice::from_ref(name)).unwrap();
                true
            }
            Evidence::Replaced | Evidence::Unsupported => {
                let InboxValue::Record { envelope, system_fields, .. } = &captured.entry.value else { unreachable!() };
                let mut update = advance_server_record(&Saved { name: name.clone(), envelope: envelope.0.clone(),
                    system_fields: system_fields.0.clone() });
                let supported = matches!(evidence, Evidence::Replaced);
                if !supported {
                    assert_eq!(update.envelope[..2], [8, 1]);
                    update.envelope[1] = 2;
                }
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[update], &[]).unwrap();
                supported
            }
            Evidence::Malformed => {
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone,
                    &[Saved { name: name.clone(), envelope: vec![1], system_fields: vec![2] }], &[]).unwrap();
                false
            }
            Evidence::Older => {
                let baseline = fixture.records.iter().find(|record| &record.name == name).unwrap();
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(baseline), &[]).unwrap();
                false
            }
            Evidence::Unrelated => {
                let baseline = fixture.records.iter().find(|record| &record.name != name && matches!(
                    codec::Entity::decode(&record.envelope).unwrap(),
                    codec::DecodeOutcome::Supported(entity) if entity.fields.kind == codec::Kind::Deck)).unwrap();
                let update = advance_server_record(baseline);
                let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
                entity.fields.generation = Some([71; 16]);
                let bytes = codec::Entity::build(entity.fields, entity.payload.as_deref().unwrap()).unwrap().encode().unwrap();
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone,
                    &[Saved { envelope: bytes, ..update }], &[]).unwrap();
                let waiting = cloud.inbox_entries(owner).unwrap().into_iter().find(|item| item.entry.record_name == baseline.name).unwrap();
                assert_eq!(waiting.entry.reason, InboxReason::Generation);
                false
            }
        };
        let attention = if settles { None } else { Some(1) };
        assert_eq!(incomplete_count(&fixture.host), attention);
        incomplete_record(&fixture, &end);
        assert_eq!(incomplete_count(&fixture.host), attention);
        assert!(!cloud.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready,
            fixture.host.engine().generation().0).unwrap().facts.summary.attention_known);
        fixture.host.close().unwrap();
        let reopened = CoreHost::open(fixture.config.clone()).unwrap();
        assert_eq!(incomplete_count(&reopened), attention);
        reopened.close().unwrap();
    }
}

fn advance_server_record(previous: &Saved) -> Saved {
    let codec::DecodeOutcome::Supported(mut decoded) = codec::Entity::decode(&previous.envelope).unwrap()
        else { panic!("fixture requires a supported entity") };
    decoded.fields.erev += 1;
    Saved {
        name: previous.name.clone(),
        system_fields: b"next-server-receipt".to_vec(),
        envelope: codec::Entity::build(decoded.fields, decoded.payload.as_deref().unwrap()).unwrap().encode().unwrap(),
    }
}

#[test]
fn exchange_incomplete_clean_and_unsent_drop_are_durable_done_proofs() {
    let rig = rig();
    let collection = rig.host.collection().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Undo will make this clean".into()).unwrap();
    let action = incomplete_end(&rig);
    incomplete_record(&rig, &action);
    collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
    assert_eq!(incomplete_count(&rig.host), None);
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Later dirty again".into()).unwrap();
    assert_eq!(incomplete_count(&rig.host), None);
    rig.host.close().unwrap();

    let rig = rig_with_root(false);
    fetch_first_root(&rig);
    let collection = rig.host.collection().unwrap();
    let notetype = rig.records.iter().find_map(|r| {
        let codec::DecodeOutcome::Supported(e) = codec::Entity::decode(&r.envelope).ok()? else { return None; };
        match Payload::decode(e.fields.kind, e.payload.as_deref()?).ok()? {
            Payload::Notetype(nt) => Some(meguri_model::ids::NotetypeId(nt.id)), _ => None,
        }
    }).unwrap();
    let added = meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
        notetype_id: notetype, fields: vec!["Incomplete public question".into(), "Public answer".into()],
        tags: Vec::new(), deck_id: rig.baseline_deck, guid: None, update_add_defaults: false,
    }, meguri_engine::OpSource::UserUndoable).unwrap().value;
    let action = incomplete_end(&rig);
    incomplete_record(&rig, &action);
    meguri_engine::notes::delete(&collection, vec![added.id], meguri_engine::OpSource::UserUndoable).unwrap();
    // The note and card drop unsent; the separate selected config key remains.
    assert_eq!(incomplete_count(&rig.host), Some(1));
    incomplete_ack_batch(&rig, incomplete_pending_batch(&rig));
    assert_eq!(incomplete_count(&rig.host), None);
    collection.undo(meguri_engine::undo::UndoScope::Global).unwrap();
    assert_eq!(incomplete_count(&rig.host), None);
    rig.host.close().unwrap();
}

#[test]
fn exchange_incomplete_original_success_order_ignores_backward_clock_and_first_late_delivery() {
    for late_old_success in [false, true] {
        let rig = rig();
        // An older original success can be delivered for the first time later.
        let older = policy_native_end(&rig, true, None);
        assert!(matches!(&older.action.kind, ActionKind::RecordExchange(r) if matches!(r.outcome, ExchangeOutcome::Succeeded { .. })));
        if !late_old_success { incomplete_record(&rig, &older); }
        rig.clock.step_ms(-10_000);
        meguri_engine::decks::rename(&rig.host.collection().unwrap(), rig.baseline_deck, "Newer incomplete original".into()).unwrap();
        let incomplete = incomplete_end(&rig);
        incomplete_record(&rig, &incomplete);
        incomplete_record(&rig, &older);
        incomplete_record(&rig, &older);
        assert_eq!(incomplete_count(&rig.host), Some(1));
        // Work remaining outside the selection is allowed by a later success.
        // The original's incoming transition clears only the work it owns.
        rig.clock.step_ms(-100_000);
        incomplete_ack_batch(&rig, incomplete_pending_batch(&rig));
        let later = policy_native_end(&rig, true, None);
        incomplete_record(&rig, &later);
        incomplete_record(&rig, &later);
        assert_eq!(incomplete_count(&rig.host), None);
        rig.host.close().unwrap();
        let reopened = CoreHost::open(rig.config.clone()).unwrap();
        assert_eq!(incomplete_count(&reopened), None);
        reopened.close().unwrap();
    }
}

#[test]
fn exchange_incomplete_stale_close_and_foreign_result_cannot_add_or_clear_state() {
    let rig = rig();
    meguri_engine::decks::rename(&rig.host.collection().unwrap(), rig.baseline_deck, "Scoped incomplete".into()).unwrap();
    let action = incomplete_end(&rig);
    let other = rig_with_root_seed(true, 1915);
    assert!(other.host.execute_cloud_policy_core(&action, Duration::from_secs(10)).is_err());
    assert_eq!(incomplete_count(&other.host), None);
    incomplete_record(&rig, &action);
    assert_eq!(incomplete_count(&rig.host), Some(1));
    other.host.close().unwrap();
    rig.host.close().unwrap();
    assert!(rig.host.execute_cloud_policy_core(&action, Duration::from_secs(10)).is_err());
    let reopened = CoreHost::open(rig.config.clone()).unwrap();
    assert!(reopened.execute_cloud_policy_core(&action, Duration::from_secs(10)).is_err());
    assert_eq!(incomplete_count(&reopened), Some(1));
    reopened.close().unwrap();
}

fn incomplete_reopen(rig: &mut Rig) {
    rig.host.close().unwrap();
    rig.host = Arc::new(CoreHost::open(rig.config.clone()).unwrap());
    rig.host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    })).unwrap();
    rig.host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())), origin_sdk: None,
    }).unwrap();
    rig.host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
    let runtime = rig.host.cloud_runtime().unwrap();
    runtime.reconcile_binding(&rig.host.collection().unwrap(), &rig.host.media().unwrap(), Duration::from_secs(10)).unwrap();
    runtime.accept_root_fetch(&runtime.instance().unwrap(), Some(&rig.active), true, 1).unwrap();
    let (sdk, _, zone) = runtime.register_sdk(false).unwrap();
    rig.sdk = sdk;
    rig.zone = zone;
    rig.host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
}

#[test]
fn exchange_incomplete_accepted_ack_after_reopen_uses_original_boundary_and_keeps_newer_pending() {
    for discover_by_fetch in [false, true] {
        let mut rig = rig();
        let collection = rig.host.collection().unwrap();
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Original durable boundary".into()).unwrap();
        let action = incomplete_end(&rig);
        incomplete_record(&rig, &action);
        let batch = incomplete_pending_batch(&rig);
        let original = Saved { name: batch.saves[0].name.clone(), envelope: batch.saves[0].envelope.clone(),
            system_fields: b"original-flight-landed".to_vec() };
        meguri_engine::decks::rename(&collection, rig.baseline_deck, "Unacknowledged newer revision".into()).unwrap();
        incomplete_reopen(&mut rig);
        assert_eq!(incomplete_count(&rig.host), Some(1));
        if discover_by_fetch {
            let runtime = rig.host.cloud_runtime().unwrap();
            runtime.adapter_fetched(&rig.sdk, &rig.zone, &[original], &[]).unwrap();
            runtime.drain_native_incoming(&rig.host.collection().unwrap(), &rig.sdk, Duration::from_secs(10)).unwrap();
        } else {
            let batch = incomplete_pending_batch(&rig);
            meguri_engine::decks::rename(&rig.host.collection().unwrap(), rig.baseline_deck, "Newer after reopen handoff".into()).unwrap();
            incomplete_ack_batch(&rig, batch);
        }
        assert_eq!(incomplete_count(&rig.host), None);
        assert!(!rig.host.cloud_runtime().unwrap().tracking_sendables().is_empty());
        incomplete_reopen(&mut rig);
        assert_eq!(incomplete_count(&rig.host), None);
        rig.host.close().unwrap();
    }
}

#[test]
fn exchange_incomplete_actual_later_success_clears_unsettled_original_with_backward_clock() {
    let mut rig = rig();
    incoming_deck(&rig);
    let runtime = rig.host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap().id;
    let entry = runtime.inbox_entries(binding).unwrap().remove(0).entry;
    let incomplete = incomplete_end(&rig);
    incomplete_record(&rig, &incomplete);
    let meguri_sync::cloud::InboxValue::Record { envelope, .. } = &entry.value else { unreachable!() };
    let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&envelope.0).unwrap() else { unreachable!() };
    entity.fields.erev += 1;
    // A newer malformed payload is refused by K20. Its typed version removes
    // the old queued entry, but refusal supplies no incoming-done receipt.
    let mut payload = entity.payload.unwrap();
    payload.push(0);
    let refused = Saved { name: entry.record_name, envelope: codec::Entity::build(entity.fields, &payload).unwrap().encode().unwrap(),
        system_fields: b"refused-newer-record".to_vec() };
    runtime.adapter_fetched(&rig.sdk, &rig.zone, &[refused], &[]).unwrap();
    assert!(runtime.inbox_entries(binding).unwrap().is_empty());
    assert_eq!(runtime.refused_entries(binding).unwrap().len(), 1);
    assert_eq!(incomplete_count(&rig.host), Some(1));
    rig.clock.step_ms(-100_000);
    let later = policy_native_end(&rig, true, None);
    assert!(matches!(&later.action.kind, ActionKind::RecordExchange(record) if matches!(record.outcome, ExchangeOutcome::Succeeded { .. })));
    // Original completion alone is not its durable P12.7 acceptance.
    assert_eq!(incomplete_count(&rig.host), Some(1));
    incomplete_record(&rig, &later);
    assert_eq!(incomplete_count(&rig.host), None);
    incomplete_record(&rig, &later);
    incomplete_record(&rig, &incomplete);
    assert_eq!(incomplete_count(&rig.host), None);
    assert!(rig.host.cloud_runtime().unwrap().policy_projection(
        meguri_sync::cloud::policy::Lifecycle::Ready, rig.host.engine().generation().0,
    ).unwrap().facts.summary.attention.contains_key(&meguri_sync::cloud::policy::Attention::Refused));
    incomplete_reopen(&mut rig);
    assert_eq!(incomplete_count(&rig.host), None);
    rig.host.close().unwrap();
}

#[test]
fn exchange_incomplete_tombstone_ack_survives_refused_undo_and_permitted_edit() {
    let rig = rig_with_root(false);
    fetch_first_root(&rig);
    let collection = rig.host.collection().unwrap();
    let note = rig.records.iter().find_map(|r| {
        let codec::DecodeOutcome::Supported(e) = codec::Entity::decode(&r.envelope).ok()? else { return None; };
        match Payload::decode(e.fields.kind, e.payload.as_deref()?).ok()? {
            Payload::Note(note) => Some(meguri_model::ids::NoteId(note.id)), _ => None,
        }
    }).unwrap();
    meguri_engine::notes::delete(&collection, vec![note], meguri_engine::OpSource::UserUndoable).unwrap();
    let action = incomplete_end(&rig);
    incomplete_record(&rig, &action);
    let batch = incomplete_pending_batch(&rig);
    assert!(batch.saves.iter().any(|r| matches!(codec::Entity::decode(&r.envelope).unwrap(), codec::DecodeOutcome::Supported(e) if e.fields.tombstone)));
    assert_eq!(incomplete_count(&rig.host), Some(1));
    incomplete_ack_batch(&rig, batch);
    assert_eq!(incomplete_count(&rig.host), None);
    assert!(matches!(collection.undo(meguri_engine::undo::UndoScope::Global),
        Err(meguri_engine::EngineError::UndoUnavailable {
            reason: meguri_engine::undo::reasons::DisabledReason::AlreadyUploaded,
        })));
    assert_eq!(incomplete_count(&rig.host), None);
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Permitted edit after deletion ACK".into()).unwrap();
    assert_eq!(incomplete_count(&rig.host), None);
    incomplete_record(&rig, &action);
    assert_eq!(incomplete_count(&rig.host), None);
    rig.host.close().unwrap();
    let reopened = CoreHost::open(rig.config.clone()).unwrap();
    assert_eq!(incomplete_count(&reopened), None);
    assert!(!reopened.cloud_policy_driver().unwrap().state().facts.summary.attention_known);
    reopened.close().unwrap();
}

#[test]
fn exchange_incomplete_held_selection_only_finishes_on_qualified_image_ack() {
    let rig = rig_with_root(false);
    fetch_first_root(&rig);
    let collection = rig.host.collection().unwrap();
    let driver = rig.host.cloud_policy_driver().unwrap();
    let run = command(rig.host.cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command)).unwrap(),
        |k| matches!(k, ActionKind::RunExchange { .. }));
    let ActionKind::RunExchange { exchange, .. } = run.action.kind else { unreachable!() };
    let meguri_engine::session::StartResult::Started { session_id, head, .. } = rig.host.review().start(rig.baseline_deck, None).unwrap() else { panic!("real review must start") };
    let item = &head.items[0];
    assert!(matches!(rig.host.review().answer(session_id, item.card_id,
        meguri_sched::answer::Rating::Easy, item.state_token,
        meguri_model::time::EpochMillis(rig.clock.now_ms()), 123).unwrap(),
        meguri_engine::session::AnswerOutcome::Accepted { .. }));
    rig.host.review().flush_for_background().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Edit inside the newest answer run".into()).unwrap();
    let created = meguri_engine::decks::add(&collection, "Deck created in the answer run".into()).unwrap().value.id;
    let held = collection.cloud_held_run(Some(session_id)).unwrap();
    assert!(!held.is_empty());
    assert!(held.touches(Table::Decks, &RowKey::Id(rig.baseline_deck.0)));
    assert!(held.touches(Table::Decks, &RowKey::Id(created.0)));
    let runtime = rig.host.cloud_runtime().unwrap();
    runtime.poll_review(&rig.sdk).unwrap();
    let native = driver.capture_exchange(&run, &collection).unwrap();
    // The baseline rows may have eligible older images. This held creation has
    // no row before the answer run, so it must remain deferred at capture.
    let selected = runtime.exchange_result(native).unwrap().selected_names;
    let created_name = runtime.entity_facts_page(codec::Kind::Deck, None).unwrap().into_iter()
        .find(|e| e.key == codec::id_key(created.0)).unwrap().name;
    assert!(selected.contains(&created_name));
    let held_batch = runtime.build_collection_batch(&rig.sdk, &collection, &selected).unwrap();
    assert!(!held_batch.saves.iter().any(|r| r.name == created_name));
    assert!(held_batch.saves.len() < selected.len());
    let completed = runtime.adapter_complete_exchange(&rig.sdk, native, true, None).unwrap();
    assert_eq!(completed.collection, meguri_sync::cloud::adapter::CollectionOutcome::Incomplete);
    assert!(completed.work.as_ref().unwrap().held_pending_names.contains(&created_name));
    assert!(completed.result.outstanding_names.contains(&created_name));
    let record = command(driver.complete_exchange(exchange, rig.host.engine().state(), rig.host.engine().generation()).unwrap(),
        |k| matches!(k, ActionKind::RecordExchange(_)));
    incomplete_record(&rig, &record);
    assert_eq!(incomplete_count(&rig.host), Some(1));
    if !held_batch.saves.is_empty() {
        incomplete_ack_batch(&rig, held_batch);
        assert_eq!(incomplete_count(&rig.host), Some(1));
    }
    meguri_engine::decks::rename(&collection, created, "New held edit cannot finish work".into()).unwrap();
    let newer_held = runtime.build_collection_batch(&rig.sdk, &collection, &selected).unwrap();
    assert!(!newer_held.saves.iter().any(|r| r.name == created_name));
    assert_eq!(incomplete_count(&rig.host), Some(1));
    rig.host.review().end(session_id).unwrap();
    runtime.poll_review(&rig.sdk).unwrap();
    let qualified = incomplete_pending_batch(&rig);
    assert!(qualified.saves.iter().any(|r| r.name == created_name));
    let proof = runtime.smoke_batch_for_testing(&rig.sdk, qualified.id).unwrap();
    assert!(proof.save_images.iter().any(|image| image.name == created_name && !image.held
        && image.image_seq >= completed.result.boundary_seq));
    incomplete_ack_batch(&rig, qualified);
    assert_eq!(incomplete_count(&rig.host), None);
    rig.host.close().unwrap();
}

#[test]
fn exchange_incomplete_changed_account_refuses_original_success_without_clearing() {
    let rig = rig();
    let old_success = policy_native_end(&rig, true, None);
    meguri_engine::decks::rename(&rig.host.collection().unwrap(), rig.baseline_deck, "Account scoped obligation".into()).unwrap();
    incomplete_record(&rig, &incomplete_end(&rig));
    rig.host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-B".to_vec())), origin_sdk: None,
    }).unwrap();
    assert!(rig.host.execute_cloud_policy_core(&old_success, Duration::from_secs(10)).is_err());
    assert_eq!(incomplete_count(&rig.host), Some(1));
    rig.host.close().unwrap();
}

#[test]
fn exchange_incomplete_remote_merge_requires_separate_own_ack() {
    let fixture = rig();
    let db = fixture.host.collection().unwrap();
    let cloud = fixture.host.cloud_runtime().unwrap();
    meguri_engine::decks::rename(&db, fixture.baseline_deck, "Outgoing deck revision".into()).unwrap();
    meguri_engine::decks::select(&db, fixture.baseline_deck).unwrap();
    let result = incomplete_end(&fixture);
    incomplete_record(&fixture, &result);
    let name = cloud.entity_facts_page(codec::Kind::Deck, None).unwrap().into_iter()
        .find(|entity| entity.key == codec::id_key(fixture.baseline_deck.0)).unwrap().name;
    let batch = cloud.build_collection_batch(&fixture.sdk, &db, &[name.clone()].into()).unwrap();
    assert_eq!(batch.saves.len(), 1);
    let save = &batch.saves[0];
    let codec::DecodeOutcome::Supported(mut decoded) = codec::Entity::decode(&save.envelope).unwrap() else { unreachable!() };
    decoded.fields.writer = [88; 16];
    let remote = Saved { name: name.clone(), system_fields: b"remote-writer-receipt".to_vec(),
        envelope: codec::Entity::build(decoded.fields, decoded.payload.as_deref().unwrap()).unwrap().encode().unwrap() };
    fixture.clock.step_ms(2000);
    meguri_engine::decks::rename(&db, fixture.baseline_deck, "Deck edited after handoff".into()).unwrap();
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[remote], &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let current = cloud.entity_facts(&name).unwrap().unwrap();
    assert!(current.mark.is_some());
    assert!(current.local_rev >= current.acked_rev);
    let independent = cloud.tracking_sendables().into_keys().filter(|candidate| candidate != &name).collect();
    let other_batch = cloud.build_collection_batch(&fixture.sdk, &db, &independent).unwrap();
    if !other_batch.saves.is_empty() { incomplete_ack_batch(&fixture, other_batch); }
    assert_eq!(incomplete_count(&fixture.host), Some(1));
    incomplete_ack_batch(&fixture, incomplete_pending_batch(&fixture));
    assert_eq!(incomplete_count(&fixture.host), None);
    fixture.host.close().unwrap();
}

#[test]
fn exchange_incomplete_repeated_handoff_qualifies_in_memory_and_after_reopen() {
    for fetch_ack in [false, true] {
        for during_capture in [false, true] {
            let mut fixture = rig();
            let db = fixture.host.collection().unwrap();
            meguri_engine::decks::rename(&db, fixture.baseline_deck, "Deck awaiting acknowledgement".into()).unwrap();
            let prepared = incomplete_pending_batch(&fixture);
            assert_eq!(prepared.saves.len(), 1);
            let save = &prepared.saves[0];
            let cloud = fixture.host.cloud_runtime().unwrap();
            let first_seq = cloud.entity_facts(&save.name).unwrap().unwrap().in_flight[0].image_seq;
            meguri_engine::decks::select(&db, fixture.baseline_deck).unwrap();
            let driver = fixture.host.cloud_policy_driver().unwrap();
            let run = command(fixture.host.cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command)).unwrap(),
                |action| matches!(action, ActionKind::RunExchange { .. }));
            let ActionKind::RunExchange { exchange, .. } = run.action.kind else { unreachable!() };
            let capture = driver.capture_exchange(&run, &db).unwrap();
            policy_fetch_snapshot(&fixture);
            let boundary = cloud.exchange_result(capture).unwrap().boundary_seq;
            assert!(boundary > first_seq);
            if !during_capture {
                let finished = cloud.adapter_complete_exchange(&fixture.sdk, capture, true, None).unwrap();
                assert_eq!(finished.collection, meguri_sync::cloud::adapter::CollectionOutcome::Incomplete);
                let action = command(driver.complete_exchange(exchange, fixture.host.engine().state(), fixture.host.engine().generation()).unwrap(),
                    |kind| matches!(kind, ActionKind::RecordExchange(_)));
                incomplete_record(&fixture, &action);
                assert_eq!(incomplete_count(&fixture.host), Some(1));
            }
            let again = cloud.build_collection_batch(&fixture.sdk, &db, &[save.name.clone()].into()).unwrap();
            assert_eq!(again.saves.len(), 1);
            assert_eq!(again.saves[0].envelope, save.envelope);
            assert!(db.last_seq().0 >= boundary);
            let other_names = cloud.tracking_sendables().into_keys().filter(|name| name != &save.name).collect();
            // The config remains independent work, so both paths must retain
            // an incomplete until its own qualifying acknowledgement arrives.
            if !during_capture {
                let config_batch = cloud.build_collection_batch(&fixture.sdk, &db, &other_names).unwrap();
                incomplete_ack_batch(&fixture, config_batch);
            }
            meguri_engine::decks::rename(&db, fixture.baseline_deck, "Newer unsent deck value".into()).unwrap();
            let accepted = Saved { name: save.name.clone(), envelope: save.envelope.clone(),
                system_fields: b"repeated-image-server-receipt".to_vec() };
            if !during_capture {
                incomplete_reopen(&mut fixture);
                assert_eq!(incomplete_count(&fixture.host), Some(1));
            }
            let cloud = fixture.host.cloud_runtime().unwrap();
            let db = fixture.host.collection().unwrap();
            if fetch_ack {
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[accepted], &[]).unwrap();
                cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
            } else {
                let current = incomplete_pending_batch(&fixture);
                cloud.enter_sent_handler(&fixture.sdk, current.id).unwrap();
                assert_eq!(cloud.sent_results_with_conflicts(&fixture.sdk, &db, current.id, &[], &[accepted], &[], 1).unwrap(), 1);
                cloud.return_sent_handler(&fixture.sdk).unwrap();
            }
            if during_capture {
                let finished = cloud.adapter_complete_exchange(&fixture.sdk, capture, true, None).unwrap();
                assert_eq!(finished.collection, meguri_sync::cloud::adapter::CollectionOutcome::Incomplete);
                assert!(finished.result.acked_names.contains(&save.name));
                assert!(!finished.result.outstanding_names.contains(&save.name));
                assert!(!finished.result.blocked_names.contains(&save.name));
                let driver = fixture.host.cloud_policy_driver().unwrap();
                let action = command(driver.complete_exchange(exchange, fixture.host.engine().state(), fixture.host.engine().generation()).unwrap(),
                    |kind| matches!(kind, ActionKind::RecordExchange(_)));
                incomplete_record(&fixture, &action);
                assert_eq!(incomplete_count(&fixture.host), Some(1));
                let config_batch = cloud.build_collection_batch(&fixture.sdk, &db, &other_names).unwrap();
                incomplete_ack_batch(&fixture, config_batch);
                incomplete_record(&fixture, &action);
            }
            assert_eq!(incomplete_count(&fixture.host), None);
            assert!(cloud.entity_facts(&save.name).unwrap().unwrap().mark.is_some());
            assert!(cloud.tracking_sendables().contains_key(&save.name));
            assert!(!cloud.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready,
                db.generation().0).unwrap().facts.summary.attention_known);
            fixture.host.close().unwrap();
            let reopened = CoreHost::open(fixture.config.clone()).unwrap();
            assert_eq!(incomplete_count(&reopened), None);
            reopened.close().unwrap();
        }
    }
}

#[test]
fn exchange_incomplete_qualified_held_image_ack_survives_newer_held_edits() {
    let rig = rig_with_root(false);
    fetch_first_root(&rig);
    let collection = rig.host.collection().unwrap();
    let runtime = rig.host.cloud_runtime().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Eligible image before the answer".into()).unwrap();
    let record = incomplete_end(&rig);
    incomplete_record(&rig, &record);
    let meguri_engine::session::StartResult::Started { session_id, head, .. } = rig.host.review().start(rig.baseline_deck, None).unwrap() else { panic!("review starts") };
    let item = &head.items[0];
    assert!(matches!(rig.host.review().answer(session_id, item.card_id,
        meguri_sched::answer::Rating::Easy, item.state_token,
        meguri_model::time::EpochMillis(rig.clock.now_ms()), 123).unwrap(),
        meguri_engine::session::AnswerOutcome::Accepted { .. }));
    rig.host.review().flush_for_background().unwrap();
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "New deck value held with the answer".into()).unwrap();
    let held = collection.cloud_held_run(Some(session_id)).unwrap();
    assert!(!held.is_empty());
    assert!(held.touches(Table::Decks, &RowKey::Id(rig.baseline_deck.0)));
    runtime.poll_review(&rig.sdk).unwrap();
    let batch = incomplete_pending_batch(&rig);
    let smoke = runtime.smoke_batch_for_testing(&rig.sdk, batch.id).unwrap();
    assert!(smoke.save_images.iter().any(|image| image.held));
    assert_eq!(incomplete_count(&rig.host), Some(1));
    incomplete_ack_batch(&rig, batch);
    assert_eq!(incomplete_count(&rig.host), None);
    meguri_engine::decks::rename(&collection, rig.baseline_deck, "Another held edit after the accepted image".into()).unwrap();
    let held = runtime.build_collection_batch(&rig.sdk, &collection, &runtime.tracking_sendables().into_keys().collect()).unwrap();
    assert!(held.saves.is_empty());
    assert_eq!(incomplete_count(&rig.host), None);
    assert!(!runtime.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap().facts.summary.attention_known);
    rig.host.review().end(session_id).unwrap();
    rig.host.close().unwrap();
}

// These cases prove accepted replacement of an earlier version. The separate
// same-entry cases below exercise the worker without replacing that version.
#[test]
fn exchange_incomplete_supported_successors_settle_each_original_queue_category() {
    use meguri_sync::cloud::{InboxReason, InboxValue, incoming::AssetReadKind, workflow::FetchedRecord};
    enum QueueCategory { Ordinary, UnknownFormat, NeedsPayload, FailedApply }
    for category in [QueueCategory::Ordinary, QueueCategory::UnknownFormat,
        QueueCategory::NeedsPayload, QueueCategory::FailedApply] {
        for destination in [InboxReason::Generation, InboxReason::Reference, InboxReason::Structure] {
            let fixture = rig();
            let (deck_id, unchanged, worker) = incoming_deck(&fixture);
            let cloud = fixture.host.cloud_runtime().unwrap();
            let db = fixture.host.collection().unwrap();
            let owner = cloud.status().unwrap().binding.unwrap().id;
            let first = cloud.inbox_entries(owner).unwrap().remove(0).entry;
            let InboxValue::Record { envelope, system_fields, .. } = first.value else { unreachable!() };
            let source = Saved { name: first.record_name, system_fields: system_fields.0, envelope: envelope.0 };
            let expected = match category {
                QueueCategory::Ordinary => InboxReason::Ready,
                QueueCategory::UnknownFormat => {
                    fixture.host.execute_cloud_policy_core(&worker, Duration::from_secs(10)).unwrap();
                    let mut future = advance_server_record(&source);
                    future.envelope[1] = 2;
                    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[future], &[]).unwrap();
                    InboxReason::Format
                }
                QueueCategory::NeedsPayload => {
                    let update = advance_server_record(&source);
                    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
                    let Payload::Deck(mut payload) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
                    payload.name.resize(codec::INLINE_ENVELOPE_BYTES + 1, b'x');
                    let large = codec::Entity::build(entity.fields, &Payload::Deck(payload).encode().unwrap()).unwrap().encode().unwrap();
                    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[Saved { envelope: large, ..update }], &[]).unwrap();
                    InboxReason::Asset
                }
                QueueCategory::FailedApply => {
                    cloud.interrupt_next_incoming_for_testing(ApplyInterruption::Error);
                    assert!(matches!(fixture.host.execute_cloud_policy_core(&worker, Duration::from_secs(10)),
                        Err(DriverError::ApplyDeferred(_))));
                    assert_eq!(deck_row(&fixture, deck_id), unchanged);
                    assert!(cloud.apply_intents(owner).unwrap().is_empty());
                    let reason = cloud.inbox_entries(owner).unwrap().remove(0).entry.reason;
                    assert!(matches!(reason, InboxReason::ApplyFailed { .. }));
                    reason
                }
            };
            let original = cloud.inbox_entries(owner).unwrap().remove(0);
            assert_eq!(original.entry.reason, expected);
            let end = incomplete_end(&fixture);
            incomplete_record(&fixture, &end);
            assert_eq!(incomplete_count(&fixture.host), Some(1));
            let later = advance_server_record(&advance_server_record(&source));
            let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&later.envelope).unwrap() else { unreachable!() };
            let Payload::Deck(mut payload) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
            match destination {
                InboxReason::Generation => entity.fields.generation = Some([72; 16]),
                InboxReason::Reference => payload.parent_id = 9_000_001,
                InboxReason::Structure => {
                    entity.fields.tombstone = true;
                    entity.fields.deleted_at_ms = fixture.clock.now_ms();
                }
                _ => unreachable!(),
            }
            let replacement = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(payload).encode().unwrap()).unwrap().encode().unwrap(), ..later };
            if matches!(category, QueueCategory::NeedsPayload) && destination != InboxReason::Generation {
                let pass = cloud.begin_asset_read_pass(&fixture.sdk, &db, AssetReadKind::Retry).unwrap();
                let request = cloud.next_asset_read(&db, &pass).unwrap().unwrap();
                assert!(cloud.submit_asset_read_queue(&db, &request, || true).unwrap());
                assert!(cloud.asset_read_result(&fixture.sdk, &db, &request,
                    Some(&FetchedRecord { record: replacement, asset_path: None })).unwrap());
                cloud.cancel_asset_read_pass(&pass);
            } else {
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[replacement], &[]).unwrap();
            }
            cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
            let pending = cloud.inbox_entries(owner).unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].entry.reason, destination);
            assert_ne!(pending[0].entry.value, original.entry.value);
            assert_eq!(incomplete_count(&fixture.host), None);
            incomplete_record(&fixture, &end);
            assert_eq!(incomplete_count(&fixture.host), None);
            let summary = cloud.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, db.generation().0).unwrap().facts.summary;
            assert_eq!(summary.incoming_waiting, 1);
            assert!(!summary.attention_known);
            fixture.host.close().unwrap();
            let reopened = CoreHost::open(fixture.config.clone()).unwrap();
            assert_eq!(incomplete_count(&reopened), None);
            reopened.close().unwrap();
        }
    }
}

#[test]
fn exchange_incomplete_replaced_generation_rejects_result_without_tracking_writes() {
    let rig = rig();
    meguri_engine::decks::rename(&rig.host.collection().unwrap(), rig.baseline_deck, "Before generation replacement".into()).unwrap();
    let old = incomplete_end(&rig);
    let slot = rig.host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.enter_exclusive(true).unwrap().replace(|_| Ok(())).unwrap();
    drop(slot);
    rig.host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
    let runtime = rig.host.cloud_runtime().unwrap();
    let before = runtime.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, rig.host.engine().generation().0).unwrap();
    let tracking_bytes = || ["meguri-cloud.db", "meguri-cloud.db-wal"].map(|name| {
        match std::fs::read(rig._root.path().join("collection").join(name)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("synthetic tracking file: {error}"),
        }
    });
    let saved = tracking_bytes();
    assert!(rig.host.execute_cloud_policy_core(&old, Duration::from_secs(10)).is_err());
    assert_eq!(tracking_bytes(), saved);
    let after = runtime.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, rig.host.engine().generation().0).unwrap();
    assert_eq!(after.evidence, before.evidence);
    assert_eq!(after.facts.summary.attention, before.facts.summary.attention);
    assert!(!after.facts.summary.attention_known);
    rig.host.close().unwrap();
}

#[test]
fn exchange_incomplete_same_entry_classification_and_asset_completion_are_durable_proofs() {
    use meguri_sync::cloud::{InboxReason, incoming::AssetReadKind, workflow::FetchedRecord};
    for asset in [false, true] {
        for outcome in 0..3 {
            let rig = rig();
            let runtime = rig.host.cloud_runtime().unwrap();
            let collection = rig.host.collection().unwrap();
            let (_, _, _) = incoming_deck(&rig);
            let binding = runtime.status().unwrap().binding.unwrap().id;
            let queued = runtime.inbox_entries(binding).unwrap().remove(0);
            let meguri_sync::cloud::InboxValue::Record { envelope, .. } = queued.entry.value else { unreachable!() };
            let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&envelope.0).unwrap() else { unreachable!() };
            let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
            entity.fields.erev += 1;
            if outcome == 2 {
                entity.fields.tombstone = true;
                entity.fields.deleted_at_ms = rig.clock.now_ms();
            } else if outcome == 1 {
                deck.parent_id = 9_000_002;
            }
            if asset { deck.name = vec![b'y'; codec::INLINE_ENVELOPE_BYTES + 1]; }
            let payload = Payload::Deck(deck).encode().unwrap();
            let remote = Saved { name: queued.entry.record_name, system_fields: b"same-entry-classification".to_vec(),
                envelope: codec::Entity::build(entity.fields, &payload).unwrap().encode().unwrap() };
            runtime.adapter_fetched(&rig.sdk, &rig.zone, std::slice::from_ref(&remote), &[]).unwrap();
            let original = runtime.inbox_entries(binding).unwrap().remove(0);
            assert_eq!(original.entry.reason, if asset { InboxReason::Asset } else { InboxReason::Ready });
            let record = incomplete_end(&rig);
            incomplete_record(&rig, &record);
            assert_eq!(incomplete_count(&rig.host), Some(1));
            if asset {
                let downloaded = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(downloaded.path(), &payload).unwrap();
                let pass = runtime.begin_asset_read_pass(&rig.sdk, &collection, AssetReadKind::Retry).unwrap();
                let request = runtime.next_asset_read(&collection, &pass).unwrap().unwrap();
                assert!(runtime.submit_asset_read_queue(&collection, &request, || true).unwrap());
                assert!(runtime.asset_read_result(&rig.sdk, &collection, &request,
                    Some(&FetchedRecord { record: remote, asset_path: Some(downloaded.path().to_owned()) })).unwrap());
                runtime.cancel_asset_read_pass(&pass);
                let staged = runtime.inbox_entries(binding).unwrap().remove(0);
                assert_eq!(staged.id, original.id);
                assert_eq!(staged.entry.reason, InboxReason::Ready);
                assert_eq!(incomplete_count(&rig.host), Some(1));
            }
            runtime.drain_native_incoming(&collection, &rig.sdk, Duration::from_secs(10)).unwrap();
            let remaining = runtime.inbox_entries(binding).unwrap();
            if outcome == 0 {
                assert!(remaining.is_empty());
                assert!(runtime.apply_intents(binding).unwrap().is_empty());
                assert_eq!(runtime.entity_facts(&original.entry.record_name).unwrap().unwrap().server_erev,
                    match original.entry.value { meguri_sync::cloud::InboxValue::Record { erev, .. } => erev, _ => unreachable!() });
            } else {
                assert_eq!(remaining.len(), 1);
                assert_eq!(remaining[0].id, original.id);
                assert_eq!(remaining[0].entry.reason, if outcome == 2 { InboxReason::Structure } else { InboxReason::Reference });
            }
            assert_eq!(incomplete_count(&rig.host), None);
            incomplete_record(&rig, &record);
            assert_eq!(incomplete_count(&rig.host), None);
            rig.host.close().unwrap();
            let reopened = CoreHost::open(rig.config.clone()).unwrap();
            assert_eq!(incomplete_count(&reopened), None);
            reopened.close().unwrap();
        }
    }
}

// An apply failure can be retried without accepting a newer server version.
#[test]
fn exchange_incomplete_failed_entry_retries_to_apply_or_reference_without_replacement() {
    use meguri_sync::cloud::{InboxReason, InboxValue};
    for lose_parent in [false, true] {
        let mut fixture = rig();
        let db = fixture.host.collection().unwrap();
        let parent = lose_parent.then(|| meguri_engine::decks::add(&db, "Temporary parent".into()).unwrap().value.id);
        let (deck_id, unchanged, worker) = incoming_deck(&fixture);
        let cloud = fixture.host.cloud_runtime().unwrap();
        let owner = cloud.status().unwrap().binding.unwrap().id;
        if let Some(parent) = parent {
            let queued = cloud.inbox_entries(owner).unwrap().remove(0).entry;
            let InboxValue::Record { envelope, system_fields, .. } = queued.value else { unreachable!() };
            let update = advance_server_record(&Saved { name: queued.record_name, envelope: envelope.0, system_fields: system_fields.0 });
            let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
            let Payload::Deck(mut payload) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
            payload.parent_id = parent.0;
            let bytes = codec::Entity::build(entity.fields, &Payload::Deck(payload).encode().unwrap()).unwrap().encode().unwrap();
            cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[Saved { envelope: bytes, ..update }], &[]).unwrap();
        }
        cloud.interrupt_next_incoming_for_testing(ApplyInterruption::Error);
        assert!(matches!(fixture.host.execute_cloud_policy_core(&worker, Duration::from_secs(10)), Err(DriverError::ApplyDeferred(_))));
        assert_eq!(deck_row(&fixture, deck_id), unchanged);
        assert!(cloud.apply_intents(owner).unwrap().is_empty());
        let failed = cloud.inbox_entries(owner).unwrap().remove(0);
        assert!(matches!(failed.entry.reason, InboxReason::ApplyFailed { .. }));
        let end = incomplete_end(&fixture);
        incomplete_record(&fixture, &end);
        incomplete_record(&fixture, &end);
        assert_eq!(incomplete_count(&fixture.host), Some(1));
        if let Some(parent) = parent {
            let parent_facts = cloud.entity_facts_page(codec::Kind::Deck, None).unwrap().into_iter()
                .find(|entity| entity.key == codec::id_key(parent.0)).unwrap();
            assert!(!parent_facts.sent);
            assert_eq!(parent_facts.server_erev, 0);
            assert_eq!(parent_facts.acked_rev, 0);
            assert!(parent_facts.in_flight.is_empty());
            // The unuploaded parent creation remains undoable. Removing it
            // changes the classifier's live references, not the server entry.
            db.undo(meguri_engine::undo::UndoScope::Global).unwrap();
            {
                let current = fixture.host.collection().unwrap();
                let reader = current.reader(ReaderRole::Query).unwrap();
                assert!(reader.row_image(Table::Decks, &RowKey::Id(parent.0)).unwrap().is_none());
            }
            let absent = cloud.entity_facts(&parent_facts.name).unwrap().unwrap();
            assert!(!absent.sent);
            assert_eq!(absent.server_erev, 0);
            assert_eq!(absent.acked_rev, 0);
            assert!(absent.in_flight.is_empty());
            assert_eq!(incomplete_count(&fixture.host), Some(1));
            let tracking_bytes = || ["meguri-cloud.db", "meguri-cloud.db-wal"].map(|name| {
                match std::fs::read(fixture._root.path().join("collection").join(name)) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => panic!("synthetic tracking file: {error}"),
                }
            });
            let before = tracking_bytes();
            assert!(matches!(cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[], &[]), Err(RuntimeError::StaleIncarnation)));
            assert_eq!(tracking_bytes(), before);
            assert!(fixture.host.execute_cloud_policy_core(&end, Duration::from_secs(10)).is_err());
            assert_eq!(tracking_bytes(), before);
            // Undo retired the original authority. Reopen, reconcile and register
            // a current SDK before granting another attempt to this inbox row.
            drop(db);
            incomplete_reopen(&mut fixture);
            assert!(fixture.host.cloud_runtime().unwrap().entity_facts(&parent_facts.name).unwrap().is_none());
            assert_eq!(incomplete_count(&fixture.host), Some(1));
        }
        let db = fixture.host.collection().unwrap();
        let cloud = fixture.host.cloud_runtime().unwrap();
        assert_eq!(cloud.status().unwrap().binding.unwrap().id, owner);
        let before_retry = cloud.inbox_entries(owner).unwrap().remove(0);
        assert_eq!(before_retry.id, failed.id);
        assert_eq!(before_retry.entry.value, failed.entry.value);
        assert!(matches!(before_retry.entry.reason, InboxReason::ApplyFailed { .. }));
        // A real SDK delivery grants the bounded worker another attempt.
        cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[], &[]).unwrap();
        cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
        let after = cloud.inbox_entries(owner).unwrap();
        if lose_parent {
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].id, failed.id);
            assert_eq!(after[0].entry.value, failed.entry.value);
            assert_eq!(after[0].entry.reason, InboxReason::Reference);
        } else {
            assert!(after.is_empty());
            assert_ne!(deck_row(&fixture, deck_id), unchanged);
        }
        assert!(cloud.apply_intents(owner).unwrap().is_empty());
        assert_eq!(incomplete_count(&fixture.host), None);
        if !lose_parent { incomplete_record(&fixture, &end); }
        assert_eq!(incomplete_count(&fixture.host), None);
        let projection = cloud.policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, db.generation().0).unwrap();
        assert_eq!(projection.facts.summary.incoming_waiting, u64::from(lose_parent));
        assert!(!projection.facts.summary.attention_known);
        fixture.host.close().unwrap();
        let reopened = CoreHost::open(fixture.config.clone()).unwrap();
        assert_eq!(incomplete_count(&reopened), None);
        reopened.close().unwrap();
    }
}

#[path = "support/row_classification.rs"]
mod row_classification;
