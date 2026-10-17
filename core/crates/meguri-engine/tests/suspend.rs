mod common;
use meguri_engine::{OpSpec, SuspendOutcome};
use meguri_model::changes::OpSource;
use std::time::Duration;

#[test]
fn suspend_closes_connections_and_resume_keeps_undo() {
    let h = common::Harness::open();
    let (nid, _) = common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["durable", "back"]).value;
    assert_eq!(
        h.engine.suspend(Duration::from_secs(2)).unwrap(),
        SuspendOutcome::Suspended
    );
    assert!(h.engine.threads().busy_threads().is_empty());
    assert!(!h.path.with_extension("anki2-wal").exists());
    assert!(
        h.collection
            .run(OpSpec::named("blocked", OpSource::UserUndoable), |_| Ok(()))
            .is_err()
    );
    let report = h.engine.resume().unwrap();
    assert!(!report.external_change);
    assert!(!report.rolled_over);
    assert_eq!(common::note_fields(&h.collection, nid), "durable\u{1f}back");
    h.engine.close().unwrap();
}

#[test]
fn repeated_suspend_does_not_lose_external_changes() {
    let h = common::Harness::open();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&h.path).unwrap();
    db.execute_batch("UPDATE notes SET flds = 'offline'").unwrap();
    drop(db);
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    assert!(h.engine.resume().unwrap().external_change);
}

#[test]
fn a_continued_download_defers_suspension_and_other_jobs_are_cancelled() {
    use meguri_engine::{EngineError, JobKind};
    use std::sync::mpsc;
    let h = common::Harness::open();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (finish_tx, finish_rx) = mpsc::sync_channel(1);
    let job = h
        .engine
        .start_continued_job(JobKind::OneWaySync, move |ctx| {
            entered_tx.send(()).unwrap();
            finish_rx.recv().unwrap();
            assert!(!ctx.cancel.is_cancelled());
            Ok(())
        })
        .unwrap();
    entered_rx.recv().unwrap();
    let other = h
        .engine
        .start_job(JobKind::NormalSync, |ctx| {
            while !ctx.cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(EngineError::Cancelled)
        })
        .unwrap();
    assert_eq!(
        h.engine.suspend(Duration::from_secs(2)).unwrap(),
        SuspendOutcome::Deferred { job_id: job.id }
    );
    assert!(!job.cancel.is_cancelled());
    assert!(other.cancel.is_cancelled());
    finish_tx.send(()).unwrap();
    while h.engine.jobs().running().iter().any(|j| j.id == job.id) {
        std::thread::yield_now();
    }
    assert_eq!(
        h.engine.suspend(Duration::from_secs(2)).unwrap(),
        SuspendOutcome::Suspended
    );
    assert!(h.engine.threads().busy_threads().is_empty());
}

#[test]
fn flush_participant_commits_five_pending_ops_and_following_edit_is_accepted() {
    use meguri_engine::{Collection, EngineError, Ports, SuspendParticipant};
    use std::sync::{Arc, OnceLock};
    #[derive(Default)]
    struct Pending {
        collection: OnceLock<Collection>,
        flushed: std::sync::atomic::AtomicBool,
    }
    impl SuspendParticipant for Pending {
        fn name(&self) -> &str {
            "pending test ops"
        }
        fn flush(&self) -> Result<(), EngineError> {
            if !self.flushed.swap(true, std::sync::atomic::Ordering::SeqCst) {
                for i in 0..5 {
                    self.collection.get().unwrap().run(
                        OpSpec::named("pending", OpSource::UserUndoable),
                        move |ctx| {
                            ctx.scope()
                                .add_note(&common::note(&[&format!("pending-{i}"), "back"]))?;
                            Ok(())
                        },
                    )?;
                }
            }
            Ok(())
        }
    }
    let participant = Arc::new(Pending::default());
    let h = common::Harness::open_with(Ports::new().suspend_participant(participant.clone()));
    participant.collection.set(h.collection.clone()).unwrap();
    let before = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    h.engine.flush_for_background().unwrap();
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes"),
        before + 5
    );
    common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["following", "edit"]);
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes"),
        before + 6
    );
    h.engine.close().unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&h.path).unwrap();
    let mut count = 0;
    db.for_each("SELECT count(*) FROM notes WHERE flds LIKE 'pending-%'", &[], |r| {
        if let meguri_storage::raw::RawValue::Integer(n) = r[0] {
            count = n;
        }
    })
    .unwrap();
    assert_eq!(count, 5);
}

