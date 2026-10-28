//! Generic raw read dependencies (`model/row-records`, `storage/schema-18`,
//! `engine/change-stream` S2.4, `sync/cloudkit-records` K6.1).
//! Generated storage fixtures only; these are not CloudKit acceptance tests.

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::ids::{CardId, NotetypeId};
use meguri_model::rng::SeededRandom;
use meguri_storage::{
    Cell, CollectionBuilder, OpenOptions, OpenedCollection, ReadRows, ReaderRole, RowImage, RowKey, StampMode, Table,
    WriteScope, open_collection,
};

const NOW: i64 = 1_894_701_600_000;

fn clock() -> FixedClock {
    FixedClock::new(NOW, UtcOffset::UTC)
}

fn collection() -> (tempfile::TempDir, OpenedCollection) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collection.anki2");
    let clock = clock();
    let random = SeededRandom::new(73);
    let options = OpenOptions::new(&clock, &random);
    CollectionBuilder::create(
        &path,
        &Col {
            version: 18,
            ..Col::default()
        },
        &[ConfigEntry {
            key: "schedVer".into(),
            value: b"2".to_vec(),
            ..ConfigEntry::default()
        }],
        &options,
    )
    .unwrap()
    .finish()
    .unwrap();
    (directory, open_collection(&path, &options).unwrap())
}

fn image(table: Table, key: &RowKey) -> RowImage {
    let mut cells: RowImage = table
        .columns()
        .iter()
        .map(|column| match *column {
            "guid" | "name" | "tag" | "KEY" => Cell::text(key.to_string()),
            "tags" | "flds" | "sfld" | "data" | "conf" | "models" | "decks" | "dconf" => Cell::text(""),
            "config" | "common" | "kind" | "val" => Cell::Blob(vec![0xa0, 0x01, 0x07, 0xff]),
            _ => Cell::Integer(0),
        })
        .collect();
    match key {
        RowKey::Id(id) => cells[0] = Cell::Integer(*id),
        RowKey::Pair(first, second) => {
            cells[0] = Cell::Integer(*first);
            cells[1] = Cell::Integer(*second);
        }
        RowKey::Text(bytes) => cells[0] = Cell::Text(bytes.clone()),
    }
    cells
}

fn store(scope: &mut WriteScope<'_>, table: Table, cells: &RowImage) {
    let key = RowKey::from_stored(table, cells).unwrap();
    scope.restore(table, &key, Some(cells)).unwrap();
}

#[test]
fn checked_cursor_covers_every_table_and_rejects_malformed_cells_without_content() {
    for table in Table::ALL {
        let key = match table {
            Table::Config | Table::Tags => RowKey::Text(vec![b'x', 0xff, 0, b'Y']),
            Table::Graves | Table::Fields | Table::Templates => RowKey::Pair(i64::MIN, i64::MAX),
            _ => RowKey::Id(i64::MIN),
        };
        let cells = image(table, &key);
        assert_eq!(RowKey::from_stored(table, &cells).unwrap(), key);
        for width in 0..cells.len() {
            assert!(RowKey::from_stored(table, &cells[..width]).is_err());
        }
        let mut wide = cells.clone();
        wide.push(Cell::Null);
        assert!(RowKey::from_stored(table, &wide).is_err());
        for column in 0..table.key_len() {
            for wrong in [Cell::Null, Cell::Real(1.5), Cell::Blob(b"private-cell".to_vec())] {
                let mut malformed = cells.clone();
                malformed[column] = wrong;
                let error = RowKey::from_stored(table, &malformed).unwrap_err();
                assert_eq!(error.column, table.columns()[column]);
                assert!(!format!("{error:?}").contains("private-cell"));
            }
            let mut malformed = cells.clone();
            malformed[column] = if matches!(table, Table::Config | Table::Tags) {
                Cell::Integer(5)
            } else {
                Cell::text("private-cell")
            };
            assert!(RowKey::from_stored(table, &malformed).is_err());
        }
    }
}

