//! Browse's suggestions (`specs/search/filter-tokens.md` T12.1 to T12.3;
//! T16's suggestion rows): the empty field, a word at the caret with its
//! classes, sections, limits and recent rows, a kind's value list, values
//! already in a token, Field and Card Type rows with their note types, and
//! Anki syntax at the caret handed to `editing-services` D6. Counts are
//! meguri-engine's `tests/filter_counts.rs`.

use meguri_model::ids::{DeckId, NotetypeId};
use meguri_search::filter_tokens::suggest::{BuiltIn, Labels, Row, Section, SuggestRequest, SuggestionRow, suggest};
use meguri_search::filter_tokens::{DeckValue, DueValue, FilterQuery, Period, StateValue, Token, Value, ValueToken};
use meguri_search::name_index::{DeckRow, NameIndex, NotetypeNames};
use meguri_search::suggest::CandidateKind;
use meguri_search::tail::InvalidOffset;
use meguri_storage::ConfigValue;

const FLAGS: [&str; 7] = ["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"];

fn index(decks: &[(i64, &str)], tags: &[&str], notetypes: &[(i64, &str, &[&str], &[&str])], saved: &str) -> NameIndex {
    let mut names = NameIndex::default();
    names.set_decks(
        decks
            .iter()
            .map(|(id, name)| DeckRow {
                id: DeckId(*id),
                native: name.replace("::", "\u{1f}"),
                filtered: false,
            })
            .collect(),
    );
    names.set_tags(tags.iter().map(|t| (*t).to_owned()).collect());
    names.set_notetypes(
        notetypes
            .iter()
            .map(|(id, name, fields, templates)| NotetypeNames {
                id: NotetypeId(*id),
                name: (*name).to_owned(),
                fields: fields.iter().map(|f| (*f).to_owned()).collect(),
                templates: templates.iter().map(|t| (*t).to_owned()).collect(),
            })
            .collect(),
    );
    names.set_saved_searches(ConfigValue::parse(saved.as_bytes()).as_ref());
    names.set_flag_names(FLAGS.map(str::to_owned));
    names
}

/// T16's collection, its names made neutral: a deck Pelmo and its subdeck,
/// tags plin-1 and plin-2, a field Plin in two note types.
fn pelmo() -> NameIndex {
    index(
        &[(1, "Default"), (2, "Pelmo"), (3, "Pelmo::Ostrel"), (4, "Gravik::Plindo")],
        &["plin-1", "plin-2", "chalvo"],
        &[
            (10, "Basic", &["Front", "Back"], &["Card 1"]),
            (11, "Lab", &["Plin", "Back"], &["Card 1", "Prandel"]),
            (12, "Zubo", &["plin"], &["Prandel"]),
        ],
        r#"{"Phalmor":"tag:phal","Wexo":"prop:lapses>3"}"#,
    )
}

fn ask(names: &NameIndex, query: &FilterQuery, caret: usize, scope: Option<Section>, recent: &[String]) -> Vec<SuggestionRow> {
    let request = SuggestRequest {
        query,
        caret,
        scope,
        recent,
    };
    suggest(names, &Labels::default(), &request).unwrap()
}

fn text(t: &str) -> FilterQuery {
    FilterQuery::new(vec![], t)
}

fn at_end(names: &NameIndex, t: &str) -> Vec<SuggestionRow> {
    ask(names, &text(t), t.chars().count(), None, &[])
}

/// A row as a short text: its section and what it names.
fn show(row: &SuggestionRow) -> String {
    let section = row.section.map(|s| format!("{s:?}")).unwrap_or_default();
    let what = match &row.row {
        Row::Contains(w) => format!("contains {w}"),
        Row::BuiltIn(b) => format!("{b:?}"),
        Row::SavedSearch { name, .. } => format!("saved {name}"),
        Row::FilterBy(kind) => format!("{kind:?}"),
        Row::Value(value) => value_name(value),
        Row::Field { name, .. } => format!("field {name}"),
        Row::CardType { value, .. } => value_name(value),
        Row::ShowAll(section) => format!("show all {section:?}"),
        Row::SaveSearch => "save search".to_owned(),
        Row::Syntax(candidate) => format!("{:?} {}", candidate.kind, candidate.label),
    };
    if section.is_empty() { what } else { format!("{section}: {what}") }
}

