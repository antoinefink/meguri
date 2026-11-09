//! J2.1.1: retained native admission on the original registered pool.
use super::super::policy::policy_job_tests::{finished, hold_pool};
use super::*;
use meguri_engine::{JobKind, JobOutcome};
use std::sync::mpsc;

fn policy_failure(failure: CloudPolicyFailure) -> CoreError {
    CoreError::CloudPolicy { failure }
}

struct Release(mpsc::Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

enum AdmissionJob {
    Register(CloudPolicyRegistrationJob),
    Retire(CloudPolicyRetirementJob),
}
impl AdmissionJob {
    fn handle(&self) -> &crate::jobs::JobHandle {
        match self {
            Self::Register(job) => &job.handle,
            Self::Retire(job) => &job.handle,
        }
    }
    fn result(&self) -> Result<Option<()>, CoreError> {
        match self {
            Self::Register(job) => job.result.result().map(|value| value.map(|_| ())),
            Self::Retire(job) => job.result.result().map(|value| value.map(|_| ())),
        }
    }
}
fn admit(execution: Arc<CloudPolicyExecution>, retire: bool) -> Result<AdmissionJob, CoreError> {
    if retire {
        execution.start_retire_original().map(AdmissionJob::Retire)
    } else {
        execution.start_register_sdk().map(AdmissionJob::Register)
    }
}
fn admission_fixture(retire: bool) -> (Fixture, Arc<CloudPolicy>, Arc<CloudPolicyExecution>, Option<String>) {
    let (fixture, policy, execution) = start();
    if !retire {
        return (fixture, policy, execution, None);
    }
    let original = execution.register_sdk().unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let command = commands(policy.clone().input(settings(true, false)).unwrap())
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::ConfigureAutomatic { on: true }))
        .unwrap();
    let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
    (fixture, policy, execution, Some(original.instance_id))
}

#[test]
fn native_admission_jobs_start_without_slot_while_driver_projection_and_review_run() {
    for retire in [false, true] {
        let (fixture, policy, execution, _) = admission_fixture(retire);
        meguri_engine::sched::fsrs_policy::enable_fsrs(
            &fixture.workflow.host.collection().unwrap(),
            &Default::default(),
        )
        .unwrap();
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let projection_state = Arc::new(Mutex::new((false, false)));
        let held_state = projection_state.clone();
        driver.set_projection_hook_for_testing(move || {
            *held_state.lock().unwrap() = (true, false);
            let _ = entered.send(());
            let watchdog_released = resume.recv_timeout(Duration::from_secs(10)).is_err();
            *held_state.lock().unwrap() = (false, watchdog_released);
        });
        let (entry, admitted, registered, pending, slot, review, at_review_completion, projection, starter) =
            std::thread::scope(|scope| {
                let release = Release(release);
                let held = scope.spawn(move || policy.input(CloudPolicyEvent::Foreground));
                let entry = reached.recv_timeout(Duration::from_secs(5));
                let (sent, received) = mpsc::channel();
                let starter = scope.spawn(move || {
                    let _ = sent.send(admit(execution, retire));
                });
                let admitted = received.recv_timeout(Duration::from_secs(5));
                let mut registered = false;
                let mut pending = false;
                if let Ok(Ok(job)) = &admitted {
                    registered = engine
                        .jobs()
                        .running()
                        .iter()
                        .any(|running| running.id.0 == job.handle().id);
                    pending = matches!(job.result(), Ok(None));
                }
                let slot = engine.slot_holder();
                let host = fixture.workflow.host.clone();
                let (sent, received) = mpsc::channel();
                let review = engine.start_job(JobKind::BulkOp, move |job| {
                    let result = host
                        .review()
                        .start_controlled(
                            meguri_model::ids::DeckId(1),
                            None,
                            job.cancel.clone(),
                            job.progress.clone(),
                        )
                        .and_then(|result| {
                            if let meguri_engine::session::StartResult::Started { session_id, .. } = result {
                                host.review()
                                    .end_controlled(session_id, job.cancel.clone(), job.progress.clone())?;
                                Ok(true)
                            } else {
                                Ok(false)
                            }
                        });
                    let _ = sent.send(result);
                    Ok(())
                });
                let review = (review, received.recv_timeout(Duration::from_secs(5)));
                let at_review_completion = *projection_state.lock().unwrap();
                drop(release);
                let projection = held.join();
                let starter = starter.join();
                (
                    entry,
                    admitted,
                    registered,
                    pending,
                    slot,
                    review,
                    at_review_completion,
                    projection,
                    starter,
                )
            });
        // Release and join both scoped threads before inspecting any assertion.
        let job = admitted.expect("bounded native job start").unwrap();
        let outcome = finished(&events, job.handle().id);
        let value = job.result();
        fixture.workflow.host.close().unwrap();
        assert_eq!(
            at_review_completion,
            (true, false),
            "projection remained held without watchdog release through review completion"
        );
        assert!(
            !projection_state.lock().unwrap().1,
            "projection watchdog did not release the hook"
        );
        entry.expect("real projection owns Driver");
        starter.unwrap();
        projection.unwrap().unwrap();
        review.0.unwrap();
        assert!(
            review
                .1
                .expect("review start/end completed before projection release")
                .unwrap()
        );
        assert!(registered && pending);
        assert!(slot.is_none());
        assert_eq!(outcome, JobOutcome::Succeeded);
        assert_eq!(value, Ok(Some(())));
    }
}

