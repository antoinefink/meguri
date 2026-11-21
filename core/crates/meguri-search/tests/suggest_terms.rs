//! Candidates for the term at the cursor (`specs/search/editing-services.md`
//! D6, which Browse's suggestions hand Anki syntax to: `filter-tokens`
//! T12.2): D6.3's contexts, D6.4's scope and classes, D6.5's order with and
//! without recent texts and the cut at the limit, and D6.6's insertions.

use meguri_model::ids::{DeckConfigId, DeckId, NotetypeId};
use meguri_search::name_index::{DeckRow, NameIndex, NotetypeNames, PresetName};
use meguri_search::suggest::{Candidate, CandidateKind, Detail, Unit, key_at, suggest};
use meguri_search::tail::InvalidOffset;
use meguri_storage::ConfigValue;

fn names() -> NameIndex {
    let mut names = NameIndex::default();
    let decks = [
        (1, "Default", false),
        (2, "Languages::French", false),
        (3, "Languages", false),
        (4, "a_b* (x)", false),
        (5, "axb", false),
        (6, "a::b", false),
        (7, "a b", false),
        (8, "a-b", false),
        (9, "a", false),
        (10, "Japanese::Lesson1", false),
        (11, "Lessons", false),
        (12, "Japanese", false),
        (13, "Cram", true),
        (14, "current", false),
    ];
    names.set_decks(
        decks
            .iter()
            .map(|(id, name, filtered)| DeckRow {
                id: DeckId(*id),
                native: name.replace("::", "\u{1f}"),
                filtered: *filtered,
            })
            .collect(),
    );
    names.set_tags(
        ["animal", "animal::mammal", "Japanese::Audio", "japan", "verb"]
            .iter()
            .map(|t| (*t).to_owned())
            .collect(),
    );
    let types: [(i64, &str, &[&str], &[&str]); 3] = [
        (100, "Basic", &["Front", "Back"], &["Card 1"]),
        (101, "Kaishi", &["Word", "Meaning", "Extra Info"], &["Recognition", "Recall"]),
        (102, "Odd", &["my:field", "Deck"], &["7", "Card 1", "Card 2"]),
    ];
    names.set_notetypes(
        types
            .iter()
            .map(|(id, name, fields, templates)| NotetypeNames {
                id: NotetypeId(*id),
                name: (*name).to_owned(),
                fields: fields.iter().map(|f| (*f).to_owned()).collect(),
                templates: templates.iter().map(|t| (*t).to_owned()).collect(),
            })
            .collect(),
    );
    names.set_presets(vec![
        PresetName {
            id: DeckConfigId(1),
            name: "Default".to_owned(),
        },
        PresetName {
            id: DeckConfigId(2),
            name: "Fast \"one\"".to_owned(),
        },
    ]);
    names.set_saved_searches(
        ConfigValue::parse(br#"{"Leeches":"tag:leech","Broken":"(a","Leech review":"tag:leech is:review"}"#).as_ref(),
    );
    names.set_flag_names(["Red", "Orange", "Important", "Blue", "Pink", "Turquoise", "Purple"].map(str::to_owned));
    names
}

fn at_end(text: &str) -> Vec<Candidate> {
    suggest(text, text.chars().count(), 50, &[], &names()).unwrap()
}

fn labels(candidates: &[Candidate]) -> Vec<String> {
    candidates.iter().map(|c| c.label.clone()).collect()
}

fn inserted(candidate: &Candidate) -> &str {
    &candidate.insert.as_ref().unwrap().text
}

/// The field text after accepting `candidate` in `text`.
fn after(text: &str, candidate: &Candidate) -> (String, usize) {
    let insert = candidate.insert.as_ref().unwrap();
    let chars: Vec<char> = text.chars().collect();
    let mut out: String = chars[..insert.range.start].iter().collect();
    out.push_str(&insert.text);
    out.extend(&chars[insert.range.end..]);
    (out, insert.cursor_after)
}

fn find<'a>(candidates: &'a [Candidate], label: &str) -> &'a Candidate {
    candidates
        .iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("{label} in {:?}", labels(candidates)))
}

