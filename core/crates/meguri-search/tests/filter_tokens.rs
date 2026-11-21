//! Browse's filter tokens (`specs/search/filter-tokens.md` T16, core part):
//! T4's run texts, T7's readings, T5's text items, T7.1's round trip as a
//! property test whose run texts are run by the product's term SQL on a
//! copied schema-18 collection, and the whole collection in Notes mode.

use std::collections::{BTreeSet, HashMap};
use std::{fs, path::Path};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::rng::SeededRandom;
use meguri_search::builder::{BuiltItem, build, build_query};
use meguri_search::filter_tokens::{
    DeckValue, DueValue, FieldTest, FilterQuery, Kind, NotetypeValue, Period, RunTextError, StateValue, TextReading,
    Token, Value, ValueToken, card_type_text, classify_text, read_text,
};
use meguri_search::name_index::{DeckRow, NameIndex, NotetypeNames};
use meguri_search::tail::{Incomplete, Tail, classify_tail};
use meguri_search::{
    FieldShape, FoldMode, NotetypeShape, Param, Plan, SearchContext, SearchMode, SearchNotetypes, parse,
};
use meguri_storage::{Initializers, OpenOptions, OpenedCollection, open_collection, raw::SqlValue};

use crate::canonical_roundtrip::Seq;

/// The next day boundary of the context; day windows count back from it.
const NEXT_DAY: i64 = 2_000_000_100;
const DAY: i64 = 86_400;
const NOW: i64 = 2_000_000_000;

fn context() -> SearchContext {
    SearchContext {
        now: NOW,
        today: 12,
        next_day_at: NEXT_DAY,
        learn_ahead_secs: 1200,
        current_deck: DeckId(10),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

fn s(text: &str) -> String {
    text.to_owned()
}

fn value(values: Vec<Value>) -> Token {
    Token::Value(ValueToken::new(values, false))
}

fn not_value(values: Vec<Value>) -> Token {
    Token::Value(ValueToken::new(values, true))
}

// Decks (id, full name, filtered). Deck 18's name J3.5 refuses (U+00A0),
// so it and its subdeck 19 are written by id.
const DECKS: [(i64, &str, bool); 14] = [
    (1, "Default", false),
    (10, "Spanish", false),
    (11, "Spanish::Verbs", false),
    (12, "Lang", false),
    (13, "Lang::Spanish", false),
    (15, "日本語", false),
    (14, "日本語::Kanji", false),
    (16, "x_y*", false),
    (17, "My Deck (old)", false),
    (18, "a\u{a0}b", false),
    (19, "a\u{a0}b::kid", false),
    (20, "Ünïcode", false),
    (21, "Filtered One", true),
    (22, "current", false),
];

// Registered tags; `r` U+00A0 `s` is written by J3.2's regex form.
const TAGS: [&str; 7] = ["alpha", "beta", "beta::child", "Gamma", "ünïcode", "leech", "r\u{a0}s"];

struct Type {
    id: i64,
    name: &'static str,
    fields: &'static [&'static str],
    templates: &'static [&'static str],
}

// Note type 102's name is not in NFC: J3.5 refuses it, so it is written by id.
const TYPES: [Type; 3] = [
    Type {
        id: 100,
        name: "Basic",
        fields: &["Front", "Back", "Extra"],
        templates: &["Card 1", "Card 2"],
    },
    Type {
        id: 101,
        name: "Vocab Ünit",
        fields: &["Word", "Notes Field"],
        templates: &["Plimbo", "Varnex"],
    },
    Type {
        id: 102,
        name: "e\u{301}",
        fields: &["Text"],
        templates: &["Card 1"],
    },
];

fn names() -> NameIndex {
    let mut names = NameIndex::default();
    names.set_decks(
        DECKS
            .iter()
            .map(|(id, name, filtered)| DeckRow {
                id: DeckId(*id),
                native: name.replace("::", "\u{1f}"),
                filtered: *filtered,
            })
            .collect(),
    );
    names.set_tags(TAGS.iter().map(|tag| s(tag)).collect());
    names.set_notetypes(
        TYPES
            .iter()
            .map(|t| NotetypeNames {
                id: NotetypeId(t.id),
                name: s(t.name),
                templates: t.templates.iter().map(|x| s(x)).collect(),
                fields: t.fields.iter().map(|x| s(x)).collect(),
            })
            .collect(),
    );
    names
}

fn notetypes() -> SearchNotetypes {
    SearchNotetypes::new(
        TYPES
            .iter()
            .map(|t| NotetypeShape {
                id: NotetypeId(t.id),
                name: s(t.name),
                cloze: false,
                sort_field: 0,
                fields: t
                    .fields
                    .iter()
                    .map(|name| FieldShape {
                        name: s(name),
                        excluded: false,
                    })
                    .collect(),
            })
            .collect(),
    )
}

fn deck(names: &NameIndex, id: i64) -> Value {
    Value::Deck(DeckValue::of(names, DeckId(id)).unwrap())
}

fn notetype(names: &NameIndex, id: i64) -> Value {
    let n = names.notetypes().iter().find(|n| n.id == NotetypeId(id)).unwrap();
    Value::Notetype(NotetypeValue::of(n))
}

fn tag(name: &str) -> Value {
    Value::Tag(s(name))
}

fn field(name: &str, test: FieldTest) -> Value {
    Value::Field { field: s(name), test }
}

