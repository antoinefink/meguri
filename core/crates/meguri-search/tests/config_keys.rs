//! The browser and search config keys (`specs/browser/config-keys.md`):
//! every key's default and reading (K1, K2), the column catalog (K3), and
//! every row of the K4.4, K5.1, K5.2, K6.4 and K7.2 tables, with the
//! refusals. Expected values are the spec's. The writes these values go
//! through, their stamps and their undo class are meguri-engine's
//! `tests/browser_config.rs`.

use std::fs;
use std::path::Path;

use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_search::SearchMode::{self, Cards, Notes};
use meguri_search::config::{
    ActiveColumns, BrowserConfig, Column, Key, Refusal, SavedSearches, Section, Shape, Sort, SortDirection,
    default_search_is_set,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, OpenOptions, ReaderRole, open_collection};

use SortDirection::{Ascending, Descending};

/// A JSON text as a config value; `None` for text that is not JSON (C3).
fn json(text: &str) -> Option<ConfigValue> {
    ConfigValue::parse(text.as_bytes())
}

fn value(text: &str) -> ConfigValue {
    json(text).unwrap_or_else(|| panic!("{text:?} is JSON"))
}

/// A config where `key` holds `text` (`None`: missing or unreadable) and
/// every other key is missing.
fn with(key: Key, text: Option<&str>) -> BrowserConfig {
    BrowserConfig::from_stored(|k| if k == key { text.and_then(json) } else { None })
}

fn empty() -> BrowserConfig {
    BrowserConfig::from_stored(|_| None)
}

fn columns(mode: SearchMode, stored: Option<&str>) -> ActiveColumns {
    ActiveColumns::from_stored(mode, stored.and_then(json).as_ref())
}

fn keys(list: &[Column]) -> Vec<&'static str> {
    list.iter().map(|c| c.key()).collect()
}

fn col(key: &str) -> Column {
    Column::from_key(key).unwrap_or_else(|| panic!("{key} is a column key"))
}

fn compact(value: Option<ConfigValue>) -> Option<String> {
    value.map(|v| v.compact())
}

#[test]
fn every_key_reads_its_k1_default_when_absent() {
    let config = empty();
    assert_eq!(
        keys(&config.columns(Cards).displayed()),
        ["noteFld", "template", "cardDue", "deck"]
    );
    assert_eq!(
        keys(&config.columns(Notes).displayed()),
        ["noteFld", "note", "template", "noteTags"]
    );
    let created_ascending = Sort {
        column: Column::Created,
        direction: Ascending,
    };
    assert_eq!(config.sort(Cards), created_ascending);
    assert_eq!(config.sort(Notes), created_ascending);
    assert!(!config.notes_mode());
    assert!(config.saved_searches().list().is_empty());
    assert_eq!(config.flag_labels(), None);
    for section in Section::ALL {
        assert!(!config.section_collapsed(section), "{section:?}");
    }
    assert!(!config.preview_both_sides());
    assert_eq!(config.set_due_browser(), "0");
    assert_eq!(config.default_search_text(), "");
    assert_eq!(config.default_search(), None);
    assert!(!config.ignore_accents_in_search());
    assert!(config.restore_position_browser(), "K1: true by default");
    assert!(!config.reset_counts_browser());
    assert!(config.normalize_note_text(), "K1: true by default");
}