fn value_name(value: &Value) -> String {
    match value {
        Value::Deck(deck) => format!("deck {}", deck.name()),
        Value::Tag(name) => format!("tag {name}"),
        Value::Notetype(n) => format!("notetype {}", n.name),
        Value::CardType(name) => format!("card type {name}"),
        other => format!("{other:?}"),
    }
}

fn shown(rows: &[SuggestionRow]) -> Vec<String> {
    rows.iter().map(show).collect()
}

// T12.1's first row: the built-ins, the user's saved searches, then Filter
// By's kinds; no Save Search for the empty query.
#[test]
fn the_empty_field_lists_saved_searches_then_the_kinds() {
    let names = pelmo();
    let rows = at_end(&names, "");
    assert_eq!(
        shown(&rows),
        [
            "SavedSearches: DueToday",
            "SavedSearches: AddedInLast7Days",
            "SavedSearches: Leeches",
            "SavedSearches: saved Phalmor",
            "SavedSearches: saved Wexo",
            "FilterBy: Decks",
            "FilterBy: Tags",
            "FilterBy: Notetypes",
            "FilterBy: CardTypes",
            "FilterBy: CardState",
            "FilterBy: Flag",
            "FilterBy: Due",
            "FilterBy: Added",
            "FilterBy: Edited",
            "FilterBy: Studied",
            "FilterBy: Fields",
        ]
    );
    assert_eq!(BuiltIn::Leeches.value(), Value::State(StateValue::Leech));
    assert_eq!(BuiltIn::DueToday.value(), Value::Due(DueValue::Today));
    assert_eq!(BuiltIn::AddedInLast7Days.value(), Value::Added(Period::Last7Days));
    // Empty text after a token: the same list, then Save Search.
    let query = FilterQuery::new(vec![Token::Value(ValueToken::new(vec![Value::NoFlag], false))], "");
    let rows = ask(&names, &query, 0, None, &[]);
    assert_eq!(show(rows.first().unwrap()), "SavedSearches: DueToday");
    assert_eq!(show(rows.last().unwrap()), "save search");
    // The caret right after White_Space: the word is empty too.
    let rows = at_end(&names, "pelmo ");
    assert_eq!(show(&rows[0]), "SavedSearches: DueToday");
}

// T16: `ch` starts the tag chalvo, and lies inside the built-in Leeches
// and the label Leech (the inner class, Saved Searches first); nothing
// else holds it.
#[test]
fn a_word_lists_contains_then_its_matches() {
    let names = pelmo();
    assert_eq!(
        shown(&at_end(&names, "ch")),
        [
            "contains ch",
            "Tags: tag chalvo",
            "SavedSearches: Leeches",
            "CardState: State(Leech)",
            "save search"
        ]
    );
}

// T12.3: the prefix class before the inner class, by section within a
// class, sections shown in the order of their first row.
#[test]
fn prefix_matches_come_first_and_sections_follow_their_first_row() {
    let names = pelmo();
    assert_eq!(
        shown(&at_end(&names, "p")),
        [
            "contains p",
            "SavedSearches: saved Phalmor",
            // Pink and Purple start with p.
            "Flag: Flag(5)",
            "Flag: Flag(7)",
            // Pelmo and its subdeck by their first component, then
            // Gravik::Plindo, an inner match (a component skipped).
            "Decks: deck Pelmo",
            "Decks: deck Pelmo::Ostrel",
            "Decks: deck Gravik::Plindo",
            "Tags: tag plin-1",
            "Tags: tag plin-2",
            "CardTypes: card type Prandel",
            "Fields: field Plin",
            // Only inner matches: its section comes after the others.
            "CardState: State(Suspended)",
            "save search",
        ]
    );
}

// T12.1: a value already in an `is` token is not suggested again; one in
// an `is not` token is.
#[test]
fn values_in_an_is_token_are_left_out() {
    let names = pelmo();
    let pelmo = Value::Deck(DeckValue::of(&names, DeckId(2)).unwrap());
    let is = FilterQuery::new(vec![Token::Value(ValueToken::new(vec![pelmo.clone()], false))], "pel");
    let rows = shown(&ask(&names, &is, 3, None, &[]));
    assert!(!rows.contains(&"Decks: deck Pelmo".to_owned()), "{rows:?}");
    assert!(rows.contains(&"Decks: deck Pelmo::Ostrel".to_owned()), "{rows:?}");
    let not = FilterQuery::new(vec![Token::Value(ValueToken::new(vec![pelmo], true))], "pel");
    let rows = shown(&ask(&names, &not, 3, None, &[]));
    assert!(rows.contains(&"Decks: deck Pelmo".to_owned()), "{rows:?}");
    // A fixed value: Suspended in Card State's is token.
    let suspended = Token::Value(ValueToken::new(vec![Value::State(StateValue::Suspended)], false));
    let rows = shown(&ask(&names, &FilterQuery::new(vec![suspended], "sus"), 3, None, &[]));
    assert_eq!(rows, ["contains sus", "save search"]);
}