#[test]
fn held_reader_times_out_and_resume_recovers() {
    use meguri_engine::{EngineError, monotonic};
    let h = common::Harness::open();
    let lease = h.collection.reader(meguri_storage::ReaderRole::Sync).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let start = monotonic::now();
    assert!(matches!(
        h.engine.suspend(Duration::from_millis(30)),
        Err(EngineError::SuspendDeadline { .. })
    ));
    assert!(monotonic::now() - start < Duration::from_millis(200));
    drop(snapshot);
    drop(lease);
    h.engine.resume().unwrap();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
}

#[test]
fn five_queued_ops_and_cancelled_job_suspend_before_deadline_at_150k() {
    use meguri_engine::{EngineError, JobKind, monotonic};
    let fixture = meguri_testkit::bench::fixtures::profile_collection("150k").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(fixture.dir.join("collection.anki2"), &path).unwrap();
    let h = common::Harness::open_path(dir, path);
    let before = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    let gate = common::Gate::new();
    let blocker = common::block_writer(&h.collection, OpSource::UserUndoable, &gate);
    let mut pending = Vec::new();
    for i in 0..5 {
        pending.push(
            h.collection
                .submit(OpSpec::named("pending", OpSource::UserUndoable), move |ctx| {
                    ctx.scope().add_note(&common::note(&[&format!("queued-{i}"), "back"]))?;
                    Ok(())
                }),
        );
    }
    let job = h
        .engine
        .start_job(JobKind::NormalSync, |ctx| {
            while !ctx.cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(EngineError::Cancelled)
        })
        .unwrap();
    let start = monotonic::now();
    gate.open();
    assert_eq!(
        h.engine.suspend(Duration::from_secs(2)).unwrap(),
        SuspendOutcome::Suspended
    );
    assert!(monotonic::now() - start < Duration::from_secs(2));
    blocker.wait().unwrap();
    for op in pending {
        op.wait().unwrap();
    }
    assert!(job.cancel.is_cancelled());
    assert!(h.engine.threads().threads().iter().all(|t| !t.busy));
    assert!(!h.path.with_extension("anki2-wal").exists());
    // A foreign writer can immediately acquire an exclusive transaction:
    // every reader and writer connection has released its SQLite locks.
    let db = meguri_storage::raw::PlainDatabase::open(&h.path).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE; ROLLBACK").unwrap();
    drop(db);
    h.engine.resume().unwrap();
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes"),
        before + 5
    );
}

#[test]
fn busy_uncooperative_thread_prevents_successful_suspend() {
    use meguri_engine::{EngineError, Qos};
    let h = common::Harness::open();
    let gate = common::Gate::new();
    let worker_gate = gate.clone();
    let worker = h
        .engine
        .threads()
        .spawn("test-uncooperative", Qos::Utility, move |activity| {
            let _busy = activity.busy();
            worker_gate.enter_and_wait();
        })
        .unwrap();
    gate.wait_entered();
    assert!(matches!(
        h.engine.suspend(Duration::from_millis(30)),
        Err(EngineError::SuspendDeadline { .. })
    ));
    gate.open();
    worker.join().unwrap();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
}

#[test]
fn an_idle_thread_without_a_registered_participant_fails_enumeration() {
    use meguri_engine::{EngineError, Qos};
    let h = common::Harness::open();
    let (finish, wait) = std::sync::mpsc::channel();
    let worker = h
        .engine
        .threads()
        .spawn("missing-participant", Qos::Utility, move |_| {
            wait.recv().unwrap();
        })
        .unwrap();
    assert!(
        matches!(h.engine.suspend(Duration::from_secs(2)), Err(EngineError::InvalidArgument { message }) if message.contains("missing-participant"))
    );
    finish.send(()).unwrap();
    worker.join().unwrap();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
}

