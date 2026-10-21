//! U43 C12 durability packet. Authored in restricted fixer session; root verifies.
//! Start: ca7889f58d372b7063ac7f398720e2ca9067a804 (filtered checkout).
//! Session: s-20261021T075119Z-31c69d87; clean input: 4ede736c3753d603a5201673eb519f7fc0e6e72e.
mod session_support;

use meguri_engine::{
    Collection, Engine, EngineConfig, EventKind, OpSource, OpSpec, OpenConfig, SessionId, UndoScope,
    executor::catalog,
    monotonic,
    session::{AnswerOutcome, Head, JournalStage, ReviewService, StartResult},
};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    ids::{CardId, DeckId, RevlogId},
    rng::SeededRandom,
    time::EpochMillis,
    usn::Usn,
};
use meguri_sched::answer::Rating;
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, StampMode, Table};
use session_support::{Harness, NOW};
use std::{
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

fn accept(review: &ReviewService, session: SessionId, head: &Head, rating: Rating, at: i64, elapsed: u32) -> Head {
    let item = &head.items[0];
    match review
        .answer(
            session,
            item.card_id,
            rating,
            item.state_token,
            EpochMillis(at),
            u64::from(elapsed),
        )
        .unwrap()
    {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("answer refused: {other:?}"),
    }
}

fn journal(path: &Path) -> std::path::PathBuf {
    path.parent().unwrap().join("meguri-pending-answers.jsonl")
}

fn records(bytes: &[u8]) -> Vec<serde_json::Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| {
            let frame: serde_json::Value = serde_json::from_str(line).unwrap();
            serde_json::from_str(frame["payload"].as_str().unwrap()).unwrap()
        })
        .collect()
}

fn prepared(bytes: &[u8]) -> Vec<(u64, [i64; 9], [u8; 32], [u8; 32])> {
    records(bytes)
        .into_iter()
        .filter_map(|record| {
            let p = record.get("Prepared")?;
            Some((
                p["event"].as_u64().unwrap(),
                serde_json::from_value(p["receipt"]["row"].clone()).unwrap(),
                serde_json::from_value(p["receipt"]["base"].clone()).unwrap(),
                serde_json::from_value(p["receipt"]["post"].clone()).unwrap(),
            ))
        })
        .collect()
}

fn log_rows(c: &Collection) -> Vec<(i64, Vec<Cell>)> {
    let r = c.reader(ReaderRole::Query).unwrap();
    r.ids_after(Table::Revlog, None, 100_000)
        .unwrap()
        .into_iter()
        .map(|id| (id, r.row_image(Table::Revlog, &RowKey::Id(id)).unwrap().unwrap()))
        .collect()
}

fn card_row(c: &Collection, card: CardId) -> Vec<Cell> {
    c.reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(card.0))
        .unwrap()
        .unwrap()
}

fn assert_receipt_row(c: &Collection, row: &[i64; 9]) {
    let actual = c
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Revlog, &RowKey::Id(row[0]))
        .unwrap()
        .unwrap();
    assert_eq!(actual, row.iter().copied().map(Cell::Integer).collect::<Vec<_>>());
}

fn hold_writer(c: &Collection) -> (mpsc::Sender<()>, meguri_engine::Pending<meguri_engine::OpResult<()>>) {
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::channel();
    let pending = c.submit(
        OpSpec::new(catalog::BULK_OP, OpSource::UserUndoable).declare_long_write(Duration::from_secs(3)),
        move |_| {
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        },
    );
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    (release, pending)
}

