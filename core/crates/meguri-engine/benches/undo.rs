//! The undo wall-clock suite (U12 harness, R1864; `specs/engine/undo.md`
//! M12): the round trip of undoing an answer (submit, writer, the step's
//! row check, restores, commit, result) on generated collections. Undo and
//! redo of the same answer alternate, so every call is one restore of an
//! answer's rows (the card, its review-log row and `col`), as R1811 times it.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (comma-separated, default
//! `10k,150k`). The result goes to `$MEGURI_BENCH_RESULTS/undo.json`. On
//! Apple hosts (`tools/mg bench-mac`) the bench exits with status 1 after
//! writing its result when the p95 is 5 ms or more, a tenth of R1811's
//! 50 ms for the whole undo.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::{Collection, Engine, EngineConfig, OpSpec, OpenConfig, StepKind, UndoScope};
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

const UNDO_ANSWER: &str = "engine.undo_answer";
const BUDGET: Duration = Duration::from_millis(5);

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// An answer recorded as an undo step: the card's next state and its
/// review-log row.
fn answer(collection: &Collection, card: CardId) {
    collection
        .run(
            OpSpec::named("answer_card", OpSource::UserUndoable).undo_step("Answer Card", StepKind::Answer),
            move |ctx| {
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
            },
        )
        .expect("the answer commits");
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("undo");
    let mut group = criterion.benchmark_group("undo");
    for size in sizes() {
        let generated = match fixtures::profile_collection(&size) {
            Ok(generated) => generated,
            Err(e) => {
                eprintln!("undo bench: the {size} collection: {e}");
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
            collection.undo(UndoScope::Global).expect("the warm-up undo");
            collection.redo(UndoScope::Global).expect("the warm-up redo");
        }
        answer(&collection, CardId(cards[cards.len() / 2]));

        let mut undo = Measurement::start(UNDO_ANSWER, Some(&size));
        let mut undone = false;
        group.bench_function(format!("undo_answer/{size}"), |b| {
            b.iter_custom(|iters| {
                undo.run(iters, |_| {
                    if undone {
                        collection.redo(UndoScope::Global).expect("the redo commits");
                    } else {
                        collection.undo(UndoScope::Global).expect("the undo commits");
                    }
                    undone = !undone;
                })
            })
        });
        suite.push(undo.finish());
        engine.close().expect("the collection closes");
    }
    group.finish();
    criterion.final_summary();

    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    match suite.write(&report::results_dir(target)) {
        Ok(path) => eprintln!("undo bench: wrote {}", path.display()),
        Err(e) => {
            eprintln!("undo bench: writing the result: {e}");
            return ExitCode::FAILURE;
        }
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let (p50, p95) = (record.percentile(Percentile::P50), record.percentile(Percentile::P95));
        eprintln!(
            "undo bench: {} on {}: p50 {p50:?}, p95 {p95:?}",
            record.name,
            record.fixture.as_deref().unwrap_or("-")
        );
        if p95 >= BUDGET && cfg!(target_vendor = "apple") {
            eprintln!("undo bench: {} is over its {BUDGET:?} p95 budget", record.name);
            status = ExitCode::FAILURE;
        }
    }
    status
}
