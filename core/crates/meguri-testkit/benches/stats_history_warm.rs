//! U289: Cards Over Time and Started with `stats_history` current
//! (`stats/history-reads` H19: under 50 ms, sums over deck keys), from the
//! request to its result, G1's context included.
//!
//! - `stats_history_warm.collection_all`, `.collection_year`: All Decks,
//!   all history and the last 12 months.
//! - `stats_history_warm.deck_all`, `.deck_year`: the deck with the most
//!   cards.
//! - `stats_history_warm.knowledge_collection_all` and the three others:
//!   Knowledge Over Time (U290) for the same scopes, today's point
//!   replayed from every card's record.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

#[path = "support/stats_history.rs"]
mod support;

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Duration;

use criterion::Criterion;
use meguri_model::ids::DeckId;
use meguri_stats::graphs::{History, Scope};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-history-warm");
    for size in support::sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = support::open_host(&copy.path, auxiliary.path());
        support::read(&host, &Scope::Collection, History::AllHistory);
        let deck = Scope::Deck(DeckId(support::largest_deck(&host)));
        let cases = [
            ("stats_history_warm.collection_all", Scope::Collection, History::AllHistory),
            ("stats_history_warm.collection_year", Scope::Collection, History::Last12Months),
            ("stats_history_warm.deck_all", deck.clone(), History::AllHistory),
            ("stats_history_warm.deck_year", deck, History::Last12Months),
        ];
        for (name, scope, history) in cases {
            let first = support::read(&host, &scope, history);
            eprintln!("{name}/{size}: {} days with a change", first.card_states.deltas.len());
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(support::read(&host, &scope, history))))
            });
            if measurement.samples().count() > 0 {
                suite.push(measurement.finish());
            }
        }
        let deck = Scope::Deck(DeckId(support::largest_deck(&host)));
        let cases = [
            ("stats_history_warm.knowledge_collection_all", Scope::Collection, History::AllHistory),
            ("stats_history_warm.knowledge_collection_year", Scope::Collection, History::Last12Months),
            ("stats_history_warm.knowledge_deck_all", deck.clone(), History::AllHistory),
            ("stats_history_warm.knowledge_deck_year", deck, History::Last12Months),
        ];
        for (name, scope, history) in cases {
            let first = support::read_knowledge(&host, &scope, history);
            eprintln!(
                "{name}/{size}: {} samples, today {:?}",
                first.knowledge.samples.len(),
                first.knowledge.samples.last()
            );
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(support::read_knowledge(&host, &scope, history))))
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