#[test]
fn resume_never_reopens_writes_on_a_corrupt_collection_or_during_replacement() {
    let h = common::Harness::open();
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    assert!(h.engine.resume().is_err());
    assert!(
        h.collection
            .run(OpSpec::named("blocked", OpSource::UserUndoable), |_| Ok(()))
            .is_err()
    );
    drop(exclusive);
    drop(slot);
    h.engine.mark_corrupt("injected corruption");
    assert!(h.engine.resume().is_err());
    assert!(
        h.collection
            .run(OpSpec::named("blocked", OpSource::UserUndoable), |_| Ok(()))
            .is_err()
    );
    h.engine.close().unwrap();
}

#[test]
fn capacity_provider_is_injected_at_open_and_checked_before_large_work() {
    use meguri_engine::{DiskSpaceProvider, Engine, EngineConfig, EngineError};
    use meguri_storage::StorageError;
    use std::sync::Arc;
    struct LowSpace;
    impl DiskSpaceProvider for LowSpace {
        fn available_bytes(&self, _: &std::path::Path) -> Result<u64, StorageError> {
            Ok(19)
        }
    }
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let clock = Arc::new(meguri_model::clock::StepClock::new(
        common::NOW_MS,
        meguri_model::clock::UtcOffset::UTC,
        "UTC",
    ));
    let collection = engine
        .open_raw(
            &path,
            common::open_config(&clock).disk_space_provider(Arc::new(LowSpace)),
        )
        .unwrap();
    assert_eq!(
        collection.require_space(20),
        Err(EngineError::Storage(StorageError::InsufficientSpace {
            required: 20,
            available: 19
        }))
    );
    collection.require_space(19).unwrap();
    engine.close().unwrap();
}

