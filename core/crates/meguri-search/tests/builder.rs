//! Built searches (`specs/search/join-and-replace.md` J3 to J5, J8 to J11;
//! `specs/search/filter-tokens.md` T3.1): J9's `build` examples, every
//! refusal of J3, J10's differences from Anki, the items with no oracle
//! scenario (J11), AE60's canonical text, nesting without recursion, and
//! J8's round-trip property test. The oracle goldens of
//! `conformance/goldens/search/builder/` run through the testkit's
//! `search_build` adapter in T0.

use meguri_model::ids::NotetypeId as Ntid;
use meguri_search::ast::{CardState, IdKind, NodeKind};
use meguri_search::builder::{BuilderError, BuiltItem, ErrorSource, InvalidItem, Joiner, build, build_query};
use meguri_search::plan::{BuildError, Plan, SearchMode};
use meguri_search::{canonical_text, parse};

use crate::canonical_roundtrip::{
    Seq, collection, context, ids, is_exception, is_group_of_group, meaning_root, same_meaning, sequence, stand_in,
};

use BuiltItem::*;

fn text(item: &BuiltItem) -> String {
    build(item).unwrap_or_else(|e| panic!("{item:?}: {e:?}"))
}

fn refused(item: &BuiltItem) -> InvalidItem {
    match build(item) {
        Err(BuilderError::InvalidItem(reason)) => reason,
        other => panic!("{item:?}: {other:?}"),
    }
}

fn s(text: &str) -> String {
    text.to_owned()
}

fn group(joiner: Joiner, items: Vec<BuiltItem>) -> BuiltItem {
    Group { joiner, items }
}

fn not(item: BuiltItem) -> BuiltItem {
    Negated(Box::new(item))
}

fn parsed(text: &str) -> BuiltItem {
    Parsed {
        text: s(text),
        field_text: false,
    }
}

// J9, the `build` rows.
#[test]
fn the_spec_examples_build_their_text() {
    let cases = [
        (Deck(s("French::Verbs")), "deck:French::Verbs"),
        (Deck(s("My Deck")), "\"deck:My Deck\""),
        (Deck(s("a_b*")), "deck:a\\_b\\*"),
        (Deck(s("C++ (old)")), "\"deck:C++ (old)\""),
        (Deck(s("say \"hi\"")), "\"deck:say \\\"hi\\\"\""),
        (Deck(s("x\\y")), "deck:x\\\\y"),
        (Tag(s("x y")), "\"tag:x y\""),
        (Notetype(s("Basic (and reversed card)")), "\"note:Basic (and reversed card)\""),
        (FieldPresent(s("Extra Info")), "\"Extra Info:_*\""),
        (FieldPresent(s("my:field")), "my\\:field:_*"),
        (FieldPresent(s("myfield")), "myfield:_*"),
        (Literal(s("and")), "\"and\""),
        (Literal(s("-x")), "\"-x\""),
        (Literal(s("3:30")), "3\\:30"),
        (Literal(s("50%_off")), "50%\\_off"),
        (group(Joiner::And, vec![Notetype(s("Basic")), Template(0)]), "note:Basic card:1"),
        (
            group(Joiner::Or, vec![State(CardState::New), State(CardState::Learn)]),
            "is:new OR is:learn",
        ),
        (
            group(Joiner::And, vec![State(CardState::Due), not(DueOn(0))]),
            "is:due -prop:due=0",
        ),
        (AnyFlag, "-flag:0"),
        (NoFlag, "flag:0"),
        (Flag(1), "flag:1"),
        (Added(0), "added:1"),
        (
            Answered {
                days: 1,
                button: Some(1),
            },
            "rated:1:1",
        ),
        (Rescheduled(0), "resched:0"),
        (
            Ids {
                kind: IdKind::Notes,
                ids: vec![5, 3],
            },
            "nid:5,3",
        ),
        (
            Duplicates {
                notetype: Ntid(1_600_000_000_000),
                text: s("a\\b \"c\""),
            },
            "\"dupe:1600000000000,a\\\\b \\\"c\\\"\"",
        ),
    ];
    for (item, expected) in cases {
        assert_eq!(text(&item), expected, "{item:?}");
    }
}

