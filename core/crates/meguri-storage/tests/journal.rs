//! The write journal and the storage-only builder (U23): one entry per
//! changed row with its first before-image, note after-images as digests,
//! before-images spilled past the threshold, bounded memory over a 150k-note
//! rewrite, and a builder that refuses existing and open paths and keeps no
//! journal.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{NOW_MS, card, clock, empty_collection, exec, new_col, new_config, note, open};
use meguri_model::card::CardQueue;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::journal::After;
use meguri_storage::{
    BuilderRefusal, Cell, CollectionBuilder, JournalOptions, OpenOptions, ReadRows, Record, RowKey, StampMode,
    StorageError, Table,
};

/// Counts the bytes this test process holds on the Rust heap, and the peak
/// since the last reset.
struct Counting;

static HELD: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract is System's.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let held = HELD.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(held, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is System's.
        unsafe { System.dealloc(ptr, layout) };
        HELD.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn a_card_updated_twice_has_one_entry_with_the_original_before_image() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope.add_note(&note(1, &["a", "b"])).unwrap();
    let card_id = scope.add_card(&card(id, 0)).unwrap();
    scope.commit().unwrap();
    let original = writer.card(card_id).unwrap().unwrap();

    clock.step_ms(60_000);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(collection.journal_options()))
        .unwrap();
    let mut first = original.clone();
    first.queue = CardQueue::Suspended;
    scope.put(&first).unwrap();
    let mut second = scope.card(card_id).unwrap().unwrap();
    second.flags = 3;
    scope.put(&second).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();

    let cards: Vec<_> = journal.entries().filter(|e| e.table() == Table::Cards).collect();
    assert_eq!(cards.len(), 1);
    let entry = cards[0];
    assert_eq!(entry.key(), &RowKey::Id(card_id.0));
    assert_eq!(journal.before(entry).unwrap(), Some(original.to_cells()));
    let stored = writer.card(card_id).unwrap().unwrap();
    assert_eq!(entry.after(), &After::Row(stored.to_cells()));
    // The other entry is col.mod.
    assert_eq!(journal.len(), 2);
    assert!(journal.entry(Table::Col, &RowKey::Id(1)).is_some());
}

#[test]
fn a_note_after_image_is_its_digest_and_changed_columns() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let id = scope.add_note(&note(1, &["a", "b"])).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    let After::Note(added) = journal.entry(Table::Notes, &RowKey::Id(id.0)).unwrap().after() else {
        panic!("not a digest");
    };
    assert_eq!(added.changed.names(Table::Notes).len(), Table::Notes.columns().len());

    clock.step_ms(60_000);
    let mut edited = writer.note(id).unwrap().unwrap();
    edited.fields = "a\u{1f}c".to_owned();
    edited.sort_field = "a".to_owned();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope.put(&edited).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    let entry = journal.entry(Table::Notes, &RowKey::Id(id.0)).unwrap();
    let After::Note(digest) = entry.after() else {
        panic!("not a digest");
    };
    // usn was already -1: only the modification time and the fields change.
    assert_eq!(digest.changed.names(Table::Notes), ["mod", "flds"]);
    let other = journal.before(entry).unwrap().unwrap();
    assert_ne!(digest.sha256, [0; 32]);
    assert_eq!(other[6], Cell::text("a\u{1f}b"));
}

