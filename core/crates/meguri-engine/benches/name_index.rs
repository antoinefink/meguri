//! The name index wall-clock suite (U12 harness, R1864;
//! `specs/search/editing-services.md` D6.7): `engine.name_index_build`
//! builds the whole index from a snapshot of a generated collection, as the
//! open step and a replacement's reopen do. Suggestions then read the index
//! and never the collection; their own budget (50 candidates in under 4 ms
//! with 10,000 tags) belongs to the suggestion unit's bench.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (comma-separated, default
//! `10k,150k`). The result goes to `$MEGURI_BENCH_RESULTS/name_index.json`.

use std::process::ExitCode;
use std::sync::Arc;

use criterion::Criterion;
use meguri_engine::browse::NameIndexProvider;
use meguri_engine::{Engine, EngineConfig, OpenConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const BUILD: &str = "engine.name_index_build";

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("name_index");
    let mut group = criterion.benchmark_group("name_index");
    group.sample_size(20);
    for size in sizes() {
        let generated = match fixtures::profile_collection(&size) {
            Ok(generated) => generated,
            Err(e) => {
                eprintln!("name_index bench: the {size} collection: {e}");
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
        let provider = NameIndexProvider::new();
        // Warm: the reader's statements prepared, the pages in the cache.
        provider.rebuild(&collection).expect("the warm-up build");
        let index = provider.index();
        eprintln!(
            "name_index bench: {size}: {} decks, {} tags, {} note types",
            index.decks().len(),
            index.tags().len(),
            index.notetypes().len()
        );

        let mut build = Measurement::start(BUILD, Some(&size));
        group.bench_function(format!("build/{size}"), |b| {
            b.iter_custom(|iters| build.run(iters, |_| provider.rebuild(&collection).expect("the build")))
        });
        suite.push(build.finish());
        engine.close().expect("the collection closes");
    }
    group.finish();
    criterion.final_summary();

    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    match suite.write(&report::results_dir(target)) {
        Ok(path) => eprintln!("name_index bench: wrote {}", path.display()),
        Err(e) => {
            eprintln!("name_index bench: writing the result: {e}");
            return ExitCode::FAILURE;
        }
    }
    for record in suite.records() {
        eprintln!(
            "name_index bench: {} on {}: p50 {:?}, p95 {:?}",
            record.name,
            record.fixture.as_deref().unwrap_or("-"),
            record.percentile(Percentile::P50),
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
