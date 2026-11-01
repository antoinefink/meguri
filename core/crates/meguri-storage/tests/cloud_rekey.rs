//! Synthetic storage-only K14.2/I5 reference writers; no tracking fixture.
mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{NOW_MS, card, clock, deck, new_col, new_config, note, open, preset, revlog};
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckKind, DeckKindMessage, FilteredDeck};
use meguri_model::proto::notetype::TemplateConfig;
use meguri_model::rng::SeededRandom;
use meguri_storage::journal::{After, Journal, digest};
use meguri_storage::{
    Cell, CollectionBuilder, JournalOptions, OpenOptions, ReadRows, Record, RowImage, RowKey, StampMode, StorageError,
    Table, WriteScope,
};

const OLD: i64 = 100;
const NEW: i64 = 102;
const CAP: i64 = NOW_MS + 86_400_000;
const KINDS: [Table; 6] = [
    Table::Notes,
    Table::Cards,
    Table::Revlog,
    Table::Decks,
    Table::DeckConfig,
    Table::Notetypes,
];
type Rows = BTreeMap<(Table, RowKey), RowImage>;

// Compare every key and cell without printing config values or a whole map.
fn assert_row(table: Table, index: usize, actual: Option<&RowImage>, expected: Option<&RowImage>) {
    assert!(
        actual.is_some() == expected.is_some(),
        "{table} key index {index}: row presence differs (actual {}, expected {})",
        usize::from(actual.is_some()),
        usize::from(expected.is_some())
    );
    if let (Some(actual), Some(expected)) = (actual, expected) {
        assert_eq!(actual.len(), expected.len(), "{table} key index {index}: cell count");
        let cell_count = expected.len();
        for (column, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                actual == expected,
                "{table} key index {index}: column {column} differs ({cell_count} cells)"
            );
        }
    }
}

fn assert_rows(actual: &Rows, expected: &Rows) {
    for (index, ((actual_key, actual_row), (expected_key, expected_row))) in actual.iter().zip(expected).enumerate() {
        assert!(
            actual_key == expected_key,
            "{} key index {index}: keys differ (actual rows {}, expected rows {})",
            expected_key.0,
            actual.len(),
            expected.len()
        );
        assert_row(expected_key.0, index, Some(actual_row), Some(expected_row));
    }
    assert_eq!(actual.len(), expected.len(), "whole collection row count");
}

fn rows(reader: &impl ReadRows) -> Rows {
    let mut result = Rows::new();
    for table in Table::ALL {
        let mut after = None;
        loop {
            let page = reader.row_page(table, after.as_ref()).unwrap();
            if page.is_empty() {
                break;
            }
            for row in page {
                let key = RowKey::from_stored(table, &row).unwrap();
                after = Some(key.clone());
                result.insert((table, key), row);
            }
        }
    }
    result
}

fn raw(scope: &mut WriteScope<'_>, table: Table, row: RowImage) {
    let key = RowKey::from_stored(table, &row).unwrap();
    scope.restore(table, &key, Some(&row)).unwrap();
}

