//! U98: the `stats_columns` component (`stats/graphs-common` G9.2, G9.3,
//! G9.5): its build off the launch path, and the reload after a memory
//! warning.
//!
//! - `stats_columns_build.build`: an invalidation's rebuild (every card and
//!   every review-log row read, both column files written and checked),
//!   from the request to `Ready`.
//! - `stats_columns_build.release_reload`: a memory warning with a
//!   one-answer overlay (merged into new files), then the first request,
//!   which maps and checks the files again.
//!
//! Peak memory is the suite's. Sizes come from `$MEGURI_BENCH_SIZES`
//! (default `10k,150k`), made first by `tools/mg bench`.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::Criterion;
use meguri_engine::derived_host::Invalidation;
use meguri_engine::{CancelToken, OpSpec, Readiness};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::changes::OpSource;
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::{CardId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::rng::SeededRandom;
use meguri_model::usn::Usn;
use meguri_stats::graphs::{COMPONENT, Columns, GraphRequest, GraphResult, History, Scope};
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

fn wait_ready(host: &CoreHost) {
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        host.derived().flush().unwrap();
        if host.derived().readiness(COMPONENT).map(|r| r.readiness) == Some(Readiness::Ready) {
            return;
        }
        assert!(Instant::now() < deadline, "the stats columns never became ready");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn request(host: &CoreHost) {
    let request = GraphRequest {
        scope: Scope::Collection,
        history: History::AllHistory,
        graphs: Vec::new(),
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    let collection = host.collection().unwrap();
    match host.stats().request(&collection, host.derived(), &request).unwrap() {
        GraphResult::Data(data) => assert!(data.scope.from_caches()),
        other => panic!("{other:?}"),
    }
}

fn some_card(host: &CoreHost) -> i64 {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    match snapshot.execute_raw("SELECT min(id) FROM cards", &[]).unwrap().rows[0][0] {
        SqlValue::Integer(id) => id,
        ref other => panic!("{other:?}"),
    }
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(1))
        .measurement_time(Duration::from_secs(20))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-columns-build");
    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = open_host(&copy.path, auxiliary.path());
        host.derived().request_refresh(COMPONENT);
        wait_ready(&host);
        let name = "stats_columns_build.build";
        let mut measurement = Measurement::start(name, Some(&size));
        criterion.bench_function(&format!("{name}/{size}"), |b| {
            b.iter_custom(|n| {
                measurement.run(n, |_| {
                    host.derived().invalidate(Invalidation::CheckDatabase);
                    wait_ready(&host);
                })
            })
        });
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
        let card = some_card(&host);
        let mut next = 0i64;
        let name = "stats_columns_build.release_reload";
        let mut measurement = Measurement::start(name, Some(&size));
        criterion.bench_function(&format!("{name}/{size}"), |b| {
            b.iter_custom(|n| {
                measurement.run(n, |_| {
                    next += 1;
                    let entry = RevlogEntry {
                        id: RevlogId(DEFAULT_NOW_MS + next),
                        card_id: CardId(card),
                        usn: Usn(-1),
                        button: 3,
                        interval: 3,
                        last_interval: 1,
                        ease_factor: 2500,
                        answer_millis: 5_000,
                        kind: RevlogKind::Review,
                    };
                    host.collection()
                        .unwrap()
                        .run(OpSpec::named("answer", OpSource::UserUndoable), move |ctx| {
                            ctx.scope().insert_revlog(&entry)?;
                            Ok(())
                        })
                        .unwrap();
                    host.derived().flush().unwrap();
                    meguri_stats::graphs::release_stats_caches(host.stats(), host.derived());
                    request(&host);
                })
            })
        });
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
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
