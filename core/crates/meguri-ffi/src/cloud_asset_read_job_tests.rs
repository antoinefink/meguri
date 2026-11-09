//! J2.1.1/P4/K12.3: real pass capabilities on the original registered pool.
//! Component source: 03abad1a; batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
use super::*;
use crate::sync::tests::policy::policy_job_tests::{finished, hold_pool};
use meguri_engine::{JobKind, JobOutcome};
use meguri_sync::cloud::testing::IncomingStage as Stage;
use std::sync::mpsc;

#[derive(Clone)]
enum Port {
    Retry(Arc<CloudPolicyExecution>),
    First(Arc<CloudWorkflow>, String),
}
impl Port {
    fn begin(&self) -> Result<Arc<CloudAssetReadPass>, CoreError> {
        match self {
            Self::Retry(execution) => execution.clone().begin_asset_read_pass(),
            Self::First(workflow, sdk) => workflow.begin_first_asset_read_pass(sdk.clone()),
        }
    }

    fn start(&self) -> Result<CloudPolicyAssetReadPassJob, CoreError> {
        match self {
            Self::Retry(execution) => execution.clone().start_begin_asset_read_pass(),
            Self::First(workflow, sdk) => workflow.clone().start_begin_first_asset_read_pass(sdk.clone()),
        }
    }
}

fn workflow(fixture: &PublishedFixture) -> Arc<CloudWorkflow> {
    Arc::new(CloudWorkflow {
        host: fixture.workflow.host.clone(),
        core: fixture.workflow.core.clone(),
        identity: fixture.workflow.identity.clone(),
    })
}

fn target(first: bool) -> (PublishedFixture, CloudEngineRegistration, Port, String) {
    if first {
        let FirstConflict {
            fixture, sdk, server, ..
        } = first_conflict();
        let port = Port::First(workflow(&fixture), sdk.instance_id.clone());
        (fixture, sdk, port, server.name)
    } else {
        let (fixture, sdk) = published();
        let (server, _, _) = waiting_note(&fixture, &sdk);
        let command = retry_command(&fixture, &sdk);
        let execution = fixture
            .policy
            .clone()
            .prepare_execution(command.handle)
            .unwrap()
            .unwrap();
        (fixture, sdk, Port::Retry(execution), server.name)
    }
}

