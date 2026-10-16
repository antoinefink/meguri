//! Card flags and flag names (`specs/tags/flags-and-marked.md` F2 to F4):
//! setting keeps the other bits and the card's `mod`, the toggles of review
//! and Browse, and renaming flags in `flagLabels`, each one undo step.

mod common;

use common::tagging::{card_with_flags, fresh, raw};
use meguri_engine::flags::{self, LABELS_KEY};
use meguri_engine::tags::RunOptions;
use meguri_engine::undo::names::{RENAME_FLAG, SET_FLAG};
use meguri_engine::{Collection, EngineError, UndoScope};
use meguri_model::card::Card;
use meguri_model::ids::CardId;
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};

fn opts() -> RunOptions {
    RunOptions::default()
}

fn card(c: &Collection, id: CardId) -> Card {
    c.reader(ReaderRole::Query)
        .unwrap()
        .card(id)
        .unwrap()
        .expect("the card exists")
}

fn labels(c: &Collection) -> Option<String> {
    c.reader(ReaderRole::Query)
        .unwrap()
        .config_value(LABELS_KEY)
        .unwrap()
        .map(|value| value.compact())
}

fn set_labels(c: &Collection, json: &str) {
    raw(
        c,
        "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES (?, 0, 0, CAST(? AS BLOB))",
        &[SqlValue::Text(LABELS_KEY.to_owned()), SqlValue::Text(json.to_owned())],
    );
}

fn top_step(c: &Collection) -> Option<&'static str> {
    c.peek_undo(UndoScope::Global).unwrap().name()
}

#[test]
fn setting_a_flag_keeps_the_other_bits_and_the_cards_mod() {
    let h = fresh();
    let c = &h.collection;
    let id = card_with_flags(c, 0b1000_0011);
    raw(c, "UPDATE cards SET usn = 3", &[]);
    let before = card(c, id);
    h.clock.step_ms(3_600_000);
    assert_eq!(flags::set_flag(c, &[id], 5, &opts()).unwrap().value, 1);
    let after = card(c, id);
    assert_eq!(after.flags, 0b1000_0101);
    assert_eq!(after.usn, Usn(-1));
    assert_eq!(after.mtime, before.mtime, "a flag change keeps mod");
    assert_eq!(common::col_mod(c), common::NOW_MS + 3_600_000);
    assert_eq!(top_step(c), Some(SET_FLAG));

    h.clock.step_ms(3_600_000);
    assert_eq!(
        flags::set_flag(c, &[id], 5, &opts()).unwrap().value,
        0,
        "the same flag writes nothing"
    );
    assert_eq!(common::col_mod(c), common::NOW_MS + 3_600_000);
    assert_eq!(flags::set_flag(c, &[id], 0, &opts()).unwrap().value, 1);
    assert_eq!(card(c, id).flags, 0b1000_0000);
    assert!(matches!(
        flags::set_flag(c, &[id], 8, &opts()),
        Err(EngineError::InvalidArgument { .. })
    ));
    assert_eq!(card(c, id).flags, 0b1000_0000);
}

#[test]
fn undoing_a_flag_restores_flags_usn_and_col_mod() {
    let h = fresh();
    let c = &h.collection;
    let id = card_with_flags(c, 2);
    raw(c, "UPDATE cards SET usn = 3", &[]);
    let (before, col_before) = (card(c, id), common::col_mod(c));
    h.clock.step_ms(3_600_000);
    flags::set_flag(c, &[id], 4, &opts()).unwrap();
    c.undo(UndoScope::Global).unwrap();
    let after = card(c, id);
    assert_eq!(
        (after.flags, after.usn, after.mtime),
        (before.flags, before.usn, before.mtime)
    );
    assert_eq!(common::col_mod(c), col_before);
}