// T12.3 step 3: at most 5 rows a section, then Show All; 50 rows in all.
#[test]
fn sections_are_cut_at_five_rows_and_the_list_at_fifty() {
    let tags: Vec<String> = (0..10_000).map(|k| format!("topic{k:05}")).collect();
    let decks: Vec<(i64, String)> = (0..20).map(|k| (100 + k, format!("Toprel {k:02}"))).collect();
    let deck_refs: Vec<(i64, &str)> = decks.iter().map(|(id, n)| (*id, n.as_str())).collect();
    let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
    let names = index(&deck_refs, &tag_refs, &[], "{}");
    let rows = at_end(&names, "top");
    assert_eq!(
        shown(&rows),
        [
            "contains top",
            "Decks: deck Toprel 00",
            "Decks: deck Toprel 01",
            "Decks: deck Toprel 02",
            "Decks: deck Toprel 03",
            "Decks: deck Toprel 04",
            "Decks: show all Decks",
            "Tags: tag topic00000",
            "Tags: tag topic00001",
            "Tags: tag topic00002",
            "Tags: tag topic00003",
            "Tags: tag topic00004",
            "Tags: show all Tags",
            "save search",
        ]
    );
    // Many sections: never more than 50 rows besides Show All and Save Search.
    let many: Vec<(i64, &str, &[&str], &[&str])> = Vec::new();
    let names = index(&deck_refs, &tag_refs, &many, "{}");
    let counted = at_end(&names, "o")
        .iter()
        .filter(|row| !matches!(row.row, Row::ShowAll(_) | Row::SaveSearch))
        .count();
    assert!(counted <= 50, "{counted}");
    // A kind's value list holds every value.
    let all = ask(&names, &text(""), 0, Some(Section::Tags), &[]);
    assert_eq!(all.len(), 10_001);
    assert_eq!(show(&all[0]), "Tags: Untagged");
}

// T12.3 step 2: recently accepted rows first within their class and
// section, newest first; never across classes.
#[test]
fn recently_accepted_rows_come_first_in_their_class() {
    let names = pelmo();
    let rows = at_end(&names, "plin");
    let key = |name: &str| {
        rows.iter()
            .find(|row| show(row) == name)
            .and_then(SuggestionRow::key)
            .unwrap_or_else(|| panic!("{name}"))
    };
    let recent = vec![key("Tags: tag plin-2"), key("Fields: field Plin")];
    let ranked = ask(&names, &text("plin"), 4, None, &recent);
    let tags: Vec<String> = shown(&ranked).into_iter().filter(|s| s.starts_with("Tags")).collect();
    assert_eq!(tags, ["Tags: tag plin-2", "Tags: tag plin-1"]);
    // Plindo is the second component: an inner match, after the prefix
    // class whatever the recent rows say.
    let all = shown(&ranked);
    let plindo = all.iter().position(|s| s == "Decks: deck Gravik::Plindo").unwrap();
    let field = all.iter().position(|s| s == "Fields: field Plin").unwrap();
    assert!(field < plindo, "{all:?}");
}