#[test]
fn c12_ordinary_queue_burst_covers_every_acceptance_within_one_second() {
    let fixture = Harness::new(12);
    let (active, mut screen) = fixture.start();
    fixture.settle();
    let notifications = fixture.engine.stream().subscribe();
    assert!(fixture.collection.write_delay().is_none());
    let (durable_send, durable_receive) = mpsc::sync_channel(8);
    fixture
        .review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == JournalStage::IntentSynced {
                durable_send.send(monotonic::now()).unwrap();
            }
        }))
        .unwrap();

    let (arrival_send, arrival_receive) = mpsc::sync_channel(1);
    let (leave_send, leave_receive) = mpsc::sync_channel(1);
    let front_operation = fixture.collection.submit(
        OpSpec::named("short queue entry gate", OpSource::Maintenance),
        move |_| {
            let opened = monotonic::now();
            arrival_send.send(()).unwrap();
            leave_receive.recv_timeout(Duration::from_millis(150)).unwrap();
            assert!(monotonic::now() - opened < Duration::from_millis(200));
            Ok(())
        },
    );
    arrival_receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let (span_send, span_receive) = mpsc::sync_channel(20);
    let mut queued = Vec::with_capacity(20);
    for position in 0..20 {
        let output = span_send.clone();
        queued.push(fixture.collection.submit(
            OpSpec::named("short queued maintenance", OpSource::Maintenance),
            move |_| {
                let begin = monotonic::now();
                std::thread::sleep(Duration::from_millis(80));
                let finish = monotonic::now();
                output.send((position, begin, finish)).unwrap();
                assert!(finish - begin < Duration::from_millis(200));
                Ok(())
            },
        ));
    }
    let mut accepted_cards = Vec::new();
    let mut acceptance_spans = Vec::new();
    for answer_index in 0..8 {
        let displayed = screen.items[0].card_id;
        assert!(!accepted_cards.contains(&displayed));
        assert!(fixture.collection.write_delay().is_none());
        let before_call = monotonic::now();
        screen = accept(&fixture.review, active, &screen, Rating::Easy, NOW, 100 + answer_index);
        acceptance_spans.push((before_call, monotonic::now()));
        accepted_cards.push(displayed);
        assert_ne!(screen.items[0].card_id, displayed);
        assert!(fixture.collection.write_delay().is_none());
    }
    leave_send.send(()).unwrap();
    for (before_call, _) in &acceptance_spans {
        let file_sync = durable_receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(file_sync >= *before_call);
        assert!(file_sync - *before_call <= Duration::from_secs(1));
        assert!(fixture.collection.write_delay().is_none());
    }
    front_operation.wait().unwrap();
    for pending in queued {
        pending.wait().unwrap();
    }
    let spans: Vec<_> = (0..20)
        .map(|position| {
            let span = span_receive.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(span.0, position);
            span
        })
        .collect();
    let last_finish = spans[19].2;
    assert!(last_finish - spans[0].1 > Duration::from_secs(1));
    assert!(last_finish - acceptance_spans[7].1 > Duration::from_secs(1));
    fixture.settle();
    assert!(fixture.collection.write_delay().is_none());
    while let Some(envelope) = notifications.try_recv() {
        assert!(!matches!(envelope.kind, EventKind::WriteDelay { .. }));
    }

    let persisted = log_rows(&fixture.collection);
    assert_eq!(persisted.len(), 8);
    let snapshot = fixture.collection.reader(ReaderRole::Query).unwrap();
    for (answer_index, card_id) in accepted_cards.iter().enumerate() {
        assert_eq!(
            persisted
                .iter()
                .filter(|(_, cells)| cells[1] == Cell::Integer(card_id.0))
                .count(),
            1
        );
        let expected_id = RevlogId(NOW + answer_index as i64);
        let review_row = snapshot.revlog_entry(expected_id).unwrap().unwrap();
        assert_eq!(review_row.id, expected_id);
        assert_eq!(review_row.card_id, *card_id);
        assert_eq!(review_row.button, Rating::Easy as u8);
        assert_eq!(review_row.answer_millis, 100 + answer_index as u32);
        let stored_card = snapshot.card(*card_id).unwrap().unwrap();
        assert_eq!(stored_card.reps, 1);
        assert_eq!(stored_card.data.last_review_secs, Some(NOW / 1000));
    }
}

