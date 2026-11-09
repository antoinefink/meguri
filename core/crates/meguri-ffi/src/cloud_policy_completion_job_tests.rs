//! Original-SDK Ended work: existing pool, Driver, Runtime and original owners.
use super::*;
use meguri_engine::EventKind;
use std::sync::atomic::{AtomicBool, Ordering};

fn facts() -> CloudPolicyOperationFacts {
    CloudPolicyOperationFacts {
        zones: vec![
            CloudPolicyZone::Root,
            CloudPolicyZone::Collection,
            CloudPolicyZone::Media,
        ],
        kind: CloudPolicyOperationKind::Send,
        started: false,
        failed: false,
        code: None,
        retry_after_ms: None,
        saved: 73,
        app_cancelled: false,
    }
}

fn active_fixture() -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>, String) {
    let (fixture, policy, execution) = start();
    let sdk = execution.register_sdk().unwrap().instance_id;
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let mut started = facts();
    started.started = true;
    started.saved = 0;
    execution.operation_activity(started).unwrap();
    (fixture, policy, execution, sdk)
}

#[test]
fn required_sdk_completion_runs_after_queued_cancel_once() {
    for (failed, code, app_cancelled) in [
        (false, None, false),
        (true, Some(7), false),
        (true, None, true),
        (true, Some(20), false),
    ] {
        let mut baseline = None;
        for asynchronous in [false, true] {
            let (fixture, _, execution, sdk) = active_fixture();
            let runtime = fixture.workflow.core.clone();
            let before = runtime.adapter_guard();
            let mut actual = facts();
            actual.failed = failed;
            actual.code = code;
            actual.retry_after_ms = Some(9000);
            actual.app_cancelled = app_cancelled;
            let events = fixture.workflow.host.engine().stream().subscribe();
            let pool = hold_pool(&fixture);
            let (job, output) = if asynchronous {
                let job = execution.start_report_completion(actual).unwrap();
                assert!(matches!(job.result.result(), Ok(None)));
                assert!(fixture.workflow.host.engine().slot_holder().is_none());
                job.handle.cancel.cancel().unwrap();
                drop(pool);
                assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
                assert!(job.handle.cancel.requested());
                let output = job.result.result().map(|value| value.expect("actual stored return"));
                (Some(job), output)
            } else {
                drop(pool);
                (None, execution.operation_activity(actual))
            };
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let state = driver.state();
            assert!(state.cycles.values().all(|cycle| !cycle.active));
            if failed && !app_cancelled && code != Some(20) {
                let error = state.last_error.as_ref().unwrap();
                assert_eq!(error.code, 7);
                assert_eq!(error.retry_after_ms, Some(9000));
                assert_eq!(error.class, p::OperationError::Throttled);
            }
            let snapshot = (invocation_snapshot(output), state.cycles, state.last_error);
            assert_eq!(
                runtime.adapter_guard(),
                before,
                "reporting grants no new network authority"
            );
            runtime.validate_sdk(&sdk).unwrap();
            // The registered fence makes duplicate completion events observable.
            let fence = fixture.workflow.host.engine().jobs().start(
                "completion event fence",
                fixture.workflow.host.engine().generation(),
                None,
                |_| Ok(()),
            );
            loop {
                let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
                if let EventKind::JobFinished { id, .. } = &event.kind {
                    if let Some(job) = &job {
                        assert_ne!(id.0, job.handle.id, "duplicate terminal event");
                    }
                    if *id == fence.id {
                        break;
                    }
                }
            }
            fixture.workflow.host.close().unwrap();
            if let Some(job) = job {
                assert_eq!(
                    invocation_snapshot(job.result.result().map(|value| value.unwrap())),
                    snapshot.0
                );
                assert_eq!(baseline.as_ref().unwrap(), &snapshot);
            } else {
                baseline = Some(snapshot);
            }
        }
    }
}

