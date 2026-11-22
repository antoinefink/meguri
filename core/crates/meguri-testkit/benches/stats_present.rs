//! U101: the card records of the Statistics page's present tier (the
//! Statistics plan's section 3: "well under 100 ms"; `stats/graphs-cards`).
//! Each operation is one request from current column caches naming the
//! seven card graphs (Future Due, Card Counts, Intervals, Stability,
//! Difficulty, Retrievability, Added), so it counts the request, the scope
//! and every record.
//!
//! - `stats_present.collection`: the Collection scope.
//! - `stats_present.deck`: the deck with the most cards.
//! - `stats_present.retrievability`: the Collection scope, Retrievability
//!   alone (the one record with an FSRS call per card, never cached).
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::{CancelToken, Readiness};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::DeckId;
use meguri_model::rng::SeededRandom;
use meguri_stats::graphs::{COMPONENT, Columns, Graph, GraphRequest, GraphResult, History, Scope};
use meguri_storage::ReaderRole;
use meguri_storage::raw::SqlValue;
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const CARD_GRAPHS: [Graph; 7] = [
    Graph::FutureDue,
    Graph::CardCounts,
    Graph::Intervals,
    Graph::Stability,
    Graph::Difficulty,
    Graph::Retrievability,
    Graph::Added,
];

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn open_host(path: &std::path::Path, auxiliary: &std::path::Path) -> CoreHost {
    std::fs::create_dir_all(auxiliary.join("backups")).unwrap();
    CoreHost::open(HostConfig::new(
        path.parent().unwrap().into(),
        auxiliary.join("backups"),
        auxiliary.join("app-group/widget.snapshot"),
        Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(1)),
        Arc::new(StatvfsSpace),
    ))
    .unwrap()
}

/// Builds the column caches and waits until they are ready.
fn build_columns(host: &CoreHost) {
    host.derived().request_refresh(COMPONENT);
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(600);
    while host.derived().readiness(COMPONENT).map(|r| r.readiness) != Some(Readiness::Ready) {
        assert!(meguri_engine::monotonic::now() < deadline, "the stats columns never became ready");
        std::thread::sleep(Duration::from_millis(5));
    }
    host.derived().flush().unwrap();
}

fn largest_deck(host: &CoreHost) -> i64 {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let rows = snapshot
        .execute_raw("SELECT did, count(*) FROM cards GROUP BY did ORDER BY 2 DESC, 1 LIMIT 1", &[])
        .unwrap();
    match rows.rows[0][0] {
        SqlValue::Integer(did) => did,
        ref other => panic!("{other:?}"),
    }
}

/// One request from the caches; the number of records.
fn run(host: &CoreHost, scope: &Scope, graphs: &[Graph]) -> usize {
    let request = GraphRequest {
        scope: scope.clone(),
        history: History::Last12Months,
        graphs: graphs.to_vec(),
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    let collection = host.collection().unwrap();
    match host.stats().request(&collection, host.derived(), &request).unwrap() {
        GraphResult::Data(data) => {
            assert!(data.scope.from_caches(), "{scope:?}");
            black_box(&data.records);
            data.records.len()
        }
        other => panic!("{scope:?}: {other:?}"),
    }
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-present");
    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = open_host(&copy.path, auxiliary.path());
        build_columns(&host);
        let deck = largest_deck(&host);
        let cases: [(&str, Scope, &[Graph]); 3] = [
            ("stats_present.collection", Scope::Collection, &CARD_GRAPHS),
            ("stats_present.deck", Scope::Deck(DeckId(deck)), &CARD_GRAPHS),
            ("stats_present.retrievability", Scope::Collection, &[Graph::Retrievability]),
        ];
        for (name, scope, graphs) in cases {
            // Warm the maps and the statements outside the samples.
            assert_eq!(run(&host, &scope, graphs), graphs.len());
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| run(&host, &scope, graphs)))
            });
            if measurement.samples().count() > 0 {
                suite.push(measurement.finish());
            }
        }
        host.close().unwrap();
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    for record in suite.records() {
        eprintln!(
            "{} fixture {:?}: p95 {:?}",
            record.name,
            record.fixture,
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
