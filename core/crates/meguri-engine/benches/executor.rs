//! The executor's wall-clock suite (U12 harness, R1864;
//! `specs/engine/op-model.md` O10): the round trip of an empty op (submit,
//! writer, commit, result) and of an answer-sized write phase (one card
//! update and one review-log insert), on generated collections.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (comma-separated, default
//! `10k,150k`). The result goes to `$MEGURI_BENCH_RESULTS/executor.json`.
//! On Apple hosts (`tools/mg bench-mac`, the M3 Max budgets) the bench exits
//! with status 1 after writing its result when an empty op's p50 is 50 µs or
//! more, or an answer's p95 is 5 ms or more.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::{Collection, Engine, EngineConfig, OpSpec, OpenConfig};
use meguri_model::changes::OpSource;
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::{CardId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::rng::SeededRandom;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const EMPTY_OP: &str = "engine.empty_op";
const ANSWER: &str = "engine.answer_write";
const EMPTY_OP_BUDGET: Duration = Duration::from_micros(50);
const ANSWER_BUDGET: Duration = Duration::from_millis(5);

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// An answer's writes: the card's next state and its review-log row.
fn answer(collection: &Collection, card: CardId) {
    collection
        .run(OpSpec::named("answer", OpSource::UserUndoable), move |ctx| {
            let mut stored = ctx.scope().card(card)?.expect("the card exists");
            stored.reps += 1;
            stored.due += 1;
            ctx.scope().put(&stored)?;
            ctx.scope().add_revlog(&RevlogEntry {
                id: RevlogId(0),
                card_id: card,
                usn: Usn(-1),
                button: 3,
                interval: 3,
                last_interval: 1,
                ease_factor: 0,
                answer_millis: 4_000,
                kind: RevlogKind::Review,
            })?;
            Ok(())
        })
        .expect("the answer commits");
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("executor");
    let mut group = criterion.benchmark_group("executor");
    for size in sizes() {
        let generated = match fixtures::profile_collection(&size) {
            Ok(generated) => generated,
            Err(e) => {
                eprintln!("executor bench: the {size} collection: {e}");
                return ExitCode::FAILURE;
            }
        };
        let copy = generated.working_copy().expect("a working copy of the collection");
        let engine = Engine::new(EngineConfig::default()).expect("an engine");
        let config = OpenConfig::new(
            Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(1)),
        );
        let collection = engine.open_raw(&copy.path, config).expect("the collection opens");
        let cards: Vec<i64> = collection
            .reader(ReaderRole::Query)
            .expect("a reader")
            .ids_after(Table::Cards, None, 10_000)
            .expect("card ids");
        // Warm: the writer's statements prepared, the pages in the cache.
        for id in cards.iter().take(200) {
            answer(&collection, CardId(*id));
        }

        let mut empty = Measurement::start(EMPTY_OP, Some(&size));
        group.bench_function(format!("empty_op/{size}"), |b| {
            b.iter_custom(|iters| {
                empty.run(iters, |_| {
                    collection
                        .run(OpSpec::named("empty", OpSource::UserUndoable), |_| Ok(()))
                        .expect("the empty op runs")
                })
            })
        });
        suite.push(empty.finish());

        let mut answers = Measurement::start(ANSWER, Some(&size));
        let mut next = 0usize;
        group.bench_function(format!("answer/{size}"), |b| {
            b.iter_custom(|iters| {
                answers.run(iters, |_| {
                    next = (next + 1) % cards.len();
                    answer(&collection, CardId(cards[next]));
                })
            })
        });
        suite.push(answers.finish());
        engine.close().expect("the collection closes");
    }
    group.finish();
    criterion.final_summary();

    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    match suite.write(&report::results_dir(target)) {
        Ok(path) => eprintln!("executor bench: wrote {}", path.display()),
        Err(e) => {
            eprintln!("executor bench: writing the result: {e}");
            return ExitCode::FAILURE;
        }
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let (p50, p95) = (record.percentile(Percentile::P50), record.percentile(Percentile::P95));
        eprintln!(
            "executor bench: {} on {}: p50 {p50:?}, p95 {p95:?}",
            record.name,
            record.fixture.as_deref().unwrap_or("-")
        );
        let over = match record.name.as_str() {
            EMPTY_OP => p50 >= EMPTY_OP_BUDGET,
            ANSWER => p95 >= ANSWER_BUDGET,
            _ => false,
        };
        if over && cfg!(target_vendor = "apple") {
            eprintln!(
                "executor bench: {} is over its budget (specs/engine/op-model.md O10)",
                record.name
            );
            status = ExitCode::FAILURE;
        }
    }
    status
}
