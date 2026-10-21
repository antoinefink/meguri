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

    for mode in ["sync", "ordinary", "rebuild", "expired"] {
        let h = Harness::new(4);
        if mode == "expired" {
            let learning = h.cards[3];
            h.collection
                .run(
                    OpSpec::named("held learning validity", OpSource::Maintenance),
                    move |ctx| {
                        let mut card = ctx.scope().card(learning)?.unwrap();
                        card.card_type = meguri_model::card::CardType::Learning;
                        card.queue = meguri_model::card::CardQueue::Learning;
                        card.due = (NOW / 1000 + 1205) as i32;
                        card.reps = 1;
                        card.remaining_steps = 1001;
                        ctx.scope().put(&card)?;
                        Ok(())
                    },
                )
                .unwrap();
        }
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

        if mode == "rebuild" {
            let timer = !original.items[0].home_preset.show_timer;
            h.collection
                .run(
                    OpSpec::named("unrelated background rebuild", OpSource::Rollover),
                    move |ctx| {
                        ctx.scope()
                            .set_config("collapseTime", &meguri_storage::ConfigValue::from(600u64))?;
                        meguri_engine::deck_config::update_in(
                            ctx.scope(),
                            meguri_model::ids::DeckConfigId(1),
                            |fields| {
                                fields.show_timer = timer;
                            },
                        )?;
                        Ok(())
                    },
                )
                .unwrap();
            h.settle();
        }
        if mode == "expired" {
            let before_day = h.collection.today().unwrap();
            h.clock.set_now_ms((original.valid_until + 1) * 1000);
            assert_eq!(h.collection.today().unwrap(), before_day);
            assert!(h.clock.now_ms() / 1000 < h.collection.next_day_at().unwrap());
        }
        let cache_before = h.review.chooser_for_testing(session).unwrap();
        let (notify, started) = mpsc::sync_channel(1);
        let (_keep_sync_busy, sync_window) = mpsc::channel::<()>();
        // An ordinary short op stays below O8's ~200 ms declaration threshold.
        let duration = Duration::from_millis(if mode == "ordinary" { 100 } else { 200 });
        let spec = if mode == "ordinary" {
            OpSpec::named("ordinary writer window", OpSource::Rollover)
        } else {
            OpSpec::new(catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming).declare_long_write(duration)
        };
        let writing = h.collection.submit(spec, move |_| {
            notify.send(()).unwrap();
            Ok(matches!(
                sync_window.recv_timeout(duration),
                Err(mpsc::RecvTimeoutError::Timeout)
            ))
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(h.collection.write_delay().is_some(), mode != "ordinary");
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
        let cache_during_sync = h.review.chooser_for_testing(session).unwrap();
        let occupied_for_full_window = writing.wait().unwrap().value;
        caller.join().unwrap();
        h.settle();

        assert!(occupied_for_full_window);
        let returned = returned.expect("session undo exceeded 50 ms").unwrap();
        assert_eq!(returned.items[0].card_id, card_id, "{mode}");
        assert_eq!(presented_during_sync.unwrap(), returned, "{mode}");
        assert_eq!(
            h.review.head(session, 3).unwrap(),
            returned,
            "optimistic and actual U25 head: {mode}"
        );
        if mode == "rebuild" {
            assert_ne!(returned.items[0].state_token, original.items[0].state_token);
            assert_ne!(
                returned.items[0].home_preset.show_timer,
                original.items[0].home_preset.show_timer
            );
            assert_eq!(cache_during_sync, cache_before, "W9 leaves a rebuilt cache alone");
            assert_eq!(h.review.chooser_for_testing(session).unwrap(), cache_before);
        } else if mode == "expired" {
            assert!(returned.valid_until > h.clock.now_ms() / 1000);
            assert_ne!(returned.valid_until, original.valid_until);
            assert_eq!(
                returned.counts, original.counts,
                "P9 retains its before-answer cutoff/count snapshot"
            );
        } else {
            assert_eq!(returned, original);
        }
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

#[test]
fn c15_multiple_pending_redos_keep_each_effective_instant_rating_and_elapsed() {
    use meguri_storage::RowKey;

    let h = Harness::new(1);
    let (id, initial) = h.start();
    let card = initial.items[0].card_id;
    let (release, writing) = hold_writer(&h);
    let instants = [NOW - 10_000, NOW - 1000];
    let elapsed = [99, 1234];
    let mut head = initial.clone();
    for index in 0..2 {
        let AnswerOutcome::Accepted { head_diff, .. } = h
            .review
            .answer(
                id,
                card,
                Rating::Again,
                head.items[0].state_token,
                EpochMillis(instants[index]),
                elapsed[index],
            )
            .unwrap()
        else {
            panic!("pending learning answer refused")
        };
        assert_eq!(head_diff.head.items[0].card_id, card);
        head = head_diff.head;
    }
    assert_eq!(h.review.undo(UndoScope::Session(id)).unwrap().items[0].card_id, card);
    assert_eq!(
        h.review.undo(UndoScope::Session(id)).unwrap().items[0].state_token,
        initial.items[0].state_token
    );
    assert!(h.logs().is_empty());
    h.clock.step_ms(10_000);
    let first_redo = h.review.redo(UndoScope::Session(id)).unwrap();
    assert_eq!(first_redo.items[0].card_id, card);
    // The second pending redo must survive accepting the first redo.
    h.review.redo(UndoScope::Session(id)).unwrap();
    assert!(h.logs().is_empty());
    release.send(()).unwrap();
    writing.wait().unwrap();
    h.settle();
    assert_eq!(
        h.logs(),
        instants,
        "redo must not clamp an older retained instant to last_answer"
    );
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    for index in 0..2 {
        let row = reader
            .revlog_entry(meguri_model::ids::RevlogId(instants[index]))
            .unwrap()
            .unwrap();
        assert_eq!(row.card_id, card);
        assert_eq!(row.button, Rating::Again as u8);
        assert_eq!(row.answer_millis, elapsed[index] as u32);
        assert_eq!(row.kind, meguri_model::revlog::RevlogKind::Learning);
        assert!(row.interval < 0);
        assert!(row.ease_factor > 0);
    }
    let stored = reader.card(card).unwrap().unwrap();
    assert_eq!(stored.reps, 2);
    assert_eq!(stored.data.last_review_secs, Some(instants[1] / 1000));
    let assigned_card = reader.row_image(Table::Cards, &RowKey::Id(card.0)).unwrap().unwrap();
    let assigned_log = reader
        .row_image(Table::Revlog, &RowKey::Id(instants[1]))
        .unwrap()
        .unwrap();
    drop(reader);
    h.review.undo(UndoScope::Session(id)).unwrap();
    h.settle();
    h.review.redo(UndoScope::Session(id)).unwrap();
    assert_eq!(h.logs(), instants);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        reader.row_image(Table::Cards, &RowKey::Id(card.0)).unwrap().unwrap(),
        assigned_card
    );
    assert_eq!(
        reader
            .row_image(Table::Revlog, &RowKey::Id(instants[1]))
            .unwrap()
            .unwrap(),
        assigned_log
    );
}

#[test]
fn c9_w2_no_list_stays_absent_through_real_accept_commit_undo_redo() {
    use meguri_engine::sched::answer::predict;
    use meguri_model::ids::DeckConfigId;
    use meguri_sched::states::{
        fsrs_glue::FsrsCache,
        interval_limits::{Delay, FuzzedDayChooser},
    };
    use meguri_storage::{ConfigValue, RowKey};

    let fixture = Harness::new(1);
    let card_id = fixture.cards[0];
    fixture
        .collection
        .run(
            OpSpec::named("enable balancing outside count window", OpSource::Maintenance),
            move |op| {
                let scope = op.scope();
                let mut row = scope.card(card_id)?.unwrap();
                row.due = 1000;
                scope.put(&row)?;
                scope.set_config("loadBalancerEnabled", &ConfigValue::Bool(true))?;
                Ok(())
            },
        )
        .unwrap();

    let (session_id, mut shown) = fixture.start();
    let built_cache = fixture.review.chooser_for_testing(session_id).unwrap();
    let require_no_preset_entries = |phase: &str| {
        let observed = fixture.review.chooser_for_testing(session_id).unwrap();
        assert_eq!(observed, built_cache, "cache changed during {phase}");
        let preset_id = DeckConfigId(1);
        for offset in 0..99 {
            assert!(observed.entries(preset_id, offset).is_empty(), "day {offset}: {phase}");
        }
        let mut offer = observed.chooser(preset_id, None, card_id.0 as u64);
        assert_eq!(offer.choose(30.0, 1, 36500), None, "missing preset list: {phase}");
    };
    require_no_preset_entries("admission");
    assert_eq!(
        shown.items[0].next_states,
        predict(&fixture.collection, card_id, &mut FsrsCache::default(), None)
            .unwrap()
            .states,
        "an unavailable balancing choice must retain ordinary fuzz"
    );

    let (unblock, occupied) = hold_writer(&fixture);
    let mut graduation_days = None;
    for (rating, millis) in [(Rating::Again, 17), (Rating::Easy, 29)] {
        let current = &shown.items[0];
        assert_eq!(current.card_id, card_id);
        if matches!(rating, Rating::Easy) {
            let Delay::Days(days) = current.next_states.easy.delay else {
                panic!("graduation must select a day interval");
            };
            graduation_days = Some(days);
        }
        shown = match fixture
            .review
            .answer(
                session_id,
                card_id,
                rating,
                current.state_token,
                EpochMillis(NOW),
                millis,
            )
            .unwrap()
        {
            AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
            other => panic!("optimistic answer was refused: {other:?}"),
        };
        require_no_preset_entries("accepted while writer occupied");
        assert!(
            fixture.logs().is_empty(),
            "acceptance must precede database application"
        );
    }
    unblock.send(()).unwrap();
    occupied.wait().unwrap();
    fixture.settle();
    require_no_preset_entries("both commits");

    let capture_assigned_rows = || {
        let ids = fixture.logs();
        let reader = fixture.collection.reader(ReaderRole::Query).unwrap();
        let card_row = reader.row_image(Table::Cards, &RowKey::Id(card_id.0)).unwrap().unwrap();
        let log_rows = ids
            .iter()
            .map(|id| reader.row_image(Table::Revlog, &RowKey::Id(*id)).unwrap().unwrap())
            .collect::<Vec<_>>();
        (ids, card_row, log_rows)
    };
    let committed_rows = capture_assigned_rows();
    assert_eq!(committed_rows.0.len(), 2);
    assert_eq!(
        fixture
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(card_id)
            .unwrap()
            .unwrap()
            .interval,
        graduation_days.unwrap()
    );

    let restored_question = fixture.review.undo(UndoScope::Session(session_id)).unwrap();
    require_no_preset_entries("optimistic undo");
    fixture.settle();
    require_no_preset_entries("applied undo");
    assert_eq!(
        restored_question.items[0].next_states,
        predict(&fixture.collection, card_id, &mut FsrsCache::default(), None)
            .unwrap()
            .states
    );
    fixture.review.redo(UndoScope::Session(session_id)).unwrap();
    require_no_preset_entries("redo");
    assert_eq!(
        capture_assigned_rows(),
        committed_rows,
        "redo must reuse the assigned rows"
    );
}

#[test]
fn c15_terminal_sync_error_blocks_committed_undo_and_restores_answered_queue() {
    use meguri_engine::session::SessionEvent;
    use meguri_storage::RowKey;
    use std::sync::Mutex;

    let h = Harness::new(3);
    let (id, initial) = h.start();
    let item = &initial.items[0];
    let card = item.card_id;
    let events = h.engine.stream().subscribe();
    let read_answer = || {
        let ids = h.logs();
        assert_eq!(ids.len(), 1);
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let rows = [(Table::Cards, card.0), (Table::Revlog, ids[0])]
            .map(|(table, key)| reader.row_image(table, &RowKey::Id(key)).unwrap().unwrap());
        (ids, rows)
    };

    let (entered, entry) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::channel();
    let resume = Mutex::new(resume);
    h.review.fail_journal_for_testing(JournalStage::TerminalSync).unwrap();
    h.review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == JournalStage::TerminalWrite {
                entered.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }))
        .unwrap();
    let answered = match h
        .review
        .answer(
            id,
            card,
            Rating::Easy,
            item.state_token,
            EpochMillis(h.clock.now_ms()),
            200,
        )
        .unwrap()
    {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        result => panic!("answer rejected: {result:?}"),
    };
    // A full flush would release the durability error before the undo request.
    h.review.application_barrier_for_testing().unwrap();
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    let saved = read_answer();

    let optimistic = h.review.undo(UndoScope::Session(id)).unwrap();
    assert_eq!(optimistic.items[0].card_id, card);
    assert_eq!(read_answer(), saved, "undo wrote while terminal writer was held");
    release.send(()).unwrap();
    assert!(h.review.flush_for_background().is_err());
    h.review.application_barrier_for_testing().unwrap();
    assert_eq!(read_answer(), saved, "terminal failure changed committed rows");
    assert_eq!(
        h.collection.peek_undo(UndoScope::Session(id)).unwrap().name(),
        Some("Answer Card")
    );

    let evidence = std::fs::read_to_string(h.dir.path().join("meguri-pending-answers.jsonl")).unwrap();
    assert!(evidence.contains("Intent"), "missing intent");
    assert!(evidence.contains("Prepared"), "missing prepared record");

    let mut last_failure = None;
    let mut restoration = None;
    while let Some(event) = events.try_recv() {
        if let meguri_engine::EventKind::Extension(extension) = &event.kind {
            match extension.as_any().downcast_ref::<SessionEvent>() {
                Some(SessionEvent::CommitFailed { card_id, error, .. }) => {
                    assert_eq!(*card_id, card);
                    assert!(format!("{error:?}").contains("TerminalSync"));
                    last_failure = Some(event.seq);
                    restoration = None;
                }
                Some(SessionEvent::QueueHeadChanged { diff, .. }) if last_failure.is_some() => {
                    assert_eq!(diff.head, answered);
                    restoration = Some(event.seq);
                }
                _ => {}
            }
        }
    }
    let failed_at = last_failure.expect("missing commit failure");
    assert!(restoration.expect("missing queue restoration") > failed_at);

    let next = &answered.items[0];
    assert!(
        h.review
            .answer(id, next.card_id, Rating::Easy, next.state_token, EpochMillis(NOW), 1)
            .is_err(),
        "answer accepted after journal failure"
    );
}