// J11: the items Anki's structured search has no counterpart for.
#[test]
fn items_without_an_oracle_scenario_build_their_text() {
    let cases = [
        (NotetypeId(Ntid(1_600_000_000_000)), "mid:1600000000000"),
        (NotetypeId(Ntid(-5)), "mid:-5"),
        (
            Ids {
                kind: IdKind::Cards,
                ids: vec![7, 0, 7],
            },
            "cid:7,0,7",
        ),
        (
            Ids {
                kind: IdKind::Decks,
                ids: vec![1, 1_600_000_000_000],
            },
            "did:1,1600000000000",
        ),
        (State(CardState::BuriedManually), "is:buried-manually"),
        (State(CardState::BuriedSibling), "is:buried-sibling"),
        (CurrentDeck, "deck:current"),
        (AnyDeck, "deck:_*"),
        (AnyNotetype, "note:_*"),
        (WholeCollection, "deck:*"),
        (Untagged, "-tag:_*"),
        (DueWithin(-1), "prop:due<=-1"),
        (Template(65534), "card:65535"),
        (Introduced(0), "introduced:1"),
        (Edited(4_294_967_295), "edited:4294967295"),
    ];
    for (item, expected) in cases {
        assert_eq!(text(&item), expected, "{item:?}");
    }
}

// T3.1: "field contains" and "field empty".
#[test]
fn the_two_field_token_items_build_their_text() {
    let contains = |field: &str, t: &str| FieldContains {
        field: s(field),
        text: s(t),
    };
    assert_eq!(text(&contains("Back", "apple")), "Back:*apple*");
    assert_eq!(text(&contains("Notes Field", "x")), "\"Notes Field:*x*\"");
    assert_eq!(text(&contains("Back", "a_b*")), "Back:*a\\_b\\**");
    assert_eq!(text(&contains("Back", "re:x")), "Back:*re:x*");
    assert_eq!(text(&FieldEmpty(s("Extra"))), "Extra:");
    assert_eq!(text(&FieldEmpty(s("my:field"))), "my\\:field:");
    // As field present; and an empty text.
    assert_eq!(refused(&contains("", "x")), InvalidItem::EmptyName);
    assert_eq!(refused(&contains("Deck", "x")), InvalidItem::ReservedFieldName);
    assert_eq!(refused(&contains("Back", "")), InvalidItem::EmptyText);
    assert_eq!(refused(&FieldEmpty(s("tag"))), InvalidItem::ReservedFieldName);
    assert_eq!(refused(&FieldEmpty(s(""))), InvalidItem::EmptyName);
    // They read back as the field terms J4 wrote.
    for item in [contains("Back", "apple"), FieldEmpty(s("Extra"))] {
        let o = text(&item);
        assert_eq!(canonical_text(&o).unwrap(), o);
    }
}

// J10: where Meguri writes another text than Anki, by design.
#[test]
fn the_deliberate_differences_from_anki() {
    let cases = [
        (Deck(s("current")), "deck:Current"),
        (Deck(s("filtered")), "deck:Filtered"),
        (Tag(s("none")), "tag:None"),
        // Only the exact lower-case words are special.
        (Deck(s("Current")), "deck:Current"),
        (Deck(s("current::x")), "deck:current::x"),
        (Deck(s("_*")), "deck:\\_\\*"),
        (Tag(s("_*")), "tag:\\_\\*"),
        (Notetype(s("_*")), "note:\\_\\*"),
        (FieldPresent(s("_*")), "\\_\\*:_*"),
        // J3.2.
        (Tag(s("r\u{a0}s")), "\"tag:re:^r\\x{a0}s(::|$)\""),
        (Tag(s("a.b\u{3000}(c)")), "\"tag:re:^a\\.b\\x{3000}\\(c\\)(::|$)\""),
        (Tag(s("x y\u{2028}")), "\"tag:re:^x\\x{20}y\\x{2028}(::|$)\""),
        // E's `_` for a field name, field text and literal.
        (FieldPresent(s("a\u{a0}b")), "a_b:_*"),
        (
            FieldText {
                field: s("f"),
                text: s("x\ty\u{1f}"),
            },
            "f:x_y_",
        ),
        (Literal(s("a\u{85}b")), "a_b"),
        (Added(0), "added:1"),
        (
            Answered {
                days: 0,
                button: None,
            },
            "rated:1",
        ),
    ];
    for (item, expected) in cases {
        assert_eq!(text(&item), expected, "{item:?}");
    }
    let tag = parse(&text(&Tag(s("a\"b\u{a0}\\")))).unwrap();
    let NodeKind::Term(term) = &tag.node(tag.root()).kind else {
        panic!("one term")
    };
    assert_eq!(
        format!("{term:?}"),
        r#"Tag { mode: Regex, text: "^a\"b\\x{a0}\\\\(::|$)" }"#
    );
}

