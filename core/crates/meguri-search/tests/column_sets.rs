//! Browse's column sets (`specs/browser/workspace.md` V2.2 to V2.4, V4.1,
//! V4.3): what `meguriBrowserColumnSets` reads as and the whole value each
//! change computes. The ops that write it, their stamps and their undo class
//! are meguri-engine's `tests/column_sets.rs`.

use meguri_model::ids::NotetypeId;
use meguri_search::SearchMode::{Cards, Notes};
use meguri_search::config::column_sets::{Change, ColumnSets, SetId, SetSort, default_columns};
use meguri_search::config::{Column, ColumnKey, Refusal, SortDirection};
use meguri_storage::ConfigValue;

use SortDirection::{Ascending, Descending};

fn value(text: &str) -> ConfigValue {
    ConfigValue::parse(text.as_bytes()).unwrap_or_else(|| panic!("{text:?} is JSON"))
}

fn read(text: &str) -> ColumnSets {
    ColumnSets::from_stored(Some(&value(text)))
}

fn col(column: Column) -> ColumnKey {
    ColumnKey::Column(column)
}

fn field(name: &str) -> ColumnKey {
    ColumnKey::Field(name.to_owned())
}

const NT: NotetypeId = NotetypeId(1_700_000_000_001);

/// The compact text a change writes.
fn written(sets: &ColumnSets, set: SetId, mode: meguri_search::SearchMode, change: Change) -> Option<String> {
    sets.change(set, mode, &change).unwrap().map(|value| value.compact())
}

#[test]
fn the_spec_example_reads_as_written() {
    let sets = read(
        r#"{"std":{"cards":["noteFld","template","deck","cardDue","cardMod"],"sort":{"cards":["noteFld",false]}},"1700000000001":{"notes":["fld:Front","fld:Back","fld:Extra","deck","cardDue"]}}"#,
    );
    let std = sets.standard();
    assert_eq!(std.columns(Cards), default_columns(Cards).as_slice());
    assert_eq!(std.columns(Notes), default_columns(Notes).as_slice(), "a missing member reads as its default");
    assert!(sets.has_own(NT));
    let own = sets.set(SetId::Notetype(NT));
    assert_eq!(
        own.columns(Notes),
        [field("Front"), field("Back"), field("Extra"), col(Column::Deck), col(Column::Due)]
    );
    assert_eq!(own.columns(Cards), default_columns(Cards).as_slice());
    assert_eq!(
        *own.sort(Notes),
        SetSort {
            column: col(Column::SortField),
            direction: Ascending
        }
    );
    // A note type without an own set uses the standard set, live.
    assert!(!sets.has_own(NotetypeId(1)));
    assert_eq!(sets.set(SetId::Notetype(NotetypeId(1))), std);
}

