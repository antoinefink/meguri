//! The executor's ordering and transaction rules (`engine/op-model` O2 to
//! O5, O8): seqs, the two queues, one transaction per op with steps,
//! panics, empty ops, declared long writes and maintenance tasks.

mod common;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{
    Gate, Harness, add_note_with_card, block_writer, change_seqs, col_mod, drain, note, note_fields, set_first_field,
};
use meguri_engine::executor::catalog;
use meguri_engine::{ChangeObserver, EngineError, EventKind, ObservedCommit, OpSpec, Ports, monotonic};
use meguri_model::changes::OpSource;
use meguri_model::ids::NoteId;
use meguri_storage::ReadRows;

/// Records the seq of every commit it observes.
#[derive(Default)]
struct SeqRecorder(Mutex<Vec<u64>>);

impl ChangeObserver for SeqRecorder {
    fn name(&self) -> &str {
        "seq-recorder"
    }

    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        self.0.lock().unwrap().push(commit.seq.0);
        Ok(())
    }
}

#[test]
fn a_thousand_ops_from_four_threads_reach_the_observer_and_the_stream_once_in_seq_order() {
    let recorder = Arc::new(SeqRecorder::default());
    let h = Harness::open_with(Ports::default().observer(recorder.clone()));
    let subscription = h.engine.stream().subscribe();

    let threads: Vec<_> = (0..4)
        .map(|t| {
            let collection = h.collection.clone();
            thread::spawn(move || {
                (0..250)
                    .map(|i| {
                        let source = if i % 2 == 0 {
                            OpSource::UserUndoable
                        } else {
                            OpSource::LocalApi
                        };
                        add_note_with_card(&collection, source, &[&format!("t{t}-{i}"), "b"])
                            .seq
                            .0
                    })
                    .collect::<Vec<u64>>()
            })
        })
        .collect();
    let mut per_thread: Vec<Vec<u64>> = threads.into_iter().map(|t| t.join().unwrap()).collect();

    // Each thread's ops commit in its submission order.
    for seqs in &per_thread {
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    }
    let mut all: Vec<u64> = per_thread.drain(..).flatten().collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 1000, "every op has its own seq");

    // The observer saw every commit once, in seq order.
    assert_eq!(*recorder.0.lock().unwrap(), all);

    // The stream carried every change set once, in seq order.
    let events = drain(&subscription, Duration::from_millis(200));
    let seqs: Vec<u64> = events.iter().map(|e| e.seq.0).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "stream out of order");
    assert_eq!(change_seqs(&events), all);
}

#[test]
fn an_op_that_writes_nothing_has_an_empty_change_set_no_event_and_the_same_col_mod() {
    let h = Harness::open();
    let (nid, _) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["same", "x"]).value;
    let before = col_mod(&h.collection);
    h.clock.step_ms(5_000);
    let subscription = h.engine.stream().subscribe();

    // Rewrites the note exactly as stored: no change (ids-usn-mtime S4).
    let empty = h
        .collection
        .run(OpSpec::named("noop", OpSource::UserUndoable), move |ctx| {
            let stored = ctx.scope().note(nid)?.unwrap();
            ctx.scope().put(&stored)?;
            Ok(())
        })
        .unwrap();
    assert!(empty.changes.is_empty());
    assert_eq!(empty.undo, None);
    assert_eq!(col_mod(&h.collection), before);

    let next = add_note_with_card(&h.collection, OpSource::UserUndoable, &["next", "y"]);
    assert!(next.seq > empty.seq);
    let events = drain(&subscription, Duration::from_millis(100));
    assert_eq!(change_seqs(&events), [next.seq.0], "only the real op published");
}

#[test]
fn a_local_api_update_queued_before_a_ui_edit_of_the_same_note_commits_first() {
    let h = Harness::open();
    let (nid, _) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["start", "x"]).value;
    let gate = Gate::new();
    let blocker = block_writer(&h.collection, OpSource::UserUndoable, &gate);

    let api = {
        let collection = h.collection.clone();
        thread::spawn(move || set_first_field(&collection, OpSource::LocalApi, nid, "from api").unwrap())
    };
    thread::sleep(Duration::from_millis(50));
    let ui = {
        let collection = h.collection.clone();
        thread::spawn(move || set_first_field(&collection, OpSource::UserUndoable, nid, "from ui").unwrap())
    };
    thread::sleep(Duration::from_millis(50));
    gate.open();
    blocker.wait().unwrap();
    let (api, ui) = (api.join().unwrap(), ui.join().unwrap());
    assert!(api.seq < ui.seq, "arrival order: {:?} then {:?}", api.seq, ui.seq);
    assert_eq!(note_fields(&h.collection, nid), "from ui\u{1f}x");
}

