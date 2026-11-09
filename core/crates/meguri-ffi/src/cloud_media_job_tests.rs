//! Complete next-work preparation on the original engine's registered pool.
use super::*;
use meguri_engine::{EventKind, JobKind, JobOutcome, Subscription};
use meguri_model::faultpoint::{self, Fault};
use std::sync::mpsc;

struct Release(mpsc::Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
fn terminal(events: &Subscription, id: u64) -> Result<JobOutcome, mpsc::RecvTimeoutError> {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(10))
            .ok_or(mpsc::RecvTimeoutError::Timeout)?;
        if let EventKind::JobFinished { id: got, outcome } = &event.kind
            && got.0 == id
        {
            return Ok(outcome.clone());
        }
    }
}
fn finished(events: &Subscription, id: u64) -> JobOutcome {
    terminal(events, id).expect("terminal event")
}
fn hold_pool(fixture: &Fixture) -> Vec<Release> {
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
                    resume.recv_timeout(Duration::from_secs(20)).unwrap();
                    Ok(())
                })
                .unwrap();
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            Release(release)
        })
        .collect()
}
static SCAN_RELEASED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SCAN_HOLD: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
fn hold_scan() {
    let hold = SCAN_HOLD.lock().unwrap().take();
    if let Some((entered, resume)) = hold {
        let _ = entered.send(());
        let _ = resume.recv_timeout(Duration::from_secs(20));
        SCAN_RELEASED.store(true, Ordering::SeqCst);
    }
}
struct ScanHold {
    _release: Release,
}
impl Drop for ScanHold {
    fn drop(&mut self) {
        faultpoint::disarm("media.scan.after_cutoff");
        SCAN_HOLD.lock().unwrap().take();
    }
}
fn arm_scan() -> (ScanHold, mpsc::Receiver<()>) {
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    SCAN_RELEASED.store(false, Ordering::SeqCst);
    *SCAN_HOLD.lock().unwrap() = Some((entered, resume));
    faultpoint::arm("media.scan.after_cutoff", Fault::Callback(hold_scan));
    (
        ScanHold {
            _release: Release(release),
        },
        reached,
    )
}

#[test]
fn media_next_job_scan_hold_admits_and_completes_real_review_before_release() {
    let (fixture, sdk) = published();
    let collection = fixture.workflow.host.collection().unwrap();
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    let media = fixture.workflow.media().unwrap();
    // Establish the original SDK before arming, so the hold is in the actual
    // next-work feed dependency, rather than an SDK setup test double.
    fixture.workflow.core.prepare_cloud_media_sdk(&sdk.instance_id).unwrap();
    let service = fixture.workflow.host.media().unwrap();
    std::fs::write(service.store().media_dir().join("job-hold.asset"), b"changed folder").unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (hold, reached) = arm_scan();
    let (entry, before_release, registered, pending, admitted, joined) = std::thread::scope(|scope| {
        let (sent, received) = mpsc::channel();
        let original = media.clone();
        let start = scope.spawn(move || {
            let _ = sent.send(original.start_next_work(sdk.instance_id));
        });
        let admitted = received.recv_timeout(Duration::from_secs(5));
        let entry = reached.recv_timeout(Duration::from_secs(5));
        let mut registered = false;
        let mut pending = false;
        let before_release = if let Ok(Ok(job)) = &admitted {
            registered = fixture
                .workflow
                .host
                .engine()
                .jobs()
                .running()
                .iter()
                .any(|r| r.id.0 == job.handle.id);
            pending = matches!(job.result.result(), Ok(None));
            let host = fixture.workflow.host.clone();
            let (done, completed) = mpsc::channel();
            let review = fixture.workflow.host.engine().start_job(JobKind::BulkOp, move |job| {
                // Same production controlled service as start_session; not a
                // no-op queued merely to witness spare pool capacity.
                let value = host.review().start_controlled(
                    meguri_model::ids::DeckId(1),
                    None,
                    job.cancel.clone(),
                    job.progress.clone(),
                );
                let _ = done.send(
                    value
                        .as_ref()
                        .map(|value| matches!(value, meguri_engine::session::StartResult::Started { .. }))
                        .map_err(Clone::clone),
                );
                if let Ok(meguri_engine::session::StartResult::Started { session_id, .. }) = value {
                    host.review()
                        .end_controlled(session_id, job.cancel.clone(), job.progress.clone())?;
                }
                Ok(())
            });
            let completed = completed.recv_timeout(Duration::from_secs(5));
            // Observe the real review JobFinished while the scan is held.
            let terminal = review.map(|job| terminal(&events, job.id.0));
            Some((completed, terminal))
        } else {
            None
        };
        let still_held = !SCAN_RELEASED.load(Ordering::SeqCst);
        drop(hold);
        let joined = start.join();
        (
            entry,
            before_release.map(|value| (value, still_held)),
            registered,
            pending,
            admitted,
            joined,
        )
    });
    let terminal = admitted
        .as_ref()
        .ok()
        .and_then(|value| value.as_ref().ok())
        .map(|job| terminal(&events, job.handle.id));
    // Always release/join/drain/close before diagnosing the expected failure.
    let closed = fixture.workflow.host.close();
    joined.unwrap();
    closed.unwrap();
    entry.expect("positive entry into media.scan.after_cutoff");
    assert!(
        registered && pending,
        "start must return a registered, pending job while scanning"
    );
    let ((review, review_terminal), still_held) = before_release.expect("start admission before scan release");
    assert!(
        still_held,
        "review completion was observed only after the scan hold ended"
    );
    assert!(
        review
            .expect("real review start completed before scan release")
            .unwrap(),
        "real review start was refused"
    );
    assert_eq!(review_terminal.unwrap().unwrap(), JobOutcome::Succeeded);
    assert_eq!(terminal.unwrap().unwrap(), JobOutcome::Succeeded);
}

