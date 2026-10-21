//! Real writer delays, IO boundaries and kill/reopen twice; no passing stand-ins.
mod session_support;
use meguri_engine::{
    OpSource, OpSpec, SessionId, UndoScope,
    executor::catalog,
    session::{AnswerOutcome, JournalStage},
};
use meguri_model::{
    clock::{Clock, StepClock, UtcOffset},
    ids::DeckId,
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ReadRows, ReaderRole, Table};
use session_support::{Harness, NOW};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
fn accepted(h: &Harness, id: SessionId, head: meguri_engine::session::Head) -> meguri_engine::session::Head {
    let item = &head.items[0];
    let AnswerOutcome::Accepted { head_diff, .. } = h
        .review
        .answer(
            id,
            item.card_id,
            Rating::Easy,
            item.state_token,
            EpochMillis(h.clock.now_ms()),
            200,
        )
        .unwrap()
    else {
        panic!("not accepted")
    };
    head_diff.head
}
fn hold_writer(h: &Harness) -> (mpsc::Sender<()>, meguri_engine::Pending<meguri_engine::OpResult<()>>) {
    let (ready, wait) = mpsc::sync_channel(1);
    let (release, released) = mpsc::channel();
    let pending = h.collection.submit(
        OpSpec::new(catalog::BULK_OP, OpSource::UserUndoable).declare_long_write(Duration::from_secs(3)),
        move |_| {
            ready.send(()).unwrap();
            released.recv().unwrap();
            Ok(())
        },
    );
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    (release, pending)
}
#[test]
fn c11_failure_three_of_five_cancels_later_answers_and_restores_failed_head() {
    let h = Harness::new(8);
    let (id, mut head) = h.start();
    let third = h.cards[2];
    let stream = h.engine.stream().subscribe();
    let (release, pending) = hold_writer(&h);
    h.review.fail_application_for_testing(3);
    for _ in 0..5 {
        head = accepted(&h, id, head);
    }
    release.send(()).unwrap();
    pending.wait().unwrap();
    h.settle();
    assert_eq!(h.logs().len(), 2);
    assert_eq!(h.review.head(id, 3).unwrap().items[0].card_id, third);
    let mut failed = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind
            && let Some(meguri_engine::session::SessionEvent::CommitFailed { card_id, .. }) = e.as_any().downcast_ref()
        {
            assert_eq!(*card_id, third);
            failed = true;
        }
    }
    assert!(failed, "failure must reach the existing ordered core stream");
}
#[test]
fn c15_pending_undo_behind_declared_write_never_inserts_revlog() {
    let h = Harness::new(5);
    let (id, head) = h.start();
    let card = head.items[0].card_id;
    let (release, pending) = hold_writer(&h);
    accepted(&h, id, head);
    let restored = h.review.undo(UndoScope::Session(id)).unwrap();
    assert_eq!(restored.items[0].card_id, card);
    release.send(()).unwrap();
    pending.wait().unwrap();
    h.settle();
    assert!(h.logs().is_empty());
}
#[test]
fn c15_global_undo_drains_two_uncommitted_answers_before_targeting_u25() {
    let h = Harness::new(5);
    let (id, head) = h.start();
    let (release, pending) = hold_writer(&h);
    let head = accepted(&h, id, head);
    accepted(&h, id, head);
    let review = h.review.clone();
    let worker = h
        .engine
        .threads()
        .spawn("test-global-undo", meguri_engine::Qos::Default, move |_| {
            review.undo(UndoScope::Global)
        })
        .unwrap();
    release.send(()).unwrap();
    pending.wait().unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(h.logs().len(), 1);
}
#[test]
fn c12_prepared_ack_io_failure_rolls_back_and_retains_intent_and_identity() {
    let h = Harness::new(2);
    let (id, head) = h.start();
    h.review.fail_journal_for_testing(JournalStage::PreparedSync).unwrap();
    accepted(&h, id, head);
    assert!(h.review.flush_for_background().is_err());
    assert!(h.logs().is_empty());
    let path = h.dir.path().join("meguri-pending-answers.jsonl");
    let bytes = std::fs::read_to_string(path).unwrap();
    assert!(bytes.contains("Intent"));
    assert!(bytes.contains("Prepared"));
}
#[test]
fn c12_terminal_sync_failure_keeps_committed_answer_and_evidence() {
    let h = Harness::new(2);
    let (id, head) = h.start();
    h.review.fail_journal_for_testing(JournalStage::TerminalSync).unwrap();
    accepted(&h, id, head);
    assert!(h.review.flush_for_background().is_err());
    assert_eq!(h.logs().len(), 1);
    assert!(
        std::fs::read_to_string(h.dir.path().join("meguri-pending-answers.jsonl"))
            .unwrap()
            .contains("Prepared")
    );
}
#[test]
fn c12_kill_at_intent_prepared_commit_wal_and_done_then_reopen_twice() {
    use std::{
        io::{BufRead, Write},
        process::{Command, Stdio},
    };
    const CHILD: &str = "MEGURI_REVIEW_CRASH_CHILD";
    if let Some(mode) = std::env::var_os(CHILD) {
        let path = std::path::PathBuf::from(std::env::var_os("MEGURI_REVIEW_CRASH_PATH").unwrap());
        let mode = mode.to_string_lossy().into_owned();
        let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
        let (_engine, c, review) = session_support::open(&path, clock.clone());
        let (id, head) = match review.start(DeckId(1), None).unwrap() {
            meguri_engine::session::StartResult::Started { session_id, head, .. } => (session_id, head),
            _ => panic!("child admission"),
        };
        let c_for_hook = c.clone();
        review
            .journal_hook_for_testing(Arc::new(move |stage| {
                let stop = match mode.as_str() {
                    "intent" => stage == JournalStage::IntentSynced,
                    "prepared" => stage == JournalStage::PreparedSynced,
                    "wal" => stage == JournalStage::TerminalWrite,
                    "done" => stage == JournalStage::TerminalSynced,
                    "commit" => {
                        if stage == JournalStage::PreparedSynced {
                            c_for_hook.after_next_commit(|_| {
                                println!("CRASH-READY");
                                std::io::stdout().flush().unwrap();
                                loop {
                                    std::thread::park();
                                }
                            });
                        }
                        false
                    }
                    _ => panic!("unknown crash boundary"),
                };
                if stop {
                    println!("CRASH-READY");
                    std::io::stdout().flush().unwrap();
                    loop {
                        std::thread::park();
                    }
                }
            }))
            .unwrap();
        let first = &head.items[0];
        assert!(matches!(
            review
                .answer(id, first.card_id, Rating::Easy, first.state_token, EpochMillis(NOW), 10)
                .unwrap(),
            AnswerOutcome::Accepted { .. }
        ));
        review.flush_for_background().unwrap();
        panic!("crash boundary never reached");
    }
    for mode in ["intent", "prepared", "commit", "wal", "done"] {
        let h = Harness::new(3);
        let path = h.collection.path().to_owned();
        h.engine.close().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "c12_kill_at_intent_prepared_commit_wal_and_done_then_reopen_twice",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("MEGURI_REVIEW_CRASH_PATH", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (signal, ready) = mpsc::sync_channel(1);
        let reader = h
            .engine
            .threads()
            .spawn("test-crash-checkpoint", meguri_engine::Qos::Default, move |_| {
                let observed = std::io::BufReader::new(stdout)
                    .lines()
                    .any(|line| line.unwrap().contains("CRASH-READY"));
                let _ = signal.send(observed);
            })
            .unwrap();
        let reached = ready.recv_timeout(Duration::from_secs(15));
        let _ = child.kill();
        child.wait().unwrap();
        reader.join().unwrap();
        assert!(reached.unwrap(), "child exited before {mode}");
        for _ in 0..2 {
            let (engine, c, _review) =
                session_support::open(&path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
            assert_eq!(
                c.reader(ReaderRole::Query)
                    .unwrap()
                    .ids_after(Table::Revlog, None, 100)
                    .unwrap()
                    .len(),
                1,
                "{mode} must replay at most once"
            );
            engine.close().unwrap();
        }
    }
}

#[test]
fn c15_committed_undo_returns_restored_head_during_real_200ms_sync_incoming() {
    use meguri_storage::RowKey;

    let h = Harness::new(4);
    let (session, original) = h.start();
    let card_id = original.items[0].card_id;
    accepted(&h, session, original.clone());
    h.settle();
    let assigned_logs = h.logs();
    assert_eq!(assigned_logs.len(), 1);
    let assigned_card = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(card_id.0))
        .unwrap()
        .unwrap();
    let assigned_log = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Revlog, &RowKey::Id(assigned_logs[0]))
        .unwrap()
        .unwrap();

    let (notify, started) = mpsc::sync_channel(1);
    let (_keep_sync_busy, sync_window) = mpsc::channel::<()>();
    let duration = Duration::from_millis(200);
    let writing = h.collection.submit(
        OpSpec::new(catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming).declare_long_write(duration),
        move |_| {
            notify.send(()).unwrap();
            Ok(matches!(
                sync_window.recv_timeout(duration),
                Err(mpsc::RecvTimeoutError::Timeout)
            ))
        },
    );
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let (deliver, delivery) = mpsc::sync_channel(1);
    let service = h.review.clone();
    let caller = h
        .engine
        .threads()
        .spawn("test-session-undo-return", meguri_engine::Qos::Default, move |_| {
            let result = service.undo(UndoScope::Session(session));
            deliver.send(result).unwrap();
        })
        .unwrap();

    let returned = delivery.recv_timeout(Duration::from_millis(50));
    let presented_during_sync = h.review.head(session, 3);
    let logs_during_sync = h.logs();
    let occupied_for_full_window = writing.wait().unwrap().value;
    caller.join().unwrap();
    h.settle();

    assert!(occupied_for_full_window);
    assert_eq!(returned.expect("session undo exceeded 50 ms").unwrap(), original);
    assert_eq!(presented_during_sync.unwrap(), original);
    assert_eq!(logs_during_sync, assigned_logs);
    assert!(h.logs().is_empty());
    assert_eq!(h.review.head(session, 3).unwrap().items[0].card_id, card_id);

    h.review.redo(UndoScope::Session(session)).unwrap();
    assert_eq!(h.logs(), assigned_logs);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        reader.row_image(Table::Cards, &RowKey::Id(card_id.0)).unwrap().unwrap(),
        assigned_card
    );
    assert_eq!(
        reader
            .row_image(Table::Revlog, &RowKey::Id(assigned_logs[0]))
            .unwrap()
            .unwrap(),
        assigned_log
    );
}

