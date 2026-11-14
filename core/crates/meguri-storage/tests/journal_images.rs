//! Both sides, savepoint replacements and composition share one image budget.
mod common;

use common::{card, clock, empty_collection, note, open};
use meguri_storage::{Cell, JournalOptions, ReadRows, Record, RowKey, StampMode, StorageError, Table};
use std::sync::Arc;

fn data(width: usize, value: char) -> Cell {
    Cell::text(format!("{{\"payload\":\"{}\"}}", value.to_string().repeat(width)))
}

#[test]
fn checked_ordinal_reports_follow_savepoints_and_raw_write_invalidation() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Raw, &clock, None).unwrap();
    let mut original = common::notetype("Ordinal report fixture", &["Left", "Right"], &["View"]);
    let id = scope.add_notetype(&original).unwrap();
    original = scope.notetype(id).unwrap().unwrap();
    scope.commit().unwrap();

    let mut scope = writer.begin_write(StampMode::Raw, &clock, Some(JournalOptions::in_memory())).unwrap();
    let stored_notetype = |scope: &meguri_storage::WriteScope<'_>| {
        let head = scope.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap();
        let parts = [Table::Fields, Table::Templates].map(|table| {
            let mut rows = Vec::new();
            let mut after = None;
            loop {
                let page = scope.notetype_parts_raw_page(table, id, after.as_ref()).unwrap();
                let Some(last) = page.last() else {
                    break;
                };
                after = Some(last[1].clone());
                rows.extend(page);
            }
            rows
        });
        (head, parts)
    };
    let original_rows = stored_notetype(&scope);
    let mark = scope.savepoint().unwrap();
    let report = scope.start_ordinal_report(id, (original.fields.len(), original.templates.len()));
    let mut edited = original.clone();
    edited.name = "Renamed ordinal report fixture".into();
    scope.put_notetype(&edited).unwrap();
    scope.complete_ordinal_report(report);
    let checked = scope.journal().unwrap().identity_layout(id.0).unwrap();
    assert_eq!(checked.field_map, [0, 1]);
    assert_eq!(checked.template_map, [0]);
    scope.rollback_to_savepoint(mark).unwrap();
    assert!(scope.journal().unwrap().identity_layout(id.0).is_none());
    assert_eq!(stored_notetype(&scope), original_rows);

    let report = scope.start_ordinal_report(id, (original.fields.len(), original.templates.len()));
    scope.put_notetype(&edited).unwrap();
    scope.complete_ordinal_report(report);
    let edited_rows = stored_notetype(&scope);
    let mark = scope.savepoint().unwrap();
    let mut raw = edited.clone();
    raw.config.css = "raw modification".into();
    scope.put_notetype(&raw).unwrap();
    assert!(scope.journal().unwrap().identity_layout(id.0).is_none());
    // A later checked edit cannot bless the earlier raw write in this scope.
    let denied = scope.start_ordinal_report(id, (raw.fields.len(), raw.templates.len()));
    raw.name = "Later checked edit".into();
    scope.put_notetype(&raw).unwrap();
    scope.complete_ordinal_report(denied);
    assert!(scope.journal().unwrap().identity_layout(id.0).is_none());
    scope.rollback_to_savepoint(mark).unwrap();
    let checked = scope.journal().unwrap().identity_layout(id.0).unwrap();
    assert_eq!(checked.field_map, [0, 1]);
    assert_eq!(checked.template_map, [0]);
    assert_eq!(stored_notetype(&scope), edited_rows);
    let mark = scope.savepoint().unwrap();
    raw.config.css = "released raw modification".into();
    scope.put_notetype(&raw).unwrap();
    scope.release_savepoint(mark).unwrap();
    assert!(scope.commit().unwrap().journal.unwrap().identity_layout(id.0).is_none());
}