fn config(scope: &mut WriteScope<'_>, key: &str, value: Cell) {
    raw(
        scope,
        Table::Config,
        vec![Cell::text(key), Cell::Integer(7), Cell::Integer(11), value],
    );
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = clock();
    let random = SeededRandom::new(7);
    let builder = CollectionBuilder::create(
        &path,
        &new_col(NOW_MS - 60_000),
        &new_config(),
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
    for id in [1, OLD, 101, 103, 200] {
        let mut p = preset(&format!("Preset {id}"));
        p.id = DeckConfigId(id);
        scope.put(&p).unwrap();
    }
    for (id, name, config_id) in [
        (1, "Deck1", 1),
        (OLD, "Deck10", OLD),
        (101, "Deck11", OLD),
        (103, "Deck12", 103),
        (200, "dEcK10\u{1f}Deck20", 1),
        (201, "Deck10\u{1f}Deck20\u{1f}Deck30", 1),
        (202, "Deck100\u{1f}Deck40", 1),
    ] {
        let mut d = deck(name);
        d.id = DeckId(id);
        if let DeckKind::Normal(normal) = &mut d.kind.kind {
            normal.config_id = config_id;
            normal.extend_new = 7;
            normal.description = "unrelated 100".into();
        }
        let mut row = d.to_cells();
        // Explicit default + unknown field: untouched common must keep its bytes.
        row[4] = Cell::Blob(vec![8, 0, 0x98, 6, 7]);
        let Cell::Blob(kind) = &mut row[5] else { unreachable!() };
        kind.extend_from_slice(&[0x98, 6, 9]);
        raw(&mut scope, Table::Decks, row);
    }
    let mut filtered = deck("Deck50");
    filtered.id = DeckId(203);
    filtered.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
    scope.put(&filtered).unwrap();
    for id in [OLD, 101, 103, 200] {
        let mut nt = common::notetype(&format!("Type {id}"), &["Front", "Back"], &["Forward", "Reverse"]);
        nt.id = NotetypeId(id);
        for t in &mut nt.templates {
            t.mtime.0 = 19 + i64::from(t.ord);
            t.usn.0 = 23;
            t.config.q_format = format!("question {}", t.ord);
            t.config.a_format = format!("answer {}", t.ord);
            t.config.target_deck_id = if id == OLD { OLD } else { 103 };
        }
        scope.put_notetype(&nt).unwrap();
        // Noncanonical, valid unknown configs on unrelated parts stay byte-exact.
        let mut field = scope.row_image(Table::Fields, &RowKey::Pair(id, 0)).unwrap().unwrap();
        field[3] = Cell::Blob(vec![0x98, 6, 0x87, 0]);
        raw(&mut scope, Table::Fields, field);
    }
    for (id, mid) in [(OLD, OLD), (101, 200), (103, 200), (200, OLD), (201, OLD)] {
        let mut n = note(mid, &["front", "back"]);
        n.id = NoteId(id);
        n.guid = format!("guid-{id}");
        let mut row = n.to_cells();
        row[7] = Cell::Integer(id); // native sfld affinity/storage class
        row[10] = Cell::Text(vec![0xff, b'x']);
        raw(&mut scope, Table::Notes, row);
    }
    for (id, nid, did, odid) in [
        (OLD, OLD, OLD, 0),
        (101, 101, 200, OLD),
        (103, 103, 103, 103),
        (200, OLD, OLD, OLD),
        (201, 200, 1, 0),
    ] {
        let mut c = card(NoteId(nid), if id == 200 { 1 } else { 0 });
        c.id = CardId(id);
        c.deck_id = DeckId(did);
        c.original_deck_id = DeckId(odid);
        c.reps = 17;
        c.interval = 31;
        let mut row = c.to_cells();
        row[8] = Cell::text("opaque-due");
        row[14] = Cell::Real(12.5);
        row[17] = if id == OLD {
            Cell::text(" { \"opaque\" : [100], \"s\" : 1.00 } ")
        } else {
            Cell::Blob(vec![0xff, 0, 100])
        };
        raw(&mut scope, Table::Cards, row);
    }
    for (id, cid) in [(90, OLD), (OLD, OLD), (101, 103), (103, 103), (200, 200), (201, 200)] {
        let mut r = revlog(CardId(cid), 123);
        r.id = RevlogId(id);
        let mut row = r.to_cells();
        row[4] = Cell::Real(-6.5);
        raw(&mut scope, Table::Revlog, row);
    }
    for (key, value) in [
        ("curDeck", b"100".as_slice()),
        ("curModel", b"100"),
        ("activeDecks", b"[100,100,100.0,\"100\",null,[100],{\"id\":100},103]"),
        ("_deck_100_opaque", b"\xff\x00 100"),
        ("_deck_100_tail", b" { \"id\":100 } "),
        ("_nt_100_opaque", b"\xff\x00 100"),
        ("_nt_100_tail", b" [100] "),
        ("_nt_200_lastDeck", b"100"),
        ("_nt_201_lastDeck", b"100"),
        ("_deck_200_lastNotetype", b"100"),
        ("_deck_201_lastNotetype", b"100"),
        ("_nt_bad_lastDeck", b"100"),
        ("_nt_203_lastDeck", b"100.0"),
        ("_nt_204_lastDeck", b"[100]"),
        ("_deck_bad_lastNotetype", b"100"),
        ("_deck_203_lastNotetype", b"100.0"),
        ("_deck_204_lastNotetype", b"{\"id\":100}"),
        ("_deck_1000_opaque", b"100"),
        ("x_deck_100_opaque", b"100"),
        ("_nt_1000_opaque", b"100"),
        ("x_nt_100_opaque", b"100"),
        ("unrelated", b"100"),
    ] {
        config(&mut scope, key, Cell::Blob(value.to_vec()));
    }
    config(&mut scope, "_nt_205_lastDeck", Cell::text("100"));
    config(&mut scope, "_deck_205_lastNotetype", Cell::Integer(OLD));
    scope.commit().unwrap();
    builder.finish().unwrap();
    (dir, path)
}

fn stamp(table: Table, row: &mut RowImage) {
    let (usn, mtime) = match table {
        Table::Notes => (4, Some(3)),
        Table::Cards => (5, Some(4)),
        Table::Revlog => (2, None),
        Table::Config => (1, Some(2)),
        Table::Decks | Table::DeckConfig | Table::Notetypes => (3, Some(2)),
        _ => return,
    };
    row[usn] = Cell::Integer(-1);
    if let Some(column) = mtime {
        row[column] = Cell::Integer(NOW_MS / 1000);
    }
}

fn edit(expected: &mut Rows, table: Table, key: RowKey, changes: &[(usize, Cell)]) {
    let row = expected.get_mut(&(table, key)).unwrap();
    for (column, cell) in changes {
        row[*column] = cell.clone();
    }
    stamp(table, row);
}

fn moved(expected: &mut Rows, table: Table, old: RowKey, new: RowKey) {
    let mut row = expected.remove(&(table, old)).unwrap();
    match &new {
        RowKey::Id(id) | RowKey::Pair(id, _) => row[0] = Cell::Integer(*id),
        RowKey::Text(key) => row[0] = Cell::Text(key.clone()),
    }
    stamp(table, &mut row);
    assert!(expected.insert((table, new), row).is_none());
}

fn expected_move(before: &Rows, table: Table) -> Rows {
    let mut expected = before.clone();
    moved(&mut expected, table, RowKey::Id(OLD), RowKey::Id(NEW));
    expected.get_mut(&(Table::Col, RowKey::Id(1))).unwrap()[2] = Cell::Integer(NOW_MS);
    expected
}

fn journal_proof(journal: &Journal, before: &Rows, expected: &Rows) {
    let keys: BTreeSet<_> = before.keys().chain(expected.keys()).cloned().collect();
    let changed: BTreeSet<_> = keys
        .into_iter()
        .filter(|key| before.get(key) != expected.get(key))
        .collect();
    assert!(
        journal
            .entries()
            .map(|e| (e.table(), e.key().clone()))
            .collect::<BTreeSet<_>>()
            == changed,
        "journal key set differs (actual keys {}, expected keys {})",
        journal.len(),
        changed.len()
    );
    assert_eq!(journal.len(), changed.len());
    for (index, (table, key)) in changed.into_iter().enumerate() {
        let entry = journal.entry(table, &key).unwrap();
        assert_row(
            table,
            index,
            journal.before(entry).unwrap().as_ref(),
            before.get(&(table, key.clone())),
        );
        match entry.after() {
            After::Note(note) => {
                let after = expected.get(&(table, key.clone())).unwrap();
                assert_eq!(note.sha256, digest(after));
                let original = before.get(&(table, key));
                let columns: Vec<_> = table
                    .columns()
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| original.is_none_or(|row| row[*i] != after[*i]))
                    .map(|(_, name)| *name)
                    .collect();
                assert_eq!(note.changed.names(table), columns);
            }
            _ => assert_row(
                table,
                index,
                journal.after(entry).unwrap().as_ref(),
                expected.get(&(table, key)),
            ),
        }
    }
}