#[test]
fn c15_failed_committed_undo_reapplies_metadata_and_reports_on_core_stream() {
    let h = Harness::new(4);
    let (id, before) = h.start();
    let first = before.items[0].card_id;
    let after = accepted(&h, id, before.clone());
    h.settle();
    let stream = h.engine.stream().subscribe();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, held) = mpsc::channel();
    let sync = h.collection.submit(
        OpSpec::new(catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming)
            .declare_long_write(Duration::from_millis(200)),
        move |ctx| {
            entered.send(()).unwrap();
            held.recv().unwrap();
            // U25 must reject the now-conflicting answer step. The UI has
            // already undone it; the worker must redo its queue metadata.
            let mut card = ctx.scope().card(first)?.unwrap();
            card.flags ^= 1;
            ctx.scope().put(&card)?;
            Ok(())
        },
    );
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let optimistic = h.review.undo(UndoScope::Session(id)).unwrap();
    assert_eq!(optimistic.items[0].card_id, first);
    release.send(()).unwrap();
    sync.wait().unwrap();
    h.settle();
    assert_eq!(h.logs().len(), 1);
    assert_eq!(h.review.head(id, 3).unwrap().items[0].card_id, after.items[0].card_id);
    let mut failure_seq = None;
    let mut restored_after_failure = false;
    while let Some(event) = stream.try_recv() {
        assert_eq!(event.generation, h.collection.generation());
        if let meguri_engine::EventKind::Extension(e) = &event.kind {
            match e.as_any().downcast_ref::<meguri_engine::session::SessionEvent>() {
                Some(meguri_engine::session::SessionEvent::CommitFailed { card_id, .. }) => {
                    assert_eq!(*card_id, first);
                    failure_seq = Some(event.seq);
                }
                Some(meguri_engine::session::SessionEvent::QueueHeadChanged { diff, .. }) if failure_seq.is_some() => {
                    assert!(event.seq > failure_seq.unwrap());
                    assert_eq!(diff.head.items[0].card_id, after.items[0].card_id);
                    restored_after_failure = true;
                }
                _ => {}
            }
        }
    }
    assert!(failure_seq.is_some() && restored_after_failure);
}

