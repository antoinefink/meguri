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
        if let meguri_engine::EventKind::Extension(e) = &event.kind {
            if let Some(meguri_engine::session::SessionEvent::CommitFailed { card_id, .. }) = e.as_any().downcast_ref()
            {
                assert_eq!(*card_id, third);
                failed = true;
            }
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