#[test]
fn required_sdk_completion_retains_post_effect_result_error_and_followups() {
    for retire in [false, true] {
        let mut baseline = None;
        for asynchronous in [false, true] {
            let (fixture, _, execution, sdk) = active_fixture();
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let runtime = fixture.workflow.core.clone();
            let events = fixture.workflow.host.engine().stream().subscribe();
            let pool = hold_pool(&fixture);
            let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
            let token = cancel.clone();
            let observed = Arc::new(AtomicBool::new(false));
            let saw = observed.clone();
            let host = fixture.workflow.host.clone();
            driver.set_projection_hook_for_testing(move || {
                host.cloud_policy_driver()
                    .unwrap()
                    .set_projection_hook_for_testing(move || {
                        // OperationError for Root has already committed. This is
                        // its Ended projection, preserving error-before-Ended order.
                        saw.store(true, Ordering::SeqCst);
                        if let Some(token) = token.lock().unwrap().as_ref() {
                            token.trigger();
                        }
                        if retire {
                            runtime.retire_sdk(&sdk, false).unwrap();
                        }
                    });
            });
            let mut actual = facts();
            actual.failed = true;
            actual.code = Some(7);
            actual.retry_after_ms = Some(9000);
            let (job, output, outcome) = if asynchronous {
                let job = execution.start_report_completion(actual).unwrap();
                *cancel.lock().unwrap() = Some(job.handle.cancel.clone());
                drop(pool);
                let outcome = finished(&events, job.handle.id);
                let output = job.result.result().map(|value| value.unwrap());
                (Some(job), output, Some(outcome))
            } else {
                drop(pool);
                (None, execution.operation_activity(actual), None)
            };
            let snapshot = (
                invocation_snapshot(output),
                driver.state().cycles,
                driver.state().last_error,
            );
            assert!(observed.load(Ordering::SeqCst));
            assert_eq!(snapshot.2.as_ref().unwrap().code, 7);
            if retire {
                // The host projection uses its legacy Runtime error mapping;
                // never replace it with policy stale-command or cancellation.
                assert!(snapshot.0.is_err());
            } else {
                assert!(
                    snapshot.0.as_ref().is_ok_and(|actions| !actions.is_empty()),
                    "required follow-up outputs"
                );
                assert!(snapshot.1.values().all(|cycle| !cycle.active));
            }
            fixture.workflow.host.close().unwrap();
            if let Some(job) = job {
                assert!(job.handle.cancel.requested());
                assert_ne!(outcome, Some(JobOutcome::Cancelled));
                assert_eq!(
                    invocation_snapshot(job.result.result().map(|value| value.unwrap())),
                    snapshot.0
                );
                assert_eq!(baseline.as_ref().unwrap(), &snapshot);
            } else {
                baseline = Some(snapshot);
            }
        }
    }
}