#[test]
fn interactive_ops_go_before_queued_background_ops_but_never_preempt_a_running_one() {
    let h = Harness::open();
    let gate = Gate::new();
    // A background op is running on the writer.
    let running = block_writer(&h.collection, OpSource::SyncIncoming, &gate);
    let background = h
        .collection
        .submit(OpSpec::named("rollover", OpSource::Rollover), |ctx| {
            Ok(ctx.scope().add_note(&note(&["rollover", "x"]))?)
        });
    let optimizer = h
        .collection
        .submit(OpSpec::named("optimizer", OpSource::Optimizer), |ctx| {
            Ok(ctx.scope().add_note(&note(&["optimizer", "x"]))?)
        });
    let answer = h
        .collection
        .submit(OpSpec::named("answer", OpSource::UserUndoable), |ctx| {
            Ok(ctx.scope().add_note(&note(&["answer", "x"]))?)
        });
    let api = h
        .collection
        .submit(OpSpec::named("add_note", OpSource::LocalApi), |ctx| {
            Ok(ctx.scope().add_note(&note(&["api", "x"]))?)
        });
    thread::sleep(Duration::from_millis(30));
    assert!(answer.try_take().is_none(), "the running op is never preempted");
    gate.open();
    let running = running.wait().unwrap();
    let (background, optimizer) = (background.wait().unwrap(), optimizer.wait().unwrap());
    let (answer, api) = (answer.wait().unwrap(), api.wait().unwrap());
    let order = [running.seq, answer.seq, api.seq, background.seq, optimizer.seq];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

#[test]
fn a_panic_inside_a_write_phase_rolls_back_and_the_next_op_succeeds() {
    let h = Harness::open();
    let before = col_mod(&h.collection);
    let count = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    h.clock.step_ms(2_000);
    let failed = h
        .collection
        .run(
            OpSpec::named("panics", OpSource::UserUndoable),
            |ctx| -> Result<(), EngineError> {
                ctx.scope().add_note(&note(&["half", "written"]))?;
                panic!("failpoint inside the write phase");
            },
        )
        .unwrap_err();
    assert!(
        matches!(&failed, EngineError::Panicked { message } if message.contains("failpoint")),
        "{failed:?}"
    );
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM notes"), count);
    assert_eq!(col_mod(&h.collection), before);

    let next = add_note_with_card(&h.collection, OpSource::UserUndoable, &["after", "panic"]);
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes"),
        count + 1
    );
    assert!(!next.changes.is_empty());
}

#[test]
fn an_error_rolls_the_whole_op_back() {
    let h = Harness::open();
    let count = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    let error = h
        .collection
        .run(
            OpSpec::named("fails", OpSource::UserUndoable),
            |ctx| -> Result<(), EngineError> {
                ctx.scope().add_note(&note(&["one", "x"]))?;
                ctx.scope().add_note(&note(&["two", "x"]))?;
                Err(EngineError::InvalidArgument {
                    message: "the third row is invalid".into(),
                })
            },
        )
        .unwrap_err();
    assert!(matches!(error, EngineError::InvalidArgument { .. }));
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM notes"), count);
}

#[test]
fn a_composite_op_keeps_its_good_steps_and_commits_one_change_set() {
    let h = Harness::open();
    let result = h
        .collection
        .run(OpSpec::named("move_note", OpSource::UserUndoable), |ctx| {
            let created = ctx.step(|ctx| {
                let nid = ctx.scope().add_note(&note(&["created", "x"]))?;
                ctx.media_added("created.jpg");
                Ok(nid)
            })?;
            let failed = ctx.step(|ctx| -> Result<NoteId, EngineError> {
                ctx.scope().add_note(&note(&["dropped", "x"]))?;
                ctx.media_added("dropped.jpg");
                Err(EngineError::InvalidArgument {
                    message: "this step fails".into(),
                })
            });
            assert!(failed.is_err());
            let kept = ctx.step(|ctx| Ok(ctx.scope().add_note(&note(&["kept", "x"]))?))?;
            Ok((created, kept))
        })
        .unwrap();
    let (created, kept) = result.value;
    let ids: Vec<NoteId> = result.changes.notes.iter().map(|n| n.id).collect();
    assert_eq!(ids, [created, kept]);
    assert_eq!(result.changes.media_added, ["created.jpg"]);
    assert_eq!(result.undo.map(|u| u.name), Some("move_note"), "one undo entry");
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes WHERE flds LIKE 'dropped%'"),
        0
    );
}