#[test]
fn native_admission_jobs_cancel_after_real_driver_and_runtime_contention_before_effects() {
    for retire in [false, true] {
        for runtime_hold in [false, true] {
            let (fixture, policy, execution, original) = admission_fixture(retire);
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let runtime = fixture.workflow.core.clone();
            let before = runtime.adapter_guard();
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let (contended, observed) = mpsc::channel();
            let (entry, contention, admitted, cancellation, held, starter) = std::thread::scope(|scope| {
                let release = Release(release);
                let held = if runtime_hold {
                    runtime.observe_state_contention_for_testing(contended);
                    let runtime = runtime.clone();
                    scope.spawn(move || {
                        runtime.with_state_held_for_testing(|| {
                            let _ = entered.send(());
                            let _ = resume.recv_timeout(Duration::from_secs(10));
                        });
                        Ok(())
                    })
                } else {
                    driver.observe_contention_for_testing(contended);
                    driver.set_projection_hook_for_testing(move || {
                        let _ = entered.send(());
                        let _ = resume.recv_timeout(Duration::from_secs(10));
                    });
                    scope.spawn(move || policy.input(CloudPolicyEvent::Foreground).map(|_| ()))
                };
                let entry = reached.recv_timeout(Duration::from_secs(5));
                let (sent, received) = mpsc::channel();
                let selected = execution.clone();
                let starter = scope.spawn(move || {
                    let _ = sent.send(admit(selected, retire));
                });
                let admitted = received.recv_timeout(Duration::from_secs(5));
                // A positive failed try_lock proves this is an executing worker,
                // rather than cancellation of a closure still queued in the pool.
                let contention = observed.recv_timeout(Duration::from_secs(5));
                let cancellation = admitted
                    .as_ref()
                    .ok()
                    .and_then(|job| job.as_ref().ok())
                    .map(|job| job.handle().cancel.cancel());
                drop(release);
                let held = held.join();
                let starter = starter.join();
                (entry, contention, admitted, cancellation, held, starter)
            });
            let job = admitted.expect("start returned before lock release").unwrap();
            let outcome = finished(&events, job.handle().id);
            let value = job.result();
            let after = runtime.adapter_guard();
            let original_valid = original.as_ref().map(|sdk| runtime.validate_sdk(sdk));
            // Retrying the same capability must still reach its original effect.
            let retry = admit(execution, retire).unwrap();
            let retried = finished(&events, retry.handle().id);
            fixture.workflow.host.close().unwrap();
            entry.expect("real lock held");
            contention.expect("worker reached held lock");
            cancellation.unwrap().unwrap();
            held.unwrap().unwrap();
            starter.unwrap();
            assert_eq!(outcome, JobOutcome::Cancelled);
            assert_eq!(value, Err(CoreError::Cancelled));
            assert_eq!(after, before);
            if let Some(valid) = original_valid {
                valid.unwrap();
            }
            assert_eq!(retried, JobOutcome::Succeeded);
            assert_eq!(retry.result(), Ok(Some(())));
        }
    }
}