#[test]
fn nested_replacements_release_rollback_and_composition_preserve_both_sides() {
    let (dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Raw, &clock, None).unwrap();
    let nid = scope.add_note(&note(1, &["front", "back"])).unwrap();
    let cid = scope.add_card(&card(nid, 0)).unwrap();
    let mut original = scope.card(cid).unwrap().unwrap().to_cells();
    original[17] = data(400, 'a');
    scope
        .restore(Table::Cards, &RowKey::Id(cid.0), Some(&original))
        .unwrap();
    scope.commit().unwrap();
    let key = RowKey::Id(cid.0);
    let options = JournalOptions {
        spill_threshold: 4096,
        spill_dir: Some(dir.path().into()),
    };
    let mut scope = writer
        .begin_write(StampMode::Raw, &clock, Some(options.clone()))
        .unwrap();
    let mut first = original.clone();
    first[17] = data(600, 'b');
    scope.restore(Table::Cards, &key, Some(&first)).unwrap();
    let held = scope.journal().unwrap().held_bytes();
    let spilled = scope.journal().unwrap().spilled_bytes();
    let outer = scope.savepoint().unwrap();
    for i in 0..4 {
        let inner = scope.savepoint().unwrap();
        let mut next = first.clone();
        next[17] = data(700 + i, 'c');
        scope.restore(Table::Cards, &key, Some(&next)).unwrap();
        assert!(scope.journal().unwrap().held_bytes() <= 4096);
        scope.release_savepoint(inner).unwrap();
    }
    assert!(scope.journal().unwrap().spilled_bytes() > spilled);
    scope.rollback_to_savepoint(outer).unwrap();
    let journal = scope.journal().unwrap();
    assert_eq!(journal.held_bytes(), held);
    assert_eq!(journal.spilled_bytes(), spilled);
    let entry = journal.entry(Table::Cards, &key).unwrap();
    assert_eq!(journal.before(entry).unwrap(), Some(original.clone()));
    assert_eq!(journal.after(entry).unwrap(), Some(first.clone()));
    // Append after rollback; no live extent can be overwritten.
    let mark = scope.savepoint().unwrap();
    let mut ignored = first.clone();
    ignored[17] = data(8000, 'z');
    scope.restore(Table::Cards, &key, Some(&ignored)).unwrap();
    scope.rollback_to_savepoint(mark).unwrap();
    let first_journal = scope.commit().unwrap().journal.unwrap();
    let mut second = first.clone();
    second[17] = data(7000, 'd');
    let mut scope = writer.begin_write(StampMode::Raw, &clock, Some(options)).unwrap();
    scope.restore(Table::Cards, &key, Some(&second)).unwrap();
    let second_journal = scope.commit().unwrap().journal.unwrap();
    let merged = Arc::new(first_journal.then(&second_journal).unwrap());
    assert!(merged.held_bytes() <= 4096);
    drop(first_journal);
    drop(second_journal);
    drop(collection);
    let reads = merged.image_reads();
    assert_eq!(merged.entries().map(|e| e.key()).collect::<Vec<_>>(), [&key]);
    assert_eq!(merged.image_reads(), reads, "enumeration reads no payload");
    std::thread::scope(|threads| {
        for _ in 0..4 {
            let merged = merged.clone();
            let (original, second, key) = (&original, &second, &key);
            threads.spawn(move || {
                for _ in 0..2 {
                    let entry = merged.entry(Table::Cards, key).unwrap();
                    assert_eq!(merged.before(entry).unwrap().as_ref(), Some(original));
                    assert_eq!(merged.after(entry).unwrap().as_ref(), Some(second));
                }
            });
        }
    });
    let last = Arc::downgrade(&merged);
    drop(merged);
    assert!(last.upgrade().is_none(), "the last owner releases unnamed backing");
}

#[test]
fn an_oversize_row_spills_on_both_sides_and_read_failures_are_not_absence() {
    for fault in 0..3 {
        let (dir, path) = empty_collection();
        let collection = open(&path).unwrap();
        let writer = collection.writer();
        let clock = clock();
        let key = RowKey::text("large");
        let mut scope = writer.begin_write(StampMode::Raw, &clock, None).unwrap();
        scope
            .set_config(
                "large",
                &meguri_storage::ConfigValue::String("a".repeat(9 * 1024 * 1024)),
            )
            .unwrap();
        scope.commit().unwrap();
        let before = writer.row_image(Table::Config, &key).unwrap().unwrap();
        let mut after = before.clone();
        after[3] = Cell::Blob(format!("\"{}\"", "b".repeat(9 * 1024 * 1024)).into_bytes());
        let mut scope = writer
            .begin_write(StampMode::Raw, &clock, Some(JournalOptions::spilling_into(dir.path())))
            .unwrap();
        scope.restore(Table::Config, &key, Some(&after)).unwrap();
        let journal = scope.commit().unwrap().journal.unwrap();
        assert_eq!(journal.held_bytes(), 0, "oversized images must use spill storage");
        assert!(journal.spilled_bytes() > 18 * 1024 * 1024);
        let entry = journal.entry(Table::Config, &key).unwrap();
        assert_eq!(journal.before(entry).unwrap(), Some(before));
        assert_eq!(journal.after(entry).unwrap(), Some(after));
        match fault {
            0 => journal.fail_image_reads(true),
            1 => journal.truncate_backing().unwrap(),
            _ => journal.corrupt_backing().unwrap(),
        }
        if fault == 2 {
            assert!(journal.after(entry).is_err(), "the first extent holds the after-image");
            assert!(journal.before(entry).is_ok(), "corruption is local to its own extent");
        } else {
            assert!(matches!(journal.before(entry), Err(StorageError::Io { .. })));
            assert!(journal.after(entry).is_err());
        }
    }
}

#[test]
fn commit_preparation_sees_final_stamp_and_failure_rolls_back_everything() {
    let (dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let before = writer.col().unwrap();
    let clock = meguri_model::clock::FixedClock::new(common::NOW_MS + 10_000, meguri_model::clock::UtcOffset::UTC);
    let mut scope = writer
        .begin_write(
            StampMode::Local,
            &clock,
            Some(JournalOptions::spilling_into(dir.path())),
        )
        .unwrap();
    scope
        .set_config("rollback", &meguri_storage::ConfigValue::Integer(1))
        .unwrap();
    let failure = scope.commit_with(|journal, col_mod| {
        let journal = journal.unwrap();
        let entry = journal.entry(Table::Col, &RowKey::Id(1)).unwrap();
        let col = meguri_model::col::Col::from_cells(&journal.after(entry)?.unwrap())?;
        assert_ne!(col.modified, before.modified);
        assert_eq!(col_mod, Some(col.modified));
        assert_eq!(col.modified.0, common::NOW_MS + 10_000);
        Err::<(), StorageError>(StorageError::Io {
            detail: "metadata failed".into(),
        })
    });
    assert!(failure.is_err());
    assert_eq!(writer.col().unwrap(), before);
    assert!(writer.config_entry("rollback").unwrap().is_none());
}