#[test]
fn only_catalog_ops_may_declare_a_long_write_and_a_declared_write_publishes_its_delay() {
    let h = Harness::open();
    let refused = h
        .collection
        .run(
            OpSpec::named("long", OpSource::UserUndoable).declare_long_write(Duration::from_secs(1)),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(matches!(refused, EngineError::InvalidArgument { .. }), "{refused:?}");

    let subscription = h.engine.stream().subscribe();
    let collection = h.collection.clone();
    let result = h
        .collection
        .run(
            OpSpec::new(catalog::CHECK_DATABASE, OpSource::Maintenance).declare_long_write(Duration::from_millis(300)),
            move |ctx| {
                assert_eq!(collection.write_delay().map(|d| d.op), Some("check_database"));
                Ok(ctx.scope().add_note(&note(&["repaired", "x"]))?)
            },
        )
        .unwrap();
    assert_eq!(h.collection.write_delay(), None);
    let kinds: Vec<String> = drain(&subscription, Duration::from_millis(100))
        .iter()
        .map(|e| match &e.kind {
            EventKind::WriteDelay { op, declared } => format!("delay {op} {}", declared.as_millis()),
            EventKind::WriteDelayEnded { op } => format!("ended {op}"),
            EventKind::Changes(_) => format!("changes {}", e.seq.0),
            other => format!("{other:?}"),
        })
        .collect();
    // The op's change set comes before the end of the delay: once U43 sees
    // the delay end, the commit is visible.
    assert_eq!(
        kinds,
        [
            "delay check_database 300".to_owned(),
            format!("changes {}", result.seq.0),
            "ended check_database".to_owned(),
        ]
    );
}

#[test]
fn a_three_second_vacuum_publishes_its_delay_and_an_answer_commits_right_after_it() {
    let h = Harness::open();
    let subscription = h.engine.stream().subscribe();
    let gate = Gate::new();
    let inner = gate.clone();
    let vacuum = h
        .collection
        .submit_maintenance(catalog::VACUUM, Duration::from_secs(3), move |_writer| {
            inner.enter_and_wait();
            thread::sleep(Duration::from_secs(3));
            Ok(())
        });
    gate.wait_entered();
    let delay = h.collection.write_delay().expect("the delay is published");
    assert_eq!((delay.op, delay.declared), ("vacuum", Duration::from_secs(3)));

    // U43 sees the delay and keeps the answer in its overlay and journal; the
    // answer op itself waits for the writer.
    let answer = h
        .collection
        .submit(OpSpec::named("answer", OpSource::UserUndoable), |ctx| {
            Ok(ctx.scope().add_note(&note(&["answer", "x"]))?)
        });
    gate.open();
    let started = monotonic::now();
    vacuum.wait().unwrap();
    let vacuum_done = monotonic::now();
    let answer = answer.wait().unwrap();
    let answer_done = monotonic::now();
    assert!(vacuum_done - started >= Duration::from_secs(3));
    assert!(
        answer_done - vacuum_done < Duration::from_millis(500),
        "the answer commits right after the task: {:?}",
        answer_done - vacuum_done
    );
    let events = drain(&subscription, Duration::from_millis(100));
    let order: Vec<&str> = events
        .iter()
        .map(|e| match &e.kind {
            EventKind::WriteDelay { .. } => "delay",
            EventKind::WriteDelayEnded { .. } => "ended",
            EventKind::Changes(_) => "answer",
            _ => "other",
        })
        .collect();
    assert_eq!(order, ["delay", "ended", "answer"]);
    assert_eq!(events[2].seq, answer.seq);
}

#[test]
fn a_maintenance_task_never_starts_while_an_interactive_op_waits() {
    let h = Harness::open();
    let gate = Gate::new();
    let blocker = block_writer(&h.collection, OpSource::UserUndoable, &gate);
    let order = Arc::new(Mutex::new(Vec::new()));
    let log = order.clone();
    let task = h
        .collection
        .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), move |_| {
            log.lock().unwrap().push("analyze");
            Ok(())
        });
    let log = order.clone();
    let edit = h
        .collection
        .submit(OpSpec::named("edit", OpSource::UserNonUndoable), move |ctx| {
            log.lock().unwrap().push("edit");
            Ok(ctx.scope().add_note(&note(&["edit", "x"]))?)
        });
    gate.open();
    blocker.wait().unwrap();
    task.wait().unwrap();
    edit.wait().unwrap();
    assert_eq!(*order.lock().unwrap(), ["edit", "analyze"]);

    let refused = h
        .collection
        .submit_maintenance(catalog::CHECK_DATABASE, Duration::from_millis(10), |_| Ok(()))
        .wait()
        .unwrap_err();
    assert!(matches!(refused, EngineError::InvalidArgument { .. }));
}