#[test]
fn media_next_job_scan_hold_sdk_replacement_rejects_original_after_local_capture() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    fixture.workflow.core.prepare_cloud_media_sdk(&sdk.instance_id).unwrap();
    let service = fixture.workflow.host.media().unwrap();
    std::fs::write(
        service.store().media_dir().join("sdk-replaced.asset"),
        b"changed folder",
    )
    .unwrap();
    let guard = fixture.workflow.core.adapter_guard();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (hold, reached) = arm_scan();
    let started = media.clone().start_next_work(sdk.instance_id.clone()).unwrap();
    let entry = reached.recv_timeout(Duration::from_secs(5));
    let replacement = (|| {
        fixture.workflow.retire_sdk(sdk.instance_id.clone(), false)?;
        fixture.workflow.release_sdk(sdk.instance_id.clone())?;
        fixture.workflow.register_sdk(false)
    })();
    let successor_guard = fixture.workflow.core.adapter_guard();
    let still_held = !SCAN_RELEASED.load(Ordering::SeqCst);
    drop(hold);
    let terminal = terminal(&events, started.handle.id);
    let value = started.result.result();
    let closed = fixture.workflow.host.close();
    // No journal-unchanged requirement: local capture remains permitted.
    closed.unwrap();
    entry.expect("positive entry into media.scan.after_cutoff");
    assert!(still_held, "SDK replacement completed only after scan release");
    assert_ne!(replacement.unwrap().instance_id, sdk.instance_id);
    assert_eq!(successor_guard, guard);
    assert!(matches!(terminal.unwrap(), JobOutcome::Failed { .. }));
    let expected = crate::sync::cloud_error(meguri_sync::cloud::runtime::RuntimeError::StaleIncarnation);
    assert!(matches!(value, Err(error) if error == expected));
    assert!(matches!(started.result.result(), Err(error) if error == expected));
}

#[test]
fn media_next_job_queued_cancel_distinguishes_pending_from_completed_no_work_and_retains_close_result() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    accept_root(&media, &sdk);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let cancelled = media.clone().start_next_work(sdk.instance_id.clone()).unwrap();
    assert!(cancelled.result.result().unwrap().is_none());
    cancelled.handle.cancel.cancel().unwrap();
    drop(pool);
    assert_eq!(finished(&events, cancelled.handle.id), JobOutcome::Cancelled);
    assert!(matches!(cancelled.result.result(), Err(CoreError::Cancelled)));
    fixture.workflow.core.pause_cloud_media();
    let idle = media.clone().start_next_work(sdk.instance_id.clone()).unwrap();
    assert_eq!(finished(&events, idle.handle.id), JobOutcome::Succeeded);
    assert!(idle.result.result().unwrap().unwrap().work.is_none());
    let expected = media.clone().next_work("wrong-sdk".into()).err().unwrap();
    let failed = media.clone().start_next_work("wrong-sdk".into()).unwrap();
    assert!(matches!(finished(&events, failed.handle.id), JobOutcome::Failed { .. }));
    fixture.workflow.host.close().unwrap();
    assert!(idle.result.result().unwrap().unwrap().work.is_none());
    assert!(matches!(failed.result.result(), Err(error) if error == expected));
    assert!(media.start_next_work(sdk.instance_id).is_err());
}