fn commit_move(path: &std::path::Path, table: Table, before: Rows, expected: Rows) {
    let collection = open(path).unwrap();
    let clock = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    if table == Table::Revlog {
        assert_eq!(scope.max_revlog_id(CardId(OLD)).unwrap(), OLD);
    }
    assert_eq!(scope.cloud_rekey_candidate(table, OLD, || false).unwrap(), NEW);
    scope.cloud_rekey_id(table, OLD, NEW, || false).unwrap();
    assert!(scope.row_image(table, &RowKey::Id(OLD)).unwrap().is_none());
    assert!(scope.row_image(table, &RowKey::Id(NEW)).unwrap().is_some());
    if table == Table::Revlog {
        assert_eq!(scope.max_revlog_id(CardId(OLD)).unwrap(), NEW);
    }
    let committed = scope.commit().unwrap();
    assert!(committed.changed);
    assert!(committed.post_commit_error.is_none());
    assert_rows(&rows(collection.writer()), &expected);
    journal_proof(committed.journal.as_ref().unwrap(), &before, &expected);
    let target_count = match table {
        Table::Notes | Table::Cards | Table::DeckConfig => 5,
        Table::Revlog => 4,
        Table::Decks => 18,
        Table::Notetypes => 21,
        _ => unreachable!(),
    };
    assert_eq!(committed.journal.as_ref().unwrap().len(), target_count);
    assert!(expected.keys().all(|(table, _)| *table != Table::Graves));
    collection.close().unwrap();
    let reopened = open(path).unwrap();
    assert_rows(&rows(reopened.writer()), &expected);
    if table == Table::Revlog {
        assert_eq!(reopened.writer().max_revlog_id(CardId(OLD)).unwrap(), NEW);
    }
    reopened.close().unwrap();
}

