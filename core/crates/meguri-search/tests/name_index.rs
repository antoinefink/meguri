//! The name index's values (`specs/search/editing-services.md` D6.7): D6.5
//! step 3's order for every list, subdecks, tag and note type updates, the
//! two config keys, and which parts a change set touches. Building it from a
//! collection and keeping it current is meguri-engine's
//! `tests/name_index.rs`.

use meguri_model::changes::{
    ChangeKind, ChangeSet, ConfigKeyChange, DeckChange, NotetypeChange, OpSource, TagChange,
};
use meguri_model::ids::{DeckConfigId, DeckId, NotetypeId};
use meguri_search::name_index::{DeckRow, NameIndex, NotetypeNames, PresetName, Sections};
use meguri_storage::ConfigValue;

fn deck(id: i64, native: &str) -> DeckRow {
    DeckRow {
        id: DeckId(id),
        native: native.to_owned(),
        filtered: id == 9,
    }
}

fn names(index: &NameIndex) -> Vec<(&str, bool, bool)> {
    index
        .decks()
        .iter()
        .map(|d| (d.name.as_str(), d.has_subdecks, d.filtered))
        .collect()
}

#[test]
fn decks_are_in_stored_name_order_with_their_subdecks_marked() {
    let mut index = NameIndex::default();
    index.set_decks(vec![
        deck(1, "a-b"),
        deck(2, "a b"),
        deck(3, "a\u{1f}b"),
        deck(4, "A"),
        deck(5, "Lessons"),
        deck(6, "japanese\u{1f}Lesson1"),
        deck(7, "Japanese"),
        deck(9, "Filtered"),
    ]);
    // `a::b` before `a b` and `a-b` (D7): U+001F sorts before both.
    assert_eq!(
        names(&index),
        vec![
            ("A", true, false),
            ("a::b", false, false),
            ("a b", false, false),
            ("a-b", false, false),
            ("Filtered", false, true),
            ("Japanese", true, false),
            ("japanese::Lesson1", false, false),
            ("Lessons", false, false),
        ]
    );
    assert_eq!(index.decks()[1].native, "a\u{1f}b");
    assert_eq!(index.decks()[1].id, DeckId(3));
}

#[test]
fn tags_are_kept_in_unicase_order() {
    let mut index = NameIndex::default();
    index.set_tags(vec!["b".into(), "A::x".into(), "a".into(), "Ä".into(), "c".into()]);
    assert_eq!(index.tags(), ["a", "A::x", "b", "c", "Ä"]);
    index.put_tag("B2");
    index.put_tag("A");
    index.remove_tag("C");
    index.remove_tag("missing");
    assert_eq!(index.tags(), ["A", "A::x", "b", "B2", "Ä"]);
    // A shared index keeps its tags when another copy changes.
    let before = index.clone();
    index.put_tag("z");
    assert_eq!(before.tags().len(), 5);
    assert_eq!(index.tags().len(), 6);
}

#[test]
fn note_types_presets_saved_searches_and_flags() {
    let notetype = |id: i64, name: &str| NotetypeNames {
        id: NotetypeId(id),
        name: name.to_owned(),
        templates: vec!["Card 1".into()],
        fields: vec!["Front".into(), "Back".into()],
    };
    let mut index = NameIndex::default();
    index.set_notetypes(vec![notetype(3, "cloze"), notetype(1, "Basic"), notetype(2, "basic")]);
    let order = |index: &NameIndex| index.notetypes().iter().map(|n| n.id.0).collect::<Vec<_>>();
    assert_eq!(order(&index), [1, 2, 3]);
    index.put_notetype(notetype(1, "Zed"));
    index.put_notetype(notetype(4, "Alpha"));
    index.remove_notetype(NotetypeId(3));
    assert_eq!(order(&index), [4, 2, 1]);
    assert_eq!(index.notetypes()[2].fields, ["Front", "Back"]);

    index.set_presets(vec![
        PresetName {
            id: DeckConfigId(2),
            name: "evening".into(),
        },
        PresetName {
            id: DeckConfigId(1),
            name: "Default".into(),
        },
    ]);
    assert_eq!(index.presets()[0].name, "Default");

    let saved = ConfigValue::parse(br#"{"b":"x","A":"is:due","n":3,"a":"tag:leech"}"#).unwrap();
    index.set_saved_searches(Some(&saved));
    let saved: Vec<(&str, &str)> = index
        .saved_searches()
        .iter()
        .map(|s| (s.name.as_str(), s.text.as_str()))
        .collect();
    assert_eq!(saved, [("A", "is:due"), ("a", "tag:leech"), ("b", "x")]);
    index.set_saved_searches(None);
    assert!(index.saved_searches().is_empty());

    let names = std::array::from_fn(|k| format!("flag {}", k + 1));
    index.set_flag_names(names);
    assert_eq!(index.flag_names()[6], "flag 7");
}

#[test]
fn a_change_set_touches_only_its_parts() {
    let mut changes = ChangeSet::empty(OpSource::UserUndoable);
    assert!(!Sections::of(&changes).any());
    changes.config_keys.push(ConfigKeyChange {
        key: "curDeck".into(),
        kind: ChangeKind::Updated,
    });
    assert!(!Sections::of(&changes).any());
    changes.config_keys.push(ConfigKeyChange {
        key: "savedFilters".into(),
        kind: ChangeKind::Updated,
    });
    changes.decks.push(DeckChange {
        id: DeckId(1),
        kind: ChangeKind::Added,
    });
    assert_eq!(
        Sections::of(&changes),
        Sections {
            decks: true,
            saved_searches: true,
            ..Sections::default()
        }
    );
    changes.tags.push(TagChange {
        name: "x".into(),
        kind: ChangeKind::Removed,
    });
    changes.notetypes.push(NotetypeChange {
        id: NotetypeId(1),
        kind: ChangeKind::Updated,
        schema_changed: false,
    });
    changes.config_keys.push(ConfigKeyChange {
        key: "flagLabels".into(),
        kind: ChangeKind::Removed,
    });
    let sections = Sections::of(&changes);
    assert!(sections.tags && sections.notetypes && sections.flag_names && !sections.presets);
    let mut whole = ChangeSet::empty(OpSource::Maintenance);
    whole.whole_collection = true;
    assert_eq!(Sections::of(&whole), Sections::ALL);
}
