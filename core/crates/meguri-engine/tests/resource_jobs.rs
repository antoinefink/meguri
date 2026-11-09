//! Original-resource admission and teardown; no resource selector production caller.
mod common;

use common::Harness;
use meguri_engine::jobs::registry::{OriginalHostLiveness, OriginalHostState, ResourceMode};
use meguri_engine::{Engine, EngineConfig, EngineError, EventKind, JobOutcome, Ports, SuspendParticipant};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(10);

// Own every release sender: assertion unwinding disconnects blocked workers.
struct Occupied(Vec<mpsc::Sender<()>>);
impl Occupied {
    fn new(engine: &Engine) -> Self {
        let mut releases = Vec::new();
        for _ in 0..4 {
            let (entered, ready) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            engine
                .jobs()
                .start("occupy actual pool", engine.generation(), None, move |_| {
                    entered.send(()).unwrap();
                    let _ = wait.recv_timeout(WAIT);
                    Ok(())
                });
            ready.recv_timeout(WAIT).unwrap();
            releases.push(release);
        }
        assert_eq!(
            engine
                .threads()
                .threads()
                .iter()
                .filter(|t| t.name.starts_with("meguri-job-"))
                .count(),
            4
        );
        Self(releases)
    }
    fn release(self) {
        drop(self.0);
    }
}
fn live() -> OriginalHostLiveness {
    let live = OriginalHostLiveness::default();
    live.publish(OriginalHostState::Active);
    live
}
fn outcome(engine: &Engine, subscription: &meguri_engine::Subscription, id: meguri_engine::JobId) -> JobOutcome {
    loop {
        let event = subscription.recv_timeout(WAIT).expect("terminal event");
        if let EventKind::JobFinished { id: actual, outcome } = &event.kind
            && *actual == id
        {
            assert!(!engine.jobs().running().iter().any(|j| j.id == id));
            return outcome.clone();
        }
    }
}