#[test]
fn extracted_cursors_continue_the_existing_pager_with_stored_keys_and_collation() {
    let (_directory, collection) = collection();
    let clock = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &clock, None)
        .unwrap();
    for table in Table::ALL.into_iter().filter(|table| *table != Table::Col) {
        let keys = match table {
            Table::Config | Table::Tags => vec![RowKey::text("Straße"), RowKey::Text(vec![0xff, b'z'])],
            Table::Graves | Table::Fields | Table::Templates => {
                vec![RowKey::Pair(-12, i64::MIN), RowKey::Pair(-12, i64::MAX)]
            }
            _ => vec![RowKey::Id(i64::MIN), RowKey::Id(i64::MAX)],
        };
        for key in keys {
            store(&mut scope, table, &image(table, &key));
        }
    }
    scope.commit().unwrap();
    let reader = collection.open_reader(ReaderRole::Cloud).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    for table in Table::ALL {
        let first = snapshot.row_page(table, None).unwrap();
        assert!(!first.is_empty());
        for (index, cells) in first.iter().enumerate() {
            let cursor = RowKey::from_stored(table, cells).unwrap();
            assert_eq!(snapshot.row_page(table, Some(&cursor)).unwrap(), first[index + 1..]);
            assert_eq!(snapshot.row_image(table, &cursor).unwrap().as_ref(), Some(cells));
        }
    }
    // The extracted spelling remains exact, while continuation uses unicase.
    let key = RowKey::text("Straße");
    let stored = snapshot
        .row_image(Table::Tags, &RowKey::text("STRASSE"))
        .unwrap()
        .unwrap();
    assert_eq!(RowKey::from_stored(Table::Tags, &stored).unwrap(), key);
    assert_eq!(
        snapshot.row_page(Table::Tags, Some(&key)).unwrap(),
        snapshot.row_page(Table::Tags, Some(&RowKey::text("STRASSE"))).unwrap(),
    );
    drop(snapshot);
    drop(reader);
    collection.close().unwrap();
}

fn part(table: Table, parent: NotetypeId, ord: i64) -> RowImage {
    let mut cells = image(table, &RowKey::Pair(parent.0, ord));
    let mut opaque = format!("part {ord}").into_bytes();
    opaque.extend_from_slice(&[0, 0xff]);
    cells[2] = Cell::Text(opaque);
    let config = cells.len() - 1;
    if ord % 2 == 0 {
        cells[config] = Cell::Text(vec![0xff, 0, 0xa0, 1, 7]);
    }
    cells
}

fn parts(read: &impl ReadRows, table: Table, parent: NotetypeId, after: Option<i64>) -> Vec<RowImage> {
    match table {
        Table::Fields => read.notetype_fields_page(parent, after).unwrap(),
        Table::Templates => read.notetype_templates_page(parent, after).unwrap(),
        _ => unreachable!(),
    }
}

#[test]
fn raw_parts_page_by_actual_ordinal_and_keep_snapshot_bytes_after_commits() {
    let (_directory, collection) = collection();
    let clock = clock();
    let parent = NotetypeId(10);
    let other = NotetypeId(20);
    let missing = NotetypeId(30);
    let mut ordinals = vec![i64::MIN, -7, 0];
    ordinals.extend((1_i64..250).map(|index| index * 4));
    ordinals.push(i64::MAX);
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &clock, None)
        .unwrap();
    for id in [parent, other] {
        store(
            &mut scope,
            Table::Notetypes,
            &image(Table::Notetypes, &RowKey::Id(id.0)),
        );
    }
    for table in [Table::Fields, Table::Templates] {
        for ord in &ordinals {
            store(&mut scope, table, &part(table, parent, *ord));
        }
        store(&mut scope, table, &part(table, other, 0));
        // No foreign key enforces the parent; unknown parents still return empty.
        store(&mut scope, table, &part(table, missing, 0));
    }
    scope.commit().unwrap();
    let reader = collection.open_reader(ReaderRole::Cloud).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();

    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &clock, None)
        .unwrap();
    for table in [Table::Fields, Table::Templates] {
        scope.restore(table, &RowKey::Pair(parent.0, -7), None).unwrap();
        store(&mut scope, table, &part(table, parent, 1));
        let mut changed = part(table, parent, 0);
        let config = changed.len() - 1;
        changed[config] = Cell::Blob(b"later".to_vec());
        store(&mut scope, table, &changed);
    }
    scope.commit().unwrap();

    for table in [Table::Fields, Table::Templates] {
        let mut after = None;
        let mut found = 0;
        loop {
            let page = parts(&snapshot, table, parent, after);
            assert!(page.len() <= 250);
            if page.is_empty() {
                break;
            }
            if after.is_none() {
                assert_eq!(page.len(), 250);
            }
            for cells in &page {
                let ord = ordinals[found];
                assert_eq!(*cells, part(table, parent, ord));
                assert_eq!(RowKey::from_stored(table, cells).unwrap(), RowKey::Pair(parent.0, ord));
                found += 1;
                after = Some(ord);
            }
        }
        assert_eq!(found, ordinals.len());
        assert_eq!(parts(&snapshot, table, other, None), vec![part(table, other, 0)]);
        assert!(parts(&snapshot, table, missing, None).is_empty());
        assert!(parts(&snapshot, table, parent, Some(i64::MAX)).is_empty());
        assert_eq!(
            parts(collection.writer(), table, parent, Some(0))[0],
            part(table, parent, 1)
        );
    }
    drop(snapshot);
    for table in [Table::Fields, Table::Templates] {
        assert_eq!(parts(&reader, table, parent, Some(0))[0], part(table, parent, 1));
    }
    drop(reader);
    collection.close().unwrap();
}

