//! The write journal and the storage-only builder (U23): one entry per
//! changed row with its first before-image, note after-images as digests,
//! before-images spilled past the threshold, bounded memory over a 150k-note
//! rewrite, and a builder that refuses existing and open paths and keeps no
//! journal.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{NOW_MS, card, clock, empty_collection, exec, new_col, new_config, note, open, revlog, tag};
use meguri_model::card::CardQueue;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::NoteId;
use meguri_model::rng::SeededRandom;
use meguri_model::tag::Tag;
use meguri_model::usn::Usn;
use meguri_storage::journal::{After, Journal};
use meguri_storage::raw::SqlValue;
use meguri_storage::{
    BuilderRefusal, Cell, CollectionBuilder, JournalOptions, OpenOptions, ReadRows, Record, RowImage, RowKey,
    StampMode, StorageError, Table, Writer,
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

/// A stored row as SQLite holds it, by storage class, read with raw SQL
/// (between scopes: raw SQL runs on the writer outside any scope).
fn stored_row(writer: &Writer, table: Table, condition: &str) -> RowImage {
    let rows = exec(writer, &format!("SELECT * FROM {} WHERE {condition}", table.name()));
    assert_eq!(rows.rows.len(), 1, "{table} {condition}: {rows:?}");
    rows.rows[0]
        .iter()
        .map(|value| match value {
            SqlValue::Null => Cell::Null,
            SqlValue::Integer(v) => Cell::Integer(*v),
            SqlValue::Real(v) => Cell::Real(*v),
            SqlValue::Text(v) => Cell::text(v.clone()),
            SqlValue::Blob(v) => Cell::Blob(v.clone()),
        })
        .collect()
}

fn note_after(journal: &Journal, id: i64) -> meguri_storage::journal::NoteDigest {
    match journal.entry(Table::Notes, &RowKey::Id(id)).map(|e| e.after()) {
        Some(After::Note(digest)) => *digest,
        other => panic!("note {id}: {other:?}"),
    }
}

/// The digest the journal gives a note row written exactly as `image`
/// holds it: the row removed and `image` restored in one scope, rolled
/// back. `image` is a stored row, so what is written is what is stored.
fn digest_when_restored(writer: &Writer, clock: &StepClock, image: &RowImage) -> [u8; 32] {
    let id = image[0].as_i64().unwrap();
    let key = RowKey::Id(id);
    let mut scope = writer
        .begin_write(StampMode::CallerSupplied, clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope.restore(Table::Notes, &key, None).unwrap();
    scope.restore(Table::Notes, &key, Some(image)).unwrap();
    let digest = note_after(scope.journal().unwrap(), id);
    scope.rollback().unwrap();
    digest.sha256
}

/// After-images are the rows as SQLite stored them, each column's affinity
/// applied: a sort field "123" is stored as INTEGER 123, so editing another
/// field does not list `sfld`, and the digest is the stored row's. The same
/// holds for bulk inserts and for other tables' after-images.
#[test]
fn after_images_are_the_rows_as_stored() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");

    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let id = scope.add_note(&note(1, &["123", "b"])).unwrap();
    let card_id = scope.add_card(&card(id, 0)).unwrap();
    let added = note_after(&scope.commit().unwrap().journal.unwrap(), id.0);
    let stored = stored_row(writer, Table::Notes, &format!("id = {}", id.0));
    assert_eq!(stored[7], Cell::Integer(123));
    assert_eq!(added.sha256, digest_when_restored(writer, &clock, &stored));

    clock.step_ms(60_000);
    let mut edited = writer.note(id).unwrap().unwrap();
    assert_eq!(edited.sort_field, "123");
    edited.fields = "123\u{1f}c".to_owned();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope.put(&edited).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    let entry = journal.entry(Table::Notes, &RowKey::Id(id.0)).unwrap();
    assert_eq!(journal.before(entry).unwrap(), Some(stored));
    let digest = note_after(&journal, id.0);
    assert_eq!(digest.changed.names(Table::Notes), ["mod", "flds"]);
    let stored = stored_row(writer, Table::Notes, &format!("id = {}", id.0));
    assert_eq!(digest.sha256, digest_when_restored(writer, &clock, &stored));

    // A row written over a stored one with a value of another storage class.
    let mut card_image = stored_row(writer, Table::Cards, &format!("id = {}", card_id.0));
    card_image[8] = Cell::text("42");
    let mut scope = writer
        .begin_write(StampMode::CallerSupplied, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope
        .restore(Table::Cards, &RowKey::Id(card_id.0), Some(&card_image))
        .unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    let card_stored = stored_row(writer, Table::Cards, &format!("id = {}", card_id.0));
    assert_eq!(card_stored[8], Cell::Integer(42));
    assert_eq!(
        journal.entry(Table::Cards, &RowKey::Id(card_id.0)).unwrap().after(),
        &After::Row(card_stored)
    );

    // Bulk inserts.
    let mut bulk_note = stored.clone();
    bulk_note[0] = Cell::Integer(id.0 + 1000);
    bulk_note[1] = Cell::text("bulk");
    bulk_note[7] = Cell::text("7");
    let mut bulk_revlog = revlog(card_id, 1000).to_cells();
    bulk_revlog[0] = Cell::Integer(5);
    bulk_revlog[3] = Cell::text("3");
    let mut scope = writer
        .begin_write(StampMode::CallerSupplied, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope.insert_rows(Table::Notes, &[bulk_note]).unwrap();
    scope.insert_rows(Table::Revlog, &[bulk_revlog]).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    let note_stored = stored_row(writer, Table::Notes, &format!("id = {}", id.0 + 1000));
    assert_eq!(note_stored[7], Cell::Integer(7));
    assert_eq!(
        note_after(&journal, id.0 + 1000).sha256,
        digest_when_restored(writer, &clock, &note_stored)
    );
    let revlog_stored = stored_row(writer, Table::Revlog, "id = 5");
    assert_eq!(revlog_stored[3], Cell::Integer(3));
    assert_eq!(
        journal.entry(Table::Revlog, &RowKey::Id(5)).unwrap().after(),
        &After::Row(revlog_stored)
    );
}

/// The `tags` key compares with `unicase`: a tag renamed from `Foo` to
/// `foo` and then changed again in one operation is one row, so one entry,
/// whose before-image is the row as it was before the operation. The entry
/// replays by its key both ways.
#[test]
fn a_tag_whose_casing_changes_then_changes_again_has_one_entry() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    scope.put(&tag("Foo")).unwrap();
    scope.commit().unwrap();
    let original = stored_row(writer, Table::Tags, "1");

    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert!(scope.put(&tag("foo")).unwrap().changed());
    let collapsed = Tag {
        collapsed: true,
        ..tag("foo")
    };
    assert!(scope.put(&collapsed).unwrap().changed());
    let journal = scope.commit().unwrap().journal.unwrap();
    let final_row = stored_row(writer, Table::Tags, "1");
    assert_eq!(final_row[0], Cell::text("foo"));

    let tags: Vec<_> = journal.entries().filter(|e| e.table() == Table::Tags).collect();
    assert_eq!(tags.len(), 1, "{tags:?}");
    let entry = tags[0];
    assert_eq!(journal.before(entry).unwrap(), Some(original.clone()));
    assert_eq!(entry.after(), &After::Row(final_row.clone()));
    assert!(journal.entry(Table::Tags, &RowKey::text("FOO")).is_some());

    // Undo and redo by the entry's key.
    let key = entry.key().clone();
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    scope.restore(Table::Tags, &key, Some(&original)).unwrap();
    scope.commit().unwrap();
    assert_eq!(stored_row(writer, Table::Tags, "1"), original);
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    scope.restore(Table::Tags, &key, Some(&final_row)).unwrap();
    scope.commit().unwrap();
    assert_eq!(stored_row(writer, Table::Tags, "1"), final_row);
}

/// Builders racing for one path: exactly one makes the collection, every
/// other is refused, and no refused builder removes the winner's files.
#[test]
fn builders_racing_for_one_path_leave_the_winner_s_collection_intact() {
    const BUILDERS: usize = 8;
    for round in 0..25 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let barrier = Barrier::new(BUILDERS);
        let outcomes: Vec<Result<(), StorageError>> = std::thread::scope(|threads| {
            let (path, barrier) = (&path, &barrier);
            let running: Vec<_> = (0..BUILDERS)
                .map(|i| {
                    threads.spawn(move || {
                        let clock = clock();
                        let random = SeededRandom::new(i as u64);
                        barrier.wait();
                        let builder = CollectionBuilder::create(
                            path,
                            &new_col(NOW_MS),
                            &new_config(),
                            &OpenOptions::new(&clock, &random),
                        )?;
                        let mut scope = builder.begin(StampMode::Raw, &clock)?;
                        let mut given = note(1, &["built", &i.to_string()]);
                        given.id = NoteId(1);
                        scope.put(&given)?;
                        scope.commit()?;
                        builder.finish()?;
                        Ok(())
                    })
                })
                .collect();
            running.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let built = outcomes.iter().filter(|o| o.is_ok()).count();
        assert_eq!(built, 1, "round {round}: {outcomes:?}");
        for outcome in &outcomes {
            if let Err(error) = outcome {
                assert!(
                    matches!(error, StorageError::BuilderRefused { .. }),
                    "round {round}: {error:?}"
                );
            }
        }
        let opened = open(&path).unwrap();
        let built = opened.writer().note(NoteId(1)).unwrap().unwrap();
        assert!(built.fields.starts_with("built\u{1f}"), "round {round}: {built:?}");
    }
}

/// A builder removes only files it made: a refused path keeps the files
/// that were there, and a new file the open gate refuses goes with every
/// file SQLite made next to it.
#[test]
fn a_refused_or_failed_builder_removes_only_the_files_it_made() {
    let clock = clock();
    let random = SeededRandom::new(1);
    let options = OpenOptions::new(&clock, &random);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let wal = dir.path().join("collection.anki2-wal");
    std::fs::write(&wal, b"not ours").unwrap();
    let refused = CollectionBuilder::create(&path, &new_col(NOW_MS), &new_config(), &options).unwrap_err();
    assert_eq!(
        refused,
        StorageError::BuilderRefused {
            reason: BuilderRefusal::Exists
        }
    );
    assert_eq!(
        common::listing(dir.path()).into_iter().collect::<Vec<_>>(),
        ["collection.anki2-wal"]
    );
    assert_eq!(std::fs::read(&wal).unwrap(), b"not ours");

    // No schedVer: the open gate refuses the new file as v1.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let failed = CollectionBuilder::create(&path, &new_col(NOW_MS), &[], &options).unwrap_err();
    assert!(!matches!(failed, StorageError::BuilderRefused { .. }), "{failed:?}");
    assert!(
        common::listing(dir.path()).is_empty(),
        "{:?}",
        common::listing(dir.path())
    );
}

/// Marking many rows with a server usn (S3) holds at most a page of rows:
/// with no journal, no row image is read; with one, before-images spill
/// as they are read. 50,000 notes of 1 KiB fields are about 50 MB.
const MARK_ROWS: i64 = 50_000;
const UNJOURNALLED_MARK_BUDGET: usize = 4 * 1024 * 1024;
const JOURNALLED_MARK_BUDGET: usize = 20 * 1024 * 1024;

#[test]
fn marking_a_large_batch_with_a_usn_holds_no_more_than_a_page_of_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let random = SeededRandom::new(3);
    let builder = CollectionBuilder::create(
        &path,
        &new_col(NOW_MS),
        &new_config(),
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    let field = "f".repeat(1024);
    let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
    for start in (0..MARK_ROWS).step_by(10_000) {
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
    let keys: Vec<RowKey> = (1..=MARK_ROWS).map(RowKey::Id).collect();

    let baseline = HELD.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    assert_eq!(scope.mark_usn(Table::Notes, &keys, Usn(7)).unwrap(), MARK_ROWS as usize);
    let committed = scope.commit().unwrap();
    let peak = PEAK.load(Ordering::Relaxed) - baseline;
    assert!(committed.changed && committed.journal.is_none() && committed.col_mod.is_none());
    assert!(
        peak <= UNJOURNALLED_MARK_BUDGET,
        "peak heap growth {peak} bytes over a budget of {UNJOURNALLED_MARK_BUDGET}"
    );

    let options = JournalOptions {
        spill_threshold: 1024 * 1024,
        spill_dir: Some(dir.path().to_path_buf()),
    };
    let baseline = HELD.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let mut scope = writer
        .begin_write(StampMode::CallerSupplied, &clock, Some(options))
        .unwrap();
    assert_eq!(scope.mark_usn(Table::Notes, &keys, Usn(8)).unwrap(), MARK_ROWS as usize);
    let journal = scope.commit().unwrap().journal.unwrap();
    let peak = PEAK.load(Ordering::Relaxed) - baseline;
    assert_eq!(journal.len(), MARK_ROWS as usize);
    assert!(journal.held_bytes() <= 1024 * 1024, "{}", journal.held_bytes());
    assert!(journal.spilled_bytes() > 40_000_000, "{}", journal.spilled_bytes());
    let last = journal.entry(Table::Notes, &RowKey::Id(MARK_ROWS)).unwrap();
    assert_eq!(journal.before(last).unwrap().unwrap()[4], Cell::Integer(7));
    assert_eq!(note_after(&journal, MARK_ROWS).changed.names(Table::Notes), ["usn"]);
    assert_eq!(
        common::int(&exec(writer, "SELECT count(*) FROM notes WHERE usn = 8")),
        MARK_ROWS
    );
    assert!(
        peak <= JOURNALLED_MARK_BUDGET,
        "peak heap growth {peak} bytes over a budget of {JOURNALLED_MARK_BUDGET}"
    );
}

#[test]
fn two_journals_compose_into_the_journal_of_both_operations() {
    // engine/undo A3: merged note edits keep the first edit's before-images
    // and take the second's after-images.
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope.add_note(&note(1, &["a", "b"])).unwrap();
    let card_id = scope.add_card(&card(id, 0)).unwrap();
    scope.commit().unwrap();
    let original = writer.note(id).unwrap().unwrap();
    let original_card = writer.card(card_id).unwrap().unwrap();

    let edit = |fields: &str, tagged: Option<&str>| {
        clock.step_ms(10_000);
        let mut scope = writer
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        let mut edited = scope.note(id).unwrap().unwrap();
        edited.fields = fields.to_owned();
        scope.put(&edited).unwrap();
        if let Some(name) = tagged {
            scope.put(&tag(name)).unwrap();
        }
        scope.commit().unwrap().journal.unwrap()
    };
    let first = edit("a\u{1f}c", None);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let mut suspended = original_card.clone();
    suspended.queue = CardQueue::Suspended;
    scope.put(&suspended).unwrap();
    let card_journal = scope.commit().unwrap().journal.unwrap();
    let second = edit("x\u{1f}c", Some("later"));
    let col_before = first.before(first.entry(Table::Col, &RowKey::Id(1)).unwrap()).unwrap();

    let both = first.then(&second).unwrap();
    let keys: Vec<(Table, RowKey)> = both.entries().map(|e| (e.table(), e.key().clone())).collect();
    assert_eq!(
        keys,
        [
            (Table::Notes, RowKey::Id(id.0)),
            (Table::Col, RowKey::Id(1)),
            (Table::Tags, RowKey::Text(b"later".to_vec())),
        ],
        "the first journal's keys, then the keys only the second changed"
    );
    let note_entry = both.entry(Table::Notes, &RowKey::Id(id.0)).unwrap();
    assert_eq!(both.before(note_entry).unwrap(), Some(original.to_cells()));
    let After::Note(digest) = note_entry.after() else {
        panic!("not a digest");
    };
    assert_eq!(digest.sha256, note_after(&second, id.0).sha256);
    assert!(
        ["mod", "flds"]
            .iter()
            .all(|c| digest.changed.names(Table::Notes).contains(c)),
        "at least the columns either edit changed"
    );
    let col = both.entry(Table::Col, &RowKey::Id(1)).unwrap();
    assert_eq!(both.before(col).unwrap(), col_before, "col as before the first");
    assert_eq!(col.after(), second.entry(Table::Col, &RowKey::Id(1)).unwrap().after());
    let tag_entry = both.entry(Table::Tags, &RowKey::Text(b"later".to_vec())).unwrap();
    assert!(!tag_entry.existed_before() && tag_entry.exists_after());

    // A key only the first changed keeps its own after-image.
    let with_card = card_journal.then(&second).unwrap();
    let card_entry = with_card.entry(Table::Cards, &RowKey::Id(card_id.0)).unwrap();
    assert_eq!(with_card.before(card_entry).unwrap(), Some(original_card.to_cells()));
    assert_eq!(
        card_entry.after(),
        card_journal
            .entry(Table::Cards, &RowKey::Id(card_id.0))
            .unwrap()
            .after()
    );
    let note_only_later = with_card.entry(Table::Notes, &RowKey::Id(id.0)).unwrap();
    assert_eq!(
        with_card.before(note_only_later).unwrap(),
        second
            .before(second.entry(Table::Notes, &RowKey::Id(id.0)).unwrap())
            .unwrap()
    );
}

#[test]
fn structure_metadata_uses_journal_marks_and_keeps_spilled_images_owned() {
    use meguri_storage::journal::{LayoutReport, StructureReport, StructureReason};
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let clock = clock();
    let mut setup = collection.writer().begin_write(StampMode::Local, &clock, None).unwrap();
    let id = setup.add_notetype(&common::notetype("Report", &["Front", "Back"], &["Card"])).unwrap();
    setup.commit().unwrap();
    let mut scope = collection.writer().begin_write(StampMode::Local, &clock, Some(JournalOptions {
        spill_threshold: 0, spill_dir: path.parent().map(|p| p.to_owned()),
    })).unwrap();
    let outer = scope.savepoint().unwrap();
    let mut nt = scope.notetype(id).unwrap().unwrap();
    let mut report = StructureReport::default();
    report.layouts.insert(id.0, LayoutReport::identity(&nt).unwrap());
    let token = scope.start_structure_report(report).unwrap();
    nt.fields[0].name = "Prompt".into();
    scope.put_notetype(&nt).unwrap();
    scope.complete_structure_report(token).unwrap();
    let inner = scope.savepoint().unwrap();
    let mut reason = StructureReport::default();
    reason.reasons.insert(StructureReason::ApkgApply);
    let token = scope.start_structure_report(reason).unwrap();
    scope.complete_structure_report(token).unwrap();
    scope.release_savepoint(inner).unwrap();
    scope.rollback_to_savepoint(outer).unwrap();
    assert!(scope.journal().unwrap().structure_report().is_none());
    assert_eq!(scope.notetype(id).unwrap().unwrap().fields[0].name, "Front");
    let mut nt = scope.notetype(id).unwrap().unwrap();
    let mut report = StructureReport::default();
    report.layouts.insert(id.0, LayoutReport::identity(&nt).unwrap());
    let token = scope.start_structure_report(report).unwrap();
    nt.fields[0].name = "Final prompt".into();
    scope.put_notetype(&nt).unwrap();
    scope.complete_structure_report(token).unwrap();
    scope.check_structure_report().unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    assert_eq!(journal.held_bytes(), 0);
    assert!(journal.spilled_bytes() > 0);
    let metadata = journal.structure_report_owner();
    let empty = Journal::new(JournalOptions::spilling_into(path.parent().unwrap()));
    let composed = journal.then(&empty).unwrap();
    drop(journal);
    assert_eq!(composed.structure_report(), Some(metadata.as_ref()));
    let entry = composed.entry(Table::Fields, &RowKey::Pair(id.0, 0)).unwrap();
    for _ in 0..2 {
        assert_eq!(composed.before(entry).unwrap().unwrap()[2], Cell::text("Front"));
        assert_eq!(composed.after(entry).unwrap().unwrap()[2], Cell::text("Final prompt"));
    }
}
