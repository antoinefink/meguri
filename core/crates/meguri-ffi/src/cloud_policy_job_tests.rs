//! engine/jobs J2.1.1, sync/policy P2.1: real retained owners and pool admission.
use super::*;
use meguri_engine::{EventKind, JobKind, JobOutcome, Subscription};
use meguri_model::clock::Clock;
use std::sync::mpsc;

#[test]
fn policy_original_owner_metadata_survives_real_suspend_while_jobs_stay_cancelled() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    let engine = fixture.workflow.host.engine();
    let events = engine.stream().subscribe();
    assert_eq!(
        engine.suspend(Duration::from_secs(5)).unwrap(),
        meguri_engine::SuspendOutcome::Suspended
    );
    policy.validate_original_owner().unwrap();
    let status = policy.clone().start_status().unwrap();
    assert_eq!(finished(&events, status.handle.id), JobOutcome::Cancelled);
    assert!(matches!(status.result.result(), Err(CoreError::Cancelled)));
    let input = policy.clone().start_input(settings(true)).unwrap();
    assert_eq!(finished(&events, input.handle.id), JobOutcome::Cancelled);
    assert!(matches!(input.result.result(), Err(CoreError::Cancelled)));
    policy.validate_original_owner().unwrap();
    fixture.workflow.host.close().unwrap();
    failure_is(policy.validate_original_owner(), CloudPolicyFailure::Closed);
    let (replacement, _) = unbound_fixture(Arc::new(Account));
    failure_is(policy.validate_original_owner(), CloudPolicyFailure::Closed);
    replacement.workflow.host.close().unwrap();
}

#[test]
fn policy_original_owner_metadata_reports_real_generation_replacement() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
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
    failure_is(policy.validate_original_owner(), CloudPolicyFailure::GenerationChanged);
    fixture.workflow.host.close().unwrap();
    failure_is(policy.validate_original_owner(), CloudPolicyFailure::Closed);
}

#[test]
fn policy_original_owner_metadata_completes_while_real_driver_projection_is_held() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
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
    std::thread::scope(|scope| {
        // Drop before scope joins on every failure, including missing entry.
        let release = ReleaseDeckAccount(release);
        let held_policy = policy.clone();
        let held = scope.spawn(move || held_policy.input(CloudPolicyEvent::Foreground));
        reached
            .recv_timeout(Duration::from_secs(5))
            .expect("real projection owns Driver");
        let (validated, completed) = mpsc::channel();
        let validating_policy = policy.clone();
        let validation = scope.spawn(move || {
            let _ = validated.send(validating_policy.validate_original_owner());
        });
        let before_release = completed.recv_timeout(Duration::from_secs(5));
        drop(release);
        let released = did_release.recv_timeout(Duration::from_secs(5));
        held.join().unwrap().unwrap();
        validation.join().unwrap();
        released.expect("projection released");
        before_release.expect("metadata validation waited for Driver").unwrap();
    });
    fixture.workflow.host.close().unwrap();
}

pub(super) fn finished(events: &Subscription, id: u64) -> JobOutcome {
    loop {
        let event = events.recv_timeout(Duration::from_secs(10)).expect("JobFinished");
        if let EventKind::JobFinished { id: got, outcome } = &event.kind
            && got.0 == id
        {
            return outcome.clone();
        }
    }
}

#[derive(Debug)]
struct ProjectionClock {
    pause: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}
