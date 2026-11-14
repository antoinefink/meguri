//! P4/V3.4: real retained Host, issued commands and guarded registration ports.
use super::*;
use crate::cloud_policy::*;
use meguri_sync::cloud::policy as p;

fn fixture() -> Fixture {
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
        identity: Arc::new(Account),
        files: Arc::new(NativeFiles),
    });
    let host = Arc::new(CoreHost::open(config).unwrap());
    let core = host.cloud_runtime().unwrap().clone();
    Fixture {
        _directory: directory,
        workflow: CloudWorkflow {
            host,
            core,
            identity: None,
        },
        clock,
        random,
    }
}
fn commands(output: CloudPolicyOutput) -> Vec<CloudPolicyIssuedCommand> {
    let CloudPolicyOutput::Commands { commands } = output else {
        panic!("expected actual policy commands")
    };
    commands
}
fn settings(automatic: bool, paused: bool) -> CloudPolicyEvent {
    CloudPolicyEvent::Settings {
        settings: CloudPolicyDeviceSettings {
            enabled: true,
            paused,
            automatic,
            media: false,
        },
    }
}
fn start() -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>) {
    let (fixture, policy, execution, _) = start_with_root();
    (fixture, policy, execution)
}
fn start_with_root() -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>, CloudRecord) {
    start_with_settings(settings(false, false))
}
fn start_with_root_and_accepted_epoch() -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>, CloudRecord) {
    start_with_settings_and_epoch(settings(false, false), true)
}
fn start_with_settings(event: CloudPolicyEvent) -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>, CloudRecord) {
    start_with_settings_and_epoch(event, false)
}
fn start_with_settings_and_epoch(
    event: CloudPolicyEvent,
    accept_epoch: bool,
) -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>, CloudRecord) {
    let fixture = fixture();
    let (sdk, root, _) = if accept_epoch {
        fixture.baseline_with_accepted_epoch()
    } else {
        fixture.baseline()
    };
    fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
    fixture.workflow.release_sdk(sdk.instance_id).unwrap();
    let policy = fixture.workflow.policy().unwrap();
    let mut issued = commands(policy.clone().input(event).unwrap());
    issued.extend(commands(policy.clone().refresh_account().unwrap()));
    issued.extend(commands(
        policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    ));
    let command = issued
        .into_iter()
        .rev()
        .find(|c| matches!(c.action, CloudPolicyAction::StartEngine { .. }))
        .expect("real StartEngine");
    let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
    (fixture, policy, execution, root)
}

#[test]
fn guarded_registration_accepts_once_and_retains_actual_sdk_identity() {
    let (fixture, _policy, execution) = start();
    assert!(execution.original_sdk().unwrap().is_none());
    let registration = execution.register_sdk().unwrap();
    execution.validate_execution().unwrap();
    fixture.workflow.validate_sdk(registration.instance_id).unwrap();
    assert!(execution.register_sdk().is_err());
    fixture.workflow.host.close().unwrap();
}

fn media_settings(automatic: bool, paused: bool) -> CloudPolicyEvent {
    CloudPolicyEvent::Settings {
        settings: CloudPolicyDeviceSettings {
            enabled: true,
            paused,
            automatic,
            media: true,
        },
    }
}