fn baseline(path: &std::path::Path) -> Rows {
    let collection = open(path).unwrap();
    let before = rows(collection.writer());
    collection.close().unwrap();
    before
}

#[test]
fn note_move_rewrites_every_card_nid_and_preserves_raw_cells() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::Notes);
    for id in [OLD, 200] {
        edit(&mut expected, Table::Cards, RowKey::Id(id), &[(1, Cell::Integer(NEW))]);
    }
    commit_move(&path, Table::Notes, before, expected);
}

#[test]
fn card_move_rewrites_every_history_cid_without_touching_other_history() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::Cards);
    for id in [90, OLD] {
        edit(&mut expected, Table::Revlog, RowKey::Id(id), &[(1, Cell::Integer(NEW))]);
    }
    commit_move(&path, Table::Cards, before, expected);
}

#[test]
fn latest_revlog_move_journals_derived_card_without_normalizing_native_data() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::Revlog);
    edit(&mut expected, Table::Cards, RowKey::Id(OLD), &[]);
    commit_move(&path, Table::Revlog, before, expected);
}

#[test]
fn latest_revlog_move_preserves_opaque_blob_card_storage_class() {
    let (_dir, path) = fixture();
    let collection = open(&path).unwrap();
    let clock = clock();
    let mut scope = collection.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    let mut c = scope.row_image(Table::Cards, &RowKey::Id(OLD)).unwrap().unwrap();
    c[17] = Cell::Blob(vec![0xff, 0, 100]);
    raw(&mut scope, Table::Cards, c);
    scope.commit().unwrap();
    collection.close().unwrap();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::Revlog);
    edit(&mut expected, Table::Cards, RowKey::Id(OLD), &[]);
    commit_move(&path, Table::Revlog, before, expected);
}

#[test]
fn nonlatest_revlog_move_leaves_derived_card_and_schedule_clean() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = before.clone();
    moved(&mut expected, Table::Revlog, RowKey::Id(90), RowKey::Id(91));
    expected.get_mut(&(Table::Col, RowKey::Id(1))).unwrap()[2] = Cell::Integer(NOW_MS);
    let collection = open(&path).unwrap();
    let clock = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.max_revlog_id(CardId(OLD)).unwrap(), OLD);
    assert_eq!(scope.cloud_rekey_candidate(Table::Revlog, 90, || false).unwrap(), 91);
    scope.cloud_rekey_id(Table::Revlog, 90, 91, || false).unwrap();
    assert_eq!(scope.max_revlog_id(CardId(OLD)).unwrap(), OLD);
    let committed = scope.commit().unwrap();
    assert_eq!(committed.journal.as_ref().unwrap().len(), 3);
    journal_proof(committed.journal.as_ref().unwrap(), &before, &expected);
    assert_rows(&rows(collection.writer()), &expected);
    collection.close().unwrap();
    let reopened = open(&path).unwrap();
    assert_rows(&rows(reopened.writer()), &expected);
    reopened.close().unwrap();
}