#[test]
fn c11_initial_intent_barrier_failure_restores_and_reports_instead_of_tracing_only() {
    let h = Harness::new(4);
    let (id, before) = h.start();
    let stream = h.engine.stream().subscribe();
    h.review.fail_journal_for_testing(JournalStage::IntentSync).unwrap();
    accepted(&h, id, before.clone());
    assert!(h.review.flush_for_background().is_err());
    assert!(h.logs().is_empty());
    assert!(
        h.review
            .answer(
                id,
                before.items[0].card_id,
                Rating::Good,
                before.items[0].state_token,
                EpochMillis(h.clock.now_ms()),
                1
            )
            .is_err()
    );
    let mut failed = false;
    let mut restored = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind {
            match e.as_any().downcast_ref::<meguri_engine::session::SessionEvent>() {
                Some(meguri_engine::session::SessionEvent::CommitFailed { card_id, .. }) => {
                    assert_eq!(*card_id, before.items[0].card_id);
                    failed = true;
                }
                Some(meguri_engine::session::SessionEvent::QueueHeadChanged { diff, .. }) => {
                    assert!(failed);
                    assert_eq!(diff.head.items[0].card_id, before.items[0].card_id);
                    restored = true;
                }
                _ => {}
            }
        }
    }
    assert!(failed && restored);
    assert!(
        std::fs::read_to_string(h.dir.path().join("meguri-pending-answers.jsonl"))
            .unwrap()
            .contains("Intent")
    );
}

