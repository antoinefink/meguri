//! U266: Find Duplicates (`specs/browser/workspace.md` V10; its budget: 2 s
//! over 150,000 notes, off the main thread).
//!
//! - `browse.find_duplicates.all_notes` (each size): the Front field over
//!   All Notes, as the sheet's default target reads it.
//! - `browse.find_duplicates.all_found`: the same over All Found, a
//!   selection of every row of a Notes search of the whole collection.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`; they count
//! cards, and the log names the notes read), made first by `tools/mg bench`.
//! The result goes to `$MEGURI_BENCH_RESULTS`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::ProgressSink;
use meguri_engine::browse::search::{
    DuplicatesTarget, HandleId, SearchEvent, SearchListener, SortDirection, SortSpec, find_duplicates,
};
use meguri_engine::CancelToken;
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

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// The field every standard note type of the fixture has first.
const FIELD: &str = "Front";
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

/// The complete handle and count of a whole-collection Notes search.
fn every_note(events: &Receiver<SearchEvent>, seq: meguri_engine::browse::search::RequestSeq) -> (HandleId, u64) {
    loop {
        let event = events.recv_timeout(WAIT).expect("a search event");
        if event.seq() != seq {
            continue;
        }
        match event {
            SearchEvent::Complete { handle, count, .. } => return (handle, count),
            SearchEvent::FirstPage { .. } | SearchEvent::Progress { .. } => {}
            other => panic!("the search ended with {other:?}"),
        }
    }
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .configure_from_args();
    let mut suite = Suite::wall_clock("browse-duplicates");
    let push = |suite: &mut Suite, measurement: Measurement| {
        // A criterion filter skips the other cases: no row for them.
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
    };

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
        let engine = host.engine();
        let (sender, events) = mpsc::channel();
        let session = engine.open_search_session(Arc::new(Events(Mutex::new(sender))));
        let seq = session.start_search("", SearchMode::Notes, SortSpec::new("noteCrt", SortDirection::Ascending), None);
        let (handle, count) = every_note(&events, seq);
        let found = session.selection(handle, 0, vec![0..count]);
        let targets = [
            ("all_notes", DuplicatesTarget::AllNotes(engine)),
            ("all_found", DuplicatesTarget::Found(&found)),
        ];
        for (case, target) in targets {
            let run = || {
                find_duplicates(target, FIELD, &CancelToken::new(), &ProgressSink::new()).expect("the duplicates")
            };
            // Warm the pages outside the samples.
            let warm = run();
            eprintln!(
                "browse_duplicates bench: {size} {case}: {count} notes, {} read, {} groups of {} notes",
                warm.considered,
                warm.groups.len(),
                warm.note_ids().len()
            );
            let name = format!("browse.find_duplicates.{case}");
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(run())))
            });
            push(&mut suite, measurement);
        }
        drop(found);
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
