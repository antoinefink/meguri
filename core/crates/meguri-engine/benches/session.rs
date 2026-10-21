//! U12 production C5 budgets. Prepare 14400/150k fixtures in another process.
use criterion::Criterion;
use meguri_engine::{
    Engine, EngineConfig, OpSource, OpSpec, OpenConfig, Ports, UndoScope,
    sched::queue::QueueProvider,
    session::{AnswerOutcome, JournalStage, PeekResult, ReviewService, StartResult},
};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
use meguri_testkit::{
    bench::{
        fixtures,
        peak_alloc::PeakAlloc,
        report::{self, Measurement, Suite},
        timing::Percentile,
    },
    generate::DEFAULT_NOW_MS,
};
use std::{
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;
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
        let id = start(&review);
        let mut render = Measurement::start("session.renderHead", Some(size));
        criterion.bench_function(&format!("session/render/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let head = review.head(id, 3).unwrap();
                    let key = head.items[0].render_key;
                    review.clear_render_cache_for_testing();
                    let before = review.render_count();
                    total += render.run(1, |_| {
                        let rendered = review
                            .render_head(id, &[key], meguri_engine::rendering::Platform::Mac)
                            .unwrap();
                        assert!(rendered[0].question.is_some());
                    });
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
        // Keep the sync reader's snapshot open throughout answer measurement.
        // Search and maintenance each repeat their real storage work alongside
        // it. U95 has no statistics provider yet; that concurrency remains unmeasured.
        let stop = Arc::new(AtomicBool::new(false));
        let sync = {
            let collection = c.clone();
            let stopping = Arc::clone(&stop);
            engine
                .threads()
                .spawn("bench-sync-snapshot", meguri_engine::Qos::Utility, move |_| {
                    let reader = collection.reader(ReaderRole::Sync).unwrap();
                    let snapshot = reader.snapshot().unwrap();
                    while !stopping.load(Ordering::Acquire) {
                        std::hint::black_box(snapshot.col().unwrap());
                    }
                })
                .unwrap()
        };
        let search = {
            let collection = c.clone();
            let stopping = Arc::clone(&stop);
            engine
                .threads()
                .spawn("bench-regex-search", meguri_engine::Qos::Utility, move |_| {
                    let context = meguri_search::SearchContext {
                        now: DEFAULT_NOW_MS / 1000,
                        today: 0,
                        next_day_at: DEFAULT_NOW_MS / 1000 + 86400,
                        learn_ahead_secs: 1200,
                        current_deck: DeckId(1),
                        normalize_note_text: true,
                        ignore_accents: false,
                        fold: Default::default(),
                    };
                    let expression = meguri_search::parse("re:.*").unwrap();
                    let plan = meguri_search::Plan::build(&expression, meguri_search::SearchMode::Cards, &context)
                        .expect("production regex plan");
                    let bindings: Vec<_> = plan
                        .params()
                        .iter()
                        .map(|parameter| {
                            use meguri_search::plan::Param;
                            use meguri_storage::Cell;
                            match parameter {
                                Param::Integer(number) => Cell::Integer(*number),
                                Param::Real(number) => Cell::Real(*number),
                                Param::Text(text) => Cell::text(text),
                            }
                        })
                        .collect();
                    while !stopping.load(Ordering::Acquire) {
                        let reader = collection.search_reader(0).unwrap();
                        std::hint::black_box(reader.search_card_ids(plan.sql(), &bindings).unwrap());
                    }
                })
                .unwrap()
        };
        let maintenance = {
            let collection = c.clone();
            let stopping = Arc::clone(&stop);
            engine
                .threads()
                .spawn("bench-maintenance-history", meguri_engine::Qos::Utility, move |_| {
                    use meguri_storage::queries::revlog_fsrs::HistorySelection;
                    while !stopping.load(Ordering::Acquire) {
                        let reader = collection.reader(ReaderRole::Maintenance).unwrap();
                        let snapshot = reader.snapshot().unwrap();
                        let homes = snapshot.deck_rows().unwrap().into_iter().map(|deck| deck.id).collect();
                        snapshot
                            .visit_fsrs_histories(
                                &HistorySelection::HomeDecks(homes),
                                |_, history| -> Result<(), meguri_storage::StorageError> {
                                    std::hint::black_box(history);
                                    Ok(())
                                },
                            )
                            .unwrap();
                    }
                })
                .unwrap()
        };
        let starts_at = Arc::new(Mutex::new(std::collections::VecDeque::<Instant>::new()));
        let coverage = Arc::new(Mutex::new(Vec::new()));
        let starts_hook = starts_at.clone();
        let coverage_hook = coverage.clone();
        review
            .journal_hook_for_testing(Arc::new(move |stage| {
                if stage == JournalStage::IntentSynced {
                    if let Some(accepted) = starts_hook.lock().unwrap().pop_front() {
                        coverage_hook.lock().unwrap().push(accepted.elapsed());
                    }
                }
            }))
            .unwrap();
        let mut answer = Measurement::start("session.answer_overlay_concurrent", Some(size));
        criterion.bench_function(&format!("session/answer_overlay_concurrent/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let head = review.head(id, 3).unwrap();
                    let card = &head.items[0];
                    starts_at.lock().unwrap().push_back(Instant::now());
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
                    review.flush_for_background().unwrap();
                    review.undo(UndoScope::Session(id)).unwrap();
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
        stop.store(true, Ordering::Release);
        sync.join().unwrap();
        search.join().unwrap();
        maintenance.join().unwrap();
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
                }
                total
            })
        });
        suite.push(undo.finish());
        let mut delayed_undo = Measurement::start("session.undo_to_head_sync200ms", Some(size));
        criterion.bench_function(&format!("session/undo_to_head_sync200ms/{size}"), |b| {
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
                    total += delayed_undo.run(1, |_| {
                        assert_eq!(
                            review.undo(UndoScope::Session(id)).unwrap().items[0].card_id,
                            card.card_id
                        );
                    });
                    write.wait().unwrap();
                }
                total
            })
        });
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
            "session.answer_overlay_concurrent" => Duration::from_millis(1),
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