struct Release(mpsc::Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn outcome(job: &CloudPolicyAssetReadPassJob) -> Result<Option<()>, CoreError> {
    job.result.result().map(|pass| pass.map(|_| ()))
}

#[test]
fn native_admission_jobs_asset_passes_queue_cancel_replace_and_close_without_rebinding() {
    for first in [false, true] {
        for transition in 0..3 {
            let (fixture, sdk, port, name) = target(first);
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let before = fixture.workflow.core.adapter_snapshot().unwrap();
            let pool = hold_pool(&fixture);
            let job = port.start().unwrap();
            let pending = outcome(&job);
            let mut cancellation = None;
            match transition {
                0 => cancellation = Some(job.handle.cancel.cancel()),
                1 => {
                    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                    let mut exclusive = slot.enter_exclusive(true).unwrap();
                    exclusive.replace(|_| Ok(())).unwrap();
                }
                _ => {
                    let (requested, closed) = std::thread::scope(|scope| {
                        let host = fixture.workflow.host.clone();
                        let closing = scope.spawn(move || host.close());
                        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(3);
                        while !job.handle.cancel.requested() && meguri_engine::monotonic::now() < deadline {
                            std::thread::yield_now();
                        }
                        let requested = job.handle.cancel.requested();
                        drop(pool);
                        (requested, closing.join())
                    });
                    let terminal = finished(&events, job.handle.id);
                    let drained = engine.jobs().running().is_empty();
                    let replacement = published().0;
                    let value = outcome(&job);
                    replacement.workflow.host.close().unwrap();
                    closed.unwrap().unwrap();
                    assert!(requested && drained);
                    assert_eq!(pending, Ok(None));
                    assert_eq!(terminal, JobOutcome::Cancelled);
                    assert_eq!(value, Err(CoreError::Cancelled));
                    continue;
                }
            }
            drop(pool);
            let terminal = finished(&events, job.handle.id);
            let value = outcome(&job);
            let retry = if transition == 0 {
                let after = fixture.workflow.core.adapter_snapshot().unwrap();
                let original = fixture.workflow.core.validate_sdk(&sdk.instance_id);
                let next = port.start().unwrap();
                let terminal = finished(&events, next.handle.id);
                let pass = next.result.result().unwrap().unwrap();
                let work = pass.clone().next().unwrap().unwrap();
                let actual_name = work.name;
                let actual_kind = work.kind;
                pass.cancel(work.handle).unwrap();
                pass.finish().unwrap();
                Some((after == before, original, terminal, actual_name, actual_kind))
            } else {
                None
            };
            fixture.workflow.host.close().unwrap();
            let replacement = published().0;
            let retained = outcome(&job);
            let refused = port.start().err();
            let legacy = port.begin().err();
            replacement.workflow.host.close().unwrap();
            assert_eq!(pending, Ok(None));
            assert_eq!(retained, value);
            match &port {
                Port::Retry(_) => assert_eq!(
                    refused,
                    Some(CoreError::CloudPolicy {
                        failure: CloudPolicyFailure::Closed
                    })
                ),
                Port::First(_, _) => assert_eq!(
                    refused,
                    Some(CoreError::Unavailable {
                        reason: "cloud connection host closed".into()
                    })
                ),
            }
            assert_eq!(refused, legacy);
            if let Some((unchanged, original, retried, actual_name, actual_kind)) = retry {
                cancellation.unwrap().unwrap();
                assert!(unchanged);
                original.unwrap();
                assert_eq!(terminal, JobOutcome::Cancelled);
                assert_eq!(value, Err(CoreError::Cancelled));
                assert_eq!(retried, JobOutcome::Succeeded);
                assert_eq!(actual_name, name);
                assert_eq!(
                    actual_kind,
                    if first {
                        CloudAssetReadKind::SaveConflictFirst
                    } else {
                        CloudAssetReadKind::Retry
                    }
                );
            } else {
                assert!(matches!(terminal, JobOutcome::Failed { .. }));
                assert_eq!(
                    value,
                    Err(CoreError::CloudPolicy {
                        failure: CloudPolicyFailure::GenerationChanged
                    })
                );
            }
        }
    }
}

#[test]
fn native_admission_jobs_asset_passes_contended_cancel_preserves_original_selection_and_review() {
    for first in [false, true] {
        // The first-read port intentionally never acquires Driver authority.
        for driver_hold in [false, true].into_iter().filter(|driver| !first || !driver) {
            let (fixture, sdk, port, name) = target(first);
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let runtime = fixture.workflow.core.clone();
            meguri_engine::sched::fsrs_policy::enable_fsrs(
                &fixture.workflow.host.collection().unwrap(),
                &Default::default(),
            )
            .unwrap();
            let existing = port.start().unwrap();
            let original_terminal = finished(&events, existing.handle.id);
            let existing = existing.result.result().unwrap().unwrap();
            let selected = existing.clone().next().unwrap().unwrap();
            let before = runtime.adapter_snapshot().unwrap();
            let attempts_before = attempts(&fixture, &name);
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let (contended, observed) = mpsc::channel();
            let hold_state = Arc::new(Mutex::new((false, false)));
            let state = hold_state.clone();
            let hold = move || {
                *state.lock().unwrap() = (true, false);
                let _ = entered.send(());
                let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
                *state.lock().unwrap() = (false, timed_out);
            };
            let policy = fixture.policy.clone();
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let (
                entry,
                contention,
                admitted,
                pending,
                registered,
                slot,
                review,
                held_at_review,
                cancellation,
                holder,
                starter,
            ) = std::thread::scope(|scope| {
                let release = Release(release);
                let runtime = runtime.clone();
                let holder = if driver_hold {
                    driver.observe_contention_for_testing(contended);
                    driver.set_projection_hook_for_testing(hold);
                    scope.spawn(move || policy.input(CloudPolicyEvent::Foreground).map(|_| ()))
                } else {
                    runtime.observe_state_contention_for_testing(contended);
                    scope.spawn(move || {
                        runtime.with_state_held_for_testing(hold);
                        Ok(())
                    })
                };
                let entry = reached.recv_timeout(Duration::from_secs(5));
                let (sent, received) = mpsc::channel();
                let port = port.clone();
                let starter = scope.spawn(move || {
                    let _ = sent.send(port.start());
                });
                let admitted = received.recv_timeout(Duration::from_secs(5));
                let contention = observed.recv_timeout(Duration::from_secs(5));
                let pending = admitted.as_ref().ok().and_then(|job| job.as_ref().ok()).map(outcome);
                let registered = admitted
                    .as_ref()
                    .ok()
                    .and_then(|job| job.as_ref().ok())
                    .is_some_and(|job| {
                        engine
                            .jobs()
                            .running()
                            .iter()
                            .any(|running| running.id.0 == job.handle.id)
                    });
                let slot = engine.slot_holder();
                let (sent, received) = mpsc::channel();
                let host = fixture.workflow.host.clone();
                let review = engine.start_job(JobKind::BulkOp, move |job| {
                    let value = host
                        .review()
                        .start_controlled(
                            meguri_model::ids::DeckId(1),
                            None,
                            job.cancel.clone(),
                            job.progress.clone(),
                        )
                        .and_then(|value| {
                            if let meguri_engine::session::StartResult::Started { session_id, .. } = value {
                                host.review()
                                    .end_controlled(session_id, job.cancel.clone(), job.progress.clone())?;
                                Ok(true)
                            } else {
                                Ok(false)
                            }
                        });
                    let _ = sent.send(value);
                    Ok(())
                });
                let review = (review, received.recv_timeout(Duration::from_secs(5)));
                let held_at_review = *hold_state.lock().unwrap();
                let cancellation = admitted
                    .as_ref()
                    .ok()
                    .and_then(|job| job.as_ref().ok())
                    .map(|job| job.handle.cancel.cancel());
                drop(release);
                (
                    entry,
                    contention,
                    admitted,
                    pending,
                    registered,
                    slot,
                    review,
                    held_at_review,
                    cancellation,
                    holder.join(),
                    starter.join(),
                )
            });
            let job = admitted.expect("start completed before held lock release").unwrap();
            let terminal = finished(&events, job.handle.id);
            let value = outcome(&job);
            let after = runtime.adapter_snapshot().unwrap();
            let original_sdk = runtime.validate_sdk(&sdk.instance_id);
            let attempts_after = attempts(&fixture, &name);
            // Exact outstanding selection survives cancelled creation, including
            // its one-work rule and uncharged/refused native enqueue.
            let second = existing.clone().next();
            let refused = existing.submit(selected.handle, Queue::new(false));
            existing.finish().unwrap();
            let next = port.start().unwrap();
            let retried = finished(&events, next.handle.id);
            let pass = next.result.result().unwrap().unwrap();
            let next_work = pass.clone().next().unwrap().unwrap();
            let next_name = next_work.name;
            pass.cancel(next_work.handle).unwrap();
            pass.finish().unwrap();
            fixture.workflow.host.close().unwrap();
            entry.expect("real lock holder entered");
            contention.expect("positive worker try_lock contention");
            holder.unwrap().unwrap();
            starter.unwrap();
            cancellation.unwrap().unwrap();
            review.0.unwrap();
            assert!(review.1.expect("unrelated review completed before release").unwrap());
            assert_eq!(held_at_review, (true, false));
            assert!(!hold_state.lock().unwrap().1);
            assert!(registered && slot.is_none());
            assert_eq!(pending, Some(Ok(None)));
            assert_eq!(original_terminal, JobOutcome::Succeeded);
            assert_eq!(terminal, JobOutcome::Cancelled);
            assert_eq!(value, Err(CoreError::Cancelled));
            assert!(before == after);
            original_sdk.unwrap();
            assert_eq!(attempts_before, attempts_after);
            assert!(second.is_err());
            assert_eq!(refused, Ok(false));
            assert_eq!(retried, JobOutcome::Succeeded);
            assert_eq!(next_name, name);
        }
    }
}

#[test]
fn native_admission_jobs_asset_passes_post_effect_cancel_retains_exact_pass_after_close() {
    for first in [false, true] {
        for stage in [Stage::AssetReadPassEffect, Stage::AssetReadPassCreated] {
            let (fixture, sdk, port, name) = target(first);
            let runtime = fixture.workflow.core.clone();
            let events = fixture.workflow.host.engine().stream().subscribe();
            let pool = hold_pool(&fixture);
            let token = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
            let cancel = token.clone();
            let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let saw = observed.clone();
            runtime.set_incoming_stage_hook_for_testing(move |hit| {
                if hit == stage {
                    saw.store(true, Ordering::SeqCst);
                    cancel.lock().unwrap().as_ref().unwrap().trigger();
                }
            });
            let job = port.start().unwrap();
            *token.lock().unwrap() = Some(job.handle.cancel.clone());
            drop(pool);
            let terminal = finished(&events, job.handle.id);
            runtime.set_incoming_stage_hook_for_testing(|_| {});
            let pass = job.result.result().unwrap().unwrap();
            let issued = pass.clone().next().unwrap().unwrap();
            let actual_name = issued.name;
            let actual_kind = issued.kind;
            let original_sdk = runtime.validate_sdk(&sdk.instance_id);
            pass.cancel(issued.handle).unwrap();
            pass.finish().unwrap();
            fixture.workflow.host.close().unwrap();
            let replacement = published().0;
            let retained = job.result.result().unwrap().unwrap();
            replacement.workflow.host.close().unwrap();
            assert!(observed.load(Ordering::SeqCst) && job.handle.cancel.requested());
            assert_eq!(terminal, JobOutcome::Succeeded);
            assert!(Arc::ptr_eq(&pass, &retained));
            assert_eq!(actual_name, name);
            assert_eq!(
                actual_kind,
                if first {
                    CloudAssetReadKind::SaveConflictFirst
                } else {
                    CloudAssetReadKind::Retry
                }
            );
            original_sdk.unwrap();
        }
    }
}

#[test]
fn native_admission_job_completed_empty_first_pass_is_some_and_wrong_sdk_keeps_legacy_error() {
    let (fixture, sdk) = published();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let original = workflow(&fixture);
    let pool = hold_pool(&fixture);
    let job = original
        .clone()
        .start_begin_first_asset_read_pass(sdk.instance_id)
        .unwrap();
    let pending = outcome(&job);
    drop(pool);
    let terminal = finished(&events, job.handle.id);
    let pass = job.result.result().unwrap().unwrap();
    let empty = pass.clone().next();
    pass.finish().unwrap();
    let legacy = fixture.workflow.begin_first_asset_read_pass("wrong-sdk".into()).err();
    let wrong = original.start_begin_first_asset_read_pass("wrong-sdk".into()).unwrap();
    let failed = finished(&events, wrong.handle.id);
    fixture.workflow.host.close().unwrap();
    assert_eq!(pending, Ok(None));
    assert_eq!(terminal, JobOutcome::Succeeded);
    assert!(matches!(empty, Ok(None)));
    assert!(matches!(failed, JobOutcome::Failed { .. }));
    assert_eq!(outcome(&wrong).err(), legacy);
    assert!(job.result.result().unwrap().is_some());
}

#[test]
fn native_admission_job_asset_pass_nonretry_command_keeps_exact_public_refusal() {
    let (fixture, _) = published();
    let command = fixture
        .outstanding_collection
        .lock()
        .unwrap()
        .iter()
        .find(|command| matches!(command.action, CloudPolicyAction::RequestFetch { .. }))
        .unwrap()
        .handle
        .clone();
    let execution = fixture.policy.clone().prepare_execution(command).unwrap().unwrap();
    let legacy = execution.clone().begin_asset_read_pass().err();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let job = execution.start_begin_asset_read_pass().unwrap();
    let terminal = finished(&events, job.handle.id);
    let value = outcome(&job);
    fixture.workflow.host.close().unwrap();
    assert!(matches!(terminal, JobOutcome::Failed { .. }));
    assert_eq!(
        legacy,
        Some(cloud_error(meguri_sync::cloud::policy::DriverError::NotCoreAction))
    );
    assert_eq!(value.err(), legacy);
    assert_eq!(fixture.workflow.core.asset_read_pass_count_for_testing(), 0);
}

#[test]
fn native_admission_jobs_asset_pass_tracking_wait_checks_work_before_selection_cleanup() {
    for first in [false, true] {
        let (fixture, sdk, port, name) = target(first);
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let runtime = fixture.workflow.core.clone();
        let existing = port.start().unwrap();
        let original_terminal = finished(&events, existing.handle.id);
        let existing = existing.result.result().unwrap().unwrap();
        let selected = existing.clone().next().unwrap().unwrap();
        let before = runtime.adapter_snapshot().unwrap();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        let watch = Arc::new(Mutex::new((false, false)));
        let held_copy = watch.clone();
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadPassTrackingContended {
                let _ = contended.send(());
            }
        });
        let (entry, contention, admitted, pending, cancellation, held_at_cancel, holder, starter) =
            std::thread::scope(|scope| {
                let release = Release(release);
                let holder = scope.spawn(move || {
                    r::with_tracking_access(|| {
                        *held_copy.lock().unwrap() = (true, false);
                        let _ = entered.send(());
                        let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
                        *held_copy.lock().unwrap() = (false, timed_out);
                    });
                });
                let entry = reached.recv_timeout(Duration::from_secs(5));
                let (sent, received) = mpsc::channel();
                let selected_port = port.clone();
                let starter = scope.spawn(move || {
                    let _ = sent.send(selected_port.start());
                });
                let admitted = received.recv_timeout(Duration::from_secs(5));
                let contention = observed.recv_timeout(Duration::from_secs(5));
                let pending = admitted.as_ref().ok().and_then(|job| job.as_ref().ok()).map(outcome);
                let cancellation = admitted
                    .as_ref()
                    .ok()
                    .and_then(|job| job.as_ref().ok())
                    .map(|job| job.handle.cancel.cancel());
                let held_at_cancel = *watch.lock().unwrap();
                drop(release);
                (
                    entry,
                    contention,
                    admitted,
                    pending,
                    cancellation,
                    held_at_cancel,
                    holder.join(),
                    starter.join(),
                )
            });
        let job = admitted.expect("start did not acquire tracking").unwrap();
        let terminal = finished(&events, job.handle.id);
        runtime.set_incoming_stage_hook_for_testing(|_| {});
        let after = runtime.adapter_snapshot().unwrap();
        let sdk_valid = runtime.validate_sdk(&sdk.instance_id);
        let outstanding = existing.clone().next();
        let refused = existing.submit(selected.handle, Queue::new(false));
        existing.finish().unwrap();
        let next = port.start().unwrap();
        let retried = finished(&events, next.handle.id);
        let pass = next.result.result().unwrap().unwrap();
        let issued = pass.clone().next().unwrap().unwrap();
        let next_name = issued.name;
        pass.cancel(issued.handle).unwrap();
        pass.finish().unwrap();
        fixture.workflow.host.close().unwrap();
        entry.expect("real tracking gate held");
        contention.expect("worker positively encountered held tracking gate");
        cancellation.unwrap().unwrap();
        holder.unwrap();
        starter.unwrap();
        assert_eq!(held_at_cancel, (true, false));
        assert!(!watch.lock().unwrap().1);
        assert_eq!(pending, Some(Ok(None)));
        assert_eq!(original_terminal, JobOutcome::Succeeded);
        assert_eq!(terminal, JobOutcome::Cancelled);
        assert_eq!(outcome(&job), Err(CoreError::Cancelled));
        assert!(before == after);
        sdk_valid.unwrap();
        assert!(outstanding.is_err());
        assert_eq!(refused, Ok(false));
        assert_eq!(retried, JobOutcome::Succeeded);
        assert_eq!(next_name, name);
    }
}

