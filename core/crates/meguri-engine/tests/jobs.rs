//! Jobs, arbitration and threads (`engine/jobs` J1 to J3, J6; R171; KTD9).

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

use common::{Gate, Harness, drain};
use meguri_engine::{
    ChangeStream, EngineError, EventKind, JobId, JobKind, JobOutcome, JobRegistry, Qos, ThreadRegistry, monotonic,
};

/// The outcome of every `JobFinished` among `events`, by job.
fn finished(events: &[Arc<meguri_engine::Event>]) -> BTreeMap<JobId, Vec<JobOutcome>> {
    let mut out: BTreeMap<JobId, Vec<JobOutcome>> = BTreeMap::new();
    for event in events {
        if let EventKind::JobFinished { id, outcome } = &event.kind {
            out.entry(*id).or_default().push(outcome.clone());
        }
    }
    out
}

#[test]
fn normal_and_media_sync_run_together() {
    let h = Harness::open();
    let barrier = Arc::new(Barrier::new(3));
    let (a, b) = (barrier.clone(), barrier.clone());
    let normal = h
        .engine
        .start_job(JobKind::NormalSync, move |_| {
            a.wait();
            Ok(())
        })
        .unwrap();
    let media = h
        .engine
        .start_job(JobKind::MediaSync, move |_| {
            b.wait();
            Ok(())
        })
        .unwrap();
    // Both are running at once, or this would wait forever.
    barrier.wait();
    assert_ne!(normal.id, media.id);
}

#[test]
fn check_database_while_a_colpkg_import_holds_the_slot_is_busy_and_the_slot_is_free_once_it_finishes() {
    let h = Harness::open();
    let subscription = h.engine.stream().subscribe();
    let gate = Gate::new();
    let inner = gate.clone();
    let import = h
        .engine
        .start_job(JobKind::ColpkgImport, move |ctx| {
            assert!(ctx.slot().is_some());
            inner.enter_and_wait();
            Ok(())
        })
        .unwrap();
    gate.wait_entered();
    let busy = h.engine.start_job(JobKind::CheckDatabase, |_| Ok(())).unwrap_err();
    assert_eq!(
        busy,
        EngineError::Busy {
            running_op: ".colpkg import".to_owned()
        }
    );
    gate.open();
    // The slot is released before JobFinished is published.
    let event = loop {
        let event = subscription.recv_timeout(Duration::from_secs(10)).expect("JobFinished");
        if matches!(event.kind, EventKind::JobFinished { .. }) {
            break event;
        }
    };
    assert!(matches!(&event.kind, EventKind::JobFinished { id, outcome: JobOutcome::Succeeded } if *id == import.id));
    assert!(h.engine.start_job(JobKind::CheckDatabase, |_| Ok(())).is_ok());
}

#[test]
fn every_job_ends_with_exactly_one_job_finished() {
    let h = Harness::open();
    let subscription = h.engine.stream().subscribe();
    let ok = h.engine.start_job(JobKind::Export, |_| Ok(())).unwrap();
    let failed = h
        .engine
        .start_job(JobKind::Backup, |_| {
            Err(EngineError::InvalidArgument {
                message: "no space".into(),
            })
        })
        .unwrap();
    let panicked = h.engine.start_job(JobKind::EmptyCards, |_| panic!("job bug")).unwrap();
    let cancelled = h
        .engine
        .start_job(JobKind::CheckMedia, |ctx| {
            loop {
                ctx.cancel.check()?;
                thread::sleep(Duration::from_millis(1));
            }
        })
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    assert!(h.engine.jobs().cancel(cancelled.id));
    let events = drain(&subscription, Duration::from_millis(300));
    let outcomes = finished(&events);
    assert_eq!(outcomes[&ok.id], [JobOutcome::Succeeded]);
    assert_eq!(
        outcomes[&failed.id],
        [JobOutcome::Failed {
            error: EngineError::InvalidArgument {
                message: "no space".into()
            }
        }]
    );
    assert!(matches!(
        &outcomes[&panicked.id][..],
        [JobOutcome::Failed { error: EngineError::Internal { message } }] if message.contains("job bug")
    ));
    assert_eq!(outcomes[&cancelled.id], [JobOutcome::Cancelled]);
    assert_eq!(outcomes.len(), 4);
    assert!(
        !h.engine.jobs().cancel(cancelled.id),
        "a finished job is no longer running"
    );
    assert!(h.engine.jobs().running().is_empty());
}