fn run(query: &FilterQuery) -> String {
    query.run_text(&names()).unwrap_or_else(|e| panic!("{query:?}: {e:?}"))
}

// T16: T4's table, the run texts exactly.
#[test]
fn the_run_texts_of_t4s_table() {
    let names = names();
    let spanish = deck(&names, 10);
    let cases = [
        (FilterQuery::new(vec![value(vec![spanish.clone()])], ""), "deck:Spanish"),
        (
            FilterQuery::new(
                vec![
                    value(vec![spanish]),
                    value(vec![tag("alpha"), tag("beta")]),
                    not_value(vec![Value::State(StateValue::Suspended)]),
                ],
                "word",
            ),
            "deck:Spanish (tag:alpha OR tag:beta) -is:suspended word",
        ),
        (
            FilterQuery::new(vec![not_value(vec![tag("alpha"), tag("beta")])], ""),
            "-(tag:alpha OR tag:beta)",
        ),
        (
            FilterQuery::new(
                vec![value(vec![Value::Due(DueValue::Today), Value::Due(DueValue::Overdue)])],
                "",
            ),
            "prop:due=0 OR (is:due -prop:due=0)",
        ),
        (
            FilterQuery::new(vec![value(vec![Value::State(StateValue::Leech)])], ""),
            "tag:leech",
        ),
        (
            FilterQuery::new(
                vec![
                    value(vec![field("Back", FieldTest::Contains(s("x")))]),
                    value(vec![field("Extra", FieldTest::Empty)]),
                ],
                "",
            ),
            "Back:*x* Extra:",
        ),
        (
            FilterQuery::new(vec![Token::Advanced(s("prop:ivl>30"))], "two words"),
            "prop:ivl>30 two words",
        ),
    ];
    for (query, expected) in cases {
        assert_eq!(run(&query), expected, "{query:?}");
    }
}

// T3's other built items, each value alone.
#[test]
fn each_value_runs_its_built_item() {
    let names = names();
    let cases = [
        (Value::CurrentDeck, "deck:current"),
        (deck(&names, 22), "deck:Current"),
        (deck(&names, 16), "deck:x\\_y\\*"),
        (deck(&names, 17), "\"deck:My Deck (old)\""),
        (deck(&names, 14), "deck:日本語::Kanji"),
        // J3.5: the deck and its subdecks by id.
        (deck(&names, 18), "did:18,19"),
        (Value::Untagged, "-tag:_*"),
        (tag("r\u{a0}s"), "\"tag:re:^r\\x{a0}s(::|$)\""),
        (notetype(&names, 101), "\"note:Vocab Ünit\""),
        (notetype(&names, 102), "mid:102"),
        (Value::CardType(s("Card 1")), "\"card:Card 1\""),
        (Value::CardType(s("a_b")), "card:a\\_b"),
        (Value::State(StateValue::New), "is:new"),
        (Value::State(StateValue::Learning), "is:learn"),
        (Value::State(StateValue::Review), "is:review"),
        (Value::State(StateValue::Buried), "is:buried"),
        (Value::Flag(1), "flag:1"),
        (Value::Flag(7), "flag:7"),
        (Value::NoFlag, "flag:0"),
        (Value::AnyFlag, "-flag:0"),
        (Value::Due(DueValue::Overdue), "is:due -prop:due=0"),
        (Value::Due(DueValue::Next7Days), "prop:due>=1 prop:due<=7"),
        (Value::Added(Period::Today), "added:1"),
        (Value::Added(Period::Last7Days), "added:7"),
        (Value::Edited(Period::Last30Days), "edited:30"),
        (Value::Studied(Period::Last7Days), "rated:7"),
        (field("Notes Field", FieldTest::Contains(s("x"))), "\"Notes Field:*x*\""),
        (field("Back", FieldTest::HasContent), "Back:_*"),
        (Value::Duplicates(vec![NoteId(5), NoteId(3)]), "nid:5,3"),
        (Value::StudiedOn(vec![CardId(7)]), "cid:7"),
    ];
    for (value, expected) in cases {
        let query = FilterQuery::new(vec![self::value(vec![value.clone()])], "");
        assert_eq!(run(&query), expected, "{value:?}");
    }
}

