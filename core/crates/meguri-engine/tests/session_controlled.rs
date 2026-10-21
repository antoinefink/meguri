//! U37 lifecycle ownership, pending redo identity and controlled rollback.
#![cfg(feature = "test-hooks")]
mod common;
mod session_support;
use common::{Gate, block_writer};
use meguri_engine::{
    CancelToken, EngineError, EventKind, JobKind, OpSource, ProgressPhase, ProgressSink, SessionId, UndoScope,
    UndoStatus,
    session::{AnswerOutcome, Head, HistoryRestore, ReviewService, SessionEvent, StartResult},
};
use meguri_model::{clock::Clock, ids::DeckId, time::EpochMillis};
use meguri_sched::answer::Rating;
use session_support::Harness;
use std::{
    sync::{Arc, Mutex, Weak, mpsc},
    time::Duration,
};
struct Release(Gate);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.open();
    }
}
fn wait_for(c: &meguri_engine::Collection, name: &str) {
    let until = meguri_engine::monotonic::now() + Duration::from_secs(5);
    while !c.has_queued_op_for_testing(name) {
        assert!(meguri_engine::monotonic::now() < until, "never queued {name}");
        std::thread::yield_now();
    }
}
fn answer(h: &Harness, id: SessionId, head: &Head) -> Head {
    let first = &head.items[0];
    match h
        .review
        .answer(
            id,
            first.card_id,
            Rating::Easy,
            first.state_token,
            EpochMillis(h.clock.now_ms()),
            100,
        )
        .unwrap()
    {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("{other:?}"),
    }
}
fn available(status: UndoStatus) -> meguri_engine::StepInfo {
    match status {
        UndoStatus::Available(step) => step,
        other => panic!("{other:?}"),
    }
}
#[test]
fn pending_redos_keep_original_events_and_undo_bypasses_four_occupied_workers() {
    let h = Harness::new(4);
    let (id, initial) = h.start();
    let writer = Gate::new();
    let _writer_release = Release(writer.clone());
    let blocking = block_writer(&h.collection, OpSource::Rollover, &writer);
    let next = answer(&h, id, &initial);
    let first = available(h.review.peek_undo(UndoScope::Session(id)).unwrap());
    answer(&h, id, &next);
    let second = available(h.review.peek_undo(UndoScope::Session(id)).unwrap());
    assert_ne!(first.id, second.id);
    assert_ne!(first.id.0, 0);
    let mut workers = Vec::new();
    for _ in 0..4 {
        let gate = Gate::new();
        let release = Release(gate.clone());
        let entered = gate.clone();
        let handle = h
            .engine
            .start_job(JobKind::BulkOp, move |_| {
                entered.enter_and_wait();
                Ok(())
            })
            .unwrap();
        gate.wait_entered();
        workers.push((release, handle));
    }
    let at = meguri_engine::monotonic::now();
    let restored = h
        .review
        .try_pending_restore(UndoScope::Session(id), false)
        .unwrap()
        .unwrap();
    assert!(meguri_engine::monotonic::now() - at < Duration::from_millis(100));
    assert_eq!(restored.items[0].card_id, next.items[0].card_id);
    assert_eq!(available(h.review.peek_redo(UndoScope::Session(id)).unwrap()), second);
    h.review
        .try_pending_restore(UndoScope::Session(id), false)
        .unwrap()
        .unwrap();
    assert_eq!(available(h.review.peek_redo(UndoScope::Session(id)).unwrap()), first);
    let wrong = UndoScope::Session(SessionId(id.0 + 1));
    assert!(h.review.peek_redo(wrong).is_err());
    assert!(h.review.try_pending_restore(wrong, true).is_err());
    h.review
        .try_pending_restore(UndoScope::Session(id), true)
        .unwrap()
        .unwrap();
    assert_eq!(available(h.review.peek_redo(UndoScope::Session(id)).unwrap()), second);
    h.review
        .try_pending_restore(UndoScope::Session(id), true)
        .unwrap()
        .unwrap();
    assert!(!matches!(
        h.review.peek_redo(UndoScope::Session(id)).unwrap(),
        UndoStatus::Available(_)
    ));
    assert!(
        h.review
            .try_pending_restore(UndoScope::Session(id), true)
            .unwrap()
            .is_none()
    );
    for (release, _) in &workers {
        release.0.open();
    }
    writer.open();
    blocking.wait().unwrap();
    h.settle();
    assert_eq!(h.logs().len(), 2);
}