#[test]
fn native_admission_jobs_asset_passes_keep_exact_first_retry_members_and_wrong_owner_refusal() {
    let FirstConflict {
        fixture, sdk, server, ..
    } = first_conflict();
    let (retry_server, _, _) = waiting_note(&fixture, &sdk);
    let command = retry_command(&fixture, &sdk);
    let execution = fixture
        .policy
        .clone()
        .prepare_execution(command.handle)
        .unwrap()
        .unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let first = workflow(&fixture)
        .start_begin_first_asset_read_pass(sdk.instance_id.clone())
        .unwrap();
    let first_terminal = finished(&events, first.handle.id);
    let retry = execution.start_begin_asset_read_pass().unwrap();
    let retry_terminal = finished(&events, retry.handle.id);
    let first = first.result.result().unwrap().unwrap();
    let retry = retry.result.result().unwrap().unwrap();
    let first_work = first.clone().next().unwrap().unwrap();
    let retry_work = retry.clone().next().unwrap().unwrap();
    let first_busy = first.clone().next();
    let retry_busy = retry.clone().next();
    let queue = Queue::new(true);
    let wrong = retry.submit(first_work.handle.clone(), queue.clone());
    let first_refused = first.submit(first_work.handle, Queue::new(false));
    let retry_refused = retry.submit(retry_work.handle, Queue::new(false));
    let first_attempts = attempts(&fixture, &server.name);
    let retry_attempts = attempts(&fixture, &retry_server.name);
    first.finish().unwrap();
    retry.finish().unwrap();
    fixture.workflow.host.close().unwrap();
    assert_eq!(first_terminal, JobOutcome::Succeeded);
    assert_eq!(retry_terminal, JobOutcome::Succeeded);
    assert_eq!(first_work.name, server.name);
    assert_eq!(retry_work.name, retry_server.name);
    assert_eq!(first_work.kind, CloudAssetReadKind::SaveConflictFirst);
    assert_eq!(retry_work.kind, CloudAssetReadKind::Retry);
    assert!(first_busy.is_err() && retry_busy.is_err());
    assert_eq!(wrong, Err(cloud_error("wrong asset read owner")));
    assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
    assert_eq!(first_refused, Ok(false));
    assert_eq!(retry_refused, Ok(false));
    assert_eq!((first_attempts, retry_attempts), (0, 0));
}