#[test]
fn before_images_past_the_threshold_spill_and_read_back() {
    let (dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let ids: Vec<_> = (0..50)
        .map(|i| scope.add_note(&note(1, &[&"x".repeat(200), &i.to_string()])).unwrap())
        .collect();
    scope.commit().unwrap();
    let originals: Vec<_> = ids.iter().map(|id| writer.note(*id).unwrap().unwrap()).collect();

    clock.step_ms(60_000);
    let options = JournalOptions {
        spill_threshold: 2_000,
        spill_dir: Some(dir.path().to_path_buf()),
    };
    let mut scope = writer.begin_write(StampMode::Local, &clock, Some(options)).unwrap();
    for original in &originals {
        let mut edited = original.clone();
        edited.fields.push('!');
        scope.put(&edited).unwrap();
    }
    let journal = scope.commit().unwrap().journal.unwrap();
    assert!(journal.held_bytes() <= 2_000, "{}", journal.held_bytes());
    assert!(journal.spilled_bytes() > 0);
    for original in &originals {
        let entry = journal.entry(Table::Notes, &RowKey::Id(original.id.0)).unwrap();
        assert_eq!(journal.before(entry).unwrap(), Some(original.to_cells()));
    }
    // The spill file has no name in the directory.
    assert_eq!(
        common::listing(dir.path())
            .into_iter()
            .filter(|n| !n.starts_with("collection.anki2"))
            .count(),
        0
    );
}

#[test]
fn the_builder_writes_new_files_with_no_journal_and_refuses_existing_and_open_paths() {
    let clock = clock();
    let random = SeededRandom::new(1);
    let options = OpenOptions::new(&clock, &random);

    // An existing collection.
    let (_dir, existing) = common::copy_fixture("schema-18.anki2");
    let refused = CollectionBuilder::create(&existing, &new_col(NOW_MS), &new_config(), &options).unwrap_err();
    assert_eq!(
        refused,
        StorageError::BuilderRefused {
            reason: BuilderRefusal::Exists
        }
    );

    // A collection open in this process, whose files were removed under it.
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let opened = open(&live).unwrap();
    for name in common::listing(dir.path()) {
        std::fs::remove_file(dir.path().join(name)).unwrap();
    }
    let refused = CollectionBuilder::create(&live, &new_col(NOW_MS), &new_config(), &options).unwrap_err();
    assert_eq!(
        refused,
        StorageError::BuilderRefused {
            reason: BuilderRefusal::Open
        }
    );
    drop(opened);

    // A new file: rows as given, no journal, col.mod left alone.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("built.anki2");
    let builder = CollectionBuilder::create(&path, &new_col(NOW_MS), &new_config(), &options).unwrap();
    assert!(builder.begin(StampMode::Local, &clock).is_err());
    let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
    let mut given = note(1, &["a", "b"]);
    given.id.0 = 5;
    given.usn.0 = 3;
    given.mtime.0 = 1_600_000_000;
    scope.put(&given).unwrap();
    let images: Vec<_> = (10..2510)
        .map(|i| {
            let mut n = note(1, &[&format!("n{i}"), "x"]);
            n.id.0 = i;
            n.to_cells()
        })
        .collect();
    assert_eq!(scope.insert_rows(Table::Notes, &images).unwrap(), 2500);
    assert!(scope.journal().is_none());
    let committed = scope.commit().unwrap();
    assert!(committed.journal.is_none() && committed.col_mod.is_none() && committed.changed);
    let path = builder.finish().unwrap();

    let opened = open(&path).unwrap();
    let writer = opened.writer();
    assert_eq!(writer.note(meguri_model::ids::NoteId(5)).unwrap().unwrap(), given);
    assert_eq!(common::int(&exec(writer, "SELECT count(*) FROM notes")), 2501);
    assert_eq!(writer.col().unwrap(), new_col(NOW_MS));
    assert_eq!(common::object_set(writer), {
        let mut expected = common::anki_made_objects();
        expected.retain(|(_, name)| !name.starts_with("sqlite_stat"));
        expected
    });
    // The builder refuses a path it already made.
    drop(opened);
    let refused = CollectionBuilder::create(&path, &new_col(NOW_MS), &new_config(), &options).unwrap_err();
    assert_eq!(
        refused,
        StorageError::BuilderRefused {
            reason: BuilderRefusal::Exists
        }
    );
}

/// The journal's bound (R1851): rewriting the fields of 150,000 notes in
/// one operation keeps note after-images as digests and spills
/// before-images, so the process's Rust heap grows by no more than this
/// many bytes (the entries and their index), against about 75 MB of
/// before-images. U245 sets the journal's share of R1851's 150 MB; this
/// budget is the bound until then.
const REWRITE_HEAP_BUDGET: usize = 48 * 1024 * 1024;

#[test]
fn a_150k_note_field_rewrite_stays_within_the_journal_memory_bound() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let random = SeededRandom::new(2);
    let builder = CollectionBuilder::create(
        &path,
        &new_col(NOW_MS),
        &new_config(),
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    let field = "f".repeat(400);
    let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
    for start in (0..150_000i64).step_by(10_000) {
        let images: Vec<_> = (start..start + 10_000)
            .map(|i| {
                let mut n = note(1, &[&field, &i.to_string()]);
                n.id.0 = i + 1;
                n.to_cells()
            })
            .collect();
        scope.insert_rows(Table::Notes, &images).unwrap();
    }
    scope.commit().unwrap();
    builder.finish().unwrap();

    let collection = open(&path).unwrap();
    let writer = collection.writer();
    exec(writer, "PRAGMA synchronous = OFF");
    clock.step_ms(60_000);
    let baseline = HELD.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(collection.journal_options()))
        .unwrap();
    let mut after = None;
    loop {
        let page = scope.notes_after(after, 1000).unwrap();
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for mut n in page {
            n.fields = format!("{}!", n.fields);
            scope.put(&n).unwrap();
        }
    }
    let committed = scope.commit().unwrap();
    let peak = PEAK.load(Ordering::Relaxed) - baseline;
    let journal = committed.journal.unwrap();
    assert_eq!(journal.len(), 150_001);
    assert!(journal.spilled_bytes() > 50_000_000, "{}", journal.spilled_bytes());
    assert!(
        peak <= REWRITE_HEAP_BUDGET,
        "peak heap growth {peak} bytes over a budget of {REWRITE_HEAP_BUDGET}"
    );
}
