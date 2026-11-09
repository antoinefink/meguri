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

#[derive(Clone, Copy, Debug)]
enum AdmissionKind {
    Register,
    Retire,
    Media,
    Fetch,
    Send,
    NoCall,
    Invocation,
}
const ADMISSIONS: [AdmissionKind; 7] = [
    AdmissionKind::Register,
    AdmissionKind::Retire,
    AdmissionKind::Media,
    AdmissionKind::Fetch,
    AdmissionKind::Send,
    AdmissionKind::NoCall,
    AdmissionKind::Invocation,
];
#[derive(Debug, PartialEq, Eq)]
enum AdmissionValue {
    Done,
    Media(bool),
    Fetch(u64),
    Send(Option<u64>),
}
enum AdmissionJob {
    Register(CloudPolicyRegistrationJob),
    Retire(CloudPolicyRetirementJob),
    Media(CloudPolicyMediaJob),
    Fetch(CloudPolicyFetchJob),
    Send(CloudPolicySendJob),
    Invocation(CloudPolicyOutputJob),
}
impl AdmissionJob {
    fn handle(&self) -> &crate::jobs::JobHandle {
        match self {
            Self::Register(job) => &job.handle,
            Self::Retire(job) => &job.handle,
            Self::Media(job) => &job.handle,
            Self::Fetch(job) => &job.handle,
            Self::Send(job) => &job.handle,
            Self::Invocation(job) => &job.handle,
        }
    }
    fn value(&self) -> Result<Option<AdmissionValue>, CoreError> {
        match self {
            Self::Register(job) => job.result.result().map(|value| value.map(|_| AdmissionValue::Done)),
            Self::Retire(job) => job.result.result().map(|value| value.map(|_| AdmissionValue::Done)),
            Self::Media(job) => job.result.result().map(|value| value.map(AdmissionValue::Media)),
            Self::Fetch(job) => job.result.result().map(|value| value.map(AdmissionValue::Fetch)),
            Self::Invocation(job) => job.result.result().map(|value| {
                value.map(|output| {
                    let _ = commands(output); // Preserve the actual projection shape.
                    AdmissionValue::Done
                })
            }),
            Self::Send(job) => job
                .result
                .result()
                .map(|value| value.map(|value| AdmissionValue::Send(value.call))),
        }
    }
    fn result(&self) -> Result<Option<()>, CoreError> {
        self.value().map(|value| value.map(|_| ()))
    }
}
fn expected_value(kind: AdmissionKind) -> AdmissionValue {
    match kind {
        AdmissionKind::Register | AdmissionKind::Retire | AdmissionKind::Invocation => AdmissionValue::Done,
        AdmissionKind::Media => AdmissionValue::Media(false),
        AdmissionKind::Fetch => AdmissionValue::Fetch(1),
        AdmissionKind::Send => AdmissionValue::Send(Some(1)),
        AdmissionKind::NoCall => AdmissionValue::Send(None),
    }
}
#[derive(Clone)]
struct AdmissionTarget {
    execution: Arc<CloudPolicyExecution>,
    zones: Vec<String>,
}
fn admit(target: AdmissionTarget, kind: AdmissionKind) -> Result<AdmissionJob, CoreError> {
    let execution = target.execution;
    match kind {
        AdmissionKind::Invocation => execution
            .start_report_invocation(
                vec![CloudPolicyZone::Root, CloudPolicyZone::Collection],
                CloudPolicyOperationKind::Fetch,
            )
            .map(AdmissionJob::Invocation),
        AdmissionKind::Register => execution.start_register_sdk().map(AdmissionJob::Register),
        AdmissionKind::Retire => execution.start_retire_original().map(AdmissionJob::Retire),
        AdmissionKind::Media => execution.start_begin_media().map(AdmissionJob::Media),
        AdmissionKind::Fetch => execution.start_begin_fetch(target.zones).map(AdmissionJob::Fetch),
        AdmissionKind::Send | AdmissionKind::NoCall => execution.start_begin_send().map(AdmissionJob::Send),
    }
}
fn admission_fixture(kind: AdmissionKind) -> (Fixture, Arc<CloudPolicy>, AdmissionTarget, Option<String>) {
    let (fixture, policy, execution, root) = start_with_root();
    if matches!(kind, AdmissionKind::Register) {
        return (
            fixture,
            policy,
            AdmissionTarget {
                execution,
                zones: Vec::new(),
            },
            None,
        );
    }
    let original = execution.register_sdk().unwrap();
    if matches!(kind, AdmissionKind::Send | AdmissionKind::NoCall) {
        fixture
            .workflow
            .core
            .accept_root_fetch(&original.instance_id, Some(&root.saved()), true, 1)
            .unwrap();
    }
    policy
        .clone()
        .input(CloudPolicyEvent::Refresh {
            cause: CloudPolicyChangeCause::Cloud,
        })
        .unwrap();
    let command = if matches!(kind, AdmissionKind::Retire) {
        commands(policy.clone().input(settings(true, false)).unwrap())
            .into_iter()
            .find(|c| matches!(c.action, CloudPolicyAction::ConfigureAutomatic { on: true }))
            .unwrap()
    } else if matches!(kind, AdmissionKind::Media) {
        policy
            .clone()
            .input(CloudPolicyEvent::Settings {
                settings: CloudPolicyDeviceSettings {
                    enabled: true,
                    paused: false,
                    automatic: false,
                    media: true,
                },
            })
            .unwrap();
        commands(
            policy
                .clone()
                .input(CloudPolicyEvent::VisibleMedia {
                    names: vec!["VISIBLE.PNG".into()],
                })
                .unwrap(),
        )
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::PrioritizeMedia { .. }))
        .unwrap()
    } else {
        commands(
            policy
                .clone()
                .input(CloudPolicyEvent::SyncNowRequested {
                    source: CloudPolicySource::Command,
                })
                .unwrap(),
        )
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::RunExchange { .. }))
        .unwrap()
    };
    if matches!(kind, AdmissionKind::NoCall) {
        policy.clone().capture_exchange(command.handle.clone()).unwrap();
    }
    let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
    let target = AdmissionTarget {
        execution,
        zones: vec!["meguri-root".into(), original.collection_zone],
    };
    (fixture, policy, target, Some(original.instance_id))
}

