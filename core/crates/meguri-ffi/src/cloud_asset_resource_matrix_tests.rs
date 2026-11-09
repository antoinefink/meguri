//! Additional resource witnesses over the retained lifecycle fixtures.
use super::*;

#[test]
fn asset_lifecycle_jobs_full_bodies_registered_before_pause_finish_before_suspend() {
    for first in [false, true] {
        let (fixture, sdk, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let bodies = Arc::new(Mutex::new(BTreeMap::new()));
        let observed = bodies.clone();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            *observed.lock().unwrap().entry(stage).or_insert(0) += 1;
            if matches!(
                stage,
                Stage::AssetReadCompleteBody | Stage::AssetReadCancelBody | Stage::AssetReadFinishBody
            ) {
                entered.send(stage).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            }
        });
        let pool = hold_pool(&fixture);
        let complete = pass.clone().start_complete(work.clone(), None).unwrap();
        let cancel = pass.clone().start_cancel(work).unwrap();
        let finish = pass.clone().start_finish().unwrap();
        std::thread::scope(|scope| {
            let releases = [Release(release.clone()), Release(release.clone()), Release(release)];
            let (done, returned) = mpsc::channel();
            let suspending = scope.spawn(move || done.send(engine.suspend(Duration::from_secs(5))).unwrap());
            let until = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while !(complete.handle.cancel.requested()
                && cancel.handle.cancel.requested()
                && finish.handle.cancel.requested())
                && meguri_engine::monotonic::now() < until
            {
                std::thread::yield_now();
            }
            assert!(complete.handle.cancel.requested());
            assert!(cancel.handle.cancel.requested());
            assert!(finish.handle.cancel.requested());
            drop(pool);
            let mut stages = BTreeMap::new();
            for _ in 0..3 {
                *stages
                    .entry(reached.recv_timeout(Duration::from_secs(5)).unwrap())
                    .or_insert(0) += 1;
            }
            assert_eq!(stages.len(), 3);
            assert!(stages.values().all(|count| *count == 1));
            assert!(matches!(returned.try_recv(), Err(mpsc::TryRecvError::Empty)));
            assert!(fixture.workflow.core.validate_sdk(&sdk.instance_id).is_ok());
            assert!(complete.result.result().unwrap().is_none());
            assert_eq!(cancel.result.result(), Ok(None));
            assert_eq!(finish.result.result(), Ok(None));
            drop(releases);
            assert_eq!(
                returned.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
                meguri_engine::SuspendOutcome::Suspended
            );
            suspending.join().unwrap();
        });
        let got = collect_job_outcomes(
            engine,
            &events,
            &[complete.handle.id, cancel.handle.id, finish.handle.id],
        );
        assert!(got.values().all(|outcome| *outcome == JobOutcome::Succeeded));
        assert!(!complete.result.result().unwrap().unwrap().intake_accepted);
        assert_eq!(cancel.result.result(), Ok(Some(false)));
        assert_eq!(finish.result.result(), Ok(Some(true)));
        for stage in [
            Stage::AssetReadCompleteBody,
            Stage::AssetReadCancelBody,
            Stage::AssetReadFinishBody,
            Stage::AssetReadPrepared,
            Stage::AssetReadFinishing,
        ] {
            assert_eq!(bodies.lock().unwrap()[&stage], 1);
        }
        assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
        assert!(fixture.workflow.core.validate_sdk(&sdk.instance_id).is_err());
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_actual_resource_busy_tail_joins_suspend_and_late_tail_expires() {
    for first in [false, true] {
        let (fixture, sdk, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        let engine = fixture.workflow.host.engine();
        let generation = engine.generation();
        let events = engine.stream().subscribe();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        engine.jobs().hold_next_busy_tail_for_testing(entered, resume);
        let complete = pass.clone().start_complete(work.clone(), None).unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(finished(&events, complete.handle.id), JobOutcome::Succeeded);
        assert!(!complete.result.result().unwrap().unwrap().intake_accepted);
        assert!(engine.jobs().running().is_empty());
        assert!(!engine.threads().busy_threads().is_empty());
        fixture.workflow.core.validate_sdk(&sdk.instance_id).unwrap();
        std::thread::scope(|scope| {
            // The guard releases before the scoped join, also on assertion failure.
            let release = Release(release);
            let (done, returned) = mpsc::channel();
            let suspending = scope.spawn(move || done.send(engine.suspend(Duration::from_secs(5))).unwrap());
            let until = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while fixture
                .workflow
                .core
                .validate_native_sdk_for_testing(&sdk.instance_id)
                .is_ok()
                && meguri_engine::monotonic::now() < until
            {
                std::thread::yield_now();
            }
            assert!(
                fixture
                    .workflow
                    .core
                    .validate_native_sdk_for_testing(&sdk.instance_id)
                    .is_err()
            );
            let early = returned.recv_timeout(Duration::from_millis(40));
            let retained = engine.collection().is_ok();
            drop(release);
            suspending.join().unwrap();
            assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)));
            assert!(retained);
            assert_eq!(
                returned.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
                meguri_engine::SuspendOutcome::Suspended
            );
        });
        assert!(engine.threads().busy_threads().is_empty());

        // This real resource is registered after successful teardown's cutoff.
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        engine.jobs().hold_next_busy_tail_for_testing(entered, resume);
        let late = pass.clone().start_complete(work, None).unwrap();
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(finished(&events, late.handle.id), JobOutcome::Failed { .. }));
        assert_eq!(
            late.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
        assert!(engine.jobs().running().is_empty());
        assert!(!engine.threads().busy_threads().is_empty());
        assert_eq!(
            engine.suspend(Duration::from_millis(30)),
            Err(meguri_engine::EngineError::SuspendDeadline {
                phase: "thread registry".into()
            })
        );
        assert!(
            engine.collection().is_ok(),
            "incomplete teardown retains the collection"
        );
        assert_eq!(engine.generation(), generation);
        assert!(!engine.threads().busy_threads().is_empty());
        assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
        drop(release);
        fixture.workflow.host.close().unwrap();
        assert!(engine.threads().busy_threads().is_empty());
    }
}