// T3: a value whose built item J3 refuses is not offered, and a query
// holding one has no run text. J3.5: a fallback deck list with a negative
// descendant id is not built; no id is dropped from it.
#[test]
fn values_without_a_built_item_are_not_offered() {
    let mut names = names();
    let mut rows: Vec<DeckRow> = names
        .decks()
        .iter()
        .map(|d| DeckRow {
            id: d.id,
            native: d.native.clone(),
            filtered: d.filtered,
        })
        .collect();
    rows.push(DeckRow {
        id: DeckId(30),
        native: s("q\u{a0}r"),
        filtered: false,
    });
    rows.push(DeckRow {
        id: DeckId(-31),
        native: s("q\u{a0}r\u{1f}neg"),
        filtered: false,
    });
    names.set_decks(rows);
    let refused_deck = Value::Deck(DeckValue::of(&names, DeckId(30)).unwrap());
    assert_eq!(
        DeckValue::of(&names, DeckId(30)).unwrap().tree,
        vec![DeckId(30), DeckId(-31)]
    );
    let not_offered = [
        refused_deck.clone(),
        Value::Notetype(NotetypeValue {
            id: NotetypeId(-5),
            name: s("e\u{301}"),
        }),
        Value::Notetype(NotetypeValue {
            id: NotetypeId(5),
            name: s(""),
        }),
        Value::Tag(s("")),
        Value::Flag(0),
        Value::Flag(8),
        Value::CardType(s("")),
        Value::CardType(s("123")),
        Value::CardType(s("+1")),
        Value::CardType(s("a\u{a0}b")),
        Value::CardType(s("e\u{301}")),
        field("deck", FieldTest::HasContent),
        field("", FieldTest::Empty),
        field("Back", FieldTest::Contains(s(""))),
        Value::Duplicates(vec![]),
        Value::StudiedOn(vec![CardId(-1)]),
    ];
    for value in not_offered {
        assert!(!value.is_offered(), "{value:?}");
        let query = FilterQuery::new(
            vec![self::value(vec![Value::NoFlag]), self::value(vec![value.clone()])],
            "",
        );
        assert_eq!(
            query.run_text(&names),
            Err(RunTextError::NotOffered { token: 1, value: 0 }),
            "{value:?}"
        );
    }
    // A name of ASCII digits is not offered even where G8 would read it as
    // a name; a name with letters is.
    assert_eq!(card_type_text("65536"), None);
    assert_eq!(card_type_text("Card 2").as_deref(), Some("\"card:Card 2\""));
    // Tokens with no values or mixed kinds have no run text.
    let empty = FilterQuery::new(vec![Token::Value(ValueToken::new(vec![], false))], "");
    assert_eq!(empty.run_text(&names), Err(RunTextError::InvalidToken { token: 0 }));
    let mixed = FilterQuery::new(vec![value(vec![Value::NoFlag, tag("alpha")])], "");
    assert_eq!(mixed.run_text(&names), Err(RunTextError::InvalidToken { token: 0 }));
}

/// A small index for T7's table: `Lang::Spanish`, `A`, `B`, and the tags.
fn table_names() -> NameIndex {
    let mut names = NameIndex::default();
    names.set_decks(
        [
            (1, "Lang"),
            (2, "Lang\u{1f}Spanish"),
            (3, "A"),
            (4, "B"),
            (5, "Spanish"),
        ]
        .into_iter()
        .map(|(id, native)| DeckRow {
            id: DeckId(id),
            native: s(native),
            filtered: false,
        })
        .collect(),
    );
    names.set_tags(vec![s("alpha"), s("beta")]);
    names
}

// T16: T7's table; a text with a top-level OR; `deck:*`; empty.
#[test]
fn t7s_table_reads_its_queries() {
    let names = table_names();
    let d = |id| Value::Deck(DeckValue::of(&names, DeckId(id)).unwrap());
    let cases = [
        (
            "deck:\"Lang::Spanish\" -is:suspended tag:alpha",
            FilterQuery::new(
                vec![
                    value(vec![d(2)]),
                    not_value(vec![Value::State(StateValue::Suspended)]),
                    value(vec![tag("alpha")]),
                ],
                "",
            ),
        ),
        (
            "tag:alpha or tag:beta word",
            FilterQuery::new(vec![Token::Advanced(s("tag:alpha OR tag:beta word"))], ""),
        ),
        (
            "(tag:alpha or tag:beta) word",
            FilterQuery::new(vec![value(vec![tag("alpha"), tag("beta")])], "word"),
        ),
        (
            "deck:A deck:B",
            FilterQuery::new(vec![value(vec![d(3)]), Token::Advanced(s("deck:B"))], ""),
        ),
        (
            "prop:ivl>30 is:review",
            FilterQuery::new(
                vec![
                    Token::Advanced(s("prop:ivl>30")),
                    value(vec![Value::State(StateValue::Review)]),
                ],
                "",
            ),
        ),
        ("\"deck:a", FilterQuery::new(vec![], "\"deck:a")),
        // A top-level OR of one kind is one token (step 2).
        (
            "tag:alpha OR tag:beta",
            FilterQuery::new(vec![value(vec![tag("alpha"), tag("beta")])], ""),
        ),
        (
            "is:new or -is:new",
            FilterQuery::new(vec![Token::Advanced(s("is:new OR -is:new"))], ""),
        ),
        // Step 1.
        ("deck:*", FilterQuery::default()),
        ("", FilterQuery::default()),
        ("  \u{1f} ", FilterQuery::default()),
    ];
    for (text, expected) in cases {
        assert_eq!(read_text(text, &names), expected, "{text:?}");
    }
    // The unclosed quote runs as its literal word.
    assert_eq!(
        read_text("\"deck:a", &names).run_text(&names).unwrap(),
        build(&BuiltItem::Literal(s("\"deck:a"))).unwrap()
    );
}