#[test]
fn direct_job_registry_starts_cannot_escape_the_suspended_admission_gate() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let h = common::Harness::open();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let inner = ran.clone();
    let job = h
        .engine
        .jobs()
        .start("late worker", h.collection.generation(), None, move |_| {
            inner.store(true, Ordering::SeqCst);
            Ok(())
        });
    assert!(job.cancel.is_cancelled());
    assert!(!ran.load(Ordering::SeqCst));
    assert!(h.engine.jobs().running().is_empty());
    assert!(h.engine.threads().busy_threads().is_empty());
    h.engine.resume().unwrap();
    let inner = ran.clone();
    h.engine
        .start_job(meguri_engine::JobKind::Export, move |_| {
            inner.store(true, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
}

#[test]
fn failed_flush_closes_admission_and_retry_flushes_pending_work() {
    use meguri_engine::{Collection, EngineError, Ports, SuspendParticipant};
    use std::sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    };
    #[derive(Default)]
    struct Pending {
        fail_once: AtomicBool,
        collection: OnceLock<Collection>,
    }
    impl SuspendParticipant for Pending {
        fn name(&self) -> &str {
            "retrying flush"
        }
        fn flush(&self) -> Result<(), EngineError> {
            if self.fail_once.swap(false, Ordering::SeqCst) {
                return Err(EngineError::SuspendDeadline {
                    phase: self.name().into(),
                });
            }
            self.collection
                .get()
                .unwrap()
                .run(OpSpec::named("pending", OpSource::UserUndoable), |ctx| {
                    ctx.scope().add_note(&common::note(&["retried flush", "back"]))?;
                    Ok(())
                })?;
            Ok(())
        }
    }
    let pending = Arc::new(Pending::default());
    pending.fail_once.store(true, Ordering::SeqCst);
    let h = common::Harness::open_with(Ports::new().suspend_participant(pending.clone()));
    pending.collection.set(h.collection.clone()).unwrap();
    assert!(matches!(
        h.engine.suspend(Duration::from_secs(2)),
        Err(EngineError::SuspendDeadline { .. })
    ));
    assert!(
        h.collection
            .run(OpSpec::named("late edit", OpSource::UserUndoable), |_| Ok(()))
            .is_err()
    );
    assert!(
        !meguri_storage::sidecar::Sidecar::load(&h.dir.path().join("meguri-sidecar.json"))
            .unwrap()
            .clean_shutdown
    );
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    h.engine.resume().unwrap();
    assert_eq!(
        common::query_int(
            &h.collection,
            "SELECT count(*) FROM notes WHERE flds = 'retried flush' || char(31) || 'back'"
        ),
        1
    );
    h.engine.close().unwrap();
}

#[test]
fn close_cancels_jobs_and_drains_writes_before_quiescing_participants() {
    use meguri_engine::{Collection, EngineError, JobKind, Ports, SuspendParticipant};
    use std::sync::{Arc, OnceLock};
    #[derive(Default)]
    struct Participant {
        collection: OnceLock<Collection>,
        cancel: OnceLock<Arc<meguri_engine::CancelToken>>,
    }
    impl SuspendParticipant for Participant {
        fn name(&self) -> &str {
            "close ordering"
        }
        fn flush(&self) -> Result<(), EngineError> {
            Ok(())
        }
        fn suspend(&self) -> Result<(), EngineError> {
            assert!(self.cancel.get().unwrap().is_cancelled(), "cancel before quiescence");
            assert!(
                self.collection
                    .get()
                    .unwrap()
                    .run(OpSpec::named("too late", OpSource::UserUndoable), |_| Ok(()))
                    .is_err(),
                "close admission before quiescence"
            );
            Ok(())
        }
    }
    let participant = Arc::new(Participant::default());
    let h = common::Harness::open_with(Ports::new().suspend_participant(participant.clone()));
    participant.collection.set(h.collection.clone()).unwrap();
    let job = h
        .engine
        .start_job(JobKind::NormalSync, |ctx| {
            while !ctx.cancel.is_cancelled() {
                std::thread::yield_now();
            }
            Err(EngineError::Cancelled)
        })
        .unwrap();
    participant.cancel.set(job.cancel).unwrap();
    h.engine.close().unwrap();
}

#[test]
fn external_snapshot_blocks_checkpoint_without_marking_shutdown_clean() {
    use meguri_model::rng::SeededRandom;
    use meguri_storage::{OpenOptions, ReaderRole, open_collection};
    let h = common::Harness::open();
    let random = SeededRandom::new(99);
    let foreign = open_collection(&h.path, &OpenOptions::new(h.clock.as_ref(), &random)).unwrap();
    let reader = foreign.open_reader(ReaderRole::Sync).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    use meguri_storage::ReadRows;
    snapshot.col().unwrap();
    common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["before checkpoint", "back"]);
    let began = meguri_engine::monotonic::now();
    assert!(h.engine.suspend(Duration::from_millis(30)).is_err());
    assert!(meguri_engine::monotonic::now() - began < Duration::from_millis(200));
    assert!(
        !meguri_storage::sidecar::Sidecar::load(&h.dir.path().join("meguri-sidecar.json"))
            .unwrap()
            .clean_shutdown
    );
    drop(snapshot);
    drop(reader);
    foreign.close().unwrap();
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    assert!(
        meguri_storage::sidecar::Sidecar::load(&h.dir.path().join("meguri-sidecar.json"))
            .unwrap()
            .clean_shutdown
    );
    h.engine.resume().unwrap();
    h.engine.close().unwrap();
}

#[test]
fn replacement_never_runs_a_swap_after_collection_close_fails() {
    use meguri_model::faultpoint::{self, Fault};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let h = common::Harness::open();
    let (nid, _) = common::steps::setup_note(&h.collection, "preserved");
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let swapped = Arc::new(AtomicBool::new(false));
    let observed = swapped.clone();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
    let result = exclusive.replace(move |_| {
        observed.store(true, Ordering::SeqCst);
        Ok(())
    });
    faultpoint::disarm_all();
    assert!(result.is_err());
    assert!(!swapped.load(Ordering::SeqCst), "never swap after failed close");
    drop(exclusive);
    drop(slot);
    h.engine.close().unwrap();
    let collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(common::note_fields(&collection, nid), "preserved\u{1f}back");
    h.engine.close().unwrap();
}

#[test]
fn launch_cleans_media_temps_without_following_links_or_descending() {
    use std::os::unix::fs::symlink;
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let media = dir.path().join("collection.media");
    std::fs::create_dir(&media).unwrap();
    let temp = media.join(".meguri-tmp-media-killed");
    std::fs::write(&temp, b"partial").unwrap();
    std::fs::write(media.join("photo.jpg"), b"published").unwrap();
    let nested = media.join("album");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join(".meguri-tmp-media-kept"), b"nested").unwrap();
    symlink(
        nested.join(".meguri-tmp-media-kept"),
        media.join(".meguri-tmp-media-link"),
    )
    .unwrap();
    let h = common::Harness::open_path(dir, path);
    assert!(!temp.exists());
    assert_eq!(std::fs::read(media.join("photo.jpg")).unwrap(), b"published");
    assert!(nested.join(".meguri-tmp-media-kept").exists());
    assert!(
        media
            .join(".meguri-tmp-media-link")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    h.engine.close().unwrap();
}

