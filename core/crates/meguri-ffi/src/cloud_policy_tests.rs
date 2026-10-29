//! Actual exported methods over a retained Host; no DTO supplies core facts.
use super::*;
use crate::cloud_policy::*;
use meguri_sync::cloud::testing::ApplyInterruption;

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
fn policy_unknown_projection_and_unqualified_completion_never_claim_success() {
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
    assert!(status.media.upload_files.is_none() && status.media.download_files.is_none());
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