#[test]
fn deck_move_rewrites_native_references_overrides_and_direct_child_target() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::Decks);
    edit(&mut expected, Table::Cards, RowKey::Id(OLD), &[(2, Cell::Integer(NEW))]);
    edit(
        &mut expected,
        Table::Cards,
        RowKey::Id(101),
        &[(15, Cell::Integer(NEW))],
    );
    edit(
        &mut expected,
        Table::Cards,
        RowKey::Id(200),
        &[(2, Cell::Integer(NEW)), (15, Cell::Integer(NEW))],
    );
    edit(&mut expected, Table::Decks, RowKey::Id(200), &[]);
    for key in ["curDeck", "_nt_200_lastDeck", "_nt_201_lastDeck"] {
        edit(
            &mut expected,
            Table::Config,
            RowKey::text(key),
            &[(3, Cell::Blob(b"102".to_vec()))],
        );
    }
    edit(
        &mut expected,
        Table::Config,
        RowKey::text("activeDecks"),
        &[(
            3,
            Cell::Blob(b"[102,102,100.0,\"100\",null,[100],{\"id\":100},103]".to_vec()),
        )],
    );
    for tail in ["opaque", "tail"] {
        moved(
            &mut expected,
            Table::Config,
            RowKey::text(&format!("_deck_100_{tail}")),
            RowKey::text(&format!("_deck_102_{tail}")),
        );
    }
    for ord in [0, 1] {
        let row = expected.get_mut(&(Table::Templates, RowKey::Pair(OLD, ord))).unwrap();
        let Cell::Blob(bytes) = &row[5] else { unreachable!() };
        let mut config = ProtoBlob::<TemplateConfig>::decode(bytes).unwrap();
        assert_eq!(config.target_deck_id, OLD);
        config.target_deck_id = NEW;
        row[5] = Cell::Blob(config.to_bytes().into_owned());
    }
    edit(&mut expected, Table::Notetypes, RowKey::Id(OLD), &[]);
    commit_move(&path, Table::Decks, before, expected);
}

#[test]
fn preset_move_changes_only_normal_deck_users_and_retains_unknown_blob_facts() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::DeckConfig);
    for id in [OLD, 101] {
        let row = expected.get(&(Table::Decks, RowKey::Id(id))).unwrap();
        let Cell::Blob(bytes) = &row[5] else { unreachable!() };
        let mut kind = ProtoBlob::<DeckKindMessage>::decode(bytes).unwrap();
        assert_eq!(kind.unknown_fields().iter().next().unwrap().bytes(), &[0x98, 6, 9]);
        let DeckKind::Normal(normal) = &mut kind.kind else {
            panic!("normal deck")
        };
        assert_eq!(normal.config_id, OLD);
        normal.config_id = NEW;
        let bytes = kind.to_bytes().into_owned();
        let reread = ProtoBlob::<DeckKindMessage>::decode(&bytes).unwrap();
        assert_eq!(reread.unknown_fields(), kind.unknown_fields());
        assert!(matches!(&reread.kind, DeckKind::Normal(n) if n.config_id == NEW));
        edit(&mut expected, Table::Decks, RowKey::Id(id), &[(5, Cell::Blob(bytes))]);
    }
    commit_move(&path, Table::DeckConfig, before, expected);
}

#[test]
fn notetype_move_moves_all_raw_parts_notes_and_config_references() {
    let (_dir, path) = fixture();
    let before = baseline(&path);
    let mut expected = expected_move(&before, Table::Notetypes);
    for id in [OLD, 200, 201] {
        edit(&mut expected, Table::Notes, RowKey::Id(id), &[(2, Cell::Integer(NEW))]);
    }
    for table in [Table::Fields, Table::Templates] {
        for ord in [0, 1] {
            moved(&mut expected, table, RowKey::Pair(OLD, ord), RowKey::Pair(NEW, ord));
        }
    }
    for key in ["curModel", "_deck_200_lastNotetype", "_deck_201_lastNotetype"] {
        edit(
            &mut expected,
            Table::Config,
            RowKey::text(key),
            &[(3, Cell::Blob(b"102".to_vec()))],
        );
    }
    for tail in ["opaque", "tail"] {
        moved(
            &mut expected,
            Table::Config,
            RowKey::text(&format!("_nt_100_{tail}")),
            RowKey::text(&format!("_nt_102_{tail}")),
        );
    }
    commit_move(&path, Table::Notetypes, before, expected);
}

