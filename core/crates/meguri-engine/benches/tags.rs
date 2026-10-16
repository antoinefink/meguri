//! The tag wall-clock suite (U12 harness, R1864, R1831;
//! `specs/tags/tags.md` T7, T10): bulk tag operations on 10,000 notes of a
//! generated collection, each one op from the call to its result, planning
//! included. `engine.tags_bulk_rename` renames a parent tag that the notes
//! carry with a child, back and forth; `engine.tags_bulk_add` adds a tag to
//! the notes and the next sample removes it again (`tags_bulk_remove`).
//! A collection with fewer notes uses all of them.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (comma-separated, default
//! `10k,150k`). The result goes to `$MEGURI_BENCH_RESULTS/tags.json`. On
//! Apple hosts (`tools/mg bench-mac`) the bench exits with status 1 after
//! writing its result when a p95 is 2 s or more: R1831's budget for a bulk
//! operation on 10,000 notes.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::tags::{self, RunOptions};
use meguri_engine::{Engine, EngineConfig, OpSpec, OpenConfig};
use meguri_model::changes::OpSource;
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::NoteId;
use meguri_model::rng::SeededRandom;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const RENAME: &str = "engine.tags_bulk_rename";
const ADD_REMOVE: &str = "engine.tags_bulk_add_remove";
const NOTES: usize = 10_000;
const BUDGET: Duration = Duration::from_secs(2);

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Up to [`NOTES`] notes spread over the whole collection.
fn spread_notes(all: &[i64]) -> Vec<NoteId> {
    let step = (all.len() / NOTES).max(1);
    all.iter().step_by(step).take(NOTES).map(|id| NoteId(*id)).collect()
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("tags");
    let mut group = criterion.benchmark_group("tags");
    // Each sample is a whole bulk operation (hundreds of milliseconds).
    group.sample_size(10);
    let options = RunOptions::default();
    for size in sizes() {
        let generated = match fixtures::profile_collection(&size) {
            Ok(generated) => generated,
            Err(e) => {
                eprintln!("tags bench: the {size} collection: {e}");
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
        let all = collection
            .reader(ReaderRole::Query)
            .expect("a reader")
            .ids_after(Table::Notes, None, usize::MAX)
            .expect("note ids");
        let notes = spread_notes(&all);
        eprintln!("tags bench: {size}: {} of {} notes", notes.len(), all.len());
        {
            let notes = notes.clone();
            collection
                .run(
                    OpSpec::named("bench_setup", OpSource::UserNonUndoable).skip_undo(),
                    move |ctx| tags::add_in(ctx.scope(), &notes, "U30bench::child"),
                )
                .expect("the setup tags the notes");
        }
        // Warm: the writer's statements prepared, the pages in the cache.
        tags::rename(&collection, "U30bench", "U30bench2", &options).expect("the warm-up rename");
        tags::rename(&collection, "U30bench2", "U30bench", &options).expect("the warm-up rename back");

        let mut rename = Measurement::start(RENAME, Some(&size));
        let mut renamed = false;
        group.bench_function(format!("bulk_rename/{size}"), |b| {
            b.iter_custom(|iters| {
                rename.run(iters, |_| {
                    let (from, to) = if renamed {
                        ("U30bench2", "U30bench")
                    } else {
                        ("U30bench", "U30bench2")
                    };
                    let done = tags::rename(&collection, from, to, &options).expect("the rename commits");
                    assert_eq!(done.value, notes.len());
                    renamed = !renamed;
                })
            })
        });
        suite.push(rename.finish());

        let mut add_remove = Measurement::start(ADD_REMOVE, Some(&size));
        let mut added = false;
        group.bench_function(format!("bulk_add_remove/{size}"), |b| {
            b.iter_custom(|iters| {
                add_remove.run(iters, |_| {
                    let done = if added {
                        tags::remove_from_notes(&collection, &notes, "u30extra", &options)
                    } else {
                        tags::add_to_notes(&collection, &notes, "U30extra", &options)
                    }
                    .expect("the bulk op commits");
                    assert_eq!(done.value, notes.len());
                    added = !added;
                })
            })
        });
        suite.push(add_remove.finish());
        engine.close().expect("the collection closes");
    }
    group.finish();
    criterion.final_summary();

    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    match suite.write(&report::results_dir(target)) {
        Ok(path) => eprintln!("tags bench: wrote {}", path.display()),
        Err(e) => {
            eprintln!("tags bench: writing the result: {e}");
            return ExitCode::FAILURE;
        }
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let (p50, p95) = (record.percentile(Percentile::P50), record.percentile(Percentile::P95));
        eprintln!(
            "tags bench: {} on {}: p50 {p50:?}, p95 {p95:?}",
            record.name,
            record.fixture.as_deref().unwrap_or("-")
        );
        if p95 >= BUDGET && cfg!(target_vendor = "apple") {
            eprintln!("tags bench: {} is over its {BUDGET:?} p95 budget", record.name);
            status = ExitCode::FAILURE;
        }
    }
    status
}