#[test]
fn c12_every_acceptance_is_observed_fullsynced_while_a_later_writer_is_held() {
    let h = Harness::new(80);
    let (id, mut head) = h.start();
    let (covered, coverage) = mpsc::channel();
    let observation_clock = meguri_testkit::wallclock::Stopwatch::start();
    h.review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == JournalStage::IntentSynced {
                covered.send(observation_clock.elapsed()).unwrap();
            }
        }))
        .unwrap();
    let mut starts = vec![observation_clock.elapsed()];
    head = accepted(&h, id, head);
    // The delay starts after the first Accepted, rather than protecting only
    // answers accepted after a pre-existing WriteDelay announcement.
    let (release, writer) = hold_writer(&h);
    for _ in 1..64 {
        starts.push(observation_clock.elapsed());
        head = accepted(&h, id, head);
    }
    for start in starts {
        let synced = coverage.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(synced.saturating_sub(start) <= Duration::from_secs(1));
    }
    let journal = std::fs::read_to_string(h.dir.path().join("meguri-pending-answers.jsonl")).unwrap();
    assert_eq!(journal.lines().filter(|line| line.contains("Intent")).count(), 64);
    release.send(()).unwrap();
    writer.wait().unwrap();
    h.settle();
    assert_eq!(
        h.logs().len(),
        64,
        "every acknowledged acceptance survives the writer delay"
    );
}

