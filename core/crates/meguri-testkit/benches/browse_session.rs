//! U85: Browse search sessions (`specs/browser/result-sets.md`) on a working
//! copy of a generated collection, through the engine's entry points
//! (plan section 3; `browser/workspace` V17's budgets: simple-token first
//! page 100 ms, text, field and regex first page 300 ms, sort 200 ms).
//!
//! - `browse.first_page.<case>`: from `start_search` to the `FirstPage`
//!   event, sorted as Browse sorts; the rest of the events are drained
//!   outside the sample.
//! - `browse.resort.<mode>.<key>`: from `resort` of a whole-collection
//!   handle to its `FirstPage`.
//! - `browse.toggle.<from>.<key>`: from `toggle` of a whole-collection
//!   handle to its `FirstPage`, derived without a predicate run (H11).
//! - `browse.reverse.cards`, `browse.rows.page`: a reverse in memory, and a
//!   page of 400 rows (the visible rows and 200 each side, H7) with the
//!   default cards-mode columns.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`. The result goes to `$MEGURI_BENCH_RESULTS`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::browse::search::{
    HandleId, RequestSeq, SearchEvent, SearchListener, SearchSession, SortDirection, SortSpec,
};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_search::SearchMode;
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;
use meguri_testkit::wallclock::Stopwatch;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// `browse.first_page.<name>`: simple tokens first, then text terms.
const FIRST_PAGES: [(&str, &str, SearchMode, &str); 11] = [
    ("whole.cards.noteFld", "", SearchMode::Cards, "noteFld"),
    ("whole.cards.cardDue", "", SearchMode::Cards, "cardDue"),
    ("whole.notes.noteFld", "", SearchMode::Notes, "noteFld"),
    ("whole.notes.deck", "", SearchMode::Notes, "deck"),
    ("due.cards.cardDue", "is:due", SearchMode::Cards, "cardDue"),
    ("tag.cards.noteFld", "tag:a*", SearchMode::Cards, "noteFld"),
    ("deck.notes.noteCrt", "deck:a*", SearchMode::Notes, "noteCrt"),
    ("text.cards.noteFld", "kariwa", SearchMode::Cards, "noteFld"),
    ("field.cards.noteFld", "front:ka*", SearchMode::Cards, "noteFld"),
    ("regex.cards.noteFld", "re:^ka.*wa", SearchMode::Cards, "noteFld"),
    ("text.notes.noteFld", "kariwa", SearchMode::Notes, "noteFld"),
];

/// `browse.resort.<mode>.<key>`: from a whole-collection handle.
const RESORTS: [(SearchMode, &str, &str); 6] = [
    (SearchMode::Cards, "cardMod", "noteFld"),
    (SearchMode::Cards, "noteFld", "cardDue"),
    (SearchMode::Cards, "cardDue", "deck"),
    (SearchMode::Notes, "noteCrt", "noteFld"),
    (SearchMode::Notes, "noteFld", "deck"),
    (SearchMode::Notes, "noteFld", "cardDue"),
];

const WAIT: Duration = Duration::from_secs(120);

struct Events(Mutex<mpsc::Sender<SearchEvent>>);

impl SearchListener for Events {
    fn event(&self, event: SearchEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn mode_name(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Cards => "cards",
        SearchMode::Notes => "notes",
    }
}

/// Waits for `seq`'s first page (timed from `watch`), then drains its
/// events up to its terminal one; returns the first-page time and the
/// complete handle.
fn first_page(events: &Receiver<SearchEvent>, seq: RequestSeq, watch: Stopwatch) -> (Duration, HandleId) {
    let mut first = None;
    loop {
        let event = events.recv_timeout(WAIT).expect("a search event");
        if event.seq() != seq {
            continue;
        }
        match event {
            SearchEvent::FirstPage { .. } if first.is_none() => first = Some(watch.elapsed()),
            SearchEvent::Complete { handle, .. } => return (first.expect("a first page"), handle),
            SearchEvent::FirstPage { .. } | SearchEvent::Progress { .. } => {}
            other => panic!("the search ended with {other:?}"),
        }
    }
}

fn search(session: &SearchSession, events: &Receiver<SearchEvent>, text: &str, mode: SearchMode, key: &str) -> (Duration, HandleId) {
    let watch = Stopwatch::start();
    let seq = session.start_search(text, mode, SortSpec::new(key, SortDirection::Ascending), None);
    first_page(events, seq, watch)
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("browse-session");
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
        let (sender, events) = mpsc::channel();
        let session = host
            .engine()
            .open_search_session(Arc::new(Events(Mutex::new(sender))));
        let push = |suite: &mut Suite, measurement: Measurement| {
            // A criterion filter skips the other cases: no row for them.
            if measurement.samples().count() > 0 {
                suite.push(measurement.finish());
            }
        };
        for (name, text, mode, key) in FIRST_PAGES {
            let name = format!("browse.first_page.{name}");
            // Warm the pages and the statement outside the samples.
            black_box(search(&session, &events, text, mode, key));
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run_timed(n, |_| search(&session, &events, text, mode, key).0))
            });
            push(&mut suite, measurement);
        }
        for (mode, from, to) in RESORTS {
            let name = format!("browse.resort.{}.{to}", mode_name(mode));
            let resort = |handle: HandleId| {
                let watch = Stopwatch::start();
                let seq = session
                    .resort(handle, SortSpec::new(to, SortDirection::Ascending), None)
                    .expect("a complete handle");
                first_page(&events, seq, watch)
            };
            // Each sample resorts a fresh whole-collection handle: the
            // session keeps only its two newest complete handles.
            let (_, source) = search(&session, &events, "", mode, from);
            black_box(resort(source));
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run_timed(n, |_| {
                        let (_, source) = search(&session, &events, "", mode, from);
                        resort(source).0
                    })
                })
            });
            push(&mut suite, measurement);
        }
        // A toggle of `is:new` (card scope) is derived from the handle.
        for (mode, from, to) in [(SearchMode::Cards, "noteFld", "noteFld"), (SearchMode::Notes, "noteFld", "cardDue")] {
            let name = format!("browse.toggle.{}.{to}", mode_name(mode));
            let toggle = |handle: HandleId| {
                let watch = Stopwatch::start();
                let seq = session
                    .toggle(handle, SortSpec::new(to, SortDirection::Ascending), None)
                    .expect("a complete handle");
                first_page(&events, seq, watch)
            };
            let (_, source) = search(&session, &events, "is:new", mode, from);
            black_box(toggle(source));
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run_timed(n, |_| {
                        let (_, source) = search(&session, &events, "is:new", mode, from);
                        toggle(source).0
                    })
                })
            });
            push(&mut suite, measurement);
        }
        {
            let (_, handle) = search(&session, &events, "", SearchMode::Cards, "noteFld");
            let name = "browse.reverse.cards";
            let mut revision = 0;
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run(n, |_| {
                        revision = session.reverse(handle, revision).expect("a complete handle");
                    })
                })
            });
            push(&mut suite, measurement);
            let name = "browse.rows.page";
            let columns = ["noteFld", "template", "cardDue", "deck"];
            let span = session.handle_state(handle).expect("the handle").count.saturating_sub(400).max(1);
            let mut measurement = Measurement::start(name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run(n, |i| {
                        let start = (i * 997) % span;
                        session
                            .rows(handle, revision, start..start + 400, &columns)
                            .expect("a page")
                            .len()
                    })
                })
            });
            push(&mut suite, measurement);
        }
        drop(session);
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
