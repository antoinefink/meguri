//! What U289's three `stats_history` suites share: the host, the build and
//! one read of Cards Over Time and Started.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use meguri_engine::{CancelToken, Readiness};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_stats::graphs::{History, Scope};
use meguri_stats::history::{self, HISTORY_COMPONENT, HistoryData, HistoryRequest, HistoryResult};
use meguri_storage::ReaderRole;
use meguri_storage::raw::SqlValue;
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::generate::DEFAULT_NOW_MS;

pub fn sizes() -> Vec<String> {
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

/// Waits until the component is ready (a build ends there).
pub fn wait_ready(host: &CoreHost) {
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(900);
    loop {
        host.derived().flush().unwrap();
        if host.derived().readiness(HISTORY_COMPONENT).map(|r| r.readiness) == Some(Readiness::Ready) {
            return;
        }
        assert!(meguri_engine::monotonic::now() < deadline, "the stats history never became ready");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// One read of H2's parts; the component must serve it.
pub fn read(host: &CoreHost, scope: &Scope, history: History) -> Box<HistoryData> {
    let request = HistoryRequest {
        scope: scope.clone(),
        history,
        cancel: CancelToken::new(),
    };
    let collection = host.collection().unwrap();
    match history::card_states(host.stats(), &collection, host.derived(), &request, &mut |_| {}).unwrap() {
        HistoryResult::Data(data) => {
            assert!(data.from_component, "{scope:?}");
            data
        }
        other => panic!("{scope:?}: {other:?}"),
    }
}

/// The value of one SQL integer the fixture holds.
pub fn integer(host: &CoreHost, sql: &str) -> i64 {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    match snapshot.execute_raw(sql, &[]).unwrap().rows[0][0] {
        SqlValue::Integer(value) => value,
        ref other => panic!("{other:?}"),
    }
}

/// The deck with the most cards.
pub fn largest_deck(host: &CoreHost) -> i64 {
    integer(host, "SELECT did FROM cards GROUP BY did ORDER BY count(*) DESC, did LIMIT 1")
}

/// The derived store's size on disk, its log included: with the column
/// caches held (never released in these suites), the component's.
pub fn store_bytes(path: &std::path::Path) -> u64 {
    let store = path.with_file_name("meguri-derived.db");
    ["", "-wal"]
        .iter()
        .filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", store.display())).ok())
        .map(|meta| meta.len())
        .sum()
}

/// Every collection page these suites print, after the records.
pub fn print(suite: &meguri_testkit::bench::report::Suite) {
    for record in suite.records() {
        eprintln!(
            "{} fixture {:?}: p50 {:?} p95 {:?}",
            record.name,
            record.fixture,
            record.percentile(meguri_testkit::bench::timing::Percentile::P50),
            record.percentile(meguri_testkit::bench::timing::Percentile::P95)
        );
    }
}