// J3's refusals, one per reason, and where they stand in an item.
#[test]
fn refused_items_give_their_reason() {
    let cases = [
        (group(Joiner::And, vec![]), InvalidItem::EmptyGroup),
        (group(Joiner::Or, vec![Flag(1), group(Joiner::And, vec![])]), InvalidItem::EmptyGroup),
        (not(group(Joiner::And, vec![])), InvalidItem::EmptyGroup),
        (FieldPresent(s("deck")), InvalidItem::ReservedFieldName),
        (FieldPresent(s("Has-CD")), InvalidItem::ReservedFieldName),
        (
            FieldText {
                field: s("IS"),
                text: s("x"),
            },
            InvalidItem::ReservedFieldName,
        ),
        (Deck(s("a\u{a0}b")), InvalidItem::UnrepresentableName),
        (Deck(s("a::\u{1}b")), InvalidItem::UnrepresentableName),
        (Deck(s("a:: b")), InvalidItem::UnrepresentableName),
        (Deck(s("a ::b")), InvalidItem::UnrepresentableName),
        (Deck(s("a::::b")), InvalidItem::UnrepresentableName),
        (Deck(s("::a")), InvalidItem::UnrepresentableName),
        (Deck(s("a:::b")), InvalidItem::UnrepresentableName),
        (Deck(s("a:")), InvalidItem::UnrepresentableName),
        (Deck(s("e\u{301}")), InvalidItem::UnrepresentableName),
        (Notetype(s("a\u{a0}b")), InvalidItem::UnrepresentableName),
        (Notetype(s("a\u{1c}")), InvalidItem::UnrepresentableName),
        (Notetype(s("e\u{301}")), InvalidItem::UnrepresentableName),
        (Deck(s("")), InvalidItem::EmptyName),
        (Tag(s("")), InvalidItem::EmptyName),
        (Notetype(s("")), InvalidItem::EmptyName),
        (FieldPresent(s("")), InvalidItem::EmptyName),
        (Literal(s("")), InvalidItem::EmptyText),
        (Template(65535), InvalidItem::OrdinalOutOfRange),
        (Flag(0), InvalidItem::FlagOutOfRange),
        (Flag(8), InvalidItem::FlagOutOfRange),
        (Added(4_294_967_296), InvalidItem::DaysOutOfRange),
        (Rescheduled(u64::MAX), InvalidItem::DaysOutOfRange),
        (
            Answered {
                days: 1,
                button: Some(5),
            },
            InvalidItem::ButtonOutOfRange,
        ),
        (
            Answered {
                days: 1,
                button: Some(0),
            },
            InvalidItem::ButtonOutOfRange,
        ),
        (DueOn(2_147_483_648), InvalidItem::DayOutOfRange),
        (DueWithin(-2_147_483_649), InvalidItem::DayOutOfRange),
        (
            Ids {
                kind: IdKind::Notes,
                ids: vec![],
            },
            InvalidItem::EmptyIdList,
        ),
        (
            Ids {
                kind: IdKind::Decks,
                ids: vec![1, -1],
            },
            InvalidItem::NegativeId,
        ),
        (
            Duplicates {
                notetype: Ntid(1),
                text: s("a\u{a0}b"),
            },
            InvalidItem::UnrepresentableText,
        ),
        (
            Duplicates {
                notetype: Ntid(1),
                text: s("a\u{1f}b"),
            },
            InvalidItem::UnrepresentableText,
        ),
    ];
    for (item, reason) in cases {
        assert_eq!(refused(&item), reason, "{item:?}");
    }
    // A deck whose levels J3.5 accepts: inner spaces, `:` inside, non-ASCII.
    assert_eq!(text(&Deck(s("a b::c:d::日本"))), "\"deck:a b::c:d::日本\"");
    assert_eq!(InvalidItem::EmptyGroup.reason(), "empty_group");
    assert_eq!(InvalidItem::ReservedFieldName.reason(), "reserved_field_name");
    assert_eq!(InvalidItem::UnrepresentableName.reason(), "unrepresentable_name");
}