#[test]
fn toggling_the_flag_a_card_has_removes_it() {
    let h = fresh();
    let c = &h.collection;
    let id = card_with_flags(c, 0b1000_0011);
    flags::toggle_flag(c, id, &[id], 5, &opts()).unwrap();
    assert_eq!(card(c, id).flags, 0b1000_0101);
    flags::toggle_flag(c, id, &[id], 5, &opts()).unwrap();
    assert_eq!(card(c, id).flags, 0b1000_0000);
    assert_eq!(top_step(c), Some(SET_FLAG));
}

#[test]
fn browse_toggles_every_selected_card_by_the_focused_cards_flag() {
    let h = fresh();
    let c = &h.collection;
    let focused = card_with_flags(c, 5);
    let red = card_with_flags(c, 1);
    let none = card_with_flags(c, 0);
    flags::toggle_flag(c, focused, &[focused, red, none], 5, &opts()).unwrap();
    assert_eq!([focused, red, none].map(|id| card(c, id).flags), [0, 0, 0]);
    flags::toggle_flag(c, red, &[focused, red, none], 5, &opts()).unwrap();
    assert_eq!([focused, red, none].map(|id| card(c, id).flags), [5, 5, 5]);
}

#[test]
fn renaming_a_flag_sets_its_member_and_restoring_removes_it() {
    let h = fresh();
    let c = &h.collection;
    set_labels(c, r#"{"4":"Look up","x":[1]}"#);
    assert!(flags::rename_flag(c, 1, "Hard").unwrap().value);
    assert_eq!(labels(c).as_deref(), Some(r#"{"1":"Hard","4":"Look up","x":[1]}"#));
    assert_eq!(top_step(c), Some(RENAME_FLAG));
    assert_eq!(
        flags::flag_names(&*c.reader(ReaderRole::Query).unwrap()).unwrap(),
        ["Hard", "Orange", "Green", "Look up", "Pink", "Turquoise", "Purple"]
    );
    assert!(flags::restore_flag_name(c, 1).unwrap().value);
    assert_eq!(labels(c).as_deref(), Some(r#"{"4":"Look up","x":[1]}"#));
    assert!(!flags::restore_flag_name(c, 1).unwrap().value, "nothing to restore");

    // One undo puts the name back.
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(labels(c).as_deref(), Some(r#"{"1":"Hard","4":"Look up","x":[1]}"#));
}

#[test]
fn a_rename_drops_quotes_and_an_equal_or_empty_name_writes_nothing_new() {
    let h = fresh();
    let c = &h.collection;
    assert!(
        !flags::rename_flag(c, 2, "Orange").unwrap().value,
        "the shown default name"
    );
    assert_eq!(labels(c), None);
    assert!(flags::rename_flag(c, 2, "\"Lo\"ok\"").unwrap().value);
    assert_eq!(labels(c).as_deref(), Some(r#"{"2":"Look"}"#));
    assert!(
        !flags::rename_flag(c, 2, "Look").unwrap().value,
        "the shown custom name"
    );
    assert!(
        flags::rename_flag(c, 2, "\"\"").unwrap().value,
        "empty after the quotes go: restore"
    );
    assert_eq!(labels(c).as_deref(), Some("{}"), "the key stays");
    assert!(matches!(
        flags::rename_flag(c, 8, "x"),
        Err(EngineError::InvalidArgument { .. })
    ));
}

#[test]
fn a_value_that_is_not_an_object_is_replaced() {
    let h = fresh();
    let c = &h.collection;
    set_labels(c, "[1,2]");
    assert!(flags::rename_flag(c, 3, "Done").unwrap().value);
    assert_eq!(labels(c).as_deref(), Some(r#"{"3":"Done"}"#));
    assert_eq!(
        c.reader(ReaderRole::Query).unwrap().config_value(LABELS_KEY).unwrap(),
        Some(ConfigValue::Object(vec![(
            "3".to_owned(),
            ConfigValue::String("Done".to_owned())
        )]))
    );
}