#[test]
fn the_keys_have_k1s_names_and_shapes() {
    let names: Vec<&str> = Key::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(
        names,
        [
            "activeCols",
            "activeNoteCols",
            "sortType",
            "sortBackwards",
            "noteSortType",
            "browserNoteSortBackwards",
            "browserTableShowNotesMode",
            "savedFilters",
            "flagLabels",
            "collapseSavedSearches",
            "collapseToday",
            "collapseFlags",
            "collapseCardState",
            "collapseDecks",
            "collapseNotetypes",
            "collapseTags",
            "previewBothSides",
            "setDueBrowser",
            "defaultSearchText",
            "ignoreAccentsInSearch",
            "restorePositionBrowser",
            "resetCountsBrowser",
            "normalize_note_text",
        ]
    );
    assert_eq!(Key::from_name("activeCols"), Some(Key::ActiveCols));
    assert_eq!(Key::from_name("activecols"), None, "names compare exactly");
    let section_keys: Vec<&str> = Section::ALL.iter().map(|s| s.key().name()).collect();
    assert_eq!(
        section_keys,
        [
            "collapseSavedSearches",
            "collapseToday",
            "collapseFlags",
            "collapseCardState",
            "collapseDecks",
            "collapseNotetypes",
            "collapseTags"
        ]
    );
    for (key, shape) in [
        (Key::ActiveCols, Shape::Columns),
        (Key::ActiveNoteCols, Shape::Columns),
        (Key::SortType, Shape::SortColumn),
        (Key::NoteSortType, Shape::SortColumn),
        (Key::SortBackwards, Shape::Boolean(false)),
        (Key::BrowserNoteSortBackwards, Shape::Boolean(false)),
        (Key::BrowserTableShowNotesMode, Shape::Boolean(false)),
        (Key::SavedFilters, Shape::Names),
        (Key::FlagLabels, Shape::Names),
        (Key::CollapseTags, Shape::Boolean(false)),
        (Key::PreviewBothSides, Shape::Boolean(false)),
        (Key::SetDueBrowser, Shape::Text("0")),
        (Key::DefaultSearchText, Shape::Text("")),
        (Key::IgnoreAccentsInSearch, Shape::Boolean(false)),
        (Key::RestorePositionBrowser, Shape::Boolean(true)),
        (Key::ResetCountsBrowser, Shape::Boolean(false)),
        (Key::NormalizeNoteText, Shape::Boolean(true)),
    ] {
        assert_eq!(key.shape(), shape, "{key}");
    }
    // K10.1: every key but normalize_note_text has a whole-value write.
    let unwritable: Vec<Key> = Key::ALL.into_iter().filter(|k| !k.writable()).collect();
    assert_eq!(unwritable, [Key::NormalizeNoteText]);
}

#[test]
fn whole_value_writes_take_values_of_the_keys_shape() {
    for (shape, accepted, refused) in [
        (
            Shape::Boolean(false),
            &["true", "false"][..],
            &["1", "\"true\"", "null"][..],
        ),
        (
            Shape::Text("0"),
            &["\"\"", "\"1-7!\""][..],
            &["5", "null", "[\"a\"]"][..],
        ),
        (
            Shape::SortColumn,
            &["\"noteFld\"", "\"myAddonColumn\""][..],
            &["5", "null"][..],
        ),
        (
            Shape::Columns,
            &["[]", "[\"question\",\"addonX\",\"\"]"][..],
            &["\"noteFld\"", "[\"noteFld\",5]", "{}"][..],
        ),
        (
            Shape::Names,
            &["{}", "{\"3\":\"Important\",\"1\":\"\"}"][..],
            &["[]", "{\"a\":3}", "\"x\""][..],
        ),
    ] {
        for text in accepted {
            assert!(shape.accepts(&value(text)), "{shape:?} takes {text}");
        }
        for text in refused {
            assert!(!shape.accepts(&value(text)), "{shape:?} refuses {text}");
        }
    }
}

/// K2.1's table, on `browserTableShowNotesMode` (default false) and on the
/// two keys whose default is true.
#[test]
fn boolean_keys_read_only_a_json_literal() {
    for (stored, reads) in [
        (Some("true"), Some(true)),
        (Some(" true "), Some(true)),
        (Some("false"), Some(false)),
        (Some("1"), None),
        (Some("0"), None),
        (Some("1.0"), None),
        (Some("\"true\""), None),
        (Some("\"false\""), None),
        (Some("null"), None),
        (Some("[true]"), None),
        (Some("{}"), None),
        // Not JSON, and a TEXT value: C3 reads both as unreadable.
        (Some("{"), None),
        (None, None),
    ] {
        for (key, default) in [
            (Key::BrowserTableShowNotesMode, false),
            (Key::RestorePositionBrowser, true),
            (Key::NormalizeNoteText, true),
        ] {
            assert_eq!(
                with(key, stored).boolean(key),
                Some(reads.unwrap_or(default)),
                "{key} stored as {stored:?}"
            );
        }
    }
    assert!(with(Key::RestorePositionBrowser, Some("0")).restore_position_browser());
    assert!(with(Key::NormalizeNoteText, Some("\"false\"")).normalize_note_text());
    assert!(with(Key::CollapseDecks, Some(" true ")).section_collapsed(Section::Decks));
    assert!(!with(Key::CollapseTags, Some("1.0")).section_collapsed(Section::Tags));
    assert!(with(Key::PreviewBothSides, Some("true")).preview_both_sides());
    assert!(with(Key::IgnoreAccentsInSearch, Some("true")).ignore_accents_in_search());
    assert!(!with(Key::IgnoreAccentsInSearch, Some("\"yes\"")).ignore_accents_in_search());
    assert!(with(Key::ResetCountsBrowser, Some("true")).reset_counts_browser());
    assert!(with(Key::BrowserTableShowNotesMode, Some("true")).notes_mode());
    assert_eq!(empty().boolean(Key::SetDueBrowser), None, "not a boolean key");
}