// J1: the first parse error in item order, with its source path; J2 for
// field text only.
#[test]
fn parse_errors_name_their_item() {
    let error = |item: &BuiltItem| match build(item) {
        Err(BuilderError::Parse { source, error }) => (source, error.code(), error.range()),
        other => panic!("{item:?}: {other:?}"),
    };
    assert_eq!(error(&parsed("(a")), (ErrorSource::Item(vec![]), "unclosed_group", 0..1));
    let item = group(
        Joiner::And,
        vec![
            Deck(s("X")),
            not(group(Joiner::Or, vec![Flag(1), parsed("is:foo")])),
            parsed("\"x"),
        ],
    );
    assert_eq!(error(&item), (ErrorSource::Item(vec![1, 0, 1]), "invalid_state", 0..6));
    // A refusal met first in item order wins over a later parse error.
    let item = group(Joiner::And, vec![Flag(9), parsed("(")]);
    assert_eq!(refused(&item), InvalidItem::FlagOutOfRange);
    let item = group(Joiner::And, vec![parsed("("), Flag(9)]);
    assert_eq!(error(&item).1, "unclosed_group");
    // J2: U+001C-U+001F are spaces in field text, term characters otherwise.
    let separated = "a\u{1f}b";
    assert_eq!(
        text(&Parsed {
            text: s(separated),
            field_text: true
        }),
        "a b"
    );
    assert_eq!(text(&parsed(separated)), separated);
}

// J4, J5 on groups, negations and parsed text.
#[test]
fn groups_negations_and_parsed_text() {
    let cases = [
        (group(Joiner::And, vec![Deck(s("X"))]), "deck:X"),
        (group(Joiner::Or, vec![group(Joiner::And, vec![Flag(1)])]), "flag:1"),
        (
            group(Joiner::And, vec![Deck(s("X")), group(Joiner::And, vec![Flag(1), Flag(2)])]),
            "deck:X (flag:1 flag:2)",
        ),
        (
            group(Joiner::Or, vec![Deck(s("X")), group(Joiner::And, vec![Flag(1), Flag(2)])]),
            "deck:X OR (flag:1 flag:2)",
        ),
        (not(not(Deck(s("X")))), "deck:X"),
        (not(AnyFlag), "flag:0"),
        (not(Untagged), "tag:_*"),
        (not(group(Joiner::And, vec![not(Deck(s("X")))])), "deck:X"),
        (not(parsed("-(a b)")), "a b"),
        (not(parsed("a b")), "-(a b)"),
        (parsed(""), "deck:*"),
        (parsed("((a b))"), "(a b)"),
        (group(Joiner::And, vec![parsed("((a b))"), Literal(s("c"))]), "((a b)) c"),
        (group(Joiner::And, vec![parsed("a or b"), Deck(s("X"))]), "(a OR b) deck:X"),
        (group(Joiner::Or, vec![WholeCollection, Tag(s("x"))]), "deck:* OR tag:x"),
    ];
    for (item, expected) in cases {
        assert_eq!(text(&item), expected, "{item:?}");
    }
}

// J11: AE60's joined Stats text parses, and its canonical text is the
// golden of `stats/browse-queries/parse-all`.
#[test]
fn the_ae60_text_parses_to_its_golden() {
    let joined = r#"(deck:"Japanese") AND ("is:review" AND -"is:learn") AND "prop:ivl<21" AND -("is:buried" OR "is:suspended")"#;
    assert_eq!(
        canonical_text(joined).unwrap(),
        "(deck:Japanese) (is:review -is:learn) prop:ivl<21 -(is:buried OR is:suspended)"
    );
}

// E6: no nesting depth exhausts the stack, building or dropping.
#[test]
fn deep_items_build_and_drop_without_recursion() {
    let mut item = Deck(s("X"));
    for _ in 0..100_001 {
        item = not(item);
    }
    assert_eq!(text(&item), "-deck:X");
    let mut item = Flag(1);
    for _ in 0..50_000 {
        item = group(Joiner::Or, vec![Flag(2), item]);
    }
    let o = text(&item);
    assert!(o.starts_with("flag:2 OR (flag:2 OR ("), "{}", &o[..40]);
    let inner = o.trim_end_matches(')');
    assert!(inner.ends_with("(flag:2 OR flag:1"), "{}", &inner[inner.len() - 40..]);
    assert_eq!(o.len() - inner.len(), 49_999);
    assert_eq!(canonical_text(&o).unwrap(), o);
    let mut item = Flag(3);
    for _ in 0..100_000 {
        item = group(Joiner::And, vec![item]);
    }
    assert_eq!(text(&item), "flag:3");
    let deep = format!("{}a b{}", "(".repeat(20_000), ")".repeat(20_000));
    assert!(text(&parsed(&deep)).starts_with("((("));
}