#[test]
fn resource_jobs_cancelled_in_actual_pool_preserve_real_returns_and_one_terminal() {
    let h = Harness::open();
    let occupied = Occupied::new(&h.engine);
    let sub = h.engine.stream().subscribe();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut jobs = Vec::new();
    for result in [
        Ok(()),
        Err(EngineError::Internal {
            message: "actual error".into(),
        }),
        Err(EngineError::Cancelled),
    ] {
        let cell = Arc::new(Mutex::new(None));
        let stored = cell.clone();
        let called = calls.clone();
        let expected = result.clone();
        let authority = h.engine.original_resource_authority(&h.collection, live());
        let job = h.engine.jobs().start_original_resource(
            "no category inferred from name",
            authority,
            move |ctx, mut authority| {
                assert!(ctx.slot().is_none());
                assert!(ctx.cancel.is_cancelled());
                assert_eq!(authority.recheck(), ResourceMode::Full);
                called.fetch_add(1, Ordering::SeqCst);
                *stored.lock().unwrap() = Some(result.clone());
                result
            },
        );
        assert!(h.engine.jobs().cancel(job.id));
        jobs.push((job, cell, expected));
    }
    let ordinary_ran = Arc::new(AtomicBool::new(false));
    let ran = ordinary_ran.clone();
    let ordinary = h
        .engine
        .jobs()
        .start("ordinary", h.engine.generation(), None, move |_| {
            ran.store(true, Ordering::SeqCst);
            Ok(())
        });
    ordinary.cancel.cancel();
    let required_ran = Arc::new(AtomicBool::new(false));
    let ran = required_ran.clone();
    let required = h
        .engine
        .jobs()
        .start_required_completion("required", h.engine.generation(), move |_| {
            ran.store(true, Ordering::SeqCst);
            Ok(())
        });
    required.cancel.cancel();
    occupied.release();
    // Collect together: workers may finish in any order.
    let mut seen = std::collections::BTreeMap::<_, Vec<_>>::new();
    while jobs.iter().any(|(j, _, _)| !seen.contains_key(&j.id))
        || !seen.contains_key(&ordinary.id)
        || !seen.contains_key(&required.id)
    {
        let event = sub.recv_timeout(WAIT).unwrap();
        if let EventKind::JobFinished { id, outcome } = &event.kind {
            if let Some((_, cell, expected)) = jobs.iter().find(|(j, _, _)| j.id == *id) {
                assert_eq!(*cell.lock().unwrap(), Some(expected.clone()), "return precedes event");
            }
            seen.entry(*id).or_default().push(outcome.clone());
        }
    }
    h.engine.close().unwrap();
    while let Some(event) = sub.try_recv() {
        if let EventKind::JobFinished { id, outcome } = &event.kind {
            if let Some((_, cell, expected)) = jobs.iter().find(|(j, _, _)| j.id == *id) {
                assert_eq!(*cell.lock().unwrap(), Some(expected.clone()), "return precedes event");
            }
            seen.entry(*id).or_default().push(outcome.clone());
        }
    }
    for (job, cell, expected) in jobs {
        assert_eq!(*cell.lock().unwrap(), Some(expected.clone()));
        let expected = match expected {
            Ok(()) => JobOutcome::Succeeded,
            Err(EngineError::Cancelled) => JobOutcome::Cancelled,
            Err(error) => JobOutcome::Failed { error },
        };
        assert_eq!(seen[&job.id], [expected]);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(!ordinary_ran.load(Ordering::SeqCst));
    assert!(required_ran.load(Ordering::SeqCst));
    assert_eq!(seen[&ordinary.id], [JobOutcome::Cancelled]);
    assert_eq!(seen[&required.id], [JobOutcome::Succeeded]);
}

#[test]
fn resource_jobs_latch_original_facts_and_never_upgrade_after_restore_or_reopen() {
    let h = Harness::open();
    let other = Harness::open();
    let occupied = Occupied::new(&h.engine);
    let sub = h.engine.stream().subscribe();
    let mut pending = Vec::new();
    for state in [
        OriginalHostState::Prepared,
        OriginalHostState::Active,
        OriginalHostState::Closing,
        OriginalHostState::Closed,
    ] {
        let host = OriginalHostLiveness::default();
        host.publish(state);
        let authority = h.engine.original_resource_authority(&h.collection, host.clone());
        let (sent, value) = mpsc::channel();
        let job = h
            .engine
            .jobs()
            .start_original_resource("original", authority, move |_, mut authority| {
                sent.send(authority.recheck()).unwrap();
                Ok(())
            });
        host.publish(OriginalHostState::Active); // failed-close restoration cannot upgrade a latched call
        pending.push((
            job,
            value,
            if state == OriginalHostState::Active {
                ResourceMode::Full
            } else {
                ResourceMode::DisposalOnly
            },
        ));
    }
    let foreign = h.engine.original_resource_authority(&other.collection, live());
    let (sent, value) = mpsc::channel();
    let job = h
        .engine
        .jobs()
        .start_original_resource("wrong collection", foreign, move |_, mut authority| {
            sent.send(authority.recheck()).unwrap();
            Ok(())
        });
    pending.push((job, value, ResourceMode::DisposalOnly));
    let lost_host = live();
    let authority = h.engine.original_resource_authority(&h.collection, lost_host.clone());
    let (sent, value) = mpsc::channel();
    let job = h
        .engine
        .jobs()
        .start_original_resource("lost authority", authority, move |_, mut authority| {
            sent.send(authority.recheck()).unwrap();
            Ok(())
        });
    lost_host.publish(OriginalHostState::Closing);
    pending.push((job, value, ResourceMode::DisposalOnly));
    occupied.release();
    for (_, value, expected) in &pending {
        assert_eq!(value.recv_timeout(WAIT).unwrap(), *expected);
    }
    // Close drains all events, without assuming completion order.
    h.engine.close().unwrap();
    let old = h.engine.original_resource_authority(&h.collection, live());
    h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    let job = h
        .engine
        .jobs()
        .start_original_resource("old after reopen", old, |_, mut authority| {
            assert_eq!(authority.recheck(), ResourceMode::DisposalOnly);
            Ok(())
        });
    assert_eq!(outcome(&h.engine, &sub, job.id), JobOutcome::Succeeded);
}

struct ResourceParticipant {
    returned: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
}
impl SuspendParticipant for ResourceParticipant {
    fn name(&self) -> &str {
        "resource holder"
    }
    fn flush(&self) -> Result<(), EngineError> {
        Ok(())
    }
    fn suspend(&self) -> Result<(), EngineError> {
        assert!(self.returned.load(Ordering::SeqCst));
        Ok(())
    }
    fn release_connections_until(&self, _: Duration) -> Result<(), EngineError> {
        assert!(self.returned.load(Ordering::SeqCst));
        self.released.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn resource_jobs_full_early_drain_retains_real_reader_and_result_before_teardown() {
    for close in [false, true] {
        let returned = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let h = Harness::open_with(Ports::new().suspend_participant(Arc::new(ResourceParticipant {
            returned: returned.clone(),
            released: released.clone(),
        })));
        let sub = h.engine.stream().subscribe();
        let (entered, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel::<()>();
        let collection = h.collection.clone();
        let done = returned.clone();
        let authority = h.engine.original_resource_authority(&collection, live());
        let job = h.engine.jobs().start_original_resource(
            "retained original holder",
            authority,
            move |ctx, mut authority| {
                let reader = collection.reader(meguri_storage::ReaderRole::Cloud)?;
                entered.send(()).unwrap();
                let _ = wait.recv_timeout(WAIT);
                assert!(ctx.cancel.is_cancelled());
                assert_eq!(authority.recheck(), ResourceMode::Full, "pause alone keeps Full");
                drop(reader);
                done.store(true, Ordering::SeqCst);
                Err(EngineError::Internal {
                    message: "real body result".into(),
                })
            },
        );
        ready.recv_timeout(WAIT).unwrap();
        std::thread::scope(|scope| {
            let teardown = scope.spawn(|| {
                if close {
                    h.engine.close_with_host_cleanup(|_| {
                        assert!(returned.load(Ordering::SeqCst));
                        assert!(!released.load(Ordering::SeqCst));
                        Ok(())
                    })
                } else {
                    h.engine.suspend(Duration::from_secs(5)).map(|_| ())
                }
            });
            // Cancellation is teardown's positive registration barrier.
            let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
            while !job.cancel.is_cancelled() && meguri_engine::monotonic::now() < deadline {
                std::thread::yield_now();
            }
            drop(release);
            assert!(job.cancel.is_cancelled());
            teardown.join().unwrap().unwrap();
        });
        assert!(released.load(Ordering::SeqCst));
        assert_eq!(
            outcome(&h.engine, &sub, job.id),
            JobOutcome::Failed {
                error: EngineError::Internal {
                    message: "real body result".into()
                }
            }
        );
    }
}

#[test]
fn resource_jobs_actual_busy_tail_blocks_suspend_and_no_collection_close_cutoff() {
    for (no_collection, repeated) in [(false, false), (true, false), (true, true)] {
        let h = Harness::open();
        if no_collection {
            h.engine.close().unwrap();
        }
        // Cover both initial NoCollection and reuse of a previously sealed registry.
        let empty = Engine::new(EngineConfig::default()).unwrap();
        let engine = if no_collection && !repeated { &empty } else { &h.engine };
        let (tail, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        engine.jobs().hold_next_busy_tail_for_testing(tail, wait);
        let authority = engine.original_resource_authority(&h.collection, live());
        engine
            .jobs()
            .start_original_resource("busy tail", authority, |_, _| Ok(()));
        ready.recv_timeout(WAIT).unwrap();
        assert!(engine.jobs().running().is_empty());
        assert!(
            engine
                .threads()
                .busy_threads()
                .iter()
                .any(|t| t.name.starts_with("meguri-job-"))
        );
        let failed = if no_collection {
            engine.close()
        } else {
            engine.suspend(Duration::from_millis(40)).map(|_| ())
        };
        drop(release);
        assert!(matches!(failed, Err(EngineError::SuspendDeadline { .. })));
        engine.close().unwrap();
        let stores = Arc::new(AtomicUsize::new(0));
        let spy = stores.clone();
        let sub = engine.stream().subscribe();
        let job = engine.jobs().start_original_resource(
            "post cutoff",
            engine.original_resource_authority(&h.collection, live()),
            move |_, mut authority| {
                if authority.recheck() == ResourceMode::Full {
                    spy.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            },
        );
        assert_eq!(outcome(engine, &sub, job.id), JobOutcome::Succeeded);
        assert_eq!(stores.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn resource_jobs_second_engine_close_drains_new_registered_work() {
    let h = Harness::open();
    h.engine.close().unwrap();
    let (entered, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel::<()>();
    let returned = Arc::new(AtomicBool::new(false));
    let done = returned.clone();
    let job = h.engine.jobs().start(
        "ordinary work after completed cutoff",
        h.engine.generation(),
        None,
        move |_| {
            entered.send(()).unwrap();
            let _ = wait.recv_timeout(WAIT);
            done.store(true, Ordering::SeqCst);
            Ok(())
        },
    );
    ready.recv_timeout(WAIT).unwrap();
    std::thread::scope(|scope| {
        let (sent, closed) = mpsc::channel();
        let engine = &h.engine;
        let close = scope.spawn(move || {
            let result = engine.close();
            sent.send(result).unwrap();
        });
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
        while !job.cancel.is_cancelled() && meguri_engine::monotonic::now() < deadline {
            std::thread::yield_now();
        }
        let cancelled = job.cancel.is_cancelled();
        let early = closed.recv_timeout(Duration::from_millis(40));
        let still_registered = h.engine.jobs().running().iter().any(|j| j.id == job.id);
        drop(release);
        close.join().unwrap();
        assert!(cancelled && still_registered);
        assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)));
        closed.recv_timeout(WAIT).unwrap().unwrap();
    });
    assert!(returned.load(Ordering::SeqCst));
}

#[test]
fn resource_jobs_cleanup_latches_through_resume_with_required_admission_closed() {
    let h = Harness::open();
    h.engine.suspend(Duration::from_secs(5)).unwrap();
    let sub = h.engine.stream().subscribe();
    let untouched = Arc::new(AtomicBool::new(false));
    for required in [false, true] {
        let ran = untouched.clone();
        let work = move |_: &meguri_engine::JobContext| {
            ran.store(true, Ordering::SeqCst);
            Ok(())
        };
        let handle = if required {
            h.engine
                .jobs()
                .start_required_completion("paused required", h.engine.generation(), work)
        } else {
            h.engine
                .jobs()
                .start("paused ordinary", h.engine.generation(), None, work)
        };
        assert!(handle.cancel.is_cancelled());
        assert_eq!(outcome(&h.engine, &sub, handle.id), JobOutcome::Cancelled);
    }
    assert!(!untouched.load(Ordering::SeqCst));
    let mut releases = Vec::new();
    for _ in 0..4 {
        let (sent, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel::<()>();
        h.engine.jobs().start_original_resource(
            "paused memory work",
            h.engine.original_resource_authority(&h.collection, live()),
            move |_, mut authority| {
                assert_eq!(authority.recheck(), ResourceMode::DisposalOnly);
                sent.send(()).unwrap();
                let _ = wait.recv_timeout(WAIT);
                Ok(())
            },
        );
        ready.recv_timeout(WAIT).unwrap();
        releases.push(release);
    }
    let stores = Arc::new(AtomicUsize::new(0));
    let spy = stores.clone();
    let (sent, ready) = mpsc::channel();
    let job = h.engine.jobs().start_original_resource(
        "paused then resumed",
        h.engine.original_resource_authority(&h.collection, live()),
        move |_, mut authority| {
            let mode = authority.recheck();
            if mode == ResourceMode::Full {
                spy.fetch_add(1, Ordering::SeqCst);
            }
            sent.send(mode).unwrap();
            Ok(())
        },
    );
    h.engine.resume().unwrap();
    drop(releases);
    assert_eq!(ready.recv_timeout(WAIT).unwrap(), ResourceMode::DisposalOnly);
    assert_eq!(outcome(&h.engine, &sub, job.id), JobOutcome::Succeeded);
    assert_eq!(stores.load(Ordering::SeqCst), 0);
    let current = h.engine.jobs().start_original_resource(
        "new after resume",
        h.engine.original_resource_authority(&h.collection, live()),
        |_, mut authority| {
            assert_eq!(authority.recheck(), ResourceMode::Full);
            Ok(())
        },
    );
    assert_eq!(outcome(&h.engine, &sub, current.id), JobOutcome::Succeeded);
}

#[test]
fn resource_jobs_early_drain_deadline_keeps_original_reader_and_result_for_retry() {
    let returned = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let h = Harness::open_with(Ports::new().suspend_participant(Arc::new(ResourceParticipant {
        returned: returned.clone(),
        released: released.clone(),
    })));
    let (sent, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel::<()>();
    let collection = h.collection.clone();
    let done = returned.clone();
    let sub = h.engine.stream().subscribe();
    let job = h.engine.jobs().start_original_resource(
        "deadline holder",
        h.engine.original_resource_authority(&collection, live()),
        move |_, mut authority| {
            let reader = collection.reader(meguri_storage::ReaderRole::Cloud)?;
            sent.send(()).unwrap();
            let _ = wait.recv_timeout(WAIT);
            assert_eq!(authority.recheck(), ResourceMode::Full);
            drop(reader);
            done.store(true, Ordering::SeqCst);
            Err(EngineError::Cancelled)
        },
    );
    ready.recv_timeout(WAIT).unwrap();
    let failed = h.engine.suspend(Duration::from_millis(40));
    let retired = released.load(Ordering::SeqCst);
    let retained = h.engine.jobs().running().iter().any(|j| j.id == job.id);
    drop(release);
    assert!(matches!(failed, Err(EngineError::SuspendDeadline { .. })));
    assert!(!retired && retained);
    assert_eq!(outcome(&h.engine, &sub, job.id), JobOutcome::Cancelled);
    h.engine.close().unwrap();
    assert!(released.load(Ordering::SeqCst));
}

#[test]
fn resource_jobs_queued_full_rechecks_real_replacement_and_current_executor_generation() {
    let h = Harness::open();
    let occupied = Occupied::new(&h.engine);
    let sub = h.engine.stream().subscribe();
    let stores = Arc::new(AtomicUsize::new(0));
    let spy = stores.clone();
    let old = h.engine.original_resource_authority(&h.collection, live());
    let job = h
        .engine
        .jobs()
        .start_original_resource("Full before real replacement", old, move |_, mut authority| {
            let mode = authority.recheck();
            if mode == ResourceMode::Full {
                spy.fetch_add(1, Ordering::SeqCst);
            }
            assert_eq!(mode, ResourceMode::DisposalOnly);
            Ok(())
        });
    let replacement = h.dir.path().join("resource-replacement.anki2");
    std::fs::copy(common::fixture("schema-18.anki2"), &replacement).unwrap();
    // The calling thread owns its borrowed guards; no guard enters a static job.
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    exclusive
        .replace(move |live| {
            std::fs::rename(&replacement, live).map_err(|error| EngineError::Internal {
                message: error.to_string(),
            })
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    assert_ne!(h.collection.generation(), h.engine.generation());
    occupied.release();
    assert_eq!(outcome(&h.engine, &sub, job.id), JobOutcome::Succeeded);
    assert_eq!(stores.load(Ordering::SeqCst), 0);
    let current = h.engine.collection().unwrap();
    let job = h.engine.jobs().start_original_resource(
        "new capability after replacement",
        h.engine.original_resource_authority(&current, live()),
        |_, mut authority| {
            assert_eq!(authority.recheck(), ResourceMode::Full);
            Ok(())
        },
    );
    assert_eq!(outcome(&h.engine, &sub, job.id), JobOutcome::Succeeded);
}

#[test]
fn resource_jobs_queued_full_keeps_early_drain_authority_across_pause() {
    let returned = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let h = Harness::open_with(Ports::new().suspend_participant(Arc::new(ResourceParticipant {
        returned: returned.clone(),
        released: released.clone(),
    })));
    let occupied = Occupied::new(&h.engine);
    let done = returned.clone();
    let job = h.engine.jobs().start_original_resource(
        "queued before pause",
        h.engine.original_resource_authority(&h.collection, live()),
        move |ctx, mut authority| {
            assert!(ctx.cancel.is_cancelled());
            assert_eq!(authority.recheck(), ResourceMode::Full);
            done.store(true, Ordering::SeqCst);
            Ok(())
        },
    );
    std::thread::scope(|scope| {
        let suspend = scope.spawn(|| h.engine.suspend(Duration::from_secs(5)));
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
        while !job.cancel.is_cancelled() && meguri_engine::monotonic::now() < deadline {
            std::thread::yield_now();
        }
        occupied.release();
        assert!(job.cancel.is_cancelled());
        assert_eq!(
            suspend.join().unwrap().unwrap(),
            meguri_engine::SuspendOutcome::Suspended
        );
    });
    assert!(returned.load(Ordering::SeqCst) && released.load(Ordering::SeqCst));
}