#[test]
fn suspend_during_a_slow_open_returns_within_its_budget() {
    use meguri_engine::{Collection, Engine, EngineConfig, EngineError, OpenStep, Ports};
    use std::sync::{Arc, Mutex, mpsc};
    struct SlowOpen(mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>);
    impl OpenStep for SlowOpen {
        fn name(&self) -> &str {
            "slow open"
        }
        fn run(&self, _: &Collection) -> Result<(), EngineError> {
            self.0.send(()).unwrap();
            self.1.lock().unwrap().recv().unwrap();
            Ok(())
        }
    }
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let (entered, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let opener = engine.clone();
    let opening = std::thread::spawn(move || {
        let clock = Arc::new(meguri_model::clock::StepClock::new(
            common::NOW_MS,
            meguri_model::clock::UtcOffset::UTC,
            "UTC",
        ));
        opener.open_raw(
            &path,
            common::open_config(&clock)
                .ports(Ports::default().open_step(Arc::new(SlowOpen(entered, Mutex::new(wait))))),
        )
    });
    ready.recv().unwrap();
    let watchdog = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        release.send(()).unwrap();
    });
    let started = meguri_engine::monotonic::now();
    let result = engine.suspend(Duration::from_millis(30));
    let elapsed = meguri_engine::monotonic::now() - started;
    watchdog.join().unwrap();
    opening.join().unwrap().unwrap();
    engine.close().unwrap();
    assert!(result.is_err());
    assert!(elapsed < Duration::from_millis(200), "suspend blocked for {elapsed:?}");
}

#[test]
fn a_dispatcher_that_only_delivers_values_allows_suspend_and_close() {
    use meguri_engine::Qos;
    let h = common::Harness::open();
    let (stop, wait) = std::sync::mpsc::channel();
    let worker = h
        .engine
        .threads()
        .spawn_owned("meguri-events", Qos::UserInitiated, "events", move |_| {
            wait.recv().unwrap();
        })
        .unwrap();
    let result = h.engine.suspend(Duration::from_secs(2));
    let closed = if result.is_ok() { h.engine.close() } else { Ok(()) };
    // Always release the test worker, including on a failed assertion.
    stop.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(result.unwrap(), SuspendOutcome::Suspended);
    closed.unwrap();
}

#[test]
fn reentrant_job_starts_during_control_calls_are_refused_promptly() {
    use meguri_engine::{Engine, EngineError, JobKind, Ports, SuspendParticipant};
    use std::sync::Arc;
    struct StartsJob(Engine);
    impl SuspendParticipant for StartsJob {
        fn name(&self) -> &str {
            "job starter"
        }
        fn flush(&self) -> Result<(), EngineError> {
            assert!(self.0.start_job(JobKind::NormalSync, |_| Ok(())).is_err());
            assert!(self.0.start_continued_job(JobKind::OneWaySync, |_| Ok(())).is_err());
            Ok(())
        }
    }
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let engine = Engine::new(meguri_engine::EngineConfig::default()).unwrap();
    let clock = Arc::new(meguri_model::clock::StepClock::new(
        common::NOW_MS,
        meguri_model::clock::UtcOffset::UTC,
        "UTC",
    ));
    engine
        .open_raw(
            &path,
            common::open_config(&clock)
                .ports(Ports::default().suspend_participant(Arc::new(StartsJob(engine.clone())))),
        )
        .unwrap();
    let (finished, outcome) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let suspended = engine.suspend(Duration::from_secs(2));
        let closed = engine.close();
        finished.send((suspended, closed)).unwrap();
    });
    let (suspended, closed) = outcome
        .recv_timeout(Duration::from_secs(3))
        .expect("job registration deadlocked the control lane");
    suspended.unwrap();
    closed.unwrap();
    worker.join().unwrap();
    drop(dir);
}