#[test]
fn string_keys_read_a_json_string_empty_included() {
    assert_eq!(with(Key::SetDueBrowser, Some("\"\"")).set_due_browser(), "");
    assert_eq!(with(Key::SetDueBrowser, Some("\"1-7!\"")).set_due_browser(), "1-7!");
    assert_eq!(with(Key::SetDueBrowser, Some("5")).set_due_browser(), "0");
    assert_eq!(with(Key::SetDueBrowser, Some("null")).set_due_browser(), "0");
    assert_eq!(with(Key::DefaultSearchText, Some("null")).default_search_text(), "");
    assert_eq!(
        with(Key::DefaultSearchText, Some("\"  deck:x \"")).default_search_text(),
        "  deck:x "
    );
    assert_eq!(empty().text(Key::DefaultSearchText), Some(String::new()));
    assert_eq!(empty().text(Key::ActiveCols), None, "not a text key");
}

/// K3's table.
#[test]
fn the_column_catalog_is_k3s() {
    let table = [
        ("noteFld", true, true, Some(Ascending)),
        ("question", false, false, None),
        ("answer", false, false, None),
        ("template", true, true, Some(Descending)),
        ("deck", true, true, Some(Ascending)),
        ("cardDue", true, true, Some(Descending)),
        ("cardEase", true, true, Some(Descending)),
        ("cardIvl", true, true, Some(Descending)),
        ("cardReps", true, true, Some(Descending)),
        ("cardLapses", true, true, Some(Descending)),
        ("noteCrt", true, true, Some(Descending)),
        ("cardMod", true, true, Some(Descending)),
        ("noteMod", true, true, Some(Descending)),
        ("note", true, true, Some(Ascending)),
        ("noteTags", true, true, Some(Ascending)),
        ("originalPosition", true, true, Some(Descending)),
        ("stability", true, false, Some(Descending)),
        ("difficulty", true, false, Some(Descending)),
        ("retrievability", true, false, Some(Descending)),
    ];
    assert_eq!(Column::ALL.len(), table.len());
    for (column, (key, cards, notes, first)) in Column::ALL.into_iter().zip(table) {
        assert_eq!(column.key(), key);
        assert_eq!(Column::from_key(key), Some(column));
        assert_eq!(column.sortable(Cards), cards, "{key} in cards mode");
        assert_eq!(column.sortable(Notes), notes, "{key} in notes mode");
        assert_eq!(column.first_direction(), first, "{key}");
    }
    for unknown in ["NoteFld", "", "addonX", "noteFld "] {
        assert_eq!(Column::from_key(unknown), None, "{unknown:?}");
    }
}

/// One K4.4 action.
enum Action {
    Show(&'static str),
    Hide(&'static str),
    Reorder(&'static [&'static str]),
}

/// One K4.4 row: stored, displayed, action, written (`None`: nothing).
type ColumnRow = (
    Option<&'static str>,
    &'static [&'static str],
    Action,
    Result<Option<&'static str>, Refusal>,
);