#[test]
fn native_admission_jobs_asset_pass_activity_guard_failure_cleans_original_and_preserves_successor() {
    for first in [false, true] {
        let (fixture, sdk, port, _) = target(first);
        let runtime = fixture.workflow.core.clone();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let token = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
        let cancel = token.clone();
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let saw = observed.clone();
        let original = workflow(&fixture);
        let successor = Arc::new(Mutex::new(None));
        let retained = successor.clone();
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadPassCreated && !saw.swap(true, Ordering::SeqCst) {
                // This witness is outside all Core locks. Retire the actual SDK
                // and install a real successor before the old activity read.
                cancel.lock().unwrap().as_ref().unwrap().trigger();
                original.retire_sdk(sdk.instance_id.clone(), false).unwrap();
                original.release_sdk(sdk.instance_id.clone()).unwrap();
                let new_sdk = original.register_sdk(false).unwrap();
                let new_pass = original
                    .begin_first_asset_read_pass(new_sdk.instance_id.clone())
                    .unwrap();
                *retained.lock().unwrap() = Some((new_sdk, new_pass));
            }
        });
        let job = port.start().unwrap();
        *token.lock().unwrap() = Some(job.handle.cancel.clone());
        drop(pool);
        let terminal = finished(&events, job.handle.id);
        runtime.set_incoming_stage_hook_for_testing(|_| {});
        let value = outcome(&job);
        let (new_sdk, new_pass) = successor.lock().unwrap().take().unwrap();
        let sdk_valid = runtime.validate_sdk(&new_sdk.instance_id);
        let guard_valid = runtime.asset_read_pass_count_for_testing();
        let successor_work = new_pass.clone().next();
        if let Ok(Some(work)) = &successor_work {
            new_pass.cancel(work.handle.clone()).unwrap();
        }
        new_pass.finish().unwrap();
        let remaining = runtime.asset_read_pass_count_for_testing();
        fixture.workflow.host.close().unwrap();
        assert!(observed.load(Ordering::SeqCst) && job.handle.cancel.requested());
        assert!(matches!(terminal, JobOutcome::Failed { .. }));
        assert_eq!(value, Err(cloud_error(r::RuntimeError::StaleIncarnation)));
        assert_eq!(outcome(&job), value);
        sdk_valid.unwrap();
        assert_eq!(guard_valid, 1, "only the actual successor pass remains");
        assert!(successor_work.is_ok());
        assert_eq!(remaining, 0);
    }
}