#[test]
fn nothing_stored_reads_the_defaults() {
    for sets in [ColumnSets::from_stored(None), read("{}"), read("[]"), read(r#"{"std":null}"#)] {
        let std = sets.standard();
        for mode in [Cards, Notes] {
            assert_eq!(std.columns(mode), default_columns(mode).as_slice());
            assert_eq!(std.sort(mode).column, col(Column::SortField));
            assert_eq!(std.sort(mode).direction, Ascending);
        }
    }
}

#[test]
fn column_keys_round_trip() {
    for column in Column::ALL {
        assert_eq!(ColumnKey::parse(column.key()), Some(col(column)));
        assert_eq!(col(column).key(), column.key());
    }
    assert_eq!(ColumnKey::parse("fld:A:b"), Some(field("A:b")), "the stored name after the prefix");
    assert_eq!(field("Front").key(), "fld:Front");
    assert_eq!(ColumnKey::parse("Front"), None);
    assert_eq!(ColumnKey::parse("noteFLD"), None, "keys compare exactly");
}

#[test]
fn a_header_click_sorts_in_the_first_direction_then_reverses() {
    let empty = ColumnSets::from_stored(None);
    let cases = [
        (field("Back"), r#"["fld:Back",false]"#),
        (col(Column::Due), r#"["cardDue",true]"#),
        (col(Column::Deck), r#"["deck",false]"#),
        (col(Column::SortField), r#"["noteFld",true]"#),
    ];
    for (column, sort) in cases {
        assert_eq!(
            written(&empty, SetId::Standard, Notes, Change::SortBy(column.clone())),
            Some(format!(r#"{{"std":{{"sort":{{"notes":{sort}}}}}}}"#)),
            "{column:?}"
        );
    }
    let sorted = read(r#"{"std":{"sort":{"cards":["fld:Back",false]}}}"#);
    assert_eq!(
        written(&sorted, SetId::Standard, Cards, Change::SortBy(field("Back"))).as_deref(),
        Some(r#"{"std":{"sort":{"cards":["fld:Back",true]}}}"#)
    );
    assert_eq!(
        written(&sorted, SetId::Standard, Cards, Change::SetSort(field("Back"), Ascending)),
        None,
        "the same sort writes nothing"
    );
}

#[test]
fn a_sort_that_cannot_sort_in_its_mode_falls_back() {
    let sets = read(
        r#"{"std":{"sort":{"cards":["answer",true],"notes":["retrievability",true]}},"1700000000001":{"sort":{"notes":["nothing",true]}}}"#,
    );
    for (set, mode) in [(SetId::Standard, Cards), (SetId::Standard, Notes), (SetId::Notetype(NT), Notes)] {
        let read = sets.set(set);
        assert_eq!(read.sort(mode).column, col(Column::SortField), "{set:?} {mode:?}");
        assert_eq!(read.sort(mode).direction, Ascending);
    }
    assert_eq!(sets.standard().sort_request(Notes).direction(), Ascending);
    let stability = read(r#"{"std":{"sort":{"cards":["stability",true]}}}"#);
    assert_eq!(stability.standard().sort(Cards).column, col(Column::Stability));
}

#[test]
fn show_hide_and_reorder() {
    let sets = read(r#"{"std":{"cards":["noteFld","deck"]}}"#);
    let std = SetId::Standard;
    assert_eq!(
        written(&sets, std, Cards, Change::Show(field("Front"))).as_deref(),
        Some(r#"{"std":{"cards":["noteFld","deck","fld:Front"]}}"#),
        "checking adds the column at the end"
    );
    assert_eq!(written(&sets, std, Cards, Change::Show(col(Column::Deck))), None);
    assert_eq!(
        written(&sets, std, Cards, Change::Hide(col(Column::Deck))).as_deref(),
        Some(r#"{"std":{"cards":["noteFld"]}}"#)
    );
    assert_eq!(written(&sets, std, Cards, Change::Hide(col(Column::Tags))), None);
    assert_eq!(
        written(&sets, std, Cards, Change::Reorder(vec![col(Column::Deck), col(Column::SortField)])).as_deref(),
        Some(r#"{"std":{"cards":["deck","noteFld"]}}"#)
    );
    assert_eq!(
        written(&sets, std, Cards, Change::Reorder(vec![col(Column::SortField), col(Column::Deck)])),
        None
    );
    let one = read(r#"{"std":{"cards":["deck"]}}"#);
    assert_eq!(
        one.change(std, Cards, &Change::Hide(col(Column::Deck))),
        Err(Refusal::LastColumn)
    );
    for order in [
        vec![col(Column::SortField)],
        vec![col(Column::SortField), col(Column::SortField)],
        vec![col(Column::SortField), col(Column::Deck), col(Column::Tags)],
        vec![col(Column::SortField), col(Column::Tags)],
    ] {
        assert_eq!(
            sets.change(std, Cards, &Change::Reorder(order.clone())),
            Err(Refusal::InvalidOrder),
            "{order:?}"
        );
    }
    for (mode, column) in [(Cards, Column::Question), (Cards, Column::Answer), (Notes, Column::Difficulty)] {
        assert_eq!(
            sets.change(std, mode, &Change::SortBy(col(column))),
            Err(Refusal::UnsortableColumn)
        );
        assert_eq!(
            sets.change(std, mode, &Change::SetSort(col(column), Descending)),
            Err(Refusal::UnsortableColumn)
        );
    }
    assert!(Change::SortBy(field("x")).check(Notes).is_ok(), "a field column is never refused");
}

#[test]
fn a_note_types_first_change_writes_a_copy_of_the_standard_set() {
    let sets = read(r#"{"std":{"cards":["deck"],"x":1,"sort":{"notes":["cardDue",true],"y":2}},"9":"kept"}"#);
    assert_eq!(
        written(&sets, SetId::Notetype(NT), Cards, Change::Show(field("Front"))).as_deref(),
        Some(
            r#"{"1700000000001":{"cards":["deck","fld:Front"],"notes":["noteFld","note","template","noteTags","noteCrt"],"sort":{"cards":["noteFld",false],"notes":["cardDue",true]}},"9":"kept","std":{"cards":["deck"],"sort":{"notes":["cardDue",true],"y":2},"x":1}}"#
        )
    );
    assert_eq!(sets.use_standard(NT), None, "no own set to reset");
    assert_eq!(
        sets.use_standard(NotetypeId(9)).map(|value| value.compact()).as_deref(),
        None,
        "a set of another shape is no own set"
    );
    let own = read(r#"{"std":{"cards":["deck"],"w":0},"1700000000001":{"cards":["fld:Front"],"z":true,"sort":{"y":1}}}"#);
    assert_eq!(
        own.use_standard(NT).map(|value| value.compact()).as_deref(),
        Some(
            r#"{"1700000000001":{"cards":["deck"],"notes":["noteFld","note","template","noteTags","noteCrt"],"sort":{"cards":["noteFld",false],"notes":["noteFld",false],"y":1},"z":true},"std":{"cards":["deck"],"w":0}}"#
        )
    );
    assert_eq!(
        written(&own, SetId::Notetype(NT), Cards, Change::Show(col(Column::Deck))).as_deref(),
        Some(r#"{"1700000000001":{"cards":["fld:Front","deck"],"sort":{"y":1},"z":true},"std":{"cards":["deck"],"w":0}}"#),
        "an own set's later change writes only its member, unknown members kept"
    );
}

/// V2.4 as decided on 2026-11-21: unknown and repeated keys are
/// skipped on reading, and dropped only when their set is written.
#[test]
fn a_list_reads_as_its_known_keys_each_once() {
    let sets = read(
        r#"{"std":{"cards":["later","deck","noteFld","deck",7,"fld:A"],"notes":["future"],"x":{"keep":[1]}},"5":{"cards":["unknown","deck"]}}"#,
    );
    let std = sets.standard();
    assert_eq!(std.columns(Cards), [col(Column::Deck), col(Column::SortField), field("A")]);
    assert_eq!(std.columns(Notes), default_columns(Notes).as_slice(), "no known key: the default");
    assert_eq!(sets.set(SetId::Notetype(NotetypeId(5))).columns(Cards), [col(Column::Deck)]);
    assert_eq!(
        written(&sets, SetId::Standard, Cards, Change::Show(col(Column::Tags))).as_deref(),
        Some(
            r#"{"5":{"cards":["unknown","deck"]},"std":{"cards":["deck","noteFld","fld:A","noteTags"],"notes":["noteFld","note","template","noteTags","noteCrt"],"x":{"keep":[1]}}}"#
        )
    );
    assert_eq!(
        written(&sets, SetId::Standard, Cards, Change::SortBy(col(Column::Deck))).as_deref(),
        Some(
            r#"{"5":{"cards":["unknown","deck"]},"std":{"cards":["deck","noteFld","fld:A"],"notes":["noteFld","note","template","noteTags","noteCrt"],"sort":{"cards":["deck",false]},"x":{"keep":[1]}}}"#
        ),
        "a sort change writes the set's lists as they read"
    );
    assert_eq!(written(&sets, SetId::Standard, Cards, Change::Show(col(Column::Deck))), None);
}