#[test]
fn native_admission_jobs_replacement_after_entry_flush_rejects_waiting_generation() {
    for retire in [false, true] {
        let (fixture, _, execution, original) = admission_fixture(retire);
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let runtime = fixture.workflow.core.clone();
        let before = runtime.adapter_guard();
        let old_generation = engine.generation();
        let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
        // Entry has finished all participant flushes before this worker starts.
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        let (advanced, generation_reached) = mpsc::channel();
        let (continue_writer, writer_resume) = mpsc::channel();
        let runtime_state = Arc::new(Mutex::new((false, false)));
        let writer_state = Arc::new(Mutex::new((false, false)));
        let writer_held = writer_state.clone();
        let generation_source = engine.clone();
        // Replacement's first day check commits after the real generation
        // advance. Pause before its observers and the Runtime reopen listener.
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
            admitted,
            advanced,
            held_at_advance,
            value,
            after,
            original_valid,
            held_at_result,
            holder,
            starter,
            replacement,
        ) = std::thread::scope(|scope| {
            let release = Release(release);
            let continue_writer = Release(continue_writer);
            let held_runtime = runtime.clone();
            let held_state = runtime_state.clone();
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
                let _ = sent.send(admit(execution, retire));
            });
            let admitted = received.recv_timeout(Duration::from_secs(5));
            // Only the worker can reach Runtime here; replacement has not begun.
            let contention = observed.recv_timeout(Duration::from_secs(5));
            let replacement = scope.spawn(move || exclusive.replace(|_| Ok(())).map(|_| ()));
            let advanced = generation_reached.recv_timeout(Duration::from_secs(5));
            let held_at_advance = *runtime_state.lock().unwrap();
            drop(release);
            let holder = holder.join();
            // The writer's reserved event precedes JobFinished. Inspect the
            // retained cell while the old Runtime is still bound instead.
            let value = if let Ok(Ok(job)) = &admitted {
                let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
                loop {
                    let value = job.result();
                    if !matches!(value, Ok(None)) || meguri_engine::monotonic::now() >= deadline {
                        break Some(value);
                    }
                    std::thread::yield_now();
                }
            } else {
                None
            };
            let after = runtime.adapter_guard();
            let original_valid = original.as_ref().map(|sdk| runtime.validate_sdk(sdk));
            let held_at_result = *writer_state.lock().unwrap();
            drop(continue_writer);
            let replacement = replacement.join();
            let starter = starter.join();
            (
                entry,
                contention,
                admitted,
                advanced,
                held_at_advance,
                value,
                after,
                original_valid,
                held_at_result,
                holder,
                starter,
                replacement,
            )
        });
        drop(slot);
        let closed = fixture.workflow.host.close();
        let job = admitted.expect("start returned while Runtime was held").unwrap();
        let outcome = finished(&events, job.handle().id);
        // All locks and threads are released and the Host is closed before assertions.
        closed.unwrap();
        holder.unwrap();
        starter.unwrap();
        replacement.unwrap().unwrap();
        entry.expect("Runtime held after Exclusive entry flush");
        contention.expect("worker passed Driver acquisition and waited for Runtime");
        assert!(advanced.expect("replacement first day check") > old_generation);
        assert_eq!(held_at_advance, (true, false));
        assert_eq!(held_at_result, (true, false));
        assert!(!runtime_state.lock().unwrap().1 && !writer_state.lock().unwrap().1);
        assert_eq!(after, before);
        if let Some(valid) = original_valid {
            valid.unwrap();
        }
        assert!(matches!(outcome, JobOutcome::Failed { .. }));
        assert_eq!(value, Some(Err(policy_failure(CloudPolicyFailure::GenerationChanged))));
        assert_eq!(job.result(), Err(policy_failure(CloudPolicyFailure::GenerationChanged)));
    }
}