#[test]
fn required_sdk_completion_rechecks_original_after_driver_wait() {
    for close in [false, true] {
        let (fixture, policy, execution, sdk) = active_fixture();
        let engine = fixture.workflow.host.engine();
        let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
        let events = engine.stream().subscribe();
        let before = driver.state().cycles;
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        driver.observe_contention_for_testing(contended);
        driver.set_projection_hook_for_testing(move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        let mut actual = facts();
        actual.app_cancelled = true;
        let (job, closing) = std::thread::scope(|scope| {
            let release = Release(release);
            let held = scope.spawn(move || policy.input(CloudPolicyEvent::Foreground));
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            let job = execution.start_report_completion(actual).unwrap();
            observed
                .recv_timeout(Duration::from_secs(5))
                .expect("real Driver acquisition contention");
            job.handle.cancel.cancel().unwrap();
            let closing = if close {
                let host = fixture.workflow.host.clone();
                let closing = scope.spawn(move || host.close());
                // Closing revokes original Host metadata before its early drain.
                let deadline = meguri_engine::monotonic::now() + Duration::from_secs(3);
                while fixture.workflow.host.cloud_connection_generation().is_ok()
                    && meguri_engine::monotonic::now() < deadline
                {
                    std::thread::yield_now();
                }
                Some(closing)
            } else {
                fixture.workflow.core.retire_sdk(&sdk, false).unwrap();
                None
            };
            drop(release);
            let _ = held.join().unwrap();
            (job, closing.map(|closing| closing.join().unwrap()))
        });
        let outcome = finished(&events, job.handle.id);
        let after = driver.state().cycles;
        assert_eq!(after, before, "stale app-cancel must not clear active cycles");
        assert!(matches!(outcome, JobOutcome::Failed { .. }));
        if close {
            closing.unwrap().unwrap();
            assert!(matches!(
                job.result.result(),
                Err(CoreError::CloudPolicy {
                    failure: CloudPolicyFailure::Closed
                })
            ));
        } else {
            assert!(matches!(
                job.result.result(),
                Err(CoreError::CloudConnection {
                    failure: crate::cloud_connection::CloudConnectionFailure::AuthorityChanged
                })
            ));
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn required_sdk_completion_clear_active_rechecks_sdk_after_its_own_driver_wait() {
    let (fixture, policy, execution, sdk) = active_fixture();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let (_, command) = execution.asset_read_driver().unwrap();
    let before = driver.state().cycles;
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (contended, observed) = mpsc::channel();
    driver.observe_contention_for_testing(contended);
    driver.set_projection_hook_for_testing(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let result = std::thread::scope(|scope| {
        let release = Release(release);
        let held = scope.spawn(move || policy.input(CloudPolicyEvent::Foreground));
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let waiting_sdk = sdk.clone();
        let waiting = scope.spawn(move || {
            let cancel = meguri_engine::CancelToken::new();
            cancel.cancel();
            let mut work = p::PolicyWork::required_completion(&cancel);
            driver.cancel_native_activity_with_work(
                command,
                &waiting_sdk,
                p::Operation {
                    zone: p::Zone::Collection,
                    kind: p::OperationKind::Send,
                },
                &mut work,
            )
        });
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        fixture.workflow.core.retire_sdk(&sdk, false).unwrap();
        drop(release);
        let _ = held.join().unwrap();
        waiting.join().unwrap()
    });
    assert!(matches!(
        result,
        Err(p::DriverError::Runtime(r::RuntimeError::StaleIncarnation))
    ));
    assert_eq!(driver.state().cycles, before);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn required_sdk_completion_production_drain_before_pause_and_late_valid_sdk_refusal() {
    let (fixture, _, execution, sdk) = active_fixture();
    let engine = fixture.workflow.host.engine();
    let events = engine.stream().subscribe();
    // Native producers join Ended/results before the production freeze/pause.
    let job = execution.clone().start_report_completion(facts()).unwrap();
    assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
    assert!(job.result.result().unwrap().is_some());
    let expected = meguri_engine::EngineError::Busy {
        running_op: "retain valid SDK at closed gate".into(),
    };
    assert_eq!(engine.close_with_host_cleanup(|_| Err(expected.clone())), Err(expected));
    fixture.workflow.core.validate_sdk(&sdk).unwrap();
    let late = execution.clone().start_report_completion(facts()).unwrap();
    assert_eq!(finished(&events, late.handle.id), JobOutcome::Cancelled);
    assert!(
        matches!(late.result.result(), Ok(None)),
        "no required body return was stored"
    );
    let ordinary = execution
        .clone()
        .start_report_invocation(vec![CloudPolicyZone::Collection], CloudPolicyOperationKind::Send)
        .unwrap();
    assert_eq!(finished(&events, ordinary.handle.id), JobOutcome::Cancelled);
    assert!(
        matches!(ordinary.result.result(), Err(CoreError::Cancelled)),
        "ordinary placeholder remains"
    );
    let mut invalid = facts();
    invalid.started = true;
    assert!(matches!(
        execution.clone().start_report_completion(invalid),
        Err(CoreError::InvalidArgument { .. })
    ));
    // The existing synchronous wrapper does not acquire registry admission.
    let mut synchronous = facts();
    synchronous.app_cancelled = true;
    execution.operation_activity(synchronous).unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn required_sdk_completion_early_drain_deadline_preserves_real_runtime_until_retry() {
    let (fixture, _, execution, sdk) = active_fixture();
    let engine = fixture.workflow.host.engine();
    let events = engine.stream().subscribe();
    let pool = hold_pool(&fixture);
    let mut actual = facts();
    actual.app_cancelled = true;
    let job = execution.start_report_completion(actual).unwrap();
    let failed = engine.suspend(Duration::from_millis(100));
    let retained = engine
        .jobs()
        .running()
        .iter()
        .any(|running| running.id.0 == job.handle.id);
    let sdk_before_release = fixture.workflow.core.validate_sdk(&sdk);
    let no_body = matches!(job.result.result(), Ok(None));
    drop(pool);
    let outcome = finished(&events, job.handle.id);
    assert_eq!(
        failed,
        Err(meguri_engine::EngineError::SuspendDeadline { phase: "jobs".into() })
    );
    assert!(retained && no_body && job.handle.cancel.requested());
    sdk_before_release.unwrap();
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(job.result.result().unwrap().is_some());
    engine.suspend(Duration::from_secs(5)).unwrap();
    assert!(fixture.workflow.core.validate_sdk(&sdk).is_err());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn required_sdk_completion_generation_loss_during_real_runtime_wait_refuses_before_clear() {
    let (fixture, _, execution, sdk) = active_fixture();
    let engine = fixture.workflow.host.engine();
    let events = engine.stream().subscribe();
    let runtime = fixture.workflow.core.clone();
    let before = fixture.workflow.host.cloud_policy_driver().unwrap().state().cycles;
    let old_generation = engine.generation();
    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (contended, observed) = mpsc::channel();
    let (advanced, generation_reached) = mpsc::channel();
    let (continue_writer, writer_resume) = mpsc::channel();
    let generation_source = engine.clone();
    exclusive.collection().after_next_commit(move |_| {
        advanced.send(generation_source.generation()).unwrap();
        writer_resume.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    runtime.observe_state_contention_for_testing(contended);
    let (job, advanced, result, unchanged, valid) = std::thread::scope(|scope| {
        let release = Release(release);
        let continue_writer = Release(continue_writer);
        let held_runtime = runtime.clone();
        let holder = scope.spawn(move || {
            held_runtime.with_state_held_for_testing(|| {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(10)).unwrap();
            });
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut actual = facts();
        actual.app_cancelled = true;
        let job = execution.start_report_completion(actual).unwrap();
        observed
            .recv_timeout(Duration::from_secs(5))
            .expect("worker owns Driver and waits for actual Runtime");
        job.handle.cancel.cancel().unwrap();
        let replacing = scope.spawn(move || exclusive.replace(|_| Ok(())).map(|_| ()));
        let advanced = generation_reached.recv_timeout(Duration::from_secs(5));
        drop(release);
        holder.join().unwrap();
        // Replacement holds the writer's reserved event ahead of JobFinished.
        // Inspect the stored result while the original Runtime remains bound.
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
        let result = loop {
            let result = job.result.result();
            if !matches!(result, Ok(None)) || meguri_engine::monotonic::now() >= deadline {
                break result;
            }
            std::thread::yield_now();
        };
        let unchanged = fixture.workflow.host.cloud_policy_driver().unwrap().state().cycles == before;
        let valid = runtime.validate_sdk(&sdk);
        drop(continue_writer);
        replacing.join().unwrap().unwrap();
        (job, advanced, result, unchanged, valid)
    });
    drop(slot);
    let outcome = finished(&events, job.handle.id);
    fixture.workflow.host.close().unwrap();
    assert!(advanced.unwrap() > old_generation);
    assert!(unchanged, "generation loss cleared active state");
    valid.unwrap();
    assert!(matches!(
        result,
        Err(CoreError::CloudPolicy {
            failure: CloudPolicyFailure::GenerationChanged
        })
    ));
    assert!(matches!(outcome, JobOutcome::Failed { .. }));
    assert!(matches!(
        job.result.result(),
        Err(CoreError::CloudPolicy {
            failure: CloudPolicyFailure::GenerationChanged
        })
    ));
}