#[test]
fn the_pool_is_bounded_and_a_job_cancelled_while_it_waits_never_runs() {
    let threads = ThreadRegistry::new();
    let stream = ChangeStream::new();
    let subscription = stream.subscribe();
    let registry = JobRegistry::new(&threads, stream.clone(), 2).unwrap();
    let gate = Gate::new();
    let running = Arc::new(Mutex::new(0usize));
    let peak = Arc::new(Mutex::new(0usize));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (gate, running, peak) = (gate.clone(), running.clone(), peak.clone());
        handles.push(
            registry.start("blocking", meguri_engine::Generation(0), None, move |_| {
                {
                    let mut now = running.lock().unwrap();
                    *now += 1;
                    let mut top = peak.lock().unwrap();
                    *top = (*top).max(*now);
                }
                gate.enter_and_wait();
                *running.lock().unwrap() -= 1;
                Ok(())
            }),
        );
    }
    let ran = Arc::new(Mutex::new(false));
    let flag = ran.clone();
    let waiting = registry.start("waiting", meguri_engine::Generation(0), None, move |_| {
        *flag.lock().unwrap() = true;
        Ok(())
    });
    gate.wait_entered();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(*peak.lock().unwrap(), 2, "two workers");
    assert!(registry.cancel(waiting.id));
    gate.open();
    let events = drain(&subscription, Duration::from_millis(500));
    let outcomes = finished(&events);
    assert_eq!(outcomes.len(), 5);
    assert_eq!(outcomes[&waiting.id], [JobOutcome::Cancelled]);
    assert!(!*ran.lock().unwrap());
    let workers: Vec<_> = threads
        .threads()
        .into_iter()
        .filter(|t| t.name.starts_with("meguri-job-"))
        .collect();
    assert_eq!(workers.len(), 2);
    assert!(workers.iter().all(|t| t.qos == Qos::UserInitiated));
}

#[test]
fn every_registered_thread_reports_its_qos_class() {
    let h = Harness::open();
    // U26's derived writer runs at utility; this stands in for it.
    let stop = Gate::new();
    let inner = stop.clone();
    let derived = h
        .engine
        .threads()
        .spawn("meguri-derived-writer", Qos::Utility, move |_| inner.enter_and_wait())
        .unwrap();
    let threads = h.engine.threads().threads();
    let qos_of = |name: &str| {
        threads
            .iter()
            .find(|t| t.name == name)
            .map(|t| (t.qos, t.reported_qos))
            .unwrap_or_else(|| panic!("{name} is not registered: {threads:?}"))
    };
    let (writer, writer_reported) = qos_of("meguri-writer");
    let (derived_qos, derived_reported) = qos_of("meguri-derived-writer");
    assert_eq!(writer, Qos::UserInitiated);
    assert_eq!(derived_qos, Qos::Utility);
    if cfg!(target_vendor = "apple") {
        assert_eq!(writer_reported, Some(Qos::UserInitiated));
        assert_eq!(derived_reported, Some(Qos::Utility));
    } else {
        assert_eq!(
            (writer_reported, derived_reported),
            (None, None),
            "no QoS API: recorded only"
        );
    }
    stop.open();
    derived.join().unwrap();
    assert!(
        h.engine
            .threads()
            .threads()
            .iter()
            .all(|t| t.name != "meguri-derived-writer"),
        "an ended thread leaves the list"
    );
}

#[test]
fn a_thread_another_runtime_started_is_listed_while_it_is_registered() {
    // J6: a thread the core did not spawn through the registry (the sync
    // runtime's tokio threads) lists itself from its own start hook.
    let threads = ThreadRegistry::new();
    let listing = threads.clone();
    let (listed, busy, idle, after) = thread::spawn(move || {
        let registered = listing.register_current("runtime-thread", Qos::UserInitiated);
        let listed = listing.threads();
        registered.set_busy(true);
        let busy = listing.busy_threads();
        registered.set_busy(false);
        let idle = listing.busy_threads();
        drop(registered);
        (listed, busy, idle, listing.threads())
    })
    .join()
    .unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].name, "runtime-thread");
    assert_eq!(listed[0].qos, Qos::UserInitiated);
    let reported = if cfg!(target_vendor = "apple") {
        Some(Qos::UserInitiated)
    } else {
        None
    };
    assert_eq!(listed[0].reported_qos, reported);
    assert!(!listed[0].busy, "listed idle");
    assert_eq!(busy.len(), 1);
    assert!(idle.is_empty());
    assert!(after.is_empty(), "dropped, it leaves the list: {after:?}");
    assert!(threads.threads().is_empty());
}

#[test]
fn a_busy_thread_is_listed_busy_until_its_work_ends() {
    let h = Harness::open();
    let gate = Gate::new();
    let blocker = common::block_writer(&h.collection, meguri_model::changes::OpSource::UserUndoable, &gate);
    let busy: Vec<String> = h.engine.threads().busy_threads().into_iter().map(|t| t.name).collect();
    assert!(busy.contains(&"meguri-writer".to_owned()), "{busy:?}");
    gate.open();
    blocker.wait().unwrap();
    let deadline = monotonic::now() + Duration::from_secs(5);
    while !h.engine.threads().busy_threads().is_empty() {
        assert!(
            monotonic::now() < deadline,
            "threads stay busy: {:?}",
            h.engine.threads().busy_threads()
        );
        thread::sleep(Duration::from_millis(5));
    }
}