#[test]
fn c15_committed_leech_undo_renders_projected_note_while_writer_is_withheld() {
    use meguri_model::{
        card::{CardQueue, CardType},
        ids::DeckConfigId,
    };
    let h = Harness::new(2);
    let card = h.cards[0];
    h.collection
        .run(
            OpSpec::named("leech rendering setup", OpSource::Maintenance),
            move |ctx| {
                let mut row = ctx.scope().card(card)?.unwrap();
                row.card_type = CardType::Review;
                row.queue = CardQueue::Review;
                row.due = 0;
                row.interval = 10;
                row.ease_factor = 2500;
                row.reps = 10;
                row.lapses = 7;
                ctx.scope().put(&row)?;
                meguri_engine::deck_config::update_in(ctx.scope(), DeckConfigId(1), |p| {
                    p.leech_threshold = 8;
                    p.leech_action = 1;
                })?;
                let note = ctx.scope().note(row.note_id)?.unwrap();
                let mut nt = ctx.scope().notetype(note.notetype_id)?.unwrap();
                nt.templates[0].config.q_format = "{{Front}}|tags={{Tags}}".into();
                ctx.scope().put_notetype(&nt)?;
                Ok(())
            },
        )
        .unwrap();
    let id = match h.review.start(DeckId(1), Some(card)).unwrap() {
        meguri_engine::session::StartResult::Started { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    let original = h.review.head(id, 3).unwrap();
    let item = &original.items[0];
    assert_eq!(item.card_id, card);
    let original_page = h
        .review
        .render_head(id, &[item.render_key], meguri_engine::rendering::Platform::Mac)
        .unwrap()
        .remove(0);
    assert!(!original_page.question.as_deref().unwrap().contains("leech"));
    assert!(matches!(
        h.review
            .answer(id, card, Rating::Again, item.state_token, EpochMillis(NOW), 100)
            .unwrap(),
        AnswerOutcome::Accepted { leech: true, .. }
    ));
    h.settle();
    let note_id = item.note_id;
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(note_id)
            .unwrap()
            .unwrap()
            .tags
            .contains("leech")
    );
    h.review.clear_render_cache_for_testing();
    let (release, writing) = hold_writer(&h);
    let restored = h.review.undo(UndoScope::Session(id)).unwrap();
    let rendered = h.review.render_head(
        id,
        &[restored.items[0].render_key],
        meguri_engine::rendering::Platform::Mac,
    );
    let physical_tags = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(note_id)
        .unwrap()
        .unwrap()
        .tags;
    release.send(()).unwrap();
    writing.wait().unwrap();
    h.settle();
    assert!(physical_tags.contains("leech"), "the render preceded physical U25 undo");
    assert_eq!(restored.items[0].render_key, item.render_key);
    let rendered = rendered.unwrap().remove(0);
    assert_eq!(rendered.question, original_page.question);
    assert!(!rendered.question.as_deref().unwrap().contains("leech"));
    assert_eq!(
        h.review.head(id, 3).unwrap().items[0].render_key,
        restored.items[0].render_key
    );
    assert!(h.logs().is_empty());
}

#[test]
fn c15_pending_redo_keeps_effective_instant_after_clock_moves_back() {
    let h = Harness::new(2);
    let (id, initial) = h.start();
    let item = &initial.items[0];
    let (release, writing) = hold_writer(&h);
    accepted(&h, id, initial.clone());
    let restored = h.review.undo(UndoScope::Session(id)).unwrap();
    h.clock.set_now_ms(NOW - 1000);
    // Only an internal redo may replay the retained effective instant.
    let future = h.review.answer(
        id,
        item.card_id,
        Rating::Easy,
        restored.items[0].state_token,
        EpochMillis(NOW),
        200,
    );
    let redo = h.review.redo(UndoScope::Session(id));
    release.send(()).unwrap();
    writing.wait().unwrap();
    assert!(matches!(
        future,
        Err(meguri_engine::EngineError::InvalidArgument { .. })
    ));
    redo.unwrap();
    h.settle();
    assert_eq!(h.logs(), [NOW]);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let log = reader.revlog_entry(meguri_model::ids::RevlogId(NOW)).unwrap().unwrap();
    assert_eq!(log.card_id, item.card_id);
    assert_eq!(log.button, Rating::Easy as u8);
    assert_eq!(log.answer_millis, 200);
    assert_eq!(reader.card(item.card_id).unwrap().unwrap().reps, 1);
}
