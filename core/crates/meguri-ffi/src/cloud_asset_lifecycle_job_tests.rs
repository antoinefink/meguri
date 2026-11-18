//! Original whole-submit/resource lifecycle; base 21c5e02e, batch 1416a520.
use super::*;
use meguri_engine::EventKind;
use meguri_storage::ReaderRole;
use std::collections::BTreeMap;

fn collect_job_outcomes(
    engine: &meguri_engine::Engine,
    events: &meguri_engine::Subscription,
    ids: &[u64],
) -> BTreeMap<u64, JobOutcome> {
    let mut got = BTreeMap::new();
    while got.len() != ids.len() {
        let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
        if let EventKind::JobFinished { id, outcome } = &event.kind
            && ids.contains(&id.0)
        {
            assert!(got.insert(id.0, outcome.clone()).is_none(), "duplicate terminal");
        }
    }
    let fence = engine
        .jobs()
        .start("asset resource terminal fence", engine.generation(), None, |_| Ok(()));
    loop {
        let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
        if let EventKind::JobFinished { id, .. } = &event.kind {
            assert!(
                !ids.contains(&id.0),
                "duplicate target terminal after its stored result"
            );
            if *id == fence.id {
                break;
            }
        }
    }
    got
}

#[test]
fn asset_lifecycle_jobs_required_queued_cancel_runs_actual_complete_cancel_finish_once() {
    for first in [false, true] {
        let (fixture, _, port, name) = target(first);
        let pass = port.begin().unwrap();
        let issued = pass.clone().next().unwrap().unwrap();
        assert!(pass.submit(issued.handle.clone(), Queue::new(true)).unwrap());
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let bodies = Arc::new(Mutex::new(BTreeMap::new()));
        let observed = bodies.clone();
        fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            if matches!(
                stage,
                Stage::AssetReadCompleteBody | Stage::AssetReadCancelBody | Stage::AssetReadFinishBody
            ) {
                *observed.lock().unwrap().entry(stage).or_insert(0) += 1;
            }
        });
        let pool = hold_pool(&fixture);
        let complete = pass.clone().start_complete(issued.handle.clone(), None).unwrap();
        let cancel = pass.clone().start_cancel(issued.handle.clone()).unwrap();
        let finish = pass.clone().start_finish().unwrap();
        complete.handle.cancel.cancel().unwrap();
        cancel.handle.cancel.cancel().unwrap();
        finish.handle.cancel.cancel().unwrap();
        assert!(complete.result.result().unwrap().is_none());
        assert!(cancel.result.result().unwrap().is_none());
        assert!(finish.result.result().unwrap().is_none());
        drop(pool);
        let got = collect_job_outcomes(
            engine,
            &events,
            &[complete.handle.id, cancel.handle.id, finish.handle.id],
        );
        assert!(got.values().all(|outcome| *outcome == JobOutcome::Succeeded));
        assert!(!complete.result.result().unwrap().unwrap().intake_accepted);
        assert_eq!(cancel.result.result().unwrap(), Some(false));
        assert_eq!(finish.result.result().unwrap(), Some(true));
        for stage in [
            Stage::AssetReadCompleteBody,
            Stage::AssetReadCancelBody,
            Stage::AssetReadFinishBody,
        ] {
            assert_eq!(bodies.lock().unwrap()[&stage], 1);
        }
        assert_eq!(attempts(&fixture, &name), u32::from(!first));
        assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
        fixture.workflow.host.close().unwrap();
        // Typed reads remain independent of lifecycle and cancellation.
        assert_eq!(cancel.result.result().unwrap(), Some(false));
        assert_eq!(finish.result.result().unwrap(), Some(true));
        assert!(!complete.result.result().unwrap().unwrap().intake_accepted);
    }
}