// D6.3: a word gives `as_typed`, then the keywords it begins, then field
// keys and saved searches; `pr` gives `prop:` before `preset:`.
#[test]
fn a_word_offers_keywords_field_keys_and_saved_searches() {
    let de = at_end("de");
    assert_eq!(de[0].kind, CandidateKind::AsTyped);
    assert_eq!(de[1].label, "deck:");
    assert!(!de[1].completes);
    assert_eq!(after("de", &de[1]).0, "deck:");
    // The field Deck of Odd is offered as a field key? No: J3.3 refuses a
    // field named like a reserved key.
    assert!(!labels(&de).contains(&"Deck".to_owned()), "{:?}", labels(&de));
    let pr = at_end("pr");
    assert_eq!(labels(&pr)[1..3], ["prop:", "preset:"]);
    // `-de`: the `-` stays.
    let minus = at_end("-de");
    assert_eq!(after("-de", &minus[1]).0, "-deck:");
}

// D6.6: a field key is quoted when it needs it; the cursor stops before
// the closing quote.
#[test]
fn a_field_key_is_written_with_its_cursor_inside_the_quotes() {
    let ext = at_end("Ext");
    let key = find(&ext, "Extra Info");
    assert_eq!(key.kind, CandidateKind::Field);
    let (text, cursor) = after("Ext", key);
    assert_eq!(text, "\"Extra Info:\"");
    assert_eq!(cursor, 12);
    assert!(!key.completes);
}

// D6.4's scope: `note:Kaishi Mea` offers the field key Meaning, whose
// forms come once `Meaning:` is typed.
#[test]
fn field_keys_come_from_the_scoped_note_types() {
    let text = "note:Kaishi Mea";
    let found = at_end(text);
    assert_eq!(labels(&found), ["Mea", "Meaning"]);
    let (text, _) = after(text, &found[1]);
    assert_eq!(text, "note:Kaishi Meaning:");
    let forms = at_end(&text);
    assert_eq!(
        labels(&forms),
        ["Meaning:", "Meaning:_*", "Meaning:", "Meaning:re:", "Meaning:nc:"]
    );
    let inserts: Vec<&str> = forms[1..].iter().map(inserted).collect();
    assert_eq!(inserts, ["Meaning:_*", "Meaning:", "Meaning:re:", "Meaning:nc:"]);
    let completes: Vec<bool> = forms[1..].iter().map(|c| c.completes).collect();
    assert_eq!(completes, [true, true, false, false]);
    // An escaped key names the field `my:field`.
    let escaped = at_end(r"my\:field:");
    assert_eq!(inserted(&escaped[1]), r"my\:field:_*");
    // A key naming no field of the scope gives only `as_typed`.
    assert_eq!(at_end("note:Basic Meaning:").len(), 1);
}

// D6.4: `card:` lists the scoped note types' templates; a scope naming no
// note type leaves only `as_typed`.
#[test]
fn templates_follow_the_scope() {
    let basic = at_end("note:\"Basic\" card:");
    assert_eq!(labels(&basic), ["card:", "1", "Card 1"]);
    assert_eq!(inserted(&basic[2]), "\"card:Card 1\"");
    assert_eq!(at_end("note:Kaishx card:").len(), 1);
    // Every note type when no note: term stands in the run: numbers up to
    // the largest template count; the template named `7` reads back as a
    // number and is not offered by name.
    let all = at_end("card:");
    assert_eq!(labels(&all)[..4], ["card:", "1", "2", "3"]);
    assert!(!labels(&all)[4..].contains(&"7".to_owned()));
    // An OR between runs: the other run's note: term does not scope.
    let or = at_end("note:Basic or card:Rec");
    assert_eq!(labels(&or), ["card:Rec", "Recall", "Recognition"]);
    // An enclosing group's run scopes; a negated note: term does not.
    assert_eq!(labels(&at_end("note:Kaishi (card:Rec")), ["card:Rec", "Recall", "Recognition"]);
    assert_eq!(labels(&at_end("-note:Basic card:Rec")), ["card:Rec", "Recall", "Recognition"]);
}

