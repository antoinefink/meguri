//! The long-operation slot, availability waits and the exclusive phase
//! (`engine/lifecycle` L4 to L8; R171, R1706, R1797; KTD17).

mod common;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{Harness, add_note_with_card, drain, note};
use meguri_engine::{
    EngineError, EventKind, ExclusivePhase, Generation, LifecycleState, LongOp, OpSpec, Ports, SuspendParticipant,
    monotonic,
};
use meguri_model::changes::OpSource;
use meguri_storage::ReaderRole;
use meguri_storage::raw::SqlValue;

#[test]
fn a_held_slot_refuses_a_second_request_and_a_waiter_times_out_unavailable() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let busy = h.engine.acquire_slot(LongOp::CheckDatabase).unwrap_err();
    assert_eq!(
        busy,
        EngineError::Busy {
            running_op: "one-way sync".to_owned()
        }
    );
    // Migaku's wait (R1706).
    let started = monotonic::now();
    let error = h.engine.wait_available(Duration::from_secs(5)).unwrap_err();
    let waited = monotonic::now() - started;
    assert!(
        matches!(&error, EngineError::CollectionUnavailable { reason } if reason.contains("one-way sync")),
        "{error:?}"
    );
    assert!(
        waited >= Duration::from_secs(5) && waited < Duration::from_secs(6),
        "{waited:?}"
    );
    drop(slot);
    assert!(h.engine.acquire_slot(LongOp::CheckDatabase).is_ok());
}

#[test]
fn check_database_holding_the_slot_keeps_ready_lets_answers_commit_and_blocks_a_server_waiter_until_release() {
    let h = Harness::open();
    let subscription = h.engine.stream().subscribe();
    let slot = h.engine.acquire_slot(LongOp::CheckDatabase).unwrap();
    assert_eq!(
        h.engine.state(),
        LifecycleState::Ready {
            generation: Generation(1)
        }
    );
    add_note_with_card(&h.collection, OpSource::UserUndoable, &["answer", "x"]);

    // AnkiConnect's wait (R1797): 30 s at most.
    let engine = h.engine.clone();
    let waiter = thread::spawn(move || {
        let started = monotonic::now();
        let got = engine.wait_available(Duration::from_secs(30));
        (got.map(|c| c.generation()), monotonic::now() - started)
    });
    thread::sleep(Duration::from_millis(300));
    assert!(!waiter.is_finished(), "the waiter blocks while the slot is held");
    drop(slot);
    let (got, waited) = waiter.join().unwrap();
    assert_eq!(got, Ok(Generation(1)));
    assert!(
        waited >= Duration::from_millis(250) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
    let available = drain(&subscription, Duration::from_millis(100))
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Available))
        .count();
    assert_eq!(available, 1, "the release publishes one availability event");
}

/// A participant that flushes pending answers into ops when asked.
struct Overlay {
    collection: Mutex<Option<meguri_engine::Collection>>,
    flushed: Mutex<u32>,
}

impl SuspendParticipant for Overlay {
    fn name(&self) -> &str {
        "answer-overlay"
    }

    fn flush(&self) -> Result<(), EngineError> {
        *self.flushed.lock().unwrap() += 1;
        if let Some(collection) = &*self.collection.lock().unwrap() {
            collection.run(OpSpec::named("answer", OpSource::UserUndoable), |ctx| {
                Ok(ctx.scope().add_note(&note(&["flushed answer", "x"]))?)
            })?;
        }
        Ok(())
    }
}

/// The rows of every table as stored, read with raw SQL on `read`.
fn dump(read: impl Fn(&str) -> Vec<Vec<SqlValue>>) -> Vec<Vec<Vec<SqlValue>>> {
    [
        "SELECT * FROM col",
        "SELECT * FROM config ORDER BY KEY",
        "SELECT * FROM deck_config ORDER BY id",
        "SELECT * FROM decks ORDER BY id",
        "SELECT * FROM notetypes ORDER BY id",
        "SELECT * FROM fields ORDER BY ntid, ord",
        "SELECT * FROM templates ORDER BY ntid, ord",
        "SELECT * FROM tags ORDER BY tag",
        "SELECT * FROM graves ORDER BY oid, type",
        "SELECT * FROM notes ORDER BY id",
        "SELECT * FROM cards ORDER BY id",
        "SELECT * FROM revlog ORDER BY id",
    ]
    .iter()
    .map(|sql| read(sql))
    .collect()
}