#[test]
fn c9_balancer_two_optimistic_same_card_choices_count_once_through_commit_undo_redo() {
    use meguri_model::{card::CardQueue, ids::DeckConfigId};
    use meguri_sched::states::{engine::State, interval_limits::Delay, labels::format_seconds};
    use meguri_storage::{Cell, ConfigValue, RowKey};

    let h = Harness::new(2);
    let subject = h.cards[0];
    let bucket_member = h.cards[1];
    h.collection
        .run(
            OpSpec::named("same-card cache exercise", OpSource::Maintenance),
            move |ctx| {
                ctx.scope()
                    .set_config("loadBalancerEnabled", &ConfigValue::Bool(true))?;
                let mut target = ctx.scope().card(subject)?.unwrap();
                target.due = 1000;
                ctx.scope().put(&target)?;
                // Retain an existing W2 preset list without gathering this card.
                let mut counted = ctx.scope().card(bucket_member)?.unwrap();
                counted.queue = CardQueue::Suspended;
                ctx.scope().put(&counted)?;
                Ok(())
            },
        )
        .unwrap();
    let (session, initial) = h.start();
    let contributions = || {
        let cache = h.review.chooser_for_testing(session).unwrap();
        (0..=98).fold(0usize, |total, day| {
            total
                + cache
                    .entries(DeckConfigId(1), day)
                    .iter()
                    .filter(|entry| entry.card == subject)
                    .count()
        })
    };
    let answer = |head: &meguri_engine::session::Head, rating, elapsed| match h
        .review
        .answer(
            session,
            subject,
            rating,
            head.items[0].state_token,
            EpochMillis(h.clock.now_ms()),
            elapsed,
        )
        .unwrap()
    {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("same-card answer was refused: {other:?}"),
    };

    assert_eq!(contributions(), 0);
    let first_choice = initial.items[0].next_states.again.clone();
    let (release, writing) = hold_writer(&h);
    let learning_head = answer(&initial, Rating::Again, 17);
    let after_learning = contributions();
    let second_choice = learning_head.items[0].next_states.easy.clone();
    answer(&learning_head, Rating::Easy, 29);
    let after_graduation = contributions();
    let State::Review(graduation) = second_choice.state else {
        release.send(()).unwrap();
        writing.wait().unwrap();
        panic!("Easy did not choose review");
    };
    let chosen_day_count = h
        .review
        .chooser_for_testing(session)
        .unwrap()
        .entries(DeckConfigId(1), graduation.scheduled_days)
        .iter()
        .filter(|entry| entry.card == subject)
        .count();
    let uncommitted_logs = h.logs();
    release.send(()).unwrap();
    writing.wait().unwrap();
    h.settle();

    assert_eq!(learning_head.items[0].card_id, subject);
    assert!(matches!(first_choice.state, State::Learning(_)));
    assert!(matches!(first_choice.delay, Delay::Seconds(_)));
    assert_eq!(after_learning, 0);
    assert_eq!(after_graduation, 1);
    assert_eq!(chosen_day_count, 1);
    assert!(uncommitted_logs.is_empty());
    assert_eq!(contributions(), 1);
    assert_eq!(second_choice.delay, Delay::Days(graduation.scheduled_days));
    let assigned = h.logs();
    assert_eq!(assigned.len(), 2);
    for (log_id, choice) in assigned.iter().zip([&first_choice, &second_choice]) {
        assert_eq!(choice.label, format_seconds(choice.delay.label_seconds(), 1200));
        let row = h
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Revlog, &RowKey::Id(*log_id))
            .unwrap()
            .unwrap();
        assert_eq!(row[4], Cell::Integer(i64::from(choice.revlog.interval)));
    }
    let committed = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(subject)
        .unwrap()
        .unwrap();
    assert_eq!(committed.interval, graduation.scheduled_days);

    let undone = h.review.undo(UndoScope::Session(session)).unwrap();
    assert_eq!(undone.items[0].card_id, subject);
    assert_eq!(contributions(), 0);
    h.review.redo(UndoScope::Session(session)).unwrap();
    assert_eq!(contributions(), 0);
    assert_eq!(h.logs(), assigned);
    let redone = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(subject)
        .unwrap()
        .unwrap();
    assert_eq!(redone.id, committed.id);
    assert_eq!(redone.interval, committed.interval);
    assert_eq!(redone.due, committed.due);
    h.review.undo(UndoScope::Session(session)).unwrap();
    assert_eq!(contributions(), 0);
}