/// K4.4's table, on `activeCols`.
#[test]
fn column_writes_follow_k4_4() {
    let default: &[&str] = &["noteFld", "template", "cardDue", "deck"];
    let rows: [ColumnRow; 12] = [
        (
            None,
            default,
            Action::Show("noteCrt"),
            Ok(Some(r#"["noteFld","template","cardDue","deck","noteCrt"]"#)),
        ),
        (
            None,
            default,
            Action::Hide("template"),
            Ok(Some(r#"["noteFld","cardDue","deck"]"#)),
        ),
        (
            Some(r#"["noteFld","addonX","deck"]"#),
            &["noteFld", "deck"],
            Action::Show("cardDue"),
            Ok(Some(r#"["noteFld","addonX","deck","cardDue"]"#)),
        ),
        (
            Some(r#"["noteFld","addonX","deck"]"#),
            &["noteFld", "deck"],
            Action::Hide("noteFld"),
            Ok(Some(r#"["addonX","deck"]"#)),
        ),
        (
            Some(r#"["noteFld","addonX","deck","cardDue"]"#),
            &["noteFld", "deck", "cardDue"],
            Action::Reorder(&["cardDue", "noteFld", "deck"]),
            Ok(Some(r#"["cardDue","addonX","noteFld","deck"]"#)),
        ),
        (
            Some(r#"["deck","deck","addonX","deck"]"#),
            &["deck"],
            Action::Show("noteFld"),
            Ok(Some(r#"["deck","addonX","noteFld"]"#)),
        ),
        (
            Some(r#"["deck"]"#),
            &["deck"],
            Action::Hide("deck"),
            Err(Refusal::LastColumn),
        ),
        (
            Some(r#"["addonX"]"#),
            default,
            Action::Hide("cardDue"),
            Ok(Some(r#"["addonX","noteFld","template","deck"]"#)),
        ),
        (
            Some("[]"),
            default,
            Action::Show("noteTags"),
            Ok(Some(r#"["noteFld","template","cardDue","deck","noteTags"]"#)),
        ),
        (
            Some(r#"["noteFld",5,"addonX"]"#),
            &["noteFld"],
            Action::Show("deck"),
            Ok(Some(r#"["noteFld",5,"addonX","deck"]"#)),
        ),
        (Some(r#""noteFld""#), default, Action::Show("deck"), Ok(None)),
        (
            Some(r#""noteFld""#),
            default,
            Action::Show("note"),
            Ok(Some(r#"["noteFld","template","cardDue","deck","note"]"#)),
        ),
    ];
    for (stored, displayed, action, written) in rows {
        let active = columns(Cards, stored);
        assert_eq!(keys(&active.displayed()), displayed, "{stored:?}");
        let got = match action {
            Action::Show(key) => Ok(active.show(col(key))),
            Action::Hide(key) => active.hide(col(key)),
            Action::Reorder(order) => {
                let order: Vec<Column> = order.iter().map(|k| col(k)).collect();
                active.reorder(&order).map(Some)
            }
        };
        assert_eq!(got.map(compact), written.map(|w| w.map(str::to_owned)), "{stored:?}");
    }
}

#[test]
fn notes_mode_columns_have_their_own_default() {
    let active = columns(Notes, None);
    assert_eq!(keys(&active.displayed()), ["noteFld", "note", "template", "noteTags"]);
    assert_eq!(
        compact(active.show(Column::Stability)).as_deref(),
        Some(r#"["noteFld","note","template","noteTags","stability"]"#)
    );
}

#[test]
fn hiding_a_column_that_is_not_displayed_writes_nothing() {
    let active = columns(Cards, Some(r#"["noteFld","addonX","deck"]"#));
    assert_eq!(active.hide(Column::Due), Ok(None));
}

#[test]
fn a_reorder_must_name_every_displayed_column_once() {
    let active = columns(Cards, Some(r#"["noteFld","addonX","deck","cardDue"]"#));
    for order in [
        &["noteFld", "deck"][..],
        &["noteFld", "deck", "cardDue", "cardDue"][..],
        &["noteFld", "deck", "deck"][..],
        &["noteFld", "deck", "noteCrt"][..],
        &["noteFld", "deck", "cardDue", "noteCrt"][..],
        &[][..],
    ] {
        let order: Vec<Column> = order.iter().map(|k| col(k)).collect();
        assert_eq!(active.reorder(&order), Err(Refusal::InvalidOrder), "{order:?}");
    }
    // The same order gives the list without its repeated known columns,
    // unknown entries in place.
    let repeated = columns(Cards, Some(r#"["deck","addonX","deck",null,"noteFld"]"#));
    assert_eq!(
        repeated
            .reorder(&[Column::Deck, Column::SortField])
            .map(|v| v.compact()),
        Ok(r#"["deck","addonX",null,"noteFld"]"#.to_owned())
    );
    assert_eq!(
        repeated
            .reorder(&[Column::SortField, Column::Deck])
            .map(|v| v.compact()),
        Ok(r#"["noteFld","addonX",null,"deck"]"#.to_owned())
    );
}

/// K5.1's table.
#[test]
fn sort_reads_fall_back_to_created() {
    let sort = |column, direction| Sort { column, direction };
    for (mode, stored_column, stored_backwards, expected) in [
        (
            Cards,
            Some(r#""noteFld""#),
            Some("false"),
            sort(Column::SortField, Ascending),
        ),
        (Cards, Some(r#""cardDue""#), Some("true"), sort(Column::Due, Descending)),
        (Cards, None, None, sort(Column::Created, Ascending)),
        (
            Cards,
            Some(r#""question""#),
            Some("true"),
            sort(Column::Created, Descending),
        ),
        (
            Cards,
            Some(r#""stability""#),
            Some("true"),
            sort(Column::Stability, Descending),
        ),
        (Notes, Some(r#""stability""#), None, sort(Column::Created, Ascending)),
        (
            Notes,
            Some(r#""myAddonColumn""#),
            Some("false"),
            sort(Column::Created, Ascending),
        ),
        (Cards, Some("5"), Some("1"), sort(Column::Created, Ascending)),
    ] {
        let read = Sort::from_stored(
            mode,
            stored_column.and_then(json).as_ref(),
            stored_backwards.and_then(json).as_ref(),
        );
        assert_eq!(read, expected, "{mode:?} {stored_column:?} {stored_backwards:?}");
    }
    // Through the accessor, on each mode's own keys.
    let config = BrowserConfig::from_stored(|key| match key {
        Key::SortType => json(r#""question""#),
        Key::NoteSortType => json(r#""noteMod""#),
        Key::BrowserNoteSortBackwards => json("true"),
        _ => None,
    });
    assert_eq!(config.sort(Cards), sort(Column::Created, Ascending));
    assert_eq!(config.sort(Notes), sort(Column::NoteModified, Descending));
}

/// K5.2's table: the sort each action leaves; the keys written are those
/// whose value changes (C4, checked in meguri-engine's tests).
#[test]
fn sort_writes_follow_k5_2() {
    let stored = |mode, column: Option<&str>, backwards: Option<&str>| {
        Sort::from_stored(mode, column.and_then(json).as_ref(), backwards.and_then(json).as_ref())
    };
    let sort = |column, direction| Sort { column, direction };
    assert_eq!(
        stored(Cards, Some(r#""noteFld""#), Some("false")).clicked(Cards, Column::SortField),
        Ok(sort(Column::SortField, Descending))
    );
    assert_eq!(
        stored(Cards, Some(r#""noteFld""#), Some("false")).clicked(Cards, Column::Due),
        Ok(sort(Column::Due, Descending))
    );
    assert_eq!(
        stored(Cards, Some(r#""noteFld""#), Some("true")).clicked(Cards, Column::Deck),
        Ok(sort(Column::Deck, Ascending))
    );
    assert_eq!(
        stored(Cards, Some(r#""question""#), Some("false")).clicked(Cards, Column::Created),
        Ok(sort(Column::Created, Descending)),
        "Created is the current column through the fallback"
    );
    assert_eq!(
        stored(Notes, None, None).clicked(Notes, Column::NoteModified),
        Ok(sort(Column::NoteModified, Descending))
    );
    assert_eq!(
        Sort::chosen(Notes, Column::Created, Ascending),
        Ok(sort(Column::Created, Ascending))
    );
    for mode in [Cards, Notes] {
        let current = stored(mode, None, None);
        assert_eq!(current.clicked(mode, Column::Question), Err(Refusal::UnsortableColumn));
        assert_eq!(
            Sort::chosen(mode, Column::Answer, Descending),
            Err(Refusal::UnsortableColumn)
        );
    }
    assert_eq!(
        stored(Notes, None, None).clicked(Notes, Column::Stability),
        Err(Refusal::UnsortableColumn)
    );
    assert_eq!(
        stored(Cards, None, None).clicked(Cards, Column::Stability),
        Ok(sort(Column::Stability, Descending))
    );
}

/// K6.4's table and its trimmed characters.
#[test]
fn a_default_search_is_set_when_text_remains_after_trimming() {
    for (stored, opens_with) in [
        (None, None),
        (Some(r#""""#), None),
        (Some(r#""   ""#), None),
        (Some("\"\u{3000}\""), None),
        (Some(r#""  deck:x ""#), Some("  deck:x ")),
        (Some("\"\u{200b}\""), Some("\u{200b}")),
    ] {
        assert_eq!(
            with(Key::DefaultSearchText, stored).default_search().as_deref(),
            opens_with,
            "{stored:?}"
        );
    }
    let trimmed: String = [
        '\u{9}', '\u{a}', '\u{b}', '\u{c}', '\u{d}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}', ' ', '\u{85}', '\u{a0}',
        '\u{1680}', '\u{2000}', '\u{2005}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
    ]
    .into_iter()
    .collect();
    assert!(!default_search_is_set(&trimmed));
    assert!(default_search_is_set(&format!("{trimmed}x{trimmed}")));
    for kept in ['\u{8}', '\u{e}', '\u{1b}', '\u{200b}', '\u{feff}', '\u{180e}'] {
        assert!(default_search_is_set(&kept.to_string()), "{:04x}", u32::from(kept));
    }
}

#[test]
fn saved_searches_are_the_string_members_of_an_object() {
    let saved = SavedSearches::from_stored(
        json(r#"{"b":"x","Leeches":"tag:leech","n":3,"leeches":"is:due","a":"y","a":"z"}"#).as_ref(),
    );
    assert_eq!(
        saved.list(),
        [("Leeches", "tag:leech"), ("a", "z"), ("b", "x"), ("leeches", "is:due")]
    );
    assert_eq!(saved.search("Leeches"), Some("tag:leech"));
    assert_eq!(saved.search("leeches"), Some("is:due"), "names compare exactly");
    assert_eq!(saved.search("n"), None, "a member of another type is no saved search");
    for stored in [None, Some("[1,2]"), Some(r#""x""#), Some("{")] {
        assert!(
            SavedSearches::from_stored(stored.and_then(json).as_ref())
                .list()
                .is_empty(),
            "{stored:?}"
        );
    }
    assert!(empty().saved_searches().list().is_empty());
}

/// One K7.2 write.
enum SavedWrite {
    Save(&'static str, &'static str),
    Update(&'static str, &'static str),
    Rename(&'static str, &'static str),
    Delete(&'static [&'static str]),
}

/// K7.2's table, then Update and the refusal.
#[test]
fn saved_search_writes_follow_k7_2() {
    let rows: [(Option<&str>, SavedWrite, Option<&str>); 14] = [
        (
            None,
            SavedWrite::Save("Leeches", "tag:leech"),
            Some(r#"{"Leeches":"tag:leech"}"#),
        ),
        (
            Some(r#"{"Leeches":"tag:leech"}"#),
            SavedWrite::Save("Leeches", "tag:leech"),
            None,
        ),
        (
            Some(r#"{"Leeches":"tag:leech"}"#),
            SavedWrite::Save("Leeches", "tag:leech is:suspended"),
            Some(r#"{"Leeches":"tag:leech is:suspended"}"#),
        ),
        (
            Some(r#"{"b":"x","a":"y"}"#),
            SavedWrite::Save("c", "is:new"),
            Some(r#"{"a":"y","b":"x","c":"is:new"}"#),
        ),
        (
            Some(r#"{"Due":"is:due","n":3}"#),
            SavedWrite::Rename("Due", r#""Due" now"#),
            Some(r#"{"Due now":"is:due","n":3}"#),
        ),
        (
            Some(r#"{"A":"is:new","B":"is:due"}"#),
            SavedWrite::Rename("A", "B"),
            Some(r#"{"B":"is:new"}"#),
        ),
        (Some(r#"{"A":"is:new"}"#), SavedWrite::Rename("A", r#""""#), None),
        (
            Some(r#"{"A":"is:new","B":"is:due","C":"flag:1"}"#),
            SavedWrite::Delete(&["A", "C", "Z"]),
            Some(r#"{"B":"is:due"}"#),
        ),
        (Some(r#"{"A":"is:new"}"#), SavedWrite::Delete(&["A"]), Some("{}")),
        (
            Some("[1,2]"),
            SavedWrite::Save("A", "is:new"),
            Some(r#"{"A":"is:new"}"#),
        ),
        // Rename: an equal name and a missing old name write nothing.
        (Some(r#"{"A":"is:new"}"#), SavedWrite::Rename("A", "A"), None),
        (Some(r#"{"A":"is:new","n":3}"#), SavedWrite::Rename("n", "m"), None),
        // Delete: names that are no member write nothing.
        (Some(r#"{"A":"is:new"}"#), SavedWrite::Delete(&["a", "Z"]), None),
        // Update: a missing name is added.
        (
            Some(r#"{"A":"is:new"}"#),
            SavedWrite::Update("B", "is:due"),
            Some(r#"{"A":"is:new","B":"is:due"}"#),
        ),
    ];
    for (stored, write, written) in rows {
        let saved = SavedSearches::from_stored(stored.and_then(json).as_ref());
        let got = match write {
            SavedWrite::Save(name, search) => saved.save(name, search).unwrap(),
            SavedWrite::Update(name, search) => saved.update(name, search),
            SavedWrite::Rename(old, new) => saved.rename(old, new),
            SavedWrite::Delete(names) => saved.delete(names),
        };
        assert_eq!(compact(got).as_deref(), written, "{stored:?}");
    }
    let saved = SavedSearches::from_stored(json(r#"{"A":"is:new"}"#).as_ref());
    assert_eq!(saved.update("A", "is:new"), None);
    assert_eq!(
        compact(saved.update("A", "is:due")).as_deref(),
        Some(r#"{"A":"is:due"}"#)
    );
    assert_eq!(saved.save("", "is:due"), Err(Refusal::EmptyName));
    assert_eq!(
        compact(saved.save(" ", "is:due").unwrap()).as_deref(),
        Some(r#"{" ":"is:due","A":"is:new"}"#),
        "names are stored as given"
    );
}

#[test]
fn refusals_have_the_specs_kinds() {
    assert_eq!(Refusal::LastColumn.kind(), "last_column");
    assert_eq!(Refusal::InvalidOrder.kind(), "invalid_order");
    assert_eq!(Refusal::UnsortableColumn.kind(), "unsortable_column");
    assert_eq!(Refusal::EmptyName.kind(), "empty_name");
}

const BROWSER_KEYS: &str = "'activeCols', 'activeNoteCols', 'sortType', 'noteSortType', 'sortBackwards', \
     'browserNoteSortBackwards', 'browserTableShowNotesMode', 'savedFilters', 'flagLabels', \
     'collapseSavedSearches', 'collapseToday', 'collapseFlags', 'collapseCardState', 'collapseDecks', \
     'collapseNotetypes', 'collapseTags', 'previewBothSides', 'setDueBrowser', 'defaultSearchText', \
     'ignoreAccentsInSearch', 'restorePositionBrowser', 'resetCountsBrowser', 'normalize_note_text'";

/// K2, R1138: on a collection holding none of the keys, every accessor
/// gives its default, and reading leaves every config row and `col.mod` as
/// they were, through the writer as through a reader.
#[test]
fn reading_every_key_on_an_empty_config_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    let clock = StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC");
    let random = SeededRandom::new(1);
    let opened = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let writer = opened.writer();
    writer
        .execute_raw(&format!("DELETE FROM config WHERE KEY IN ({BROWSER_KEYS})"), &[])
        .unwrap();
    let state = || {
        let config = writer
            .execute_raw(
                "SELECT KEY, usn, mtime_secs, val, typeof(val) FROM config ORDER BY KEY",
                &[],
            )
            .unwrap()
            .rows;
        let col = writer.execute_raw("SELECT mod FROM col", &[]).unwrap().rows;
        (config, col)
    };
    let before = state();
    assert!(!before.0.is_empty());
    assert!(before.0.iter().all(|row| match &row[0] {
        SqlValue::Text(key) => Key::from_name(key).is_none(),
        other => panic!("key {other:?}"),
    }));

    let reader = opened.open_reader(ReaderRole::Query).unwrap();
    for config in [
        BrowserConfig::read(writer).unwrap(),
        BrowserConfig::read(&reader).unwrap(),
    ] {
        for key in Key::ALL {
            assert_eq!(config.stored(key), None, "{key}");
        }
        assert_eq!(config, empty());
        for mode in [Cards, Notes] {
            assert_eq!(config.columns(mode).displayed(), empty().columns(mode).displayed());
            assert_eq!(config.sort(mode).column, Column::Created);
        }
        assert!(config.restore_position_browser() && config.normalize_note_text());
        assert_eq!(config.set_due_browser(), "0");
        assert_eq!(config.default_search(), None);
        assert!(config.saved_searches().list().is_empty());
    }
    assert_eq!(state(), before, "reading wrote nothing");
}
