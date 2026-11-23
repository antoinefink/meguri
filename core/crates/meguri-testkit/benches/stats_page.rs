//! U293: the Statistics page's one request per refresh (`stats/history-reads`
//! H15, H19; `stats/statistics-page` S19), every part named, from the
//! request to the messages the page waits on, G1's context included.
//!
//! With the column caches and `stats_history` current:
//!
//! - `stats_page.present_year`: to the last present-tier part (R2067: well
//!   under 100 ms), All Decks, the last 12 months.
//! - `stats_page.log_year`, `.log_all`: to the last review-log part, the
//!   history parts included (R1838: under 300 ms; R2068: All under 1 s).
//! - `stats_page.deck_year`: the same for the deck with the most cards (a
//!   deck change, R1838).
//!
//! Before them, once and printed only: the first request after the open,
//! with nothing built (the scope read from the snapshot and the history
//! build waited for), to each tier's last part (R2077's core side).
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

#[path = "support/stats_history.rs"]
mod support;

use std::process::ExitCode;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::{CancelToken, Readiness};
use meguri_host::CoreHost;
use meguri_model::ids::DeckId;
use meguri_stats::graphs::{COMPONENT, Columns, History, Scope};
use meguri_stats::page::{self, PageEnd, PageMessage, PageRequest, Part, PartOutcome, Tier};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// When the last part of each tier came, from the start of the request.
#[derive(Debug, Default)]
struct Timings {
    present: Duration,
    log: Duration,
    whole: Duration,
}

fn run(host: &CoreHost, scope: &Scope, history: History) -> Timings {
    let request = PageRequest {
        scope: scope.clone(),
        history,
        parts: Part::all().collect(),
        refresh: 1,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    let collection = host.collection().unwrap();
    let start = meguri_engine::monotonic::now();
    let mut timings = Timings::default();
    let end = page::request(host.stats(), &collection, host.derived(), &request, &mut |message| {
        if let PageMessage::Part(part) = message {
            if let PartOutcome::Failed(failure) = &part.outcome {
                assert!(
                    matches!(failure, page::PartFailure::NotBuilt),
                    "{:?}: {failure:?}",
                    part.part
                );
            }
            let at = meguri_engine::monotonic::now().saturating_sub(start);
            match part.part.tier() {
                Tier::Present => timings.present = at,
                Tier::ReviewLog => timings.log = at,
                Tier::Knowledge => {}
            }
        }
    })
    .unwrap();
    assert_eq!(end, PageEnd::Complete, "{scope:?}");
    timings.whole = meguri_engine::monotonic::now().saturating_sub(start);
    timings
}

/// Builds the column caches and waits until they are ready.
fn build_columns(host: &CoreHost) {
    host.derived().request_refresh(COMPONENT);
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(900);
    while host.derived().readiness(COMPONENT).map(|r| r.readiness) != Some(Readiness::Ready) {
        assert!(meguri_engine::monotonic::now() < deadline, "the stats columns never became ready");
        std::thread::sleep(Duration::from_millis(5));
    }
    host.derived().flush().unwrap();
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-page");
    for size in support::sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = support::open_host(&copy.path, auxiliary.path());
        // Nothing built: the open defers both components.
        let cold = run(&host, &Scope::Collection, History::AllHistory);
        eprintln!("stats_page.cold_all/{size}: {cold:?}");
        build_columns(&host);
        support::wait_ready(&host);
        let deck = Scope::Deck(DeckId(support::largest_deck(&host)));
        type Pick = fn(&Timings) -> Duration;
        let cases: [(&str, Scope, History, Pick); 4] = [
            ("stats_page.present_year", Scope::Collection, History::Last12Months, |t| t.present),
            ("stats_page.log_year", Scope::Collection, History::Last12Months, |t| t.log),
            ("stats_page.log_all", Scope::Collection, History::AllHistory, |t| t.log),
            ("stats_page.deck_year", deck, History::Last12Months, |t| t.log),
        ];
        for (name, scope, history, pick) in cases {
            eprintln!("{name}/{size}: first {:?}", run(&host, &scope, history));
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run_timed(n, |_| pick(&run(&host, &scope, history))))
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
    support::print(&suite);
    ExitCode::SUCCESS
}