#[test]
fn native_admission_jobs_queued_cancel_and_generation_change_keep_original_typed_results() {
    for retire in [false, true] {
        for replace in [false, true] {
            let (fixture, _, execution, _) = admission_fixture(retire);
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let pool = hold_pool(&fixture);
            let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
            let job = admit(execution.clone(), retire).unwrap();
            let pending = job.result();
            if replace {
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive.replace(|_| Ok(())).unwrap();
            } else {
                job.handle().cancel.cancel().unwrap();
            }
            drop(slot);
            drop(pool);
            let outcome = finished(&events, job.handle().id);
            fixture.workflow.host.close().unwrap();
            // A later owner never substitutes Closed for the original result.
            let replacement = super::fixture();
            let value = job.result();
            let closed_start = admit(execution, retire);
            replacement.workflow.host.close().unwrap();
            assert_eq!(pending, Ok(None));
            assert!(matches!(
                closed_start,
                Err(CoreError::CloudPolicy {
                    failure: CloudPolicyFailure::Closed
                })
            ));
            if replace {
                assert!(matches!(outcome, JobOutcome::Failed { .. }));
                assert_eq!(value, Err(policy_failure(CloudPolicyFailure::GenerationChanged)));
            } else {
                assert_eq!(outcome, JobOutcome::Cancelled);
                assert_eq!(value, Err(CoreError::Cancelled));
            }
        }
    }
}

#[test]
fn native_admission_jobs_original_close_cancels_and_drains_queued_work() {
    for retire in [false, true] {
        let (fixture, _, execution, _) = admission_fixture(retire);
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let pool = hold_pool(&fixture);
        let job = admit(execution, retire).unwrap();
        let (cancelled, closed) = std::thread::scope(|scope| {
            let host = fixture.workflow.host.clone();
            let closing = scope.spawn(move || host.close());
            let deadline = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while !job.handle().cancel.requested() && meguri_engine::monotonic::now() < deadline {
                std::thread::yield_now();
            }
            let cancelled = job.handle().cancel.requested();
            // Release the pool before joining close, including timeout failures.
            drop(pool);
            (cancelled, closing.join())
        });
        let outcome = finished(&events, job.handle().id);
        let drained = engine.jobs().running().is_empty();
        let replacement = super::fixture();
        let retained = job.result();
        replacement.workflow.host.close().unwrap();
        closed.unwrap().unwrap();
        assert!(cancelled && drained);
        assert_eq!(outcome, JobOutcome::Cancelled);
        assert_eq!(retained, Err(CoreError::Cancelled));
    }
}

