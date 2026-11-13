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
        files: Arc::new(NativeFiles),
    });
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
            confirm_root.then(|| b"synthetic-sdk-state".to_vec()),
        )
        .unwrap();
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
        runtime
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
            assert!(runtime.adapter_did_fetch(&rig.sdk, "rejected-root", true).is_err());
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