#[test]
fn native_admission_jobs_start_without_slot_while_driver_projection_and_review_run() {
    for kind in ADMISSIONS {
        for runtime_hold in [false, true] {
            let (fixture, policy, execution, _) = admission_fixture(kind);
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
            let runtime = fixture.workflow.core.clone();
            let hold = move || {
                *held_state.lock().unwrap() = (true, false);
                let _ = entered.send(());
                let watchdog_released = resume.recv_timeout(Duration::from_secs(10)).is_err();
                *held_state.lock().unwrap() = (false, watchdog_released);
            };
            let (entry, admitted, registered, pending, slot, review, at_review_completion, projection, starter) =
                std::thread::scope(|scope| {
                    let release = Release(release);
                    let held = if runtime_hold {
                        scope.spawn(move || {
                            runtime.with_state_held_for_testing(hold);
                            Ok(CloudPolicyOutput::Commands { commands: Vec::new() })
                        })
                    } else {
                        driver.set_projection_hook_for_testing(hold);
                        scope.spawn(move || policy.input(CloudPolicyEvent::Foreground))
                    };
                    let entry = reached.recv_timeout(Duration::from_secs(5));
                    let (sent, received) = mpsc::channel();
                    let starter = scope.spawn(move || {
                        let _ = sent.send(admit(execution, kind));
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
                                    host.review().end_controlled(
                                        session_id,
                                        job.cancel.clone(),
                                        job.progress.clone(),
                                    )?;
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
            assert_eq!(job.value(), Ok(Some(expected_value(kind))));
        }
    }
}

#[test]
fn native_admission_jobs_cancel_after_real_driver_and_runtime_contention_before_effects() {
    for kind in ADMISSIONS {
        for runtime_hold in [false, true] {
            let (fixture, policy, execution, original) = admission_fixture(kind);
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let runtime = fixture.workflow.core.clone();
            let before = runtime.adapter_guard();
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let (contended, observed) = mpsc::channel();
            let hold_state = Arc::new(Mutex::new((false, false)));
            let state = hold_state.clone();
            let (entry, contention, admitted, cancellation, held, starter) = std::thread::scope(|scope| {
                let release = Release(release);
                let held = if runtime_hold {
                    runtime.observe_state_contention_for_testing(contended);
                    let runtime = runtime.clone();
                    scope.spawn(move || {
                        runtime.with_state_held_for_testing(|| {
                            *state.lock().unwrap() = (true, false);
                            let _ = entered.send(());
                            let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
                            *state.lock().unwrap() = (false, timed_out);
                        });
                        Ok(())
                    })
                } else {
                    driver.observe_contention_for_testing(contended);
                    driver.set_projection_hook_for_testing(move || {
                        *state.lock().unwrap() = (true, false);
                        let _ = entered.send(());
                        let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
                        *state.lock().unwrap() = (false, timed_out);
                    });
                    scope.spawn(move || policy.input(CloudPolicyEvent::Foreground).map(|_| ()))
                };
                let entry = reached.recv_timeout(Duration::from_secs(5));
                let (sent, received) = mpsc::channel();
                let selected = execution.clone();
                let starter = scope.spawn(move || {
                    let _ = sent.send(admit(selected, kind));
                });
                let admitted = received.recv_timeout(Duration::from_secs(5));
                // A positive failed try_lock proves this is an executing worker,
                // rather than cancellation of a closure still queued in the pool.
                let contention = observed.recv_timeout(Duration::from_secs(5));
                let held_at_contention = *hold_state.lock().unwrap();
                let cancellation = admitted
                    .as_ref()
                    .ok()
                    .and_then(|job| job.as_ref().ok())
                    .map(|job| job.handle().cancel.cancel());
                drop(release);
                let held = held.join();
                let starter = starter.join();
                (
                    entry,
                    contention.map(|()| held_at_contention),
                    admitted,
                    cancellation,
                    held,
                    starter,
                )
            });
            let job = admitted.expect("start returned before lock release").unwrap();
            let outcome = finished(&events, job.handle().id);
            let value = job.result();
            let after = runtime.adapter_guard();
            let original_valid = original.as_ref().map(|sdk| runtime.validate_sdk(sdk));
            // Retrying the same capability must still reach its original effect.
            let retry = admit(execution, kind).unwrap();
            let retried = finished(&events, retry.handle().id);
            fixture.workflow.host.close().unwrap();
            entry.expect("real lock held");
            assert_eq!(contention.expect("worker reached held lock"), (true, false));
            assert!(
                !hold_state.lock().unwrap().1,
                "lock watchdog did not release the worker"
            );
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
            assert_eq!(retry.value(), Ok(Some(expected_value(kind))));
        }
    }
}

#[test]
fn native_admission_jobs_replacement_after_entry_flush_rejects_waiting_generation() {
    for kind in ADMISSIONS {
        let (fixture, _, execution, original) = admission_fixture(kind);
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
                let _ = sent.send(admit(execution, kind));
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
    for kind in ADMISSIONS {
        for replace in [false, true] {
            let (fixture, _, execution, _) = admission_fixture(kind);
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let pool = hold_pool(&fixture);
            let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
            let job = admit(execution.clone(), kind).unwrap();
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
            let retry = if replace {
                None
            } else {
                let retry = admit(execution.clone(), kind).unwrap();
                let outcome = finished(&events, retry.handle().id);
                Some((outcome, retry.value()))
            };
            fixture.workflow.host.close().unwrap();
            // A later owner never substitutes Closed for the original result.
            let replacement = super::fixture();
            let value = job.result();
            let closed_start = admit(execution, kind);
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
                assert_eq!(retry, Some((JobOutcome::Succeeded, Ok(Some(expected_value(kind))))));
            }
        }
    }
}

#[test]
fn native_admission_jobs_original_close_cancels_and_drains_queued_work() {
    for kind in ADMISSIONS {
        let (fixture, _, execution, _) = admission_fixture(kind);
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let pool = hold_pool(&fixture);
        let job = admit(execution, kind).unwrap();
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
    let (fixture, _, target, original) = admission_fixture(AdmissionKind::Retire);
    let execution = target.execution;
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

#[test]
fn native_admission_jobs_post_effect_cancel_keeps_media_fetch_and_send_values_after_close() {
    use c::testing::IncomingStage as Stage;
    for (kind, stage) in [
        (AdmissionKind::Media, Stage::PolicyMediaPrepared),
        (AdmissionKind::Media, Stage::PolicyMediaSelected),
        (AdmissionKind::Fetch, Stage::PolicyFetchBegun),
        (AdmissionKind::Send, Stage::PolicySendRetryCleared),
    ] {
        let (fixture, _, target, original) = admission_fixture(kind);
        let runtime = fixture.workflow.core.clone();
        if stage == Stage::PolicyMediaSelected {
            runtime.prepare_cloud_media_sdk(original.as_deref().unwrap()).unwrap();
        }
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let token = cancel.clone();
        let saw = observed.clone();
        runtime.set_incoming_stage_hook_for_testing(move |hit| {
            if hit == stage {
                saw.store(true, std::sync::atomic::Ordering::SeqCst);
                token.lock().unwrap().as_ref().unwrap().trigger();
            }
        });
        let job = admit(target, kind).unwrap();
        *cancel.lock().unwrap() = Some(job.handle().cancel.clone());
        drop(pool);
        let outcome = finished(&events, job.handle().id);
        runtime.set_incoming_stage_hook_for_testing(|_| {});
        fixture.workflow.host.close().unwrap();
        let replacement = super::fixture();
        let value = job.value();
        replacement.workflow.host.close().unwrap();
        assert!(observed.load(std::sync::atomic::Ordering::SeqCst) && job.handle().cancel.requested());
        assert_eq!(outcome, JobOutcome::Succeeded);
        assert_eq!(value, Ok(Some(expected_value(kind))));
    }
}

#[test]
fn native_admission_job_media_preparation_retains_actual_guard_error_after_cancel_and_close() {
    let (fixture, _, target, _) = admission_fixture(AdmissionKind::Media);
    let runtime = fixture.workflow.core.clone();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
    let token = cancel.clone();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw = observed.clone();
    let changed = runtime.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::PolicyMediaPrepared {
            token.lock().unwrap().as_ref().unwrap().trigger();
            // Preparation has released State; change the actual backend guard.
            changed.adapter_pause(true).unwrap();
            saw.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
    let job = admit(target, AdmissionKind::Media).unwrap();
    *cancel.lock().unwrap() = Some(job.handle().cancel.clone());
    drop(pool);
    let outcome = finished(&events, job.handle().id);
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    fixture.workflow.host.close().unwrap();
    let replacement = super::fixture();
    let value = job.result();
    replacement.workflow.host.close().unwrap();
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst) && job.handle().cancel.requested());
    assert!(matches!(outcome, JobOutcome::Failed { .. }));
    assert_eq!(
        value,
        Err(CoreError::CloudConnection {
            failure: crate::cloud_connection::CloudConnectionFailure::AuthorityChanged,
        })
    );
}

#[test]
fn native_admission_job_send_limit_clears_retrying_and_retains_completed_no_call() {
    let (fixture, _, target, original) = admission_fixture(AdmissionKind::NoCall);
    let runtime = fixture.workflow.core.clone();
    let sdk = original.unwrap();
    let seen = runtime
        .adapter_snapshot()
        .unwrap()
        .entities
        .into_iter()
        .map(|entity| entity.name)
        .collect();
    runtime
        .exchange_fetch_completed(&sdk, w::FetchPhase::BeforeSend, &seen)
        .unwrap();
    let issued: Vec<_> = (0..8)
        .map(|_| runtime.start_manual_send_call(&sdk).unwrap().unwrap().id)
        .collect();
    runtime.adapter_sdk_retrying(&sdk, true).unwrap();
    let was_retrying = runtime.sdk_retrying_for_testing();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
    let token = cancel.clone();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw = observed.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::PolicySendRetryCleared {
            saw.store(true, std::sync::atomic::Ordering::SeqCst);
            token.lock().unwrap().as_ref().unwrap().trigger();
        }
    });
    let job = admit(target, AdmissionKind::NoCall).unwrap();
    let pending = job.value();
    *cancel.lock().unwrap() = Some(job.handle().cancel.clone());
    drop(pool);
    let outcome = finished(&events, job.handle().id);
    let retrying = runtime.sdk_retrying_for_testing();
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    fixture.workflow.host.close().unwrap();
    assert_eq!(issued, (1..=8).collect::<Vec<_>>());
    assert_eq!(pending, Ok(None));
    assert!(was_retrying && !retrying && observed.load(std::sync::atomic::Ordering::SeqCst));
    assert!(job.handle().cancel.requested());
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(job.value(), Ok(Some(AdmissionValue::Send(None))));
}

#[test]
fn native_admission_job_resume_media_send_retains_pending_effect_through_later_state_acquisition() {
    let (fixture, policy, _, original) = admission_fixture(AdmissionKind::Send);
    let sdk = original.unwrap();
    let command = commands(
        policy
            .clone()
            .input(CloudPolicyEvent::Settings {
                settings: CloudPolicyDeviceSettings {
                    enabled: true,
                    paused: false,
                    automatic: true,
                    media: true,
                },
            })
            .unwrap(),
    )
    .into_iter()
    .find(|command| matches!(command.action, CloudPolicyAction::ResumeMedia))
    .unwrap();
    let execution = policy.prepare_execution(command.handle).unwrap().unwrap();
    let runtime = fixture.workflow.core.clone();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let media = execution.clone().start_begin_media().unwrap();
    let media_outcome = finished(&events, media.handle.id);
    let _ = fixture.workflow.take_pending_updates(sdk.clone()).unwrap();
    let pool = hold_pool(&fixture);
    let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
    let token = cancel.clone();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw = observed.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::PolicyPendingUpdated {
            saw.store(true, std::sync::atomic::Ordering::SeqCst);
            token.lock().unwrap().as_ref().unwrap().trigger();
        }
    });
    let send = execution.start_begin_send().unwrap();
    *cancel.lock().unwrap() = Some(send.handle.cancel.clone());
    drop(pool);
    let send_outcome = finished(&events, send.handle.id);
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    let pending = fixture.workflow.take_pending_updates(sdk).unwrap();
    fixture.workflow.host.close().unwrap();
    assert_eq!(media_outcome, JobOutcome::Succeeded);
    assert_eq!(media.result.result(), Ok(Some(true)));
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst) && send.handle.cancel.requested());
    assert!(!pending.is_empty());
    assert_eq!(send_outcome, JobOutcome::Succeeded);
    assert_eq!(send.result.result(), Ok(Some(CloudPolicySendCall { call: Some(1) })));
}

