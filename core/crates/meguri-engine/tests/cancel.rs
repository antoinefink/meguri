//! Cancellation and progress (`engine/op-model` O5, `engine/jobs` J4, J5;
//! R1858, R1860): a cancel before the write phase writes nothing, one after
//! it starts is reported past the cancel point, and progress reaches its
//! listener about ten times a second with every phase change.

mod common;

use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use common::{Gate, Harness, add_notes, block_writer, col_mod, note, query_int};
use meguri_engine::executor::catalog;
use meguri_engine::{
    CancelToken, EngineError, OpSpec, Plan, Progress, ProgressListener, ProgressPhase, ProgressSink, plan_and_commit,
};
use meguri_model::changes::OpSource;
use meguri_storage::{ReadRows, ReaderRole};

#[test]
fn cancel_during_the_plan_of_a_ten_thousand_note_op_writes_nothing() {
    let h = Harness::open();
    add_notes(&h.collection, 10_000, "note");
    let before = col_mod(&h.collection);
    h.clock.step_ms(5_000);
    let token = CancelToken::new();
    let progress = ProgressSink::new();
    let seen = Arc::new(Recorder::default());
    progress.set_listener(seen.clone());
    let canceller = Arc::clone(&token);
    let error = plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("rewrite_all", OpSource::UserNonUndoable)
            .cancel(Arc::clone(&token))
            .progress(progress.clone()),
        |plan| {
            let mut writes = Vec::new();
            let mut after = None;
            loop {
                plan.check_cancel()?;
                let page = plan.snapshot().notes_after(after, 500)?;
                let Some(last) = page.last() else { break };
                after = Some(last.id);
                writes.extend(page.iter().map(|n| n.id));
                if let Some(p) = plan.progress() {
                    p.report(writes.len() as u64, Some(10_000));
                }
                if writes.len() >= 5_000 {
                    // The user presses Cancel halfway through planning.
                    canceller.cancel();
                }
            }
            Ok(Plan {
                value: writes,
                preconditions: Vec::new(),
            })
        },
        |ctx, ids: Vec<meguri_model::ids::NoteId>| {
            for id in ids {
                let mut stored = ctx.scope().note(id)?.unwrap();
                stored.fields.push('!');
                ctx.scope().put(&stored)?;
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error, EngineError::Cancelled);
    assert_eq!(col_mod(&h.collection), before);
    assert_eq!(
        query_int(&h.collection, "SELECT count(*) FROM notes WHERE flds LIKE '%!'"),
        0
    );
    let phases: Vec<ProgressPhase> = seen.updates().into_iter().map(|p| p.phase).collect();
    assert!(phases.iter().all(|p| *p == ProgressPhase::Planning), "{phases:?}");
}

#[test]
fn cancel_after_the_write_phase_started_completes_the_op_and_reports_it() {
    let h = Harness::open();
    let token = CancelToken::new();
    let progress = ProgressSink::new();
    let seen = Arc::new(Recorder::default());
    progress.set_listener(seen.clone());
    let canceller = Arc::clone(&token);
    let sink = progress.clone();
    let result = h
        .collection
        .run(
            OpSpec::named("add_many", OpSource::UserNonUndoable)
                .cancel(Arc::clone(&token))
                .progress(progress.clone()),
            move |ctx| {
                assert_eq!(sink.phase(), ProgressPhase::Committing, "Cancel is no longer offered");
                ctx.scope().add_note(&note(&["one", "x"]))?;
                canceller.cancel();
                // Past the cancel point: the op completes.
                ctx.check_cancel()?;
                ctx.scope().add_note(&note(&["two", "x"]))?;
                Ok(())
            },
        )
        .unwrap();
    assert!(result.past_cancel_point);
    assert_eq!(result.changes.notes.len(), 2);
    let phases: Vec<ProgressPhase> = seen.updates().into_iter().map(|p| p.phase).collect();
    assert_eq!(phases, [ProgressPhase::Committing]);
}

#[test]
fn a_cancel_while_queued_runs_nothing() {
    let h = Harness::open();
    let gate = Gate::new();
    let blocker = block_writer(&h.collection, OpSource::UserUndoable, &gate);
    let token = CancelToken::new();
    let ran = Arc::new(Mutex::new(false));
    let flag = ran.clone();
    let pending = h.collection.submit(
        OpSpec::named("queued", OpSource::UserUndoable).cancel(Arc::clone(&token)),
        move |ctx| {
            *flag.lock().unwrap() = true;
            Ok(ctx.scope().add_note(&note(&["never", "x"]))?)
        },
    );
    token.cancel();
    gate.open();
    blocker.wait().unwrap();
    assert_eq!(pending.wait().unwrap_err(), EngineError::Cancelled);
    assert!(!*ran.lock().unwrap());
}

#[test]
fn a_write_phase_the_catalog_declares_cancellable_rolls_back_between_batches() {
    let h = Harness::open();
    let count = query_int(&h.collection, "SELECT count(*) FROM notes");
    let token = CancelToken::new();
    let canceller = Arc::clone(&token);
    let error = h
        .collection
        .run(
            OpSpec::new(catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming)
                .declare_long_write(Duration::from_secs(2))
                .cancel(Arc::clone(&token)),
            move |ctx| {
                for batch in 0..10 {
                    ctx.check_cancel()?;
                    for i in 0..100 {
                        let mut incoming = note(&[&format!("b{batch}-{i}"), "x"]);
                        incoming.id = meguri_model::ids::NoteId(1_600_000_000_000 + batch * 1000 + i);
                        ctx.scope().put(&incoming)?;
                    }
                    if batch == 4 {
                        canceller.cancel();
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error, EngineError::Cancelled);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM notes"), count);
}

struct ThreadWaker(thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        thread::park();
    }
}

#[test]
fn the_async_token_completes_when_another_thread_cancels() {
    let token = CancelToken::new();
    assert_eq!(token.check(), Ok(()));
    let waiter = {
        let token = Arc::clone(&token);
        thread::spawn(move || block_on(token.cancelled()))
    };
    thread::sleep(Duration::from_millis(30));
    assert!(!waiter.is_finished());
    token.cancel();
    waiter.join().unwrap();
    assert_eq!(token.check(), Err(EngineError::Cancelled));
    token.cancel();
    block_on(token.cancelled());
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Progress>>);

impl Recorder {
    fn updates(&self) -> Vec<Progress> {
        self.0.lock().unwrap().clone()
    }
}

impl ProgressListener for Recorder {
    fn on_progress(&self, progress: &Progress) {
        self.0.lock().unwrap().push(progress.clone());
    }
}

thread_local! {
    static NOW_MS: Cell<u64> = const { Cell::new(0) };
}

fn fake_clock() -> Duration {
    Duration::from_millis(NOW_MS.get())
}

#[test]
fn a_thousand_updates_a_second_reach_the_listener_about_ten_times_a_second_with_every_phase_change() {
    let sink = ProgressSink::with_clock(fake_clock);
    let recorder = Arc::new(Recorder::default());
    sink.set_listener(recorder.clone());
    let mut delivered_at = Vec::new();
    for ms in 0..3_000u64 {
        NOW_MS.set(ms);
        let before = recorder.updates().len();
        if ms == 1_505 {
            sink.set_phase(ProgressPhase::Committing);
        } else if ms == 2_502 {
            sink.set_phase(ProgressPhase::Named("finishing"));
        }
        let phase_total = if ms < 1_505 { 1_505 } else { 3_000 };
        sink.report(ms % 1_505, Some(phase_total));
        if recorder.updates().len() > before {
            delivered_at.push(ms);
        }
    }
    let updates = recorder.updates();
    // Every phase change arrived, in order.
    let mut phases: Vec<ProgressPhase> = updates.iter().map(|u| u.phase.clone()).collect();
    phases.dedup();
    assert_eq!(
        phases,
        [
            ProgressPhase::Planning,
            ProgressPhase::Committing,
            ProgressPhase::Named("finishing")
        ]
    );
    // About ten a second: at most eleven in any second, plus the two phase
    // changes.
    for &start in &delivered_at {
        let in_window = delivered_at
            .iter()
            .filter(|&&t| t >= start && t < start + 1_000)
            .count();
        assert!(in_window <= 13, "{in_window} updates in the second from {start} ms");
    }
    assert!(
        updates.len() >= 28,
        "the throttle starves the listener: {}",
        updates.len()
    );
    assert!(ProgressPhase::Planning.cancellable() && !ProgressPhase::Committing.cancellable());
}

#[test]
fn progress_carries_an_eta_at_the_phase_rate_and_nothing_reaches_a_missing_listener() {
    NOW_MS.set(0);
    let sink = ProgressSink::with_clock(fake_clock);
    sink.report(10, Some(100));
    let recorder = Arc::new(Recorder::default());
    sink.set_listener(recorder.clone());
    assert!(recorder.updates().is_empty(), "attaching delivers nothing");
    NOW_MS.set(2_000);
    sink.report(20, Some(100));
    let update = recorder.updates().pop().unwrap();
    assert_eq!((update.completed, update.total), (20, Some(100)));
    // 20 done in 2 s: 80 left take 8 s.
    assert_eq!(update.eta, Some(Duration::from_secs(8)));
    NOW_MS.set(2_050);
    sink.report(30, None);
    assert_eq!(recorder.updates().len(), 1, "within 100 ms of the last delivery");
    NOW_MS.set(2_200);
    sink.report(40, None);
    assert_eq!(recorder.updates().pop().unwrap().eta, None, "no total, no ETA");
}
