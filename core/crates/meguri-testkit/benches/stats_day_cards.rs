//! U288: the cards behind one day for Show in Browse (`stats/history-reads`
//! H13, H19: under 50 ms, R2073), with the column caches current, on the
//! context of a page result.
//!
//! - `stats_day_cards.collection`: All Decks, yesterday's reviewed cards.
//! - `stats_day_cards.deck`: the deck with the most cards, the same day.
//! - `stats_day_cards.today_answered`: All Decks, today's answered cards
//!   (day 0 has no upper end: every later row is on it).
//!
//! Before them, once and printed only: the same read from the snapshot
//! (the caches not built), which reads every card.
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
use meguri_stats::graphs::{COMPONENT, Columns, Context, Scope};
use meguri_stats::parts::day_cards::{DayCards, DayCardsRequest, DayKind, stats_day_cards};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn read(host: &CoreHost, context: &Context, scope: &Scope, day: i32, kind: DayKind, columns: Columns) -> usize {
    let request = DayCardsRequest {
        scope: scope.clone(),
        context: context.clone(),
        day,
        kind,
        cancel: CancelToken::new(),
        columns,
    };
    let collection = host.collection().unwrap();
    match stats_day_cards(host.stats(), &collection, host.derived(), &request).unwrap() {
        DayCards::Cards(cards) => cards.len(),
        DayCards::NoCards => 0,
        other => panic!("{scope:?} {day} {kind:?}: {other:?}"),
    }
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
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-day-cards");
    for size in support::sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = support::open_host(&copy.path, auxiliary.path());
        let collection = host.collection().unwrap();
        let context = meguri_stats::history::page_context(&collection, host.derived()).unwrap();
        let start = meguri_engine::monotonic::now();
        let cold = read(&host, &context, &Scope::Collection, -1, DayKind::Reviewed, Columns::Snapshot);
        eprintln!(
            "stats_day_cards.snapshot/{size}: {cold} cards in {:?}",
            meguri_engine::monotonic::now().saturating_sub(start)
        );
        build_columns(&host);
        let deck = Scope::Deck(DeckId(support::largest_deck(&host)));
        let cases: [(&str, Scope, i32, DayKind); 3] = [
            ("stats_day_cards.collection", Scope::Collection, -1, DayKind::Reviewed),
            ("stats_day_cards.deck", deck, -1, DayKind::Reviewed),
            ("stats_day_cards.today_answered", Scope::Collection, 0, DayKind::Answered),
        ];
        for (name, scope, day, kind) in cases {
            eprintln!("{name}/{size}: {} cards", read(&host, &context, &scope, day, kind, Columns::Cached));
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run(n, |_| {
                        read(&host, &context, &scope, day, kind, Columns::Cached);
                    })
                })
            });
            if measurement.samples().count() > 0 {
                suite.push(measurement.finish());
            }
        }
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