const NAMES: [&str; 24] = [
    "French::Verbs",
    "My Deck",
    "a_b*",
    "C++ (old)",
    "say \"hi\"",
    "x\\y",
    "and",
    "-x",
    "3:30",
    "日本",
    "é",
    "_*",
    "x:y",
    "a::b",
    "(",
    "re:x",
    "nc:y",
    "OR",
    "a\\\"b",
    "current",
    "none",
    "Extra Info",
    "a\u{a0}b",
    "--",
];

fn name(seq: &mut Seq) -> String {
    seq.pick(&NAMES).to_owned()
}

/// One random item, `depth` levels of groups and negations at most.
fn item(seq: &mut Seq, depth: u32) -> BuiltItem {
    let states = CardState::ALL;
    match seq.below(if depth == 0 { 30 } else { 34 }) {
        0 => Deck(name(seq)),
        1 => Tag(name(seq)),
        2 => Notetype(name(seq)),
        3 => FieldPresent(name(seq)),
        4 => FieldText {
            field: name(seq),
            text: name(seq),
        },
        5 => FieldContains {
            field: name(seq),
            text: name(seq),
        },
        6 => FieldEmpty(name(seq)),
        7 => Literal(name(seq)),
        8 => Flag(1 + seq.below(7) as u8),
        // Not the whole collection: its `deck:*` reads back as a deck term
        // the stand-in compilers do not equate with it (Q2's meaning is
        // checked by the example tests).
        9 => [NoFlag, AnyFlag, AnyTag, Untagged, AnyDeck, CurrentDeck, AnyNotetype]
            .into_iter()
            .nth(seq.below(7))
            .unwrap_or(NoFlag),
        10 => State(states[seq.below(states.len())]),
        11 => Added(seq.below(400) as u64),
        12 => Edited(seq.below(400) as u64),
        13 => Introduced(seq.below(400) as u64),
        14 => Answered {
            days: seq.below(40) as u64,
            button: [None, Some(1), Some(4)][seq.below(3)],
        },
        15 => Rescheduled(seq.below(40) as u64),
        16 => DueOn(seq.below(60) as i64 - 30),
        17 => DueWithin(seq.below(60) as i64 - 30),
        18 => Ids {
            kind: [IdKind::Notes, IdKind::Cards, IdKind::Decks][seq.below(3)],
            ids: (0..=seq.below(3)).map(|k| (k * 7 + seq.below(9)) as i64).collect(),
        },
        19 => Duplicates {
            notetype: Ntid(seq.below(5) as i64),
            text: name(seq),
        },
        20 => NotetypeId(Ntid(seq.below(9) as i64 - 4)),
        21 => Template(seq.below(5) as u32),
        22..=29 => Parsed {
            text: sequence(seq, 2),
            field_text: seq.below(2) == 0,
        },
        30 | 31 => group(
            [Joiner::And, Joiner::Or][seq.below(2)],
            (0..1 + seq.below(3)).map(|_| item(seq, depth - 1)).collect(),
        ),
        _ => not(item(seq, depth - 1)),
    }
}

/// Whether a parsed-text item anywhere in `item` holds one of F7's first
/// three shapes (J8 excludes them).
fn holds_an_exception(item: &BuiltItem) -> bool {
    let mut stack = vec![item];
    while let Some(item) = stack.pop() {
        match item {
            Parsed { text, field_text } => {
                let read = if *field_text {
                    meguri_search::builder::search_field_text(text).into_owned()
                } else {
                    text.clone()
                };
                if parse(&read).is_ok_and(|q| is_exception(&q)) {
                    return true;
                }
            }
            Group { items, .. } => stack.extend(items),
            Negated(child) => stack.push(child),
            _ => {}
        }
    }
    false
}