#[test]
fn media_next_job_cancel_after_runtime_lockwait_precedes_sdk_and_selector_effects() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    let before = fixture.workflow.core.adapter_guard();
    let events = fixture.workflow.host.engine().stream().subscribe();
    std::thread::scope(|scope| {
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let runtime = fixture.workflow.core.clone();
        let held = scope.spawn(move || {
            runtime.with_state_held_for_testing(|| {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(20)).unwrap();
            })
        });
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let (contended, observed) = mpsc::channel();
        fixture.workflow.core.observe_state_contention_for_testing(contended);
        let started = media.clone().start_next_work(sdk.instance_id.clone()).unwrap();
        let contention = observed.recv_timeout(Duration::from_secs(5));
        started.handle.cancel.cancel().unwrap();
        drop(release);
        let joined = held.join();
        let terminal = finished(&events, started.handle.id);
        joined.unwrap();
        contention.expect("worker reached original Runtime lock");
        assert_eq!(terminal, JobOutcome::Cancelled);
        assert!(matches!(started.result.result(), Err(CoreError::Cancelled)));
    });
    assert_eq!(fixture.workflow.core.adapter_guard(), before);
    assert!(media.clone().next_work(sdk.instance_id).unwrap().is_some());
    fixture.workflow.host.close().unwrap();
}

fn post_effect_cancel(fail_hash: bool) {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    fixture.workflow.core.prepare_cloud_media_sdk(&sdk.instance_id).unwrap();
    let root = fixture.workflow.core.adapter_snapshot().unwrap().root.unwrap();
    fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
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
    let service = fixture.workflow.host.media().unwrap();
    std::fs::write(service.store().media_dir().join("post-effect.asset"), b"new content").unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (hold, reached) = arm_scan();
    if fail_hash {
        faultpoint::arm(
            "media.scan.before_hash",
            Fault::IoError(std::io::ErrorKind::PermissionDenied),
        );
    }
    let started = media.clone().start_next_work(sdk.instance_id).unwrap();
    let entry = reached.recv_timeout(Duration::from_secs(5));
    // Adoption of the new SDK already cleared original media reservations:
    // cancellation must not erase those effects or their exact typed outcome.
    started.handle.cancel.cancel().unwrap();
    drop(hold);
    let terminal = finished(&events, started.handle.id);
    faultpoint::disarm("media.scan.before_hash");
    let value = started.result.result();
    fixture.workflow.host.close().unwrap();
    entry.expect("positive post-SDK-effect scan hold");
    if fail_hash {
        assert!(matches!(terminal, JobOutcome::Failed { .. }));
        let expected = value.err().expect("exact media failure");
        assert_ne!(expected, CoreError::Cancelled);
        assert!(matches!(started.result.result(), Err(error) if error == expected));
    } else {
        assert_eq!(terminal, JobOutcome::Succeeded);
        let work = value.unwrap().unwrap().work.unwrap();
        assert!(matches!(work.request, CloudMediaRequest::Root));
        let retained = started.result.result().unwrap().unwrap().work.unwrap();
        assert!(Arc::ptr_eq(&work.handle, &retained.handle));
    }
}
#[test]
fn media_next_job_post_effect_cancellation_retains_original_issued_work_after_close() {
    post_effect_cancel(false);
}
#[test]
fn media_next_job_post_effect_cancellation_retains_exact_error_after_close() {
    post_effect_cancel(true);
}

#[test]
fn media_next_job_queued_sdk_replacement_and_stale_owner_keep_original_completed_cell() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let saved = media.clone().start_next_work(sdk.instance_id.clone()).unwrap();
    assert_eq!(finished(&events, saved.handle.id), JobOutcome::Succeeded);
    let original = saved.result.result().unwrap().unwrap().work.unwrap();
    let pool = hold_pool(&fixture);
    let queued = media.clone().start_next_work(sdk.instance_id.clone()).unwrap();
    fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
    fixture.workflow.release_sdk(sdk.instance_id).unwrap();
    let _replacement = fixture.workflow.register_sdk(false).unwrap();
    drop(pool);
    assert!(matches!(finished(&events, queued.handle.id), JobOutcome::Failed { .. }));
    let exact = queued.result.result().err().unwrap();
    let engine = fixture.workflow.host.engine();
    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    exclusive.replace(|_| Ok(())).unwrap();
    drop(exclusive);
    drop(slot);
    assert!(media.clone().start_next_work("stale-owner".into()).is_err());
    fixture.workflow.host.close().unwrap();
    let retained = saved.result.result().unwrap().unwrap().work.unwrap();
    assert!(Arc::ptr_eq(&original.handle, &retained.handle));
    assert!(matches!(queued.result.result(), Err(error) if error == exact));
}
