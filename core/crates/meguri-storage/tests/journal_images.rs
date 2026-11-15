//! Both sides, savepoint replacements and composition share one image budget.
mod common;

use common::{card, clock, empty_collection, note, open};
use meguri_storage::journal::{LayoutReport, StructureReport};
use meguri_storage::{Cell, JournalOptions, ReadRows, Record, RowKey, StampMode, StorageError, Table};
use std::sync::Arc;

fn data(width: usize, value: char) -> Cell {
    Cell::text(format!("{{\"payload\":\"{}\"}}", value.to_string().repeat(width)))
}

// Seed deletion cases without a journal so a failed journalled insertion
// cannot hide rejection of the same opaque before-image during deletion.
fn raw_layout_image_round_trip(table: Table, journal: Option<JournalOptions>, seed_without_journal: bool) {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let (key, image) = match table {
        Table::Notetypes => (
            RowKey::Id(100),
            vec![Cell::Integer(100), Cell::text("Raw"), Cell::Integer(0), Cell::Integer(0), Cell::Blob(vec![0x80])],
        ),
        Table::Fields => {
            let mut scope = writer.begin_write(StampMode::Raw, &clock, None).unwrap();
            let id = scope.add_notetype(&common::notetype("Raw field parent", &["Ordinary"], &["View"])).unwrap();
            let mut image = scope.row_image(Table::Fields, &RowKey::Pair(id.0, 0)).unwrap().unwrap();
            // ddl18.sql permits any non-null INTEGER ordinal. Keep the
            // ordinary stored config and use a distinct name for its index.
            image[1] = Cell::Integer(-7);
            image[2] = Cell::text("Negative ordinal");
            scope.commit().unwrap();
            (RowKey::Pair(id.0, -7), image)
        }
        _ => unreachable!("only raw layout images are covered here"),
    };
    assert!(writer.row_image(table, &key).unwrap().is_none());
    let insertion_journal = if seed_without_journal { None } else { journal.clone() };
    let mut scope = writer.begin_write(StampMode::Raw, &clock, insertion_journal).unwrap();
    scope.restore(table, &key, Some(&image)).expect("raw restore accepts the opaque stored image");
    assert_eq!(scope.row_image(table, &key).unwrap().as_ref(), Some(&image));
    scope.commit().unwrap();
    assert_eq!(writer.row_image(table, &key).unwrap().as_ref(), Some(&image));

    let mut scope = writer.begin_write(StampMode::Raw, &clock, journal).unwrap();
    scope.restore(table, &key, None).expect("raw deletion accepts the opaque stored before-image");
    assert!(scope.row_image(table, &key).unwrap().is_none());
    scope.commit().unwrap();
    assert!(writer.row_image(table, &key).unwrap().is_none());
}

#[test]
fn raw_notetype_restore_without_journal_keeps_malformed_protobuf_opaque() {
    raw_layout_image_round_trip(Table::Notetypes, None, false);
}

#[test]
fn raw_notetype_restore_with_journal_keeps_malformed_protobuf_opaque() {
    raw_layout_image_round_trip(Table::Notetypes, Some(JournalOptions::in_memory()), false);
}

#[test]
fn raw_notetype_delete_without_journal_keeps_malformed_protobuf_opaque() {
    raw_layout_image_round_trip(Table::Notetypes, None, true);
}

#[test]
fn raw_notetype_delete_with_journal_keeps_malformed_protobuf_opaque() {
    raw_layout_image_round_trip(Table::Notetypes, Some(JournalOptions::in_memory()), true);
}

#[test]
fn raw_field_restore_without_journal_preserves_negative_ordinal() {
    raw_layout_image_round_trip(Table::Fields, None, false);
}

#[test]
fn raw_field_restore_with_journal_preserves_negative_ordinal() {
    raw_layout_image_round_trip(Table::Fields, Some(JournalOptions::in_memory()), false);
}

#[test]
fn raw_field_delete_without_journal_accepts_negative_ordinal() {
    raw_layout_image_round_trip(Table::Fields, None, true);
}