// T16: the negative and positive forms of one another, and names matched
// with letter case folded.
#[test]
fn flags_untagged_and_folded_names_read_as_values() {
    let mut names = table_names();
    names.set_tags(vec![s("alpha")]);
    let cases = [
        ("flag:0", value(vec![Value::NoFlag])),
        ("-flag:0", value(vec![Value::AnyFlag])),
        ("-tag:_*", value(vec![Value::Untagged])),
        (
            "deck:spanish",
            value(vec![Value::Deck(DeckValue::of(&names, DeckId(5)).unwrap())]),
        ),
        ("tag:Alpha", value(vec![tag("alpha")])),
        // T10's forms: Is Not is never set on No Flag, Any Flag or Untagged.
        ("-(flag:0)", Token::Advanced(s("-(flag:0)"))),
        ("tag:_*", Token::Advanced(s("tag:_*"))),
        ("-(flag:1 OR flag:0)", Token::Advanced(s("-(flag:1 OR flag:0)"))),
        ("-(-tag:_* OR tag:alpha)", Token::Advanced(s("-(-tag:_* OR tag:alpha)"))),
        ("-flag:3", not_value(vec![Value::Flag(3)])),
        ("deck:current", value(vec![Value::CurrentDeck])),
        ("deck:filtered", Token::Advanced(s("deck:filtered"))),
        // A deck named by no deck, or by a pattern, stays advanced.
        ("deck:French", Token::Advanced(s("deck:French"))),
        ("deck:Span*", Token::Advanced(s("deck:Span*"))),
        ("tag:gamma", Token::Advanced(s("tag:gamma"))),
    ];
    for (text, expected) in cases {
        assert_eq!(read_text(text, &names).tokens, vec![expected], "{text:?}");
    }
}

// T16: the run texts of one OR token, read back by T7 (step 2).
#[test]
fn one_or_token_reads_back_as_itself() {
    let names = names();
    let tokens = [
        value(vec![tag("alpha"), tag("beta")]),
        value(vec![Value::Due(DueValue::Today), Value::Due(DueValue::Overdue)]),
        // Two sidebar values by ⌘-click.
        value(vec![deck(&names, 10), deck(&names, 13)]),
        value(vec![Value::Flag(1), Value::NoFlag]),
        value(vec![
            Value::Due(DueValue::Next7Days),
            Value::Due(DueValue::Overdue),
            Value::Due(DueValue::Today),
        ]),
        value(vec![Value::State(StateValue::New), Value::State(StateValue::Leech)]),
        value(vec![tag("alpha"), tag("leech")]),
        value(vec![
            field("Back", FieldTest::Contains(s("x y"))),
            field("Back", FieldTest::Empty),
        ]),
    ];
    for token in tokens {
        let query = FilterQuery::new(vec![token], "");
        let text = run(&query);
        assert_eq!(read_text(&text, &names), query, "{text:?}");
    }
}

// T7: `tag:leech` alone is Card State: Leech; among tags a tag, among
// states a state. T7.1: a Tag token holding only leech comes back as it.
#[test]
fn leech_reads_by_its_neighbours() {
    let names = names();
    let leech = Value::State(StateValue::Leech);
    assert_eq!(read_text("tag:leech", &names).tokens, vec![value(vec![leech.clone()])]);
    assert_eq!(
        read_text("-tag:LEECH", &names).tokens,
        vec![not_value(vec![leech.clone()])]
    );
    assert_eq!(
        read_text("(tag:leech OR is:new)", &names).tokens,
        vec![value(vec![leech.clone(), Value::State(StateValue::New)])]
    );
    assert_eq!(
        read_text("(tag:beta OR tag:leech)", &names).tokens,
        vec![value(vec![tag("beta"), tag("leech")])]
    );
    let only = FilterQuery::new(vec![value(vec![tag("leech")])], "");
    assert_eq!(read_text(&run(&only), &names).tokens, vec![value(vec![leech])]);
}

// T7's comparisons: field names with letter case folded keep the stored
// spelling; J3.2's tags; T3.1's tests; a field naming no field is text.
#[test]
fn field_tag_and_card_type_items_read_as_values() {
    let names = names();
    let cases = [
        ("back:*x*", value(vec![field("Back", FieldTest::Contains(s("x")))])),
        ("\"Notes Field:\"", value(vec![field("Notes Field", FieldTest::Empty)])),
        ("Extra:_*", value(vec![field("Extra", FieldTest::HasContent)])),
        (
            "-Front:*a\\_b*",
            not_value(vec![field("Front", FieldTest::Contains(s("a_b")))]),
        ),
        ("\"tag:re:^r\\x{a0}s(::|$)\"", value(vec![tag("r\u{a0}s")])),
        ("\"card:Card 1\"", value(vec![Value::CardType(s("Card 1"))])),
        ("card:varnex", value(vec![Value::CardType(s("varnex"))])),
        // Other forms stay advanced: a test T3 has not, a pattern, a mode,
        // an ordinal, an id list.
        ("Back:x", Token::Advanced(s("Back:x"))),
        ("Back:*x", Token::Advanced(s("Back:*x"))),
        ("B*:_*", Token::Advanced(s("B*:_*"))),
        ("Back:re:x", Token::Advanced(s("Back:re:x"))),
        ("card:2", Token::Advanced(s("card:2"))),
        ("card:Card*", Token::Advanced(s("card:Card*"))),
        ("did:18,19", Token::Advanced(s("did:18,19"))),
        ("mid:102", Token::Advanced(s("mid:102"))),
        ("nid:1,2", Token::Advanced(s("nid:1,2"))),
    ];
    for (text, expected) in cases {
        assert_eq!(read_text(text, &names).tokens, vec![expected], "{text:?}");
    }
    let query = read_text("10:45 -xyz", &names);
    assert_eq!(query, FilterQuery::new(vec![], "10:45 -xyz"));
}