#[cfg(unix)]
#[test]
fn c12_sigkill_same_card_committed_prefix_and_second_prepared_reopens_twice() {
    use std::{
        io::{BufRead, Write},
        os::unix::{fs::MetadataExt, process::ExitStatusExt},
        process::{Command, Stdio},
    };
    const CHILD: &str = "MEGURI_U43_PREFIX_CHILD";
    const NAME: &str = "c12_sigkill_same_card_committed_prefix_and_second_prepared_reopens_twice";
    if let Some(path) = std::env::var_os(CHILD) {
        let path = std::path::PathBuf::from(path);
        let (_engine, c, review) = session_support::open(&path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        let (id, mut head) = match review.start(DeckId(1), None).unwrap() {
            StartResult::Started { session_id, head, .. } => (session_id, head),
            other => panic!("child admission: {other:?}"),
        };
        let captures = path.parent().unwrap().to_owned();
        let file = journal(&path);
        let count = Mutex::new(0);
        review
            .journal_hook_for_testing(Arc::new(move |stage| {
                if stage == JournalStage::PreparedSynced {
                    let mut count = count.lock().unwrap();
                    *count += 1;
                    fs::write(
                        captures.join(format!("prefix-prepared-{}.jsonl", *count)),
                        fs::read(&file).unwrap(),
                    )
                    .unwrap();
                    if *count == 2 {
                        println!("PREFIX-SECOND-PREPARED");
                        std::io::stdout().flush().unwrap();
                        loop {
                            std::thread::park();
                        }
                    }
                }
            }))
            .unwrap();
        let (release, held) = hold_writer(&c);
        let card = head.items[0].card_id;
        for (at, elapsed) in [(NOW - 10_000, 99), (NOW - 1000, 1234)] {
            assert_eq!(head.items[0].card_id, card);
            head = accept(&review, id, &head, Rating::Again, at, elapsed);
        }
        release.send(()).unwrap();
        held.wait().unwrap();
        review.flush_for_background().unwrap();
        panic!("second prepare boundary never reached");
    }

    let h = Harness::new(1);
    let card = h.cards[0];
    let path = h.collection.path().to_owned();
    h.engine.close().unwrap();
    let identity = |path: &Path| {
        let m = fs::metadata(path).unwrap();
        (m.dev(), m.ino())
    };
    let original_identity = identity(&path);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture"])
        .env(CHILD, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (signal, ready) = mpsc::sync_channel(1);
    let reader = h
        .engine
        .threads()
        .spawn("test-prefix-checkpoint", meguri_engine::Qos::Default, move |_| {
            let reached = std::io::BufReader::new(stdout)
                .lines()
                .any(|line| line.unwrap().contains("PREFIX-SECOND-PREPARED"));
            let _ = signal.send(reached);
        })
        .unwrap();
    let reached = ready.recv_timeout(Duration::from_secs(15));
    let killed = child.kill();
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(reached.unwrap(), "child exited before second PreparedSynced");
    killed.unwrap();
    assert_eq!(
        status.signal(),
        Some(9),
        "must kill the actual parked child with SIGKILL"
    );
    assert_eq!(identity(&path), original_identity);

    let first_bytes = fs::read(h.dir.path().join("prefix-prepared-1.jsonl")).unwrap();
    let second_bytes = fs::read(h.dir.path().join("prefix-prepared-2.jsonl")).unwrap();
    let first = prepared(&first_bytes)[0];
    let second_receipts = prepared(&second_bytes);
    assert_eq!(second_receipts.len(), 2);
    assert_eq!(second_receipts[0], first);
    let second = second_receipts[1];
    assert_eq!(
        second.2, first.3,
        "child's resolved base is the committed predecessor postimage"
    );
    let second_intent = records(&second_bytes)
        .into_iter()
        .find_map(|r| {
            r.get("Intent")
                .filter(|i| i["event"].as_u64() == Some(second.0))
                .cloned()
        })
        .unwrap();
    assert_eq!(second_intent["answer"]["base"]["Predecessor"].as_u64(), Some(first.0));
    assert_eq!([first.1[0], second.1[0]], [NOW - 10_000, NOW - 1000]);

    // All child owners are dead and the parent owner is closed. Inspect the
    // surviving SQLite prefix through one plain engine, without Review replay.
    let plain = Engine::new(EngineConfig::default()).unwrap();
    let c = plain
        .open_raw(
            &path,
            OpenConfig::new(
                Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")),
                Arc::new(SeededRandom::new(43)),
            ),
        )
        .unwrap();
    assert_eq!(log_rows(&c).len(), 1);
    assert_receipt_row(&c, &first.1);
    {
        let r = c.reader(ReaderRole::Query).unwrap();
        assert!(r.revlog_entry(RevlogId(second.1[0])).unwrap().is_none());
        assert_eq!(r.row_digest(Table::Cards, &RowKey::Id(card.0)).unwrap(), Some(first.3));
        let stored = r.card(card).unwrap().unwrap();
        assert_eq!(stored.reps, 1);
        assert_eq!(stored.data.last_review_secs, Some((NOW - 10_000) / 1000));
    }
    let surviving_row = log_rows(&c)[0].clone();
    plain.close().unwrap();
    for _ in 0..2 {
        let (engine, c, _) = session_support::open(&path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(
            log_rows(&c).iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [first.1[0], second.1[0]]
        );
        assert_eq!(
            log_rows(&c)[0],
            surviving_row,
            "committed full row must never be overwritten"
        );
        assert_receipt_row(&c, &first.1);
        assert_receipt_row(&c, &second.1);
        {
            let r = c.reader(ReaderRole::Query).unwrap();
            assert_eq!(r.row_digest(Table::Cards, &RowKey::Id(card.0)).unwrap(), Some(second.3));
            let stored = r.card(card).unwrap().unwrap();
            assert_eq!(stored.reps, 2);
            assert_eq!(stored.data.last_review_secs, Some((NOW - 1000) / 1000));
            for (receipt, elapsed) in [(first, 99), (second, 1234)] {
                let row = r.revlog_entry(RevlogId(receipt.1[0])).unwrap().unwrap();
                assert_eq!(row.card_id, card);
                assert_eq!(row.button, Rating::Again as u8);
                assert_eq!(row.answer_millis, elapsed);
                assert_eq!(row.kind, meguri_model::revlog::RevlogKind::Learning);
            }
        }
        engine.close().unwrap();
    }
}

#[test]
fn c12_failed_real_wal_sync_retains_prepared_until_retry_and_done() {
    let rig = Harness::new(2);
    let (session_key, visible) = rig.start();
    rig.settle();
    rig.collection.record_durability_events();
    let (notify_commit, observed_commit) = mpsc::sync_channel(1);
    let (notify_file, observed_file) = mpsc::channel();
    let metadata = rig.collection.clone();
    let evidence_path = journal(rig.collection.path());
    rig.review
        .journal_hook_for_testing(Arc::new(move |point| {
            if point == JournalStage::PreparedSynced {
                let notify_commit = notify_commit.clone();
                metadata.after_next_commit(move |commit_key| notify_commit.send(commit_key).unwrap());
            }
            if matches!(point, JournalStage::TerminalWrite | JournalStage::TerminalSynced) {
                notify_file
                    .send((
                        point,
                        monotonic::now(),
                        metadata.durability_events(),
                        fs::read(&evidence_path).unwrap(),
                    ))
                    .unwrap();
            }
        }))
        .unwrap();
    rig.collection
        .submit_maintenance(catalog::WAL_CHECKPOINT, Duration::ZERO, |database| {
            database.fail_next_wal_sync_for_test();
            Ok(())
        })
        .wait()
        .unwrap();
    accept(&rig.review, session_key, &visible, Rating::Easy, NOW, 200);
    rig.review.application_barrier_for_testing().unwrap();
    let applied_key = observed_commit.recv_timeout(Duration::from_secs(5)).unwrap();
    let (write_point, write_time, attempts_at_write, unfinished_bytes) =
        observed_file.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(write_point, JournalStage::TerminalWrite);
    let unsuccessful = attempts_at_write
        .syncs
        .iter()
        .find(|attempt| !attempt.succeeded && attempt.through == applied_key)
        .unwrap()
        .clone();
    assert!(unsuccessful.completed <= write_time);
    assert!(
        !attempts_at_write
            .syncs
            .iter()
            .any(|attempt| attempt.succeeded && attempt.through >= applied_key)
    );
    let witnesses = prepared(&unfinished_bytes);
    assert_eq!(witnesses.len(), 1);
    assert!(
        !records(&unfinished_bytes)
            .iter()
            .any(|frame| frame.get("Terminal").is_some())
    );
    let (sync_point, sync_time, _, finished_bytes) = observed_file.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(sync_point, JournalStage::TerminalSynced);
    assert!(
        records(&finished_bytes)
            .iter()
            .any(|frame| frame["Terminal"]["event"].as_u64() == Some(witnesses[0].0)
                && frame["Terminal"]["status"] == "Done")
    );
    let stop_waiting = monotonic::now() + Duration::from_secs(5);
    let successful = loop {
        if let Some(attempt) = rig
            .collection
            .durability_events()
            .syncs
            .into_iter()
            .find(|attempt| attempt.succeeded && attempt.through == applied_key)
        {
            break attempt;
        }
        assert!(monotonic::now() < stop_waiting, "retry coverage was never recorded");
        std::thread::sleep(Duration::from_millis(5));
    };
    // Completion includes the synchronous journal callback; no public hook
    // timestamps the database's physical sync independently.
    assert!(unsuccessful.completed <= successful.started);
    assert!(successful.started <= write_time);
    assert!(write_time <= sync_time);
    assert!(sync_time <= successful.completed);
    rig.settle();
    assert_receipt_row(&rig.collection, &witnesses[0].1);
    let expected_entries = log_rows(&rig.collection);
    let subject = visible.items[0].card_id;
    let expected_card = card_row(&rig.collection, subject);
    let database_path = rig.collection.path().to_owned();
    rig.engine.close().unwrap();
    for _ in 0..2 {
        let (owner, restored, _) =
            session_support::open(&database_path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(log_rows(&restored), expected_entries);
        assert_eq!(card_row(&restored, subject), expected_card);
        owner.close().unwrap();
    }
}

#[test]
fn c12_owned_revlog_usn_update_and_delete_wait_for_durable_done() {
    for delete in [false, true] {
        revlog_mutation(delete, false);
    }
}

#[test]
fn c12_terminal_sync_failure_refuses_owned_revlog_usn_update_and_delete() {
    for delete in [false, true] {
        revlog_mutation(delete, true);
    }
}

fn revlog_mutation(delete: bool, fail_terminal: bool) {
    let h = Harness::new(2);
    let (id, head) = h.start();
    let card = head.items[0].card_id;
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let done = Arc::new(AtomicBool::new(false));
    let durable = done.clone();
    if fail_terminal {
        h.review.fail_journal_for_testing(JournalStage::TerminalSync).unwrap();
    }
    h.review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == JournalStage::TerminalWrite {
                entered.send(()).unwrap();
                gate.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            } else if stage == JournalStage::TerminalSynced {
                durable.store(true, Ordering::Release);
            }
        }))
        .unwrap();
    accept(&h.review, id, &head, Rating::Easy, NOW, 200);
    h.review.application_barrier_for_testing().unwrap();
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    let before_logs = log_rows(&h.collection);
    assert_eq!(before_logs.len(), 1);
    let assigned = before_logs[0].0;
    let before_card = card_row(&h.collection, card);
    let ran = Arc::new(AtomicBool::new(false));
    let body_ran = ran.clone();
    let body_done = done.clone();
    let pending = h.collection.submit(
        OpSpec::named("owned unresolved revlog mutation", OpSource::SyncIncoming).stamp(StampMode::Raw),
        move |ctx| {
            body_ran.store(true, Ordering::Release);
            assert!(
                body_done.load(Ordering::Acquire),
                "mutation entered before durable Done"
            );
            if delete {
                ctx.scope().remove_revlog(RevlogId(assigned))?;
            } else {
                let mut row = ctx.scope().revlog_entry(RevlogId(assigned))?.unwrap();
                row.usn = Usn(71);
                ctx.scope().put(&row)?;
            }
            Ok(())
        },
    );
    assert!(!ran.load(Ordering::Acquire));
    assert_eq!(log_rows(&h.collection), before_logs);
    release.send(()).unwrap();
    let result = pending.wait();
    if fail_terminal {
        let error = result.expect_err("terminal failure must refuse the mutation");
        assert!(
            format!("{error:?}").contains("TerminalSync"),
            "explicit durability error: {error:?}"
        );
        assert!(
            !ran.load(Ordering::Acquire),
            "mutation body ran after failed terminal sync"
        );
        assert!(!done.load(Ordering::Acquire));
        assert_eq!(log_rows(&h.collection), before_logs);
        assert_eq!(card_row(&h.collection, card), before_card);
        assert!(h.review.flush_for_background().is_err());
        let evidence = fs::read(journal(h.collection.path())).unwrap();
        assert_eq!(
            prepared(&evidence).len(),
            1,
            "failed completion must retain the witness"
        );
        assert!(records(&evidence).iter().any(|r| r.get("Intent").is_some()));
        return;
    }
    result.unwrap();
    assert!(ran.load(Ordering::Acquire) && done.load(Ordering::Acquire));
    h.settle();
    let after = log_rows(&h.collection);
    if delete {
        assert!(after.is_empty());
    } else {
        let mut expected = before_logs.clone();
        expected[0].1[2] = Cell::Integer(71);
        assert_eq!(after, expected);
    }
    assert_eq!(card_row(&h.collection, card), before_card);
    let path = h.collection.path().to_owned();
    h.engine.close().unwrap();
    for _ in 0..2 {
        let (engine, c, _) = session_support::open(&path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(log_rows(&c), after, "old Accepted replayed over its owned mutation");
        assert_eq!(card_row(&c, card), before_card);
        engine.close().unwrap();
    }
}

#[test]
fn c12_late_prepared_ack_after_pending_undo_cannot_commit_and_cancel_precedes_conflict() {
    let h = Harness::new(2);
    let (id, initial) = h.start();
    let card = initial.items[0].card_id;
    let before_card = card_row(&h.collection, card);
    let before_logs = log_rows(&h.collection);
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let cancelled = Arc::new(AtomicBool::new(false));
    let durable_cancelled = cancelled.clone();
    let file = journal(h.collection.path());
    h.review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == JournalStage::PreparedSynced {
                entered.send(()).unwrap();
                gate.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            } else if stage == JournalStage::TerminalSynced {
                let bytes = fs::read(&file).unwrap();
                if records(&bytes).iter().any(|r| r["Terminal"]["status"] == "Cancelled") {
                    durable_cancelled.store(true, Ordering::Release);
                }
            }
        }))
        .unwrap();
    accept(&h.review, id, &initial, Rating::Easy, NOW, 200);
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    let review = h.review.clone();
    let undo = h
        .engine
        .threads()
        .spawn("test-late-prepare-undo", meguri_engine::Qos::Default, move |_| {
            review.undo(UndoScope::Session(id))
        })
        .unwrap();
    let deadline = monotonic::now() + Duration::from_secs(5);
    loop {
        let restored = h.review.head(id, 3).unwrap();
        if restored.items[0].card_id == card && restored.items[0].state_token == initial.items[0].state_token {
            break;
        }
        assert!(
            monotonic::now() < deadline,
            "pending undo never restored the optimistic head"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        !cancelled.load(Ordering::Acquire),
        "cancel should still be behind PreparedSynced gate"
    );
    assert_eq!(card_row(&h.collection, card), before_card);
    assert_eq!(log_rows(&h.collection), before_logs);

    let expected_card = before_card.clone();
    let expected_logs = before_logs.clone();
    let conflict_cancelled = cancelled.clone();
    let conflict = h.collection.submit(
        OpSpec::named("conflicting work after pending cancellation", OpSource::Maintenance),
        move |ctx| {
            assert!(
                conflict_cancelled.load(Ordering::Acquire),
                "conflicting work preceded durable Cancelled"
            );
            assert_eq!(
                ctx.scope().row_image(Table::Cards, &RowKey::Id(card.0))?.unwrap(),
                expected_card
            );
            assert_eq!(
                ctx.scope().ids_after(Table::Revlog, None, 100_000)?.len(),
                expected_logs.len()
            );
            let mut row = ctx.scope().card(card)?.unwrap();
            row.flags ^= 1;
            ctx.scope().put(&row)?;
            Ok(())
        },
    );
    // This exercises an ACK delivered to the still-waiting before_commit
    // receiver after cancellation was registered. It does not claim the
    // receiver was dropped or a rollback had already completed at this point.
    release.send(()).unwrap();
    assert_eq!(undo.join().unwrap().unwrap().items[0].card_id, card);
    conflict.wait().unwrap();
    h.settle();
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(log_rows(&h.collection), before_logs);
    let after_card = card_row(&h.collection, card);
    let path = h.collection.path().to_owned();
    h.engine.close().unwrap();
    for _ in 0..2 {
        let (engine, c, _) = session_support::open(&path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(log_rows(&c), before_logs);
        assert_eq!(
            card_row(&c, card),
            after_card,
            "cancelled answer replayed over later work"
        );
        engine.close().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn c12_sigkill_one_second_after_actual_acceptance_preserves_exact_transition() {
    elapsed_crash(
        "c12_sigkill_one_second_after_actual_acceptance_preserves_exact_transition",
        false,
    );
}

#[cfg(unix)]
#[test]
fn c12_sigkill_during_three_second_write_recovers_deadlined_intent_once() {
    elapsed_crash(
        "c12_sigkill_during_three_second_write_recovers_deadlined_intent_once",
        true,
    );
}

#[cfg(unix)]
fn elapsed_crash(test_name: &str, occupied: bool) {
    use std::{
        io::{BufRead, Write},
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
    };
    const DATABASE_ENV: &str = "MEGURI_U43_ELAPSED_CHILD";
    const CHOICE: &str = "elapsed-expected.json";
    const SYNCED: &str = "elapsed-synced.jsonl";
    const ASSIGNED: &str = "elapsed-assigned.jsonl";
    const WRITER_DONE: &str = "elapsed-operation-finished";
    const MESSAGE_LIMIT: Duration = Duration::from_secs(15);
    let announce = |message: String| {
        println!("U43ELAPSED {message}");
        std::io::stdout().flush().unwrap();
    };

    if let Some(database) = std::env::var_os(DATABASE_ENV) {
        let database = std::path::PathBuf::from(database);
        let directory = database.parent().unwrap().to_path_buf();
        let (_owner, collection, service) =
            session_support::open(&database, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        let (session, visible) = match service.start(DeckId(1), None).unwrap() {
            StartResult::Started { session_id, head, .. } => (session_id, head),
            refusal => panic!("elapsed child start: {refusal:?}"),
        };
        let item = &visible.items[0];
        let day = collection.today().unwrap();
        let expected = {
            let rows = collection.reader(ReaderRole::Query).unwrap();
            let card = rows.card(item.card_id).unwrap().unwrap();
            let deck = rows.deck(DeckId(1)).unwrap().unwrap();
            let preset =
                meguri_sched::model::preset::PresetView::for_config(&*rows, meguri_model::ids::DeckConfigId(1))
                    .unwrap();
            let meguri_model::proto::deck::DeckKind::Normal(normal) = &deck.kind.kind else {
                panic!("elapsed fixture deck is not normal");
            };
            let retention = meguri_sched::model::deck_sched::DeckSched::new(Some(normal), &deck.common)
                .retention(preset.fields().desired_retention);
            let planned = meguri_sched::answer::plan(
                &card,
                item.next_states.current,
                &item.next_states.easy,
                &preset,
                retention,
                day,
                NOW,
                321,
            )
            .unwrap();
            let result = &planned.card;
            let columns = [
                result.id.0,
                result.note_id.0,
                result.deck_id.0,
                i64::from(result.ord),
                result.mtime.0,
                i64::from(result.usn.0),
                result.card_type.stored(),
                result.queue.stored(),
                i64::from(result.due),
                i64::from(result.interval),
                i64::from(result.ease_factor),
                i64::from(result.reps),
                i64::from(result.lapses),
                i64::from(result.remaining_steps),
                i64::from(result.original_due),
                result.original_deck_id.0,
                i64::from(result.flags),
            ];
            let preview = item.next_states.easy.revlog;
            let log = [
                NOW,
                item.card_id.0,
                -1,
                Rating::Easy as i64,
                i64::from(preview.interval),
                i64::from(preview.last_interval),
                i64::from(preview.factor),
                i64::from(planned.elapsed_ms),
                i64::from(preview.kind),
            ];
            serde_json::json!({"columns": columns, "data": result.data.to_text(), "log": log})
        };
        fs::write(directory.join(CHOICE), serde_json::to_vec(&expected).unwrap()).unwrap();

        let file = journal(&database);
        let destination = directory.clone();
        service
            .journal_hook_for_testing(Arc::new(move |point| {
                if point == JournalStage::IntentSynced {
                    let at = monotonic::now();
                    fs::write(destination.join(SYNCED), fs::read(&file).unwrap()).unwrap();
                    announce(format!("SYNC {}", at.as_nanos()));
                } else if point == JournalStage::PreparedSynced {
                    fs::write(destination.join(ASSIGNED), fs::read(&file).unwrap()).unwrap();
                }
            }))
            .unwrap();

        let input = std::io::stdin();
        let mut input = input.lock();
        let mut command = String::new();
        announce("READY".into());
        input.read_line(&mut command).unwrap();
        assert_eq!(command.trim(), "CLOCK");
        let sample = monotonic::now();
        announce(format!("CLOCK {}", sample.as_nanos()));
        command.clear();
        input.read_line(&mut command).unwrap();
        assert_eq!(command.trim(), "GO");

        let _writer = if occupied {
            let (origin_tx, origin_rx) = mpsc::sync_channel(1);
            let finished = directory.join(WRITER_DONE);
            let operation = collection.submit(
                OpSpec::new(catalog::BULK_OP, OpSource::Maintenance).declare_long_write(Duration::from_secs(3)),
                move |_| {
                    let origin = monotonic::now();
                    announce(format!("ENTERED {}", origin.as_nanos()));
                    origin_tx.send(origin).unwrap();
                    std::thread::sleep(Duration::from_secs(3));
                    fs::write(finished, b"finished").unwrap();
                    Ok(())
                },
            );
            let origin = origin_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            std::thread::sleep((origin + Duration::from_millis(200)).saturating_sub(monotonic::now()));
            Some(operation)
        } else {
            None
        };
        let invoked = monotonic::now();
        accept(&service, session, &visible, Rating::Easy, NOW, 321);
        let returned = monotonic::now();
        announce(format!("RETURN {} {}", invoked.as_nanos(), returned.as_nanos()));
        loop {
            std::thread::park();
        }
    }

    let h = Harness::new(3);
    let subject = h.cards[0];
    h.collection
        .run(
            OpSpec::named("elapsed recovery preservation", OpSource::Maintenance).stamp(StampMode::Raw),
            move |ctx| {
                let mut card = ctx.scope().card(subject)?.unwrap();
                card.flags = 0xa5;
                let data = serde_json::json!({"elapsedOpaque": [subject.0, NOW], "cd": "retain"});
                card.data = meguri_model::card_data::CardData::parse(&data.to_string());
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    let unchanged_tables = |collection: &Collection| {
        let rows = collection.reader(ReaderRole::Query).unwrap();
        [Table::Notes, Table::Notetypes, Table::DeckConfig].map(|table| {
            rows.ids_after(table, None, 1000)
                .unwrap()
                .into_iter()
                .map(|id| (id, rows.row_image(table, &RowKey::Id(id)).unwrap().unwrap()))
                .collect::<Vec<_>>()
        })
    };
    let originals = unchanged_tables(&h.collection);
    let untouched: Vec<_> = h.cards[1..]
        .iter()
        .map(|card| (*card, card_row(&h.collection, *card)))
        .collect();
    assert_eq!(untouched.len(), 2);
    let database = h.collection.path().to_path_buf();
    h.engine.close().unwrap();

    let mut process = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(DATABASE_ENV, &database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let output = process.stdout.take().unwrap();
    let mut control = process.stdin.take().unwrap();
    let (notice_tx, notice_rx) = mpsc::sync_channel(8);
    let registry = meguri_engine::ThreadRegistry::default();
    let output_thread = match registry.spawn("elapsed-child-reader", meguri_engine::Qos::Default, move |_| {
        for line in std::io::BufReader::new(output).lines() {
            let line = line.unwrap();
            if let Some((_, notice)) = line.split_once("U43ELAPSED ") {
                if notice_tx.send(notice.to_owned()).is_err() {
                    break;
                }
            }
        }
    }) {
        Ok(thread) => thread,
        Err(error) => {
            let _ = process.kill();
            let _ = process.wait();
            panic!("elapsed reader could not start: {error:?}");
        }
    };

    let timing = (|| -> Result<(Duration, Duration, Duration, Option<Duration>), String> {
        let next = || notice_rx.recv_timeout(MESSAGE_LIMIT).map_err(|error| error.to_string());
        let duration = |value: &str| {
            value
                .parse::<u64>()
                .map(Duration::from_nanos)
                .map_err(|error| error.to_string())
        };
        if next()? != "READY" {
            return Err("child did not become ready".into());
        }
        let sent = monotonic::now();
        writeln!(control, "CLOCK").map_err(|error| error.to_string())?;
        control.flush().map_err(|error| error.to_string())?;
        let reply = next()?;
        let received = monotonic::now();
        let sample = duration(reply.strip_prefix("CLOCK ").ok_or("invalid clock reply")?)?;
        if received - sent > Duration::from_millis(20) {
            return Err("child clock interval exceeds 20 ms".into());
        }
        writeln!(control, "GO").map_err(|error| error.to_string())?;
        control.flush().map_err(|error| error.to_string())?;

        let mut writer_origin = None;
        let mut synced_at = None;
        let (invoked, returned) = loop {
            let notice = next()?;
            if let Some(value) = notice.strip_prefix("ENTERED ") {
                if !occupied || writer_origin.is_some() {
                    return Err("unexpected writer entry".into());
                }
                writer_origin = Some(duration(value)?);
            } else if let Some(value) = notice.strip_prefix("SYNC ") {
                synced_at = Some(duration(value)?);
            } else if let Some(value) = notice.strip_prefix("RETURN ") {
                let pair = value.split_whitespace().map(duration).collect::<Result<Vec<_>, _>>()?;
                if pair.len() != 2 || pair[0] > pair[1] {
                    return Err("invalid acceptance interval".into());
                }
                break (pair[0], pair[1]);
            } else {
                return Err(format!("unexpected acceptance notice: {notice}"));
            }
        };
        let target = if occupied {
            let origin = writer_origin.ok_or("held writer never entered")?;
            let lower = invoked.checked_sub(origin).ok_or("invocation predates writer entry")?;
            let upper = returned.checked_sub(origin).ok_or("return predates writer entry")?;
            if lower < Duration::from_millis(200) || upper > Duration::from_millis(250) {
                return Err("held acceptance lies outside 200..250 ms".into());
            }
            origin + Duration::from_millis(1500)
        } else {
            returned + Duration::from_secs(1)
        };
        let distance = target.checked_sub(sample).ok_or("target predates clock sample")?;
        let earliest = sent + distance;
        let latest = received + distance;
        while monotonic::now() < latest {
            let remaining = latest.saturating_sub(monotonic::now());
            match notice_rx.recv_timeout(remaining) {
                Ok(notice) => {
                    synced_at = Some(duration(
                        notice.strip_prefix("SYNC ").ok_or("unexpected timed notice")?,
                    )?);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok((earliest, latest, invoked, synced_at))
    })();
    // Collect every cleanup result before interpreting any failed handshake.
    let killing = monotonic::now();
    let killed = process.kill();
    let killed_at = monotonic::now();
    let waited = process.wait();
    drop(control);
    drop(notice_rx);
    let joined = output_thread.join();
    let (earliest, latest, invoked, synced_at) = timing.unwrap();
    killed.unwrap();
    let status = waited.unwrap();
    joined.unwrap();
    assert_eq!(status.signal(), Some(9));
    assert!(killing >= latest);
    assert!(killed_at - earliest <= Duration::from_millis(50));
    let synced_at = synced_at.expect("missing real IntentSynced notification");
    assert!(synced_at >= invoked);
    assert!(synced_at - invoked <= Duration::from_secs(1));
    assert!(!h.dir.path().join(WRITER_DONE).exists());

    let saved_intent = records(&fs::read(h.dir.path().join(SYNCED)).unwrap());
    let intents: Vec<_> = saved_intent.iter().filter_map(|record| record.get("Intent")).collect();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0]["answer"]["card"], subject.0);
    assert_eq!(intents[0]["answer"]["at"], NOW);
    assert_eq!(intents[0]["answer"]["elapsed"], 321);
    let assignment_path = h.dir.path().join(ASSIGNED);
    let assignment = if occupied {
        assert!(!assignment_path.exists());
        assert!(prepared(&fs::read(journal(&database)).unwrap()).is_empty());
        None
    } else {
        let receipts = prepared(&fs::read(assignment_path).unwrap());
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].1[0], NOW);
        Some(receipts[0])
    };
    let expected: serde_json::Value = serde_json::from_slice(&fs::read(h.dir.path().join(CHOICE)).unwrap()).unwrap();
    let integers = |value: &serde_json::Value| {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|number| Cell::Integer(number.as_i64().unwrap()))
            .collect::<Vec<_>>()
    };
    let mut postimage = integers(&expected["columns"]);
    postimage.push(Cell::text(expected["data"].as_str().unwrap()));
    let revlog = integers(&expected["log"]);
    assert_eq!(postimage[16], Cell::Integer(0xa5));
    let mut previous_logs = None;
    for _opening in 0..2 {
        let (owner, collection, _) = session_support::open(
            &database,
            Arc::new(StepClock::new(NOW + 172_800_000, UtcOffset::UTC, "UTC")),
        );
        let actual = log_rows(&collection);
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0], (NOW, revlog.clone()));
        assert_eq!(card_row(&collection, subject), postimage);
        let rows = collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            rows.card(subject).unwrap().unwrap().data.last_review_secs,
            Some(NOW / 1000)
        );
        if let Some((_, receipt, _, post_digest)) = assignment {
            assert_receipt_row(&collection, &receipt);
            assert_eq!(
                rows.row_digest(Table::Cards, &RowKey::Id(subject.0)).unwrap(),
                Some(post_digest)
            );
        }
        drop(rows);
        assert_eq!(unchanged_tables(&collection), originals);
        for (card, image) in &untouched {
            assert_eq!(card_row(&collection, *card), *image);
        }
        if let Some(prior) = &previous_logs {
            assert_eq!(&actual, prior);
        }
        previous_logs = Some(actual);
        owner.close().unwrap();
    }
}
