//! Browse's column sets as ops (`specs/browser/workspace.md` V2.2 to V2.4,
//! V4.3, V18): reading `meguriBrowserColumnSets` (missing key, members of
//! another shape, the default whatever Anki stored), that a change writes
//! only its set and keeps unknown members and the sets of removed note
//! types, that Anki's own column and sort keys stay byte for byte, and the
//! skip-undo class. The values each change computes are meguri-search's
//! `tests/column_sets.rs`.

mod common;

use common::Harness;
use common::tagging::{fresh, raw};
use meguri_engine::browser_config::column_sets::{self, Shown};
use meguri_engine::sched::ops::{self, Request, Targets};
use meguri_engine::{Collection, EngineError, OpSource, UndoScope};
use meguri_model::ids::NotetypeId;
use meguri_search::SearchMode::{self, Cards, Notes};
use meguri_search::config::column_sets::{Change, SetId, SetSort, default_columns};
use meguri_search::config::{Column, ColumnKey, SortDirection};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReaderRole, Table};

const KEY: &str = "meguriBrowserColumnSets";

/// Anki's column and sort keys, which Meguri never reads or writes from
/// Browse (V2.4), each stored with spacing Anki would not write.
const ANKI_KEYS: [(&str, &str); 6] = [
    ("activeCols", r#"[ "question", "cardEase" ]"#),
    ("activeNoteCols", r#"["noteMod" , "answer"]"#),
    ("sortType", r#" "cardIvl" "#),
    ("sortBackwards", " true"),
    ("noteSortType", r#""noteTags" "#),
    ("browserNoteSortBackwards", "false "),
];

/// A note type id no note type has: its set is that of a removed note type.
const REMOVED: NotetypeId = NotetypeId(1_700_000_000_001);

fn json(text: &str) -> ConfigValue {
    ConfigValue::parse(text.as_bytes()).unwrap_or_else(|| panic!("{text:?} is JSON"))
}

fn field(name: &str) -> ColumnKey {
    ColumnKey::Field(name.to_owned())
}

fn col(column: Column) -> ColumnKey {
    ColumnKey::Column(column)
}

/// Every config row: key, usn, mtime, value bytes and storage class.
fn config_rows(c: &Collection) -> Vec<Vec<SqlValue>> {
    c.reader(ReaderRole::Query)
        .unwrap()
        .execute_raw(
            "SELECT KEY, usn, mtime_secs, val, typeof(val) FROM config ORDER BY KEY",
            &[],
        )
        .unwrap()
        .rows
}

fn row(rows: &[Vec<SqlValue>], key: &str) -> Option<Vec<SqlValue>> {
    rows.iter()
        .find(|row| row[0] == SqlValue::Text(key.to_owned()))
        .cloned()
}

/// The keys whose rows differ between two reads.
fn changed_keys(before: &[Vec<SqlValue>], after: &[Vec<SqlValue>]) -> Vec<String> {
    let mut keys: Vec<String> = before
        .iter()
        .chain(after)
        .filter_map(|row| match &row[0] {
            SqlValue::Text(key) => Some(key.clone()),
            _ => None,
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys.retain(|key| row(before, key) != row(after, key));
    keys
}

/// The key's value read per C3.
fn stored(c: &Collection) -> Option<ConfigValue> {
    use meguri_storage::ReadRows;
    c.reader(ReaderRole::Query).unwrap().config_value(KEY).unwrap()
}

/// Stores `text` as `key`'s bytes, stamped as another client's write.
fn put(c: &Collection, key: &str, text: &str) {
    raw(
        c,
        "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES (?, 0, 1700000000, CAST(? AS BLOB))",
        &[SqlValue::Text(key.to_owned()), SqlValue::Text(text.to_owned())],
    );
}

fn shown(c: &Collection, notetype: Option<NotetypeId>) -> Shown {
    column_sets::shown(&*c.reader(ReaderRole::Query).unwrap(), notetype).unwrap()
}

fn change(c: &Collection, set: SetId, mode: SearchMode, change: Change) -> bool {
    column_sets::change(c, set, mode, change).unwrap().value
}

/// The first note type of the collection.
fn notetype(c: &Collection) -> NotetypeId {
    NotetypeId(common::query_int(c, "SELECT min(id) FROM notetypes"))
}

/// A harness with none of the browser keys stored but Anki's, as above.
fn with_anki_keys() -> Harness {
    let h = fresh();
    raw(&h.collection, &format!("DELETE FROM config WHERE KEY = '{KEY}'"), &[]);
    for (key, text) in ANKI_KEYS {
        put(&h.collection, key, text);
    }
    h
}

fn standard_sort() -> SetSort {
    SetSort {
        column: col(Column::SortField),
        direction: SortDirection::Ascending,
    }
}

#[test]
fn nothing_stored_reads_the_compact_default_whatever_anki_stored() {
    let h = with_anki_keys();
    let c = &h.collection;
    let nt = notetype(c);
    let before = config_rows(c);
    let list = shown(c, None);
    assert_eq!(list.set, SetId::Standard);
    assert!(!list.own);
    for mode in [Cards, Notes] {
        assert_eq!(list.columns.columns(mode), default_columns(mode).as_slice(), "{mode:?}");
        assert_eq!(*list.columns.sort(mode), standard_sort(), "{mode:?}");
    }
    assert_eq!(
        default_columns(Cards),
        [Column::SortField, Column::CardType, Column::Deck, Column::Due, Column::CardModified].map(col)
    );
    assert_eq!(
        default_columns(Notes),
        [Column::SortField, Column::Notetype, Column::CardType, Column::Tags, Column::Created].map(col)
    );
    // An existing note type without an own set uses the standard set, live.
    let list = shown(c, Some(nt));
    assert_eq!((list.set, list.own), (SetId::Notetype(nt), false));
    assert_eq!(list.columns, shown(c, None).columns);
    assert_eq!(config_rows(c), before, "reading writes nothing");
}

#[test]
fn members_of_another_shape_read_as_missing() {
    let cases = [
        r#""not an object""#,
        r#"{"std":[1]}"#,
        r#"{"std":{"cards":"noteFld","notes":[],"sort":5}}"#,
        r#"{"std":{"cards":["noteFld","noteFld"],"notes":["noteFld",3],"sort":{"cards":["deck"],"notes":["cardDue",1]}}}"#,
        r#"{"std":{"cards":["noteFld","unknownColumn"],"sort":{"cards":["question",true],"notes":["stability",true]}}}"#,
    ];
    for text in cases {
        let h = with_anki_keys();
        let c = &h.collection;
        put(c, KEY, text);
        let list = shown(c, None).columns;
        for mode in [Cards, Notes] {
            assert_eq!(list.columns(mode), default_columns(mode).as_slice(), "{text} {mode:?}");
            assert_eq!(*list.sort(mode), standard_sort(), "{text} {mode:?}");
        }
    }
}

#[test]
fn a_stored_sort_outside_the_list_still_sorts() {
    let h = with_anki_keys();
    let c = &h.collection;
    put(
        c,
        KEY,
        r#"{"std":{"cards":["noteFld"],"sort":{"cards":["fld:Back",true],"notes":["cardReps",false]}}}"#,
    );
    let list = shown(c, None).columns;
    assert_eq!(list.columns(Cards), [col(Column::SortField)]);
    assert_eq!(
        *list.sort(Cards),
        SetSort {
            column: field("Back"),
            direction: SortDirection::Descending
        }
    );
    assert_eq!(list.sort_request(Notes).column(), &meguri_search::SortColumn::Column(Column::Reviews));
}

#[test]
fn a_change_writes_only_its_set_and_keeps_anki_keys_and_unknown_members() {
    let h = with_anki_keys();
    let c = &h.collection;
    let nt = notetype(c);
    let removed_set = r#"{"notes":["fld:Front","fld:Back","deck"],"widths":{"deck":80}}"#;
    put(
        c,
        KEY,
        &format!(
            r#"{{ "std": {{"cards": ["noteFld", "deck"], "future": [1, 2.0]}}, "{}": {removed_set}, "later": {{"x": 1}} }}"#,
            REMOVED.0
        ),
    );
    let removed = json(removed_set);
    let anki_rows = |rows: &[Vec<SqlValue>]| -> Vec<Option<Vec<SqlValue>>> {
        ANKI_KEYS.iter().map(|(key, _)| row(rows, key)).collect()
    };
    let first = config_rows(c);
    let changes: Vec<(SetId, SearchMode, Change)> = vec![
        (SetId::Standard, Cards, Change::Show(field("Front"))),
        (SetId::Standard, Cards, Change::Hide(col(Column::Deck))),
        (SetId::Standard, Notes, Change::Show(col(Column::Due))),
        (
            SetId::Standard,
            Cards,
            Change::Reorder(vec![field("Front"), col(Column::SortField)]),
        ),
        (SetId::Standard, Cards, Change::SortBy(col(Column::Due))),
        (
            SetId::Standard,
            Notes,
            Change::SetSort(field("Front"), SortDirection::Descending),
        ),
        (SetId::Notetype(nt), Notes, Change::Show(field("Back"))),
    ];
    for (set, mode, edit) in changes {
        let before = config_rows(c);
        let other_members: Vec<(String, ConfigValue)> = stored(c)
            .unwrap()
            .members()
            .into_iter()
            .filter(|(name, _)| *name != set.member())
            .map(|(name, value)| (name.to_owned(), value.clone()))
            .collect();
        assert!(change(c, set, mode, edit.clone()), "{edit:?} writes");
        let after = config_rows(c);
        assert_eq!(changed_keys(&before, &after), [KEY], "{edit:?}");
        let value = stored(c).unwrap();
        for (name, member) in &other_members {
            assert_eq!(value.get(name), Some(member), "{edit:?} keeps {name}");
        }
        assert_eq!(value.get("later"), Some(&json(r#"{"x":1}"#)), "{edit:?}");
        assert_eq!(value.get(&REMOVED.0.to_string()), Some(&removed), "{edit:?}");
        assert_eq!(
            value.get("std").and_then(|std| std.get("future")),
            Some(&json("[1,2.0]")),
            "{edit:?}: an unknown member of the set is kept, an integer as an integer and a float as a float"
        );
        assert_eq!(anki_rows(&after), anki_rows(&first), "{edit:?}: Anki's keys stay byte for byte");
    }
    assert_eq!(
        stored(c).unwrap().get("std").unwrap().compact(),
        r#"{"cards":["fld:Front","noteFld"],"future":[1,2.0],"notes":["noteFld","note","template","noteTags","noteCrt","cardDue"],"sort":{"cards":["cardDue",true],"notes":["fld:Front",true]}}"#
    );
    let row = row(&config_rows(c), KEY).unwrap();
    assert_eq!(row[1], SqlValue::Integer(-1), "usn");
    assert_eq!(row[4], SqlValue::Text("blob".to_owned()));
}

#[test]
fn a_misshapen_member_is_replaced_at_the_next_write_of_its_set_only() {
    let h = with_anki_keys();
    let c = &h.collection;
    let nt = notetype(c);
    let other = r#"{"cards":"bad"}"#;
    put(
        c,
        KEY,
        &format!(r#"{{"std":{{"cards":"bad","notes":[],"sort":{{"cards":"x","zz":0}}}},"{}":{other}}}"#, nt.0),
    );
    // A change that leaves the shown set as it is writes nothing.
    assert!(!change(c, SetId::Standard, Cards, Change::Show(col(Column::SortField))));
    assert!(change(c, SetId::Standard, Notes, Change::SortBy(col(Column::Tags))));
    let value = stored(c).unwrap();
    assert_eq!(
        value.get("std").unwrap().compact(),
        r#"{"cards":["noteFld","template","deck","cardDue","cardMod"],"notes":["noteFld","note","template","noteTags","noteCrt"],"sort":{"cards":["noteFld",false],"notes":["noteTags",false],"zz":0}}"#
    );
    assert_eq!(value.get(&nt.0.to_string()), Some(&json(other)), "another set is left as it is");
}

#[test]
fn a_note_types_first_change_copies_the_standard_set_and_then_they_are_separate() {
    let h = with_anki_keys();
    let c = &h.collection;
    let nt = notetype(c);
    let own_set = SetId::Notetype(nt);
    assert!(change(c, SetId::Standard, Cards, Change::Show(col(Column::Tags))));
    assert!(change(c, SetId::Standard, Cards, Change::SortBy(col(Column::Deck))));
    // A change that changes nothing creates no own set.
    assert!(!change(c, own_set, Cards, Change::Show(col(Column::Tags))));
    assert!(!shown(c, Some(nt)).own);

    assert!(change(c, own_set, Notes, Change::Show(field("Front"))));
    let own = stored(c).unwrap().get(&nt.0.to_string()).unwrap().compact();
    assert_eq!(
        own,
        r#"{"cards":["noteFld","template","deck","cardDue","cardMod","noteTags"],"notes":["noteFld","note","template","noteTags","noteCrt","fld:Front"],"sort":{"cards":["deck",false],"notes":["noteFld",false]}}"#
    );
    let list = shown(c, Some(nt));
    assert!(list.own);
    assert_eq!(list.set, own_set);

    // A later change of the standard set leaves the own set alone.
    assert!(change(c, SetId::Standard, Cards, Change::Hide(col(Column::Tags))));
    assert_eq!(stored(c).unwrap().get(&nt.0.to_string()).unwrap().compact(), own);
    assert!(shown(c, Some(nt)).columns.columns(Cards).contains(&col(Column::Tags)));

    // Use Standard Columns: a copy of the standard set as it is now.
    assert!(column_sets::use_standard(c, nt).unwrap().value);
    let list = shown(c, Some(nt));
    assert!(list.own);
    assert_eq!(list.columns, shown(c, None).columns);
    assert!(!column_sets::use_standard(c, nt).unwrap().value, "the same copy writes nothing");
}

#[test]
fn a_set_whose_note_type_was_removed_is_ignored_and_kept() {
    let h = with_anki_keys();
    let c = &h.collection;
    let text = format!(r#"{{"{}":{{"cards":["fld:Front"],"sort":{{"cards":["fld:Front",true]}}}}}}"#, REMOVED.0);
    put(c, KEY, &text);
    let list = shown(c, Some(REMOVED));
    assert_eq!((list.set, list.own), (SetId::Standard, false));
    assert_eq!(list.columns.columns(Cards), default_columns(Cards).as_slice());

    let before = config_rows(c);
    let refused = column_sets::change(c, SetId::Notetype(REMOVED), Cards, Change::Show(col(Column::Deck)));
    assert!(
        matches!(refused, Err(EngineError::NotFound { table: Table::Notetypes, .. })),
        "{refused:?}"
    );
    assert!(column_sets::use_standard(c, REMOVED).is_err());
    assert_eq!(config_rows(c), before, "nothing written");

    assert!(change(c, SetId::Standard, Cards, Change::Show(col(Column::Tags))));
    let member = REMOVED.0.to_string();
    assert_eq!(stored(c).unwrap().get(&member), json(&text).get(&member), "kept as it is");
}

#[test]
fn refusals_write_nothing() {
    let h = with_anki_keys();
    let c = &h.collection;
    put(c, KEY, r#"{"std":{"cards":["deck"]}}"#);
    let before = config_rows(c);
    let kind = |result: Result<meguri_engine::OpResult<bool>, EngineError>| match result {
        Err(EngineError::InvalidArgument { message }) => message.split(':').next().unwrap().to_owned(),
        other => panic!("not a refusal: {other:?}"),
    };
    assert_eq!(
        kind(column_sets::change(c, SetId::Standard, Cards, Change::Hide(col(Column::Deck)))),
        "last_column"
    );
    assert_eq!(
        kind(column_sets::change(
            c,
            SetId::Standard,
            Cards,
            Change::Reorder(vec![col(Column::Deck), col(Column::Deck)])
        )),
        "invalid_order"
    );
    assert_eq!(
        kind(column_sets::change(c, SetId::Standard, Cards, Change::SortBy(col(Column::Question)))),
        "unsortable_column"
    );
    assert_eq!(
        kind(column_sets::change(
            c,
            SetId::Standard,
            Notes,
            Change::SetSort(col(Column::Stability), SortDirection::Ascending)
        )),
        "unsortable_column"
    );
    assert_eq!(config_rows(c), before);
}

/// V2.4: the current-deck selection's skip-undo class. A column change
/// records no step, keeps "Undo Suspend Cards" and empties redo, changed or
/// not (`engine/undo` A2).
#[test]
fn a_column_change_records_no_step_and_keeps_undo_suspend() {
    let h = with_anki_keys();
    let c = &h.collection;
    let nt = notetype(c);
    let (_, card) = common::add_note_with_card(c, OpSource::UserUndoable, &["front", "back"]).value;
    let top = |c: &Collection| c.peek_undo(UndoScope::Global).unwrap().name();
    let added = top(c);
    assert!(added.is_some());
    let suspended = ops::run(c, Request::Suspend(Targets::Cards(vec![card])), &Default::default()).unwrap();
    assert_eq!(suspended.value.count, 1);
    let redo = |c: &Collection| c.peek_redo(UndoScope::Global).unwrap().name();
    let suspend = top(c);
    assert_eq!(suspend, Some(meguri_engine::undo::names::SUSPEND));

    h.clock.step_ms(3_600_000);
    let result = column_sets::change(c, SetId::Standard, Cards, Change::Show(col(Column::Tags))).unwrap();
    assert!(result.value);
    assert_eq!(result.undo, None, "no undo step");
    assert_eq!(top(c), suspend, "Undo Suspend Cards stays");
    let result = column_sets::change(c, SetId::Notetype(nt), Cards, Change::SortBy(col(Column::Due))).unwrap();
    assert!(result.value);
    assert_eq!(top(c), suspend);

    // The undo still restores the card, and the column sets stay.
    let sets = stored(c);
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(stored(c), sets, "undoing the suspend leaves the column sets");
    assert_eq!(redo(c), suspend);

    // A change empties redo and keeps the step below.
    assert!(change(c, SetId::Standard, Cards, Change::Hide(col(Column::Tags))));
    assert_eq!(redo(c), None, "a column change empties redo");
    assert_eq!(top(c), added, "the step before the suspend is kept");

    // A change of nothing writes nothing and still empties redo.
    c.undo(UndoScope::Global).unwrap();
    assert!(redo(c).is_some());
    let before = config_rows(c);
    assert!(!change(c, SetId::Standard, Cards, Change::Show(col(Column::SortField))));
    assert_eq!(config_rows(c), before);
    assert_eq!(redo(c), None);
}