#[test]
fn cancelled_queued_committed_undo_restores_optimistic_presentation() {
    let h = Harness::new(3);
    let (id, initial) = h.start();
    let after = answer(&h, id, &initial);
    h.settle();
    let rows = common::steps::dump(&h.collection);
    let undo = h.collection.peek_undo(UndoScope::Session(id)).unwrap();
    let redo = h.collection.peek_redo(UndoScope::Session(id)).unwrap();
    let gate = Gate::new();
    let _release = Release(gate.clone());
    let blocking = block_writer(&h.collection, OpSource::Rollover, &gate);
    let token = CancelToken::new();
    let cancel = token.clone();
    let review = h.review.clone();
    let work = h
        .engine
        .threads()
        .spawn("test-controlled-session-undo", meguri_engine::Qos::Default, move |_| {
            review.restore_controlled(UndoScope::Session(id), false, cancel, ProgressSink::new())
        })
        .unwrap();
    wait_for(&h.collection, "undo");
    assert_eq!(h.review.head(id, 3).unwrap().items[0].card_id, initial.items[0].card_id);
    assert!(h.review.peek_undo(UndoScope::Session(id)).is_err());
    assert!(h.review.peek_redo(UndoScope::Session(id)).is_err());
    token.cancel();
    gate.open();
    blocking.wait().unwrap();
    assert!(matches!(work.join().unwrap(), Err(EngineError::Cancelled)));
    assert_eq!(h.review.head(id, 3).unwrap().items, after.items);
    assert_eq!(common::steps::dump(&h.collection), rows);
    assert_eq!(h.collection.peek_undo(UndoScope::Session(id)).unwrap(), undo);
    assert_eq!(h.collection.peek_redo(UndoScope::Session(id)).unwrap(), redo);
}
struct CommitGate(Gate);
impl meguri_engine::ProgressListener for CommitGate {
    fn on_progress(&self, p: &meguri_engine::Progress) {
        if p.phase == meguri_engine::ProgressPhase::Committing {
            self.0.enter_and_wait();
        }
    }
}
#[test]
fn session_end_and_restart_cannot_turn_a_committed_restore_into_retryable_failure() {
    for restart in [false, true] {
        let h = Harness::new(2);
        let (id, _) = h.start();
        let deck = meguri_engine::decks::add(&h.collection, "One restoration".into())
            .unwrap()
            .value
            .id;
        let gate = Gate::new();
        let _release = Release(gate.clone());
        let progress = ProgressSink::new();
        progress.set_listener(Arc::new(CommitGate(gate.clone())));
        let review = h.review.clone();
        let restore = h
            .engine
            .threads()
            .spawn("test-restore-lifecycle", meguri_engine::Qos::Default, move |_| {
                review.restore_controlled(UndoScope::Global, false, CancelToken::new(), progress)
            })
            .unwrap();
        gate.wait_entered();
        let review = h.review.clone();
        let (enter, entered) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let lifecycle = h
            .engine
            .threads()
            .spawn(
                "test-end-start-during-restore",
                meguri_engine::Qos::Default,
                move |_| {
                    enter.send(()).unwrap();
                    let result = if restart {
                        review.start(DeckId(1), None).map(Some)
                    } else {
                        review.end(id).map(|_| None)
                    };
                    done.send(()).unwrap();
                    result
                },
            )
            .unwrap();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(completed.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert!(h.review.try_pending_restore(UndoScope::Session(id), false).is_err());
        assert_eq!(h.review.active_session(), Some(id));
        gate.open();
        assert!(matches!(restore.join().unwrap(), Ok(HistoryRestore::Session(_))));
        let changed = lifecycle.join().unwrap().unwrap();
        if restart {
            assert!(matches!(changed, Some(StartResult::Started { session_id, .. }) if session_id != id));
        }
        use meguri_storage::ReadRows;
        assert!(
            h.collection
                .reader(meguri_storage::ReaderRole::Query)
                .unwrap()
                .deck(deck)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn generic_committed_undo_and_redo_report_projection_failure_with_the_commit_sequence() {
    for redo in [false, true] {
        let h = Harness::new(2);
        let (id, initial) = h.start();
        let card = initial.items[0].card_id;
        meguri_engine::flags::set_flag(&h.collection, &[card], 1, &Default::default()).unwrap();
        if redo {
            h.collection.undo(UndoScope::Global).unwrap();
        }
        h.settle();
        let before = h.review.head(id, 3).unwrap();
        assert_eq!(before.items[0].flag, u8::from(!redo));
        let stream = h.engine.stream().subscribe();
        h.review.fail_next_restore_projection_for_testing();
        let outcome = h
            .review
            .restore_controlled(UndoScope::Global, redo, CancelToken::new(), ProgressSink::new());
        let seq = match outcome {
            Err(EngineError::CommitCompleted { seq, message }) => {
                assert!(message.contains("projection"));
                seq
            }
            _ => panic!("expected a committed projection failure"),
        };
        let item = &before.items[0];
        assert!(
            h.review
                .answer(
                    id,
                    item.card_id,
                    Rating::Easy,
                    item.state_token,
                    EpochMillis(h.clock.now_ms()),
                    100
                )
                .is_err()
        );
        h.settle();
        let mut mutations = Vec::new();
        let mut failures = Vec::new();
        while let Some(event) = stream.try_recv() {
            match &event.kind {
                EventKind::Changes(_) => mutations.push(event.seq),
                EventKind::Extension(payload) => {
                    if let Some(SessionEvent::CommitFailed {
                        session_id,
                        card_id,
                        error: EngineError::CommitCompleted { seq: reported, .. },
                    }) = payload.as_any().downcast_ref::<SessionEvent>()
                    {
                        assert_eq!((*session_id, *card_id, *reported), (id, card, seq));
                        assert_eq!(event.cause_seq, Some(seq));
                        failures.push(event.seq);
                    }
                }
                _ => {}
            }
        }
        assert_eq!(mutations, vec![seq]);
        assert_eq!(failures.len(), 1);
        let opposite = if redo {
            h.collection.peek_undo(UndoScope::Global).unwrap()
        } else {
            h.collection.peek_redo(UndoScope::Global).unwrap()
        };
        assert!(matches!(opposite, UndoStatus::Available(_)));
        let consumed = if redo {
            h.collection.peek_redo(UndoScope::Global).unwrap()
        } else {
            h.collection.peek_undo(UndoScope::Global).unwrap()
        };
        assert!(!matches!(consumed, UndoStatus::Available(_)));
        let recovered = h.review.head(id, 3).unwrap();
        assert_eq!(recovered.items[0].flag, u8::from(redo));
        assert_ne!(recovered, before);
    }
}

struct CancelAtPendingCommit {
    cancel: Arc<CancelToken>,
    review: Weak<ReviewService>,
    phases: Mutex<Vec<ProgressPhase>>,
}
impl meguri_engine::ProgressListener for CancelAtPendingCommit {
    fn on_progress(&self, progress: &meguri_engine::Progress) {
        self.phases.lock().unwrap().push(progress.phase.clone());
        if progress.phase == ProgressPhase::Committing {
            assert!(self.review.upgrade().unwrap().active_session().is_some());
            self.cancel.cancel();
        }
    }
}

#[test]
fn fallback_pending_undo_and_redo_honor_the_admission_cancel_point() {
    for redo in [false, true] {
        let h = Harness::new(2);
        let (id, initial) = h.start();
        let scope = UndoScope::Session(id);
        assert!(h.review.try_pending_restore(scope, redo).unwrap().is_none());
        let writer = Gate::new();
        let _release = Release(writer.clone());
        let blocking = block_writer(&h.collection, OpSource::Rollover, &writer);
        let answered = answer(&h, id, &initial);
        if redo {
            h.review.try_pending_restore(scope, false).unwrap().unwrap();
        }
        let before = h.review.head(id, 3).unwrap();
        let undo = h.review.peek_undo(scope).unwrap();
        let redo_step = h.review.peek_redo(scope).unwrap();
        let rows = common::steps::dump(&h.collection);
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let planning = ProgressSink::new();
        assert!(matches!(
            h.review.restore_controlled(scope, redo, cancelled, planning.clone()),
            Err(EngineError::Cancelled)
        ));
        assert_eq!(planning.phase(), ProgressPhase::Planning);
        assert_eq!(h.review.head(id, 3).unwrap(), before);
        assert_eq!(h.review.peek_undo(scope).unwrap(), undo);
        assert_eq!(h.review.peek_redo(scope).unwrap(), redo_step);
        assert_eq!(common::steps::dump(&h.collection), rows);
        let token = CancelToken::new();
        let listener = Arc::new(CancelAtPendingCommit {
            cancel: token.clone(),
            review: Arc::downgrade(&h.review),
            phases: Mutex::new(Vec::new()),
        });
        let progress = ProgressSink::new();
        progress.set_listener(listener.clone());
        let restored = match h
            .review
            .restore_controlled(scope, redo, token, progress.clone())
            .unwrap()
        {
            HistoryRestore::Session(head) => head,
            _ => panic!("expected session restoration"),
        };
        assert_eq!(progress.phase(), ProgressPhase::Committing);
        assert_eq!(*listener.phases.lock().unwrap(), vec![ProgressPhase::Committing]);
        assert_eq!(restored.items, if redo { answered.items } else { initial.items });
        assert_eq!(common::steps::dump(&h.collection), rows);
        writer.open();
        blocking.wait().unwrap();
        h.settle();
        assert_eq!(h.logs().len(), usize::from(redo));
    }
}