#[test]
fn native_admission_jobs_keep_exact_action_and_scope_errors_after_close() {
    for kind in [AdmissionKind::Media, AdmissionKind::Fetch, AdmissionKind::Send] {
        let (fixture, _, execution) = start();
        execution.register_sdk().unwrap();
        let expected = match kind {
            AdmissionKind::Media => execution.begin_media().unwrap_err(),
            AdmissionKind::Fetch => execution.begin_fetch(Vec::new()).unwrap_err(),
            AdmissionKind::Send => execution.begin_send().unwrap_err(),
            _ => unreachable!(),
        };
        let events = fixture.workflow.host.engine().stream().subscribe();
        let job = admit(
            AdmissionTarget {
                execution,
                zones: Vec::new(),
            },
            kind,
        )
        .unwrap();
        let outcome = finished(&events, job.handle().id);
        fixture.workflow.host.close().unwrap();
        let replacement = super::fixture();
        let value = job.result();
        replacement.workflow.host.close().unwrap();
        assert!(matches!(outcome, JobOutcome::Failed { .. }));
        assert_eq!(value, Err(expected));
    }
}

// Full Started projections, beyond the validation-only responsiveness proof.
#[test]
fn native_admission_jobs_started_projection_cancel_before_first_effect_leaves_cycles_unchanged() {
    let (fixture, _, target, _) = admission_fixture(AdmissionKind::Invocation);
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let before = driver.state();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let held = Arc::new(Mutex::new((false, false)));
    let observation = held.clone();
    driver.set_projection_hook_for_testing(move || {
        *observation.lock().unwrap() = (true, false);
        let _ = entered.send(());
        let timed_out = resume.recv_timeout(Duration::from_secs(10)).is_err();
        *observation.lock().unwrap() = (false, timed_out);
    });
    let release = Release(release);
    let job = admit(target.clone(), AdmissionKind::Invocation).unwrap();
    let entry = reached.recv_timeout(Duration::from_secs(5));
    let at_cancel = *held.lock().unwrap();
    job.handle().cancel.cancel().unwrap();
    drop(release);
    let outcome = finished(&events, job.handle().id);
    let after = driver.state();
    let retry = admit(target, AdmissionKind::Invocation).unwrap();
    let retry_outcome = finished(&events, retry.handle().id);
    fixture.workflow.host.close().unwrap();
    entry.expect("full Started projection reached");
    assert_eq!(at_cancel, (true, false));
    assert!(!held.lock().unwrap().1, "projection watchdog did not release work");
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert_eq!(job.result(), Err(CoreError::Cancelled));
    assert_eq!(after, before);
    assert_eq!(retry_outcome, JobOutcome::Succeeded);
    assert_eq!(retry.value(), Ok(Some(AdmissionValue::Done)));
}

