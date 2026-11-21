//! The bulk action wall-clock suite (U12 harness, R1831, R1832;
//! `specs/browser/bulk-framework.md`, `specs/browser/find-replace.md`):
//! Find and Replace from the call to its result, compute phase and commit
//! included, each sample one undoable op.
//!
//! - `engine.bulk_find_replace_10k`: Find and Replace in All Fields on
//!   10,000 listed notes spread over the collection (R1831: a bulk action
//!   on 10,000 notes in 2 s), every note written: the samples alternately
//!   prefix every field with a marker and remove it again.
//! - `engine.find_replace_all_notes`: the same on the all-notes target, so
//!   every note of the collection is read and written (R1832: 150,000
//!   notes in 5 s; the `175k` card fixture holds about 150,000 notes).
//! - `engine.find_replace_listed_all`: the same with every note id listed,
//!   as Browse's All Found passes the result's ids (`browser/workspace` V9).
//! - `engine.find_replace_scan_all_notes`: the all-notes target with a
//!   find text no note holds: the compute phase alone (nothing written).
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (comma-separated, default
//! `10k,150k`); sizes count cards. The result goes to
//! `$MEGURI_BENCH_RESULTS/bulk.json`. On Apple hosts (`tools/mg bench-mac`)
//! the bench exits with status 1 after writing its result when the 10k
//! p95 is 2 s or more, or the all-notes p95 is 5 s or more on a collection
//! of at least 150,000 notes.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::bulk::NoteTarget;
use meguri_engine::bulk::find_replace::{self, FindIn, FindReplace};
use meguri_engine::{CancelToken, Collection, Engine, EngineConfig, OpenConfig, ProgressSink};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::NoteId;
use meguri_model::rng::SeededRandom;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const BULK: &str = "engine.bulk_find_replace_10k";
const ALL: &str = "engine.find_replace_all_notes";
const SCAN: &str = "engine.find_replace_scan_all_notes";
const LISTED: &str = "engine.find_replace_listed_all";
const NOTES: usize = 10_000;
const BULK_BUDGET: Duration = Duration::from_secs(2);
const ALL_BUDGET: Duration = Duration::from_secs(5);
const ALL_BUDGET_NOTES: usize = 150_000;
/// A marker no generated field holds. `^` also matches an empty field, so
/// the first marking can add the cards such a field gated; later samples
/// run on that collection.
const MARK: &str = "qzx";

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Up to [`NOTES`] notes spread over the whole collection.
fn spread_notes(all: &[i64]) -> Vec<NoteId> {
    let step = (all.len() / NOTES).max(1);
    all.iter().step_by(step).take(NOTES).map(|id| NoteId(*id)).collect()
}

fn regex(find: &str, replace: &str) -> FindReplace {
    FindReplace {
        find: find.to_owned(),
        replace: replace.to_owned(),
        regex: true,
        ignore_case: false,
        within: FindIn::AllFields,
    }
}

/// Marks every field of the target's notes, or removes the mark again.
fn toggle(collection: &Collection, target: &NoteTarget, marked: bool) -> u64 {
    let request = if marked {
        regex(&format!("^{MARK}"), "")
    } else {
        regex("^", MARK)
    };
    let done = find_replace::run(collection, target, &request, CancelToken::new(), ProgressSink::new())
        .expect("Find and Replace commits");
    done.value.affected
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("bulk");
    let mut group = criterion.benchmark_group("bulk");
    // Each sample is a whole bulk operation (hundreds of milliseconds and more).
    group.sample_size(10);
    let mut note_counts = Vec::new();
    for size in sizes() {
        let generated = match fixtures::profile_collection(&size) {
            Ok(generated) => generated,
            Err(e) => {
                eprintln!("bulk bench: the {size} collection: {e}");
                return ExitCode::FAILURE;
            }
        };
        let copy = generated.working_copy().expect("a working copy of the collection");
        let engine = Engine::new(EngineConfig::default()).expect("an engine");
        let config = OpenConfig::new(
            Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(1)),
        );
        let collection = engine.open_raw(&copy.path, config).expect("the collection opens");
        let all = collection
            .reader(ReaderRole::Query)
            .expect("a reader")
            .ids_after(Table::Notes, None, usize::MAX)
            .expect("note ids");
        let notes = spread_notes(&all);
        eprintln!("bulk bench: {size}: {} of {} notes", notes.len(), all.len());
        note_counts.push((size.clone(), all.len()));
        let listed = NoteTarget::Notes(notes.clone());
        // Warm: the statements prepared, the pages in the cache.
        assert_eq!(toggle(&collection, &listed, false), notes.len() as u64);
        assert_eq!(toggle(&collection, &listed, true), notes.len() as u64);

        let mut bulk = Measurement::start(BULK, Some(&size));
        let mut marked = false;
        group.bench_function(format!("find_replace_10k/{size}"), |b| {
            b.iter_custom(|iters| {
                bulk.run(iters, |_| {
                    assert_eq!(toggle(&collection, &listed, marked), notes.len() as u64);
                    marked = !marked;
                })
            })
        });
        suite.push(bulk.finish());
        if marked {
            toggle(&collection, &listed, true);
        }

        let mut every = Measurement::start(ALL, Some(&size));
        let mut marked = false;
        group.bench_function(format!("find_replace_all_notes/{size}"), |b| {
            b.iter_custom(|iters| {
                every.run(iters, |_| {
                    assert_eq!(toggle(&collection, &NoteTarget::AllNotes, marked), all.len() as u64);
                    marked = !marked;
                })
            })
        });
        suite.push(every.finish());
        if marked {
            toggle(&collection, &NoteTarget::AllNotes, true);
        }

        let every_id = NoteTarget::Notes(all.iter().map(|id| NoteId(*id)).collect());
        let mut listed_all = Measurement::start(LISTED, Some(&size));
        let mut marked = false;
        group.bench_function(format!("find_replace_listed_all/{size}"), |b| {
            b.iter_custom(|iters| {
                listed_all.run(iters, |_| {
                    assert_eq!(toggle(&collection, &every_id, marked), all.len() as u64);
                    marked = !marked;
                })
            })
        });
        suite.push(listed_all.finish());
        if marked {
            toggle(&collection, &every_id, true);
        }

        let mut scan = Measurement::start(SCAN, Some(&size));
        let nothing = FindReplace {
            regex: false,
            ..regex(MARK, "x")
        };
        group.bench_function(format!("find_replace_scan_all_notes/{size}"), |b| {
            b.iter_custom(|iters| {
                scan.run(iters, |_| {
                    let done = find_replace::run(
                        &collection,
                        &NoteTarget::AllNotes,
                        &nothing,
                        CancelToken::new(),
                        ProgressSink::new(),
                    )
                    .expect("the scan commits nothing");
                    assert_eq!(done.value.affected, 0);
                })
            })
        });
        suite.push(scan.finish());
        engine.close().expect("the collection closes");
    }
    group.finish();
    criterion.final_summary();

    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    match suite.write(&report::results_dir(target)) {
        Ok(path) => eprintln!("bulk bench: wrote {}", path.display()),
        Err(e) => {
            eprintln!("bulk bench: writing the result: {e}");
            return ExitCode::FAILURE;
        }
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let (p50, p95) = (record.percentile(Percentile::P50), record.percentile(Percentile::P95));
        let fixture = record.fixture.as_deref().unwrap_or("-");
        eprintln!("bulk bench: {} on {fixture}: p50 {p50:?}, p95 {p95:?}", record.name);
        let notes = note_counts
            .iter()
            .find(|(size, _)| size == fixture)
            .map_or(0, |(_, count)| *count);
        let budget = match record.name.as_str() {
            BULK => Some(BULK_BUDGET),
            ALL | LISTED if notes >= ALL_BUDGET_NOTES => Some(ALL_BUDGET),
            _ => None,
        };
        if let Some(budget) = budget
            && p95 >= budget
            && cfg!(target_vendor = "apple")
        {
            eprintln!("bulk bench: {} is over its {budget:?} p95 budget", record.name);
            status = ExitCode::FAILURE;
        }
    }
    status
}