#[test]
fn c17_last_acceptance_emits_finished_without_an_explicit_head_poll() {
    let h = Harness::new(1);
    let (id, head) = h.start();
    let stream = h.engine.stream().subscribe();
    assert!(accepted(&h, id, head).items.is_empty());
    h.settle();
    let mut finished = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind
            && let Some(meguri_engine::session::SessionEvent::Finished { session_id, state }) =
                e.as_any().downcast_ref()
        {
            assert_eq!(*session_id, id);
            let actual = meguri_engine::sched::queue::QueueProvider::default()
                .deck_study_state(&h.collection, Some(DeckId(1)))
                .unwrap();
            assert_eq!(*state, actual, "Finished uses U41's production authority");
            finished = true;
        }
    }
    assert!(finished);
}

#[test]
fn c17_waiting_until_due_minus_learn_ahead_is_emitted_at_acceptance_before_writer_release() {
    let h = Harness::new(1);
    h.collection
        .run(OpSpec::named("held learning fixture", OpSource::Maintenance), |ctx| {
            let mut preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
            let mut fields = (*preset.config).clone();
            fields.learn_steps = vec![60.0];
            preset.config = meguri_model::proto::ProtoBlob::new(fields);
            ctx.scope().put(&preset)?;
            ctx.scope()
                .set_config("collapseTime", &meguri_storage::ConfigValue::from(1200u64))?;
            Ok(())
        })
        .unwrap();
    let (id, before) = h.start();
    let item = &before.items[0];
    let accepted_transition = item.next_states.again.clone();
    let meguri_sched::states::interval_limits::Delay::Seconds(seconds) = accepted_transition.delay else {
        panic!("the selected learning transition must remain intraday");
    };
    let original_card = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(item.card_id)
        .unwrap()
        .unwrap();
    let answered_at = EpochMillis(h.clock.now_ms());
    let seed = meguri_sched::states::fuzz::seed(original_card.id.0 as u64, u64::from(original_card.reps));
    let expected_due =
        answered_at.0.div_euclid(1000) + i64::from(meguri_sched::states::fuzz::learning_delay(seconds, seed));
    // One card leaves no competing learning entry for P5 to move it behind.
    let expected_wait = expected_due - 1200;
    let stream = h.engine.stream().subscribe();
    let (release, writer) = hold_writer(&h);
    let outcome = h
        .review
        .answer(id, item.card_id, Rating::Again, item.state_token, answered_at, 10);
    let mut waits = Vec::new();
    let mut finished = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind {
            match e.as_any().downcast_ref::<meguri_engine::session::SessionEvent>() {
                Some(meguri_engine::session::SessionEvent::WaitingUntil { session_id, instant }) => {
                    waits.push((*session_id, *instant));
                }
                Some(meguri_engine::session::SessionEvent::Finished { .. }) => finished = true,
                _ => {}
            }
        }
    }
    let logs_before_release = h.logs();
    release.send(()).unwrap();
    writer.wait().unwrap();
    h.settle();

    let AnswerOutcome::Accepted { head_diff, .. } = outcome.unwrap() else {
        panic!("the learning answer was not accepted");
    };
    assert!(head_diff.head.items.is_empty());
    assert_eq!(waits, vec![(id, expected_wait)]);
    assert!(!finished, "held learning must emit WaitingUntil");
    assert!(logs_before_release.is_empty());
    let committed = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(item.card_id)
        .unwrap()
        .unwrap();
    assert_eq!(i64::from(committed.due), expected_due);
    assert_eq!(committed.queue, meguri_model::card::CardQueue::Learning);
    assert_eq!(h.logs().len(), 1);
}