// D6.3 and D6.6 for decks: specials, components in two classes, `Name::*`
// after a deck with subdecks, escapes and quotes in the insert text.
#[test]
fn decks_match_by_component_and_insert_their_items() {
    let fre = at_end("deck:fre");
    let french = find(&fre, "Languages::French");
    assert_eq!(after("deck:fre", french).0, "deck:Languages::French");
    assert!(french.completes);
    let a = at_end("deck:a_");
    assert_eq!(inserted(find(&a, "a_b* (x)")), r#""deck:a\_b\* (x)""#);
    // D6.5: `a::b` before `a b` and `a-b`; `a` and its `a::*` first.
    let order = labels(&at_end("deck:a"));
    let pos = |label: &str| order.iter().position(|l| l == label).unwrap();
    assert!(pos("a") < pos("a::*") && pos("a::*") < pos("a::b"), "{order:?}");
    assert!(pos("a::b") < pos("a b") && pos("a b") < pos("a-b"), "{order:?}");
    // D6.4's example: Lessons (prefix) before Japanese::Lesson1 (inner).
    let lesson = labels(&at_end("deck:lesson"));
    assert_eq!(lesson[1..], ["Lessons", "Japanese::Lesson1"]);
    // `deck:Cur` offers current, then the deck named current as `Current`.
    let cur = at_end("deck:Cur");
    assert_eq!(cur[1].label, "current");
    assert_eq!(inserted(&cur[1]), "deck:current");
    assert_eq!(inserted(&cur[2]), "deck:Current");
    // A filtered deck carries the marker.
    let cram = at_end("deck:cram");
    assert_eq!(find(&cram, "Cram").detail, Some(Detail::Filtered));
}

// D6.3 for tags: `none`, then registered tags by component; accepting
// `animal` leads to its descendants.
#[test]
fn tags_match_by_component() {
    assert_eq!(labels(&at_end("tag:jap")), ["tag:jap", "japan", "Japanese::Audio"]);
    assert_eq!(labels(&at_end("tag:jap::aud")), ["tag:jap::aud", "Japanese::Audio"]);
    assert_eq!(labels(&at_end("tag:no")), ["tag:no", "none"]);
    let animal = at_end("tag:animal");
    assert_eq!(inserted(find(&animal, "animal")), "tag:animal");
    assert_eq!(labels(&at_end("tag:animal")), ["tag:animal", "animal", "animal::mammal"]);
    // tag:re: and tag:nc: give nothing but `as_typed`.
    assert_eq!(at_end("tag:re:ani").len(), 1);
}

// D6.3: flags by number or by shown name, once each, in number order.
#[test]
fn flags_states_days_buttons_and_presets() {
    let imp = at_end("flag:imp");
    assert_eq!(labels(&imp), ["flag:imp", "3"]);
    assert_eq!(inserted(&imp[1]), "flag:3");
    assert_eq!(imp[1].detail, Some(Detail::FlagName("Important".to_owned())));
    let flags = at_end("flag:");
    assert_eq!(flags.len(), 9);
    assert_eq!(labels(&at_end("is:bur")), ["is:bur", "buried", "buried-manually", "buried-sibling"]);
    assert_eq!(labels(&at_end("added:")), ["added:", "1", "7", "30", "365"]);
    assert_eq!(labels(&at_end("rated:3")), ["rated:3", "30", "365"]);
    let buttons = at_end("rated:7:");
    assert_eq!(labels(&buttons), ["rated:7:", "1", "2", "3", "4"]);
    assert_eq!(inserted(&buttons[4]), "rated:7:4");
    // A preset holding `"` cannot be written.
    assert_eq!(labels(&at_end("preset:")), ["preset:", "Default"]);
    for key in ["did:", "nid:", "dupe:", "re:", "has-cd:"] {
        assert_eq!(at_end(key).len(), 1, "{key}");
    }
}

// D6.3.1.
#[test]
fn properties_then_operators_then_a_value_hint() {
    assert_eq!(
        labels(&at_end("prop:r")),
        ["prop:r", "reps", "rated", "resched", "<", "<=", ">", ">=", "=", "!="]
    );
    assert_eq!(labels(&at_end("prop:ivl>")), ["prop:ivl>", ">", ">="]);
    let hint = at_end("prop:due>-3");
    assert_eq!(hint[1].kind, CandidateKind::ValueHint);
    assert_eq!(hint[1].detail, Some(Detail::Unit(Unit::DaysFromToday)));
    assert!(hint[1].insert.is_none());
    assert_eq!(labels(&at_end("prop:cdn:k")), ["prop:cdn:k", "<", "<=", ">", ">=", "=", "!="]);
}

// D6.6: saved searches, bracketed except in an empty field; one whose text
// does not parse is not offered.
#[test]
fn saved_searches_insert_their_canonical_text() {
    let lee = at_end("deck:x lee");
    let leeches = find(&lee, "Leeches");
    assert_eq!(after("deck:x lee", leeches).0, "deck:x (tag:leech)");
    let empty = suggest("", 0, 50, &[], &names()).unwrap();
    assert_eq!(labels(&empty), ["Leech review", "Leeches"]);
    assert_eq!(after("", &empty[1]).0, "tag:leech");
    assert_eq!(inserted(&empty[0]), "tag:leech is:review");
}

// D6.5: recent texts rank first within their group, never across groups;
// the limit cuts the list.
#[test]
fn recent_texts_and_the_limit() {
    let recent = vec!["deck:Japanese::Lesson1".to_owned()];
    let ranked = suggest("deck:lesson", 11, 50, &recent, &names()).unwrap();
    assert_eq!(labels(&ranked)[1..], ["Lessons", "Japanese::Lesson1"]);
    let recent = vec!["deck:Languages".to_owned()];
    let ranked = suggest("deck:la", 7, 50, &recent, &names()).unwrap();
    assert_eq!(labels(&ranked)[1], "Languages");
    let recent = vec!["deck:Languages::French".to_owned()];
    let ranked = suggest("deck:la", 7, 50, &recent, &names()).unwrap();
    assert_eq!(labels(&ranked)[1], "Languages::French");
    assert_eq!(suggest("deck:", 5, 3, &[], &names()).unwrap().len(), 3);
    assert!(suggest("deck:", 5, 0, &[], &names()).unwrap().is_empty());
    assert_eq!(suggest("ab", 3, 50, &[], &names()), Err(InvalidOffset));
}

// D6.2: the current term is the one that holds the cursor or ends at it;
// none after a space; a quoted term's quotes are not typed text.
#[test]
fn the_current_term_and_its_key() {
    assert_eq!(key_at("deck:a is:due", 6).as_deref(), Some("deck"));
    assert_eq!(key_at("deck:a is:due", 13).as_deref(), Some("is"));
    assert_eq!(key_at("deck:a ", 7), None);
    assert_eq!(key_at("DECK:a", 6).as_deref(), Some("deck"));
    assert_eq!(key_at("word", 4), None);
    assert_eq!(key_at("tag:\"wk", 7).as_deref(), Some("tag"));
    // A cursor before a later term's start keeps the earlier one.
    let quoted = at_end("\"deck:lang");
    assert_eq!(labels(&quoted)[1], "Languages");
    let inside = suggest("deck:\"Lang", 10, 50, &[], &names()).unwrap();
    assert_eq!(labels(&inside)[1], "Languages");
    // Empty at the cursor: keywords and saved searches, no `as_typed`.
    let empty = at_end("deck:a ");
    assert_eq!(empty[0].kind, CandidateKind::Keyword);
    // A backslash before a space in the term: nothing.
    assert!(at_end("a\\ b").is_empty());
}
