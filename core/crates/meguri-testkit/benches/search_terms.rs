//! U82: one search statement per term family of `specs/search/names-and-ids`,
//! `state-and-flags`, `properties` and `history`, run whole on a working
//! copy of a generated collection (plan section 3: every unit that adds a
//! query adds its bench row). Each sample builds the plan and reads every
//! matching card id, unsorted, on a query reader with the product's SQL
//! functions; the first-page and streaming budgets (R1825, R1826) are U90's
//! once sessions exist (U85).
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`. The result goes to `$MEGURI_BENCH_RESULTS`.

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Duration;

use criterion::Criterion;
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::DeckId;
use meguri_model::rng::SeededRandom;
use meguri_search::{FoldMode, Param, Plan, SearchContext, SearchMode, parse};
use meguri_storage::raw::SqlValue;
use meguri_storage::{Initializers, OpenOptions, ReaderRole, open_collection};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// The benchmark name of each search: `search.terms.<family>.<term>`.
const SEARCHES: [(&str, &str); 9] = [
    ("names.tag", "tag:a*"),
    ("names.tag_regex", "tag:re:^a"),
    ("names.deck", "deck:a*"),
    ("names.deck_current", "deck:current"),
    ("names.preset", "preset:Default"),
    ("names.notetype", "note:*"),
    ("names.template", "card:1"),
    ("names.ids", "cid:1,2,3 OR nid:4,5,6 OR did:1"),
    ("names.dupe", "dupe:1,a"),
];

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The generator's frozen instant, with the day boundary an hour later.
fn context() -> SearchContext {
    let now = DEFAULT_NOW_MS / 1000;
    SearchContext {
        now,
        today: 1500,
        next_day_at: now + 3600,
        learn_ahead_secs: SearchContext::DEFAULT_LEARN_AHEAD_SECS,
        current_deck: DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("search-terms");
    let ctx = context();
    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let clock = FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC);
        let random = SeededRandom::new(1);
        let mut initializers = Initializers::new();
        initializers.register(meguri_search::sqlfns::register);
        let db = open_collection(&copy.path, &OpenOptions::new(&clock, &random).initializers(initializers))
            .expect("the collection opens");
        let reader = db.open_reader(ReaderRole::Query).expect("a query reader");
        for (name, text) in SEARCHES {
            let query = parse(text).expect("the search parses");
            let run = || {
                let plan = Plan::build(&query, SearchMode::Cards, &ctx).expect("the plan builds");
                let params: Vec<SqlValue> = plan
                    .params()
                    .iter()
                    .map(|p| match p {
                        Param::Integer(v) => SqlValue::Integer(*v),
                        Param::Real(v) => SqlValue::Real(*v),
                        Param::Text(v) => SqlValue::Text(v.clone()),
                    })
                    .collect();
                reader.execute_raw(plan.sql(), &params).expect("the search runs").rows.len()
            };
            // Warm the pages and the statement cache outside the samples.
            eprintln!("search-terms {size} {name}: {} cards", black_box(run()));
            let mut measurement = Measurement::start(&format!("search.terms.{name}"), Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(run())))
            });
            suite.push(measurement.finish());
        }
        drop(reader);
        db.close().expect("it closes");
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    for record in suite.records() {
        eprintln!(
            "{} fixture {} p95 {:?}",
            record.name,
            record.fixture.as_deref().unwrap_or("none"),
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
