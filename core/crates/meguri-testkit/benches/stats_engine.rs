//! U98: the Stats graph engine's overhead (`stats/graphs-common` G8, G9;
//! the Statistics plan's section 3: R1290's ≤ 50 ms for all graphs from
//! current columns). A request names no graph (the records are U99's and
//! U101's), so each operation is the request (context, scope, view, the
//! unreadable check) plus one pass over the scope's cards and rows, what
//! every record computes from.
//!
//! - `stats_engine.collection_all`: the Collection scope, all history.
//! - `stats_engine.collection_year`: the Collection scope, 12 months.
//! - `stats_engine.deck_all`: the deck with the most cards, all history.
//! - `stats_engine.search_all`: Search `is:review`, all history.
//! - `stats_engine.snapshot_all`: the Collection scope computed from the
//!   snapshot, as before the caches are current.
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
use meguri_stats::graphs::{COMPONENT, Columns, GraphRequest, GraphResult, History, Scope};
use meguri_storage::raw::SqlValue;
use meguri_storage::space::StatvfsSpace;
use meguri_storage::ReaderRole;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn open_host(path: &std::path::Path, auxiliary: &std::path::Path) -> CoreHost {
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
pub fn build_columns(host: &CoreHost) {
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

/// One request and one pass over its scope; the number of rows seen.
fn run(host: &CoreHost, scope: &Scope, history: History, columns: Columns, cached: bool) -> usize {
    let request = GraphRequest {
        scope: scope.clone(),
        history,
        graphs: Vec::new(),
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns,
    };
    let collection = host.collection().unwrap();
    match host.stats().request(&collection, host.derived(), &request).unwrap() {
        GraphResult::Data(data) => {
            assert_eq!(data.scope.from_caches(), cached, "{scope:?}");
            let cards = data.scope.cards().map(|card| u64::from(card.ivl)).sum::<u64>();
            let mut rows = 0;
            let mut time = 0u64;
            for row in data.scope.rows() {
                rows += 1;
                time += u64::from(row.row.time);
            }
            black_box((cards, time));
            rows
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
    let mut suite = Suite::wall_clock("stats-engine");
    let mut seen = Vec::new();
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
        let cases: [(&str, Scope, History, Columns, bool); 5] = [
            ("stats_engine.collection_all", Scope::Collection, History::AllHistory, Columns::Cached, true),
            ("stats_engine.collection_year", Scope::Collection, History::Last12Months, Columns::Cached, true),
            ("stats_engine.deck_all", Scope::Deck(DeckId(deck)), History::AllHistory, Columns::Cached, true),
            ("stats_engine.search_all", Scope::Search("is:review".into()), History::AllHistory, Columns::Cached, true),
            ("stats_engine.snapshot_all", Scope::Collection, History::AllHistory, Columns::Snapshot, false),
        ];
        for (name, scope, history, columns, cached) in cases {
            // Warm the maps and the statements outside the samples.
            let rows = run(&host, &scope, history, columns, cached);
            seen.push((name, size.clone(), rows));
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| run(&host, &scope, history, columns, cached)))
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
        let rows = seen
            .iter()
            .find(|(name, size, _)| *name == record.name && Some(size.as_str()) == record.fixture.as_deref())
            .map_or(0, |(_, _, rows)| *rows);
        eprintln!(
            "{} fixture {:?}: p95 {:?} ({rows} rows)",
            record.name,
            record.fixture,
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