// T12.1: a field is listed once with every note type that has it (letter
// case folded, the lowest id's spelling); a card type likewise.
#[test]
fn fields_and_card_types_carry_their_note_types() {
    let names = pelmo();
    let rows = at_end(&names, "plin");
    let field = rows
        .iter()
        .find_map(|row| match &row.row {
            Row::Field { name, notetypes } => Some((name.clone(), notetypes.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(field, ("Plin".to_owned(), vec![NotetypeId(11), NotetypeId(12)]));
    let rows = at_end(&names, "pran");
    let card_type = rows
        .iter()
        .find_map(|row| match &row.row {
            Row::CardType { value, notetypes } => Some((value.clone(), notetypes.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        card_type,
        (Value::CardType("Prandel".to_owned()), vec![NotetypeId(11), NotetypeId(12)])
    );
}

// T12.1's third row: fixed values in T3's order, then the names; a word
// narrows it.
#[test]
fn a_kinds_value_list_is_its_fixed_values_then_its_names() {
    let names = pelmo();
    assert_eq!(
        shown(&ask(&names, &text(""), 0, Some(Section::Decks), &[])),
        [
            "Decks: CurrentDeck",
            "Decks: deck Default",
            "Decks: deck Gravik::Plindo",
            "Decks: deck Pelmo",
            "Decks: deck Pelmo::Ostrel",
        ]
    );
    // Ostrel starts a component after a skipped one (inner); Current
    // Deck's label holds no `ost`.
    assert_eq!(
        shown(&ask(&names, &text("ost"), 3, Some(Section::Decks), &[])),
        ["Decks: deck Pelmo::Ostrel"]
    );
    let flags = shown(&ask(&names, &text(""), 0, Some(Section::Flag), &[]));
    assert_eq!(flags.len(), 9);
    assert_eq!(flags[0], "Flag: Flag(1)");
    assert_eq!(flags[7], "Flag: NoFlag");
    assert_eq!(flags[8], "Flag: AnyFlag");
    let saved = shown(&ask(&names, &text(""), 0, Some(Section::SavedSearches), &[]));
    assert_eq!(saved.len(), 5);
}

// T12.2: Anki syntax at the caret gives D6's candidates for that term.
#[test]
fn syntax_at_the_caret_completes_as_d6_says() {
    let names = pelmo();
    let rows = at_end(&names, "deck:pel");
    let first: Vec<String> = shown(&rows).into_iter().take(3).collect();
    assert_eq!(first, ["AsTyped deck:pel", "Deck Pelmo", "Deck Pelmo::*"]);
    assert!(matches!(&rows[1].row, Row::Syntax(c) if c.kind == CandidateKind::Deck
        && c.insert.as_ref().unwrap().text == "deck:Pelmo"));
    assert_eq!(show(rows.last().unwrap()), "save search");
    // No key before the caret: the word's own list.
    assert_eq!(show(&at_end(&names, "pelmo")[0]), "contains pelmo");
}

#[test]
fn a_caret_past_the_text_is_refused() {
    let names = pelmo();
    let request = SuggestRequest {
        query: &text("ab"),
        caret: 3,
        scope: None,
        recent: &[],
    };
    assert_eq!(suggest(&names, &Labels::default(), &request), Err(InvalidOffset));
}

// T12.1: the word's list matches deck and tag names; Current Deck and
// Untagged are in their kind's value list.
#[test]
fn current_deck_and_untagged_are_in_their_kinds_list_only() {
    let names = pelmo();
    let word = shown(&at_end(&names, "cur"));
    assert_eq!(word, ["contains cur", "save search"]);
    assert_eq!(
        shown(&ask(&names, &text("cur"), 3, Some(Section::Decks), &[])),
        ["Decks: CurrentDeck"]
    );
    assert_eq!(
        shown(&ask(&names, &text("unt"), 3, Some(Section::Tags), &[])),
        ["Tags: Untagged"]
    );
}

// More saved searches than the list holds: every kind stays, and the
// saved searches end with Show All.
#[test]
fn the_kinds_stay_when_saved_searches_overflow() {
    let saved: Vec<String> = (0..60).map(|k| format!("\"s{k:02}\":\"is:new\"")).collect();
    let names = index(&[], &[], &[], &format!("{{{}}}", saved.join(",")));
    let rows = at_end(&names, "");
    // 39 saved searches and 11 kinds: 50 rows, and Show All.
    assert_eq!(rows.len(), 51);
    assert_eq!(show(&rows[39]), "SavedSearches: show all SavedSearches");
    assert_eq!(show(&rows[40]), "FilterBy: Decks");
    assert_eq!(show(rows.last().unwrap()), "FilterBy: Fields");
}

// T12.2: a key before the caret wins over an empty word, so a quoted value
// holding a space still completes.
#[test]
fn a_quoted_value_with_a_space_still_completes() {
    let names = index(&[(1, "New York"), (2, "Default")], &[], &[], "{}");
    let rows = ask(&names, &text("deck:\"New Y"), 10, None, &[]);
    assert!(matches!(&rows[0].row, Row::Syntax(c) if c.kind == CandidateKind::AsTyped), "{:?}", shown(&rows));
    let rows = ask(&names, &text("deck:\"New "), 10, None, &[]);
    assert!(shown(&rows).iter().any(|row| row == "Deck New York"), "{:?}", shown(&rows));
}