#[test]
fn notetype_move_pages_both_gapped_part_tables_preserving_bytes_journal_and_rollback() {
    let (_dir, path) = fixture();
    let collection = open(&path).unwrap();
    let clock = clock();
    let ords: Vec<i64> = [0, 1].into_iter().chain((1..=251).map(|i| 2 * i + 1)).collect();
    assert_eq!(ords.len(), 253);
    let mut seed = collection.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    for part in [Table::Fields, Table::Templates] {
        let source = seed.row_image(part, &RowKey::Pair(OLD, 0)).unwrap().unwrap();
        for &ord in &ords[2..] {
            let mut row = source.clone();
            row[1] = Cell::Integer(ord);
            row[2] = Cell::text(format!("Part {ord}"));
            if part == Table::Templates {
                row[3] = Cell::Integer(700 + ord);
                row[4] = Cell::Integer(29);
                let Cell::Blob(bytes) = &mut row[5] else { unreachable!() };
                // Valid unknown field with a noncanonical varint encoding.
                bytes.extend_from_slice(&[0x98, 6, 0x87, 0]);
                assert!(ProtoBlob::<TemplateConfig>::decode(bytes).is_ok());
            }
            raw(&mut seed, part, row);
        }
    }
    // Storage permits gapped ordinals. Keep note field counts coherent; this
    // fixture exercises identity movement, not runtime notetype generation.
    let fields = std::iter::once("front")
        .chain(std::iter::once("back"))
        .chain(std::iter::repeat_n("extra", 251))
        .collect::<Vec<_>>()
        .join("\u{1f}");
    for id in [OLD, 200, 201] {
        let mut row = seed.row_image(Table::Notes, &RowKey::Id(id)).unwrap().unwrap();
        row[6] = Cell::text(&fields);
        raw(&mut seed, Table::Notes, row);
    }
    seed.commit().unwrap();
    let before = rows(collection.writer());
    for part in [Table::Fields, Table::Templates] {
        let first = if part == Table::Fields {
            collection.writer().notetype_fields_page(NotetypeId(OLD), None).unwrap()
        } else {
            collection
                .writer()
                .notetype_templates_page(NotetypeId(OLD), None)
                .unwrap()
        };
        assert_eq!(first.len(), 250);
        assert_eq!(first.last().unwrap()[1], Cell::Integer(ords[249]));
        let second = if part == Table::Fields {
            collection
                .writer()
                .notetype_fields_page(NotetypeId(OLD), Some(ords[249]))
                .unwrap()
        } else {
            collection
                .writer()
                .notetype_templates_page(NotetypeId(OLD), Some(ords[249]))
                .unwrap()
        };
        assert_eq!(second.len(), 3);
        assert_eq!(second.last().unwrap()[1], Cell::Integer(503));
    }
    // Private part reads hold one body. Cancel after more than 250 parts
    // of each table have moved, leaving the final old ordinal untouched.
    for (cancel_at, part) in [(252, Table::Fields), (506, Table::Templates)] {
        let mut scope = collection
            .writer()
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        let mut calls = 0;
        assert!(
            scope
                .cloud_rekey_id(Table::Notetypes, OLD, NEW, || {
                    calls += 1;
                    calls == cancel_at
                })
                .is_err()
        );
        assert_eq!(calls, cancel_at);
        assert!(calls > 250);
        assert!(scope.row_image(Table::Notetypes, &RowKey::Id(NEW)).unwrap().is_some());
        let partial = rows(&scope);
        assert!(
            partial
                .keys()
                .filter(|(table, key)| *table == part && matches!(key, RowKey::Pair(NEW, _)))
                .count()
                > 250
        );
        for &ord in &ords[..250] {
            assert!(scope.row_image(part, &RowKey::Pair(NEW, ord)).unwrap().is_some());
            assert!(scope.row_image(part, &RowKey::Pair(OLD, ord)).unwrap().is_none());
        }
        assert!(scope.row_image(part, &RowKey::Pair(OLD, 503)).unwrap().is_some());
        scope.rollback().unwrap();
        assert_rows(&rows(collection.writer()), &before);
    }
    // A complete move also remains reversible until the caller commits.
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope.cloud_rekey_id(Table::Notetypes, OLD, NEW, || false).unwrap();
    scope.rollback().unwrap();
    assert_rows(&rows(collection.writer()), &before);
    collection.close().unwrap();
    assert_rows(&baseline(&path), &before);

    let mut expected = expected_move(&before, Table::Notetypes);
    for id in [OLD, 200, 201] {
        edit(&mut expected, Table::Notes, RowKey::Id(id), &[(2, Cell::Integer(NEW))]);
    }
    for part in [Table::Fields, Table::Templates] {
        for &ord in &ords {
            moved(&mut expected, part, RowKey::Pair(OLD, ord), RowKey::Pair(NEW, ord));
        }
    }
    for key in ["curModel", "_deck_200_lastNotetype", "_deck_201_lastNotetype"] {
        edit(
            &mut expected,
            Table::Config,
            RowKey::text(key),
            &[(3, Cell::Blob(b"102".to_vec()))],
        );
    }
    for tail in ["opaque", "tail"] {
        moved(
            &mut expected,
            Table::Config,
            RowKey::text(&format!("_nt_100_{tail}")),
            RowKey::text(&format!("_nt_102_{tail}")),
        );
    }
    let collection = open(&path).unwrap();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(
        scope.cloud_rekey_candidate(Table::Notetypes, OLD, || false).unwrap(),
        NEW
    );
    scope.cloud_rekey_id(Table::Notetypes, OLD, NEW, || false).unwrap();
    let committed = scope.commit().unwrap();
    assert!(committed.changed);
    assert!(committed.post_commit_error.is_none());
    assert_eq!(committed.journal.as_ref().unwrap().len(), 1025);
    journal_proof(committed.journal.as_ref().unwrap(), &before, &expected);
    assert_rows(&rows(collection.writer()), &expected);
    assert!(expected.keys().all(|(table, _)| *table != Table::Graves));
    for part in [Table::Fields, Table::Templates] {
        assert_eq!(
            expected
                .keys()
                .filter(|(table, key)| *table == part && matches!(key, RowKey::Pair(NEW, _)))
                .count(),
            253
        );
        assert!(
            expected
                .keys()
                .all(|(table, key)| *table != part || !matches!(key, RowKey::Pair(OLD, _)))
        );
    }
    collection.close().unwrap();
    assert_rows(&baseline(&path), &expected);
}

