//! U290: Knowledge Over Time's cold build (`stats/history-reads` H19: a few
//! seconds on an iPhone after the H2 pass, target 4 s, parallel across
//! cores) and H18's read without the component.
//!
//! - `stats_knowledge_build.build`: an invalidation's rebuild of
//!   `stats_history`, which replays every card for H2 and, with FSRS, for
//!   H3 at every kept sample, from the request to `Ready`.
//! - `stats_knowledge_build.cold_read`: Knowledge Over Time for the
//!   Collection scope and all history replayed from the snapshot, nothing
//!   kept (H18).
//!
//! Peak memory is the suite's; the store's size on disk is printed. Sizes
//! come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

#[path = "support/stats_history.rs"]
mod support;

use std::process::ExitCode;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::CancelToken;
use meguri_engine::derived_host::Invalidation;
use meguri_stats::graphs::{History, Scope};
use meguri_stats::history::{self, KnowledgeRequest, KnowledgeResult};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(1))
        .measurement_time(Duration::from_secs(30))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-knowledge-build");
    eprintln!(
        "stats_knowledge_build: {} cores",
        std::thread::available_parallelism().map_or(1, |n| n.get())
    );
    for size in support::sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = support::open_host(&copy.path, auxiliary.path());
        // The first read releases and waits for the first build.
        let first = support::read_knowledge(&host, &Scope::Collection, History::AllHistory);
        eprintln!(
            "stats_knowledge fixture {size}: {} cards, {} review-log rows, {} samples, store {} bytes",
            support::integer(&host, "SELECT count(*) FROM cards"),
            support::integer(&host, "SELECT count(*) FROM revlog"),
            first.knowledge.samples.len(),
            support::store_bytes(&copy.path)
        );
        let name = "stats_knowledge_build.build";
        let mut measurement = Measurement::start(name, Some(&size));
        criterion.bench_function(&format!("{name}/{size}"), |b| {
            b.iter_custom(|n| {
                measurement.run(n, |_| {
                    host.derived().invalidate(Invalidation::CheckDatabase);
                    support::wait_ready(&host);
                })
            })
        });
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
        let name = "stats_knowledge_build.cold_read";
        let request = KnowledgeRequest {
            scope: Scope::Collection,
            history: History::AllHistory,
            oldest_day: None,
            cancel: CancelToken::new(),
        };
        let collection = host.collection().unwrap();
        let mut measurement = Measurement::start(name, Some(&size));
        criterion.bench_function(&format!("{name}/{size}"), |b| {
            b.iter_custom(|n| {
                measurement.run(n, |_| {
                    let context = history::page_context(&collection, host.derived()).unwrap();
                    match history::knowledge_cold(&collection, &context, &request).unwrap() {
                        KnowledgeResult::Data(data) => assert!(!data.from_component),
                        other => panic!("{other:?}"),
                    }
                })
            })
        });
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
        eprintln!("stats_knowledge fixture {size}: store {} bytes after the builds", support::store_bytes(&copy.path));
        drop(collection);
        host.close().unwrap();
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    support::print(&suite);
    ExitCode::SUCCESS
}
