//! Actual exported methods over a retained Host; no DTO supplies core facts.
use super::*;
use crate::cloud_policy::*;
use meguri_sync::cloud::testing::ApplyInterruption;

// K6/K11.1/K11.2/K15: these exercise the real Host observer, not seeded
// tracking rows. The existing prebound fixture supplies only the baseline.
fn captured_decks(fixture: &Fixture, prefix: &str) -> Vec<w::EntityFacts> {
    fixture
        .workflow
        .core
        .adapter_snapshot()
        .unwrap()
        .entities
        .into_iter()
        .filter(|e| matches!(&e.payload, c::entity_codec::Payload::Deck(d) if d.name.starts_with(prefix.as_bytes())))
        .collect()
}

fn assert_deck_capture_live(fixture: &Fixture, sdk: &CloudEngineRegistration) {
    let status = fixture.workflow.core.status().unwrap();
    assert!(!status.tracking_stopped);
    assert!(!status.scan_required);
    assert!(status.identity_verified && status.stored_usable);
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    assert_eq!(
        fixture.workflow.core.incoming_sdk_instance_for_testing(),
        Some(sdk.instance_id.clone())
    );
    let engine = fixture
        .workflow
        .host
        .cloud_policy_driver()
        .unwrap()
        .state()
        .engine
        .unwrap();
    assert_eq!(engine.instance, sdk.instance_id);
    assert!(!engine.stopping);
}

// A real provider call samples first, then waits off every core lock. The
// release guard also unblocks the worker if an assertion unwinds the test.
struct ReleaseDeckAccount(std::sync::mpsc::Sender<()>);
impl Drop for ReleaseDeckAccount {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
fn hold_deck_account(provider: &SamplingAccount) -> (std::sync::mpsc::Receiver<()>, ReleaseDeckAccount) {
    let (reached, wait) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    *provider.after_sample.lock().unwrap() = Some(Box::new(move || {
        reached.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    (wait, ReleaseDeckAccount(release))
}

fn read_deck_account(provider: &SamplingAccount) -> r::IdentityStatus {
    <SamplingAccount as r::IdentityProvider>::read_identity(
        provider,
        &c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
    )
}

fn complete_deck_account(fixture: &Fixture, request: u64, observed: r::IdentityStatus) -> bool {
    let r::IdentityStatus::Available(account) = observed else {
        panic!("original provider must return its sampled account")
    };
    fixture
        .workflow
        .complete_identity_read(request, CloudAccountStatus::Available, Some(account.0))
        .unwrap()
}

#[test]
fn new_deck_and_ancestors_capture_once_during_original_sdk_account_read_and_real_review() {
    let provider = SamplingAccount::new();
    let (fixture, review_deck) = unbound_fixture(provider.clone());
    let collection = fixture.workflow.host.collection().unwrap();
    let notetype = meguri_engine::notetypes::add(
        &collection,
        meguri_engine::notetypes::NewNotetype {
            name: "Deck capture review".into(),
            kind: meguri_engine::notetypes::NotetypeKind::Standard,
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![meguri_engine::notetypes::NewTemplate {
                name: "Face".into(),
                front: "{{Front}}".into(),
                back: "{{Back}}".into(),
                ..Default::default()
            }],
            css: None,
        },
    )
    .unwrap()
    .value
    .id;
    meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: notetype,
            fields: vec!["Question".into(), "Answer".into()],
            tags: Vec::new(),
            deck_id: review_deck,
            guid: None,
            update_add_defaults: false,
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    let policy = fixture.workflow.policy().unwrap();
    drain(&policy, policy.clone().input(settings(false)).unwrap());
    drain(&policy, policy.clone().refresh_account().unwrap());
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    );
    let (sdk, _, _) = fixture.baseline();
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
    );
    assert_deck_capture_live(&fixture, &sdk);
    let guard = fixture.workflow.core.adapter_guard();
    let counter = fixture.workflow.core.status().unwrap().change_counter;
    let request = fixture
        .workflow
        .begin_identity_read(false, Some(sdk.instance_id.clone()))
        .unwrap();
    let (reached, release) = hold_deck_account(&provider);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| read_deck_account(&provider));
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let added = meguri_engine::decks::add(&collection, "Held account::Ancestor::New deck".into()).unwrap();
        fixture
            .workflow
            .await_tracking(collection.generation().0, added.seq.0)
            .unwrap();
        let captured = captured_decks(&fixture, "Held account");
        assert_eq!(captured.len(), 3);
        assert_eq!(fixture.workflow.core.status().unwrap().change_counter, counter + 3);
        let binding = fixture.workflow.core.status().unwrap().binding.unwrap();
        let reader = collection.reader(ReaderRole::Query).unwrap();
        let native = reader.deck_rows().unwrap();
        drop(reader);
        let mut changes = Vec::new();
        for entity in &captured {
            let c::entity_codec::Payload::Deck(payload) = &entity.payload else {
                unreachable!()
            };
            let row = native.iter().find(|d| d.id.0 == payload.id).unwrap();
            assert_eq!(payload.name, row.name.as_bytes());
            let parent = meguri_engine::decks::names::parent(&row.name)
                .map(|name| meguri_engine::decks::names::find(&native, name).unwrap().id.0)
                .unwrap_or(0);
            assert_eq!(payload.parent_id, parent);
            assert_eq!(entity.key, c::entity_codec::id_key(row.id.0));
            assert_eq!(
                entity.name,
                c::entity_codec::entity_name(&binding.facts.record_key.0, c::entity_codec::Kind::Deck, &entity.key)
                    .unwrap()
            );
            assert_eq!((entity.local_rev, entity.acked_rev, entity.server_erev), (1, 0, 0));
            assert!(entity.base_payload.is_none() && entity.acked_payload.is_none());
            assert_eq!(entity.observed_seq, added.seq.0);
            assert_eq!(entity.observed_generation, collection.generation().0);
            let mark = entity.mark.as_ref().unwrap();
            assert_eq!(mark.source, r::DirtySource::Observer);
            assert_eq!(mark.payload_digest, entity.payload.payload_digest().unwrap());
            changes.push(mark.change_seq);
            assert!(
                fixture
                    .workflow
                    .tracking_sendables()
                    .unwrap()
                    .contains_key(&entity.name)
            );
        }
        changes.sort();
        assert_eq!(changes, vec![counter + 1, counter + 2, counter + 3]);
        assert!(
            captured
                .iter()
                .enumerate()
                .all(|(i, e)| captured[..i].iter().all(|p| p.origin != e.origin))
        );
        let duplicate = meguri_engine::decks::add(&collection, "HELD ACCOUNT::ANCESTOR::NEW DECK".into()).unwrap();
        assert_eq!(duplicate.value.id, added.value.id);
        assert!(captured_decks(&fixture, "Held account") == captured);
        assert_eq!(fixture.workflow.core.status().unwrap().change_counter, counter + 3);

        let review = fixture.workflow.host.review();
        let meguri_engine::session::StartResult::Started { session_id, head, .. } =
            review.start(review_deck, None).unwrap()
        else {
            panic!("real review must start")
        };
        let card = &head.items[0];
        assert!(matches!(
            review
                .answer(
                    session_id,
                    card.card_id,
                    meguri_sched::answer::Rating::Good,
                    card.state_token,
                    meguri_model::time::EpochMillis(NOW),
                    100,
                )
                .unwrap(),
            meguri_engine::session::AnswerOutcome::Accepted { .. }
        ));
        review.end(session_id).unwrap();
        assert!(review.active_session().is_none());
        assert_deck_capture_live(&fixture, &sdk);
        assert_eq!(fixture.workflow.core.adapter_guard(), guard);
        drop(release);
        let observed = read.join().unwrap();
        assert!(complete_deck_account(&fixture, request, observed));
        drain(&policy, policy.clone().project_identity_read(request).unwrap());
        assert!(captured_decks(&fixture, "Held account") == captured);
    });
    assert_deck_capture_live(&fixture, &sdk);
    fixture
        .workflow
        .state_update(sdk.instance_id, b"original-sdk-after-deck-read".to_vec())
        .unwrap();
    assert_eq!(
        fixture.workflow.core.adapter_snapshot().unwrap().serialization,
        Some(b"original-sdk-after-deck-read".to_vec())
    );
    fixture.workflow.host.close().unwrap();
}

