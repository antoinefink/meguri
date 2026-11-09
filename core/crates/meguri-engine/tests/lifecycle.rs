//! The collection lifecycle (`engine/lifecycle` L1 to L3, L8, L10): states
//! and their events, open steps, refusals, generations and the generation
//! change after a swap, close.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Harness, add_note_with_card, copy_fixture, drain, note, open_config};
use meguri_engine::executor::catalog;
use meguri_engine::{
    Collection, Engine, EngineConfig, EngineError, EventKind, FailedReason, Generation, GenerationListener,
    LifecycleState, LongOp, OpSpec, OpenStep, Ports,
};
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_storage::{OpenError, ReadRows, ReaderRole};

fn lifecycle_events(events: &[Arc<meguri_engine::Event>]) -> Vec<LifecycleState> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Lifecycle(state) => Some(state.clone()),
            _ => None,
        })
        .collect()
}

/// An open step that records what it saw and commits one op.
struct Step {
    engine: Engine,
    saw: Mutex<Option<(LifecycleState, bool)>>,
}

impl OpenStep for Step {
    fn name(&self) -> &str {
        "replay"
    }

    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        let others_refused = self.engine.collection().is_err();
        *self.saw.lock().unwrap() = Some((self.engine.state(), others_refused));
        collection.run(OpSpec::named("replay_answer", OpSource::UserUndoable), |ctx| {
            Ok(ctx.scope().add_note(&note(&["replayed", "x"]))?)
        })?;
        Ok(())
    }
}

#[test]
fn opening_runs_the_open_steps_alone_then_becomes_ready_at_generation_one() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let clock = Arc::new(StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC"));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert_eq!(engine.state(), LifecycleState::NoCollection);
    assert_eq!(engine.generation(), Generation(0));
    let subscription = engine.stream().subscribe();
    let step = Arc::new(Step {
        engine: engine.clone(),
        saw: Mutex::new(None),
    });
    let collection = engine
        .open_raw(
            &path,
            open_config(&clock).ports(Ports::default().open_step(step.clone())),
        )
        .unwrap();
    assert_eq!(collection.generation(), Generation(1));
    assert_eq!(
        engine.state(),
        LifecycleState::Ready {
            generation: Generation(1)
        }
    );
    assert_eq!(*step.saw.lock().unwrap(), Some((LifecycleState::Opening, true)));
    assert_eq!(
        common::query_int(&collection, "SELECT count(*) FROM notes WHERE flds LIKE 'replayed%'"),
        1
    );
    let events = drain(&subscription, Duration::from_millis(100));
    assert_eq!(
        lifecycle_events(&events),
        [
            LifecycleState::Opening,
            LifecycleState::Ready {
                generation: Generation(1)
            }
        ]
    );
    assert!(events.iter().any(|e| matches!(e.kind, EventKind::Available)));
}

