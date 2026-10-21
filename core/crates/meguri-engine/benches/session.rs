//! U12 production C5 budgets. Prepare 14400/150k fixtures in another process.
use criterion::Criterion;
use meguri_engine::{
    Engine, EngineConfig, OpSource, OpSpec, OpenConfig, Ports, UndoScope,
    sched::queue::QueueProvider,
    session::{AnswerOutcome, JournalStage, PeekResult, ReviewService, StartResult},
};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, Table};
use meguri_testkit::{
    bench::{
        fixtures,
        peak_alloc::PeakAlloc,
        report::{self, Measurement, Suite},
        timing::Percentile,
    },
    generate::DEFAULT_NOW_MS,
    wallclock::Stopwatch,
};
use std::{
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

// Own both benchmark readers until their threads have been joined.
#[derive(Default)]
struct AnswerLoad {
    stop: Arc<AtomicBool>,
    progress: [Arc<AtomicU64>; 2],
    workers: Vec<(&'static str, std::thread::JoinHandle<()>)>,
}
impl AnswerLoad {
    fn counts(&self) -> [u64; 2] {
        std::array::from_fn(|index| self.progress[index].load(Ordering::Acquire))
    }

    fn assert_running(&mut self) {
        let ended: Vec<_> = self
            .workers
            .iter()
            .filter_map(|(label, task)| task.is_finished().then_some(*label))
            .collect();
        if ended.is_empty() {
            return;
        }
        let join_errors = self.stop_and_join();
        panic!("benchmark readers ended: {ended:?}; unsuccessful joins: {join_errors:?}");
    }

    fn stop_and_join(&mut self) -> Vec<&'static str> {
        self.stop.store(true, Ordering::Release);
        self.workers
            .drain(..)
            .filter_map(|(label, task)| task.join().err().map(|_| label))
            .collect()
    }
}
impl Drop for AnswerLoad {
    fn drop(&mut self) {
        let join_errors = self.stop_and_join();
        if join_errors.is_empty() {
            return;
        }
        if std::thread::panicking() {
            eprintln!("benchmark reader cleanup could not join: {join_errors:?}");
        } else {
            panic!("benchmark reader cleanup could not join: {join_errors:?}");
        }
    }
}

#[derive(Debug)]
enum HistoryVisitFailure {
    Cancelled,
    ReadFailed(meguri_storage::StorageError),
}
impl From<meguri_storage::StorageError> for HistoryVisitFailure {
    fn from(cause: meguri_storage::StorageError) -> Self {
        Self::ReadFailed(cause)
    }
}
fn start(review: &ReviewService) -> meguri_engine::SessionId {
    match review.start(DeckId(1), None).expect("production admission") {
        StartResult::Started { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    }
}
fn main() -> ExitCode {
    let mut criterion = Criterion::default().sample_size(20).configure_from_args();
    let mut suite = Suite::wall_clock("session");
    let mut durability_failed = false;
    for size in std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "14400,150k".into())
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let fixture = fixtures::profile_collection(size).expect("prepared fixture");
        assert!(fixture.reused, "prepare/reuse fixtures before the measured process");
        eprintln!("session fixture {size}: fixture.reused={}", fixture.reused);
        let copy = fixture.working_copy().unwrap();
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let queues = Arc::new(QueueProvider::default());
        let review = ReviewService::new(&engine, queues.clone()).unwrap();
        let ports = Ports::new()
            .observer(queues.clone())
            .generation_listener(queues.clone())
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone());
        let c = engine
            .open_raw(
                &copy.path,
                OpenConfig::new(
                    Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
                    Arc::new(SeededRandom::new(43)),
                )
                .ports(ports),
            )
            .unwrap();
        // Existing generated profiles may have FSRS off. The real policy owner
        // performs enable before measuring; unsupported homes remain refusals.
        meguri_engine::sched::fsrs_policy::enable_fsrs(&c, &Default::default()).unwrap();
        c.run(OpSpec::named("benchmark selection", OpSource::Maintenance), |ctx| {
            ctx.scope().set_config("curDeck", &ConfigValue::from(1i64))?;
            Ok(())
        })
        .unwrap();
        let mut peek = Measurement::start("session.peekFirstCard", Some(size));
        criterion.bench_function(&format!("session/peek/{size}"), |b| {
            b.iter_custom(|n| {
                peek.run(n, |_| {
                    assert!(matches!(
                        review.peek_first_card(DeckId(1)).unwrap(),
                        PeekResult::Card(_)
                    ));
                })
            })
        });
        suite.push(peek.finish());
        let mut starts = Measurement::start("session.start_head3", Some(size));
        criterion.bench_function(&format!("session/start_head3/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let mut id = None;
                    total += starts.run(1, |_| {
                        let started = start(&review);
                        assert!(!review.head(started, 3).unwrap().items.is_empty());
                        id = Some(started);
                    });
                    review.end(id.unwrap()).unwrap();
                }
                total
            })
        });
        suite.push(starts.finish());
        // Select the profile's largest generated shape, not Default's head.
        // All inspection and any scheduling-only preparation are untimed.
        let (kind, note, card) = {
            let reader = c.reader(ReaderRole::Query).unwrap();
            let snapshot = reader.snapshot().unwrap();
            let kind = snapshot
                .ids_after(Table::Notetypes, None, 1000)
                .unwrap()
                .into_iter()
                .map(|id| snapshot.notetype(meguri_model::ids::NotetypeId(id)).unwrap().unwrap())
                .find(|kind| kind.name == "Meguri Synthetic 1")
                .expect("largest generated profile note type");
            let note = snapshot.notetype_notes_after(kind.id, None, 1).unwrap().remove(0);
            let card = snapshot
                .cards_of_note(note.id)
                .unwrap()
                .into_iter()
                .find(|card| card.ord == 0)
                .expect("generated first-template card");
            (kind, note, card)
        };
        assert_eq!(kind.fields.len(), 12);
        assert_eq!(note.field_values().count(), 12);
        assert_eq!(kind.templates.len(), 1);
        assert!(kind.config.css.len() >= 9000);
        let template = &kind.templates[0];
        assert_eq!(template.config.q_format.len(), 31500);
        assert_eq!(template.config.a_format.len(), 32000);
        let script = |text: &str| {
            text.split_once("<script>")
                .unwrap()
                .1
                .split_once("</script>")
                .unwrap()
                .0
                .to_owned()
        };
        let question_script = script(&template.config.q_format);
        let answer_script = script(&template.config.a_format);
        assert_eq!(question_script.len() + answer_script.len(), 60000);
        let mut render_id = match review.start(card.deck_id, Some(card.id)).unwrap() {
            StartResult::Started { session_id, .. } => session_id,
            other => panic!("large-shape admission: {other:?}"),
        };
        let prepared = review
            .head(render_id, 3)
            .unwrap()
            .items
            .first()
            .map(|item| item.card_id)
            != Some(card.id);
        if prepared {
            review.end(render_id).unwrap();
            // The generated card may be future/buried or outside a daily cap.
            // Only its scheduling/placement changes on this private copy; the
            // original row is restored immediately after the render case.
            let card_id = card.id;
            c.run(
                OpSpec::named("benchmark large render preparation", OpSource::Maintenance),
                move |ctx| {
                    let mut selected = ctx.scope().card(card_id)?.unwrap();
                    selected.deck_id = DeckId(1);
                    selected.original_deck_id = DeckId(0);
                    selected.original_due = 0;
                    selected.card_type = CardType::New;
                    selected.queue = CardQueue::New;
                    selected.due = 0;
                    ctx.scope().put(&selected)?;
                    Ok(())
                },
            )
            .unwrap();
            render_id = match review.start(DeckId(1), Some(card.id)).unwrap() {
                StartResult::Started { session_id, .. } => session_id,
                other => panic!("prepared large-shape admission: {other:?}"),
            };
        }
        let head = review.head(render_id, 3).unwrap();
        assert_eq!(
            head.items[0].card_id, card.id,
            "large generated card must actually be presented"
        );
        let render_key = head.items[0].render_key;
        // Warm the parser and retain the real payload as an untimed witness.
        let expected = review
            .render_head(render_id, &[render_key], meguri_engine::rendering::Platform::Mac)
            .unwrap()
            .remove(0);
        assert_eq!(expected.card_id, card.id);
        assert_eq!(expected.css, kind.config.css);
        assert_eq!(expected.comparable.outcome, meguri_engine::rendering::Outcome::Rendered);
        let question = expected.question.as_deref().unwrap();
        let answer_html = expected.answer.as_deref().unwrap();
        assert!(question.contains(&question_script) && answer_html.contains(&answer_script));
        assert!(question.contains("<!-- mg-fill ") && answer_html.contains("<!-- mg-fill "));
        let fields: Vec<_> = note.field_values().collect();
        assert!(question.contains(fields[0]) && answer_html.contains(fields[1]));
        eprintln!(
            "session render {size}: card={:?}, notetype={:?}, fields={}, field_bytes={:?}, CSS={}, templates={}/{}, scripts={}/{}, HTML={}/{}, media={}, AV={}/{}, type_in={}, private_scheduling_preparation={prepared}; Mac U62, parser warmed, session LRU miss per sample",
            card.id,
            kind.id,
            fields.len(),
            fields.iter().map(|field| field.len()).collect::<Vec<_>>(),
            expected.css.len(),
            template.config.q_format.len(),
            template.config.a_format.len(),
            question_script.len(),
            answer_script.len(),
            question.len(),
            answer_html.len(),
            expected.media_refs.len(),
            expected.comparable.question_av.len(),
            expected.comparable.answer_av.len(),
            expected.type_in.is_some()
        );
        let mut render = Measurement::start("session.renderHead", Some(size));
        criterion.bench_function(&format!("session/render/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let head = review.head(render_id, 3).unwrap();
                    assert_eq!(head.items[0].card_id, card.id);
                    let key = head.items[0].render_key;
                    review.clear_render_cache_for_testing();
                    let before = review.render_count();
                    let mut payload = None;
                    total += render.run(1, |_| {
                        payload = Some(
                            review
                                .render_head(render_id, &[key], meguri_engine::rendering::Platform::Mac)
                                .unwrap(),
                        );
                    });
                    assert_eq!(
                        payload.unwrap(),
                        vec![expected.clone()],
                        "actual large-shape HTML/media/AV/type-in payload"
                    );
                    assert_eq!(
                        review.render_count(),
                        before + 1,
                        "measure an actual U62 render, not an LRU hit"
                    );
                }
                total
            })
        });
        suite.push(render.finish());
        review.end(render_id).unwrap();
        if prepared {
            c.run(
                OpSpec::named("benchmark restore render card", OpSource::Maintenance),
                move |ctx| {
                    ctx.scope().put(&card)?;
                    Ok(())
                },
            )
            .unwrap();
        }
        let id = start(&review);
        eprintln!(
            "session {size}: current answer workload = sync snapshot + maintenance history load; U81 regex compiler, U85 slow streaming search, U95 stats aggregate build PENDING. Full three-workload concurrency remains mandatory at <=1 ms p95; this measurement does not establish it."
        );
        let mut load = AnswerLoad::default();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let sync = {
            let collection = c.clone();
            let stopping = Arc::clone(&load.stop);
            let progress = Arc::clone(&load.progress[0]);
            let ready = ready_tx.clone();
            engine
                .threads()
                .spawn("bench-sync-snapshot", meguri_engine::Qos::Utility, move |activity| {
                    let _busy = activity.busy();
                    let reader = collection.reader(ReaderRole::Sync).unwrap();
                    let snapshot = reader.snapshot().unwrap();
                    ready.send((0, false)).unwrap();
                    std::hint::black_box(snapshot.col().unwrap());
                    progress.fetch_add(1, Ordering::Release);
                    ready.send((0, true)).unwrap();
                    // One real col read per stop check; the same owned snapshot
                    // and lease stay held until all answer samples have ended.
                    while !stopping.load(Ordering::Acquire) {
                        std::hint::black_box(snapshot.col().unwrap());
                        progress.fetch_add(1, Ordering::Release);
                    }
                })
                .unwrap()
        };
        load.workers.push(("sync snapshot", sync));
        let maintenance = {
            let collection = c.clone();
            let stopping = Arc::clone(&load.stop);
            let progress = Arc::clone(&load.progress[1]);
            let ready = ready_tx.clone();
            engine
                .threads()
                .spawn(
                    "bench-maintenance-history",
                    meguri_engine::Qos::Utility,
                    move |activity| {
                        use meguri_storage::queries::revlog_fsrs::HistorySelection;
                        let _busy = activity.busy();
                        let mut announced_ready = false;
                        let mut announced_work = false;
                        while !stopping.load(Ordering::Acquire) {
                            let reader = collection.reader(ReaderRole::Maintenance).unwrap();
                            let snapshot = reader.snapshot().unwrap();
                            let homes = snapshot.deck_rows().unwrap().into_iter().map(|deck| deck.id).collect();
                            if !announced_ready {
                                ready.send((1, false)).unwrap();
                                announced_ready = true;
                            }
                            let result = snapshot.visit_fsrs_histories(
                                &HistorySelection::HomeDecks(homes),
                                |card, history| -> Result<(), HistoryVisitFailure> {
                                    if stopping.load(Ordering::Acquire) {
                                        return Err(HistoryVisitFailure::Cancelled);
                                    }
                                    std::hint::black_box(card);
                                    for entry in history {
                                        std::hint::black_box(entry);
                                    }
                                    if !history.is_empty() {
                                        progress.fetch_add(1, Ordering::Release);
                                        if !announced_work {
                                            ready.send((1, true)).unwrap();
                                            announced_work = true;
                                        }
                                    }
                                    Ok(())
                                },
                            );
                            match result {
                                Ok(()) => assert!(announced_work, "fixture must contain actual histories"),
                                Err(HistoryVisitFailure::Cancelled) => break,
                                Err(HistoryVisitFailure::ReadFailed(error)) => {
                                    panic!("maintenance history load: {error}")
                                }
                            }
                        }
                    },
                )
                .unwrap()
        };
        load.workers.push(("maintenance history", maintenance));
        drop(ready_tx);
        let mut ready = [false; 2];
        let mut first_work = [false; 2];
        while !ready.iter().chain(&first_work).all(|seen| *seen) {
            let (worker, worked) = ready_rx
                .recv_timeout(Duration::from_secs(30))
                .expect("both workers must acquire snapshots and perform first real work");
            if worked {
                first_work[worker] = true;
            } else {
                ready[worker] = true;
            }
        }
        load.assert_running();
        let initial_progress = load.counts();
        let mut answer_progress_samples = [0_u64; 2];
        let starts_at = Arc::new(Mutex::new(std::collections::VecDeque::<Stopwatch>::new()));
        let coverage = Arc::new(Mutex::new(Vec::new()));
        let starts_hook = starts_at.clone();
        let coverage_hook = coverage.clone();
        review
            .journal_hook_for_testing(Arc::new(move |stage| {
                if stage == JournalStage::IntentSynced
                    && let Some(accepted) = starts_hook.lock().unwrap().pop_front()
                {
                    coverage_hook.lock().unwrap().push(accepted.elapsed());
                }
            }))
            .unwrap();
        let mut answer = Measurement::start("session.answer_overlay_sync_history", Some(size));
        criterion.bench_function(&format!("session/answer_overlay_sync_history/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let head = review.head(id, 3).unwrap();
                    let card = &head.items[0];
                    load.assert_running();
                    let progress_before = load.counts();
                    starts_at.lock().unwrap().push_back(Stopwatch::start());
                    total += answer.run(1, |_| {
                        assert!(matches!(
                            review
                                .answer(
                                    id,
                                    card.card_id,
                                    Rating::Good,
                                    card.state_token,
                                    EpochMillis(DEFAULT_NOW_MS),
                                    4000
                                )
                                .unwrap(),
                            AnswerOutcome::Accepted { .. }
                        ));
                    });
                    let progress_after = load.counts();
                    for worker in 0..2 {
                        answer_progress_samples[worker] += u64::from(progress_after[worker] > progress_before[worker]);
                    }
                    load.assert_running();
                    review.flush_for_background().unwrap();
                    review.undo(UndoScope::Session(id)).unwrap();
                    review.flush_for_background().unwrap();
                }
                total
            })
        });
        suite.push(answer.finish());
        durability_failed |= coverage.lock().unwrap().iter().any(|d| *d > Duration::from_secs(1))
            || !starts_at.lock().unwrap().is_empty();
        eprintln!(
            "session {size}: {} independently observed acceptance->intent full-sync samples",
            coverage.lock().unwrap().len()
        );
        load.assert_running();
        let final_progress = load.counts();
        let worker_failures = load.stop_and_join();
        assert!(
            worker_failures.is_empty(),
            "answer load worker failures: {worker_failures:?}"
        );
        for worker in 0..2 {
            assert!(
                final_progress[worker] > initial_progress[worker],
                "worker must progress during answer measurement"
            );
            assert!(
                answer_progress_samples[worker] > 0,
                "worker must progress across actual answer calls"
            );
        }
        eprintln!(
            "session {size}: sync col reads / real nonempty histories during answer measurement = {:?}; answer calls overlapping worker progress = {answer_progress_samples:?}",
            [
                final_progress[0] - initial_progress[0],
                final_progress[1] - initial_progress[1]
            ]
        );
        let mut undo = Measurement::start("session.undo_to_head", Some(size));
        criterion.bench_function(&format!("session/undo_to_head/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let head = review.head(id, 3).unwrap();
                    let card = &head.items[0];
                    review
                        .answer(
                            id,
                            card.card_id,
                            Rating::Good,
                            card.state_token,
                            EpochMillis(DEFAULT_NOW_MS),
                            4000,
                        )
                        .unwrap();
                    review.flush_for_background().unwrap();
                    total += undo.run(1, |_| {
                        assert_eq!(
                            review.undo(UndoScope::Session(id)).unwrap().items[0].card_id,
                            card.card_id
                        );
                    });
                    review.flush_for_background().unwrap();
                }
                total
            })
        });
        suite.push(undo.finish());
        let mut delayed_undo = Measurement::start("session.undo_to_head_sync200ms", Some(size));
        // Every sample needs a real 200 ms writer occupancy. Criterion would
        // calibrate from just the tiny measured undo and request huge setups.
        for _ in 0..20 {
            let head = review.head(id, 3).unwrap();
            let card = &head.items[0];
            review
                .answer(
                    id,
                    card.card_id,
                    Rating::Good,
                    card.state_token,
                    EpochMillis(DEFAULT_NOW_MS),
                    4000,
                )
                .unwrap();
            review.flush_for_background().unwrap();
            let (entered, ready) = std::sync::mpsc::sync_channel(1);
            let write = c.submit(
                OpSpec::new(
                    meguri_engine::executor::catalog::SYNC_INCOMING_MERGE,
                    OpSource::SyncIncoming,
                )
                .declare_long_write(Duration::from_millis(200)),
                move |_| {
                    entered.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(200));
                    Ok(())
                },
            );
            ready.recv().unwrap();
            delayed_undo.run(1, |_| {
                assert_eq!(
                    review.undo(UndoScope::Session(id)).unwrap().items[0].card_id,
                    card.card_id
                );
            });
            write.wait().unwrap();
            review.flush_for_background().unwrap();
        }
        suite.push(delayed_undo.finish());
        review.end(id).unwrap();
        engine.close().unwrap();
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    let mut failed = durability_failed;
    for record in suite.records() {
        let budget = match record.name.as_str() {
            "session.peekFirstCard" => Duration::from_millis(20),
            "session.start_head3" => Duration::from_millis(150),
            "session.renderHead" => Duration::from_millis(5),
            "session.answer_overlay_sync_history" => Duration::from_millis(1),
            "session.undo_to_head" | "session.undo_to_head_sync200ms" => Duration::from_millis(50),
            _ => unreachable!(),
        };
        let p95 = record.percentile(Percentile::P95);
        eprintln!("{} {:?}: p95={p95:?}, budget={budget:?}", record.name, record.fixture);
        if cfg!(target_vendor = "apple") && p95 > budget {
            failed = true;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