fn log(id: i64, card: CardId, kind: i64, ease: i64) -> RowImage {
    let mut cells = image(Table::Revlog, &RowKey::Id(id));
    cells[1] = Cell::Integer(card.0);
    cells[3] = Cell::Integer(ease);
    cells[7] = Cell::Text(vec![0xff]);
    cells[8] = Cell::Integer(kind);
    cells
}

#[test]
fn maximum_revlog_id_is_unfiltered_signed_milliseconds_on_the_read_view() {
    let (_directory, collection) = collection();
    let clock = clock();
    let card = CardId(41);
    let negative = CardId(42);
    let empty = CardId(43);
    for kind in 0_i64..=5 {
        let id = NOW + 123 + kind;
        let mut scope = collection
            .writer()
            .begin_write(StampMode::CallerSupplied, &clock, None)
            .unwrap();
        // Includes manual/rescheduled ease 0 and filtered type 3 with factor 0.
        store(
            &mut scope,
            Table::Revlog,
            &log(id, card, kind, if kind >= 4 { 0 } else { 3 }),
        );
        assert_eq!(scope.max_revlog_id(card).unwrap(), id);
        scope.commit().unwrap();
        assert_eq!(collection.writer().max_revlog_id(card).unwrap(), id);
    }
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &clock, None)
        .unwrap();
    store(&mut scope, Table::Revlog, &log(i64::MIN, negative, 4, 0));
    store(&mut scope, Table::Revlog, &log(-1, negative, 5, 0));
    scope.commit().unwrap();
    let reader = collection.open_reader(ReaderRole::Cloud).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    assert_eq!(snapshot.max_revlog_id(card).unwrap(), NOW + 128);
    assert_eq!(snapshot.max_revlog_id(negative).unwrap(), -1);
    assert_eq!(snapshot.max_revlog_id(empty).unwrap(), 0);
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &clock, None)
        .unwrap();
    store(&mut scope, Table::Revlog, &log(NOW + 999, card, 255, 0));
    store(&mut scope, Table::Revlog, &log(i64::MAX, CardId(99), 1, 4));
    store(&mut scope, Table::Revlog, &log(7, empty, 3, 0));
    scope.commit().unwrap();
    assert_eq!(snapshot.max_revlog_id(card).unwrap(), NOW + 128);
    assert_eq!(snapshot.max_revlog_id(empty).unwrap(), 0);
    assert_eq!(collection.writer().max_revlog_id(card).unwrap(), NOW + 999);
    assert_eq!(collection.writer().max_revlog_id(CardId(99)).unwrap(), i64::MAX);
    drop(snapshot);
    assert_eq!(reader.max_revlog_id(card).unwrap(), NOW + 999);
    assert_eq!(reader.max_revlog_id(empty).unwrap(), 7);
    drop(reader);
    collection.close().unwrap();
}