#[test]
fn a_user_undoable_op_reports_its_undo_name_and_other_sources_do_not() {
    let h = Harness::open();
    let undoable = add_note_with_card(&h.collection, OpSource::UserUndoable, &["u", "x"]);
    assert_eq!(
        undoable.undo.map(|u| (u.name, u.source)),
        Some(("add_note", OpSource::UserUndoable))
    );
    assert_eq!(undoable.generation, h.collection.generation());
    for source in [OpSource::LocalApi, OpSource::UserNonUndoable, OpSource::Optimizer] {
        assert_eq!(
            add_note_with_card(&h.collection, source, &["n", "x"]).undo,
            None,
            "{source}"
        );
    }
    // A sync row keeps the usn it arrives with (CallerSupplied).
    let synced = h
        .collection
        .run(OpSpec::named("merge", OpSource::SyncIncoming), |ctx| {
            let mut incoming = note(&["synced", "x"]);
            incoming.id = meguri_model::ids::NoteId(1_700_000_000_000);
            incoming.usn = meguri_model::usn::Usn(42);
            incoming.mtime = meguri_model::time::EpochSecs(1_700_000_000);
            ctx.scope().put(&incoming)?;
            Ok(incoming.id)
        })
        .unwrap();
    let reader = h.collection.reader(meguri_storage::ReaderRole::Query).unwrap();
    let stored = reader.note(synced.value).unwrap().unwrap();
    assert_eq!((stored.usn.0, stored.mtime.0), (42, 1_700_000_000));
}

#[test]
fn ops_on_the_writer_never_run_twice_and_each_result_arrives_once() {
    let h = Harness::open();
    let (nid, _) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["count", "0"]).value;
    let pendings: Vec<_> = (1..=50)
        .map(|i| {
            h.collection
                .submit(OpSpec::named("bump", OpSource::UserUndoable), move |ctx| {
                    let mut stored = ctx.scope().note(nid)?.unwrap();
                    stored.fields = format!("count\u{1f}{i}");
                    ctx.scope().put(&stored)?;
                    Ok(i)
                })
        })
        .collect();
    let values: Vec<i32> = pendings.into_iter().map(|p| p.wait().unwrap().value).collect();
    assert_eq!(values, (1..=50).collect::<Vec<_>>());
    assert_eq!(note_fields(&h.collection, nid), "count\u{1f}50");
}

#[test]
fn an_answer_submitted_while_a_background_op_plans_for_three_seconds_commits_first_within_a_second() {
    let h = Harness::open();
    common::add_notes(&h.collection, 200, "n");
    let planning = Gate::new();
    let background = {
        let collection = h.collection.clone();
        let planning = planning.clone();
        thread::spawn(move || {
            meguri_engine::plan_and_commit(
                &collection,
                meguri_storage::ReaderRole::Maintenance,
                OpSpec::named("optimizer_commit", OpSource::Optimizer),
                |plan| {
                    let notes = plan.snapshot().notes_after(None, 1_000)?;
                    planning.open();
                    // Training-sized planning, off the writer.
                    thread::sleep(Duration::from_secs(3));
                    Ok(meguri_engine::Plan {
                        value: notes.len(),
                        preconditions: Vec::new(),
                    })
                },
                |ctx, planned: usize| Ok((planned, ctx.scope().add_note(&note(&["optimized", "x"]))?)),
            )
            .unwrap()
        })
    };
    planning.wait_open();
    let submitted = monotonic::now();
    let answer = add_note_with_card(&h.collection, OpSource::UserUndoable, &["answer while planning", "x"]);
    let committed = monotonic::now() - submitted;
    assert!(committed < Duration::from_secs(1), "the answer waited {committed:?}");
    // Committed: another connection to the file sees it. The sync that makes
    // it durable on the disk within 1 s is U32's flush
    // (`storage/open-and-connections` C9.2).
    let seen = common::query_int(
        &h.collection,
        "SELECT count(*) FROM notes WHERE flds LIKE 'answer while planning%'",
    );
    assert_eq!(seen, 1);
    assert!(!background.is_finished(), "the background op is still planning");
    let background = background.join().unwrap();
    assert!(background.seq > answer.seq, "the answer committed first");
    assert!(
        background.value.0 >= 200,
        "planned on the snapshot: {}",
        background.value.0
    );
}
