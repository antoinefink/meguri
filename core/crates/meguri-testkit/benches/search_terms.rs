//! U82 and U81: one search statement per term family of
//! `specs/search/names-and-ids`, `state-and-flags`, `properties`, `history`,
//! `text-matching` and `fields`, run whole on a working copy of a generated
//! collection (plan section 3: every unit that adds a query adds its bench
//! row). Each sample builds the plan and reads every matching id, unsorted,
//! on a query reader with the product's SQL functions; the note types are
//! read once per collection, outside the samples. The first-page and
//! streaming budgets (R1825, R1826) are U90's once sessions exist (U85).
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
use meguri_search::{FoldMode, Param, Plan, SearchContext, SearchMode, SearchNotetypes, parse};
use meguri_storage::raw::SqlValue;
use meguri_storage::{Initializers, OpenOptions, ReaderRole, open_collection};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// The benchmark name of each search, `search.terms.<family>.<term>`, in
/// cards mode.
const SEARCHES: [(&str, &str); 33] = [
    ("names.tag", "tag:a*"),
    ("names.tag_regex", "tag:re:^a"),
    ("names.deck", "deck:a*"),
    ("names.deck_current", "deck:current"),
    ("names.preset", "preset:Default"),
    ("names.notetype", "note:*"),
    ("names.template", "card:1"),
    ("names.ids", "cid:1,2,3 OR nid:4,5,6 OR did:1"),
    ("names.dupe", "dupe:1,a"),
    ("state.due", "is:due"),
    ("state.new_suspended", "is:new OR is:suspended"),
    ("state.flag", "flag:1"),
    ("props.columns", "prop:ivl>10 prop:reps>3"),
    ("props.due", "prop:due<0"),
    ("props.stability", "prop:s>10"),
    ("props.retrievability", "prop:r<0.9"),
    ("props.custom_data", "has-cd:v"),
    ("history.rated", "rated:1"),
    ("history.introduced", "introduced:7"),
    ("history.added_edited", "added:30 OR edited:30"),
    ("history.prop_rated", "prop:rated=-1"),
    ("history.resched", "resched:30"),
    // Windows over the whole review log.
    ("history.introduced_wide", "introduced:36500"),
    ("history.rated_wide", "rated:36500"),
    ("history.prop_rated_wide", "prop:rated<=0"),
    // U81: the generator's fields are words of its syllables.
    ("text.plain", "kariwa"),
    ("text.field", "front:ka*"),
    ("text.field_regex", "front:re:wa$"),
    ("text.regex", "re:^ka.*wa"),
    ("text.nc", "nc:kariwa"),
    ("text.sc", "sc:kariwa"),
    ("text.word", "w:kamo"),
    ("text.all_fields", "*:ka*"),
];

/// Text searches in notes mode, `search.terms.<family>.<term>`.
const NOTE_SEARCHES: [(&str, &str); 3] = [
    ("text.plain_notes", "kariwa"),
    ("text.field_notes", "front:ka*"),
    ("text.regex_notes", "re:^ka.*wa"),
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
        let notetypes = SearchNotetypes::read(&reader).expect("the note types read");
        let searches = SEARCHES
            .iter()
            .map(|&(name, text)| (name, text, SearchMode::Cards))
            .chain(NOTE_SEARCHES.iter().map(|&(name, text)| (name, text, SearchMode::Notes)));
        for (name, text, mode) in searches {
            let query = parse(text).expect("the search parses");
            let run = || {
                let plan = Plan::build(&query, mode, &ctx, &notetypes).expect("the plan builds");
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
            eprintln!("search-terms {size} {name}: {} {mode:?}", black_box(run()));
            let mut measurement = Measurement::start(&format!("search.terms.{name}"), Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(run())))
            });
            // A criterion filter skips the other searches: no row for them.
            if measurement.samples().count() > 0 {
                suite.push(measurement.finish());
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