fn invocation_snapshot(output: Result<CloudPolicyOutput, CoreError>) -> Result<Vec<String>, CoreError> {
    output.map(|output| {
        commands(output)
            .into_iter()
            .map(|command| format!("{:?}", command.action))
            .collect()
    })
}

#[test]
fn native_admission_jobs_started_multizone_keeps_exact_outputs_and_typed_error_after_effect_cancel_close() {
    for retire in [false, true] {
        let mut baseline = None;
        for asynchronous in [false, true] {
            let (fixture, _, target, sdk) = admission_fixture(AdmissionKind::Invocation);
            let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let pool = hold_pool(&fixture);
            let cancel = Arc::new(Mutex::new(None::<Arc<crate::cancel::CancelToken>>));
            let token = cancel.clone();
            let saw = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed = saw.clone();
            let host = fixture.workflow.host.clone();
            let runtime = fixture.workflow.core.clone();
            // The second projection is reached only after the first zone's
            // state publication. Rearm the existing Rust-only one-shot hook.
            driver.set_projection_hook_for_testing(move || {
                host.cloud_policy_driver()
                    .unwrap()
                    .set_projection_hook_for_testing(move || {
                        observed.store(true, std::sync::atomic::Ordering::SeqCst);
                        if let Some(token) = token.lock().unwrap().as_ref() {
                            token.trigger();
                        }
                        if retire {
                            runtime.retire_sdk(sdk.as_deref().unwrap(), false).unwrap();
                        }
                    });
            });
            let zones = vec![
                CloudPolicyZone::Root,
                CloudPolicyZone::Collection,
                CloudPolicyZone::Media,
            ];
            let (job, output, outcome) = if asynchronous {
                let job = target
                    .execution
                    .start_report_invocation(zones, CloudPolicyOperationKind::Fetch)
                    .unwrap();
                *cancel.lock().unwrap() = Some(job.handle.cancel.clone());
                drop(pool);
                let outcome = finished(&events, job.handle.id);
                let output = job.result.result().map(|value| value.expect("completed typed result"));
                (Some(job), output, Some(outcome))
            } else {
                drop(pool);
                (
                    None,
                    target.execution.operation_activity(CloudPolicyOperationFacts {
                        zones,
                        kind: CloudPolicyOperationKind::Fetch,
                        started: true,
                        failed: false,
                        code: None,
                        retry_after_ms: None,
                        saved: 0,
                        app_cancelled: false,
                    }),
                    None,
                )
            };
            let cycles = driver.state().cycles;
            fixture.workflow.host.close().unwrap();
            let snapshot = invocation_snapshot(output);
            assert!(saw.load(std::sync::atomic::Ordering::SeqCst));
            if retire {
                assert!(snapshot.is_err());
            } else {
                assert!(snapshot.as_ref().is_ok_and(|actions| !actions.is_empty()));
                for zone in [p::Zone::Root, p::Zone::Collection, p::Zone::Media] {
                    assert!(
                        cycles
                            .get(&p::Operation {
                                zone,
                                kind: p::OperationKind::Fetch
                            })
                            .unwrap()
                            .active
                    );
                }
            }
            if let Some(job) = job {
                let outcome = outcome.unwrap();
                assert_eq!(matches!(outcome, JobOutcome::Failed { .. }), retire);
                if !retire {
                    assert_eq!(outcome, JobOutcome::Succeeded);
                }
                assert!(job.handle.cancel.requested());
                let replacement = super::fixture();
                let reread = invocation_snapshot(job.result.result().map(|value| value.unwrap()));
                replacement.workflow.host.close().unwrap();
                assert_eq!(reread, snapshot);
                assert_eq!(baseline.as_ref().unwrap(), &(snapshot, cycles));
            } else {
                baseline = Some((snapshot, cycles));
            }
        }
    }
}