#[test]
fn native_admission_jobs_register_once_and_retire_original_without_touching_successor() {
    let (fixture, _, execution, original) = admission_fixture(true);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let retired = execution.clone().start_retire_original().unwrap();
    assert_eq!(finished(&events, retired.handle.id), JobOutcome::Succeeded);
    assert_eq!(retired.result.result(), Ok(Some(true)));
    let before_release = execution.clone().start_register_sdk().unwrap();
    assert!(matches!(
        finished(&events, before_release.handle.id),
        JobOutcome::Failed { .. }
    ));
    let expected_before_release = before_release.result.result().err().unwrap();
    fixture.workflow.release_sdk(original.unwrap()).unwrap();
    let registered = execution.clone().start_register_sdk().unwrap();
    assert_eq!(finished(&events, registered.handle.id), JobOutcome::Succeeded);
    let registration = registered.result.result().unwrap().unwrap();
    let duplicate = execution.clone().start_register_sdk().unwrap();
    assert!(matches!(
        finished(&events, duplicate.handle.id),
        JobOutcome::Failed { .. }
    ));
    let successor_retire = execution.start_retire_original().unwrap();
    assert!(matches!(
        finished(&events, successor_retire.handle.id),
        JobOutcome::Failed { .. }
    ));
    fixture.workflow.validate_sdk(registration.instance_id.clone()).unwrap();
    fixture.workflow.host.close().unwrap();
    let replacement = super::fixture();
    let retained = registered.result.result().unwrap().unwrap();
    assert_eq!(retained.instance_id, registration.instance_id);
    assert_eq!(retained.serialization, registration.serialization);
    assert_eq!(retained.collection_zone, registration.collection_zone);
    assert_eq!(retained.needs_root_fetch, registration.needs_root_fetch);
    assert_eq!(
        duplicate.result.result().err(),
        Some(policy_failure(CloudPolicyFailure::StaleCommand))
    );
    assert_eq!(
        successor_retire.result.result(),
        Err(policy_failure(CloudPolicyFailure::StaleCommand))
    );
    assert_eq!(before_release.result.result().err(), Some(expected_before_release));
    assert_eq!(retired.result.result(), Ok(Some(true)));
    replacement.workflow.host.close().unwrap();
}

static REGISTER_CANCEL: Mutex<Option<Arc<crate::cancel::CancelToken>>> = Mutex::new(None);
fn cancel_after_registration_effect() {
    if let Some(cancel) = REGISTER_CANCEL.lock().unwrap().take() {
        cancel.trigger();
    }
}
struct RegistrationFault;
impl Drop for RegistrationFault {
    fn drop(&mut self) {
        meguri_model::faultpoint::disarm("cloud.adapter.register_sdk.before_pending");
        REGISTER_CANCEL.lock().unwrap().take();
    }
}
#[test]
fn native_admission_job_post_effect_cancel_keeps_exact_registration_after_close() {
    let (fixture, _, execution) = start();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let fault = RegistrationFault;
    meguri_model::faultpoint::arm(
        "cloud.adapter.register_sdk.before_pending",
        meguri_model::faultpoint::Fault::Callback(cancel_after_registration_effect),
    );
    let registered = execution.clone().start_register_sdk().unwrap();
    *REGISTER_CANCEL.lock().unwrap() = Some(registered.handle.cancel.clone());
    drop(pool);
    let outcome = finished(&events, registered.handle.id);
    drop(fault);
    let registration = registered.result.result().unwrap().unwrap();
    fixture.workflow.validate_sdk(registration.instance_id.clone()).unwrap();
    fixture.workflow.host.close().unwrap();
    assert!(
        registered.handle.cancel.requested(),
        "positive post-install cancellation"
    );
    assert_eq!(outcome, JobOutcome::Succeeded);
    let retained = registered.result.result().unwrap().unwrap();
    assert_eq!(retained.instance_id, registration.instance_id);
    assert_eq!(retained.serialization, registration.serialization);
    assert_eq!(retained.collection_zone, registration.collection_zone);
    assert_eq!(retained.needs_root_fetch, registration.needs_root_fetch);
}