#[test]
fn during_a_stalled_one_way_upload_other_ops_are_refused_or_parked_and_run_or_go_stale_after_the_swap() {
    let overlay = Arc::new(Overlay {
        collection: Mutex::new(None),
        flushed: Mutex::new(0),
    });
    let h = Harness::open_with(Ports::default().suspend_participant(overlay.clone()));
    *overlay.collection.lock().unwrap() = Some(h.collection.clone());
    add_note_with_card(&h.collection, OpSource::UserUndoable, &["synced", "x"]);
    // Writes queued before the phase begins commit in its drain.
    let queued: Vec<_> = (0..5)
        .map(|i| {
            h.collection
                .submit(OpSpec::named("edit", OpSource::UserUndoable), move |ctx| {
                    Ok(ctx.scope().add_note(&note(&[&format!("queued {i}"), "x"]))?)
                })
        })
        .collect();

    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    assert_eq!(*overlay.flushed.lock().unwrap(), 1, "the overlay was flushed first");
    for pending in queued {
        pending.wait().unwrap();
    }
    assert!(matches!(
        h.engine.state(),
        LifecycleState::ExclusiveBusy {
            op: LongOp::OneWaySync,
            phase: ExclusivePhase::Working,
            cancellable: true
        }
    ));

    // The upload's snapshot: VACUUM INTO on the sync reader, after the drain.
    let uploaded = h.dir.path().join("upload.anki2");
    {
        let privileged = exclusive.collection();
        let reader = privileged.reader(ReaderRole::Sync).unwrap();
        reader
            .execute_raw("VACUUM INTO ?1", &[SqlValue::Text(uploaded.display().to_string())])
            .unwrap();
    }
    let expected = {
        let copy = h.dir.path().join("expected.anki2");
        std::fs::copy(&uploaded, &copy).unwrap();
        let opened = meguri_storage::open_collection(
            &copy,
            &meguri_storage::OpenOptions::new(h.clock.as_ref(), &meguri_model::rng::SeededRandom::new(1)),
        )
        .unwrap();
        let rows = dump(|sql| opened.writer().execute_raw(sql, &[]).unwrap().rows);
        opened.close().unwrap();
        rows
    };

    // The upload stalls. Meanwhile:
    let rollover = h.collection.submit(
        OpSpec::named("rollover", OpSource::Rollover)
            .park_when_unavailable()
            .run_on_any_generation(),
        |ctx| Ok(ctx.scope().add_note(&note(&["unburied on rollover", "x"]))?),
    );
    let optimizer = h.collection.submit(
        OpSpec::named("optimizer_commit", OpSource::Optimizer).park_when_unavailable(),
        |ctx| Ok(ctx.scope().add_note(&note(&["optimizer", "x"]))?),
    );
    let api_add = h
        .collection
        .run(OpSpec::named("addNote", OpSource::LocalApi), |ctx| {
            Ok(ctx.scope().add_note(&note(&["from migaku", "x"]))?)
        })
        .unwrap_err();
    assert!(
        matches!(&api_add, EngineError::CollectionUnavailable { reason } if reason.contains("one-way sync")),
        "{api_add:?}"
    );
    thread::sleep(Duration::from_millis(200));
    assert!(
        rollover.try_take().is_none() && optimizer.try_take().is_none(),
        "parked"
    );
    // The exclusive job's own writes still commit.
    exclusive
        .collection()
        .run(OpSpec::named("mark_uploaded", OpSource::Maintenance), |ctx| {
            Ok(ctx.scope().add_note(&note(&["after the snapshot", "x"]))?)
        })
        .unwrap();

    // The swap: the uploaded snapshot becomes the live collection.
    let source = uploaded.clone();
    let new = exclusive
        .replace(move |live| {
            std::fs::copy(&source, live)
                .map(|_| ())
                .map_err(|e| EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    {
        let reader = new.reader(ReaderRole::Query).unwrap();
        let live = dump(|sql| reader.execute_raw(sql, &[]).unwrap().rows);
        assert_eq!(live, expected, "the live collection equals the uploaded snapshot");
    }
    drop(exclusive);
    drop(slot);

    // Parked ops: re-run on the new generation, or stale.
    let rollover = rollover.wait().unwrap();
    assert_eq!(rollover.generation, Generation(2));
    assert_eq!(
        optimizer.wait().unwrap_err(),
        EngineError::StaleGeneration {
            planned: Generation(1),
            current: Generation(2)
        }
    );
    assert_eq!(
        h.engine.state(),
        LifecycleState::Ready {
            generation: Generation(2)
        }
    );
    assert!(h.engine.is_available());
}

#[test]
fn only_replacement_ops_enter_the_exclusive_phase_and_dropping_it_restores_ready() {
    let h = Harness::open();
    let check = h.engine.acquire_slot(LongOp::CheckDatabase).unwrap();
    assert!(matches!(
        check.enter_exclusive(false),
        Err(EngineError::InvalidArgument { .. })
    ));
    drop(check);

    let slot = h.engine.acquire_slot(LongOp::ColpkgImport).unwrap();
    let parked = {
        let exclusive = slot.enter_exclusive(true).unwrap();
        let parked = h.collection.submit(
            OpSpec::named("edit", OpSource::UserUndoable).park_when_unavailable(),
            |ctx| Ok(ctx.scope().add_note(&note(&["after cancel", "x"]))?),
        );
        drop(exclusive);
        parked
    };
    // Cancelled without a swap: same generation, the parked op runs.
    assert_eq!(parked.wait().unwrap().generation, Generation(1));
    assert_eq!(
        h.engine.state(),
        LifecycleState::Ready {
            generation: Generation(1)
        }
    );
    assert!(!h.engine.is_available(), "the slot is still held");
    drop(slot);
    assert!(h.engine.is_available());
}