// J8: every output o of `build` is canonical text (canonical(o) = o), and o
// means what its item means: it reads back as the same query and, on the
// fixture collection, finds the same ids (through the stand-in term
// compilers of `canonical_roundtrip.rs`, as F7's tests do). F7's first three
// shapes in parsed text are excluded; the fourth is checked for meaning only.
#[test]
fn built_searches_round_trip() {
    let conn = collection();
    let mut seq = Seq(0x00B0_1D3E);
    let (mut checked, mut refusals, mut fourth_shape, mut parsed_checked) = (0, 0, 0, 0);
    let mut attempts = 0;
    while checked < 5_000 {
        attempts += 1;
        assert!(attempts < 100_000, "the generator yields too few items that build");
        let item = item(&mut seq, 3);
        let query = match build_query(&item) {
            Ok(query) => query,
            Err(BuilderError::InvalidItem(_)) => {
                refusals += 1;
                continue;
            }
            Err(BuilderError::Parse { .. }) => continue,
        };
        // F7's first three shapes; and the whole collection, whose `deck:*`
        // reads back as a deck term (Q2) that the stand-in compilers do not
        // equate with it: the example tests check that text.
        let whole = query
            .nodes()
            .iter()
            .any(|node| node.kind == NodeKind::Term(meguri_search::Term::WholeCollection));
        if holds_an_exception(&item) || is_exception(&query) || whole {
            continue;
        }
        // A parsed-text item means what its text means, independently of
        // the builder: the same query as `grammar.md` reads from the text.
        if let Parsed { text, field_text } = &item {
            let read = if *field_text {
                meguri_search::builder::search_field_text(text).into_owned()
            } else {
                text.clone()
            };
            let original = parse(&read).unwrap();
            assert!(
                same_meaning(&original, meaning_root(&original), &query, meaning_root(&query)),
                "{item:?} builds another query"
            );
            parsed_checked += 1;
        }
        let o = query.canonical_text();
        assert_eq!(build(&item).as_deref(), Ok(o.as_str()));
        let again = parse(&o).unwrap_or_else(|e| panic!("{item:?} -> {o:?}: {e:?}"));
        assert!(
            same_meaning(&query, meaning_root(&query), &again, again.root()),
            "{item:?} -> {o:?} reads differently"
        );
        if is_group_of_group(&query) {
            fourth_shape += 1;
        } else {
            assert_eq!(again.canonical_text(), o, "{item:?}");
        }
        for mode in [SearchMode::Cards, SearchMode::Notes] {
            let first = Plan::build_with(&query, mode, &context(), &mut stand_in);
            let second = Plan::build_with(&again, mode, &context(), &mut stand_in);
            match (first, second) {
                (Ok(first), Ok(second)) => assert_eq!(ids(&conn, &first), ids(&conn, &second), "{o:?} in {mode:?}"),
                (Err(BuildError::Search(a)), Err(BuildError::Search(b))) => assert_eq!(a.code(), b.code(), "{o:?}"),
                (a, b) => panic!("{o:?} in {mode:?}: {a:?} / {b:?}"),
            }
        }
        checked += 1;
    }
    assert!(refusals > 10, "the generator should also meet refusals ({refusals})");
    assert!(parsed_checked > 500, "parsed-text items checked: {parsed_checked}");
    assert!(fourth_shape > 0, "the generator should meet F7's fourth shape");
}

// J3.3's reserved keys are exactly the keys the parser does not read as a
// field name: each, in any letter case, never gives a field term.
#[test]
fn the_reserved_keys_are_the_parsers() {
    use meguri_search::builder::RESERVED_KEYS;
    for key in RESERVED_KEYS {
        for written in [key.to_owned(), key.to_ascii_uppercase()] {
            if let Ok(query) = parse(&format!("{written}:1")) {
                assert!(
                    !matches!(query.node(query.root()).kind, NodeKind::Term(meguri_search::Term::Field { .. })),
                    "{written}"
                );
            }
            assert_eq!(refused(&FieldPresent(written.clone())), InvalidItem::ReservedFieldName);
        }
    }
    for field in ["decks", "tags", "has_cd", "rid", "and"] {
        let query = parse(&format!("{field}:1")).unwrap();
        assert!(matches!(query.node(query.root()).kind, NodeKind::Term(meguri_search::Term::Field { .. })));
        assert!(build(&FieldPresent(s(field))).is_ok(), "{field}");
    }
}