#[test]
fn an_upgrade_reports_upgrading_and_a_refused_file_leaves_failed() {
    let (_dir, path) = copy_fixture("schema-17.anki2");
    let clock = Arc::new(StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC"));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let subscription = engine.stream().subscribe();
    engine.open_raw(&path, open_config(&clock)).unwrap();
    let states = lifecycle_events(&drain(&subscription, Duration::from_millis(100)));
    assert!(
        states.contains(&LifecycleState::Upgrading { from: 17, step: 18 }),
        "{states:?}"
    );
    engine.close().unwrap();

    let (_dir, refused) = copy_fixture("ver-19-wal.anki2");
    let error = engine.open_raw(&refused, open_config(&clock)).unwrap_err();
    assert_eq!(error, EngineError::Open(OpenError::TooNew { version: 19 }));
    assert_eq!(
        engine.state(),
        LifecycleState::Failed {
            reason: FailedReason::Open(OpenError::TooNew { version: 19 })
        }
    );
    assert!(engine.collection().is_err());
    // A good file opens after a refusal.
    let collection = engine.open_raw(&path, open_config(&clock)).unwrap();
    assert_eq!(collection.generation(), Generation(2));
}

#[derive(Default)]
struct Listener(Mutex<Vec<(Generation, Generation)>>);

impl GenerationListener for Listener {
    fn name(&self) -> &str {
        "undo-and-derived"
    }

    fn collection_replaced(&self, old: Generation, new: Generation) {
        self.0.lock().unwrap().push((old, new));
    }
}

#[test]
fn a_generation_change_rejects_old_handles_publishes_one_replaced_event_and_reopens_readers() {
    let listener = Arc::new(Listener::default());
    let h = Harness::open_with(Ports::default().generation_listener(listener.clone()));
    let old = h.collection.clone();
    let (nid, _) = add_note_with_card(&old, OpSource::UserUndoable, &["before swap", "x"]).value;
    // The file swapped in: a copy of the fixture, without that note.
    let replacement = h.dir.path().join("replacement.anki2");
    std::fs::copy(common::fixture("schema-18.anki2"), &replacement).unwrap();
    {
        let reader = old.reader(ReaderRole::Query).unwrap();
        assert!(reader.note(nid).unwrap().is_some());
    }
    let subscription = h.engine.stream().subscribe();

    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let swapped_in = replacement.clone();
    let new = exclusive
        .replace(move |live| {
            std::fs::rename(&swapped_in, live).map_err(|e| EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    drop(exclusive);
    drop(slot);

    assert_eq!(new.generation(), Generation(2));
    assert_eq!(*listener.0.lock().unwrap(), [(Generation(1), Generation(2))]);
    let events = drain(&subscription, Duration::from_millis(100));
    assert_eq!(replaced_generations(&events), [Generation(2)]);

    let stale = old
        .run(OpSpec::named("late", OpSource::UserUndoable), |_| Ok(()))
        .unwrap_err();
    assert_eq!(
        stale,
        EngineError::StaleGeneration {
            planned: Generation(1),
            current: Generation(2)
        }
    );
    assert!(matches!(
        old.reader(ReaderRole::Query),
        Err(EngineError::StaleGeneration { .. })
    ));

    let current = h.engine.collection().unwrap();
    assert_eq!(current.generation(), Generation(2));
    let reader = current.reader(ReaderRole::Query).unwrap();
    assert!(reader.note(nid).unwrap().is_none(), "the readers read the new file");
    drop(reader);
    add_note_with_card(&current, OpSource::UserUndoable, &["after swap", "x"]);
    assert_eq!(
        h.engine.state(),
        LifecycleState::Ready {
            generation: Generation(2)
        }
    );
}

/// The `CollectionReplaced` generations among `events`.
fn replaced_generations(events: &[Arc<meguri_engine::Event>]) -> Vec<Generation> {
    events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::CollectionReplaced { generation } => Some(generation),
            _ => None,
        })
        .collect()
}

#[test]
fn a_swap_that_fails_after_replacing_the_file_still_moves_to_the_next_generation() {
    let listener = Arc::new(Listener::default());
    let h = Harness::open_with(Ports::default().generation_listener(listener.clone()));
    let old = h.collection.clone();
    let (nid, _) = add_note_with_card(&old, OpSource::UserUndoable, &["before swap", "x"]).value;
    let replacement = h.dir.path().join("replacement.anki2");
    std::fs::copy(common::fixture("schema-18.anki2"), &replacement).unwrap();
    let subscription = h.engine.stream().subscribe();

    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    // The file is in place, then a later step of the swap (say, syncing the
    // directory) fails.
    let failure = EngineError::Internal {
        message: "syncing the directory failed".to_owned(),
    };
    let returned = failure.clone();
    let error = exclusive
        .replace(move |live| {
            std::fs::rename(&replacement, live).unwrap();
            Err(returned)
        })
        .unwrap_err();
    assert_eq!(error, failure);
    // The phase's handle follows the file that is open now.
    assert_eq!(exclusive.collection().generation(), Generation(2));
    exclusive
        .collection()
        .run(OpSpec::named("restore_step", OpSource::Maintenance), |_| Ok(()))
        .unwrap();
    drop(exclusive);

    assert_eq!(
        h.engine.state(),
        LifecycleState::Ready {
            generation: Generation(2)
        }
    );
    assert_eq!(*listener.0.lock().unwrap(), [(Generation(1), Generation(2))]);
    let events = drain(&subscription, Duration::from_millis(100));
    assert_eq!(replaced_generations(&events), [Generation(2)]);
    assert_eq!(
        old.run(OpSpec::named("late", OpSource::UserUndoable), |_| Ok(()))
            .unwrap_err(),
        EngineError::StaleGeneration {
            planned: Generation(1),
            current: Generation(2)
        }
    );
    assert!(matches!(
        old.reader(ReaderRole::Query),
        Err(EngineError::StaleGeneration { .. })
    ));
    let current = h.engine.collection().unwrap();
    assert_eq!(current.generation(), Generation(2));
    assert!(
        current.reader(ReaderRole::Query).unwrap().note(nid).unwrap().is_none(),
        "the readers read the new file"
    );
    // The slot belongs to the new generation: its holder can enter again.
    drop(slot.enter_exclusive(false).unwrap());
}

#[test]
fn a_swap_that_fails_before_touching_the_file_keeps_the_generation() {
    let listener = Arc::new(Listener::default());
    let h = Harness::open_with(Ports::default().generation_listener(listener.clone()));
    let old = h.collection.clone();
    let (nid, _) = add_note_with_card(&old, OpSource::UserUndoable, &["kept", "x"]).value;
    let subscription = h.engine.stream().subscribe();

    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let failure = EngineError::Internal {
        message: "the staging copy failed its integrity check".to_owned(),
    };
    let returned = failure.clone();
    let error = exclusive.replace(move |_| Err(returned)).unwrap_err();
    assert_eq!(error, failure);
    assert_eq!(exclusive.collection().generation(), Generation(1));
    drop(exclusive);
    drop(slot);

    assert_eq!(
        h.engine.state(),
        LifecycleState::Ready {
            generation: Generation(1)
        }
    );
    assert!(listener.0.lock().unwrap().is_empty());
    assert!(replaced_generations(&drain(&subscription, Duration::from_millis(100))).is_empty());
    // Old handles keep working on the reopened, unchanged file.
    assert!(old.reader(ReaderRole::Query).unwrap().note(nid).unwrap().is_some());
    add_note_with_card(&old, OpSource::UserUndoable, &["after the failed swap", "x"]);
}

#[test]
fn closing_commits_queued_ops_then_refuses_every_op() {
    let h = Harness::open();
    let pending: Vec<_> = (0..20)
        .map(|i| {
            h.collection
                .submit(OpSpec::named("add", OpSource::UserUndoable), move |ctx| {
                    Ok(ctx.scope().add_note(&note(&[&format!("q{i}"), "x"]))?)
                })
        })
        .collect();
    h.engine.close().unwrap();
    for p in pending {
        p.wait().unwrap();
    }
    assert_eq!(h.engine.state(), LifecycleState::NoCollection);
    let refused = h
        .collection
        .run(OpSpec::named("late", OpSource::UserUndoable), |_| Ok(()))
        .unwrap_err();
    assert!(
        matches!(refused, EngineError::CollectionUnavailable { .. }),
        "{refused:?}"
    );
    assert!(h.engine.collection().is_err());
    // The file holds everything and no -wal is left after a clean close.
    assert!(!h.dir.path().join("collection.anki2-wal").exists());
    let reopened = Harness::open_path(h.dir, h.path);
    assert_eq!(
        common::query_int(&reopened.collection, "SELECT count(*) FROM notes WHERE flds LIKE 'q%'"),
        20
    );
}

#[test]
fn a_corrupt_collection_refuses_ops_and_lets_only_restore_and_one_way_sync_take_the_slot() {
    let h = Harness::open();
    let generation = h.engine.generation();
    h.engine.mark_corrupt("page 12 is damaged");
    let refused = h
        .collection
        .run(OpSpec::named("edit", OpSource::UserUndoable), |_| Ok(()))
        .unwrap_err();
    assert_eq!(refused, EngineError::CollectionCorrupt);
    assert!(matches!(
        h.engine.acquire_slot(LongOp::CheckDatabase),
        Err(EngineError::CollectionUnavailable { .. })
    ));
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(false).unwrap();
    let refused = exclusive
        .collection()
        .run(
            OpSpec::named("restore_step", OpSource::Maintenance),
            |_| -> Result<(), EngineError> { panic!("corrupt collection must refuse restore_step") },
        )
        .unwrap_err();
    assert_eq!(refused, EngineError::CollectionCorrupt);
    let refused = exclusive
        .collection()
        .submit_maintenance(
            catalog::ANALYZE,
            Duration::from_millis(10),
            |_| -> Result<(), EngineError> { panic!("corrupt collection must refuse maintenance work") },
        )
        .wait()
        .unwrap_err();
    assert_eq!(refused, EngineError::CollectionCorrupt);
    drop(exclusive);
    assert_eq!(h.engine.generation(), generation);
    assert!(
        matches!(h.engine.state(), LifecycleState::Failed { .. }),
        "no swap: still corrupt"
    );
}

#[test]
fn cloud_lifetime_pin_and_refused_leases_block_real_replacement() {
    use meguri_engine::ExclusivePhase;
    use std::sync::mpsc;
    // Successful transfer, refused owned Cloud capture, refused non-Cloud.
    for mode in 0..3 {
        let h = Harness::open();
        let role = if mode == 2 { ReaderRole::Sync } else { ReaderRole::Cloud };
        let mut lease = h.collection.reader(role).unwrap();
        let snapshot = if mode == 1 {
            Some(lease.capture_sync(&h.collection).unwrap().0)
        } else {
            None
        };
        let (pin, refused) = match lease.into_lifetime_pin() {
            Ok(pin) => {
                assert_eq!(mode, 0);
                (Some(pin), None)
            }
            Err((lease, error)) => {
                assert_ne!(mode, 0);
                assert!(matches!(error, EngineError::InvalidArgument { .. }));
                (None, Some(lease))
            }
        };
        let generation = h.engine.generation();
        let replacement = h.dir.path().join("replacement.anki2");
        std::fs::copy(common::fixture("schema-18.anki2"), &replacement).unwrap();
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(false).unwrap();
        let privileged = exclusive.collection().clone();
        let subscription = h.engine.stream().subscribe();
        let (swapped, swap_seen) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = exclusive.replace(move |live| {
                swapped.send(()).unwrap();
                std::fs::rename(&replacement, live).map_err(|e| EngineError::internal(e.to_string()))
            });
            drop(exclusive);
            drop(slot);
            done.send(result.map(|c| c.generation())).unwrap();
        });
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_sub(meguri_engine::monotonic::now());
            let event = subscription
                .recv_timeout(remaining)
                .expect("replacement reaches reader close");
            if matches!(
                event.kind,
                EventKind::Lifecycle(LifecycleState::ExclusiveBusy {
                    phase: ExclusivePhase::Replacing,
                    ..
                })
            ) {
                break;
            }
        }
        assert!(matches!(
            swap_seen.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(finished.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert_eq!(h.engine.generation(), generation);
        // The real writer still owns SQLite while readers are being drained.
        privileged
            .run(
                OpSpec::named("reader_barrier_writer_probe", OpSource::Rollover),
                |ctx| Ok(ctx.scope().col()?),
            )
            .unwrap();
        if let Some(snapshot) = snapshot {
            assert!(snapshot.col().is_ok());
            drop(snapshot);
            // Capture returned an OWNED snapshot. Dropping it must not release
            // the refused lease's independent replacement barrier.
            assert!(matches!(
                swap_seen.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            assert_eq!(h.engine.generation(), generation);
        }
        drop((pin, refused));
        swap_seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
            Generation(generation.0 + 1)
        );
        worker.join().unwrap();
        h.engine.close().unwrap();
    }
}

#[test]
fn cloud_lifetime_pin_blocks_real_close_before_connection_release() {
    use meguri_engine::SuspendParticipant;
    use std::sync::mpsc;
    struct Participant {
        quiesced: mpsc::Sender<()>,
        released: mpsc::Sender<()>,
    }
    impl SuspendParticipant for Participant {
        fn name(&self) -> &str {
            "reader close witness"
        }
        fn flush(&self) -> Result<(), EngineError> {
            Ok(())
        }
        fn suspend(&self) -> Result<(), EngineError> {
            self.quiesced.send(()).unwrap();
            Ok(())
        }
        fn release_connections_until(&self, _: Duration) -> Result<(), EngineError> {
            self.released.send(()).unwrap();
            Ok(())
        }
    }
    let (quiesced, reached) = mpsc::channel();
    let (released, release_seen) = mpsc::channel();
    let h = Harness::open_with(Ports::new().suspend_participant(Arc::new(Participant { quiesced, released })));
    let lease = h.collection.reader(ReaderRole::Cloud).unwrap();
    let pin = lease
        .into_lifetime_pin()
        .unwrap_or_else(|(_, error)| panic!("{error:?}"));
    let generation = h.engine.generation();
    let engine = h.engine.clone();
    let (done, finished) = mpsc::channel();
    let closer = std::thread::spawn(move || done.send(engine.close()).unwrap());
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        release_seen.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(matches!(finished.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(h.engine.generation(), generation);
    assert!(h.path.with_file_name("collection.anki2-wal").exists());
    drop(pin);
    release_seen.recv_timeout(Duration::from_secs(5)).unwrap();
    finished.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    closer.join().unwrap();
    assert_eq!(h.engine.state(), LifecycleState::NoCollection);
    assert!(!h.path.with_file_name("collection.anki2-wal").exists());
    assert!(h.collection.reader(ReaderRole::Cloud).is_err());
}
