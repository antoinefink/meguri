//! The held-back run's memory (`specs/engine/undo.md` M8, M9.1; R1851): a
//! sync's barrier hands over the run without reading its before-images, and
//! the run reads them one at a time, from the journal's spill file when they
//! spilled. Its own test binary, so the counting allocator sees only it.

mod common;

use std::collections::BTreeMap;

use common::Harness;
use common::steps;
use meguri_engine::{OpSpec, SessionId, StepKind, SyncHold};
use meguri_model::changes::OpSource;
use meguri_model::note::Note;
use meguri_storage::{Record, RowKey, Table};
use meguri_testkit::bench::peak_alloc::PeakAlloc;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const S1: SessionId = SessionId(1);
/// The notes a session-scoped bulk edit rewrites. Their before-images, about
/// 18 MiB, pass the journal's 8 MiB spill threshold.
const NOTES: usize = 192;
const FIELD: usize = 96 * 1024;
/// What the barrier and the read of one image may add: far below one
/// image per note of the run.
const BOUND: usize = 2 * 1024 * 1024;

#[test]
fn a_sync_barrier_reads_no_before_image_and_the_run_reads_them_one_at_a_time() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, card) = steps::setup_note(c, "a");
    let originals: Vec<String> = (0..NOTES).map(|i| format!("{i:04}{}", "x".repeat(FIELD))).collect();
    let added = originals.clone();
    let notes = c
        .run(
            OpSpec::named("setup_notes", OpSource::UserNonUndoable).skip_undo(),
            move |ctx| {
                added
                    .iter()
                    .map(|text| Ok(ctx.scope().add_note(&common::note(&[text, "back"]))?))
                    .collect::<Result<Vec<_>, meguri_engine::EngineError>>()
            },
        )
        .unwrap()
        .value;
    let original_of: BTreeMap<i64, &String> = notes.iter().map(|nid| nid.0).zip(&originals).collect();
    steps::answer(c, Some(S1), card).unwrap();
    let edited = notes.clone();
    c.run(
        OpSpec::named("find_and_replace", OpSource::UserUndoable)
            .undo_step("Find and Replace", StepKind::Other)
            .session(S1),
        move |ctx| {
            for nid in edited {
                steps::set_first_field(ctx, nid, "short".to_owned())?;
            }
            Ok(())
        },
    )
    .unwrap();

    PeakAlloc::reset_peak();
    let base = PeakAlloc::current();
    let hold: SyncHold = c
        .run(OpSpec::named("sync_snapshot", OpSource::SyncIncoming), |ctx| {
            ctx.begin_sync_hold(Some(S1))
        })
        .unwrap()
        .value;
    let barrier = PeakAlloc::peak().saturating_sub(base);
    assert_eq!(hold.run.steps.len(), 2, "[the bulk edit, the answer]");
    assert!(
        barrier < BOUND,
        "the barrier allocated {barrier} bytes for a run whose before-images hold {} bytes",
        NOTES * FIELD
    );

    PeakAlloc::reset_peak();
    let base = PeakAlloc::current();
    let mut read = 0;
    for row in hold.run.rows() {
        let row = row.unwrap();
        if row.table != Table::Notes {
            continue;
        }
        let RowKey::Id(id) = row.key else {
            panic!("a note key is an id")
        };
        let note = Note::from_cells(row.before.as_deref().unwrap()).unwrap();
        assert_eq!(note.field_values().next(), Some(original_of[&id].as_str()));
        read += 1;
    }
    let streaming = PeakAlloc::peak().saturating_sub(base);
    assert_eq!(read, NOTES);
    assert!(
        streaming < BOUND,
        "reading the images one at a time allocated {streaming} bytes at most"
    );

    let last = notes[NOTES - 1];
    let row = hold.run.before(Table::Notes, &RowKey::Id(last.0)).unwrap().unwrap();
    let note = Note::from_cells(row.before.as_deref().unwrap()).unwrap();
    assert_eq!(note.field_values().next(), Some(originals[NOTES - 1].as_str()));
    assert!(hold.run.before(Table::Notes, &RowKey::Id(-1)).unwrap().is_none());
}