#[test]
fn asset_lifecycle_jobs_account_change_preserves_full_error_and_retires_original_request() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        fixture
            .workflow
            .account_changed(None, CloudAccountStatus::NoAccount, None)
            .unwrap();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let complete = pass.clone().start_complete(work.clone(), None).unwrap();
        assert!(matches!(
            finished(&events, complete.handle.id),
            JobOutcome::Failed { .. }
        ));
        assert_eq!(
            complete.result.result().err(),
            Some(CoreError::Unavailable {
                reason: if first {
                    "cloud runtime incarnation is stale"
                } else {
                    "cloud policy command is stale"
                }
                .into()
            })
        );
        assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
        let cancel = pass.clone().start_cancel(work).unwrap();
        let finish = pass.clone().start_finish().unwrap();
        collect_job_outcomes(
            fixture.workflow.host.engine(),
            &events,
            &[cancel.handle.id, finish.handle.id],
        );
        assert_eq!(cancel.result.result(), Ok(Some(false)));
        assert_eq!(finish.result.result(), Ok(Some(true)));
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_final_submit_policy_error_precedes_runtime_error_without_charge_or_grant() {
    use meguri_sync::cloud::policy as p;

    for first in [false, true] {
        let (fixture, sdk, port, name) = target(first);
        let pass = port.begin().unwrap();
        let issued = pass.clone().next().unwrap().unwrap();
        let kind = issued.kind;
        let root = fixture.workflow.core.adapter_snapshot().unwrap().root.unwrap();
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        let accounting = Arc::new(AtomicUsize::new(0));
        let observed = accounting.clone();
        fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadSubmitEdge {
                entered.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            } else if stage == Stage::AssetReadSubmitAccounting {
                observed.fetch_add(1, Ordering::SeqCst);
            }
        });
        let queue = Queue::new(true);
        let submit = pass.clone().start_submit(issued.handle.clone(), queue.clone()).unwrap();
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let guard = fixture.workflow.core.adapter_guard();
        fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
        assert_eq!(fixture.workflow.core.adapter_guard(), guard);
        assert!(matches!(
            fixture.workflow.core.validate_native_sdk_for_testing(&sdk.instance_id),
            Err(r::RuntimeError::StaleIncarnation)
        ));
        let settings = |paused| CloudPolicyEvent::Settings {
            settings: CloudPolicyDeviceSettings {
                enabled: true,
                paused,
                automatic: false,
                media: false,
            },
        };
        let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
        let original_account = driver.state().account;
        let final_check = Arc::new(AtomicUsize::new(0));
        if first {
            // A first read owns an SDK, without an original retry command.
            fixture.policy.clone().input(settings(true)).unwrap();
        } else {
            assert_eq!(original_account.status, p::AccountStatus::Available);
            assert!(original_account.identity.is_some());
            let host = fixture.workflow.host.clone();
            let before = driver.state().guard();
            let observed = final_check.clone();
            fixture
                .workflow
                .host
                .cloud_policy_driver()
                .unwrap()
                .set_native_validation_hook_for_testing(move || {
                    // The actual validator has sampled Runtime before this commit.
                    host.cloud_policy_input(p::DeviceEvent::Account {
                        identity: r::IdentityStatus::Configuration,
                        origin_sdk: None,
                    })
                    .unwrap();
                    let driver = host.cloud_policy_driver().unwrap();
                    let after = driver.state();
                    assert_eq!(after.account.status, p::AccountStatus::Configuration);
                    assert_eq!(after.guard().generation, before.generation);
                    assert_eq!(after.guard().binding, before.binding);
                    assert!(
                        after.guard().token > before.token,
                        "account change must stale the policy guard"
                    );
                    assert!(driver.native_validation_matches_state_for_testing());
                    observed.fetch_add(1, Ordering::SeqCst);
                });
        }
        drop(release);
        assert!(matches!(finished(&events, submit.handle.id), JobOutcome::Failed { .. }));
        assert_eq!(
            submit.result.result(),
            Err(CoreError::Unavailable {
                reason: if first {
                    "cloud runtime incarnation is stale"
                } else {
                    "cloud policy command is stale"
                }
                .into()
            })
        );
        assert_eq!(final_check.load(Ordering::SeqCst), usize::from(!first));
        assert_eq!(accounting.load(Ordering::SeqCst), 1);
        assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
        assert_eq!(attempts(&fixture, &name), 0);
        assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
        assert!(!pass.cancel(issued.handle).unwrap());
        pass.finish().unwrap();
        fixture.workflow.core.set_incoming_stage_hook_for_testing(|_| {});
        if first {
            fixture.policy.clone().input(settings(false)).unwrap();
        } else {
            fixture
                .workflow
                .host
                .cloud_policy_input(p::DeviceEvent::Account {
                    identity: r::IdentityStatus::Available(original_account.identity.clone().unwrap()),
                    origin_sdk: None,
                })
                .unwrap();
            assert!(driver.state().account == original_account);
        }
        fixture.workflow.release_sdk(sdk.instance_id).unwrap();
        let sdk = fixture.workflow.register_sdk(false).unwrap();
        fixture
            .workflow
            .core
            .accept_root_fetch(
                &sdk.instance_id,
                Some(&w::Saved {
                    name: "active".into(),
                    envelope: root.envelope.0,
                    system_fields: root.system_fields.0,
                }),
                true,
                1,
            )
            .unwrap();
        fixture
            .policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap();
        let fresh = if first {
            fixture.workflow.begin_first_asset_read_pass(sdk.instance_id).unwrap()
        } else {
            retry_pass(&fixture, &sdk)
        };
        let selected = fresh.clone().next().unwrap().unwrap();
        assert_eq!(selected.name, name);
        assert_eq!(selected.kind, kind, "the first-read grant remains selectable");
        assert_eq!(attempts(&fixture, &name), 0);
        assert!(fresh.cancel(selected.handle).unwrap());
        fresh.finish().unwrap();
        fixture.workflow.host.close().unwrap();
    }
}
