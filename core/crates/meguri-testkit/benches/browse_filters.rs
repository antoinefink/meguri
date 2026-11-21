//! U263: Browse's suggestions and filter counts (`specs/search/filter-tokens`
//! T12; `browser/workspace` V17's budgets: suggestions within one frame,
//! 50 rows with 10,000 tags in under 4 ms; every count of the sidebar and
//! the first suggestion screen within 200 ms on 150,000 cards).
//!
//! - `browse.suggestions.<case>` (fixture `10k-tags`): one keystroke's
//!   list from a name index of 10,000 tags, 300 decks and 40 note types.
//! - `browse.filter_counts.<mode>` (each size): every count of the sidebar
//!   (built-in saved searches, states, flags, every deck, note type and
//!   tag) read cold, by a provider that keeps no cache.
//! - `browse.filter_counts.cached.<mode>`: the same counts from the host's
//!   provider once it holds them.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`. The result goes to `$MEGURI_BENCH_RESULTS`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::browse::FilterCounts;
use meguri_host::{CoreHost, HostConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_model::rng::SeededRandom;
use meguri_search::SearchMode;
use meguri_search::filter_tokens::counts::CountItem;
use meguri_search::filter_tokens::suggest::{BuiltIn, Labels, SuggestRequest, suggest};
use meguri_search::filter_tokens::{DeckValue, FilterQuery, NotetypeValue, StateValue, Value};
use meguri_search::name_index::{DeckRow, NameIndex, NotetypeNames};
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// `browse.suggestions.<name>`: the field's text, the caret at its end.
const SUGGESTIONS: [(&str, &str); 6] = [
    ("empty", ""),
    ("one_letter", "t"),
    ("word", "verb"),
    ("components", "lang::ve"),
    ("no_match", "zqzq"),
    ("tag_syntax", "tag:topic::0"),
];

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 10,000 tags in three levels, 300 decks in two, 40 note types.
fn large_index() -> NameIndex {
    let mut names = NameIndex::default();
    let words = ["lang", "verb", "noun", "grammar", "kanji", "topic", "unit", "vocab", "audio", "review"];
    let mut tags = Vec::with_capacity(10_000);
    for k in 0..10_000 {
        let a = words[k % words.len()];
        let b = words[(k / 10) % words.len()];
        tags.push(format!("{a}::{b}{}::{k:05}", (k / 100) % 10));
    }
    names.set_tags(tags);
    names.set_decks(
        (0..300)
            .map(|k| DeckRow {
                id: DeckId(1000 + k as i64),
                native: format!("{}\u{1f}{} {k:03}", words[k % words.len()], words[(k / 7) % words.len()]),
                filtered: false,
            })
            .collect(),
    );
    names.set_notetypes(
        (0..40)
            .map(|k| NotetypeNames {
                id: NotetypeId(100 + k as i64),
                name: format!("Type {k:02} {}", words[k % words.len()]),
                fields: vec!["Front".to_owned(), "Back".to_owned(), format!("{} notes", words[k % words.len()])],
                templates: vec!["Card 1".to_owned(), format!("{} card", words[(k + 3) % words.len()])],
            })
            .collect(),
    );
    names.set_flag_names(["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"].map(str::to_owned));
    names
}

/// Every count the sidebar shows (V6).
fn sidebar(names: &NameIndex) -> Vec<CountItem> {
    let mut values: Vec<Value> = BuiltIn::ALL.iter().map(|b| b.value()).collect();
    values.extend(
        [
            StateValue::New,
            StateValue::Learning,
            StateValue::Review,
            StateValue::Suspended,
            StateValue::Buried,
        ]
        .map(Value::State),
    );
    values.push(Value::NoFlag);
    values.extend((1..=7).map(Value::Flag));
    values.extend(names.decks().iter().filter_map(|d| DeckValue::of(names, d.id).map(Value::Deck)));
    values.extend(names.notetypes().iter().map(|n| Value::Notetype(NotetypeValue::of(n))));
    values.extend(names.tags().iter().map(|t| Value::Tag(t.clone())));
    let mut items: Vec<CountItem> = values.into_iter().map(CountItem::Value).collect();
    items.extend(names.saved_searches().iter().map(|s| CountItem::Saved(s.text.clone())));
    items
}

fn mode_name(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Cards => "cards",
        SearchMode::Notes => "notes",
    }
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("browse-filters");
    let push = |suite: &mut Suite, measurement: Measurement| {
        // A criterion filter skips the other cases: no row for them.
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
    };

    let names = large_index();
    let labels = Labels::default();
    for (name, text) in SUGGESTIONS {
        let name = format!("browse.suggestions.{name}");
        let query = FilterQuery::new(vec![], text);
        let request = SuggestRequest {
            query: &query,
            caret: text.chars().count(),
            scope: None,
            recent: &[],
        };
        let rows = suggest(&names, &labels, &request).expect("a list");
        eprintln!("browse_filters bench: {name}: {} rows", rows.len());
        let mut measurement = Measurement::start(&name, Some("10k-tags"));
        criterion.bench_function(&format!("{name}/10k-tags"), |b| {
            b.iter_custom(|n| measurement.run(n, |_| black_box(suggest(&names, &labels, &request).expect("a list"))))
        });
        push(&mut suite, measurement);
    }

    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let directory = copy.path.parent().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(auxiliary.path().join("backups")).unwrap();
        let host = CoreHost::open(HostConfig::new(
            directory.into(),
            auxiliary.path().join("backups"),
            auxiliary.path().join("app-group/widget.snapshot"),
            Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(1)),
            Arc::new(StatvfsSpace),
        ))
        .unwrap();
        let index = host.names().index();
        let items = sidebar(&index);
        eprintln!(
            "browse_filters bench: {size}: {} counts ({} decks, {} note types, {} tags)",
            items.len(),
            index.decks().len(),
            index.notetypes().len(),
            index.tags().len()
        );
        for mode in [SearchMode::Cards, SearchMode::Notes] {
            let cold = FilterCounts::new();
            let count = || {
                cold.filter_counts(host.engine(), &index, mode, &items, None)
                    .expect("the counts")
            };
            // Warm the pages outside the samples.
            let counted = count();
            assert!(counted.iter().all(Option::is_some), "every sidebar value counts");
            let name = format!("browse.filter_counts.{}", mode_name(mode));
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(count())))
            });
            push(&mut suite, measurement);

            let cached = host.filter_counts();
            cached
                .filter_counts(host.engine(), &index, mode, &items, None)
                .expect("the counts");
            let name = format!("browse.filter_counts.cached.{}", mode_name(mode));
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run(n, |_| {
                        black_box(
                            cached
                                .filter_counts(host.engine(), &index, mode, &items, None)
                                .expect("the counts"),
                        )
                    })
                })
            });
            push(&mut suite, measurement);
        }
        host.close().unwrap();
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
