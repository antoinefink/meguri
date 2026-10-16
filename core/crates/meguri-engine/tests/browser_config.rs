//! The browser and search config writes as ops (`specs/browser/config-keys.md`
//! K4.2 to K7.2, K9, K10): what each action stores and stamps, that it
//! touches no other key, that a write of nothing writes nothing, and the
//! undo class of K10.2. The values themselves are checked against the
//! spec's tables in meguri-search's `tests/config_keys.rs`.

mod common;

use common::Harness;
use common::tagging::{fresh, raw};
use meguri_engine::browser_config::{self, refused};
use meguri_engine::undo::names::{DELETE_SAVED_SEARCH, RENAME_SAVED_SEARCH, SAVE_SEARCH, UPDATE_SAVED_SEARCH};
use meguri_engine::{ChangeKind, Collection, DisabledReason, EngineError, OpSpec, UndoScope, UndoStatus};
use meguri_model::changes::OpSource;
use meguri_search::SearchMode::{Cards, Notes};
use meguri_search::config::{Column, Key, Refusal, Section, SortDirection};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};

/// One hour after the harness's start, in seconds.
const LATER_S: i64 = common::NOW_MS / 1000 + 3600;

const TEST_STEP: &str = "Test Step";

fn json(text: &str) -> ConfigValue {
    ConfigValue::parse(text.as_bytes()).unwrap_or_else(|| panic!("{text:?} is JSON"))
}