#[test]
fn new_deck_rollback_has_no_capture_or_account_invalidation() {
    let provider = SamplingAccount::new();
    let (fixture, sdk, _) = rig_with_provider(provider.clone());
    let collection = fixture.workflow.host.collection().unwrap();
    let before = fixture.workflow.core.adapter_snapshot().unwrap();
    let counter = fixture.workflow.core.status().unwrap().change_counter;
    let request = fixture
        .workflow
        .begin_identity_read(false, Some(sdk.instance_id.clone()))
        .unwrap();
    let (reached, release) = hold_deck_account(&provider);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| read_deck_account(&provider));
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let failed: Result<meguri_engine::OpResult<()>, _> = collection.run(
            meguri_engine::OpSpec::named("rolled back deck capture", meguri_engine::OpSource::UserUndoable),
            |ctx| {
                meguri_engine::decks::add_in(ctx.scope(), "Rollback root::Child")?;
                Err(meguri_engine::EngineError::CollectionUnavailable {
                    reason: "intentional rollback".into(),
                })
            },
        );
        assert!(failed.is_err());
        assert!(fixture.workflow.core.adapter_snapshot().unwrap() == before);
        assert_eq!(fixture.workflow.core.status().unwrap().change_counter, counter);
        assert!(
            collection
                .reader(ReaderRole::Query)
                .unwrap()
                .deck_rows()
                .unwrap()
                .iter()
                .all(|d| !d.name.starts_with("Rollback root"))
        );
        drop(release);
        assert!(complete_deck_account(&fixture, request, read.join().unwrap()));
    });
    assert_deck_capture_live(&fixture, &sdk);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn new_deck_capture_failure_keeps_parent_state_atomic() {
    let provider = SamplingAccount::new();
    let (fixture, sdk, _) = rig_with_provider(provider.clone());
    let collection = fixture.workflow.host.collection().unwrap();
    let before = fixture.workflow.core.adapter_snapshot().unwrap();
    let counter = fixture.workflow.core.status().unwrap().change_counter;
    let request = fixture
        .workflow
        .begin_identity_read(false, Some(sdk.instance_id.clone()))
        .unwrap();
    fixture.workflow.core.arm_capture_failure_for_testing(1).unwrap();
    let added = meguri_engine::decks::add(&collection, "Capture failure::Ancestor::Child".into()).unwrap();
    assert!(
        fixture
            .workflow
            .await_tracking(collection.generation().0, added.seq.0)
            .is_err()
    );
    let after = fixture.workflow.core.adapter_snapshot().unwrap();
    assert!(before.entities == after.entities);
    assert!(captured_decks(&fixture, "Capture failure").is_empty());
    let status = fixture.workflow.core.status().unwrap();
    assert_eq!(status.change_counter, counter);
    assert!(status.tracking_stopped && status.scan_required);
    assert_eq!(
        collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck_rows()
            .unwrap()
            .iter()
            .filter(|d| d.name.starts_with("Capture failure"))
            .count(),
        3
    );
    assert!(!complete_deck_account(&fixture, request, read_deck_account(&provider)));
    assert!(fixture.workflow.validate_sdk(sdk.instance_id).is_err());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn successful_new_deck_capture_keeps_real_account_generation_and_suspend_invalidation() {
    for invalidation in 0..3 {
        let provider = SamplingAccount::new();
        let (fixture, sdk, _) = rig_with_provider(provider.clone());
        let collection = fixture.workflow.host.collection().unwrap();
        let request = fixture
            .workflow
            .begin_identity_read(false, Some(sdk.instance_id.clone()))
            .unwrap();
        let (reached, release) = hold_deck_account(&provider);
        std::thread::scope(|scope| {
            let read = scope.spawn(|| read_deck_account(&provider));
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            let added = meguri_engine::decks::add(&collection, "Guard check deck".into()).unwrap();
            fixture
                .workflow
                .await_tracking(collection.generation().0, added.seq.0)
                .unwrap();
            assert_deck_capture_live(&fixture, &sdk);
            if invalidation == 1 {
                let engine = fixture.workflow.host.engine();
                let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive
                    .replace(|path| {
                        assert!(path.exists());
                        Ok(())
                    })
                    .unwrap();
            } else if invalidation == 0 {
                fixture
                    .workflow
                    .core
                    .adapter_account(None, r::IdentityStatus::SignedOut)
                    .unwrap();
                let isolated = fixture.workflow.core.adapter_snapshot().unwrap();
                meguri_engine::decks::add(&collection, "While isolated".into()).unwrap();
                assert!(fixture.workflow.core.adapter_snapshot().unwrap() == isolated);
                assert!(captured_decks(&fixture, "While isolated").is_empty());
            } else {
                assert_eq!(
                    fixture.workflow.host.engine().suspend(Duration::from_secs(5)).unwrap(),
                    meguri_engine::SuspendOutcome::Suspended
                );
                fixture.workflow.host.engine().resume().unwrap();
            }
            drop(release);
            assert!(!complete_deck_account(&fixture, request, read.join().unwrap()));
            assert!(fixture.workflow.validate_sdk(sdk.instance_id.clone()).is_err());
        });
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn new_deck_real_service_saved_ack_and_next_edit_keep_origin_revision_and_base() {
    use meguri_testkit::cloud::{
        Caller, Config, Mutation, MutationSuccess, Operation, RecordInput, Reply, Scope, Service, SystemFields,
        coordinator::publication_control_record,
    };
    let (fixture, sdk, _) = rig();
    let collection = fixture.workflow.host.collection().unwrap();
    let added = meguri_engine::decks::add(&collection, "Service captured deck".into()).unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, added.seq.0)
        .unwrap();
    let initial = captured_decks(&fixture, "Service captured deck").pop().unwrap();
    assert_eq!((initial.local_rev, initial.acked_rev), (1, 0));
    assert!(initial.base_payload.is_none());
    let run = tempfile::tempdir().unwrap();
    let mut server = Service::open(run.path(), Config::default(), SeededRandom::new(113_114)).unwrap();
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    // This stable-binding fixture's prior epoch is synthetic. Only new deck
    // saves and their later conditional update are asserted against Service.
    server
        .request(
            &scope,
            Caller::default(),
            Operation::CreateZone {
                zone: sdk.collection_zone.clone(),
            },
        )
        .unwrap()
        .result
        .unwrap();
    let mut last_payload = None;
    for revision in 1..=2 {
        if revision == 2 {
            let changed =
                meguri_engine::decks::rename(&collection, added.value.id, "Service captured deck edited".into())
                    .unwrap();
            fixture
                .workflow
                .await_tracking(collection.generation().0, changed.seq.0)
                .unwrap();
        }
        assert_deck_capture_live(&fixture, &sdk);
        let pending = fixture
            .workflow
            .core
            .adapter_snapshot()
            .unwrap()
            .entities
            .into_iter()
            .find(|e| e.name == initial.name)
            .unwrap();
        assert_eq!(pending.origin, initial.origin);
        assert_eq!((pending.local_rev, pending.acked_rev), (revision, revision - 1));
        assert!(pending.base_payload == last_payload);
        let mark = pending.mark.as_ref().unwrap();
        assert_eq!(
            mark.change_seq,
            initial.mark.as_ref().unwrap().change_seq + revision - 1
        );
        let requested = fixture
            .workflow
            .tracking_sendables()
            .unwrap()
            .into_iter()
            .filter(|(name, _)| name == &initial.name)
            .collect();
        let batch = fixture
            .workflow
            .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), requested)
            .unwrap()
            .expect("real new deck must be sendable");
        assert_eq!(batch.saves.len(), 1);
        assert_eq!(batch.saves[0].name, initial.name);
        assert_eq!(batch.saves[0].system_fields.is_some(), revision == 2);
        let changes = batch
            .saves
            .into_iter()
            .map(|record| Mutation::Save {
                record: RecordInput::encrypted(record.name, "MGEntity", record.envelope),
                if_unchanged: record.system_fields.map(SystemFields::from_bytes),
            })
            .collect();
        let reply = server
            .request(
                &scope,
                Caller::default(),
                Operation::Modify {
                    zone: sdk.collection_zone.clone(),
                    atomic: false,
                    changes,
                },
            )
            .unwrap()
            .result
            .unwrap();
        let Reply::Modified(mut results) = reply else {
            panic!("real Service modify required")
        };
        assert_eq!(results.len(), 1);
        let MutationSuccess::Saved(record) = results.pop().unwrap().result.unwrap() else {
            panic!("real Service Saved required")
        };
        let value = publication_control_record(&record).unwrap();
        let binding = fixture.workflow.core.status().unwrap().binding.unwrap();
        let received = c::entity_codec::receive(
            &binding.facts.record_key.0,
            &record.name,
            value.encrypted.as_ref().unwrap(),
            c::entity_codec::AssetInput::Absent,
        )
        .unwrap();
        assert_eq!(received.entity.fields.origin, initial.origin);
        assert_eq!(received.entity.fields.erev, revision);
        assert!(received.payload == Some(pending.payload.clone()));
        fixture
            .workflow
            .sent(
                sdk.instance_id.clone(),
                batch.id,
                vec![CloudRecord {
                    name: record.name,
                    envelope: value.encrypted.unwrap(),
                    system_fields: Some(value.system_fields.0),
                    ..Default::default()
                }],
                vec![],
                vec![],
                vec![],
                1,
            )
            .unwrap();
        fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
        let clean = fixture
            .workflow
            .core
            .adapter_snapshot()
            .unwrap()
            .entities
            .into_iter()
            .find(|e| e.name == initial.name)
            .unwrap();
        assert_eq!(
            (clean.local_rev, clean.acked_rev, clean.server_erev),
            (revision, revision, revision)
        );
        assert_eq!(clean.origin, initial.origin);
        assert!(clean.mark.is_none() && clean.base_payload.is_none() && clean.in_flight.is_empty());
        assert!(clean.acked_payload == Some(pending.payload.clone()));
        assert!(
            !fixture
                .workflow
                .tracking_sendables()
                .unwrap()
                .contains_key(&initial.name)
        );
        last_payload = Some(pending.payload);
    }
    assert_deck_capture_live(&fixture, &sdk);
    fixture.workflow.host.close().unwrap();
}