impl Clock for ProjectionClock {
    fn now_ms(&self) -> i64 {
        let pause = self.pause.lock().unwrap().take();
        if let Some((entered, release)) = pause {
            entered.send(()).unwrap();
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("projection release");
        }
        NOW
    }
    fn utc_offset_at(&self, _: i64) -> UtcOffset {
        UtcOffset::UTC
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}

#[test]
fn policy_job_starts_admit_without_driver_locks_during_real_projection() {
    let clock = Arc::new(ProjectionClock {
        pause: Mutex::new(None),
    });
    let (fixture, _) = unbound_fixture_with_clock(Arc::new(Account), Some(clock.clone()));
    let policy = fixture.workflow.policy().unwrap();
    let command = commands(policy.clone().input(settings(false)).unwrap())
        .remove(0)
        .handle;
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let release = ReleaseDeckAccount(release);
    *clock.pause.lock().unwrap() = Some((entered, resume));
    let held = policy.clone().start_input(CloudPolicyEvent::Foreground).unwrap();
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    // now_ms is called by project_and_step while the REAL Driver is owned.
    // Every start must return without querying that Driver, Runtime or tracking.
    let input = policy
        .clone()
        .start_input(CloudPolicyEvent::Network { reachable: false })
        .unwrap();
    let status = policy.clone().start_status().unwrap();
    let account = policy.clone().start_refresh_account().unwrap();
    let identity = policy.clone().start_project_identity_read(u64::MAX).unwrap();
    let core = policy.clone().start_execute_core(command.clone()).unwrap();
    let native = policy
        .clone()
        .start_submit_native(command.clone(), Arc::new(Queue(AtomicUsize::new(0))))
        .unwrap();
    let capture = policy.clone().start_capture_exchange(command.clone()).unwrap();
    let complete = policy.clone().start_complete_exchange(u64::MAX).unwrap();
    let end = policy
        .clone()
        .start_end_before_capture(u64::MAX, CloudPolicyEarlyEnd::Cancelled)
        .unwrap();
    let ready = policy
        .clone()
        .start_exchange_readiness(command.clone(), false, false)
        .unwrap();
    let execution = policy.clone().start_prepare_execution(command).unwrap();
    // START admission only. Four pool workers can be blocked here; completion
    // of unrelated work under saturation is deliberately not asserted.
    let unrelated = fixture
        .workflow
        .host
        .engine()
        .start_job(JobKind::BulkOp, |_| Ok(()))
        .unwrap();
    assert!(fixture.workflow.host.engine().slot_holder().is_none());
    drop(release);
    // Joining by the registry snapshot does not consume/discard other events.
    let ids = [
        held.handle.id,
        input.handle.id,
        status.handle.id,
        account.handle.id,
        identity.handle.id,
        core.handle.id,
        native.handle.id,
        capture.handle.id,
        complete.handle.id,
        end.handle.id,
        ready.handle.id,
        execution.handle.id,
        unrelated.id.0,
    ];
    let mut pending: std::collections::BTreeSet<_> = ids.into_iter().collect();
    while !pending.is_empty() {
        let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
        if let EventKind::JobFinished { id, .. } = &event.kind {
            pending.remove(&id.0);
        }
    }
    assert!(held.result.result().unwrap().is_some());
    fixture.workflow.host.close().unwrap();
}

// RED against the supplied facade seed needs ONLY this test plus the Rust-only
// contention observation scaffold on Driver: its field/init, observer setter,
// and a try_lock/WouldBlock notification before handle's original blocking lock.
// Do not carry PolicyWork, any cancellation check, or any facade repair into RED.
#[test]
fn policy_job_cancelled_after_worker_contends_on_driver_preserves_settings_and_state() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    policy.clone().input(settings(false)).unwrap();
    policy.clone().input(CloudPolicyEvent::Foreground).unwrap();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let before = driver.state();
    let runtime_guard = fixture.workflow.core.adapter_guard();
    assert!(!before.settings.paused);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let release = ReleaseDeckAccount(release);
    driver.set_projection_hook_for_testing(move || {
        entered.send(()).unwrap();
        resume
            .recv_timeout(Duration::from_secs(10))
            .expect("projection release");
    });
    let held = policy.clone().start_input(CloudPolicyEvent::Foreground).unwrap();
    reached
        .recv_timeout(Duration::from_secs(5))
        .expect("real projection owns Driver");
    let (contended, observed) = mpsc::channel();
    driver.observe_contention_for_testing(contended);
    let cancelled = policy.clone().start_input(settings(true)).unwrap();
    // This is a positive failed try_lock by this worker, after its entry checks.
    // A job still waiting in the pool cannot send this signal.
    observed
        .recv_timeout(Duration::from_secs(5))
        .expect("worker contends on real Driver");
    assert!(cancelled.result.result().unwrap().is_none());
    cancelled.handle.cancel.cancel().unwrap();
    drop(release);
    let mut outcomes = std::collections::BTreeMap::new();
    while outcomes.len() != 2 {
        let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
        if let EventKind::JobFinished { id, outcome } = &event.kind
            && (id.0 == held.handle.id || id.0 == cancelled.handle.id)
        {
            outcomes.insert(id.0, outcome.clone());
        }
    }
    assert_eq!(outcomes[&held.handle.id], JobOutcome::Succeeded);
    assert_eq!(outcomes[&cancelled.handle.id], JobOutcome::Cancelled);
    assert!(held.result.result().unwrap().is_some());
    assert!(matches!(cancelled.result.result(), Err(CoreError::Cancelled)));
    assert_eq!(driver.state(), before, "cancelled settings input changed policy state");
    assert_eq!(
        fixture.workflow.core.adapter_guard(),
        runtime_guard,
        "cancelled input changed Runtime policy guard"
    );
    assert!(!policy.status().unwrap().controls.user_paused);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_job_cancelled_during_projection_before_first_effect_preserves_state() {
    let clock = Arc::new(ProjectionClock {
        pause: Mutex::new(None),
    });
    let (fixture, _) = unbound_fixture_with_clock(Arc::new(Account), Some(clock.clone()));
    let policy = fixture.workflow.policy().unwrap();
    policy.clone().input(CloudPolicyEvent::Foreground).unwrap();
    let before = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let release = ReleaseDeckAccount(release);
    *clock.pause.lock().unwrap() = Some((entered, resume));
    let started = policy
        .clone()
        .start_input(CloudPolicyEvent::Network { reachable: true })
        .unwrap();
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    started.handle.cancel.cancel().unwrap();
    drop(release);
    assert_eq!(finished(&events, started.handle.id), JobOutcome::Cancelled);
    assert!(matches!(started.result.result(), Err(CoreError::Cancelled)));
    assert_eq!(fixture.workflow.host.cloud_policy_driver().unwrap().state(), before);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_job_bound_idle_poll_remains_cancellable_before_projection_effects() {
    let (fixture, _, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let instance = fixture.workflow.core.instance().unwrap();
    fixture.workflow.core.validate_caller(&instance).unwrap();
    assert!(fixture.workflow.host.review().active_session().is_none());
    // This bound Runtime must pass caller_locked, unlike the unbound clock test.
    assert_eq!(fixture.workflow.core.poll_review(&instance).unwrap(), None);
    let before = driver.state();
    let runtime_guard = fixture.workflow.core.adapter_guard();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let release = ReleaseDeckAccount(release);
    driver.set_projection_hook_for_testing(move || {
        entered.send(()).unwrap();
        resume
            .recv_timeout(Duration::from_secs(10))
            .expect("projection release");
    });
    let started = policy
        .clone()
        .start_input(CloudPolicyEvent::Network { reachable: false })
        .unwrap();
    reached
        .recv_timeout(Duration::from_secs(5))
        .expect("bound idle polling completed");
    started.handle.cancel.cancel().unwrap();
    drop(release);
    assert_eq!(finished(&events, started.handle.id), JobOutcome::Cancelled);
    assert!(matches!(started.result.result(), Err(CoreError::Cancelled)));
    assert_eq!(driver.state(), before);
    assert_eq!(fixture.workflow.core.adapter_guard(), runtime_guard);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_job_cancelled_after_settings_effect_keeps_actual_projection_result() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    policy.clone().input(settings(false)).unwrap();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let release = ReleaseDeckAccount(release);
    driver.set_projection_hook_for_testing(move || {
        entered.send(()).unwrap();
        resume
            .recv_timeout(Duration::from_secs(10))
            .expect("projection release");
    });
    let started = policy.clone().start_input(settings(true)).unwrap();
    // This Driver's one-shot hook follows Settings' Runtime media/pause effects.
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    started.handle.cancel.cancel().unwrap();
    drop(release);
    assert_eq!(finished(&events, started.handle.id), JobOutcome::Succeeded);
    assert!(started.result.result().unwrap().is_some());
    assert!(policy.status().unwrap().controls.user_paused);
    fixture.workflow.host.close().unwrap();
}

// Four positively entered jobs hold the existing pool. No sleep establishes
// queue order, and the release guard works during assertion unwinding too.
fn hold_pool(fixture: &Fixture) -> Vec<ReleaseDeckAccount> {
    (0..4)
        .map(|_| {
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            fixture
                .workflow
                .host
                .engine()
                .start_job(JobKind::BulkOp, move |_| {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(10)).unwrap();
                    Ok(())
                })
                .unwrap();
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            ReleaseDeckAccount(release)
        })
        .collect()
}

#[test]
fn policy_job_queued_cancel_has_exact_cancelled_result_without_effects() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    let before = policy.status().unwrap().controls.user_paused;
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let started = policy.clone().start_input(settings(true)).unwrap();
    assert!(started.result.result().unwrap().is_none());
    started.handle.cancel.cancel().unwrap();
    drop(pool);
    assert_eq!(finished(&events, started.handle.id), JobOutcome::Cancelled);
    assert!(matches!(started.result.result(), Err(CoreError::Cancelled)));
    assert_eq!(policy.status().unwrap().controls.user_paused, before);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn policy_job_typed_errors_and_completed_value_survive_original_close_and_replacement() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let saved = policy.clone().start_status().unwrap();
    assert_eq!(finished(&events, saved.handle.id), JobOutcome::Succeeded);
    let failed = policy.clone().start_project_identity_read(u64::MAX).unwrap();
    assert!(matches!(finished(&events, failed.handle.id), JobOutcome::Failed { .. }));
    let expected = CoreError::CloudPolicy {
        failure: CloudPolicyFailure::StaleCommand,
    };
    assert!(matches!(failed.result.result(), Err(ref error) if error == &expected));
    fixture.workflow.host.close().unwrap();
    let (replacement, _) = unbound_fixture(Arc::new(Account));
    // Neither read validates through the replacement or turns the old error
    // into Closed. Result cells retain values, not a live connection/Host.
    assert!(saved.result.result().unwrap().is_some());
    assert!(matches!(failed.result.result(), Err(ref error) if error == &expected));
    assert!(matches!(
        policy.start_status(),
        Err(CoreError::CloudPolicy {
            failure: CloudPolicyFailure::Closed
        })
    ));
    replacement.workflow.host.close().unwrap();
}

#[test]
fn policy_job_close_cancels_and_drains_queued_original_work() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let started = policy.start_input(settings(true)).unwrap();
    // Close itself must cancel this registered queued work before draining.
    std::thread::scope(|scope| {
        let host = fixture.workflow.host.clone();
        let closing = scope.spawn(move || host.close());
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(3);
        while !started.handle.cancel.is_cancelled().unwrap() {
            assert!(
                meguri_engine::monotonic::now() < deadline,
                "close did not cancel queued policy work"
            );
            std::thread::yield_now();
        }
        drop(pool);
        closing.join().unwrap().unwrap();
    });
    assert_eq!(finished(&events, started.handle.id), JobOutcome::Cancelled);
    assert!(matches!(started.result.result(), Err(CoreError::Cancelled)));
    assert!(fixture.workflow.host.engine().jobs().running().is_empty());
}