// T16: T5's items of single texts; `deck:` with nothing after it is
// syntax, and D5 keeps the last results while it is typed.
#[test]
fn texts_stay_text_by_t5() {
    let names = names();
    let items = |text: &str| match classify_text(text, &names) {
        TextReading::Items(items) => items,
        TextReading::Syntax => panic!("{text:?} holds syntax"),
    };
    let parsed = |text: &str| BuiltItem::Parsed {
        text: s(text),
        field_text: false,
    };
    assert_eq!(items("10:45"), vec![BuiltItem::Literal(s("10:45"))]);
    assert_eq!(items("a-b"), vec![parsed("a-b")]);
    assert_eq!(items("-xyz"), vec![parsed("-xyz")]);
    assert_eq!(items("a_b"), vec![parsed("a_b")]);
    assert_eq!(items("\"two words\""), vec![parsed("\"two words\"")]);
    // The literal finds the characters, not a field named 10; the
    // wildcard stays one.
    assert_eq!(build(&items("10:45")[0]).unwrap(), "10\\:45");
    assert_eq!(items(""), vec![]);
    assert_eq!(items(" \t\u{3000}"), vec![]);
    // A text that does not parse: its words, literally.
    assert_eq!(
        items("a (b"),
        vec![BuiltItem::Literal(s("a")), BuiltItem::Literal(s("(b"))]
    );
    for syntax in [
        "deck:", "tag:x", "a or b", "(a b)", "Back:x", "-Back:x", "*:x", "a -and b",
    ] {
        assert_eq!(classify_text(syntax, &names), TextReading::Syntax, "{syntax:?}");
    }
    assert!(matches!(
        classify_tail("deck:", 5),
        Ok(Tail::Incomplete {
            form: Incomplete::EmptyValue,
            ..
        })
    ));
}

// T8's commit: tokens after the query's own, step 4 counting them; text
// that stays text stays as typed. The live run text of syntax is the
// committed query's.
#[test]
fn commit_reads_syntax_after_the_tokens() {
    let names = table_names();
    let spanish = Value::Deck(DeckValue::of(&names, DeckId(5)).unwrap());
    let query = FilterQuery::new(vec![value(vec![spanish.clone()])], "deck:A word");
    let committed = query.commit(&names);
    assert_eq!(
        committed,
        FilterQuery::new(vec![value(vec![spanish.clone()]), Token::Advanced(s("deck:A"))], "word")
    );
    assert_eq!(query.run_text(&names).unwrap(), "deck:Spanish deck:A word");
    assert_eq!(committed.run_text(&names).unwrap(), "deck:Spanish deck:A word");
    let typed = FilterQuery::new(vec![value(vec![spanish.clone()])], "a  b");
    assert_eq!(typed.commit(&names), typed);
    let broken = FilterQuery::new(vec![], "(a");
    assert_eq!(broken.commit(&names), broken);
    let tagged = FilterQuery::new(vec![value(vec![spanish])], "tag:alpha is:new");
    assert_eq!(
        tagged.commit(&names).tokens[1..],
        [value(vec![tag("alpha")]), value(vec![Value::State(StateValue::New)])]
    );
}

// --- The fixture collection -------------------------------------------

/// A schema-18 copy holding the decks, note types, notes, cards and revlog
/// rows of this file; note 7 has no cards.
struct Fixture {
    _dir: tempfile::TempDir,
    db: OpenedCollection,
    notetypes: SearchNotetypes,
}

fn exec(db: &OpenedCollection, sql: &str, values: Vec<SqlValue>) {
    db.writer()
        .execute_raw(sql, &values)
        .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
}

fn int(v: i64) -> SqlValue {
    SqlValue::Integer(v)
}

fn text(v: &str) -> SqlValue {
    SqlValue::Text(v.to_owned())
}

/// A card's state columns.
#[derive(Clone, Copy)]
enum St {
    New,
    Learn,
    Review(i64),
    Suspended,
    BuriedManually,
    BuriedSibling,
}