#[test]
fn native_admission_jobs_asset_pass_contended_generation_replacement_retains_sampled_failure() {
    for first in [false, true] {
        let (fixture, sdk, port, _) = target(first);
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let runtime = fixture.workflow.core.clone();
        let before = runtime.adapter_snapshot().unwrap();
        let generation = engine.generation();
        let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        let (advanced, generation_reached) = mpsc::channel();
        let (continue_writer, writer_resume) = mpsc::channel();
        let runtime_watch = Arc::new(Mutex::new((false, false)));
        let writer_watch = Arc::new(Mutex::new((false, false)));
        let writer_held = writer_watch.clone();
        let generation_source = engine.clone();
        exclusive.collection().after_next_commit(move |_| {
            *writer_held.lock().unwrap() = (true, false);
            let _ = advanced.send(generation_source.generation());
            let timed_out = writer_resume.recv_timeout(Duration::from_secs(10)).is_err();
            *writer_held.lock().unwrap() = (false, timed_out);
        });
        runtime.observe_state_contention_for_testing(contended);
        let (
            entry,
            contention,
            job,
            advanced,
            value,
            unchanged,
            original_sdk,
            held_at_result,
            holder,
            starter,
            replacement,
        ) = std::thread::scope(|scope| {
            let release = Release(release);
            let continue_writer = Release(continue_writer);
            let held_runtime = runtime.clone();
            let held_state = runtime_watch.clone();
            let holder = scope.spawn(move || {
                held_runtime.with_state_held_for_testing(|| {
                    *held_state.lock().unwrap() = (true, false);
                    let _ = entered.send(());
                    let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
                    *held_state.lock().unwrap() = (false, timed_out);
                });
            });
            let entry = reached.recv_timeout(Duration::from_secs(5));
            let (sent, received) = mpsc::channel();
            let starter = scope.spawn(move || {
                let _ = sent.send(port.start());
            });
            let job = received.recv_timeout(Duration::from_secs(5));
            let contention = observed.recv_timeout(Duration::from_secs(5));
            let replacement = scope.spawn(move || exclusive.replace(|_| Ok(())).map(|_| ()));
            let advanced = generation_reached.recv_timeout(Duration::from_secs(5));
            drop(release);
            let holder = holder.join();
            // Replacement's reserved event precedes JobFinished. Inspect the
            // retained cell while its original Runtime is still bound.
            let value = if let Ok(Ok(job)) = &job {
                let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
                loop {
                    let value = outcome(job);
                    if value != Ok(None) || meguri_engine::monotonic::now() >= deadline {
                        break Some(value);
                    }
                    std::thread::yield_now();
                }
            } else {
                None
            };
            let unchanged = runtime.adapter_snapshot().unwrap() == before;
            let original_sdk = runtime.validate_sdk(&sdk.instance_id);
            let held_at_result = *writer_watch.lock().unwrap();
            drop(continue_writer);
            (
                entry,
                contention,
                job,
                advanced,
                value,
                unchanged,
                original_sdk,
                held_at_result,
                holder,
                starter.join(),
                replacement.join(),
            )
        });
        drop(slot);
        fixture.workflow.host.close().unwrap();
        let job = job.expect("start returned while Runtime held").unwrap();
        let terminal = finished(&events, job.handle.id);
        holder.unwrap();
        starter.unwrap();
        replacement.unwrap().unwrap();
        entry.expect("Runtime held after Exclusive flush");
        contention.expect("positive worker Runtime contention");
        assert!(advanced.expect("real generation replacement") > generation);
        assert_eq!(held_at_result, (true, false));
        assert!(!runtime_watch.lock().unwrap().1 && !writer_watch.lock().unwrap().1);
        assert!(unchanged);
        original_sdk.unwrap();
        assert!(matches!(terminal, JobOutcome::Failed { .. }));
        let expected = Err(CoreError::CloudPolicy {
            failure: CloudPolicyFailure::GenerationChanged,
        });
        assert_eq!(value, Some(expected.clone()));
        assert_eq!(outcome(&job), expected);
    }
}