#[test]
fn asset_lifecycle_jobs_required_panic_retains_no_body_result_and_original_owner() {
    let (fixture, _, port, _) = target(false);
    let pass = port.begin().unwrap();
    let work = pass.clone().next().unwrap().unwrap().handle;
    assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
    let events = fixture.workflow.host.engine().stream().subscribe();
    fixture.workflow.core.set_incoming_stage_hook_for_testing(|stage| {
        if stage == Stage::AssetReadCompleteBody {
            panic!("actual resource body panic");
        }
    });
    let complete = pass.clone().start_complete(work.clone(), None).unwrap();
    assert!(
        matches!(finished(&events, complete.handle.id), JobOutcome::Failed { error: meguri_engine::EngineError::Internal { message } } if message.contains("actual resource body panic"))
    );
    assert!(complete.result.result().unwrap().is_none());
    assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 1);
    fixture.workflow.core.set_incoming_stage_hook_for_testing(|_| {});
    let complete = pass.clone().start_complete(work, None).unwrap();
    assert_eq!(finished(&events, complete.handle.id), JobOutcome::Succeeded);
    assert!(!complete.result.result().unwrap().unwrap().intake_accepted);
    pass.finish().unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn asset_lifecycle_jobs_post_enqueueing_cancellation_compact_and_transfer_refusals_settle_uncharged() {
    for first in [false, true] {
        for refusal in 0..3 {
            let (fixture, sdk, port, name) = target(first);
            let pass = port.begin().unwrap();
            let work = pass.clone().next().unwrap().unwrap().handle;
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let resume = Mutex::new(resume);
            fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
                if stage == Stage::AssetReadSubmitEdge {
                    entered.send(()).unwrap();
                    resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
                }
            });
            if refusal == 2 {
                fixture.workflow.core.capture_asset_submit_reader_for_testing();
            }
            let queue = Queue::new(true);
            let submit = pass.clone().start_submit(work.clone(), queue.clone()).unwrap();
            let release = Release(release);
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            match refusal {
                0 => submit.handle.cancel.cancel().unwrap(),
                1 => fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap(),
                _ => {}
            }
            drop(release);
            let outcome = finished(&events, submit.handle.id);
            let result = submit.result.result();
            assert!(result.is_err());
            if refusal == 0 {
                assert_eq!(result, Err(CoreError::Cancelled));
                assert_eq!(outcome, JobOutcome::Cancelled);
            } else {
                assert!(matches!(outcome, JobOutcome::Failed { .. }));
            }
            assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
            assert_eq!(attempts(&fixture, &name), 0);
            assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
            assert!(
                !pass.cancel(work).unwrap(),
                "rejected admission left no request to cancel"
            );
            fixture.workflow.core.set_incoming_stage_hook_for_testing(|_| {});
            if refusal == 1 {
                fixture.workflow.release_sdk(sdk.instance_id).unwrap();
            } else {
                let fresh = port.begin().unwrap();
                assert_eq!(
                    fresh.clone().next().unwrap().unwrap().name,
                    name,
                    "no retry/grant consumed"
                );
                fresh.finish().unwrap();
            }
            pass.finish().unwrap();
            fixture.workflow.host.close().unwrap();
        }
    }
}

struct EntryQueue {
    collection: meguri_engine::Collection,
    entered: mpsc::Sender<()>,
}
impl CloudAssetReadQueue for EntryQueue {
    fn enqueue(&self) -> bool {
        assert_eq!(self.collection.cloud_reader_slot_for_testing(), Some(true));
        self.entered.send(()).unwrap();
        true
    }
}