/// (card, note, deck, home deck, ord, state, flags, days since added)
type CardRow = (i64, i64, i64, i64, i64, St, i64, i64);

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    let clock = FixedClock::new(1_894_701_600_000, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    let mut initializers = Initializers::new();
    initializers.register(meguri_search::sqlfns::register);
    let db = open_collection(&path, &OpenOptions::new(&clock, &random).initializers(initializers)).unwrap();
    for table in ["cards", "notes", "revlog", "decks", "notetypes", "templates", "fields"] {
        exec(&db, &format!("DELETE FROM {table}"), vec![]);
    }
    for (id, name, filtered) in DECKS {
        let kind = if filtered {
            SqlValue::Blob(vec![0x12, 0x00])
        } else {
            SqlValue::Blob(vec![0x0a, 0x02, 0x08, 0x01])
        };
        exec(
            &db,
            "INSERT INTO decks (id,name,mtime_secs,usn,common,kind) VALUES (?1,?2,0,0,X'',?3)",
            vec![int(id), text(&name.replace("::", "\u{1f}")), kind],
        );
    }
    for t in &TYPES {
        exec(
            &db,
            "INSERT INTO notetypes (id,name,mtime_secs,usn,config) VALUES (?1,?2,0,0,X'')",
            vec![int(t.id), text(t.name)],
        );
        for (ord, name) in t.templates.iter().enumerate() {
            exec(
                &db,
                "INSERT INTO templates (ntid,ord,name,mtime_secs,usn,config) VALUES (?1,?2,?3,0,0,X'')",
                vec![int(t.id), int(ord as i64), text(name)],
            );
        }
    }
    // (id, note type, tags, fields, days since edited)
    let notes: [(i64, i64, &str, &[&str], i64); 8] = [
        (1, 100, " alpha ", &["apple pear", "x", "10:45"], 0),
        (2, 100, " beta::child Gamma ", &["two words here", "", ""], 3),
        (3, 101, " leech ", &["日本", "a_b note"], 20),
        (4, 101, "", &["xyz", "pe-ar"], 40),
        (5, 102, " r\u{a0}s alpha ", &["Ünïcode text"], 0),
        (6, 100, " ünïcode leech ", &["a-b", "apple", "x y z"], 5),
        (7, 100, " beta ", &["", "x", ""], 2),
        (8, 101, " Gamma ", &["word", "more words"], 0),
    ];
    for (id, mid, tags, fields, edited) in notes {
        let sort = meguri_text::checksum::sort_text_of(fields, 0).into_owned();
        exec(
            &db,
            "INSERT INTO notes (id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) VALUES (?1,?1,?2,?3,0,?4,?5,?6,0,0,'')",
            vec![
                int(id),
                int(mid),
                int(NEXT_DAY - edited * DAY - 100),
                text(tags),
                text(&fields.join("\u{1f}")),
                text(&sort),
            ],
        );
    }
    // (card, note, deck, home deck, ord, state, flags, days since added)
    let cards: [CardRow; 13] = [
        (1, 1, 10, 0, 0, St::Review(12), 0, 0),
        (2, 1, 11, 0, 1, St::Review(10), 0, 50),
        (3, 2, 13, 0, 0, St::New, 0, 2),
        (4, 2, 18, 0, 1, St::Review(15), 0, 20),
        (5, 3, 19, 0, 0, St::Learn, 0, 50),
        (6, 3, 14, 0, 1, St::Suspended, 2, 50),
        (7, 4, 16, 0, 0, St::BuriedManually, 0, 50),
        (8, 4, 17, 0, 1, St::Review(30), 3, 50),
        (9, 5, 20, 0, 0, St::New, 1, 0),
        (10, 6, 21, 10, 0, St::Review(11), 7, 50),
        (11, 6, 1, 0, 1, St::New, 2, 6),
        (12, 8, 15, 0, 0, St::Review(13), 5, 50),
        (13, 8, 22, 0, 1, St::BuriedSibling, 0, 50),
    ];
    for (k, nid, did, odid, ord, state, flags, added) in cards {
        let (kind, queue, due) = match state {
            St::New => (0, 0, k),
            St::Learn => (1, 1, NOW - 10),
            St::Review(day) => (2, 2, day),
            St::Suspended => (2, -1, 5),
            St::BuriedManually => (0, -2, k),
            St::BuriedSibling => (2, -3, 4),
        };
        exec(
            &db,
            "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data)
             VALUES (?1,?2,?3,?4,0,0,?5,?6,?7,1,2500,1,0,0,?8,?9,?10,'')",
            vec![
                int(card_id(k, added)),
                int(nid),
                int(did),
                int(ord),
                int(kind),
                int(queue),
                int(due),
                int(if odid == 0 { 0 } else { due }),
                int(odid),
                int(flags),
            ],
        );
    }
    // (card, days ago, button)
    for (k, (card, ago, ease)) in [(1, 0, 3), (2, 3, 1), (8, 20, 2), (12, 0, 4), (6, 9, 3)]
        .into_iter()
        .enumerate()
    {
        let added = cards.iter().find(|c| c.0 == card).map_or(0, |c| c.7);
        exec(
            &db,
            "INSERT INTO revlog (id,cid,usn,ease,ivl,lastIvl,factor,time,type) VALUES (?1,?2,0,?3,1,0,2500,1000,1)",
            vec![
                int((NEXT_DAY - ago * DAY - 500) * 1000 + k as i64),
                int(card_id(card, added)),
                int(ease),
            ],
        );
    }
    Fixture {
        _dir: dir,
        db,
        notetypes: notetypes(),
    }
}

/// A card id is its creation time in milliseconds: `added` days before the
/// next day boundary, and the card's number.
fn card_id(k: i64, added: i64) -> i64 {
    (NEXT_DAY - added * DAY - 1000) * 1000 + k
}

impl Fixture {
    fn ids(&self, text: &str, mode: SearchMode) -> BTreeSet<i64> {
        let query = parse(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"));
        let plan = Plan::build(&query, mode, &context(), &self.notetypes).unwrap_or_else(|e| panic!("{text:?}: {e:?}"));
        let params = plan
            .params()
            .iter()
            .map(|p| match p {
                Param::Integer(v) => SqlValue::Integer(*v),
                Param::Real(v) => SqlValue::Real(*v),
                Param::Text(v) => SqlValue::Text(v.clone()),
            })
            .collect::<Vec<_>>();
        self.db
            .writer()
            .execute_raw(plan.sql(), &params)
            .unwrap_or_else(|e| panic!("{text:?}: {e:?} in {}", plan.sql()))
            .rows
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                ref other => panic!("noninteger id: {other:?}"),
            })
            .collect()
    }
}

// T16 (changed 2026-11-21, U88a review): the empty query, a text of
// White_Space only and `deck:*` read by T7 each run the empty text, which
// in Notes finds the note without cards; `deck:*` itself does not (J8).
#[test]
fn the_whole_collection_runs_the_empty_text_in_notes() {
    let f = fixture();
    let names = names();
    let queries = [
        FilterQuery::default(),
        FilterQuery::new(vec![], " \t\u{3000} "),
        read_text("deck:*", &names),
    ];
    for query in queries {
        let run = query.run_text(&names).unwrap();
        assert_eq!(run, "", "{query:?}");
        let notes = f.ids(&run, SearchMode::Notes);
        assert!(notes.contains(&7), "{query:?}: {notes:?}");
        assert_eq!(notes.len(), 8);
    }
    assert!(!f.ids("deck:*", SearchMode::Notes).contains(&7));
}