fn rig() -> (Fixture, CloudEngineRegistration, CloudRecord) {
    rig_with_provider(Arc::new(Account))
}
fn rig_with_provider(identity: Arc<dyn r::IdentityProvider>) -> (Fixture, CloudEngineRegistration, CloudRecord) {
    let (fixture, sdk, incoming, _) = rig_with_provider_and_deck(identity);
    (fixture, sdk, incoming)
}
fn rig_with_provider_and_deck(
    identity: Arc<dyn r::IdentityProvider>,
) -> (Fixture, CloudEngineRegistration, CloudRecord, meguri_model::ids::DeckId) {
    let (fixture, deck) = unbound_fixture(identity);
    let policy = fixture.workflow.policy().unwrap();
    drain(&policy, policy.clone().input(settings(false)).unwrap());
    // Sample the provider retained by this Host before creating its baseline.
    drain(&policy, policy.clone().refresh_account().unwrap());
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    );
    let (sdk, _, incoming) = fixture.baseline();
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
    );
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    let engine = fixture
        .workflow
        .host
        .cloud_policy_driver()
        .unwrap()
        .state()
        .engine
        .unwrap();
    assert_eq!(engine.instance, sdk.instance_id);
    assert!(!engine.stopping);
    (fixture, sdk, incoming, deck)
}
fn unbound_fixture(identity: Arc<dyn r::IdentityProvider>) -> (Fixture, meguri_model::ids::DeckId) {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("collection");
    let backups = directory.path().join("backups");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    let random = Arc::new(SeededRandom::new(114));
    meguri_engine::bootstrap::create(&data.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
    let mut config = HostConfig::new(
        data,
        backups,
        directory.path().join("widget"),
        clock.clone(),
        random.clone(),
        Arc::new(meguri_storage::space::StatvfsSpace),
    );
    config.publication_binding = meguri_engine::device_state::AccountBinding::Connected {
        account: "synthetic-account-A".into(),
        configuration: "synthetic-u114".into(),
        environment: "development".into(),
    };
    config.cloud = Some(r::CloudInputs {
        scope: c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
        identity,
        files: Arc::new(NativeFiles),
    });
    let host = Arc::new(CoreHost::open(config).unwrap());
    let core = host.cloud_runtime().unwrap().clone();
    core.retain_adapter_inspection();
    let fixture = Fixture {
        _directory: directory,
        workflow: CloudWorkflow {
            host,
            core,
            identity: None,
        },
        clock,
        random,
    };
    let deck = meguri_engine::decks::add(
        &fixture.workflow.host.collection().unwrap(),
        "Policy baseline deck".into(),
    )
    .unwrap()
    .value
    .id;
    (fixture, deck)
}
fn settings(paused: bool) -> CloudPolicyEvent {
    CloudPolicyEvent::Settings {
        settings: CloudPolicyDeviceSettings {
            enabled: true,
            paused,
            automatic: true,
            media: false,
        },
    }
}
fn automatic_settings(automatic: bool) -> CloudPolicyEvent {
    CloudPolicyEvent::Settings {
        settings: CloudPolicyDeviceSettings {
            enabled: true,
            paused: false,
            automatic,
            media: false,
        },
    }
}
fn pending_start_fixture() -> (Fixture, Arc<CloudPolicy>, CloudPolicyIssuedCommand) {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    drain(&policy, policy.clone().input(settings(false)).unwrap());
    drain(&policy, policy.clone().refresh_account().unwrap());
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    );
    let collection = fixture.workflow.host.collection().unwrap();
    let media = fixture.workflow.host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    fixture
        .workflow
        .core
        .prepare_adapter_baseline_for_testing(&collection, fixture.clock.as_ref(), fixture.random.as_ref(), None)
        .unwrap();
    fixture
        .workflow
        .core
        .reconcile_binding(&collection, &media, Duration::from_secs(1))
        .unwrap();
    let start = select(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
        |a| matches!(a, CloudPolicyAction::StartEngine { automatic: true, .. }),
    );
    let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert!(state.gate_open() && state.engine.is_none());
    assert!(fixture.workflow.core.status().unwrap().binding.is_some());
    assert!(fixture.workflow.core.incoming_sdk_instance_for_testing().is_none());
    (fixture, policy, start)
}
fn scheduling_commands(
    policy: &Arc<CloudPolicy>,
    automatic: bool,
) -> (CloudPolicyIssuedCommand, CloudPolicyIssuedCommand) {
    let mut start = None;
    let mut configure = None;
    for command in commands(policy.clone().input(automatic_settings(automatic)).unwrap()) {
        match command.action {
            CloudPolicyAction::StartEngine { automatic: value, .. } => {
                assert_eq!(value, automatic);
                assert!(start.replace(command).is_none());
            }
            CloudPolicyAction::ConfigureAutomatic { on } => {
                assert_eq!(on, automatic);
                assert!(configure.replace(command).is_none());
            }
            _ => drain(
                policy,
                CloudPolicyOutput::Commands {
                    commands: vec![command],
                },
            ),
        }
    }
    (
        start.expect("fresh pending start"),
        configure.expect("authoritative configuration"),
    )
}
fn commands(output: CloudPolicyOutput) -> Vec<CloudPolicyIssuedCommand> {
    match output {
        CloudPolicyOutput::Commands { commands } => commands,
        _ => panic!("expected commands"),
    }
}
fn drain(policy: &Arc<CloudPolicy>, output: CloudPolicyOutput) {
    let mut pending: VecDeque<_> = commands(output).into();
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    while let Some(command) = pending.pop_front() {
        match command.action {
            CloudPolicyAction::UpdatePending { .. }
            | CloudPolicyAction::ApplyReady { .. }
            | CloudPolicyAction::RecordExchange { .. }
            | CloudPolicyAction::RecordQuietPoint { .. }
            | CloudPolicyAction::ProbeTracking
            | CloudPolicyAction::StatusChanged { .. } => {
                let followups = commands(policy.clone().execute_core(command.handle).unwrap());
                for followup in followups.into_iter().rev() {
                    pending.push_front(followup);
                }
            }
            CloudPolicyAction::StartEngine { .. } | CloudPolicyAction::StopEngine { .. } => {
                panic!("setup must retain the real live SDK")
            }
            CloudPolicyAction::ConfigureAutomatic { .. } => {
                let before = queue.0.load(Ordering::SeqCst);
                let result = policy.clone().submit_native(command.handle, queue.clone()).unwrap();
                match result {
                    CloudPolicyAdmission::NoOperation => assert_eq!(queue.0.load(Ordering::SeqCst), before),
                    CloudPolicyAdmission::Admitted { accepted: true } => {
                        assert_eq!(queue.0.load(Ordering::SeqCst), before + 1);
                    }
                    _ => panic!("configuration was neither retained nor admitted"),
                }
            }
            _ => assert!(matches!(
                policy.clone().submit_native(command.handle, queue.clone()).unwrap(),
                CloudPolicyAdmission::Admitted { accepted: true }
            )),
        }
    }
}
fn select(
    policy: &Arc<CloudPolicy>,
    output: CloudPolicyOutput,
    predicate: impl Fn(&CloudPolicyAction) -> bool,
) -> CloudPolicyIssuedCommand {
    let mut selected = None;
    for command in commands(output) {
        if predicate(&command.action) {
            assert!(selected.is_none(), "one required actual Driver command");
            selected = Some(command);
        } else {
            drain(
                policy,
                CloudPolicyOutput::Commands {
                    commands: vec![command],
                },
            );
        }
    }
    selected.expect("required actual Driver command")
}
fn run(policy: &Arc<CloudPolicy>) -> CloudPolicyIssuedCommand {
    select(
        policy,
        policy
            .clone()
            .input(CloudPolicyEvent::SyncNowRequested {
                source: CloudPolicySource::Key,
            })
            .unwrap(),
        |a| matches!(a, CloudPolicyAction::RunExchange { .. }),
    )
}
fn failure_is<T>(result: Result<T, CoreError>, expected: CloudPolicyFailure) {
    assert!(matches!(result, Err(CoreError::CloudPolicy { failure }) if failure == expected));
}
struct Queue(AtomicUsize);
impl CloudPolicyQueue for Queue {
    fn enqueue(&self) -> bool {
        #[cfg(debug_assertions)]
        assert!(std::panic::catch_unwind(crate::core_version).is_err());
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}
struct RejectQueue(AtomicUsize);
impl CloudPolicyQueue for RejectQueue {
    fn enqueue(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        false
    }
}

#[test]
fn policy_bound_pending_start_revalidates_automatic_without_invalidating_manual_work() {
    let (fixture, policy, start_true) = pending_start_fixture();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let runtime = &fixture.workflow.core;
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    assert!(matches!(
        policy
            .clone()
            .submit_native(start_true.handle.clone(), queue.clone())
            .unwrap(),
        CloudPolicyAdmission::Admitted { accepted: true }
    ));
    let manual = run(&policy);
    let guard = driver.state().guard();
    let backend_guard = runtime.adapter_guard();
    let (start_false, configure_off) = scheduling_commands(&policy, false);
    assert_eq!(driver.state().guard(), guard);
    assert_eq!(runtime.adapter_guard(), backend_guard);
    assert!(driver.state().gate_open() && driver.state().engine.is_none());
    // Revalidate the already admitted original before async construction can
    // use its captured true value. Off must remain executable pending intent.
    failure_is(
        policy.clone().submit_native(start_true.handle, queue.clone()),
        CloudPolicyFailure::StaleCommand,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    for command in [configure_off, start_false] {
        assert!(matches!(
            policy.clone().submit_native(command.handle, queue.clone()).unwrap(),
            CloudPolicyAdmission::Admitted { accepted: true }
        ));
    }
    assert_eq!(queue.0.load(Ordering::SeqCst), 3);

    let (queued_start_true, queued_configure_true) = scheduling_commands(&policy, true);
    for command in [&queued_start_true, &queued_configure_true] {
        assert!(matches!(
            policy
                .clone()
                .submit_native(command.handle.clone(), queue.clone())
                .unwrap(),
            CloudPolicyAdmission::Admitted { accepted: true }
        ));
    }
    let (fresh_start_false, fresh_configure_off) = scheduling_commands(&policy, false);
    for command in [queued_start_true, queued_configure_true] {
        failure_is(
            policy.clone().submit_native(command.handle, queue.clone()),
            CloudPolicyFailure::StaleCommand,
        );
    }
    assert_eq!(queue.0.load(Ordering::SeqCst), 5);
    assert_eq!(driver.state().guard(), guard);
    assert_eq!(runtime.adapter_guard(), backend_guard);
    for command in [fresh_configure_off, fresh_start_false, manual] {
        assert!(matches!(
            policy.clone().submit_native(command.handle, queue.clone()).unwrap(),
            CloudPolicyAdmission::Admitted { accepted: true }
        ));
    }
    assert_eq!(queue.0.load(Ordering::SeqCst), 8);
    assert!(!driver.state().automatic());
    assert!(driver.state().engine.is_none());
    assert!(runtime.incoming_sdk_instance_for_testing().is_none());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_bound_pending_configuration_retains_pause_account_generation_and_close_guards() {
    for invalidation in 0..4 {
        let (fixture, policy, _) = pending_start_fixture();
        let (start, configure) = scheduling_commands(&policy, false);
        match invalidation {
            0 => {
                fixture.workflow.set_paused(true).unwrap();
                fixture.workflow.set_paused(false).unwrap();
            }
            1 => fixture
                .workflow
                .core
                .adapter_account(None, r::IdentityStatus::SignedOut)
                .unwrap(),
            2 => {
                let engine = fixture.workflow.host.engine();
                let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive
                    .replace(|path| {
                        assert!(path.exists());
                        Ok(())
                    })
                    .unwrap();
                drop(exclusive);
                drop(slot);
            }
            3 => fixture.workflow.host.close().unwrap(),
            _ => unreachable!(),
        }
        let queue = Arc::new(Queue(AtomicUsize::new(0)));
        for command in [start, configure] {
            failure_is(
                policy.clone().submit_native(command.handle, queue.clone()),
                match invalidation {
                    0 | 1 => CloudPolicyFailure::StaleCommand,
                    2 => CloudPolicyFailure::GenerationChanged,
                    3 => CloudPolicyFailure::Closed,
                    _ => unreachable!(),
                },
            );
        }
        assert_eq!(queue.0.load(Ordering::SeqCst), 0);
        if invalidation != 3 {
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn policy_bound_sdk_registration_refuses_duplicate_and_retains_current_configuration_intent() {
    let (fixture, policy, start_true) = pending_start_fixture();
    let (start_false, configure_off) = scheduling_commands(&policy, false);
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let runtime = &fixture.workflow.core;
    let guard = driver.state().guard();
    let backend_guard = runtime.adapter_guard();
    let sdk = fixture.workflow.register_sdk(false).unwrap();
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    // First registration preserves opening/command guards. A duplicate is
    // refused by Runtime itself, not by inventing a policy invalidation.
    assert!(matches!(
        runtime.register_sdk(true),
        Err(r::RuntimeError::Tracking(c::Error::InvalidData))
    ));
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    assert_eq!(
        runtime.incoming_sdk_instance_for_testing(),
        Some(sdk.instance_id.clone())
    );
    assert_eq!(runtime.adapter_guard(), backend_guard);
    assert_eq!(driver.state().guard(), guard);
    assert!(driver.state().engine.is_none()); // No Driver refresh or fabricated SDK acknowledgement.
    assert!(!driver.state().automatic());

    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    failure_is(
        policy.clone().submit_native(start_true.handle, queue.clone()),
        CloudPolicyFailure::StaleCommand,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    for command in [start_false, configure_off] {
        assert!(matches!(
            policy.clone().submit_native(command.handle, queue.clone()).unwrap(),
            CloudPolicyAdmission::Admitted { accepted: true }
        ));
    }
    assert_eq!(queue.0.load(Ordering::SeqCst), 2);
    assert!(!driver.state().automatic());
    fixture.workflow.validate_sdk(sdk.instance_id).unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_unbound_host_controls_keep_settings_without_sdk_or_configuration_queue() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    let runtime = &fixture.workflow.core;
    let guard = runtime.adapter_guard();
    assert_eq!(guard.generation, 0);
    assert!(guard.binding.is_empty());
    assert!(fixture.workflow.host.engine().generation().0 > 0);
    let configure = select(&policy, policy.clone().input(settings(false)).unwrap(), |a| {
        matches!(a, CloudPolicyAction::ConfigureAutomatic { on: true })
    });
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    assert!(matches!(
        policy.clone().submit_native(configure.handle, queue.clone()).unwrap(),
        CloudPolicyAdmission::NoOperation
    ));
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.adapter_guard(), guard);
    assert!(runtime.status().unwrap().binding.is_none());
    assert!(runtime.incoming_sdk_instance_for_testing().is_none());
    let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert!(state.settings.automatic && state.engine.is_none());
    let configure_off = select(&policy, policy.clone().input(automatic_settings(false)).unwrap(), |a| {
        matches!(a, CloudPolicyAction::ConfigureAutomatic { on: false })
    });
    assert!(matches!(
        policy
            .clone()
            .submit_native(configure_off.handle, queue.clone())
            .unwrap(),
        CloudPolicyAdmission::NoOperation
    ));
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.adapter_guard(), guard);

    let quit = select(
        &policy,
        policy.clone().input(CloudPolicyEvent::QuitRequested).unwrap(),
        |a| matches!(a, CloudPolicyAction::QuitNow),
    );
    let rejected = Arc::new(RejectQueue(AtomicUsize::new(0)));
    assert!(matches!(
        policy
            .clone()
            .submit_native(quit.handle.clone(), rejected.clone())
            .unwrap(),
        CloudPolicyAdmission::Admitted { accepted: false }
    ));
    assert_eq!(rejected.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        policy.clone().submit_native(quit.handle, queue.clone()).unwrap(),
        CloudPolicyAdmission::Admitted { accepted: true }
    ));
    let cancel = select(
        &policy,
        policy.clone().input(CloudPolicyEvent::BackgroundExpiring).unwrap(),
        |a| matches!(a, CloudPolicyAction::CancelBackgroundFlush),
    );
    assert!(matches!(
        policy.clone().submit_native(cancel.handle, queue.clone()).unwrap(),
        CloudPolicyAdmission::Admitted { accepted: true }
    ));
    assert_eq!(queue.0.load(Ordering::SeqCst), 2);
    assert!(runtime.incoming_sdk_instance_for_testing().is_none());
    assert!(runtime.status().unwrap().binding.is_none());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_unbound_configuration_rejects_pause_account_generation_and_close_before_queue() {
    for invalidation in 0..4 {
        let (fixture, _) = unbound_fixture(Arc::new(Account));
        let policy = fixture.workflow.policy().unwrap();
        let configure = select(&policy, policy.clone().input(settings(false)).unwrap(), |a| {
            matches!(a, CloudPolicyAction::ConfigureAutomatic { .. })
        });
        match invalidation {
            0 => {
                fixture.workflow.set_paused(true).unwrap();
                fixture.workflow.set_paused(false).unwrap();
            }
            1 => drain(&policy, policy.clone().refresh_account().unwrap()),
            2 => {
                let engine = fixture.workflow.host.engine();
                let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive
                    .replace(|path| {
                        assert!(path.exists());
                        Ok(())
                    })
                    .unwrap();
                drop(exclusive);
                drop(slot);
            }
            3 => fixture.workflow.host.close().unwrap(),
            _ => unreachable!(),
        }
        let queue = Arc::new(Queue(AtomicUsize::new(0)));
        failure_is(
            policy.clone().submit_native(configure.handle, queue.clone()),
            match invalidation {
                0 | 1 => CloudPolicyFailure::StaleCommand,
                2 => CloudPolicyFailure::GenerationChanged,
                3 => CloudPolicyFailure::Closed,
                _ => unreachable!(),
            },
        );
        assert_eq!(queue.0.load(Ordering::SeqCst), 0);
        assert!(fixture.workflow.core.incoming_sdk_instance_for_testing().is_none());
        if invalidation != 3 {
            fixture.workflow.host.close().unwrap();
        }
    }
}

struct SamplingAccount {
    value: Mutex<r::IdentityStatus>,
    after_sample: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl SamplingAccount {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(<Account as r::IdentityProvider>::read_identity(
                &Account,
                &c::BuildScope {
                    container: c::ContainerId::new("synthetic-u114").unwrap(),
                    environment: c::Environment::Development,
                },
            )),
            after_sample: Mutex::new(None),
        })
    }
}
impl r::IdentityProvider for SamplingAccount {
    fn read_identity(&self, _: &c::BuildScope) -> r::IdentityStatus {
        let sample = self.value.lock().unwrap().clone();
        let after_sample = self.after_sample.lock().unwrap().take();
        if let Some(after_sample) = after_sample {
            after_sample();
        }
        sample
    }
}

#[test]
fn policy_failed_provider_refresh_keeps_actual_verified_sdk_owner_and_pending() {
    let provider = SamplingAccount::new();
    let (fixture, sdk, incoming, deck) = rig_with_provider_and_deck(provider.clone());
    let policy = fixture.workflow.policy().unwrap();
    let collection = fixture.workflow.host.collection().unwrap();
    meguri_engine::decks::rename(&collection, deck, "Policy pending deck".into()).unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, collection.last_seq().0)
        .unwrap();
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Local,
            })
            .unwrap(),
    );
    let pending = policy.status().unwrap().pending;
    assert!(pending > 0, "real local edit must be pending before failed sampling");
    assert!(fixture.workflow.core.status().unwrap().identity_verified);
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    let before = fixture.workflow.core.adapter_snapshot().unwrap();
    let guard = fixture.workflow.core.adapter_guard();
    *provider.value.lock().unwrap() = r::IdentityStatus::ReadFailed;
    let output = policy.clone().refresh_account().unwrap();
    let after = fixture.workflow.core.adapter_snapshot().unwrap();
    assert!(before == after, "failed sampling changed durable pending or cursor");
    assert_eq!(fixture.workflow.core.adapter_guard(), guard);
    assert!(fixture.workflow.core.status().unwrap().identity_verified);
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    let status = policy.status().unwrap();
    assert!(matches!(status.account_status, CloudPolicyAccountStatus::Available));
    assert!(status.account_matches_binding);
    assert_eq!(status.pending, pending);
    assert!(
        !fixture
            .workflow
            .host
            .cloud_policy_driver()
            .unwrap()
            .state()
            .engine
            .unwrap()
            .stopping
    );
    assert!(!commands(output.clone()).iter().any(|c| matches!(
        c.action,
        CloudPolicyAction::StopEngine { .. } | CloudPolicyAction::StartEngine { .. }
    )));
    drain(&policy, output);
    fixture
        .workflow
        .core
        .adapter_fetched(&sdk.instance_id, &sdk.collection_zone, &[incoming.saved()], &[])
        .unwrap();
    fixture
        .workflow
        .state_update(sdk.instance_id, b"after-failed-policy-read".to_vec())
        .unwrap();
    assert_eq!(
        fixture.workflow.core.adapter_snapshot().unwrap().serialization,
        Some(b"after-failed-policy-read".to_vec())
    );
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_delayed_provider_sample_cannot_cross_pause_account_generation_or_close() {
    for invalidation in 0..4 {
        let provider = SamplingAccount::new();
        let (fixture, _, _) = rig_with_provider(provider.clone());
        let policy = fixture.workflow.policy().unwrap();
        let original = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        let host = fixture.workflow.host.clone();
        let runtime = fixture.workflow.core.clone();
        *provider.after_sample.lock().unwrap() = Some(Box::new(move || {
            let _ = host.cloud_policy_driver().unwrap().status();
            match invalidation {
                0 => {
                    runtime.adapter_pause(true).unwrap();
                    runtime.adapter_pause(false).unwrap();
                }
                1 => runtime.adapter_account(None, r::IdentityStatus::SignedOut).unwrap(),
                2 => {
                    let engine = host.engine();
                    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                    let mut exclusive = slot.enter_exclusive(true).unwrap();
                    exclusive
                        .replace(|path| {
                            assert!(path.exists());
                            Ok(())
                        })
                        .unwrap();
                    drop(exclusive);
                    drop(slot);
                }
                3 => host.close().unwrap(),
                _ => unreachable!(),
            }
        }));
        failure_is(
            policy.clone().refresh_account(),
            match invalidation {
                0 | 1 => CloudPolicyFailure::StaleCommand,
                2 => CloudPolicyFailure::GenerationChanged,
                3 => CloudPolicyFailure::Closed,
                _ => unreachable!(),
            },
        );
        let current = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert_eq!(current.guard(), original.guard());
        assert_eq!(current.account, original.account, "stale sample reached policy");
        if invalidation == 1 {
            assert!(!fixture.workflow.core.status().unwrap().identity_verified);
        }
        if invalidation != 3 {
            fixture.workflow.host.close().unwrap();
        }
    }
}

