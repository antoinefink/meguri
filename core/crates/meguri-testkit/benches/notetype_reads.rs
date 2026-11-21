//! U267: the Note Types screens' reads on a working copy of a generated
//! collection, through the engine's entry points (`browser/workspace` V17:
//! Note Types counts 300 ms, "no card" 1 s, both for 50,000 notes of the
//! type; the plan's section 3).
//!
//! - `notetypes.note_counts`: A2's note counts of every note type.
//! - `notetypes.field_content_counts`: W3.1 on the type with the most notes.
//! - `notetypes.notes_without_card.<draft>`: W8 for the unchanged draft
//!   and for a draft that adds a card type with a conditional front.
//! - `notetypes.draft_summary.<draft>`: D2 for a draft that removes the last
//!   field, and for one that removes the last card type (or adds one when
//!   the type has a single card type).
//! - `notetypes.removal_counts`: W12's counts.
//!
//! The type's note count is printed with each p95: the budgets are stated
//! for 50,000 notes of the type. Sizes come from `$MEGURI_BENCH_SIZES`
//! (default `10k,150k`), made first by `tools/mg bench`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::notetypes::counts;
use meguri_engine::notetypes::structure::{self, DraftTemplate, StructureDraft};
use meguri_engine::{CancelToken, ProgressSink};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::space::StatvfsSpace;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("notetype-reads");
    let mut notes_of_type = Vec::new();
    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(auxiliary.path().join("backups")).unwrap();
        let host = CoreHost::open(HostConfig::new(
            copy.path.parent().unwrap().into(),
            auxiliary.path().join("backups"),
            auxiliary.path().join("app-group/widget.snapshot"),
            Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(1)),
            Arc::new(StatvfsSpace),
        ))
        .unwrap();
        let collection = host.collection().unwrap();
        let (id, notes) = counts::list_note_counts(&collection)
            .unwrap()
            .into_iter()
            .max_by_key(|(_, notes)| *notes)
            .expect("a note type");
        notes_of_type.push((size.clone(), notes));
        let notetype = collection
            .reader(ReaderRole::Query)
            .unwrap()
            .notetype(id)
            .unwrap()
            .unwrap();
        let unchanged = StructureDraft::of(&notetype);
        let mut added = unchanged.clone();
        let first = notetype.fields[0].name.clone();
        let last = notetype.fields.last().unwrap().name.clone();
        added.templates.push(DraftTemplate {
            origin: None,
            name: "Bench Added".to_owned(),
            front: format!("{{{{#{last}}}}}{{{{{first}}}}}{{{{/{last}}}}}"),
            back: "{{FrontSide}}".to_owned(),
            browser_front: String::new(),
            browser_back: String::new(),
            deck: 0,
            browser_font_name: String::new(),
            browser_font_size: 0,
        });
        let mut remove_field = unchanged.clone();
        if remove_field.fields.len() > 1 {
            remove_field.fields.pop();
        }
        let template_draft = if unchanged.templates.len() > 1 {
            let mut d = unchanged.clone();
            d.templates.pop();
            ("remove_template", d)
        } else {
            ("add_template", added.clone())
        };
        let cancel = CancelToken::new();
        let progress = ProgressSink::new();
        let cases: Vec<(String, Box<dyn Fn() + '_>)> = vec![
            (
                "notetypes.note_counts".to_owned(),
                Box::new(|| {
                    black_box(counts::list_note_counts(&collection).unwrap());
                }),
            ),
            (
                "notetypes.field_content_counts".to_owned(),
                Box::new(|| {
                    black_box(counts::field_content_counts(&collection, id, &cancel, &progress).unwrap());
                }),
            ),
            (
                "notetypes.notes_without_card.unchanged".to_owned(),
                Box::new(|| {
                    black_box(counts::notes_without_card(&collection, &unchanged, &cancel, &progress).unwrap());
                }),
            ),
            (
                "notetypes.notes_without_card.added".to_owned(),
                Box::new(|| {
                    black_box(counts::notes_without_card(&collection, &added, &cancel, &progress).unwrap());
                }),
            ),
            (
                "notetypes.draft_summary.remove_field".to_owned(),
                Box::new(|| {
                    black_box(structure::draft_summary(&collection, &remove_field, &cancel, &progress).unwrap());
                }),
            ),
            (
                format!("notetypes.draft_summary.{}", template_draft.0),
                Box::new(|| {
                    black_box(structure::draft_summary(&collection, &template_draft.1, &cancel, &progress).unwrap());
                }),
            ),
            (
                "notetypes.removal_counts".to_owned(),
                Box::new(|| {
                    black_box(counts::removal_counts(&collection, id, &cancel, &progress).unwrap());
                }),
            ),
        ];
        for (name, op) in cases {
            // Warm the pages and the statements outside the samples.
            op();
            let mut measurement = Measurement::start(&name, Some(&size));
            criterion.bench_function(&format!("{name}/{size}"), |b| b.iter_custom(|n| measurement.run(n, |_| op())));
            // A criterion filter skips the other cases: no row for them.
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
    for record in suite.records() {
        let notes = notes_of_type
            .iter()
            .find(|(size, _)| Some(size.as_str()) == record.fixture.as_deref())
            .map_or(0, |(_, notes)| *notes);
        eprintln!(
            "{} fixture {:?}: p95 {:?} ({notes} notes of the type)",
            record.name,
            record.fixture,
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
