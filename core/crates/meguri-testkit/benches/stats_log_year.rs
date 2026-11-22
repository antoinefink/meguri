//! U99: the review-log records (`stats/graphs-revlog` V2, V4 to V6) from
//! current column caches (the Statistics plan's section 3: the review-log
//! tier under 300 ms core for 12 months, under 1 s for all history). Each
//! operation is one request naming Reviews, Retention, Hourly Breakdown and
//! Answer Buttons. The round 11 parts (U288, U291) join these cases when
//! they land.
//!
//! - `stats_log_year.collection`: the Collection scope, 12 months.
//! - `stats_log_year.deck`: the deck with the most cards, 12 months.
//! - `stats_log_all.collection`: the Collection scope, all history.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

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

const GRAPHS: [Graph; 4] = [Graph::Reviews, Graph::Retention, Graph::Hourly, Graph::AnswerButtons];

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

/// One request for the four records; the encoded size of the Reviews one.
fn run(host: &CoreHost, scope: &Scope, history: History) -> usize {
    let request = GraphRequest {
        scope: scope.clone(),
        history,
        graphs: GRAPHS.to_vec(),
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    let collection = host.collection().unwrap();
    match host.stats().request(&collection, host.derived(), &request).unwrap() {
        GraphResult::Data(data) => {
            assert!(data.scope.from_caches(), "{scope:?}");
            let mut out = meguri_stats::graphs::encoding::Encoder::new();
            data.records[0].encode(&mut out);
            out.len()
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
    let mut suite = Suite::wall_clock("stats-log");
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
        let cases: [(&str, Scope, History); 3] = [
            ("stats_log_year.collection", Scope::Collection, History::Last12Months),
            ("stats_log_year.deck", Scope::Deck(DeckId(deck)), History::Last12Months),
            ("stats_log_all.collection", Scope::Collection, History::AllHistory),
        ];
        for (name, scope, history) in cases {
            // Warm the maps and the statements outside the samples.
            let bytes = run(&host, &scope, history);
            seen.push((name, size.clone(), bytes));
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| run(&host, &scope, history)))
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
        let bytes = seen
            .iter()
            .find(|(name, size, _)| *name == record.name && Some(size.as_str()) == record.fixture.as_deref())
            .map_or(0, |(_, _, bytes)| *bytes);
        eprintln!(
            "{} fixture {:?}: p95 {:?} (Reviews record {bytes} bytes)",
            record.name,
            record.fixture,
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