#[test]
fn asset_lifecycle_jobs_callback_enters_with_actual_driver_runtime_held_and_reader_slot_unlocked() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let runtime = fixture.workflow.core.clone();
        let (prepared, prepared_rx) = mpsc::channel();
        let (continue_submit, submit_rx) = mpsc::channel();
        let submit_rx = Mutex::new(submit_rx);
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadSubmitEdge {
                prepared.send(()).unwrap();
                submit_rx.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            }
        });
        let (entry, entered) = mpsc::channel();
        let job = pass
            .clone()
            .start_submit(
                work.clone(),
                Arc::new(EntryQueue {
                    collection: fixture.workflow.host.collection().unwrap(),
                    entered: entry,
                }),
            )
            .unwrap();
        prepared_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (driver_enter, driver_entered) = mpsc::channel();
        let (driver_release, driver_resume) = mpsc::channel();
        let (runtime_enter, runtime_entered) = mpsc::channel();
        let (runtime_release, runtime_resume) = mpsc::channel();
        let (reader_return, reader_returned) = mpsc::channel();
        let (waiting, waited) = mpsc::channel();
        let collection = fixture.workflow.host.collection().unwrap();
        collection.observe_cloud_reader_wait_for_testing(waiting);
        std::thread::scope(|scope| {
            let submit_release = Release(continue_submit);
            let driver_release = Release(driver_release);
            let runtime_release = Release(runtime_release);
            let policy = fixture.policy.clone();
            fixture
                .workflow
                .host
                .cloud_policy_driver()
                .unwrap()
                .set_projection_hook_for_testing(move || {
                    driver_enter.send(()).unwrap();
                    driver_resume.recv_timeout(Duration::from_secs(10)).unwrap();
                });
            let driver = scope.spawn(move || policy.input(CloudPolicyEvent::Foreground));
            driver_entered.recv_timeout(Duration::from_secs(5)).unwrap();
            let held = runtime.clone();
            let holder = scope.spawn(move || {
                held.with_state_held_for_testing(|| {
                    runtime_enter.send(()).unwrap();
                    runtime_resume.recv_timeout(Duration::from_secs(10)).unwrap();
                })
            });
            runtime_entered.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(submit_release);
            // Observe ENTRY; accounting is expected to wait for Runtime.
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(job.result.result(), Ok(None));
            let second = scope.spawn(move || {
                let result = collection.reader(ReaderRole::Cloud).map(|_| ());
                reader_return.send(result).unwrap();
            });
            assert!(waited.recv_timeout(Duration::from_secs(5)).unwrap());
            assert!(matches!(reader_returned.try_recv(), Err(mpsc::TryRecvError::Empty)));
            drop(runtime_release);
            drop(driver_release);
            holder.join().unwrap();
            driver.join().unwrap().unwrap();
            assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
            assert_eq!(job.result.result(), Ok(Some(true)));
            reader_returned.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            second.join().unwrap();
        });
        runtime.set_incoming_stage_hook_for_testing(|_| {});
        pass.complete(work, None).unwrap();
        pass.finish().unwrap();
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_real_replacement_waits_for_pin_after_callback_through_accounting() {
    for (first, accept) in [(false, false), (false, true), (true, false), (true, true)] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        let engine = fixture.workflow.host.engine();
        let collection = fixture.workflow.host.collection().unwrap();
        let generation = collection.generation();
        let events = engine.stream().subscribe();
        let (accounting, accounting_rx) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadSubmitAccounting {
                accounting.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            }
        });
        let queue = Queue::new(accept);
        let submit = pass.clone().start_submit(work.clone(), queue.clone()).unwrap();
        accounting_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
        let (waiting, waited) = mpsc::channel();
        collection.observe_cloud_reader_wait_for_testing(waiting);
        let (swapped, swap_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let release = Release(release);
            let replacement_engine = engine.clone();
            let replacement = scope.spawn(move || {
                let slot = replacement_engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive.replace(move |_| {
                    swapped.send(()).unwrap();
                    Ok(())
                })
            });
            assert!(waited.recv_timeout(Duration::from_secs(5)).unwrap());
            assert_eq!(engine.generation(), generation);
            assert!(matches!(swap_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            assert_eq!(submit.result.result(), Ok(None));
            drop(release);
            replacement.join().unwrap().unwrap();
        });
        swap_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let _terminal = finished(&events, submit.handle.id);
        assert!(submit.result.result() != Ok(None));
        if !accept {
            assert_eq!(submit.result.result(), Ok(Some(false)));
        }
        assert!(engine.generation() > generation);
        let complete = pass.clone().start_complete(work, None).unwrap();
        assert!(matches!(
            finished(&events, complete.handle.id),
            JobOutcome::Failed { .. }
        ));
        assert_eq!(
            complete.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
        pass.finish().unwrap();
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_late_original_cleanup_is_memory_only_after_close_unpause_and_successor() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        fixture.workflow.host.close().unwrap();
        let (successor, sdk) = published();
        let (server, _, _) = waiting_note(&successor, &sdk);
        let fresh = retry_pass(&successor, &sdk);
        let selected = fresh.clone().next().unwrap().unwrap();
        assert!(fresh.submit(selected.handle.clone(), Queue::new(true)).unwrap());
        let before = successor.workflow.core.adapter_snapshot().unwrap();
        let complete = pass.clone().start_complete(work.clone(), None).unwrap();
        let cancel = pass.clone().start_cancel(work).unwrap();
        let finish = pass.clone().start_finish().unwrap();
        let got = collect_job_outcomes(
            engine,
            &events,
            &[complete.handle.id, cancel.handle.id, finish.handle.id],
        );
        assert!(matches!(got[&complete.handle.id], JobOutcome::Failed { .. }));
        assert_eq!(
            complete.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
        assert_eq!(cancel.result.result(), Ok(Some(false)));
        assert_eq!(finish.result.result(), Ok(Some(true)));
        assert!(successor.workflow.core.adapter_snapshot().unwrap() == before);
        assert_eq!(successor.workflow.core.asset_read_owned_count_for_testing(), 1);
        assert_eq!(selected.name, server.name);
        fresh.complete(selected.handle, None).unwrap();
        fresh.finish().unwrap();
        successor.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_ordinary_queued_cancel_and_accepted_accounting_error_preserve_results() {
    for first in [false, true] {
        for accepted in [false, true] {
            let (fixture, _, port, name) = target(first);
            let pass = port.begin().unwrap();
            let selected = pass.clone().next().unwrap().unwrap();
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let pool = hold_pool(&fixture);
            let queue = Queue::new(true);
            let submit = pass
                .clone()
                .start_submit(selected.handle.clone(), queue.clone())
                .unwrap();
            if accepted {
                c::testing::arm_tracking_fault("cloud.tracking.commit.asset_read", c::testing::TrackingFault::Io)
                    .unwrap();
            } else {
                submit.handle.cancel.cancel().unwrap();
            }
            assert_eq!(submit.result.result(), Ok(None));
            drop(pool);
            let outcome = finished(&events, submit.handle.id);
            if accepted {
                assert!(matches!(outcome, JobOutcome::Failed { .. }));
                assert!(submit.result.result().is_err());
                assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
                assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 1);
                assert!(!pass.cancel(selected.handle.clone()).unwrap());
                let complete = pass.clone().start_complete(selected.handle, None).unwrap();
                assert_eq!(finished(&events, complete.handle.id), JobOutcome::Succeeded);
                assert!(!complete.result.result().unwrap().unwrap().intake_accepted);
                // Failed durable accounting remains fenced, never retried as a
                // fresh native attempt by this conversion.
                assert!(port.begin().is_err());
            } else {
                assert_eq!(outcome, JobOutcome::Cancelled);
                assert_eq!(submit.result.result(), Err(CoreError::Cancelled));
                assert_eq!(queue.calls.load(Ordering::SeqCst), 0);
                let cancel = pass.clone().start_cancel(selected.handle).unwrap();
                assert_eq!(finished(&events, cancel.handle.id), JobOutcome::Succeeded);
                assert_eq!(cancel.result.result(), Ok(Some(true)));
            }
            assert_eq!(attempts(&fixture, &name), 0);
            let finish = pass.clone().start_finish().unwrap();
            assert_eq!(finished(&events, finish.handle.id), JobOutcome::Succeeded);
            fixture.workflow.host.close().unwrap();
            assert!(submit.result.result().is_err());
        }
    }
}

#[test]
fn asset_lifecycle_jobs_original_selector_actual_cancelled_and_absent_returns_are_distinct() {
    use crate::cloud_policy::policy_jobs::{Cell, RequiredCompletion};
    let (fixture, _, port, _) = target(false);
    let pass = port.begin().unwrap();
    let collection = fixture.workflow.host.collection().unwrap();
    let original = fixture.workflow.host.original_resource_authority(&collection);
    let engine = fixture.workflow.host.engine();
    let events = engine.stream().subscribe();
    let pool = hold_pool(&fixture);
    let error = Arc::new(Cell::<bool>(Mutex::new(None)));
    let returned = RequiredCompletion(error.clone());
    let cancelled = engine
        .jobs()
        .start_original_resource("actual typed Cancelled", original.clone(), move |_, _| {
            returned.finish(Err(CoreError::Cancelled))
        });
    let absent = Arc::new(Cell::<bool>(Mutex::new(None)));
    let not_returned = RequiredCompletion(absent.clone());
    let no_body_return = engine
        .jobs()
        .start_original_resource("no stored return", original, move |_, _| {
            drop(not_returned);
            Ok(())
        });
    cancelled.cancel.cancel();
    no_body_return.cancel.cancel();
    drop(pool);
    let got = collect_job_outcomes(engine, &events, &[cancelled.id.0, no_body_return.id.0]);
    assert_eq!(got[&cancelled.id.0], JobOutcome::Cancelled);
    assert_eq!(got[&no_body_return.id.0], JobOutcome::Succeeded);
    assert_eq!(
        error.0.lock().unwrap().as_ref().map(|stored| stored.result.clone()),
        Some(Err(CoreError::Cancelled))
    );
    assert_eq!(
        absent.0.lock().unwrap().as_ref().map(|stored| stored.result.clone()),
        None
    );
    pass.finish().unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn asset_lifecycle_jobs_close_joins_original_resource_body_before_runtime_release() {
    for first in [false, true] {
        let (fixture, sdk, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadCompleteBody {
                entered.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            }
        });
        let job = pass.clone().start_complete(work, None).unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        std::thread::scope(|scope| {
            let release = Release(release);
            let (done, returned) = mpsc::channel();
            let host = fixture.workflow.host.clone();
            let closing = scope.spawn(move || {
                let result = host.close();
                done.send(result).unwrap();
            });
            let until = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while !job.handle.cancel.requested() && meguri_engine::monotonic::now() < until {
                std::thread::yield_now();
            }
            assert!(job.handle.cancel.requested());
            assert!(matches!(returned.try_recv(), Err(mpsc::TryRecvError::Empty)));
            assert!(fixture.workflow.core.validate_sdk(&sdk.instance_id).is_ok());
            assert!(job.result.result().unwrap().is_none());
            drop(release);
            returned.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            closing.join().unwrap();
        });
        assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
        assert_eq!(
            job.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
        assert_eq!(fixture.workflow.core.asset_read_owned_count_for_testing(), 0);
    }
}

#[test]
fn asset_lifecycle_jobs_resume_does_not_promote_pending_cleanup() {
    for is_initial in [false, true] {
        let (case, _, port, _) = target(is_initial);
        let reader = port.begin().unwrap();
        let request = reader.clone().next().unwrap().unwrap().handle;
        assert!(reader.submit(request.clone(), Queue::new(true)).unwrap());
        let host_engine = case.workflow.host.engine();
        let notifications = host_engine.stream().subscribe();
        assert_eq!(
            host_engine.suspend(Duration::from_secs(5)).unwrap(),
            meguri_engine::SuspendOutcome::Suspended
        );
        let (body_started, body_ready) = mpsc::channel();
        let (allow_exit, exit_permission) = mpsc::channel();
        let exit_permission = Mutex::new(exit_permission);
        case.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadCompleteBody {
                body_started.send(()).unwrap();
                exit_permission
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
        });
        let completion = reader.clone().start_complete(request.clone(), None).unwrap();
        body_ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let body_guard = Release(allow_exit);
        host_engine.resume().unwrap();
        drop(body_guard);
        assert!(matches!(
            finished(&notifications, completion.handle.id),
            JobOutcome::Failed { .. }
        ));
        assert_eq!(
            completion.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
        case.workflow.core.set_incoming_stage_hook_for_testing(|_| {});
        let cancellation = reader.clone().start_cancel(request).unwrap();
        let ending = reader.clone().start_finish().unwrap();
        collect_job_outcomes(host_engine, &notifications, &[cancellation.handle.id, ending.handle.id]);
        assert_eq!(cancellation.result.result(), Ok(Some(false)));
        assert_eq!(ending.result.result(), Ok(Some(true)));
        case.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_required_activity_keeps_raw_errors_before_single_ended_and_late_pause_refuses() {
    use meguri_sync::cloud::policy as p;
    let operation = p::Operation {
        zone: p::Zone::Collection,
        kind: p::OperationKind::Read,
    };
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let facts = |started, failures: Vec<i64>, cancelled| CloudAssetReadActivityFacts {
            started,
            failed: !failures.is_empty(),
            app_cancelled: cancelled,
            failures: failures
                .into_iter()
                .map(|code| CloudAssetReadFailureFacts {
                    code: Some(code),
                    retry_after_ms: Some(120_000),
                })
                .collect(),
        };
        let start = pass
            .clone()
            .start_operation_activity(facts(true, vec![], false))
            .unwrap();
        assert_eq!(finished(&events, start.handle.id), JobOutcome::Succeeded);
        let work = pass.clone().next().unwrap().unwrap().handle;
        let submit = pass.clone().start_submit(work.clone(), Queue::new(true)).unwrap();
        assert_eq!(finished(&events, submit.handle.id), JobOutcome::Succeeded);
        assert_eq!(submit.result.result(), Ok(Some(true)));
        let complete = pass.clone().start_complete(work, None).unwrap();
        assert_eq!(finished(&events, complete.handle.id), JobOutcome::Succeeded);
        let finish = pass.clone().start_finish().unwrap();
        assert_eq!(finished(&events, finish.handle.id), JobOutcome::Succeeded);
        let pool = hold_pool(&fixture);
        let ended = pass
            .clone()
            .start_operation_activity(facts(false, vec![3, 6, 14], false))
            .unwrap();
        ended.handle.cancel.cancel().unwrap();
        assert!(ended.result.result().unwrap().is_none());
        drop(pool);
        assert_eq!(finished(&events, ended.handle.id), JobOutcome::Succeeded);
        assert_eq!(ended.result.result().unwrap().unwrap().len(), 4);
        let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert_eq!(state.cycles[&operation].failures, 1);
        assert!(!state.cycles[&operation].active);
        assert_eq!(state.last_error.unwrap().code, 14);
        assert_eq!(
            engine.suspend(Duration::from_secs(5)).unwrap(),
            meguri_engine::SuspendOutcome::Suspended
        );
        let late = pass
            .clone()
            .start_operation_activity(facts(false, vec![6], false))
            .unwrap();
        assert_eq!(finished(&events, late.handle.id), JobOutcome::Cancelled);
        assert!(
            late.result.result().unwrap().is_none(),
            "refused required activity has no body return"
        );
        fixture.workflow.host.close().unwrap();
        assert_eq!(ended.result.result().unwrap().unwrap().len(), 4);
    }
}

#[test]
fn asset_lifecycle_jobs_activity_finishes_while_host_close_state_mutex_is_held() {
    for started in [true, false] {
        let (fixture, _, port, _) = target(false);
        let pass = port.begin().unwrap();
        let facts = |started| CloudAssetReadActivityFacts {
            started,
            failed: false,
            app_cancelled: false,
            failures: vec![],
        };
        if !started {
            pass.operation_activity(facts(true)).unwrap();
        }
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let (projected, projection_rx) = mpsc::channel();
        let (continue_activity, activity_rx) = mpsc::channel();
        fixture
            .workflow
            .host
            .cloud_policy_driver()
            .unwrap()
            .set_projection_hook_for_testing(move || {
                projected.send(()).unwrap();
                activity_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            });
        let job = pass.clone().start_operation_activity(facts(started)).unwrap();
        let (locked, locked_rx) = mpsc::channel();
        let (unlock, unlock_rx) = mpsc::channel();
        let (unlocked, unlocked_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let continue_activity = Release(continue_activity);
            let unlock = Release(unlock);
            projection_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(job.result.result().unwrap().is_none());
            let host = fixture.workflow.host.clone();
            let holder = scope.spawn(move || {
                host.with_close_state_held_for_testing(|| {
                    locked.send(()).unwrap();
                    unlock_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                });
                unlocked.send(()).unwrap();
            });
            locked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(continue_activity);
            assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
            assert_eq!(job.result.result().unwrap().unwrap().len(), 1);
            assert!(matches!(unlocked_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            drop(unlock);
            unlocked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            holder.join().unwrap();
        });
        let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        let operation = meguri_sync::cloud::policy::Operation {
            zone: meguri_sync::cloud::policy::Zone::Collection,
            kind: meguri_sync::cloud::policy::OperationKind::Read,
        };
        assert_eq!(state.cycles[&operation].active, started);
        assert_eq!(state.cycles[&operation].failures, 0);
        pass.finish().unwrap();
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn asset_lifecycle_jobs_required_complete_rechecks_original_liveness_after_actual_runtime_wait() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        assert!(pass.submit(work.clone(), Queue::new(true)).unwrap());
        let engine = fixture.workflow.host.engine();
        let events = engine.stream().subscribe();
        let runtime = fixture.workflow.core.clone();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (contended, observed) = mpsc::channel();
        runtime.observe_state_contention_for_testing(contended);
        let job = std::thread::scope(|scope| {
            let release = Release(release);
            let held = runtime.clone();
            let holder = scope.spawn(move || {
                held.with_state_held_for_testing(|| {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(10)).unwrap();
                })
            });
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            let job = pass.clone().start_complete(work, None).unwrap();
            observed.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(job.result.result().unwrap().is_none());
            let host = fixture.workflow.host.clone();
            let close = scope.spawn(move || host.close());
            let until = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while !job.handle.cancel.requested() && meguri_engine::monotonic::now() < until {
                std::thread::yield_now();
            }
            assert!(job.handle.cancel.requested());
            drop(release);
            holder.join().unwrap();
            close.join().unwrap().unwrap();
            job
        });
        assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
        assert_eq!(
            job.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
        assert_eq!(runtime.asset_read_owned_count_for_testing(), 0);
    }
}

#[test]
fn asset_lifecycle_jobs_actual_close_keeps_pin_and_accepted_result_through_accounting() {
    for first in [false, true] {
        let (fixture, _, port, _) = target(first);
        let pass = port.begin().unwrap();
        let work = pass.clone().next().unwrap().unwrap().handle;
        let collection = fixture.workflow.host.collection().unwrap();
        let engine = fixture.workflow.host.engine();
        let generation = engine.generation();
        let events = engine.stream().subscribe();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        fixture.workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == Stage::AssetReadSubmitAccounting {
                entered.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            }
        });
        let job = pass.clone().start_submit(work.clone(), Queue::new(true)).unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        std::thread::scope(|scope| {
            let release = Release(release);
            let host = fixture.workflow.host.clone();
            let (done, returned) = mpsc::channel();
            let close = scope.spawn(move || done.send(host.close()).unwrap());
            let until = meguri_engine::monotonic::now() + Duration::from_secs(3);
            while !job.handle.cancel.requested() && meguri_engine::monotonic::now() < until {
                std::thread::yield_now();
            }
            assert!(job.handle.cancel.requested());
            assert_eq!(collection.cloud_reader_slot_for_testing(), Some(true));
            assert_eq!(engine.generation(), generation);
            assert_eq!(job.result.result(), Ok(None));
            assert!(matches!(returned.try_recv(), Err(mpsc::TryRecvError::Empty)));
            drop(release);
            returned.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            close.join().unwrap();
        });
        assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
        assert_eq!(
            job.result.result(),
            Ok(Some(true)),
            "late cancellation cannot erase acceptance"
        );
        let complete = pass.clone().start_complete(work, None).unwrap();
        assert!(matches!(
            finished(&events, complete.handle.id),
            JobOutcome::Failed { .. }
        ));
        assert_eq!(
            complete.result.result().err(),
            Some(CoreError::Unavailable {
                reason: "The result cannot be applied.".into()
            })
        );
    }
}

#[test]
fn asset_lifecycle_jobs_old_terminal_cancel_duplicate_finish_cannot_mutate_actual_same_runtime_successor() {
    for first in [false, true] {
        for policy_pause in [false, true] {
            let (fixture, sdk, port, name) = target(first);
            let old = port.begin().unwrap();
            let original = old.clone().next().unwrap().unwrap().handle;
            assert!(old.submit(original.clone(), Queue::new(true)).unwrap());
            let root = fixture.workflow.core.adapter_snapshot().unwrap().root.unwrap();
            if policy_pause {
                for paused in [true, false] {
                    fixture
                        .policy
                        .clone()
                        .input(CloudPolicyEvent::Settings {
                            settings: CloudPolicyDeviceSettings {
                                enabled: true,
                                paused,
                                automatic: false,
                                media: false,
                            },
                        })
                        .unwrap();
                }
            }
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
            fixture
                .policy
                .clone()
                .input(CloudPolicyEvent::Refresh {
                    cause: CloudPolicyChangeCause::Cloud,
                })
                .unwrap();
            let successor = retry_pass(&fixture, &sdk);
            let current = successor.clone().next().unwrap().unwrap().handle;
            assert!(successor.submit(current.clone(), Queue::new(true)).unwrap());
            let before = fixture.workflow.core.adapter_snapshot().unwrap();
            let engine = fixture.workflow.host.engine();
            let events = engine.stream().subscribe();
            let complete = old.clone().start_complete(original.clone(), None).unwrap();
            let cancel = old.clone().start_cancel(original.clone()).unwrap();
            let finish = old.clone().start_finish().unwrap();
            collect_job_outcomes(
                engine,
                &events,
                &[complete.handle.id, cancel.handle.id, finish.handle.id],
            );
            assert!(complete.result.result().is_err());
            assert_eq!(cancel.result.result(), Ok(Some(false)));
            assert_eq!(finish.result.result(), Ok(Some(true)));
            assert!(
                !fixture
                    .workflow
                    .core
                    .asset_read_owned_for_testing(original.retained_for_test())
            );
            assert!(
                fixture
                    .workflow
                    .core
                    .asset_read_owned_for_testing(current.retained_for_test())
            );
            assert!(fixture.workflow.core.adapter_snapshot().unwrap() == before);
            let duplicate = old.clone().start_complete(original, None).unwrap();
            assert!(matches!(
                finished(&events, duplicate.handle.id),
                JobOutcome::Failed { .. }
            ));
            assert!(duplicate.result.result().is_err());
            assert!(
                fixture
                    .workflow
                    .core
                    .asset_read_owned_for_testing(current.retained_for_test())
            );
            let server = read(&fixture, &sdk, &name);
            let download = tempfile::tempdir().unwrap();
            let completion = successor
                .clone()
                .start_complete(
                    current,
                    Some(callback_record(&server, &sdk.collection_zone, download.path())),
                )
                .unwrap();
            assert_eq!(finished(&events, completion.handle.id), JobOutcome::Succeeded);
            assert!(completion.result.result().unwrap().unwrap().intake_accepted);
            successor.finish().unwrap();
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[path = "cloud_asset_resource_matrix_tests.rs"]
mod resource_matrix;
