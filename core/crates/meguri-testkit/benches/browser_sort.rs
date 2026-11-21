//! U84: the sorted statement of `specs/browser/sorting.md` for the keys that
//! cost the most, on a working copy of a generated collection (plan section
//! 3: every unit that adds a query adds its bench row; R1828's sort budget,
//! 200 ms on 150k). Each sample builds the sorted statement and reads every
//! id of the whole collection in order on a query reader with the
//! product's SQL functions, from the search's predicate (`for_plan`) and
//! from an id list, as a resort does (`for_ids`, `browser/result-sets` H8).
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
use meguri_search::config::SortDirection;
use meguri_search::{FoldMode, Param, Plan, SearchContext, SearchMode, SearchNotetypes, Sort, SortedQuery, parse};
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{Initializers, OpenOptions, ReaderRole, open_collection};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// `browser.sort.<mode>.<key>`: the keys with ranks, per-note subqueries or
/// per-row functions, and one plain column for scale.
const SORTS: [(SearchMode, &str); 14] = [
    (SearchMode::Cards, "cardMod"),
    (SearchMode::Cards, "noteFld"),
    (SearchMode::Cards, "template"),
    (SearchMode::Cards, "deck"),
    (SearchMode::Cards, "note"),
    (SearchMode::Cards, "cardDue"),
    (SearchMode::Cards, "originalPosition"),
    (SearchMode::Cards, "retrievability"),
    (SearchMode::Cards, "stability"),
    (SearchMode::Notes, "deck"),
    (SearchMode::Notes, "cardDue"),
    (SearchMode::Notes, "cardEase"),
    (SearchMode::Notes, "template"),
    (SearchMode::Notes, "originalPosition"),
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

fn values(params: &[Param]) -> Vec<SqlValue> {
    params
        .iter()
        .map(|p| match p {
            Param::Integer(v) => SqlValue::Integer(*v),
            Param::Real(v) => SqlValue::Real(*v),
            Param::Text(v) => SqlValue::Text(v.clone()),
        })
        .collect()
}

fn ids(rows: &RawRows) -> Vec<i64> {
    rows.rows
        .iter()
        .map(|row| match row[0] {
            SqlValue::Integer(id) => id,
            ref other => panic!("not an id: {other:?}"),
        })
        .collect()
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("browser-sort");
    let ctx = context();
    let query = parse("").expect("the whole collection parses");
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
        let notetypes = SearchNotetypes::read(&reader).expect("the note types read");
        // The baseline: every id unsorted, read the same way.
        for (mode, mode_name) in [(SearchMode::Cards, "cards"), (SearchMode::Notes, "notes")] {
            let plan = Plan::build(&query, mode, &ctx, &notetypes).expect("the plan builds");
            let run = || reader.execute_raw(plan.sql(), &[]).expect("the search runs").rows.len();
            let name = format!("browser.sort.unsorted.{mode_name}");
            eprintln!("browser-sort {size} {name}: {} rows", black_box(run()));
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("unsorted.{mode_name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(run())))
            });
            if measurement.samples().count() > 0 {
                suite.push(measurement.finish());
            }
        }
        for (mode, key) in SORTS {
            let plan = Plan::build(&query, mode, &ctx, &notetypes).expect("the plan builds");
            let all = ids(&reader.execute_raw(plan.sql(), &[]).expect("the search runs"));
            let sort = Sort::new(mode, key, SortDirection::Descending).expect("a sortable key");
            let mode_name = match mode {
                SearchMode::Cards => "cards",
                SearchMode::Notes => "notes",
            };
            let from_plan = || {
                let sorted = SortedQuery::for_plan(&plan, &sort, &ctx).expect("the sort builds");
                reader
                    .execute_raw(sorted.sql(), &values(sorted.params()))
                    .expect("the sort runs")
                    .rows
                    .len()
            };
            let from_ids = || {
                let sorted = SortedQuery::for_ids(&all, &sort, &ctx).expect("the sort builds");
                reader
                    .execute_raw(sorted.sql(), &values(sorted.params()))
                    .expect("the resort runs")
                    .rows
                    .len()
            };
            let runs: [(&str, &dyn Fn() -> usize); 2] = [("search", &from_plan), ("resort", &from_ids)];
            for (source, run) in runs {
                let name = format!("browser.sort.{source}.{mode_name}.{key}");
                // Warm the pages and the statement cache outside the samples.
                eprintln!("browser-sort {size} {name}: {} rows", black_box(run()));
                let mut measurement = Measurement::start(&name, Some(&size));
                criterion.bench_function(&format!("{source}.{mode_name}.{key}/{size}"), |b| {
                    b.iter_custom(|n| measurement.run(n, |_| black_box(run())))
                });
                // A criterion filter skips the other sorts: no row for them.
                if measurement.samples().count() > 0 {
                    suite.push(measurement.finish());
                }
            }
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