#[test]
fn native_admission_jobs_asset_pass_active_close_drains_worker_before_original_result_read() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let runtime = fixture.workflow.core.clone();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        let watch = Arc::new(Mutex::new((false, false)));
        let held = watch.clone();
        runtime.observe_state_contention_for_testing(contended);
        let (entry, contention, admitted, requested, holder, starter, closing) = std::thread::scope(|scope| {
            let release = Release(release);
            let holder = scope.spawn(move || {
                runtime.with_state_held_for_testing(|| {
                    *held.lock().unwrap() = (true, false);
                    let _ = entered.send(());
                    let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
                    *held.lock().unwrap() = (false, timed_out);
                });
            });
            let entry = reached.recv_timeout(Duration::from_secs(5));
            let (sent, received) = mpsc::channel();
            let starter = scope.spawn(move || {
                let _ = sent.send(port.start());
            });
            let admitted = received.recv_timeout(Duration::from_secs(5));
            let contention = observed.recv_timeout(Duration::from_secs(5));
            let host = fixture.workflow.host.clone();
            let closing = scope.spawn(move || host.close());
            let requested = if let Ok(Ok(job)) = &admitted {
                let deadline = meguri_engine::monotonic::now() + Duration::from_secs(3);
                while !job.handle.cancel.requested() && meguri_engine::monotonic::now() < deadline {
                    std::thread::yield_now();
                }
                job.handle.cancel.requested()
            } else {
                false
            };
            drop(release);
            (
                entry,
                contention,
                admitted,
                requested,
                holder.join(),
                starter.join(),
                closing.join(),
            )
        });
        let job = admitted.expect("start did not wait for Runtime").unwrap();
        let terminal = finished(&events, job.handle.id);
        let drained = engine.jobs().running().is_empty();
        let replacement = published().0;
        let retained = outcome(&job);
        replacement.workflow.host.close().unwrap();
        holder.unwrap();
        starter.unwrap();
        closing.unwrap().unwrap();
        entry.expect("real Runtime holder");
        contention.expect("worker reached Runtime before close");
        assert!(requested && drained);
        assert!(!watch.lock().unwrap().1);
        assert_eq!(terminal, JobOutcome::Cancelled);
        assert_eq!(retained, Err(CoreError::Cancelled));
    }
}

#[path = "cloud_asset_next_job_tests.rs"]
mod next;