#[test]
fn settings_pause_with_media_succeeds_with_revoked_or_released_original_sdk() {
    use c::testing::IncomingStage;
    use meguri_storage::rows::Table;

    for automatic in [false, true] {
        for retire_first in [false, true] {
            let (fixture, policy, start, _) = start_with_settings(media_settings(automatic, false));
            let sdk = start.register_sdk().unwrap();
            let runtime = &fixture.workflow.core;
            runtime.prepare_cloud_media_sdk(&sdk.instance_id).unwrap();
            let collection = fixture.workflow.host.collection().unwrap();
            let facts = || {
                let reader = collection.reader(ReaderRole::Query).unwrap();
                Table::ALL.map(|table| reader.row_page(table, None).unwrap())
            };
            let before = facts(); // Includes card scheduling and revlog rows.
            let seq = collection.last_seq();
            assert!(fixture.workflow.host.review().active_session().is_none());
            let original_guard = runtime.adapter_guard();
            let issued = commands(policy.clone().input(media_settings(automatic, true)).unwrap());
            let pause = issued
                .iter()
                .find(|c| matches!(c.action, CloudPolicyAction::PauseMedia))
                .unwrap();
            let stop = issued
                .iter()
                .find(|c| matches!(c.action, CloudPolicyAction::StopEngine { .. }))
                .unwrap();
            let pause = policy.clone().prepare_execution(pause.handle.clone()).unwrap().unwrap();
            let stop = policy.clone().prepare_execution(stop.handle.clone()).unwrap().unwrap();
            assert_eq!(pause.original_sdk().unwrap(), Some(sdk.instance_id.clone()));
            assert_eq!(stop.original_sdk().unwrap(), Some(sdk.instance_id.clone()));
            let paused_guard = runtime.adapter_guard();
            assert_ne!(paused_guard.token, original_guard.token);
            assert!(matches!(
                runtime.validate_sdk(&sdk.instance_id),
                Err(r::RuntimeError::StaleIncarnation)
            ));
            if retire_first {
                stop.retire_original().unwrap();
                fixture.workflow.release_sdk(sdk.instance_id.clone()).unwrap();
                assert!(runtime.incoming_sdk_instance_for_testing().is_none());
            }
            let stages = Arc::new(Mutex::new(Vec::new()));
            let observed = stages.clone();
            runtime.set_incoming_stage_hook_for_testing(move |stage| observed.lock().unwrap().push(stage));
            let result = pause.begin_media();
            runtime.set_incoming_stage_hook_for_testing(|_| {});
            assert_eq!(result, Ok(false));
            assert!(!stages.lock().unwrap().contains(&IncomingStage::PolicyMediaPrepared));
            assert!(stages.lock().unwrap().contains(&IncomingStage::PolicyMediaSelected));
            assert!(
                runtime
                    .cleanup_observation_for_testing(&sdk.instance_id)
                    .unwrap()
                    .media_paused
            );
            assert_eq!(runtime.adapter_guard(), paused_guard);
            assert!(matches!(
                runtime.validate_sdk(&sdk.instance_id),
                Err(r::RuntimeError::StaleIncarnation)
            ));
            assert!(runtime.prepare_cloud_media_sdk(&sdk.instance_id).is_err());
            assert!(
                fixture
                    .workflow
                    .begin_fetch(sdk.instance_id.clone(), vec!["meguri-root".into()])
                    .is_err()
            );
            assert!(fixture.workflow.begin_send(sdk.instance_id.clone()).is_err());
            if !retire_first {
                stop.retire_original().unwrap();
                fixture.workflow.release_sdk(sdk.instance_id.clone()).unwrap();
            }
            // Reopening the gate must never restore the original SDK's authority.
            policy.clone().input(media_settings(automatic, false)).unwrap();
            assert!(matches!(
                runtime.validate_sdk(&sdk.instance_id),
                Err(r::RuntimeError::StaleIncarnation)
            ));
            assert_eq!(collection.last_seq(), seq);
            assert_eq!(facts(), before);
            assert!(fixture.workflow.host.review().active_session().is_none());
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn settings_pause_with_media_rejects_stale_policy_runtime_guards_and_wrong_owner() {
    for runtime_only in [false, true] {
        let (fixture, policy, start, _) = start_with_settings(media_settings(false, false));
        start.register_sdk().unwrap();
        let command = commands(policy.clone().input(media_settings(false, true)).unwrap())
            .into_iter()
            .find(|c| matches!(c.action, CloudPolicyAction::PauseMedia))
            .unwrap();
        let pause = policy
            .clone()
            .prepare_execution(command.handle.clone())
            .unwrap()
            .unwrap();
        if runtime_only {
            fixture.workflow.core.adapter_pause(false).unwrap();
        } else {
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let guard = driver.state().guard();
            fixture
                .workflow
                .account_changed(None, CloudAccountStatus::NoAccount, None)
                .unwrap();
            policy
                .clone()
                .input(CloudPolicyEvent::Refresh {
                    cause: CloudPolicyChangeCause::Cloud,
                })
                .unwrap();
            assert_ne!(driver.state().guard(), guard);
        }
        assert!(matches!(
            pause.begin_media(),
            Err(CoreError::CloudPolicy {
                failure: CloudPolicyFailure::StaleCommand
            })
        ));
        fixture.workflow.host.close().unwrap();
        let replacement = self::fixture();
        let other = replacement.workflow.policy().unwrap();
        assert!(matches!(
            other.prepare_execution(command.handle),
            Err(CoreError::CloudPolicy {
                failure: CloudPolicyFailure::WrongOwner
            })
        ));
        assert!(matches!(
            pause.begin_media(),
            Err(CoreError::CloudPolicy {
                failure: CloudPolicyFailure::Closed
            })
        ));
        replacement.workflow.host.close().unwrap();
    }
}

#[test]
fn policy_reads_and_sends_have_no_capability_while_the_original_engine_stops() {
    let (fixture, policy, start, root) = start_with_root_and_accepted_epoch();
    let sdk = start.register_sdk().unwrap();
    let fetch = fixture
        .workflow
        .begin_fetch(sdk.instance_id.clone(), vec!["meguri-root".into()])
        .unwrap();
    fixture
        .workflow
        .will_fetch(sdk.instance_id.clone(), "p4-origin".into(), true)
        .unwrap();
    fixture
        .workflow
        .fetch_zone(sdk.instance_id.clone(), "meguri-root".into(), true, false)
        .unwrap();
    fixture
        .workflow
        .fetched(sdk.instance_id.clone(), "meguri-root".into(), vec![root], vec![])
        .unwrap();
    fixture
        .workflow
        .fetch_zone(sdk.instance_id.clone(), "meguri-root".into(), false, false)
        .unwrap();
    fixture
        .workflow
        .did_fetch(sdk.instance_id.clone(), "p4-origin".into(), true)
        .unwrap();
    assert!(fixture.workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
    let collection = fixture.workflow.host.collection().unwrap();
    let added = meguri_engine::decks::add(&collection, "P4 pending".into()).unwrap();
    fixture
        .workflow
        .await_tracking(collection.generation().0, added.seq.0)
        .unwrap();
    let changed = commands(policy.clone().input(settings(true, false)).unwrap());
    let configure = changed
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::ConfigureAutomatic { .. }))
        .expect("actual scheduling command");
    let pending = commands(policy.clone().input(CloudPolicyEvent::Foreground).unwrap());
    let operations: Vec<_> = pending
        .into_iter()
        .filter(|c| {
            matches!(
                c.action,
                CloudPolicyAction::RequestFetch { .. } | CloudPolicyAction::RequestSend { .. }
            )
        })
        .collect();
    assert_eq!(operations.len(), 2, "actual foreground fetch and send");
    for command in &operations {
        assert!(
            policy
                .clone()
                .prepare_execution(command.handle.clone())
                .unwrap()
                .is_some()
        );
    }
    let configure = policy.clone().prepare_execution(configure.handle).unwrap().unwrap();
    configure.retire_original().unwrap();
    assert!(matches!(
        fixture.workflow.validate_sdk(sdk.instance_id.clone()),
        Err(CoreError::CloudConnection {
            failure: crate::cloud_connection::CloudConnectionFailure::AuthorityChanged
        })
    ));
    for command in &operations {
        assert!(
            policy
                .clone()
                .prepare_execution(command.handle.clone())
                .unwrap()
                .is_none()
        );
    }
    fixture.workflow.release_sdk(sdk.instance_id).unwrap();
    for command in &operations {
        assert!(
            policy
                .clone()
                .prepare_execution(command.handle.clone())
                .unwrap()
                .is_none()
        );
    }
    let _ = policy.clone().input(settings(true, true)).unwrap();
    for command in operations {
        assert!(matches!(
            policy.clone().prepare_execution(command.handle),
            Err(CoreError::CloudPolicy {
                failure: CloudPolicyFailure::StaleCommand
            })
        ));
    }
    fixture.workflow.host.close().unwrap();
}

// engine/jobs J2.1.1: final native enqueue checks stay short under projection.
#[test]
fn registered_execution_final_validation_completes_while_real_driver_projection_is_held() {
    use std::sync::mpsc;

    struct ReleaseProjection(mpsc::Sender<()>);
    impl Drop for ReleaseProjection {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    let (fixture, policy, execution) = start();
    let registration = execution.register_sdk().unwrap();
    execution.validate_execution().unwrap();
    fixture.workflow.validate_sdk(registration.instance_id).unwrap();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (released, did_release) = mpsc::channel();
    driver.set_projection_hook_for_testing(move || {
        entered.send(()).unwrap();
        resume
            .recv_timeout(Duration::from_secs(10))
            .expect("projection release");
        released.send(()).unwrap();
    });
    let (entry, before_release, released, projection, validation) = std::thread::scope(|scope| {
        // This must drop inside the closure, before automatic scope joining.
        let release = ReleaseProjection(release);
        let held = scope.spawn(move || policy.input(CloudPolicyEvent::Foreground));
        let entry = reached.recv_timeout(Duration::from_secs(5));
        let (validated, completed) = mpsc::channel();
        // Never exercise the check without positively observing Driver ownership.
        let validation = if entry.is_ok() {
            Some(scope.spawn(move || {
                let _ = validated.send(execution.validate_execution());
            }))
        } else {
            None
        };
        let before_release = validation
            .as_ref()
            .map(|_| completed.recv_timeout(Duration::from_secs(5)));
        drop(release);
        let released = did_release.recv_timeout(Duration::from_secs(5));
        // Capture both joins before inspecting either outcome, including panics.
        let projection = held.join();
        let validation = validation.map(|worker| worker.join());
        (entry, before_release, released, projection, validation)
    });
    // Complete real Host teardown before the expected RED assertion can unwind.
    let closed = fixture.workflow.host.close();
    closed.unwrap();
    entry.expect("real projection owns Driver");
    released.expect("projection released");
    let _ = commands(projection.expect("projection worker panicked").unwrap());
    validation
        .expect("validation worker started")
        .expect("validation worker panicked");
    before_release
        .expect("validation started after projection entry")
        .expect("final execution validation waited for Driver")
        .unwrap();
}

#[test]
fn registered_execution_final_validation_completes_while_real_runtime_state_is_held() {
    use std::sync::mpsc;

    struct ReleaseState(mpsc::Sender<()>);
    impl Drop for ReleaseState {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    let (fixture, _policy, execution) = start();
    let registration = execution.register_sdk().unwrap();
    execution.validate_execution().unwrap();
    fixture.workflow.validate_sdk(registration.instance_id).unwrap();
    let runtime = fixture.workflow.core.clone();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (released, did_release) = mpsc::channel();
    let (entry, before_release, released, held, validation) = std::thread::scope(|scope| {
        // This must drop inside the closure, before automatic scope joining.
        let release = ReleaseState(release);
        let held = scope.spawn(move || {
            // Rust-only hold of the original Runtime mutex; no provider callback.
            runtime.with_state_held_for_testing(|| {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(10)).expect("State release");
            });
            released.send(()).unwrap();
        });
        let entry = reached.recv_timeout(Duration::from_secs(5));
        let (validated, completed) = mpsc::channel();
        // Never exercise the check without positively observing State ownership.
        let validation = if entry.is_ok() {
            Some(scope.spawn(move || {
                let _ = validated.send(execution.validate_execution());
            }))
        } else {
            None
        };
        let before_release = validation
            .as_ref()
            .map(|_| completed.recv_timeout(Duration::from_secs(5)));
        drop(release);
        let released = did_release.recv_timeout(Duration::from_secs(5));
        // Capture both joins before inspecting either outcome, including panics.
        let held = held.join();
        let validation = validation.map(|worker| worker.join());
        (entry, before_release, released, held, validation)
    });
    // Complete real Host teardown before the expected RED assertion can unwind.
    let closed = fixture.workflow.host.close();
    closed.unwrap();
    entry.expect("original Runtime owns State");
    released.expect("State released");
    held.expect("State holder panicked");
    validation
        .expect("validation worker started")
        .expect("validation worker panicked");
    // This isolates mutex contention, not actual Swift SDK enqueue/Started proof.
    before_release
        .expect("validation started after State entry")
        .expect("final execution validation waited for Runtime State")
        .unwrap();
}

#[test]
fn issued_start_cannot_register_after_pause_account_generation_or_close() {
    for transition in 0..4 {
        let (fixture, policy, execution) = start();
        match transition {
            0 => {
                policy.clone().input(settings(false, true)).unwrap();
            }
            1 => {
                fixture
                    .workflow
                    .account_changed(None, CloudAccountStatus::NoAccount, None)
                    .unwrap();
            }
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
            }
            _ => {
                fixture.workflow.host.close().unwrap();
            }
        }
        assert!(
            execution.register_sdk().is_err(),
            "issued start crossed transition {transition}"
        );
        if transition != 3 {
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn configuration_cannot_retire_a_replacement_sdk() {
    let (fixture, policy, start) = start();
    let original = start.register_sdk().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let output = policy.clone().input(settings(true, false)).unwrap();
    let command = commands(output)
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::ConfigureAutomatic { on: true }))
        .unwrap();
    let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
    assert_eq!(
        execution.original_sdk().unwrap().as_deref(),
        Some(original.instance_id.as_str())
    );
    let before = fixture.workflow.current_guard().unwrap();
    execution.retire_original().unwrap();
    assert!(
        execution.register_sdk().is_err(),
        "registration overtook original release"
    );
    fixture.workflow.release_sdk(original.instance_id).unwrap();
    let replacement = execution.register_sdk().unwrap();
    assert_eq!(fixture.workflow.current_guard().unwrap().token, before.token);
    assert!(execution.retire_original().is_err());
    fixture.workflow.validate_sdk(replacement.instance_id).unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn registered_execution_is_invalid_after_lifecycle_changes() {
    for transition in 0..4 {
        let (fixture, policy, execution) = start();
        let registration = execution.register_sdk().unwrap();
        execution.validate_execution().unwrap();
        match transition {
            0 => {
                policy.clone().input(settings(false, true)).unwrap();
            }
            1 => {
                fixture
                    .workflow
                    .account_changed(
                        None,
                        CloudAccountStatus::Available,
                        Some(b"synthetic-account-B".to_vec()),
                    )
                    .unwrap();
            }
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
            }
            _ => {
                fixture.workflow.host.close().unwrap();
            }
        }
        assert!(
            execution.validate_execution().is_err(),
            "execution remained valid after lifecycle change {transition}"
        );
        assert!(!registration.instance_id.is_empty());
        if transition != 3 {
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn native_operation_error_and_activity_use_real_driver_facts() {
    let (fixture, policy, execution) = start();
    let sdk = execution.register_sdk().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    execution
        .operation_activity(CloudPolicyOperationFacts {
            zones: vec![CloudPolicyZone::Collection],
            kind: CloudPolicyOperationKind::Send,
            started: true,
            failed: false,
            code: None,
            retry_after_ms: None,
            saved: 0,
            app_cancelled: false,
        })
        .unwrap();
    execution
        .operation_activity(CloudPolicyOperationFacts {
            zones: vec![CloudPolicyZone::Collection],
            kind: CloudPolicyOperationKind::Send,
            started: false,
            failed: true,
            code: Some(7),
            retry_after_ms: Some(9000),
            saved: 0,
            app_cancelled: false,
        })
        .unwrap();
    let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    let error = state.last_error.unwrap();
    assert_eq!(error.class, p::OperationError::Throttled);
    assert_eq!(error.code, 7);
    assert_eq!(error.retry_after_ms, Some(9000));
    assert!(!state.cycles[&error.operation].active);
    fixture.workflow.validate_sdk(sdk.instance_id).unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn execution_created_without_sdk_cannot_adopt_registered_sdk() {
    let (fixture, policy, start, _) = start_with_root_and_accepted_epoch();
    let output = policy
        .clone()
        .input(CloudPolicyEvent::SyncNowRequested {
            source: CloudPolicySource::Command,
        })
        .unwrap();
    let issued = commands(output)
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::RunExchange { .. }))
        .unwrap();
    assert!(matches!(
        policy.exchange_readiness(issued.handle.clone(), false, false).unwrap(),
        CloudPolicyReadiness::Wait
    ));
    assert!(matches!(
        policy.exchange_readiness(issued.handle.clone(), true, false).unwrap(),
        CloudPolicyReadiness::End {
            outcome: CloudPolicyEarlyEnd::Waiting { .. }
        }
    ));
    let unregistered_execution = policy
        .clone()
        .prepare_execution(issued.handle.clone())
        .unwrap()
        .unwrap();
    assert!(unregistered_execution.original_sdk().unwrap().is_none());
    let sdk = start.register_sdk().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    assert!(matches!(
        policy.exchange_readiness(issued.handle.clone(), false, true).unwrap(),
        CloudPolicyReadiness::Ready
    ));
    let registered_execution = policy.clone().prepare_execution(issued.handle).unwrap().unwrap();
    assert_eq!(
        registered_execution.original_sdk().unwrap().as_deref(),
        Some(sdk.instance_id.as_str())
    );
    registered_execution.validate_execution().unwrap();
    assert!(
        unregistered_execution.validate_execution().is_err(),
        "execution created without an SDK must remain invalid"
    );
    fixture.workflow.host.close().unwrap();
}

#[test]
fn expired_readiness_cannot_accept_a_queued_ready_query() {
    let (fixture, policy, start, _) = start_with_root_and_accepted_epoch();
    start.register_sdk().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let issued = commands(
        policy
            .clone()
            .input(CloudPolicyEvent::SyncNowRequested {
                source: CloudPolicySource::Command,
            })
            .unwrap(),
    )
    .into_iter()
    .find(|c| matches!(c.action, CloudPolicyAction::RunExchange { .. }))
    .unwrap();
    assert!(matches!(
        policy.exchange_readiness(issued.handle.clone(), true, true).unwrap(),
        CloudPolicyReadiness::End {
            outcome: CloudPolicyEarlyEnd::Waiting { .. }
        }
    ));
    // Control: readiness observed by the non-expired observer remains usable.
    assert!(matches!(
        policy.exchange_readiness(issued.handle, false, true).unwrap(),
        CloudPolicyReadiness::Ready
    ));
    fixture.workflow.host.close().unwrap();
}

#[test]
fn app_cancellation_and_sdk_20_preserve_failing_cycles_and_real_sdk_14_is_an_error() {
    let (fixture, policy, execution) = start();
    execution.register_sdk().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let zones = vec![CloudPolicyZone::Collection];
    execution
        .operation_activity(CloudPolicyOperationFacts {
            zones: zones.clone(),
            kind: CloudPolicyOperationKind::Send,
            started: true,
            failed: false,
            code: None,
            retry_after_ms: None,
            saved: 0,
            app_cancelled: false,
        })
        .unwrap();
    execution
        .operation_activity(CloudPolicyOperationFacts {
            zones: zones.clone(),
            kind: CloudPolicyOperationKind::Send,
            started: false,
            failed: true,
            code: Some(14),
            retry_after_ms: None,
            saved: 0,
            app_cancelled: false,
        })
        .unwrap();
    let before = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    let error = before.last_error.clone().unwrap();
    assert_eq!(error.code, 14);
    assert_eq!(error.class, p::OperationError::Other);
    assert_eq!(before.cycles[&error.operation].failures, 1);
    for (code, app_cancelled) in [(None, true), (Some(20), false)] {
        execution
            .operation_activity(CloudPolicyOperationFacts {
                zones: zones.clone(),
                kind: CloudPolicyOperationKind::Send,
                started: true,
                failed: false,
                code: None,
                retry_after_ms: None,
                saved: 0,
                app_cancelled: false,
            })
            .unwrap();
        execution
            .operation_activity(CloudPolicyOperationFacts {
                zones: zones.clone(),
                kind: CloudPolicyOperationKind::Send,
                started: false,
                failed: true,
                code,
                retry_after_ms: None,
                saved: 0,
                app_cancelled,
            })
            .unwrap();
        let after = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert!(!after.cycles[&error.operation].active);
        assert_eq!(
            after.cycles[&error.operation].failures,
            before.cycles[&error.operation].failures
        );
        assert_eq!(
            after.last_error.unwrap().code,
            14,
            "cancellation neither invents nor clears an SDK failure"
        );
    }
    fixture.workflow.host.close().unwrap();
}

#[test]
fn actual_replacement_accepts_successor_facades_and_refuses_original_execution_and_preview() {
    use crate::cloud_connection::CloudConnectionPreviewWork;
    let (fixture, policy, execution) = start();
    let original_sdk = execution.register_sdk().unwrap();
    let original_connection = fixture.workflow.connection().unwrap();
    let CloudConnectionPreviewWork::ReadActive { preview, .. } =
        original_connection.begin_connection_preview().unwrap()
    else {
        panic!("actual preview read required");
    };
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
    assert!(execution.validate_execution().is_err());
    assert!(policy.clone().input(CloudPolicyEvent::Foreground).is_err());
    assert!(
        original_connection
            .connection_active_read(preview, crate::cloud_connection::CloudControlRead::ZoneMissing)
            .is_err()
    );
    // Drain ownership belongs to the original workflow. A fresh facade grants
    // no publication/adoption decision for the replaced collection.
    let _ = fixture.workflow.retire_sdk(original_sdk.instance_id.clone(), false);
    fixture.workflow.release_sdk(original_sdk.instance_id).unwrap();
    let successor = fixture.workflow.policy().unwrap();
    successor.clone().input(settings(false, false)).unwrap();
    successor
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let issued = commands(successor.clone().input(CloudPolicyEvent::QuitRequested).unwrap());
    let quit = issued
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::QuitNow))
        .unwrap();
    successor
        .clone()
        .prepare_execution(quit.handle)
        .unwrap()
        .unwrap()
        .validate_control()
        .unwrap();
    let connection = fixture.workflow.connection().unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
    assert!(connection.begin_connection_preview().is_ok());
    assert!(execution.validate_execution().is_err());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn cursorless_auto_off_ffi_capture_maps_before_first_root_fetch() {
    let fixture = fixture();
    let (sdk, root, _) = fixture.baseline_with_root_and_accepted_epoch(false);
    // This registration was returned before its epoch intake established P6
    // authority. It carries no initial root-fetch proof or saved cursor.
    assert!(!sdk.needs_root_fetch && sdk.serialization.is_none());
    let workflow = &fixture.workflow;
    assert!(!workflow.core.wants_send());
    workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
    workflow.release_sdk(sdk.instance_id).unwrap();
    let policy = workflow.policy().unwrap();
    let mut issued = commands(policy.clone().input(settings(false, false)).unwrap());
    issued.extend(commands(policy.clone().refresh_account().unwrap()));
    issued.extend(commands(
        policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    ));
    let command = issued
        .into_iter()
        .rev()
        .find(|c| matches!(c.action, CloudPolicyAction::StartEngine { automatic: false }))
        .expect("real automatic-off StartEngine");
    let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
    let sdk = execution.register_sdk().unwrap();
    assert!(!sdk.needs_root_fetch && sdk.serialization.is_none());
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let collection = workflow.host.collection().unwrap();
    meguri_engine::decks::add(&collection, "FFI first manual pending".into()).unwrap();
    let issued = commands(
        policy
            .clone()
            .input(CloudPolicyEvent::SyncNowRequested {
                source: CloudPolicySource::Command,
            })
            .unwrap(),
    )
    .into_iter()
    .find(|c| matches!(c.action, CloudPolicyAction::RunExchange { .. }))
    .unwrap();
    assert!(matches!(
        policy.exchange_readiness(issued.handle.clone(), false, true).unwrap(),
        CloudPolicyReadiness::Ready
    ));
    assert!(!workflow.core.sdk_root_confirmed());
    assert!(workflow.fetch_drains().unwrap().is_empty());
    let before = issued.handle.exchange_observation_for_testing().unwrap();
    assert!(before.captures.is_empty() && before.active_id.is_none());
    let captured = policy.clone().capture_exchange(issued.handle.clone()).unwrap();
    assert!(matches!(captured.output, CloudPolicyOutput::Commands { ref commands } if commands.is_empty()));
    let id = captured
        .runtime_exchange
        .expect("capture must install the first fetch worker's Runtime ID");
    let after = issued.handle.exchange_observation_for_testing().unwrap();
    assert_eq!(after.captures.get(&before.policy_exchange), Some(&id));
    assert_eq!(after.active_id, Some(id));
    let selection = workflow.core.exchange_result(id).unwrap();
    #[cfg(feature = "test-hooks")]
    {
        let capture = crate::test_hooks::test_cloud_original_exchange_capture(issued.handle.clone(), id).unwrap();
        assert_eq!(capture.boundary_seq, selection.boundary_seq);
        assert_eq!(
            capture.selected_names,
            selection.selected_names.iter().cloned().collect::<Vec<_>>()
        );
        assert!(crate::test_hooks::test_cloud_original_exchange_capture(issued.handle.clone(), id + 1).is_err());
    }
    assert_eq!(selection.boundary_seq, collection.last_seq().0);
    assert!(!selection.selected_names.is_empty());
    assert_eq!(selection.outstanding_names, selection.selected_names);
    assert!(workflow.tracking_sendables().unwrap().is_empty());
    assert!(workflow.begin_send(sdk.instance_id.clone()).unwrap().is_none());
    assert!(
        workflow
            .core
            .build_collection_batch(&sdk.instance_id, &collection, &selection.selected_names)
            .unwrap()
            .saves
            .is_empty()
    );
    // Use the exported fetch delivery and completion path; capture grants no proof.
    let fetch = workflow
        .begin_fetch(sdk.instance_id.clone(), vec!["meguri-root".into()])
        .unwrap();
    workflow
        .will_fetch(sdk.instance_id.clone(), "first-ffi".into(), true)
        .unwrap();
    workflow
        .fetch_zone(sdk.instance_id.clone(), "meguri-root".into(), true, false)
        .unwrap();
    workflow
        .fetched(sdk.instance_id.clone(), "meguri-root".into(), vec![root], vec![])
        .unwrap();
    workflow
        .fetch_zone(sdk.instance_id.clone(), "meguri-root".into(), false, false)
        .unwrap();
    workflow
        .did_fetch(sdk.instance_id.clone(), "first-ffi".into(), true)
        .unwrap();
    assert!(workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
    workflow.exchange_fetched(sdk.instance_id.clone(), true).unwrap();
    assert!(workflow.core.sdk_root_confirmed());
    assert_eq!(workflow.core.exchange_result(id), Some(selection.clone()));
    let pending = workflow.tracking_sendables().unwrap();
    assert!(selection.selected_names.iter().all(|name| pending.contains_key(name)));
    let send = workflow.begin_send(sdk.instance_id.clone()).unwrap().unwrap();
    workflow.finish_send(sdk.instance_id.clone(), send, pending).unwrap();
    let cancelled = workflow
        .complete_exchange(sdk.instance_id, id, true, true, false)
        .unwrap();
    assert_eq!(cancelled.id, id);
    assert!(!workflow.core.adapter_exchange_active());
}

// A real Settings commit changes automatic scheduling without changing either
// policy or backend guard. It must invalidate the original native selection.
#[test]
fn final_validation_rechecks_automatic_after_runtime_checks_without_guard_change() {
    for configure in [false, true] {
        for invalid_sdk in [false, true] {
            let (fixture, policy, start) = start();
            let registration = start.register_sdk().unwrap();
            let execution = if configure {
                policy.clone().input(CloudPolicyEvent::Foreground).unwrap();
                let command = commands(policy.clone().input(settings(true, false)).unwrap())
                    .into_iter()
                    .find(|c| matches!(c.action, CloudPolicyAction::ConfigureAutomatic { on: true }))
                    .unwrap();
                policy.clone().prepare_execution(command.handle).unwrap().unwrap()
            } else {
                start
            };
            execution.validate_execution().unwrap();
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            assert!(driver.native_validation_matches_state_for_testing());
            let (_, command) = execution.asset_read_driver().unwrap();
            let sdk = if invalid_sdk {
                "unissued-sdk"
            } else {
                &registration.instance_id
            };
            if invalid_sdk {
                assert!(matches!(
                    driver.validate_native_sdk(command, sdk),
                    Err(p::DriverError::Runtime(r::RuntimeError::StaleIncarnation))
                ));
            }
            let before = driver.state();
            let backend = fixture.workflow.core.adapter_guard();
            let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed = committed.clone();
            let changed_policy = policy.clone();
            let runtime = fixture.workflow.core.clone();
            let host = fixture.workflow.host.clone();
            driver.set_native_validation_hook_for_testing(move || {
                changed_policy.clone().input(settings(!configure, false)).unwrap();
                let driver = host.cloud_policy_driver().unwrap();
                let after = driver.state();
                assert_eq!(after.guard(), before.guard());
                assert_ne!(after.automatic(), before.automatic());
                assert_eq!(runtime.adapter_guard(), backend);
                assert!(driver.native_validation_matches_state_for_testing());
                observed.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            // Cover both the facade and preservation of stale-policy precedence
            // over an actual Runtime SDK error from the original check.
            if invalid_sdk {
                assert!(matches!(
                    driver.validate_native_sdk(command, sdk),
                    Err(p::DriverError::StaleCommand)
                ));
            } else {
                assert!(matches!(
                    execution.validate_execution(),
                    Err(CoreError::CloudPolicy {
                        failure: CloudPolicyFailure::StaleCommand
                    })
                ));
            }
            assert!(committed.load(std::sync::atomic::Ordering::SeqCst));
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn committed_validation_tracks_accepted_identity_and_discards_superseded_preparation() {
    for superseded in [false, true] {
        let (fixture, policy, execution) = start();
        execution.register_sdk().unwrap();
        policy.clone().input(CloudPolicyEvent::Foreground).unwrap();
        let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
        assert!(driver.native_validation_matches_state_for_testing());
        let before = driver.state();
        let request = fixture.workflow.begin_identity_read(false, None).unwrap();
        assert!(
            fixture
                .workflow
                .complete_identity_read(request, CloudAccountStatus::NoAccount, None)
                .unwrap()
        );
        if superseded {
            let runtime = fixture.workflow.core.clone();
            driver.set_projection_hook_for_testing(move || {
                runtime.adapter_begin_identity_read(false, None).unwrap();
            });
        }
        let result = policy.clone().project_identity_read(request);
        assert!(driver.native_validation_matches_state_for_testing());
        if superseded {
            assert!(matches!(
                result,
                Err(CoreError::CloudPolicy {
                    failure: CloudPolicyFailure::StaleCommand
                })
            ));
            assert_eq!(driver.state(), before, "private prepared state reached actual Owned");
        } else {
            result.unwrap();
            let after = driver.state();
            assert_ne!(after.account, before.account);
            assert_ne!(after.guard(), before.guard());
        }
        fixture.workflow.host.close().unwrap();
    }
}

#[path = "cloud_policy_execution_job_tests.rs"]
mod jobs;