#[test]
fn policy_job_revalidates_original_generation_after_queue_delay_without_taking_slot() {
    let (fixture, _) = unbound_fixture(Arc::new(Account));
    let policy = fixture.workflow.policy().unwrap();
    let engine = fixture.workflow.host.engine();
    let events = engine.stream().subscribe();
    let pool = hold_pool(&fixture);
    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    // Ordinary policy jobs are admitted while another operation owns the slot.
    let started = policy.start_status().unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    exclusive
        .replace(|path| {
            assert!(path.exists());
            Ok(())
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    drop(pool);
    assert!(matches!(
        finished(&events, started.handle.id),
        JobOutcome::Failed { .. }
    ));
    assert!(matches!(
        started.result.result(),
        Err(CoreError::CloudPolicy {
            failure: CloudPolicyFailure::GenerationChanged
        })
    ));
    fixture.workflow.host.close().unwrap();
}

struct CancelAfterEnqueue {
    token: Mutex<Option<Arc<crate::cancel::CancelToken>>>,
    calls: AtomicUsize,
}
impl CloudPolicyQueue for CancelAfterEnqueue {
    fn enqueue(&self) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Model cancellation after the native effect. No FFI reentry occurs.
        self.token.lock().unwrap().as_ref().unwrap().trigger();
        true
    }
}
#[test]
fn policy_job_cancellation_after_native_effect_keeps_actual_admission() {
    let (fixture, _, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    let command = run(&policy);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let queue = Arc::new(CancelAfterEnqueue {
        token: Mutex::new(None),
        calls: AtomicUsize::new(0),
    });
    let started = policy.start_submit_native(command.handle, queue.clone()).unwrap();
    *queue.token.lock().unwrap() = Some(started.handle.cancel.clone());
    drop(pool);
    assert_eq!(finished(&events, started.handle.id), JobOutcome::Succeeded);
    assert!(started.handle.cancel.requested());
    assert!(matches!(
        started.result.result(),
        Ok(Some(CloudPolicyAdmission::Admitted { accepted: true }))
    ));
    assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
    fixture.workflow.host.close().unwrap();
}

struct PanicQueue;
impl CloudPolicyQueue for PanicQueue {
    fn enqueue(&self) -> bool {
        panic!("synthetic policy queue panic")
    }
}
#[test]
fn policy_job_panic_has_terminal_failure_and_leaves_pool_and_close_drainable() {
    let (fixture, _, _) = rig();
    let policy = fixture.workflow.policy().unwrap();
    let command = run(&policy);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let started = policy
        .clone()
        .start_submit_native(command.handle, Arc::new(PanicQueue))
        .unwrap();
    assert!(
        matches!(finished(&events, started.handle.id), JobOutcome::Failed { error: meguri_engine::EngineError::Internal { message } } if message.contains("synthetic policy queue panic"))
    );
    // A panic has no typed return; Swift must use its JobFinished failure.
    assert!(started.result.result().unwrap().is_none());
    let next = policy.start_status().unwrap();
    assert_eq!(finished(&events, next.handle.id), JobOutcome::Succeeded);
    fixture.workflow.host.close().unwrap();
    assert!(fixture.workflow.host.engine().jobs().running().is_empty());
}
