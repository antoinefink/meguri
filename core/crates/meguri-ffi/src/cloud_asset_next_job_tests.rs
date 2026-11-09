//! Ordinary next: actual inbox selection, original pass and exact typed cells.
use super::*;
use crate::cloud_asset_reads::CloudAssetReadNextJob;
use meguri_sync::cloud::policy as p;

fn next_value(job: &CloudAssetReadNextJob) -> Result<Option<Option<String>>, CoreError> {
    job.result
        .result()
        .map(|value| value.map(|value| value.work.map(|work| work.name)))
}

#[test]
fn asset_next_jobs_cancel_after_real_driver_runtime_and_tracking_waits_without_advancing() {
    for first in [false, true] {
        for lock in 0..3 {
            if first && lock == 1 {
                continue;
            }
            let (fixture, sdk, port, name) = target(first);
            let pass = port.begin().unwrap();
            let engine = fixture.workflow.host.engine();
            let runtime = fixture.workflow.core.clone();
            let events = engine.stream().subscribe();
            let before = runtime.adapter_snapshot().unwrap();
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let (contended, observed) = mpsc::channel();
            let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let timeout = timed_out.clone();
            let hold = move || {
                let _ = entered.send(());
                timeout.store(resume.recv_timeout(Duration::from_secs(10)).is_err(), Ordering::SeqCst);
            };
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            if lock == 0 {
                runtime.observe_state_contention_for_testing(contended);
            } else if lock == 1 {
                driver.observe_contention_for_testing(contended);
            } else {
                runtime.set_incoming_stage_hook_for_testing(move |stage| {
                    if stage == Stage::AssetReadNextTrackingContended {
                        let _ = contended.send(());
                    }
                });
            }
            let (entry, contention, admitted, pending, slot, cancelled, holder, starter) =
                std::thread::scope(|scope| {
                    let release = Release(release);
                    let runtime = runtime.clone();
                    let policy = fixture.policy.clone();
                    let holder = if lock == 1 {
                        driver.set_projection_hook_for_testing(hold);
                        scope.spawn(move || policy.input(CloudPolicyEvent::Foreground).map(|_| ()))
                    } else {
                        scope.spawn(move || {
                            if lock == 0 {
                                runtime.with_state_held_for_testing(hold);
                            } else {
                                r::with_tracking_access(hold);
                            }
                            Ok(())
                        })
                    };
                    let entry = reached.recv_timeout(Duration::from_secs(5));
                    let (sent, received) = mpsc::channel();
                    let owner = pass.clone();
                    let starter = scope.spawn(move || {
                        let _ = sent.send(owner.start_next());
                    });
                    let admitted = received.recv_timeout(Duration::from_secs(5));
                    let contention = observed.recv_timeout(Duration::from_secs(5));
                    let job = admitted.as_ref().ok().and_then(|job| job.as_ref().ok());
                    let pending = job.map(next_value);
                    let slot = engine.slot_holder();
                    let cancelled = job.map(|job| job.handle.cancel.cancel());
                    drop(release);
                    (
                        entry,
                        contention,
                        admitted,
                        pending,
                        slot,
                        cancelled,
                        holder.join(),
                        starter.join(),
                    )
                });
            entry.unwrap();
            contention.expect("positive contention on the actual lock");
            holder.unwrap().unwrap();
            starter.unwrap();
            cancelled.unwrap().unwrap();
            let job = admitted.expect("start returned while the lock was held").unwrap();
            assert_eq!(finished(&events, job.handle.id), JobOutcome::Cancelled);
            runtime.set_incoming_stage_hook_for_testing(|_| {});
            assert_eq!(pending, Some(Ok(None)));
            assert!(slot.is_none());
            assert!(!timed_out.load(Ordering::SeqCst));
            assert_eq!(next_value(&job), Err(CoreError::Cancelled));
            assert!(before == runtime.adapter_snapshot().unwrap());
            assert_eq!(attempts(&fixture, &name), 0);
            runtime.validate_sdk(&sdk.instance_id).unwrap();
            let selected = pass.clone().start_next().unwrap();
            assert_eq!(finished(&events, selected.handle.id), JobOutcome::Succeeded);
            let selected = selected.result.result().unwrap().unwrap().work.unwrap();
            assert_eq!(selected.name, name); // the original cursor did not move
            assert!(pass.cancel(selected.handle).unwrap());
            pass.finish().unwrap();
            // Cancelled selection consumed neither a retry nor a first grant.
            let later = port.begin().unwrap();
            let selected = later.clone().next().unwrap().unwrap();
            assert_eq!(selected.name, name);
            assert!(later.cancel(selected.handle).unwrap());
            later.finish().unwrap();
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn asset_next_jobs_scan_cancellation_and_post_effect_results_keep_exact_original_handle() {
    for first in [false, true] {
        for stage in [
            Stage::AssetReadNextScan,
            Stage::AssetReadNextScanned,
            Stage::AssetReadNextEffect,
            Stage::AssetReadNextSelected,
            Stage::AssetReadNextReturned,
        ] {
            let (fixture, _, port, name) = target(first);
            let pass = port.begin().unwrap();
            let runtime = fixture.workflow.core.clone();
            let events = fixture.workflow.host.engine().stream().subscribe();
            let pool = hold_pool(&fixture);
            let token = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
            let cancel = token.clone();
            let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let hit = seen.clone();
            runtime.set_incoming_stage_hook_for_testing(move |at| {
                if at == stage {
                    hit.store(true, Ordering::SeqCst);
                    cancel.lock().unwrap().as_ref().unwrap().trigger();
                }
            });
            let job = pass.clone().start_next().unwrap();
            assert_eq!(next_value(&job), Ok(None));
            *token.lock().unwrap() = Some(job.handle.cancel.clone());
            drop(pool);
            let terminal = finished(&events, job.handle.id);
            runtime.set_incoming_stage_hook_for_testing(|_| {});
            assert!(seen.load(Ordering::SeqCst));
            assert_eq!(attempts(&fixture, &name), 0);
            if matches!(stage, Stage::AssetReadNextScan | Stage::AssetReadNextScanned) {
                assert_eq!(terminal, JobOutcome::Cancelled);
                assert_eq!(next_value(&job), Err(CoreError::Cancelled));
                let selected = pass.clone().next().unwrap().unwrap();
                assert_eq!(selected.name, name);
                assert!(pass.cancel(selected.handle).unwrap());
                pass.finish().unwrap();
                fixture.workflow.host.close().unwrap();
                continue;
            }
            assert_eq!(terminal, JobOutcome::Succeeded);
            let actual = job.result.result().unwrap().unwrap().work.unwrap();
            let reread = job.result.result().unwrap().unwrap().work.unwrap();
            assert!(Arc::ptr_eq(&actual.handle, &reread.handle));
            assert_eq!(actual.name, name);
            assert!(pass.clone().next().is_err()); // Selected owns the one request
            let unrelated = port.begin().unwrap();
            assert!(unrelated.cancel(actual.handle.clone()).is_err());
            unrelated.finish().unwrap();
            assert!(pass.cancel(actual.handle.clone()).unwrap());
            pass.finish().unwrap();
            fixture.workflow.host.close().unwrap();
            let replacement = published().0;
            let successor_before = replacement.workflow.core.adapter_snapshot().unwrap();
            let retained = job.result.result().unwrap().unwrap().work.unwrap();
            assert!(Arc::ptr_eq(&actual.handle, &retained.handle));
            assert!(!pass.cancel(retained.handle).unwrap());
            assert!(successor_before == replacement.workflow.core.adapter_snapshot().unwrap());
            replacement.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn asset_next_jobs_initial_empty_pass_distinguishes_pending_cancelled_and_exhausted_results() {
    let (fixture, sdk) = published();
    let pass = fixture.workflow.begin_first_asset_read_pass(sdk.instance_id).unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let cancelled = pass.clone().start_next().unwrap();
    assert_eq!(next_value(&cancelled), Ok(None));
    cancelled.handle.cancel.cancel().unwrap();
    drop(pool);
    assert_eq!(finished(&events, cancelled.handle.id), JobOutcome::Cancelled);
    let empty = pass.clone().start_next().unwrap();
    assert_eq!(finished(&events, empty.handle.id), JobOutcome::Succeeded);
    assert_eq!(next_value(&cancelled), Err(CoreError::Cancelled));
    assert_eq!(next_value(&empty), Ok(Some(None)));
    pass.finish().unwrap();
    fixture.workflow.host.close().unwrap();
    assert_eq!(next_value(&empty), Ok(Some(None)));
    assert_eq!(next_value(&cancelled), Err(CoreError::Cancelled));
}

#[test]
fn asset_next_jobs_preserve_five_retry_admissions_and_the_separate_first_grant() {
    let (fixture, sdk, port, name) = target(true);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let first = port.begin().unwrap();
    let job = first.clone().start_next().unwrap();
    assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
    let selected = job.result.result().unwrap().unwrap().work.unwrap();
    assert_eq!(selected.kind, CloudAssetReadKind::SaveConflictFirst);
    assert!(first.submit(selected.handle.clone(), Queue::new(true)).unwrap());
    assert_eq!(attempts(&fixture, &name), 0);
    assert!(!first.complete(selected.handle, None).unwrap().intake_accepted);
    first.finish().unwrap();
    let first_again = port.begin().unwrap();
    let empty = first_again.clone().start_next().unwrap();
    assert_eq!(finished(&events, empty.handle.id), JobOutcome::Succeeded);
    assert_eq!(next_value(&empty), Ok(Some(None)));
    first_again.finish().unwrap();
    for count in 0..5 {
        let issued = retry_command(&fixture, &sdk);
        let execution = fixture
            .policy
            .clone()
            .prepare_execution(issued.handle)
            .unwrap()
            .unwrap();
        let pass = execution.clone().begin_asset_read_pass().unwrap();
        let job = pass.clone().start_next().unwrap();
        assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
        assert_eq!(attempts(&fixture, &name), count);
        let value = job.result.result().unwrap().unwrap();
        let selected = value.work.unwrap();
        assert_eq!(selected.name, name);
        assert_eq!(selected.kind, CloudAssetReadKind::Retry);
        assert!(pass.submit(selected.handle.clone(), Queue::new(true)).unwrap());
        assert_eq!(attempts(&fixture, &name), count + 1);
        assert!(!pass.complete(selected.handle, None).unwrap().intake_accepted);
        pass.finish().unwrap();
        if count == 4 {
            let exhausted = execution.begin_asset_read_pass().unwrap();
            let empty = exhausted.clone().start_next().unwrap();
            assert_eq!(finished(&events, empty.handle.id), JobOutcome::Succeeded);
            assert_eq!(next_value(&empty), Ok(Some(None)));
            assert_eq!(attempts(&fixture, &name), 5);
            exhausted.finish().unwrap();
        }
    }
    fixture.workflow.host.close().unwrap();
}

#[test]
fn asset_next_jobs_authority_changes_after_retaining_guard_do_not_select_or_touch_successor() {
    for first in [false, true] {
        // First reads deliberately have no issued retry command.
        for loss in 0..3 {
            if first && loss == 2 {
                continue;
            }
            let (fixture, sdk, port, name) = target(first);
            let pass = port.begin().unwrap();
            let runtime = fixture.workflow.core.clone();
            let events = fixture.workflow.host.engine().stream().subscribe();
            let owner = pass.clone();
            let original_sdk = sdk.instance_id.clone();
            let original = workflow(&fixture);
            let policy = fixture.policy.clone();
            let successor = Arc::new(Mutex::new(None));
            let retain = successor.clone();
            let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed = seen.clone();
            runtime.set_incoming_stage_hook_for_testing(move |stage| {
                if stage != Stage::AssetReadNextGuarded || observed.swap(true, Ordering::SeqCst) {
                    return;
                }
                // This hook is outside State/tracking. All changes use real
                // original ports; the old job must not refresh its authority.
                if loss == 0 {
                    original.retire_sdk(sdk.instance_id.clone(), false).unwrap();
                    original.release_sdk(sdk.instance_id.clone()).unwrap();
                    let new_sdk = original.register_sdk(false).unwrap();
                    let new_pass = original
                        .begin_first_asset_read_pass(new_sdk.instance_id.clone())
                        .unwrap();
                    *retain.lock().unwrap() = Some((new_sdk, new_pass));
                } else if loss == 1 {
                    owner.finish().unwrap();
                } else {
                    policy
                        .clone()
                        .input(CloudPolicyEvent::Settings {
                            settings: CloudPolicyDeviceSettings {
                                enabled: true,
                                paused: true,
                                automatic: false,
                                media: false,
                            },
                        })
                        .unwrap();
                }
            });
            let job = pass.clone().start_next().unwrap();
            let terminal = finished(&events, job.handle.id);
            runtime.set_incoming_stage_hook_for_testing(|_| {});
            assert!(seen.load(Ordering::SeqCst));
            assert!(matches!(terminal, JobOutcome::Failed { .. }));
            let error = next_value(&job).unwrap_err();
            assert_ne!(error, CoreError::Cancelled);
            assert_eq!(attempts(&fixture, &name), 0);
            if loss == 2 {
                assert_eq!(
                    error,
                    cloud_error(p::DriverError::Runtime(r::RuntimeError::StaleIncarnation))
                );
                assert!(matches!(
                    runtime.validate_sdk(&original_sdk),
                    Err(r::RuntimeError::StaleIncarnation)
                ));
            }
            if let Some((sdk, next)) = successor.lock().unwrap().take() {
                runtime.validate_sdk(&sdk.instance_id).unwrap();
                assert!(next.clone().next().is_ok());
                next.finish().unwrap();
                // Refusal/cleanup on the original owner cannot remove a
                // successor pass or turn it into a new retry execution.
                assert_eq!(next_value(&job), Err(error.clone()));
            }
            pass.finish().unwrap();
            fixture.workflow.host.close().unwrap();
            assert_eq!(next_value(&job), Err(error));
        }
    }
}

#[test]
fn asset_next_retry_selection_finishes_while_real_driver_projection_owns_mutex() {
    let (fixture, _, port, name) = target(false);
    let pass = port.begin().unwrap();
    let runtime = fixture.workflow.core.clone();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (guarded, reached_guard) = mpsc::channel();
    let (continue_next, next_wait) = mpsc::channel();
    let next_wait = Mutex::new(next_wait);
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == Stage::AssetReadNextGuarded {
            let _ = guarded.send(());
            next_wait.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
        }
    });
    let job = pass.clone().start_next().unwrap();
    reached_guard.recv_timeout(Duration::from_secs(5)).unwrap();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    driver.set_projection_hook_for_testing(move || {
        let _ = entered.send(());
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let terminal = std::thread::scope(|scope| {
        let release = Release(release);
        let policy = fixture.policy.clone();
        let holder = scope.spawn(move || policy.input(CloudPolicyEvent::Foreground));
        let entry = reached.recv_timeout(Duration::from_secs(5));
        continue_next.send(()).unwrap();
        // This event must arrive before releasing the actual Driver owner.
        let terminal = finished(&events, job.handle.id);
        drop(release);
        holder.join().unwrap().unwrap();
        entry.unwrap();
        terminal
    });
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    assert_eq!(terminal, JobOutcome::Succeeded);
    let selected = job.result.result().unwrap().unwrap().work.unwrap();
    assert_eq!(selected.name, name);
    assert!(pass.cancel(selected.handle).unwrap());
    pass.finish().unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn asset_next_late_cancel_keeps_actual_counter_failure_after_cursor_effect() {
    for first in [false, true] {
        let (fixture, _, port, name) = target(first);
        let pass = port.begin().unwrap();
        let runtime = fixture.workflow.core.clone();
        runtime.exhaust_asset_read_counter_for_testing();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let token = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
        let cancel = token.clone();
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadNextEffect {
                cancel.lock().unwrap().as_ref().unwrap().trigger();
            }
        });
        let job = pass.clone().start_next().unwrap();
        *token.lock().unwrap() = Some(job.handle.cancel.clone());
        drop(pool);
        assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
        runtime.set_incoming_stage_hook_for_testing(|_| {});
        assert!(job.handle.cancel.requested());
        let error = r::RuntimeError::from(meguri_sync::cloud::Error::InvalidData);
        let expected = if first {
            cloud_error(error)
        } else {
            cloud_error(p::DriverError::Runtime(error))
        };
        assert_eq!(next_value(&job), Err(expected));
        assert_eq!(attempts(&fixture, &name), 0);
        // The actual cursor advance is preserved despite the subsequent error.
        assert!(pass.clone().next().unwrap().is_none());
        pass.finish().unwrap();
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_next_jobs_original_close_drains_queued_selection_and_retains_cell_after_reopen() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let pool = hold_pool(&fixture);
        let job = pass.clone().start_next().unwrap();
        assert_eq!(next_value(&job), Ok(None));
        let requested = std::thread::scope(|scope| {
            let host = fixture.workflow.host.clone();
            let close = scope.spawn(move || host.close());
            let deadline = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while !job.handle.cancel.requested() && meguri_engine::monotonic::now() < deadline {
                std::thread::yield_now();
            }
            let requested = job.handle.cancel.requested();
            drop(pool);
            close.join().unwrap().unwrap();
            requested
        });
        assert!(requested && engine.jobs().running().is_empty());
        assert_eq!(finished(&events, job.handle.id), JobOutcome::Cancelled);
        let successor = published().0;
        let before = successor.workflow.core.adapter_snapshot().unwrap();
        assert_eq!(next_value(&job), Err(CoreError::Cancelled));
        pass.finish().unwrap();
        assert!(before == successor.workflow.core.adapter_snapshot().unwrap());
        successor.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_next_jobs_generation_change_after_runtime_wait_refuses_before_selection() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let runtime = fixture.workflow.core.clone();
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        let (advanced, generation_reached) = mpsc::channel();
        let (continue_writer, writer_resume) = mpsc::channel();
        exclusive.collection().after_next_commit(move |_| {
            let _ = advanced.send(());
            writer_resume.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        runtime.observe_state_contention_for_testing(contended);
        let (entry, contention, advanced, value) = std::thread::scope(|scope| {
            let release = Release(release);
            let continue_writer = Release(continue_writer);
            let held_runtime = runtime.clone();
            let holder = scope.spawn(move || {
                held_runtime.with_state_held_for_testing(|| {
                    let _ = entered.send(());
                    resume.recv_timeout(Duration::from_secs(10)).unwrap();
                })
            });
            let entry = reached.recv_timeout(Duration::from_secs(5));
            let job = pass.clone().start_next().unwrap();
            let contention = observed.recv_timeout(Duration::from_secs(5));
            let replacement = scope.spawn(move || exclusive.replace(|_| Ok(())).map(|_| ()));
            let advanced = generation_reached.recv_timeout(Duration::from_secs(5));
            drop(release);
            holder.join().unwrap();
            // The replacement writer has reserved an earlier stream event.
            // Read only the retained cell until it returns, before releasing it.
            let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
            let value = loop {
                let value = next_value(&job);
                if value != Ok(None) || meguri_engine::monotonic::now() >= deadline {
                    break value;
                }
                std::thread::yield_now();
            };
            drop(continue_writer);
            replacement.join().unwrap().unwrap();
            let terminal = finished(&events, job.handle.id);
            assert!(matches!(terminal, JobOutcome::Failed { .. }));
            assert_eq!(next_value(&job), value);
            (entry, contention, advanced, value)
        });
        entry.unwrap();
        contention.unwrap();
        advanced.unwrap();
        assert_eq!(
            value,
            Err(CoreError::CloudPolicy {
                failure: CloudPolicyFailure::GenerationChanged
            })
        );
        pass.finish().unwrap();
        drop(slot);
        fixture.workflow.host.close().unwrap();
    }
}