/// Every config row: key, usn, mtime, value text and storage class.
fn config_rows(c: &Collection) -> Vec<Vec<SqlValue>> {
    let reader = c.reader(ReaderRole::Query).unwrap();
    reader
        .execute_raw(
            "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT), typeof(val) FROM config ORDER BY KEY",
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

/// The rows that differ between two reads, by key.
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

/// The stored value of `key` in compact form; `None` when unreadable.
fn stored(c: &Collection, key: &str) -> Option<String> {
    c.reader(ReaderRole::Query)
        .unwrap()
        .config_value(key)
        .unwrap()
        .map(|v| v.compact())
}

/// Stores `text` as `key`'s bytes, stamped as another client's write.
fn put(c: &Collection, key: &str, text: &str) {
    raw(
        c,
        "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES (?, 0, 1700000000, CAST(? AS BLOB))",
        &[SqlValue::Text(key.to_owned()), SqlValue::Text(text.to_owned())],
    );
}

/// An undoable op of the test: it counts up config key `testKey`, adding it
/// the first time.
fn undoable_op(c: &Collection) {
    let result = c
        .run(
            OpSpec::named("test_undoable", OpSource::UserUndoable).undo_step(TEST_STEP, Default::default()),
            |ctx| {
                let scope = ctx.scope();
                let count = scope.config_value("testKey")?.and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(scope.set_config("testKey", &ConfigValue::from(count + 1))?)
            },
        )
        .unwrap();
    assert!(result.value.changed());
    assert_eq!(top(c), Some(TEST_STEP));
}

fn top(c: &Collection) -> Option<&'static str> {
    c.peek_undo(UndoScope::Global).unwrap().name()
}

fn cleared(c: &Collection) -> bool {
    c.peek_undo(UndoScope::Global).unwrap() == UndoStatus::Unavailable(DisabledReason::ClearedByNonUndoableOp)
}

fn kind_of(error: EngineError) -> String {
    match error {
        EngineError::InvalidArgument { message } => message.split(':').next().unwrap_or_default().to_owned(),
        other => panic!("not a refusal: {other:?}"),
    }
}

/// A harness with none of the browser keys stored.
fn bare() -> Harness {
    let h = fresh();
    let names: Vec<String> = Key::ALL.iter().map(|k| format!("'{}'", k.name())).collect();
    raw(
        &h.collection,
        &format!("DELETE FROM config WHERE KEY IN ({})", names.join(", ")),
        &[],
    );
    h
}

#[test]
fn toggling_notes_mode_writes_its_key_with_no_undo_entry_and_clears_the_history() {
    let h = bare();
    let c = &h.collection;
    undoable_op(c);
    h.clock.step_ms(3_600_000);
    let before = config_rows(c);
    let result = browser_config::set_notes_mode(c, true).unwrap();
    assert!(result.value);
    assert_eq!(result.undo, None, "no undo entry");
    let changed: Vec<(&str, ChangeKind)> = result
        .changes
        .config_keys
        .iter()
        .map(|k| (k.key.as_str(), k.kind))
        .collect();
    assert_eq!(changed, [("browserTableShowNotesMode", ChangeKind::Added)]);
    let after = config_rows(c);
    assert_eq!(changed_keys(&before, &after), ["browserTableShowNotesMode"]);
    assert_eq!(
        row(&after, "browserTableShowNotesMode").unwrap()[1..],
        [
            SqlValue::Integer(-1),
            SqlValue::Integer(LATER_S),
            SqlValue::Text("true".to_owned()),
            SqlValue::Text("blob".to_owned())
        ]
    );
    assert_eq!(common::col_mod(c), common::NOW_MS + 3_600_000);
    assert!(cleared(c), "a change the user cannot undo clears the history");
    assert!(c.undo(UndoScope::Global).is_err());

    // Back to cards: one more write, still no entry.
    let result = browser_config::set_notes_mode(c, false).unwrap();
    assert!(result.value);
    assert_eq!(stored(c, "browserTableShowNotesMode").as_deref(), Some("false"));
}

#[test]
fn an_action_that_writes_nothing_keeps_the_history() {
    let h = bare();
    let c = &h.collection;
    browser_config::set_notes_mode(c, true).unwrap();
    undoable_op(c);
    h.clock.step_ms(3_600_000);
    let before = (config_rows(c), common::col_mod(c));
    // The column is displayed by default, the mode is stored already, a
    // hidden column is hidden: nothing to write (K4.2, C4).
    assert!(!browser_config::show_column(c, Cards, Column::SortField).unwrap().value);
    assert!(!browser_config::set_notes_mode(c, true).unwrap().value);
    assert!(!browser_config::hide_column(c, Notes, Column::Due).unwrap().value);
    assert_eq!((config_rows(c), common::col_mod(c)), before);
    assert_eq!(top(c), Some(TEST_STEP), "the user's undo step is kept");
}

#[test]
fn a_saved_search_is_an_undo_step_on_top_of_an_earlier_one() {
    let h = bare();
    let c = &h.collection;
    undoable_op(c);
    h.clock.step_ms(3_600_000);
    let result = browser_config::save_search(c, "Leeches", "tag:leech").unwrap();
    assert!(result.value);
    assert_eq!(result.undo.map(|u| u.name), Some(SAVE_SEARCH));
    assert_eq!(top(c), Some(SAVE_SEARCH));
    assert_eq!(stored(c, "savedFilters").as_deref(), Some(r#"{"Leeches":"tag:leech"}"#));

    c.undo(UndoScope::Global).unwrap();
    assert_eq!(stored(c, "savedFilters"), None, "the saved searches as they were");
    assert_eq!(top(c), Some(TEST_STEP));
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(stored(c, "testKey"), None, "then the earlier op's rows");
    c.redo(UndoScope::Global).unwrap();
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(stored(c, "savedFilters").as_deref(), Some(r#"{"Leeches":"tag:leech"}"#));
}

/// A write that changes nothing, after an undo. K10.2 takes its undo classes
/// from U25 (`engine/undo` M2, A2): a non-undoable action leaves both
/// histories, while a saved-search action is an undoable op, so it records
/// no step and keeps the undo steps but empties the redo history.
#[test]
fn a_saved_search_write_of_nothing_keeps_the_undo_steps_and_empties_redo() {
    type Action = fn(&Collection) -> Result<meguri_engine::OpResult<bool>, EngineError>;
    let nothing_to_write: [(&str, Action); 4] = [
        ("save the same search", |c| {
            browser_config::save_search(c, "A", "is:new")
        }),
        ("update to the same search", |c| {
            browser_config::update_saved_search(c, "A", "is:new")
        }),
        ("rename a missing name", |c| {
            browser_config::rename_saved_search(c, "missing", "R")
        }),
        ("delete a missing name", |c| {
            browser_config::delete_saved_searches(c, &["missing"])
        }),
    ];
    let h = bare();
    let c = &h.collection;
    put(c, "savedFilters", r#"{"A":"is:new"}"#);
    undoable_op(c);
    h.clock.step_ms(3_600_000);
    let redo = |c: &Collection| c.peek_redo(UndoScope::Global).unwrap().name();
    for (action, write) in nothing_to_write {
        browser_config::save_search(c, "Leeches", "tag:leech").unwrap();
        c.undo(UndoScope::Global).unwrap();
        assert_eq!(redo(c), Some(SAVE_SEARCH), "{action}");
        assert_eq!(top(c), Some(TEST_STEP), "{action}");
        let before = (config_rows(c), common::col_mod(c));

        // Cards mode shows the sort field by default: nothing to write.
        assert!(!browser_config::show_column(c, Cards, Column::SortField).unwrap().value);
        assert_eq!(redo(c), Some(SAVE_SEARCH), "{action}: a non-undoable op keeps redo");

        let result = write(c).unwrap();
        assert!(!result.value, "{action}");
        assert_eq!(result.undo, None, "{action}: no entry");
        assert_eq!(
            (config_rows(c), common::col_mod(c)),
            before,
            "{action}: nothing written"
        );
        assert_eq!(redo(c), None, "{action}: an undoable op empties redo, changed or not");
        assert_eq!(top(c), Some(TEST_STEP), "{action}: the undo steps are kept");
    }
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(stored(c, "testKey"), None, "the earlier step still undoes");
}

#[test]
fn each_saved_search_write_names_its_step_and_writing_nothing_records_none() {
    let h = bare();
    let c = &h.collection;
    put(c, "savedFilters", r#"{"A":"is:new","n":3}"#);
    let step = |result: Result<meguri_engine::OpResult<bool>, EngineError>| {
        let result = result.unwrap();
        assert!(result.value);
        result.undo.map(|u| u.name)
    };
    assert_eq!(
        step(browser_config::update_saved_search(c, "A", "is:due")),
        Some(UPDATE_SAVED_SEARCH)
    );
    assert_eq!(
        step(browser_config::rename_saved_search(c, "A", r#""B""#)),
        Some(RENAME_SAVED_SEARCH)
    );
    assert_eq!(stored(c, "savedFilters").as_deref(), Some(r#"{"B":"is:due","n":3}"#));
    assert_eq!(
        step(browser_config::delete_saved_searches(c, &["B", "Z"])),
        Some(DELETE_SAVED_SEARCH)
    );
    assert_eq!(stored(c, "savedFilters").as_deref(), Some(r#"{"n":3}"#));

    let before = config_rows(c);
    for result in [
        browser_config::save_search(c, "Q", "x").unwrap(),
        browser_config::save_search(c, "Q", "x").unwrap(),
        browser_config::update_saved_search(c, "Q", "x").unwrap(),
        browser_config::rename_saved_search(c, "Q", "\"\"").unwrap(),
        browser_config::rename_saved_search(c, "missing", "R").unwrap(),
        browser_config::delete_saved_searches(c, &["missing"]).unwrap(),
    ]
    .into_iter()
    .skip(1)
    {
        assert!(!result.value);
        assert_eq!(result.undo, None, "nothing written, no entry");
    }
    assert_eq!(changed_keys(&before, &config_rows(c)), ["savedFilters"]);
    assert_eq!(top(c), Some(SAVE_SEARCH), "the steps of writes that changed something");
}

/// The plan's scenario: a `flagLabels` write keeps every other key byte for
/// byte, the two Reposition keys included (K9, R1266).
#[test]
fn a_flag_labels_write_touches_no_other_key() {
    let h = fresh();
    let c = &h.collection;
    put(c, "activeNoteCols", r#"[ "noteFld" , "addonY" ]"#);
    put(c, "randomOrderReposition", "true");
    put(c, "shiftPositionOfExistingCards", " false ");
    put(c, "otherClient_layout", r#"{"z": 1, "a" :[1, 2.50]}"#);
    put(c, "previewBothSides", " false ");
    h.clock.step_ms(3_600_000);
    let before = config_rows(c);
    let result = browser_config::set_value(c, Key::FlagLabels, &json(r#"{"3":"Important"}"#)).unwrap();
    assert!(result.value);
    assert_eq!(result.undo, None);
    let after = config_rows(c);
    assert_eq!(changed_keys(&before, &after), ["flagLabels"]);
    assert_eq!(
        row(&after, "flagLabels").unwrap()[1..],
        [
            SqlValue::Integer(-1),
            SqlValue::Integer(LATER_S),
            SqlValue::Text(r#"{"3":"Important"}"#.to_owned()),
            SqlValue::Text("blob".to_owned())
        ]
    );
}

#[test]
fn whole_value_writes_take_only_the_keys_shape() {
    let h = bare();
    let c = &h.collection;
    let before = config_rows(c);
    for (key, value) in [
        (Key::NormalizeNoteText, "false"),
        (Key::SortBackwards, "1"),
        (Key::ActiveCols, r#"["noteFld",5]"#),
        (Key::SavedFilters, r#"{"a":3}"#),
        (Key::SetDueBrowser, "null"),
    ] {
        assert!(
            matches!(
                browser_config::set_value(c, key, &json(value)),
                Err(EngineError::InvalidArgument { .. })
            ),
            "{key} {value}"
        );
    }
    assert_eq!(config_rows(c), before);
    for (key, value) in [
        (Key::ActiveCols, r#"["question","addonX",""]"#),
        (Key::SortType, r#""myAddonColumn""#),
        (Key::SavedFilters, r#"{"b":"x","a":"y"}"#),
        (Key::SetDueBrowser, r#""1-7!""#),
        (Key::ResetCountsBrowser, "true"),
    ] {
        assert!(browser_config::set_value(c, key, &json(value)).unwrap().value, "{key}");
        assert_eq!(stored(c, key.name()), Some(json(value).compact()), "{key}");
    }
}

#[test]
fn refusals_write_nothing_and_keep_the_history() {
    let h = bare();
    let c = &h.collection;
    put(c, "activeCols", r#"["deck","addonX"]"#);
    undoable_op(c);
    let before = (config_rows(c), common::col_mod(c));
    let refusals = [
        browser_config::hide_column(c, Cards, Column::Deck).unwrap_err(),
        browser_config::reorder_columns(c, Cards, &[Column::Deck, Column::Due]).unwrap_err(),
        browser_config::sort_by_column(c, Cards, Column::Question).unwrap_err(),
        browser_config::set_sort(c, Notes, Column::Stability, SortDirection::Descending).unwrap_err(),
        browser_config::save_search(c, "", "is:due").unwrap_err(),
    ];
    let kinds: Vec<String> = refusals.into_iter().map(kind_of).collect();
    assert_eq!(
        kinds,
        [
            "last_column",
            "invalid_order",
            "unsortable_column",
            "unsortable_column",
            "empty_name"
        ]
    );
    assert_eq!(kind_of(refused(Refusal::LastColumn)), "last_column");
    assert_eq!((config_rows(c), common::col_mod(c)), before);
    assert_eq!(top(c), Some(TEST_STEP));
}

/// K4.2, K4.3, K9: column writes store the list and touch only their mode's
/// key.
#[test]
fn column_writes_store_their_modes_list() {
    let h = bare();
    let c = &h.collection;
    put(c, "activeCols", r#"["noteFld","addonX","deck"]"#);
    let before = config_rows(c);
    assert!(browser_config::show_column(c, Cards, Column::Due).unwrap().value);
    assert_eq!(
        stored(c, "activeCols").as_deref(),
        Some(r#"["noteFld","addonX","deck","cardDue"]"#)
    );
    assert!(
        browser_config::reorder_columns(c, Cards, &[Column::Due, Column::SortField, Column::Deck])
            .unwrap()
            .value
    );
    assert_eq!(
        stored(c, "activeCols").as_deref(),
        Some(r#"["cardDue","addonX","noteFld","deck"]"#)
    );
    assert!(
        !browser_config::reorder_columns(c, Cards, &[Column::Due, Column::SortField, Column::Deck])
            .unwrap()
            .value,
        "the same order writes nothing"
    );
    assert!(browser_config::hide_column(c, Cards, Column::SortField).unwrap().value);
    assert_eq!(
        stored(c, "activeCols").as_deref(),
        Some(r#"["cardDue","addonX","deck"]"#)
    );
    assert_eq!(changed_keys(&before, &config_rows(c)), ["activeCols"]);

    assert!(browser_config::hide_column(c, Notes, Column::Tags).unwrap().value);
    assert_eq!(
        stored(c, "activeNoteCols").as_deref(),
        Some(r#"["noteFld","note","template"]"#)
    );
}

/// K5.2's first rows, K9: a sort writes only its mode's two keys, and of
/// those only the ones whose value changes (C4).
#[test]
fn a_sort_writes_only_the_sort_keys_that_change() {
    let h = bare();
    let c = &h.collection;
    put(c, "sortType", r#""noteFld""#);
    put(c, "sortBackwards", "false");
    h.clock.step_ms(3_600_000);
    let before = config_rows(c);
    assert!(
        browser_config::sort_by_column(c, Cards, Column::SortField)
            .unwrap()
            .value
    );
    let after = config_rows(c);
    assert_eq!(changed_keys(&before, &after), ["sortBackwards"]);
    assert_eq!(stored(c, "sortBackwards").as_deref(), Some("true"));

    assert!(browser_config::sort_by_column(c, Cards, Column::Due).unwrap().value);
    assert_eq!(changed_keys(&after, &config_rows(c)), ["sortType"]);
    assert_eq!(stored(c, "sortType").as_deref(), Some(r#""cardDue""#));

    let before = config_rows(c);
    assert!(
        browser_config::set_sort(c, Notes, Column::Created, SortDirection::Ascending)
            .unwrap()
            .value
    );
    assert_eq!(
        changed_keys(&before, &config_rows(c)),
        ["browserNoteSortBackwards", "noteSortType"],
        "both inserted, the default direction included"
    );
    assert_eq!(stored(c, "noteSortType").as_deref(), Some(r#""noteCrt""#));
    assert_eq!(stored(c, "browserNoteSortBackwards").as_deref(), Some("false"));
}

/// K6.2 to K6.5 and K10.2's first two rows: each writes its key, and a
/// change clears the history.
#[test]
fn sidebar_previewer_and_settings_writes_clear_the_history_when_they_change() {
    let h = bare();
    let c = &h.collection;
    for section in Section::ALL {
        undoable_op(c);
        assert!(browser_config::set_section_collapsed(c, section, true).unwrap().value);
        assert_eq!(stored(c, section.key().name()).as_deref(), Some("true"));
        assert!(cleared(c), "{section:?}");
    }
    undoable_op(c);
    assert!(browser_config::set_preview_both_sides(c, true).unwrap().value);
    assert!(cleared(c));
    undoable_op(c);
    assert!(browser_config::set_default_search_text(c, "  deck:x ").unwrap().value);
    assert_eq!(stored(c, "defaultSearchText").as_deref(), Some(r#""  deck:x ""#));
    assert!(cleared(c));
    undoable_op(c);
    assert!(browser_config::set_ignore_accents_in_search(c, true).unwrap().value);
    assert_eq!(stored(c, "ignoreAccentsInSearch").as_deref(), Some("true"));
    assert!(cleared(c));
    undoable_op(c);
    assert!(!browser_config::set_ignore_accents_in_search(c, true).unwrap().value);
    assert!(!browser_config::set_default_search_text(c, "  deck:x ").unwrap().value);
    assert_eq!(top(c), Some(TEST_STEP), "unchanged settings keep the history");
}

/// K6.6: Set Due Date and Reset remember their input inside their own op; a
/// missing key is inserted even when the value is its default.
#[test]
fn remembered_inputs_are_written_inside_the_callers_op() {
    let h = bare();
    let c = &h.collection;
    let result = c
        .run(
            OpSpec::named("test_reset", OpSource::UserUndoable).undo_step(TEST_STEP, Default::default()),
            |ctx| {
                let scope = ctx.scope();
                Ok((
                    browser_config::remember_reset_options_in(scope, true, false)?,
                    browser_config::remember_set_due_in(scope, "0")?,
                ))
            },
        )
        .unwrap();
    assert_eq!(result.value, (true, true));
    assert_eq!(top(c), Some(TEST_STEP), "inside the caller's step");
    assert_eq!(stored(c, "restorePositionBrowser").as_deref(), Some("true"));
    assert_eq!(stored(c, "resetCountsBrowser").as_deref(), Some("false"));
    assert_eq!(stored(c, "setDueBrowser").as_deref(), Some(r#""0""#));
    let unchanged = c
        .run(OpSpec::named("test_set_due", OpSource::UserUndoable), |ctx| {
            let scope = ctx.scope();
            Ok((
                browser_config::remember_reset_options_in(scope, true, false)?,
                browser_config::remember_set_due_in(scope, "0")?,
            ))
        })
        .unwrap();
    assert_eq!(unchanged.value, (false, false));
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(stored(c, "setDueBrowser"), None, "undone with the caller's step");
}
