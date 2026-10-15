//! The sample wall-clock suite (U12, R1864, R1823): a warm open of a
//! generated collection through the open gate, with nothing to upgrade.
//! Area units copy its shape: a shared collection, a working copy, a warm-up
//! outside the measurement, then criterion's `iter_custom` over
//! `Measurement::run`, which times every operation on its own.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (comma-separated, default
//! `10k,150k`). The result goes to `$MEGURI_BENCH_RESULTS/open.json`. At
//! 150k the open must take at most 150 ms at p95 (R1823; the leg U22 could
//! not run before U11's generator and this harness existed): above it the
//! bench exits with status 1 after writing its result.

use std::process::ExitCode;
use std::time::Duration;

use criterion::Criterion;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// The interval and benchmark name of a collection open.
const NAME: &str = "collection.open";

/// R1823: a warm open of the 150k collection.
const BUDGET_SIZE: &str = "150k";
const BUDGET: Duration = Duration::from_millis(150);

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
    let mut suite = Suite::wall_clock("open");
    let mut group = criterion.benchmark_group("open");
    for size in sizes() {
        let collection = match fixtures::profile_collection(&size) {
            Ok(collection) => collection,
            Err(e) => {
                eprintln!("open bench: the {size} collection: {e}");
                return ExitCode::FAILURE;
            }
        };
        let copy = collection.working_copy().expect("a working copy of the collection");
        // Warm: the file's pages in the OS cache, the gate's first open done.
        fixtures::open(&copy.path)
            .expect("the collection opens")
            .close()
            .expect("it closes");

        let mut measurement = Measurement::start(NAME, Some(&size));
        group.bench_function(&size, |b| {
            b.iter_custom(|iters| measurement.run(iters, |_| fixtures::open(&copy.path).expect("the collection opens")))
        });
        suite.push(measurement.finish());
    }
    group.finish();
    criterion.final_summary();

    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    match suite.write(&report::results_dir(target)) {
        Ok(path) => eprintln!("open bench: wrote {}", path.display()),
        Err(e) => {
            eprintln!("open bench: writing the result: {e}");
            return ExitCode::FAILURE;
        }
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let p95 = record.percentile(Percentile::P95);
        if record.fixture.as_deref() == Some(BUDGET_SIZE) && p95 > BUDGET {
            eprintln!("open bench: R1823: a warm open of {BUDGET_SIZE} took {p95:?} at p95, over {BUDGET:?}");
            status = ExitCode::FAILURE;
        }
    }
    status
}