// --- T7.1's round trip ---------------------------------------------------

const TEXT_WORDS: [&str; 9] = [
    "apple",
    "pe*r",
    "-xyz",
    "a_b",
    "\"two words\"",
    "10:45",
    "ünï",
    "nofield:x",
    "a-b",
];

const CONTAINS: [&str; 7] = ["apple", "x", "日本", "a_b", "x y", "10:45", "ap*"];

const FIELDS: [&str; 6] = ["Front", "Back", "Extra", "Word", "Notes Field", "Text"];

const TEMPLATES: [&str; 5] = ["Card 1", "Card 2", "Plimbo", "Varnex", "Ünit Card"];

/// The values of one generated kind.
fn pool(names: &NameIndex, kind: usize, field_name: &str) -> Vec<Value> {
    let periods = [Period::Today, Period::Last7Days, Period::Last30Days];
    match kind {
        0 => {
            let mut values: Vec<Value> = names
                .decks()
                .iter()
                .map(|d| Value::Deck(DeckValue::of(names, d.id).unwrap()))
                .collect();
            values.push(Value::CurrentDeck);
            values
        }
        1 => {
            let mut values: Vec<Value> = TAGS.iter().map(|t| tag(t)).collect();
            values.push(Value::Untagged);
            values
        }
        2 => names
            .notetypes()
            .iter()
            .map(|n| Value::Notetype(NotetypeValue::of(n)))
            .collect(),
        3 => TEMPLATES.iter().map(|t| Value::CardType(s(t))).collect(),
        4 => [
            StateValue::New,
            StateValue::Learning,
            StateValue::Review,
            StateValue::Suspended,
            StateValue::Buried,
            StateValue::Leech,
        ]
        .map(Value::State)
        .to_vec(),
        5 => {
            let mut values: Vec<Value> = (1..=7).map(Value::Flag).collect();
            values.extend([Value::NoFlag, Value::AnyFlag]);
            values
        }
        6 => [DueValue::Today, DueValue::Overdue, DueValue::Next7Days]
            .map(Value::Due)
            .to_vec(),
        7 => periods.map(Value::Added).to_vec(),
        8 => periods.map(Value::Edited).to_vec(),
        9 => periods.map(Value::Studied).to_vec(),
        10 => {
            let mut values: Vec<Value> = CONTAINS
                .iter()
                .map(|t| field(field_name, FieldTest::Contains(s(t))))
                .collect();
            values.extend([
                field(field_name, FieldTest::Empty),
                field(field_name, FieldTest::HasContent),
            ]);
            values
        }
        _ => unreachable!(),
    }
}

/// A random query of T3 values and text that stays text: distinct kinds,
/// one to four values each, is or is not where T10 allows it.
fn query(seq: &mut Seq, names: &NameIndex) -> FilterQuery {
    let mut kinds: Vec<Kind> = Vec::new();
    let mut tokens = Vec::new();
    for _ in 0..seq.below(5) {
        let k = seq.below(13);
        let values = match k {
            11 => {
                let ids = (0..1 + seq.below(3))
                    .map(|j| NoteId(1 + j as i64 * 3 + seq.below(3) as i64))
                    .collect();
                vec![Value::Duplicates(ids)]
            }
            12 => vec![Value::StudiedOn(vec![CardId(card_id(1 + seq.below(13) as i64, 50))])],
            _ => {
                let field_name = FIELDS[seq.below(FIELDS.len())];
                let mut pool = pool(names, k, field_name);
                let mut values = Vec::new();
                for _ in 0..1 + seq.below(4) {
                    if pool.is_empty() {
                        break;
                    }
                    values.push(pool.remove(seq.below(pool.len())));
                }
                values
            }
        };
        let kind = values[0].kind();
        if kinds.iter().any(|seen| seen.same(&kind)) {
            continue;
        }
        kinds.push(kind);
        let fixed = values
            .iter()
            .any(|v| matches!(v, Value::NoFlag | Value::AnyFlag | Value::Untagged));
        let negated = !fixed && seq.below(2) == 0;
        tokens.push(Token::Value(ValueToken::new(values, negated)));
    }
    let words: Vec<&str> = (0..seq.below(4)).map(|_| seq.pick(&TEXT_WORDS)).collect();
    FilterQuery::new(tokens, words.join(" "))
}

/// The item of a token, written as a whole search: an advanced token's text.
fn whole_text(token: &Token) -> String {
    let probe = FilterQuery::new(vec![token.clone()], "");
    probe.run_text(&names()).unwrap()
}