#[test]
fn raw_field_delete_with_journal_accepts_negative_ordinal() {
    raw_layout_image_round_trip(Table::Fields, Some(JournalOptions::in_memory()), true);
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
    let identity_report = |nt: &meguri_model::notetype::Notetype| StructureReport {
        layouts: [(id.0, LayoutReport::identity(nt).unwrap())].into(),
        ..StructureReport::default()
    };
    let mark = scope.savepoint().unwrap();
    let report = scope.start_structure_report(identity_report(&original)).unwrap();
    let mut edited = original.clone();
    edited.name = "Renamed ordinal report fixture".into();
    scope.put_notetype(&edited).unwrap();
    scope.complete_structure_report(report).unwrap();
    let checked = scope.journal().unwrap().identity_layout(id.0).unwrap();
    assert_eq!(checked.field_map, [0, 1]);
    assert_eq!(checked.template_map, [0]);
    scope.rollback_to_savepoint(mark).unwrap();
    assert!(scope.journal().unwrap().identity_layout(id.0).is_none());
    assert_eq!(stored_notetype(&scope), original_rows);

    let report = scope.start_structure_report(identity_report(&original)).unwrap();
    scope.put_notetype(&edited).unwrap();
    scope.complete_structure_report(report).unwrap();
    let edited_rows = stored_notetype(&scope);
    let mark = scope.savepoint().unwrap();
    let mut raw = edited.clone();
    raw.config.css = "raw modification".into();
    scope.put_notetype(&raw).unwrap();
    assert!(scope.journal().unwrap().identity_layout(id.0).is_none());
    // A later checked edit cannot bless the earlier raw write in this scope.
    let denied = scope.start_structure_report(identity_report(&raw));
    assert!(matches!(denied, Err(StorageError::StructureNotReported { notetype_id }) if notetype_id == id));
    raw.name = "Later checked edit".into();
    scope.put_notetype(&raw).unwrap();
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

#[test]
fn accepted_sync_bracket_savepoints_restore_maps_images_and_source_receipt() {
    use meguri_model::changes::OpSource;
    let (dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Raw, &clock, None).unwrap();
    let id = scope.add_notetype(&common::notetype("Incoming journal", &["Left", "Right"], &["View"])).unwrap();
    scope.commit().unwrap();
    let original = writer.notetype(id).unwrap().unwrap();
    let original_head = writer.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap();
    let options = JournalOptions { spill_threshold: 1, spill_dir: Some(dir.path().to_path_buf()) };
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, Some(options)).unwrap();
    let mut native = original.clone();
    native.name = "Native before incoming".into();
    let report = StructureReport { layouts: [(id.0, LayoutReport::identity(&original).unwrap())].into(), ..Default::default() };
    let token = scope.start_structure_report(report.clone()).unwrap();
    scope.put_notetype(&native).unwrap();
    scope.complete_structure_report(token).unwrap();
    let old_head = scope.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap();
    let old_fields = scope.notetype_fields_page(id, None).unwrap();
    let mark = scope.savepoint().unwrap();
    let mut incoming = native.clone();
    incoming.fields[0].name = "Incoming left".into();
    scope.put_accepted_sync_notetype(&incoming).unwrap();
    assert!(scope.journal().unwrap().structure_report().is_none());
    assert!(scope.journal().unwrap().identity_layout(id.0).is_none());
    assert!(scope.check_accepted_sync_source(OpSource::Maintenance).is_err());
    scope.rollback_to_savepoint(mark).unwrap();
    assert_eq!(scope.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap(), old_head);
    assert_eq!(scope.notetype_fields_page(id, None).unwrap(), old_fields);
    assert_eq!(scope.journal().unwrap().structure_report(), Some(&report));
    assert!(scope.journal().unwrap().identity_layout(id.0).is_some());
    scope.check_accepted_sync_source(OpSource::Maintenance).unwrap();

    // A SQL failure halfway through the aggregate restores the prior rows,
    // report and receipt, even if its caller handles the error and continues.
    let mut invalid = native.clone();
    invalid.fields[1].name = invalid.fields[0].name.clone();
    assert!(scope.put_accepted_sync_notetype(&invalid).is_err());
    assert_eq!(scope.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap(), old_head);
    assert_eq!(scope.notetype_fields_page(id, None).unwrap(), old_fields);
    assert_eq!(scope.journal().unwrap().structure_report(), Some(&report));
    scope.check_accepted_sync_source(OpSource::Maintenance).unwrap();

    scope.put_accepted_sync_notetype(&incoming).unwrap();
    let incoming_fields = scope.notetype_fields_page(id, None).unwrap();
    let mark = scope.savepoint().unwrap();
    scope.put_notetype(&native).unwrap();
    scope.rollback_to_savepoint(mark).unwrap();
    assert_eq!(scope.notetype_fields_page(id, None).unwrap(), incoming_fields);
    scope.check_accepted_sync_source(OpSource::SyncIncoming).unwrap();
    assert!(scope.check_accepted_sync_source(OpSource::Maintenance).is_err());
    assert!(matches!(scope.start_structure_report(report), Err(StorageError::StructureNotReported { notetype_id }) if notetype_id == id));
    assert!(scope.journal().unwrap().held_bytes() <= 1);
    // The storage suite checks journal ownership, not executor acceptance.
    // Source/StructureNotReported enforcement is exercised through engine ops.
    scope.rollback().unwrap();
    assert_eq!(writer.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap(), original_head);
    assert_eq!(writer.notetype_fields_page(id, None).unwrap(), old_fields);
}
