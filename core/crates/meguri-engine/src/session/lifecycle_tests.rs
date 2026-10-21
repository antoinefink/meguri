//! Private admission state must stay unlocked during real pending progress.
use super::*;
use crate::{EngineConfig, OpenConfig, Ports, Progress, ProgressListener, ProgressPhase, ProgressSink};
use meguri_model::{
    clock::{Clock, StepClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::ConfigValue;

#[test]
fn lifecycle_try_admission_refuses_mutex_contention_and_logical_occupancy() {
    let gate = LifecycleGate::default();
    let state = gate.occupied.lock().unwrap();
    assert!(gate.try_acquire().is_err());
    drop(state);
    let permit = gate.acquire();
    assert!(*gate.occupied.try_lock().expect("permit must not own a mutex guard"));
    assert!(gate.try_acquire().is_err());
    drop(permit);
    assert!(gate.try_acquire().is_ok());
    let result = std::panic::catch_unwind(|| {
        let _permit = gate.acquire();
        panic!("admitted work unwound");
    });
    assert!(result.is_err());
    assert!(gate.try_acquire().is_ok());
}

#[test]
fn host_close_without_a_session_reserves_shutdown_and_releases_every_exit() {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let review = ReviewService::new(&engine, Arc::new(QueueProvider::default())).unwrap();
    let permit = review.control.acquire();
    assert_eq!(
        review.try_close_host(
            || panic!("busy close entered shutdown"),
            || panic!("busy close finished")
        ),
        Err(EngineError::Busy {
            running_op: "review lifecycle".into()
        })
    );
    drop(permit);
    for exit in 0..3 {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            review.try_close_host(
                || {
                    {
                        let occupied = review
                            .control
                            .occupied
                            .try_lock()
                            .expect("shutdown holds admission mutex");
                        assert!(*occupied);
                    }
                    assert!(review.inner.state.try_lock().is_ok(), "shutdown holds session mutex");
                    assert!(review.control.try_acquire().is_err());
                    match exit {
                        0 => Err(EngineError::unavailable("shutdown failure")),
                        1 => panic!("shutdown unwind"),
                        _ => Ok(()),
                    }
                },
                || {
                    assert_eq!(exit, 2, "failed shutdown published closed");
                    assert!(review.inner.state.try_lock().is_ok());
                    assert!(
                        review.control.try_acquire().is_err(),
                        "close released before host transition"
                    );
                },
            )
        }));
        match exit {
            0 => assert_eq!(result.unwrap(), Err(EngineError::unavailable("shutdown failure"))),
            1 => assert!(result.is_err()),
            _ => assert_eq!(result.unwrap(), Ok(())),
        }
        assert!(review.control.try_acquire().is_ok(), "close exit retained admission");
    }
}

struct InspectCommit {
    review: Weak<ReviewService>,
    scope: UndoScope,
    panic: bool,
    seen: AtomicBool,
}
impl ProgressListener for InspectCommit {
    fn on_progress(&self, progress: &Progress) {
        assert_eq!(progress.phase, ProgressPhase::Committing);
        let review = self.review.upgrade().unwrap();
        {
            let occupied = review
                .control
                .occupied
                .try_lock()
                .expect("admission mutex held in progress");
            assert!(*occupied, "lifecycle exclusion must survive delivery");
            let state = review.inner.state.try_lock().expect("session mutex held in progress");
            let session = state.session.as_ref().unwrap();
            assert!(
                session.presentation.try_lock().is_ok(),
                "presentation mutex held in progress"
            );
        }
        // This is the short FFI admission path, called synchronously from the
        // listener: it must refuse without waiting on the current owner.
        assert!(review.try_pending_restore(self.scope, false).is_err());
        self.seen.store(true, Ordering::Release);
        assert!(!self.panic, "injected listener unwind");
    }
}