struct ProjectionCountingAccount(AtomicUsize);
impl r::IdentityProvider for ProjectionCountingAccount {
    fn read_identity(&self, scope: &c::BuildScope) -> r::IdentityStatus {
        self.0.fetch_add(1, Ordering::SeqCst);
        <Account as r::IdentityProvider>::read_identity(&Account, scope)
    }
}

#[test]
fn accepted_policy_projection_never_resamples_or_consumes_the_next_actual_read() {
    let provider = Arc::new(ProjectionCountingAccount(AtomicUsize::new(0)));
    let (fixture, sdk, _) = rig_with_provider(provider.clone());
    let policy = fixture.workflow.policy().unwrap();
    let samples = provider.0.load(Ordering::SeqCst);
    let before = fixture.workflow.core.adapter_snapshot().unwrap();
    let a = fixture.workflow.begin_identity_read(false, None).unwrap();
    assert!(
        fixture
            .workflow
            .complete_identity_read(a, CloudAccountStatus::Available, Some(b"synthetic-account-A".to_vec()),)
            .unwrap()
    );
    let original = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    let b = fixture.workflow.begin_identity_read(true, None).unwrap();
    assert_eq!(b, a + 1);
    failure_is(
        policy.clone().project_identity_read(a),
        CloudPolicyFailure::StaleCommand,
    );
    let current = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert_eq!(current.guard(), original.guard());
    assert_eq!(current.account, original.account);
    assert!(
        fixture
            .workflow
            .complete_identity_read(b, CloudAccountStatus::Available, Some(b"synthetic-account-B".to_vec()),)
            .unwrap()
    );
    let output = policy.clone().project_identity_read(b).unwrap();
    assert_eq!(
        provider.0.load(Ordering::SeqCst),
        samples,
        "projection sampled cached account A"
    );
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap();
    assert_eq!(
        binding.facts.state,
        c::BindingState::Isolated(c::IsolationReason::AccountChanged)
    );
    assert!(!fixture.workflow.core.status().unwrap().identity_verified);
    assert!(fixture.workflow.validate_sdk(sdk.instance_id.clone()).is_err());
    assert_eq!(
        fixture.workflow.core.adapter_snapshot().unwrap().serialization,
        before.serialization
    );
    let status = policy.status().unwrap();
    assert!(matches!(status.account_status, CloudPolicyAccountStatus::Available));
    assert!(!status.account_matches_binding);
    assert!(
        commands(output)
            .iter()
            .any(|c| matches!(c.action, CloudPolicyAction::StopEngine { .. }))
    );
    let next = fixture.workflow.begin_identity_read(false, None).unwrap();
    assert_eq!(next, b + 1);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn accepted_policy_projection_preserves_closed_generation_and_stale_precedence() {
    for invalidation in 0..3 {
        let provider = Arc::new(ProjectionCountingAccount(AtomicUsize::new(0)));
        let (fixture, _, _) = rig_with_provider(provider.clone());
        let policy = fixture.workflow.policy().unwrap();
        let request = fixture.workflow.begin_identity_read(false, None).unwrap();
        assert!(
            fixture
                .workflow
                .complete_identity_read(
                    request,
                    CloudAccountStatus::Available,
                    Some(b"synthetic-account-A".to_vec()),
                )
                .unwrap()
        );
        let samples = provider.0.load(Ordering::SeqCst);
        let original = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        match invalidation {
            0 => {
                fixture.workflow.core.adapter_pause(true).unwrap();
                fixture.workflow.core.adapter_pause(false).unwrap();
            }
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
                drop(exclusive);
                drop(slot);
            }
            2 => fixture.workflow.host.close().unwrap(),
            _ => unreachable!(),
        }
        failure_is(
            policy.clone().project_identity_read(request),
            match invalidation {
                0 => CloudPolicyFailure::StaleCommand,
                1 => CloudPolicyFailure::GenerationChanged,
                2 => CloudPolicyFailure::Closed,
                _ => unreachable!(),
            },
        );
        assert_eq!(provider.0.load(Ordering::SeqCst), samples);
        let current = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert_eq!(current.guard(), original.guard());
        assert_eq!(current.account, original.account);
        if invalidation != 2 {
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn policy_is_one_original_host_handle_and_real_pause_invalidates_issued_native_work() {
    let (fixture, _, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    assert!(Arc::ptr_eq(&policy, &fixture.workflow.policy().unwrap()));
    let run = run(&policy);
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    assert!(matches!(
        policy.clone().submit_native(run.handle.clone(), queue.clone()).unwrap(),
        CloudPolicyAdmission::Admitted { accepted: true }
    ));
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.set_paused(true).unwrap();
    failure_is(
        policy.clone().submit_native(run.handle.clone(), queue.clone()),
        CloudPolicyFailure::StaleCommand,
    );
    // Unpausing cannot refresh authority on the original command.
    fixture.workflow.set_paused(false).unwrap();
    failure_is(
        policy.clone().submit_native(run.handle, queue.clone()),
        CloudPolicyFailure::StaleCommand,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_wrong_owner_close_and_successor_never_retarget_original_command() {
    let (first, _, _) = rig();
    let original = first.workflow.policy().unwrap();
    let run = run(&original);
    let (second, _, _) = rig();
    let successor = second.workflow.policy().unwrap();
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    failure_is(
        successor.submit_native(run.handle.clone(), queue.clone()),
        CloudPolicyFailure::WrongOwner,
    );
    first.workflow.host.close().unwrap();
    failure_is(original.status(), CloudPolicyFailure::Closed);
    failure_is(
        original.submit_native(run.handle, queue.clone()),
        CloudPolicyFailure::Closed,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    second.workflow.host.close().unwrap();
}

#[test]
fn real_generation_swap_invalidates_retained_policy_before_any_queue_callback() {
    let (fixture, _, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    let run = run(&policy);
    let engine = fixture.workflow.host.engine();
    let old_generation = engine.generation();
    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    // Real close/reopen and generation machinery, with the disposable file
    // preserved. No test-only generation DTO or policy state mutation.
    exclusive
        .replace(|path| {
            assert!(path.exists());
            Ok(())
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    assert!(engine.generation().0 > old_generation.0);
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    failure_is(
        policy.clone().submit_native(run.handle, queue.clone()),
        CloudPolicyFailure::GenerationChanged,
    );
    failure_is(policy.status(), CloudPolicyFailure::GenerationChanged);
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn failed_durable_evidence_returns_original_stop_followups_as_typed_output() {
    let (fixture, sdk, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    drain(
        &policy,
        policy
            .clone()
            .input(CloudPolicyEvent::Operation {
                instance: sdk.instance_id.clone(),
                operation: CloudPolicyOperation {
                    zone: CloudPolicyZone::Collection,
                    kind: CloudPolicyOperationKind::Fetch,
                },
                phase: CloudPolicyActivity::Started,
            })
            .unwrap(),
    );
    fixture.workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
    assert!(
        !fixture
            .workflow
            .host
            .cloud_policy_driver()
            .unwrap()
            .state()
            .engine
            .unwrap()
            .stopping
    );
    let run = run(&policy);
    let CloudPolicyAction::RunExchange { exchange, .. } = run.action else {
        unreachable!()
    };
    let record = select(
        &policy,
        policy
            .clone()
            .end_before_capture(exchange, CloudPolicyEarlyEnd::Cancelled)
            .unwrap(),
        |a| matches!(a, CloudPolicyAction::RecordExchange { .. }),
    );
    fixture
        .workflow
        .core
        .policy_evidence_constraint_for_testing(true)
        .unwrap();
    let output = policy.clone().execute_core(record.handle).unwrap();
    fixture
        .workflow
        .core
        .policy_evidence_constraint_for_testing(false)
        .unwrap();
    let CloudPolicyOutput::CoreActionFailed { error, commands } = output else {
        panic!("failure followups lost")
    };
    assert!(matches!(
        error,
        CoreError::CloudConnection {
            failure: crate::cloud_connection::CloudConnectionFailure::Tracking
        }
    ));
    let evidence = fixture
        .workflow
        .core
        .policy_projection(
            meguri_sync::cloud::policy::Lifecycle::Ready,
            fixture.workflow.host.engine().generation().0,
        )
        .unwrap()
        .evidence;
    assert!(evidence.quiet_ms.is_none() && evidence.last_attempt.is_none() && evidence.last_completed.is_none());
    let stops: Vec<_> = commands
        .iter()
        .filter(|c| matches!(c.action, CloudPolicyAction::StopEngine { .. }))
        .map(|c| c.handle.clone())
        .collect();
    assert_eq!(stops.len(), 1, "required StopEngine");
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    for command in commands {
        if matches!(command.action, CloudPolicyAction::StopEngine { .. }) {
            assert!(matches!(
                policy.clone().submit_native(command.handle, queue.clone()).unwrap(),
                CloudPolicyAdmission::Admitted { accepted: true }
            ));
        } else {
            drain(
                &policy,
                CloudPolicyOutput::Commands {
                    commands: vec![command],
                },
            );
        }
    }
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.retire_sdk(sdk.instance_id.clone(), true).unwrap();
    fixture.workflow.release_sdk(sdk.instance_id).unwrap();
    failure_is(
        policy.clone().submit_native(stops[0].clone(), queue.clone()),
        CloudPolicyFailure::StaleCommand,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    let status = policy.status().unwrap();
    assert!(status.as_of_ms.is_none() && status.last_attempt.is_none() && status.last_completed.is_none());
    assert!(
        status
            .attention
            .iter()
            .any(|a| a.kind == CloudPolicyAttention::TrackingError)
    );
    fixture.workflow.host.close().unwrap();
}

#[test]
fn actual_apply_rollback_is_deferred_and_keeps_incoming_without_tracking_stop() {
    for interruption in [ApplyInterruption::Cancel, ApplyInterruption::Error] {
        let (fixture, sdk, incoming) = rig();
        let policy = fixture.workflow.policy().unwrap();
        // Stage with Runtime's SDK validation; do not use CloudWorkflow.fetched,
        // which also drains and would consume the action under test.
        fixture
            .workflow
            .core
            .adapter_fetched(
                &sdk.instance_id,
                &sdk.collection_zone,
                &[w::Saved {
                    name: incoming.name,
                    envelope: incoming.envelope,
                    system_fields: incoming.system_fields.unwrap(),
                }],
                &[],
            )
            .unwrap();
        let apply = select(
            &policy,
            policy
                .clone()
                .input(CloudPolicyEvent::Refresh {
                    cause: CloudPolicyChangeCause::Cloud,
                })
                .unwrap(),
            |a| matches!(a, CloudPolicyAction::ApplyReady { .. }),
        );
        fixture.workflow.core.interrupt_next_incoming_for_testing(interruption);
        let CloudPolicyOutput::ApplyDeferred { error } = policy.clone().execute_core(apply.handle).unwrap() else {
            panic!("rollback became a tracking failure or success")
        };
        assert!(matches!(
            (interruption, error),
            (ApplyInterruption::Cancel, CoreError::Cancelled) | (ApplyInterruption::Error, CoreError::Internal { .. })
        ));
        assert_eq!(fixture.incoming_value(), None);
        assert!(!fixture.workflow.core.status().unwrap().tracking_stopped);
        let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
        assert_eq!(fixture.workflow.core.inbox_entries(binding).unwrap().len(), 1);
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn policy_empty_media_ledger_and_unqualified_completion_never_claim_success() {
    let (fixture, _, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let status = policy.status().unwrap();
    assert!(status.as_of_ms.is_none());
    assert_eq!(status.media.upload_files, Some(0));
    assert_eq!(status.media.upload_bytes, Some(0));
    assert_eq!(status.media.download_files, Some(0));
    assert_eq!(status.media.download_bytes, Some(0));
    assert_eq!(status.media.unknown_blobs, Some(0));
    assert_eq!(status.media.missing_names, Some(0));
    assert!(status.media.uploads_running.is_none() && status.media.downloads_running.is_none());
    assert!(status.media.remaining_download_ms.is_none());
    assert!(!matches!(status.state, CloudPolicyStatusState::UpToDate { .. }));
    assert!(matches!(status.sync_now, CloudPolicySyncNowAvailability::Available));
    failure_is(
        policy.clone().complete_exchange(9_999),
        CloudPolicyFailure::UnqualifiedCompletion,
    );
    policy.clone().input(settings(true)).unwrap();
    let refused = commands(
        policy
            .clone()
            .input(CloudPolicyEvent::SyncNowRequested {
                source: CloudPolicySource::Command,
            })
            .unwrap(),
    );
    assert!(
        !refused
            .iter()
            .any(|c| matches!(c.action, CloudPolicyAction::RunExchange { .. }))
    );
    assert!(matches!(
        policy.status().unwrap().sync_now,
        CloudPolicySyncNowAvailability::Unavailable {
            reason: CloudPolicyUnavailable::Paused
        }
    ));
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_without_binding_has_unknown_media_counts() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let status = policy.status().unwrap();
    assert!(fixture.workflow.core.status().unwrap().binding.is_none());
    assert!(status.media.upload_files.is_none() && status.media.upload_bytes.is_none());
    assert!(status.media.download_files.is_none() && status.media.download_bytes.is_none());
    assert!(status.media.unknown_blobs.is_none() && status.media.missing_names.is_none());
    assert!(status.as_of_ms.is_none());
    assert!(!matches!(status.state, CloudPolicyStatusState::UpToDate { .. }));
    failure_is(
        policy.clone().complete_exchange(9_999),
        CloudPolicyFailure::UnqualifiedCompletion,
    );
    fixture.workflow.host.close().unwrap();
}