#[test]
fn orphan_destination_part_keys_refuse_the_chosen_parent_id_and_rollback_every_row() {
    for part in [Table::Fields, Table::Templates] {
        for ord in [1, 503, 999] {
            let (_dir, path) = fixture();
            let collection = open(&path).unwrap();
            let clock = clock();
            let mut seed = collection.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
            assert!(seed.row_image(Table::Notetypes, &RowKey::Id(NEW)).unwrap().is_none());
            let mut orphan = seed.row_image(part, &RowKey::Pair(OLD, 1)).unwrap().unwrap();
            orphan[0] = Cell::Integer(NEW);
            orphan[1] = Cell::Integer(ord);
            orphan[2] = Cell::text("Unrelated orphan");
            let config_column = if part == Table::Fields { 3 } else { 5 };
            let Cell::Blob(bytes) = &mut orphan[config_column] else {
                unreachable!()
            };
            bytes.extend_from_slice(&[0x98, 6, 0x87, 0]);
            raw(&mut seed, part, orphan.clone());
            seed.commit().unwrap();
            let before = rows(collection.writer());
            let mut scope = collection
                .writer()
                .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
                .unwrap();
            assert_eq!(
                scope.cloud_rekey_candidate(Table::Notetypes, OLD, || false).unwrap(),
                NEW
            );
            assert!(scope.cloud_rekey_id(Table::Notetypes, OLD, NEW, || false).is_err());
            assert_rows(&rows(&scope), &before);
            assert!(scope.row_image(Table::Notetypes, &RowKey::Id(OLD)).unwrap().is_some());
            assert!(scope.row_image(Table::Notetypes, &RowKey::Id(NEW)).unwrap().is_none());
            assert!(scope.row_image(part, &RowKey::Pair(NEW, 0)).unwrap().is_none());
            assert_row(
                part,
                usize::try_from(ord).unwrap(),
                scope.row_image(part, &RowKey::Pair(NEW, ord)).unwrap().as_ref(),
                Some(&orphan),
            );
            scope.rollback().unwrap();
            assert_rows(&rows(collection.writer()), &before);
            collection.close().unwrap();
            assert_rows(&baseline(&path), &before);
        }
    }
}