fn epoch_loss_fixture() -> (Fixture, Arc<CloudPolicyExecution>, String) {
    let (fixture, policy, execution) = start();
    let registration = execution.register_sdk().unwrap();
    let sdk = registration.instance_id;
    let runtime = &fixture.workflow.core;
    let collection = fixture.workflow.host.collection().unwrap();
    runtime
        .adapter_account(
            Some(&sdk),
            r::IdentityStatus::Available(c::OpaqueBytes(b"synthetic-account-A".to_vec())),
        )
        .unwrap();
    let fetch = runtime
        .adapter_begin_fetch(&sdk, vec![registration.collection_zone])
        .unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    let mut reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert_eq!(reads.len(), 1);
    let followup = runtime
        .adapter_complete_fetch_read(
            &reads.remove(0),
            &collection,
            c::publication::RecordRead::Missing,
            false,
            None,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(followup.is_active());
    runtime.validate_sdk(&sdk).unwrap();
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let command = commands(policy.clone().input(settings(true, false)).unwrap())
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::ConfigureAutomatic { on: true }))
        .unwrap();
    let execution = policy.prepare_execution(command.handle).unwrap().unwrap();
    (fixture, execution, sdk)
}

#[test]
fn native_admission_job_absent_epoch_cancel_preserves_retry_authority() {
    let (fixture, execution, sdk) = epoch_loss_fixture();
    let runtime = fixture.workflow.core.clone();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let token = cancel.clone();
    let saw = observed.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::SDKRetireBeforeReacquisition {
            saw.store(true, std::sync::atomic::Ordering::SeqCst);
            token.lock().unwrap().as_ref().unwrap().trigger();
        }
    });
    let retired = execution.clone().start_retire_original().unwrap();
    *cancel.lock().unwrap() = Some(retired.handle.cancel.clone());
    drop(pool);
    let outcome = finished(&events, retired.handle.id);
    let usable = runtime.status().unwrap().stored_usable;
    let original = runtime.validate_native_sdk_for_testing(&sdk);
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    let retry = execution.start_retire_original().unwrap();
    let retried = finished(&events, retry.handle.id);
    let isolated = !runtime.status().unwrap().stored_usable;
    fixture.workflow.host.close().unwrap();
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert_eq!(retired.result.result(), Err(CoreError::Cancelled));
    assert!(usable);
    original.unwrap();
    assert_eq!(retried, JobOutcome::Succeeded);
    assert_eq!(retry.result.result(), Ok(Some(true)));
    assert!(isolated, "retry retains the original pending missing collection");
}

#[test]
fn native_admission_job_absent_epoch_isolation_retains_terminal_facts() {
    for fail_readback in [false, true] {
        let (fixture, execution, sdk) = epoch_loss_fixture();
        let runtime = fixture.workflow.core.clone();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let token = cancel.clone();
        let saw = observed.clone();
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == c::testing::IncomingStage::SDKIsolationPublished {
                saw.store(true, std::sync::atomic::Ordering::SeqCst);
                token.lock().unwrap().as_ref().unwrap().trigger();
            }
        });
        if fail_readback {
            runtime.fail_retire_readback_for_testing();
        }
        let retired = execution.start_retire_original().unwrap();
        *cancel.lock().unwrap() = Some(retired.handle.cancel.clone());
        drop(pool);
        let outcome = finished(&events, retired.handle.id);
        let unusable = !runtime.status().unwrap().stored_usable;
        let authority = runtime.validate_native_sdk_for_testing(&sdk);
        fixture.workflow.host.close().unwrap();
        let replacement = super::fixture();
        let value = retired.result.result();
        replacement.workflow.host.close().unwrap();
        assert!(observed.load(std::sync::atomic::Ordering::SeqCst) && unusable);
        if fail_readback {
            assert!(matches!(authority, Err(r::RuntimeError::IdentityUnverified)));
            assert!(matches!(outcome, JobOutcome::Failed { .. }));
            assert_eq!(
                value,
                Err(CoreError::CloudConnection {
                    failure: crate::cloud_connection::CloudConnectionFailure::Tracking,
                })
            );
        } else {
            assert!(matches!(authority, Err(r::RuntimeError::StaleIncarnation)));
            assert_eq!(outcome, JobOutcome::Succeeded);
            assert_eq!(value, Ok(Some(true)));
        }
    }
}