/// T7.1's exceptions: what each token comes back as, then step 4.
fn expected_back(query: &FilterQuery) -> Vec<Token> {
    let mut kinds: Vec<Kind> = Vec::new();
    let mut out = Vec::new();
    for token in &query.tokens {
        let Token::Value(value_token) = token else {
            out.push(token.clone());
            continue;
        };
        let by_id = value_token
            .values
            .iter()
            .any(|v| matches!(v.item(), Some(BuiltItem::Ids { .. } | BuiltItem::NotetypeId(_))));
        let back = if by_id {
            Token::Advanced(whole_text(token))
        } else if value_token.values == [tag("leech")] {
            Token::Value(ValueToken::new(
                vec![Value::State(StateValue::Leech)],
                value_token.negated,
            ))
        } else {
            token.clone()
        };
        let back = match back.kind() {
            Some(kind) if kinds.iter().any(|seen| seen.same(&kind)) => Token::Advanced(whole_text(token)),
            Some(kind) => {
                kinds.push(kind);
                back
            }
            None => back,
        };
        out.push(back);
    }
    out
}

/// The text's items, as written (texts compare by what they run).
fn text_items(text: &str, names: &NameIndex) -> Vec<String> {
    match classify_text(text, names) {
        TextReading::Items(items) => items.iter().map(|item| build(item).unwrap()).collect(),
        TextReading::Syntax => panic!("{text:?} holds syntax"),
    }
}

/// T2 in Cards mode, from each value's own cards: OR within a token, the
/// negation of the OR for is not, AND across tokens and the text's items.
fn combined(
    f: &Fixture,
    query: &FilterQuery,
    names: &NameIndex,
    cache: &mut HashMap<String, BTreeSet<i64>>,
) -> BTreeSet<i64> {
    let mut cards = |text: &str| -> BTreeSet<i64> {
        cache
            .entry(text.to_owned())
            .or_insert_with(|| f.ids(text, SearchMode::Cards))
            .clone()
    };
    let all = cards("");
    let mut found = all.clone();
    for token in &query.tokens {
        let mine = match token {
            Token::Advanced(text) => cards(text),
            Token::Value(value_token) => {
                let any: BTreeSet<i64> = value_token
                    .values
                    .iter()
                    .flat_map(|v| cards(&build(&v.item().unwrap()).unwrap()))
                    .collect();
                if value_token.negated {
                    all.difference(&any).copied().collect()
                } else {
                    any
                }
            }
        };
        found = found.intersection(&mine).copied().collect();
    }
    for item in text_items(&query.text, names) {
        found = found.intersection(&cards(&item)).copied().collect();
    }
    found
}

// T16's property: random queries of T3 values (every kind, is and is not,
// one to four values, Unicode names, names J3.5 refuses) and plain text.
// T7.1: reading the run text gives the same query back, except the listed
// exceptions; the run text returns the same cards as T2 combines from each
// value's cards, and its reading's run text the same ids in both modes,
// all through the product's term SQL on the fixture collection.
#[test]
fn queries_round_trip_through_their_run_text() {
    let f = fixture();
    let names = names();
    let mut cache = HashMap::new();
    let mut seq = Seq(0x0260_70CE);
    let (mut exact, mut excepted, mut negated, mut or_tokens) = (0, 0, 0, 0);
    let (mut some_cards, mut some_but_not_all) = (0, 0);
    let all_cards = f.ids("", SearchMode::Cards).len();
    for _ in 0..1_500 {
        let query = query(&mut seq, &names);
        let run = query.run_text(&names).unwrap_or_else(|e| panic!("{query:?}: {e:?}"));
        let back = read_text(&run, &names);
        let expected = expected_back(&query);
        assert_eq!(back.tokens, expected, "{query:?} -> {run:?}");
        assert_eq!(
            text_items(&back.text, &names),
            text_items(&query.text, &names),
            "{query:?} -> {run:?}"
        );
        if expected == query.tokens {
            exact += 1;
        } else {
            excepted += 1;
        }
        negated += query
            .tokens
            .iter()
            .filter(|t| matches!(t, Token::Value(v) if v.negated))
            .count();
        or_tokens += query
            .tokens
            .iter()
            .filter(|t| matches!(t, Token::Value(v) if v.values.len() > 1))
            .count();
        let again = back.run_text(&names).unwrap();
        for mode in [SearchMode::Cards, SearchMode::Notes] {
            assert_eq!(
                f.ids(&run, mode),
                f.ids(&again, mode),
                "{run:?} / {again:?} in {mode:?}"
            );
        }
        let cards = f.ids(&run, SearchMode::Cards);
        assert_eq!(cards, combined(&f, &query, &names, &mut cache), "{query:?} -> {run:?}");
        some_cards += usize::from(!cards.is_empty());
        some_but_not_all += usize::from(!cards.is_empty() && cards.len() < all_cards);
    }
    // The fixture tells the queries apart: many find some cards, not all.
    assert!(
        some_cards > 300 && some_but_not_all > 200,
        "{some_cards}, {some_but_not_all}"
    );
    assert!(exact > 500 && excepted > 100, "exact {exact}, exceptions {excepted}");
    assert!(
        negated > 300 && or_tokens > 300,
        "negated {negated}, OR tokens {or_tokens}"
    );
}

// The built items the property test's values make all build, and none is
// the whole collection (Browse never builds it, T4 step 4).
#[test]
fn no_value_builds_the_whole_collection() {
    let names = names();
    for kind in 0..=10 {
        for field_name in FIELDS {
            for value in pool(&names, kind, field_name) {
                let item = value.item().unwrap_or_else(|| panic!("{value:?}"));
                let query = build_query(&item).unwrap();
                assert!(
                    !query
                        .nodes()
                        .iter()
                        .any(|n| n.kind == meguri_search::NodeKind::Term(meguri_search::Term::WholeCollection)),
                    "{value:?}"
                );
            }
        }
    }
}
