//! Aggregate child membership and per-row journals with legacy ordinal gaps
//! (`notetypes/records` N1/N3; `notetypes/field-rename` F4).

mod common;

use common::{NOW_MS, empty_collection, notetype, open};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_model::proto::{BlobMessage, ProtoBlob};
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::journal::After;
use meguri_storage::{JournalOptions, ReadRows, RowImage, RowKey, StampMode, Table, Writer, Written};

fn unknown<M: BlobMessage>(blob: &ProtoBlob<M>) -> ProtoBlob<M> {
    let mut bytes = blob.to_bytes().into_owned();
    bytes.extend_from_slice(&[0xa0, 0x01, 0x07]);
    ProtoBlob::decode(&bytes).unwrap()
}

fn aggregate(id: i64) -> Notetype {
    let mut nt = notetype(
        &format!("Gapped {id}"),
        &["Absent", "Kept", "First", "Second"],
        &["Absent", "Kept", "First", "Second"],
    );
    nt.id = NotetypeId(id);
    nt.mtime = EpochSecs(42);
    nt.usn = Usn(7);
    nt.config = unknown(&nt.config);
    for (index, (field, template)) in nt.fields.iter_mut().zip(&mut nt.templates).enumerate() {
        let ord = [1, 4, 8, 13][index];
        field.ord = ord;
        field.config.id = if index == 1 { None } else { Some(-17) };
        field.config.prevent_deletion = true;
        field.config = unknown(&field.config);
        template.ord = ord;
        template.config.id = if index == 1 { None } else { Some(-29) };
        template.config.q_format = format!("front {ord}");
        template.config = unknown(&template.config);
        template.mtime = EpochSecs(321);
        template.usn = Usn(19);
    }
    nt
}

fn images(writer: &Writer, id: NotetypeId) -> Vec<(Table, RowKey, RowImage)> {
    let nt = writer.notetype(id).unwrap().unwrap();
    let mut keys = vec![(Table::Notetypes, RowKey::Id(id.0))];
    keys.extend(
        nt.fields
            .iter()
            .map(|f| (Table::Fields, RowKey::Pair(id.0, i64::from(f.ord)))),
    );
    keys.extend(
        nt.templates
            .iter()
            .map(|t| (Table::Templates, RowKey::Pair(id.0, i64::from(t.ord)))),
    );
    keys.into_iter()
        .map(|(table, key)| {
            let image = writer.row_image(table, &key).unwrap().unwrap();
            (table, key, image)
        })
        .collect()
}

#[test]
fn unchanged_gapped_aggregate_keeps_all_rows_stamps_and_empty_journal() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let nt = aggregate(1);
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    scope.put_notetype(&nt).unwrap();
    scope.commit().unwrap();
    let before = images(writer, nt.id);
    let before_col = writer.col().unwrap();
    clock.step_ms(60_000);
    let loaded = writer.notetype(nt.id).unwrap().unwrap();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.put_notetype(&loaded).unwrap(), Written::Unchanged);
    let committed = scope.commit().unwrap();
    assert!(!committed.changed);
    assert!(committed.col_mod.is_none());
    assert!(committed.journal.unwrap().is_empty());
    assert_eq!(images(writer, nt.id), before);
    assert_eq!(writer.col().unwrap(), before_col);
}

#[test]
fn gapped_swaps_remove_only_absent_keys_and_journal_stored_images_for_this_aggregate() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let nt = aggregate(1);
    let other = aggregate(2);
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    scope.put_notetype(&nt).unwrap();
    scope.put_notetype(&other).unwrap();
    scope.commit().unwrap();
    let before = images(writer, nt.id);
    let other_before = images(writer, other.id);
    let before_col = writer.col().unwrap();
    let mut edited = writer.notetype(nt.id).unwrap().unwrap();
    // The absent key is below the new length; the unchanged key is above it.
    edited.fields.remove(0);
    edited.templates.remove(0);
    edited.fields[1].name = "Second".to_owned();
    edited.fields[2].name = "First".to_owned();
    edited.templates[1].name = "Second".to_owned();
    edited.templates[2].name = "First".to_owned();
    clock.step_ms(60_000);

    // Even after delete-before-insert swaps, dropping the scope is atomic.
    {
        let mut scope = writer
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        assert_eq!(scope.put_notetype(&edited).unwrap(), Written::Updated);
    }
    assert_eq!(images(writer, nt.id), before);
    assert_eq!(writer.col().unwrap(), before_col);

    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.put_notetype(&edited).unwrap(), Written::Updated);
    let committed = scope.commit().unwrap();
    assert!(committed.changed);
    assert_eq!(committed.col_mod, Some(EpochMillis(NOW_MS + 60_000)));
    let journal = committed.journal.unwrap();
    assert_eq!(journal.len(), 8, "six changed children, parent and col");
    let stored = writer.notetype(nt.id).unwrap().unwrap();
    edited.mtime = EpochSecs((NOW_MS + 60_000) / 1000);
    edited.usn = Usn::PENDING;
    assert_eq!(stored, edited);
    assert_eq!(stored.config.to_bytes(), edited.config.to_bytes());
    for (actual, expected) in stored.fields.iter().zip(&edited.fields) {
        assert_eq!(actual.config.to_bytes(), expected.config.to_bytes());
    }
    for (actual, expected) in stored.templates.iter().zip(&edited.templates) {
        assert_eq!(actual.config.to_bytes(), expected.config.to_bytes());
    }
    assert_eq!(images(writer, other.id), other_before);
    let mut expected_col = before_col;
    expected_col.modified = EpochMillis(NOW_MS + 60_000);
    assert_eq!(writer.col().unwrap(), expected_col);
    for table in [Table::Fields, Table::Templates] {
        let mut changed: Vec<_> = journal
            .entries()
            .filter(|entry| entry.table() == table)
            .map(|entry| entry.key().clone())
            .collect();
        changed.sort();
        assert_eq!(
            changed,
            [
                RowKey::Pair(nt.id.0, 1),
                RowKey::Pair(nt.id.0, 8),
                RowKey::Pair(nt.id.0, 13)
            ]
        );
        assert!(writer.row_image(table, &RowKey::Pair(nt.id.0, 1)).unwrap().is_none());
    }
    for entry in journal.entries().filter(|entry| entry.table() != Table::Col) {
        let original = before
            .iter()
            .find(|(table, key, _)| *table == entry.table() && key == entry.key())
            .unwrap();
        assert_eq!(journal.before(entry).unwrap().as_ref(), Some(&original.2));
        match writer.row_image(entry.table(), entry.key()).unwrap() {
            Some(image) => assert_eq!(entry.after(), &After::Row(image)),
            None => assert_eq!(entry.after(), &After::Absent),
        }
    }

    // Repeating the final definition neither restamps nor journals anything.
    let after = images(writer, nt.id);
    clock.step_ms(60_000);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.put_notetype(&stored).unwrap(), Written::Unchanged);
    let committed = scope.commit().unwrap();
    assert!(!committed.changed && committed.col_mod.is_none());
    assert!(committed.journal.unwrap().is_empty());
    assert_eq!(images(writer, nt.id), after);
    assert_eq!(images(writer, other.id), other_before);
    assert_eq!(writer.col().unwrap(), expected_col);
}