/// Releases the parked writer even when a listener assertion unwinds.
struct ReleaseWriter(Arc<(Mutex<bool>, Condvar)>);
impl Drop for ReleaseWriter {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_one();
    }
}

#[test]
fn pending_committing_listener_holds_only_admission_and_all_exits_release_it() {
    for redo in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
        crate::bootstrap::create(&path, clock.as_ref(), &SeededRandom::new(43)).unwrap();
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let queues = Arc::new(QueueProvider::default());
        let review = ReviewService::new(&engine, queues.clone()).unwrap();
        let ports = Ports::new()
            .observer(queues.clone())
            .generation_listener(queues)
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone());
        let c = engine
            .open_raw(
                &path,
                OpenConfig::new(clock.clone(), Arc::new(SeededRandom::new(43))).ports(ports),
            )
            .unwrap();
        c.run(OpSpec::named("pending callback policy", OpSource::Maintenance), |ctx| {
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
            ctx.scope()
                .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
        let nt = {
            let reader = c.reader(ReaderRole::Query).unwrap();
            crate::notetypes::read::names(&*reader)
                .unwrap()
                .into_iter()
                .find(|(_, name)| name == "Basic")
                .unwrap()
                .0
        };
        crate::notes::add(
            &c,
            crate::notes::AddNote {
                notetype_id: nt,
                fields: vec!["pending callback".into(), "answer".into()],
                tags: Vec::new(),
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
        let StartResult::Started {
            session_id: id, head, ..
        } = review.start(DeckId(1), None).unwrap()
        else {
            panic!("session refused");
        };
        let scope = UndoScope::Session(id);
        let parked = Arc::new((Mutex::new(false), Condvar::new()));
        let writer_release = ReleaseWriter(parked.clone());
        let (entered, wait) = mpsc::sync_channel(1);
        let blocking = c.submit(
            OpSpec::named("pending callback blocker", OpSource::Rollover),
            move |_| {
                entered.send(()).unwrap();
                let mut released = parked.0.lock().unwrap();
                while !*released {
                    released = parked.1.wait(released).unwrap();
                }
                Ok(())
            },
        );
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let card = &head.items[0];
        assert!(matches!(
            review
                .answer(
                    id,
                    card.card_id,
                    Rating::Easy,
                    card.state_token,
                    EpochMillis(clock.now_ms()),
                    100
                )
                .unwrap(),
            AnswerOutcome::Accepted { .. }
        ));
        if redo {
            review.try_pending_restore(scope, false).unwrap().unwrap();
        }
        let cancelled = crate::CancelToken::new();
        cancelled.cancel();
        assert!(matches!(
            review.restore_controlled(scope, redo, cancelled, ProgressSink::new()),
            Err(EngineError::Cancelled)
        ));
        assert!(review.control.try_acquire().is_ok(), "cancellation releases admission");
        assert!(
            review
                .restore_controlled(
                    UndoScope::Session(SessionId(id.0 + 1)),
                    redo,
                    crate::CancelToken::new(),
                    ProgressSink::new()
                )
                .is_err()
        );
        assert!(review.control.try_acquire().is_ok(), "error releases admission");
        for panic in [true, false] {
            let listener = Arc::new(InspectCommit {
                review: Arc::downgrade(&review),
                scope,
                panic,
                seen: AtomicBool::new(false),
            });
            let progress = ProgressSink::new();
            progress.set_listener(listener.clone());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                review.restore_controlled(scope, redo, crate::CancelToken::new(), progress)
            }));
            assert!(listener.seen.load(Ordering::Acquire));
            if panic {
                assert!(result.is_err());
            } else {
                assert!(matches!(result.unwrap(), Ok(HistoryRestore::Session(_))));
            }
            assert!(
                review.control.try_acquire().is_ok(),
                "success/unwind releases admission"
            );
        }
        drop(writer_release);
        blocking.wait().unwrap();
        review.flush_for_background().unwrap();
        review.end(id).unwrap();
        engine.close().unwrap();
    }
}
