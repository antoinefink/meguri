//! U43 C2/C16 restricted-fixer packet; root owns all execution.
//! Session s-20261021T080226Z-a86bc007; canonical input 4249939c905ae02ce28ec0cc524b932322967f6a.
mod common;
mod session_support;

use meguri_engine::{
    Collection, EngineError, EventKind, JobKind, JobOutcome, LongOp, OpSource, OpSpec, Qos, Seq, SessionId,
    SuspendOutcome, ThreadRegistry,
    executor::catalog,
    monotonic,
    sched::fsrs_policy::enable_fsrs,
    session::{AnswerOutcome, Head, JournalStage, Refusal, SessionEvent, SessionStartHook, StartResult},
};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::{StepClock, UtcOffset},
    ids::{CardId, DeckConfigId, DeckId},
    proto::{ProtoBlob, deck::DeckKind},
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{Cell, ConfigValue, ReadRows, ReaderRole, RowKey, StampMode, Table};
use session_support::{Harness, NOW};
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Starts(AtomicUsize);
impl SessionStartHook for Starts {
    fn session_starting(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn set_switch(c: &Collection, key: &'static str, value: bool) {
    c.run(
        OpSpec::named("lifecycle policy setup", OpSource::Maintenance),
        move |ctx| {
            ctx.scope().set_config(key, &ConfigValue::Bool(value))?;
            Ok(())
        },
    )
    .unwrap();
}

fn assert_refusal(h: &Harness, starts: &Starts, reason: Refusal) {
    let preimage = common::steps::dump(&h.collection);
    assert_eq!(h.review.active_session(), None);
    match h.review.start(DeckId(1), None).unwrap() {
        StartResult::Refused(actual) => assert_eq!(actual, reason),
        other => panic!("admitted a refused fixture: {other:?}"),
    }
    assert_eq!(h.review.active_session(), None);
    assert_eq!(starts.0.load(Ordering::SeqCst), 0);
    assert_eq!(common::steps::dump(&h.collection), preimage);
    // The actual shared writer guard must be free after a refusal.
    if !matches!(reason, Refusal::FsrsPolicyPending) {
        enable_fsrs(&h.collection, &Default::default()).unwrap();
        assert_eq!(common::steps::dump(&h.collection), preimage);
    }
}

#[test]
fn c2_all_represented_hidden_homes_refuse_sorted_deduplicated_without_writes_or_hooks() {
    for hidden in [
        "suspended",
        "scheduler-buried",
        "user-buried",
        "future-review",
        "filtered-home",
    ] {
        let h = Harness::new(4);
        let starts = Arc::new(Starts::default());
        h.review.add_session_start_hook(starts.clone());
        let day = h.collection.today().unwrap();
        let subjects = h.cards.clone();
        h.collection
            .run(
                OpSpec::named("represented hidden homes", OpSource::Maintenance).stamp(StampMode::Raw),
                move |ctx| {
                    ctx.scope()
                        .set_config("lastUnburied", &ConfigValue::from(i64::from(day)))?;
                    let template = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                    let root = ctx.scope().deck(DeckId(1))?.unwrap();
                    for key in [19, 7] {
                        let mut preset = template.clone();
                        preset.id = DeckConfigId(key);
                        preset.name = format!("Unsupported home {key}");
                        preset.config.set_fsrs_params_6(vec![1.0; 34]);
                        ctx.scope().put(&preset)?;
                        let mut home = meguri_engine::decks::new_deck(format!("{}\u{1f}Home {key}", root.name));
                        home.id = DeckId(key);
                        let mut kind = (*home.kind).clone();
                        let DeckKind::Normal(normal) = &mut kind.kind else {
                            unreachable!()
                        };
                        normal.config_id = key;
                        home.kind = ProtoBlob::new(kind);
                        ctx.scope().put(&home)?;
                    }
                    if hidden == "filtered-home" {
                        let mut filtered = meguri_engine::decks::new_deck(format!("{}\u{1f}Mapped", root.name));
                        filtered.id = DeckId(50);
                        filtered.kind = ProtoBlob::new(meguri_model::proto::deck::DeckKindMessage::new(
                            DeckKind::Filtered(Default::default()),
                        ));
                        ctx.scope().put(&filtered)?;
                    }
                    for (card_key, home_key) in subjects.into_iter().skip(1).zip([19, 7, 19]) {
                        let mut card = ctx.scope().card(card_key)?.unwrap();
                        card.deck_id = DeckId(home_key);
                        match hidden {
                            "suspended" => card.queue = CardQueue::Suspended,
                            "scheduler-buried" => card.queue = CardQueue::BuriedByScheduler,
                            "user-buried" => card.queue = CardQueue::BuriedByUser,
                            "future-review" => {
                                card.card_type = CardType::Review;
                                card.queue = CardQueue::Review;
                                card.due = (day + 100) as i32;
                                card.interval = 100;
                            }
                            "filtered-home" => {
                                card.deck_id = DeckId(50);
                                card.original_deck_id = DeckId(home_key);
                                card.queue = CardQueue::Suspended;
                            }
                            _ => unreachable!(),
                        }
                        ctx.scope().put(&card)?;
                    }
                    Ok(())
                },
            )
            .unwrap();
        set_switch(&h.collection, "fsrs", false);
        assert_refusal(&h, &starts, Refusal::FsrsPolicyPending);
        set_switch(&h.collection, "fsrs", true);
        // With balancing off the represented-home check still applies.
        assert_refusal(
            &h,
            &starts,
            Refusal::PresetUnsupported {
                preset_ids: vec![DeckConfigId(7), DeckConfigId(19)],
            },
        );
        h.collection
            .run(OpSpec::named("restore supported homes", OpSource::Maintenance), |ctx| {
                for key in [7, 19] {
                    let mut preset = ctx.scope().deck_config(DeckConfigId(key))?.unwrap();
                    preset.config.set_fsrs_params_6(Vec::new());
                    ctx.scope().put(&preset)?;
                }
                Ok(())
            })
            .unwrap();
        let (session_key, head) = h.start();
        assert_eq!(
            head.items[0].card_id, h.cards[0],
            "supported presented card was removed in {hidden}"
        );
        assert_eq!(starts.0.load(Ordering::SeqCst), 1);
        h.review.end(session_key).unwrap();
    }
}

#[test]
fn c2_unused_invalid_easy_days_is_global_only_when_balancing_is_on() {
    let h = Harness::new(2);
    let starts = Arc::new(Starts::default());
    h.review.add_session_start_hook(starts.clone());
    h.collection
        .run(OpSpec::named("unused balancing inputs", OpSource::Maintenance), |ctx| {
            let template = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            for key in [89, 31] {
                let mut preset = template.clone();
                preset.id = DeckConfigId(key);
                preset.name = format!("Unused {key}");
                let mut fields = (*preset.config).clone();
                fields.easy_days_percentages = vec![1.0, 1.0];
                preset.config = ProtoBlob::new(fields);
                ctx.scope().put(&preset)?;
            }
            Ok(())
        })
        .unwrap();
    set_switch(&h.collection, "loadBalancerEnabled", true);
    set_switch(&h.collection, "fsrs", false);
    assert_refusal(&h, &starts, Refusal::FsrsPolicyPending);
    set_switch(&h.collection, "fsrs", true);
    assert_refusal(
        &h,
        &starts,
        Refusal::PresetUnsupported {
            preset_ids: vec![DeckConfigId(31), DeckConfigId(89)],
        },
    );
    set_switch(&h.collection, "loadBalancerEnabled", false);
    let (session_key, _) = h.start();
    assert_eq!(starts.0.load(Ordering::SeqCst), 1);
    h.review.end(session_key).unwrap();
}

struct AdmissionGate {
    reached: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    count: Arc<Starts>,
}
impl SessionStartHook for AdmissionGate {
    fn session_starting(&self) {
        self.count.session_starting();
        self.reached.send(()).unwrap();
        self.release.lock().unwrap().recv_timeout(WAIT).unwrap();
    }
}

#[test]
fn c2_writer_serializes_enable_before_start_and_start_before_enable() {
    for start_has_writer in [true, false] {
        let fixture = Harness::new(2);
        let hook_calls = Arc::new(Starts::default());
        let callers = ThreadRegistry::default();
        let (inside_tx, inside_rx) = mpsc::sync_channel(1);
        let (unlock_tx, unlock_rx) = mpsc::sync_channel(1);
        let (attempt_tx, attempt_rx) = mpsc::sync_channel(1);
        let (start_tx, start_rx) = mpsc::sync_channel(1);
        let (enable_tx, enable_rx) = mpsc::sync_channel(1);

        if start_has_writer {
            fixture.review.add_session_start_hook(Arc::new(AdmissionGate {
                reached: inside_tx,
                release: Mutex::new(unlock_rx),
                count: hook_calls.clone(),
            }));
            let review = fixture.review.clone();
            let starting = callers
                .spawn("admission-start-owner", Qos::Default, move |_| {
                    start_tx.send(review.start(DeckId(1), None)).unwrap();
                })
                .unwrap();
            inside_rx.recv_timeout(WAIT).unwrap();
            let collection = fixture.collection.clone();
            let enabling = callers
                .spawn("admission-enable-waiter", Qos::Default, move |_| {
                    attempt_tx.send(()).unwrap();
                    enable_tx
                        .send(enable_fsrs(&collection, &Default::default()).map(|_| ()))
                        .unwrap();
                })
                .unwrap();
            attempt_rx.recv_timeout(WAIT).unwrap();
            assert!(matches!(
                enable_rx.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            unlock_tx.send(()).unwrap();
            let admitted = match start_rx.recv_timeout(WAIT).unwrap().unwrap() {
                StartResult::Started { session_id, .. } => session_id,
                result => panic!("writer-owned start failed: {result:?}"),
            };
            assert_eq!(enable_rx.recv_timeout(WAIT).unwrap(), Err(EngineError::SessionActive));
            starting.join().unwrap();
            enabling.join().unwrap();
            assert_eq!(hook_calls.0.load(Ordering::SeqCst), 1);
            fixture.review.end(admitted).unwrap();
        } else {
            fixture.review.add_session_start_hook(hook_calls.clone());
            set_switch(&fixture.collection, "fsrs", false);
            fixture.collection.check_rollover().unwrap();
            fixture.settle();
            fixture.collection.after_next_commit(move |_| {
                inside_tx.send(()).unwrap();
                unlock_rx.recv_timeout(WAIT).unwrap();
            });
            let collection = fixture.collection.clone();
            let enabling = callers
                .spawn("admission-enable-owner", Qos::Default, move |_| {
                    enable_tx
                        .send(enable_fsrs(&collection, &Default::default()).map(|_| ()))
                        .unwrap();
                })
                .unwrap();
            inside_rx.recv_timeout(WAIT).unwrap();
            let review = fixture.review.clone();
            let starting = callers
                .spawn("admission-start-waiter", Qos::Default, move |_| {
                    attempt_tx.send(()).unwrap();
                    start_tx.send(review.start(DeckId(1), None)).unwrap();
                })
                .unwrap();
            attempt_rx.recv_timeout(WAIT).unwrap();
            assert!(matches!(
                start_rx.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            unlock_tx.send(()).unwrap();
            enable_rx.recv_timeout(WAIT).unwrap().unwrap();
            let admitted = match start_rx.recv_timeout(WAIT).unwrap().unwrap() {
                StartResult::Started { session_id, .. } => session_id,
                result => panic!("start did not observe the enabled policy: {result:?}"),
            };
            enabling.join().unwrap();
            starting.join().unwrap();
            assert_eq!(hook_calls.0.load(Ordering::SeqCst), 1);
            fixture.review.end(admitted).unwrap();
        }
    }
}

#[test]
fn c2_start_cancels_running_optimizer_without_waiting_for_cleanup_and_refusal_does_not() {
    let fixture = Harness::new(2);
    let hook_calls = Arc::new(Starts::default());
    fixture.review.add_session_start_hook(hook_calls.clone());
    set_switch(&fixture.collection, "fsrs", false);
    meguri_engine::decks::select(&fixture.collection, DeckId(1)).unwrap();
    let notifications = fixture.engine.stream().subscribe();
    let commits = Arc::new(AtomicUsize::new(0));
    let job_commits = commits.clone();
    let collection = fixture.collection.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (cancel_tx, cancel_rx) = mpsc::sync_channel(1);
    let (finish_cleanup_tx, finish_cleanup_rx) = mpsc::sync_channel(1);
    let optimizer = fixture
        .engine
        .start_job(JobKind::Optimizer, move |context| {
            ready_tx.send(()).unwrap();
            let expires = monotonic::now() + WAIT;
            while !context.cancel.is_cancelled() {
                assert!(monotonic::now() < expires, "optimizer did not receive cancellation");
                std::thread::sleep(Duration::from_millis(1));
            }
            cancel_tx.send(()).unwrap();
            finish_cleanup_rx.recv_timeout(WAIT).unwrap();
            context.cancel.check()?;
            // This real write would expose any continuation past the cancellation check.
            collection.run(
                OpSpec::named("test optimizer result application", OpSource::Maintenance),
                move |operation| {
                    operation
                        .scope()
                        .set_config("u43TrainingApplied", &ConfigValue::Bool(true))?;
                    job_commits.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )?;
            Ok(())
        })
        .unwrap();
    ready_rx.recv_timeout(WAIT).unwrap();
    assert_refusal(&fixture, &hook_calls, Refusal::FsrsPolicyPending);
    assert!(!optimizer.cancel.is_cancelled());
    assert!(
        fixture
            .engine
            .jobs()
            .running()
            .iter()
            .any(|running| running.id == optimizer.id)
    );
    assert_eq!(commits.load(Ordering::SeqCst), 0);

    set_switch(&fixture.collection, "fsrs", true);
    let unchanged = common::steps::dump(&fixture.collection);
    let review = fixture.review.clone();
    let callers = ThreadRegistry::default();
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let caller = callers
        .spawn("optimizer-cancelling-start", Qos::Default, move |_| {
            result_tx.send(review.start(DeckId(1), None)).unwrap();
        })
        .unwrap();
    let admitted = match result_rx.recv_timeout(WAIT).unwrap().unwrap() {
        StartResult::Started { session_id, .. } => session_id,
        result => panic!("policy-ready start failed: {result:?}"),
    };
    cancel_rx.recv_timeout(WAIT).unwrap();
    assert!(optimizer.cancel.is_cancelled());
    assert!(
        fixture
            .engine
            .jobs()
            .running()
            .iter()
            .any(|running| running.id == optimizer.id)
    );
    assert_eq!(hook_calls.0.load(Ordering::SeqCst), 1);
    assert_eq!(commits.load(Ordering::SeqCst), 0);
    assert_eq!(common::steps::dump(&fixture.collection), unchanged);
    finish_cleanup_tx.send(()).unwrap();
    caller.join().unwrap();

    let expires = monotonic::now() + WAIT;
    let first_outcome = loop {
        let notification = notifications
            .recv_timeout(expires.saturating_sub(monotonic::now()))
            .expect("optimizer completion was not delivered");
        if let EventKind::JobFinished { id, outcome } = &notification.kind
            && *id == optimizer.id
        {
            break outcome.clone();
        }
    };
    let mut outcomes = vec![first_outcome];
    while let Some(notification) = notifications.try_recv() {
        if let EventKind::JobFinished { id, outcome } = &notification.kind
            && *id == optimizer.id
        {
            outcomes.push(outcome.clone());
        }
    }
    assert_eq!(outcomes, [JobOutcome::Cancelled]);
    assert_eq!(commits.load(Ordering::SeqCst), 0);
    assert_eq!(common::steps::dump(&fixture.collection), unchanged);
    fixture.review.end(admitted).unwrap();
}

fn journal_records(bytes: &[u8]) -> Vec<serde_json::Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| {
            let frame: serde_json::Value = serde_json::from_str(line).unwrap();
            serde_json::from_str(frame["payload"].as_str().unwrap()).unwrap()
        })
        .collect()
}

#[derive(Clone, Debug)]
struct Applied {
    event: u64,
    row: [i64; 9],
    card_post: [u8; 32],
    commit: Seq,
}

#[derive(Default)]
struct Evidence {
    applied: Vec<Applied>,
    terminal: Vec<u64>,
}

fn observe_answers(
    h: &Harness,
    terminal_gate: Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>,
) -> Arc<Mutex<Evidence>> {
    let collected = Arc::new(Mutex::new(Evidence::default()));
    let for_hook = collected.clone();
    let collection = h.collection.clone();
    let journal_file = collection.path().parent().unwrap().join("meguri-pending-answers.jsonl");
    let first_terminal = Mutex::new(terminal_gate);
    h.collection.record_durability_events();
    h.review
        .journal_hook_for_testing(Arc::new(move |stage| {
            match stage {
                JournalStage::PreparedSynced => {
                    // Read journal evidence only; the writer is awaiting this acknowledgement.
                    let file_records = journal_records(&fs::read(&journal_file).unwrap());
                    let prepared_record = file_records
                        .iter()
                        .rev()
                        .find_map(|record| record.get("Prepared"))
                        .unwrap();
                    let event_id = prepared_record["event"].as_u64().unwrap();
                    let full_row: [i64; 9] = serde_json::from_value(prepared_record["receipt"]["row"].clone()).unwrap();
                    let resulting_card: [u8; 32] =
                        serde_json::from_value(prepared_record["receipt"]["post"].clone()).unwrap();
                    let on_commit = for_hook.clone();
                    collection.after_next_commit(move |sequence| {
                        on_commit.lock().unwrap().applied.push(Applied {
                            event: event_id,
                            row: full_row,
                            card_post: resulting_card,
                            commit: sequence,
                        });
                    });
                }
                JournalStage::TerminalWrite => {
                    let pause = first_terminal.lock().unwrap().take();
                    if let Some((arrived, proceed)) = pause {
                        arrived.send(()).unwrap();
                        proceed.recv_timeout(WAIT).unwrap();
                    }
                }
                JournalStage::TerminalSynced => {
                    let done = journal_records(&fs::read(&journal_file).unwrap())
                        .into_iter()
                        .filter_map(|record| {
                            let value = record.get("Terminal")?;
                            assert_eq!(value["status"], "Done");
                            Some(value["event"].as_u64().unwrap())
                        })
                        .collect();
                    for_hook.lock().unwrap().terminal = done;
                }
                _ => {}
            }
        }))
        .unwrap();
    collected
}

fn occupy_writer(c: &Collection) -> (mpsc::Sender<()>, meguri_engine::Pending<meguri_engine::OpResult<()>>) {
    let (entered, at_writer) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::channel();
    let pending = c.submit(
        OpSpec::new(catalog::BULK_OP, OpSource::Maintenance).declare_long_write(Duration::from_secs(3)),
        move |_| {
            entered.send(()).unwrap();
            gate.recv_timeout(WAIT).unwrap();
            Ok(())
        },
    );
    at_writer.recv_timeout(WAIT).unwrap();
    (release, pending)
}

fn accept_five(h: &Harness, session_key: SessionId, mut visible: Head) -> Vec<CardId> {
    let mut subjects = Vec::new();
    for elapsed in 101..=105 {
        let item = &visible.items[0];
        let subject = item.card_id;
        assert!(!subjects.contains(&subject));
        let result = h
            .review
            .answer(
                session_key,
                subject,
                Rating::Easy,
                item.state_token,
                EpochMillis(NOW),
                elapsed,
            )
            .unwrap();
        visible = match result {
            AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
            other => panic!("pending answer refused: {other:?}"),
        };
        assert_ne!(visible.items[0].card_id, subject);
        subjects.push(subject);
        assert!(h.logs().is_empty());
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        for subject in &subjects {
            assert_eq!(reader.card(*subject).unwrap().unwrap().reps, 0);
        }
    }
    subjects
}

fn assert_five_durable(h: &Harness, evidence: &Mutex<Evidence>, subjects: &[CardId]) -> Vec<Applied> {
    let applied = evidence.lock().unwrap().applied.clone();
    let terminal = evidence.lock().unwrap().terminal.clone();
    assert_eq!(applied.len(), 5);
    assert_eq!(terminal, applied.iter().map(|a| a.event).collect::<Vec<_>>());
    let stop = monotonic::now() + WAIT;
    loop {
        let syncs = h.collection.durability_events().syncs;
        if applied
            .iter()
            .all(|a| syncs.iter().any(|s| s.succeeded && s.through >= a.commit))
        {
            break;
        }
        assert!(monotonic::now() < stop, "lifecycle returned without real WAL coverage");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(h.logs().len(), 5);
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    for (ordinal, (answer, subject)) in applied.iter().zip(subjects).enumerate() {
        assert_eq!(answer.row[1], subject.0);
        assert_eq!(answer.row[3], Rating::Easy as i64);
        assert_eq!(answer.row[7], 101 + ordinal as i64);
        assert_eq!(
            rows.row_image(Table::Revlog, &RowKey::Id(answer.row[0]))
                .unwrap()
                .unwrap(),
            answer.row.iter().copied().map(Cell::Integer).collect::<Vec<_>>()
        );
        assert_eq!(
            rows.row_digest(Table::Cards, &RowKey::Id(subject.0)).unwrap(),
            Some(answer.card_post)
        );
        let card = rows.card(*subject).unwrap().unwrap();
        assert_eq!(card.reps, 1);
        assert_eq!(card.data.last_review_secs, Some(NOW / 1000));
    }
    applied
}

fn edit_and_reopen_twice(h: &Harness, subjects: &[CardId], applied: &[Applied]) {
    let subject = subjects[0];
    h.collection
        .run(
            OpSpec::named("ordinary edit after drain", OpSource::UserUndoable),
            move |ctx| {
                let mut card = ctx.scope().card(subject)?.unwrap();
                card.flags ^= 1;
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    let saved = common::steps::dump(&h.collection);
    let path = h.collection.path().to_owned();
    h.engine.close().unwrap();
    for _ in 0..2 {
        let (owner, store, _) = session_support::open(&path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(common::steps::dump(&store), saved);
        let reader = store.reader(ReaderRole::Query).unwrap();
        assert_eq!(reader.ids_after(Table::Revlog, None, 100).unwrap().len(), 5);
        for answer in applied {
            assert_eq!(
                reader
                    .row_image(Table::Revlog, &RowKey::Id(answer.row[0]))
                    .unwrap()
                    .unwrap(),
                answer.row.iter().copied().map(Cell::Integer).collect::<Vec<_>>()
            );
        }
        drop(reader);
        owner.close().unwrap();
    }
}

#[test]
fn c16_engine_background_suspend_and_exclusive_wait_for_five_real_pending_answers() {
    #[derive(Clone, Copy, Debug)]
    enum Drain {
        Background,
        Suspend,
        Exclusive,
    }
    for drain in [Drain::Background, Drain::Suspend, Drain::Exclusive] {
        let fixture = Harness::new(8);
        let (id, head) = fixture.start();
        fixture.settle();
        let (terminal_tx, terminal_rx) = mpsc::sync_channel(1);
        let (continue_tx, continue_rx) = mpsc::sync_channel(1);
        let proof = observe_answers(&fixture, Some((terminal_tx, continue_rx)));
        let (writer_continue, writer_operation) = occupy_writer(&fixture.collection);
        let cards = accept_five(&fixture, id, head);
        let engine = fixture.engine.clone();
        let (invoked_tx, invoked_rx) = mpsc::sync_channel(1);
        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        // The lifecycle caller must not belong to the engine it is suspending.
        let callers = ThreadRegistry::default();
        let caller = callers
            .spawn("five-answer-lifecycle-caller", Qos::Default, move |_| {
                invoked_tx.send(()).unwrap();
                let outcome = match drain {
                    Drain::Background => engine.flush_for_background(),
                    Drain::Suspend => engine.suspend(WAIT).map(|state| {
                        assert_eq!(state, SuspendOutcome::Suspended);
                    }),
                    Drain::Exclusive => {
                        let reservation = engine.acquire_slot(LongOp::OneWaySync).unwrap();
                        let entered = reservation.enter_exclusive(true).map(drop);
                        drop(reservation);
                        entered
                    }
                };
                finished_tx.send(outcome).unwrap();
            })
            .unwrap();
        invoked_rx.recv_timeout(WAIT).unwrap();
        assert!(
            matches!(
                finished_rx.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "{drain:?} completed with five answers still behind the writer"
        );
        writer_continue.send(()).unwrap();
        writer_operation.wait().unwrap();
        terminal_rx.recv_timeout(WAIT).unwrap();
        assert!(
            matches!(
                finished_rx.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "{drain:?} completed before durable terminal records"
        );
        continue_tx.send(()).unwrap();
        finished_rx.recv_timeout(WAIT).unwrap().unwrap();
        caller.join().unwrap();
        if matches!(drain, Drain::Suspend) {
            fixture.engine.resume().unwrap();
        }
        let receipts = assert_five_durable(&fixture, &proof, &cards);
        edit_and_reopen_twice(&fixture, &cards, &receipts);
    }
}

fn await_end_draining(h: &Harness, session_key: SessionId, token: [u8; 32]) {
    let stop = monotonic::now() + WAIT;
    loop {
        // An absent card can never be Accepted. Its ordinary NotAtHead reply
        // becomes an explicit ending refusal only after end owns control and
        // marks this real session as draining.
        match h
            .review
            .answer(session_key, CardId(-1), Rating::Easy, token, EpochMillis(NOW), 0)
        {
            Err(EngineError::CollectionUnavailable { reason }) if reason == "session is ending" => break,
            Ok(AnswerOutcome::NotAtHead { .. }) => {}
            other => panic!("unexpected end-drain probe result: {other:?}"),
        }
        assert!(monotonic::now() < stop, "end never entered the actual drain");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn c16_end_drains_before_competing_enable_or_start_and_emits_one_end() {
    for request_start in [false, true] {
        let fixture = Harness::new(8);
        let hook_calls = Arc::new(Starts::default());
        fixture.review.add_session_start_hook(hook_calls.clone());
        let (old_id, head) = fixture.start();
        let probe_token = head.items[0].state_token;
        fixture.settle();
        let stream = fixture.engine.stream().subscribe();
        let (terminal_tx, terminal_rx) = mpsc::sync_channel(1);
        let (finish_terminal_tx, finish_terminal_rx) = mpsc::sync_channel(1);
        let proof = observe_answers(&fixture, Some((terminal_tx, finish_terminal_rx)));
        let (writer_continue, writer_operation) = occupy_writer(&fixture.collection);
        let cards = accept_five(&fixture, old_id, head);
        let callers = ThreadRegistry::default();
        let review = fixture.review.clone();
        let (invoked_tx, invoked_rx) = mpsc::sync_channel(1);
        let (end_tx, end_rx) = mpsc::sync_channel(1);
        let ending = callers
            .spawn("five-answer-end-caller", Qos::Default, move |_| {
                invoked_tx.send(()).unwrap();
                end_tx.send(review.end(old_id)).unwrap();
            })
            .unwrap();
        invoked_rx.recv_timeout(WAIT).unwrap();
        await_end_draining(&fixture, old_id, probe_token);
        assert!(matches!(
            end_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        let review = fixture.review.clone();
        let collection = fixture.collection.clone();
        let (attempt_tx, attempt_rx) = mpsc::sync_channel(1);
        let (admission_tx, admission_rx) = mpsc::sync_channel(1);
        let contending = callers
            .spawn("five-answer-admission-caller", Qos::Default, move |_| {
                attempt_tx.send(()).unwrap();
                let outcome = if request_start {
                    review.start(DeckId(1), None).map(|result| match result {
                        StartResult::Started { session_id, .. } => Some(session_id),
                        refusal => panic!("replacement session failed: {refusal:?}"),
                    })
                } else {
                    enable_fsrs(&collection, &Default::default()).map(|_| None)
                };
                admission_tx.send(outcome).unwrap();
            })
            .unwrap();
        attempt_rx.recv_timeout(WAIT).unwrap();
        assert!(matches!(
            admission_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        writer_continue.send(()).unwrap();
        writer_operation.wait().unwrap();
        terminal_rx.recv_timeout(WAIT).unwrap();
        assert!(matches!(
            end_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        // Enable can observe the occupied admission; a start must wait for end's control lane.
        let before_done = match admission_rx.recv_timeout(Duration::from_millis(50)) {
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Ok(result) => {
                assert!(!request_start);
                assert_eq!(result, Err(EngineError::SessionActive));
                Some(result)
            }
            Err(disconnected) => panic!("admission caller disappeared: {disconnected}"),
        };
        assert_eq!(hook_calls.0.load(Ordering::SeqCst), 1);
        finish_terminal_tx.send(()).unwrap();
        end_rx.recv_timeout(WAIT).unwrap().unwrap();
        ending.join().unwrap();
        let outcome = match before_done {
            Some(result) => result,
            None => admission_rx.recv_timeout(WAIT).unwrap(),
        };
        contending.join().unwrap();
        let new_id = match outcome {
            Ok(value) => value,
            Err(EngineError::SessionActive) if !request_start => {
                // Successful end above establishes the state change required for a new enable call.
                enable_fsrs(&fixture.collection, &Default::default()).unwrap();
                None
            }
            failure => panic!("admission failed after the drain: {failure:?}"),
        };
        assert_eq!(hook_calls.0.load(Ordering::SeqCst), if request_start { 2 } else { 1 });
        if request_start {
            assert_ne!(new_id.unwrap(), old_id);
        } else {
            assert!(new_id.is_none());
        }
        let receipts = assert_five_durable(&fixture, &proof, &cards);
        let mut old_end_events = 0;
        while let Some(event) = stream.try_recv() {
            if let EventKind::Extension(value) = &event.kind
                && let Some(session_event) = value.as_any().downcast_ref::<SessionEvent>()
            {
                match session_event {
                    SessionEvent::SessionEnded { session_id } if *session_id == old_id => old_end_events += 1,
                    SessionEvent::CommitFailed { .. } | SessionEvent::RecoveryDropped { .. } => {
                        panic!("end lost an accepted transition: {session_event:?}");
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(old_end_events, 1);
        if let Some(id) = new_id {
            fixture.review.end(id).unwrap();
        }
        edit_and_reopen_twice(&fixture, &cards, &receipts);
    }
}

#[test]
fn c16_failed_terminal_sync_errors_and_retains_admission_and_recovery_evidence() {
    let fixture = Harness::new(8);
    let (active_id, first_page) = fixture.start();
    fixture.settle();
    let delivered = fixture.engine.stream().subscribe();
    let observations = observe_answers(&fixture, None);
    fixture
        .review
        .fail_journal_for_testing(JournalStage::TerminalSync)
        .unwrap();
    let (unblock, writing) = occupy_writer(&fixture.collection);
    let accepted_ids = accept_five(&fixture, active_id, first_page);
    assert_eq!(accepted_ids.len(), 5);
    let control_service = fixture.review.clone();
    let (completion_send, completion_receive) = mpsc::sync_channel(1);
    let outside_threads = ThreadRegistry::default();
    let end_call = outside_threads
        .spawn("terminal-failure-end-caller", Qos::Default, move |_| {
            completion_send.send(control_service.end(active_id)).unwrap();
        })
        .unwrap();
    let early_completion = completion_receive.recv_timeout(Duration::from_millis(100));
    unblock.send(()).unwrap();
    writing.wait().unwrap();
    let completion = completion_receive.recv_timeout(WAIT).unwrap();
    end_call.join().unwrap();
    assert!(matches!(early_completion, Err(mpsc::RecvTimeoutError::Timeout)));
    let persistent_error = completion.unwrap_err();
    match &persistent_error {
        EngineError::Storage(meguri_storage::StorageError::Io { detail }) => {
            assert!(detail.contains("TerminalSync"), "wrong journal fault: {detail}");
        }
        other => panic!("terminal sync returned another error category: {other:?}"),
    }
    assert_eq!(fixture.review.active_session(), Some(active_id));
    let evidence_path = fixture
        .collection
        .path()
        .parent()
        .unwrap()
        .join("meguri-pending-answers.jsonl");
    let retained_bytes = fs::read(&evidence_path).unwrap();
    let retained_records = journal_records(&retained_bytes);
    assert_eq!(
        retained_records
            .iter()
            .filter(|value| value.get("Intent").is_some())
            .count(),
        5
    );
    assert!(retained_records.iter().any(|value| value.get("Prepared").is_some()));

    let enable_result = enable_fsrs(&fixture.collection, &Default::default()).unwrap_err();
    assert_eq!(enable_result, persistent_error);
    assert_eq!(fixture.review.active_session(), Some(active_id));
    assert_eq!(fs::read(&evidence_path).unwrap(), retained_bytes);
    let start_result = fixture.review.start(DeckId(1), None).unwrap_err();
    assert_eq!(start_result, persistent_error);
    assert_eq!(fixture.review.active_session(), Some(active_id));
    assert_eq!(fs::read(&evidence_path).unwrap(), retained_bytes);

    let captured = observations.lock().unwrap();
    assert!(captured.terminal.is_empty());
    assert!(!captured.applied.is_empty());
    let stored_ids = fixture.logs();
    assert!(!stored_ids.is_empty());
    assert!(stored_ids.len() <= accepted_ids.len());
    let storage = fixture.collection.reader(ReaderRole::Query).unwrap();
    let wal = fixture.collection.durability_events();
    for receipt in &captured.applied {
        assert!(
            wal.syncs
                .iter()
                .any(|sync| !sync.succeeded && sync.through >= receipt.commit)
        );
        let row = storage
            .row_image(Table::Revlog, &RowKey::Id(receipt.row[0]))
            .unwrap()
            .unwrap();
        let assigned_cells: Vec<_> = receipt.row.iter().map(|value| Cell::Integer(*value)).collect();
        assert_eq!(row, assigned_cells);
    }
    while let Some(envelope) = delivered.try_recv() {
        if let EventKind::Extension(payload) = &envelope.kind
            && let Some(SessionEvent::SessionEnded { session_id }) = payload.as_any().downcast_ref()
        {
            assert_ne!(*session_id, active_id);
        }
    }
}

#[test]
fn c16_suspend_deadline_bounds_compaction_and_a_barrier_queued_behind_it() {
    let fixture = Harness::new(8);
    let (session_id, initial) = fixture.start();
    fixture.settle();
    fixture.collection.record_durability_events();
    let (at_compact, compact_reached) = mpsc::sync_channel(1);
    let (allow_compact, compact_release) = mpsc::sync_channel(1);
    let once = Mutex::new(Some(compact_release));
    fixture
        .review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == JournalStage::Compact
                && let Some(release) = once.lock().unwrap().take()
            {
                at_compact.send(()).unwrap();
                release.recv_timeout(WAIT).unwrap();
            }
        }))
        .unwrap();
    let (allow_writer, writer) = occupy_writer(&fixture.collection);
    let answered = accept_five(&fixture, session_id, initial);
    allow_writer.send(()).unwrap();
    writer.wait().unwrap();
    fixture.review.application_barrier_for_testing().unwrap();

    let callers = ThreadRegistry::default();
    let engine = fixture.engine.clone();
    let (first_send, first_receive) = mpsc::sync_channel(1);
    let first = callers
        .spawn("compact-deadline-caller", Qos::Default, move |_| {
            let began = monotonic::now();
            let result = engine.suspend(Duration::from_secs(1));
            first_send.send((result, monotonic::now() - began)).unwrap();
        })
        .unwrap();
    let reached = compact_reached.recv_timeout(WAIT);
    let path = fixture
        .collection
        .path()
        .parent()
        .unwrap()
        .join("meguri-pending-answers.jsonl");
    let before_release = fs::read(&path).unwrap();
    let first_reply = first_receive.recv_timeout(Duration::from_secs(2));
    let mut later = None;
    let mut later_reply = None;
    if reached.is_ok() && first_reply.is_ok() {
        let engine = fixture.engine.clone();
        let (send, receive) = mpsc::sync_channel(1);
        later = Some(
            callers
                .spawn("queued-journal-barrier-caller", Qos::Default, move |_| {
                    let began = monotonic::now();
                    let result = engine.suspend(Duration::from_millis(300));
                    send.send((result, monotonic::now() - began)).unwrap();
                })
                .unwrap(),
        );
        later_reply = Some(receive.recv_timeout(Duration::from_secs(1)));
    }
    let after_timeouts = fs::read(&path).unwrap();
    allow_compact.send(()).unwrap();
    first.join().unwrap();
    if let Some(caller) = later {
        caller.join().unwrap();
    }
    reached.unwrap();
    let (first_result, first_elapsed) = first_reply.unwrap();
    assert!(matches!(first_result, Err(EngineError::SuspendDeadline { .. })));
    assert!(first_elapsed >= Duration::from_secs(1));
    assert!(first_elapsed < Duration::from_millis(1500));
    let (second_result, second_elapsed) = later_reply.unwrap().unwrap();
    assert!(matches!(second_result, Err(EngineError::SuspendDeadline { .. })));
    assert!(second_elapsed >= Duration::from_millis(300));
    assert!(second_elapsed < Duration::from_millis(800));
    assert_eq!(before_release, after_timeouts);
    let entries = journal_records(&before_release);
    assert_eq!(entries.iter().filter(|entry| entry.get("Intent").is_some()).count(), 5);
    assert_eq!(
        entries.iter().filter(|entry| entry.get("Terminal").is_some()).count(),
        5
    );
    let assigned: Vec<[i64; 9]> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .get("Prepared")
                .map(|prepared| serde_json::from_value(prepared["receipt"]["row"].clone()).unwrap())
        })
        .collect();
    assert_eq!(assigned.len(), 5);
    assert_eq!(fixture.review.active_session(), Some(session_id));

    fixture.engine.resume().unwrap();
    fixture.review.flush_for_background().unwrap();
    assert_eq!(fixture.engine.suspend(WAIT).unwrap(), SuspendOutcome::Suspended);
    fixture.engine.resume().unwrap();
    assert_eq!(fixture.logs().len(), 5);
    let rows = fixture.collection.reader(ReaderRole::Query).unwrap();
    for (card_id, receipt) in answered.iter().zip(assigned) {
        assert_eq!(receipt[1], card_id.0);
        assert_eq!(
            rows.row_image(Table::Revlog, &RowKey::Id(receipt[0])).unwrap().unwrap(),
            receipt.into_iter().map(Cell::Integer).collect::<Vec<_>>()
        );
        let card = rows.card(*card_id).unwrap().unwrap();
        assert_eq!(card.reps, 1);
        assert_eq!(card.data.last_review_secs, Some(NOW / 1000));
    }
    assert!(
        fixture
            .collection
            .durability_events()
            .syncs
            .iter()
            .any(|sync| sync.succeeded)
    );
}