#[test]
fn all_six_refuse_missing_source_occupied_destination_cap_and_wrong_mode_then_rollback() {
    let (_dir, path) = fixture();
    let collection = open(&path).unwrap();
    let before = rows(collection.writer());
    let clock = clock();
    for table in KINDS {
        for (old, new, mode) in [
            (99, NEW, StampMode::Local),
            (OLD, 101, StampMode::Local),
            (OLD, CAP + 1, StampMode::Local),
            (OLD, OLD, StampMode::Local),
            (OLD, 99, StampMode::Local),
            (OLD, NEW, StampMode::CallerSupplied),
        ] {
            let mut scope = collection
                .writer()
                .begin_write(mode, &clock, Some(JournalOptions::in_memory()))
                .unwrap();
            assert!(scope.cloud_rekey_id(table, old, new, || false).is_err());
            scope.rollback().unwrap();
            assert_rows(&rows(collection.writer()), &before);
        }
        let scope = collection
            .writer()
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        assert!(matches!(
            scope.cloud_rekey_candidate(table, CAP, || false),
            Err(StorageError::IdsExhausted { .. })
        ));
        assert!(scope.cloud_rekey_candidate(table, OLD, || true).is_err());
        scope.rollback().unwrap();
    }
    for table in [Table::Decks, Table::DeckConfig] {
        let mut scope = collection
            .writer()
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        assert!(scope.cloud_rekey_id(table, 1, 2, || false).is_err());
        scope.rollback().unwrap();
        assert_rows(&rows(collection.writer()), &before);
    }
    collection.close().unwrap();
    assert_rows(&baseline(&path), &before);
}

#[test]
fn all_six_allow_exact_cap_and_roll_back_a_real_move_on_cancel() {
    for table in KINDS {
        let (_dir, path) = fixture();
        let collection = open(&path).unwrap();
        let clock = clock();
        let mut seed = collection.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
        let mut source = seed.row_image(table, &RowKey::Id(OLD)).unwrap().unwrap();
        // Use a separate source near the cap without duplicate unique names.
        if matches!(table, Table::Decks | Table::DeckConfig | Table::Notetypes) {
            source[1] = Cell::text("Cap source");
        }
        source[0] = Cell::Integer(CAP - 1);
        raw(&mut seed, table, source);
        if table == Table::Notetypes {
            for part in [Table::Fields, Table::Templates] {
                for ord in [0, 1] {
                    let mut row = seed.row_image(part, &RowKey::Pair(OLD, ord)).unwrap().unwrap();
                    row[0] = Cell::Integer(CAP - 1);
                    raw(&mut seed, part, row);
                }
            }
        }
        seed.commit().unwrap();
        let before = rows(collection.writer());
        let mut scope = collection
            .writer()
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        assert_eq!(scope.cloud_rekey_candidate(table, CAP - 1, || false).unwrap(), CAP);
        scope.cloud_rekey_id(table, CAP - 1, CAP, || false).unwrap();
        assert!(scope.row_image(table, &RowKey::Id(CAP - 1)).unwrap().is_none());
        assert!(scope.row_image(table, &RowKey::Id(CAP)).unwrap().is_some());
        scope.rollback().unwrap();
        assert_rows(&rows(collection.writer()), &before);
        let mut scope = collection
            .writer()
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        let mut calls = 0;
        assert!(
            scope
                .cloud_rekey_id(table, OLD, NEW, || {
                    calls += 1;
                    calls >= if table == Table::Revlog { 1 } else { 2 }
                })
                .is_err()
        );
        // Cancellation occurs after the identity has physically moved.
        assert!(scope.row_image(table, &RowKey::Id(OLD)).unwrap().is_none());
        assert!(scope.row_image(table, &RowKey::Id(NEW)).unwrap().is_some());
        scope.rollback().unwrap();
        assert_rows(&rows(collection.writer()), &before);
        collection.close().unwrap();
        assert_rows(&baseline(&path), &before);
    }
}

#[test]
fn config_destination_conflict_rolls_back_the_whole_deck_or_notetype_graph() {
    for (table, prefix) in [(Table::Decks, "deck"), (Table::Notetypes, "nt")] {
        let (_dir, path) = fixture();
        let collection = open(&path).unwrap();
        let clock = clock();
        let mut seed = collection.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
        config(&mut seed, &format!("_{prefix}_102_tail"), Cell::Blob(vec![0xff, 42]));
        seed.commit().unwrap();
        let before = rows(collection.writer());
        let mut scope = collection
            .writer()
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        assert_eq!(scope.cloud_rekey_candidate(table, OLD, || false).unwrap(), NEW);
        assert!(scope.cloud_rekey_id(table, OLD, NEW, || false).is_err());
        assert!(scope.row_image(table, &RowKey::Id(NEW)).unwrap().is_some());
        assert!(
            scope
                .row_image(Table::Config, &RowKey::text(&format!("_{prefix}_102_opaque")))
                .unwrap()
                .is_some()
        );
        scope.rollback().unwrap();
        assert_rows(&rows(collection.writer()), &before);
        collection.close().unwrap();
        assert_rows(&baseline(&path), &before);
    }
}
