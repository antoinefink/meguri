//! Reading search text (`specs/search/grammar.md`, `errors.md`): U80's test
//! scenarios, error details and ranges (E2, E5), messages (E7), depth (E6)
//! and properties over generated text. Expected values come from the specs'
//! rules and examples, never from the parser.

use meguri_search::ast::{
    BoolOperator, CardState, Comparison, Float, IdKind, NodeId, NodeKind, Property, Query, TemplateRef, TextMode,
};
use meguri_search::{ErrorKind, SearchError, Term, canonical_text, parse};

/// The query's shape: `(a AND b)`, `(a OR b)`, `NOT x`, `[x]` for a group,
/// plain text as itself and other terms in debug form.
fn shape(query: &Query) -> String {
    fn walk(query: &Query, id: NodeId, out: &mut String) {
        let join = |items: &[NodeId], joiner: &str, out: &mut String| {
            out.push('(');
            for (k, item) in items.iter().enumerate() {
                if k > 0 {
                    out.push_str(joiner);
                }
                walk(query, *item, out);
            }
            out.push(')');
        };
        match &query.node(id).kind {
            NodeKind::And(items) => join(items, " AND ", out),
            NodeKind::Or(items) => join(items, " OR ", out),
            NodeKind::Not(item) => {
                out.push_str("NOT ");
                walk(query, *item, out);
            }
            NodeKind::Group(inside) => {
                out.push('[');
                walk(query, *inside, out);
                out.push(']');
            }
            NodeKind::Term(Term::Text(text)) => out.push_str(text),
            NodeKind::Term(term) => out.push_str(&format!("{term:?}")),
            NodeKind::NegatedOperator(BoolOperator::And) => out.push_str("-and"),
            NodeKind::NegatedOperator(BoolOperator::Or) => out.push_str("-or"),
        }
    }
    let mut out = String::new();
    walk(query, query.root(), &mut out);
    out
}

fn parsed(text: &str) -> Query {
    parse(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
}

fn term(text: &str) -> Term {
    let query = parsed(text);
    match &query.node(query.root()).kind {
        NodeKind::Term(term) => term.clone(),
        other => panic!("{text:?} is {other:?}"),
    }
}

fn error(text: &str) -> SearchError {
    match parse(text) {
        Ok(query) => panic!("{text:?} parsed: {}", shape(&query)),
        Err(e) => e,
    }
}

// AE41, R1024, G2.
#[test]
fn and_binds_tighter_than_or() {
    let query = parsed("dog cat or  fish");
    assert_eq!(shape(&query), "((dog AND cat) OR fish)");
    assert_eq!(query.canonical_text(), "dog cat OR fish");
    assert_eq!(shape(&parsed("a b or c d")), "((a AND b) OR (c AND d))");
    assert_eq!(shape(&parsed("x or y and z")), "(x OR (y AND z))");
    assert_eq!(shape(&parsed("not a")), "(not AND a)");
}

// R1021, G1, Q2.
#[test]
fn an_empty_or_blank_search_is_the_whole_collection() {
    for text in ["", "   ", "\t\n", "\u{3000}", " \u{85}\u{2028} "] {
        assert_eq!(term(text), Term::WholeCollection, "{text:?}");
        assert_eq!(canonical_text(text).unwrap(), "deck:*", "{text:?}");
    }
}

// R1022, G1: the 25 White_Space characters, and some that are not.
#[test]
fn every_white_space_character_separates_terms() {
    let spaces = [
        '\u{9}', '\u{A}', '\u{B}', '\u{C}', '\u{D}', ' ', '\u{85}', '\u{A0}', '\u{1680}', '\u{2000}', '\u{2001}',
        '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}',
        '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}',
    ];
    assert_eq!(spaces.len(), 25);
    for c in spaces {
        assert_eq!(shape(&parsed(&format!("a{c}b"))), "(a AND b)", "U+{:04X}", c as u32);
        // Inside quotes too.
        assert_eq!(
            term(&format!("\"a{c}b\"")),
            Term::Text("a b".into()),
            "U+{:04X}",
            c as u32
        );
    }
    for c in ['\u{200B}', '\u{FEFF}', '\u{180E}', '\u{1C}', '\u{1F}', '\u{0}'] {
        assert_eq!(
            term(&format!("a{c}b")),
            Term::Text(format!("a{c}b")),
            "U+{:04X}",
            c as u32
        );
    }
    assert_eq!(shape(&parsed("re:a\tb")), "(Regex(\"a\") AND b)");
}

// R1023, G2.
#[test]
fn operator_words_ignore_ascii_case_and_quoted_words_are_text() {
    assert_eq!(shape(&parsed("a AND b")), "(a AND b)");
    assert_eq!(shape(&parsed("a Or b")), "(a OR b)");
    assert_eq!(shape(&parsed("a aNd b oR c")), "((a AND b) OR c)");
    assert_eq!(term("\"and\""), Term::Text("and".into()));
    assert_eq!(term("\"OR\""), Term::Text("OR".into()));
    assert_eq!(shape(&parsed("andy or orb")), "(andy OR orb)");
    assert_eq!(
        term("and:x"),
        Term::Field {
            name: "and".into(),
            mode: TextMode::Normal,
            text: "x".into()
        }
    );
}

// R1025, G4.
#[test]
fn a_minus_negates_a_group_a_quoted_term_or_a_key_term() {
    assert_eq!(shape(&parsed("-(a or b)")), "NOT [(a OR b)]");
    assert_eq!(shape(&parsed("-\"x y\"")), "NOT x y");
    assert_eq!(shape(&parsed("-deck:a")), "NOT Deck(\"a\")");
    assert_eq!(shape(&parsed("-a b")), "(NOT a AND b)");
    // The negated item may start with `-` itself.
    assert_eq!(shape(&parsed("--a")), "NOT -a");
    assert_eq!(shape(&parsed("-\\-")), "NOT -");
    // A `-` with nothing directly after it is the plain term `-`.
    assert_eq!(shape(&parsed("- a")), "(- AND a)");
    assert_eq!(shape(&parsed("-")), "-");
    assert_eq!(shape(&parsed("a -and b")), "(a AND -and AND b)");
    assert_eq!(shape(&parsed("-OR")), "-or");
    assert_eq!(shape(&parsed("-\"and\"")), "NOT and");
}

// R1026, G3 form 2.
#[test]
fn a_partly_quoted_value_needs_the_quote_right_after_the_colon() {
    let field = |name: &str, text: &str| Term::Field {
        name: name.into(),
        mode: TextMode::Normal,
        text: text.into(),
    };
    assert_eq!(term("key:\"value with spaces\""), field("key", "value with spaces"));
    let query = parsed("key: \"value\"");
    assert_eq!(shape(&query), format!("({:?} AND value)", field("key", "")));
    assert_eq!(query.canonical_text(), "key: value");
    assert_eq!(shape(&parsed("a:\"b\"c")), format!("({:?} AND c)", field("a", "b")));
    assert_eq!(shape(&parsed("a:b:\"c\"")), format!("({:?} AND c)", field("a", "b:")));
    assert_eq!(term("DECK:\"a b\""), Term::Deck("a b".into()));
}

// R1027, G3 form 3, G2: terms end at a space, `"`, `(` or `)`.
#[test]
fn spaces_need_not_separate_terms_from_quotes_and_brackets() {
    assert_eq!(shape(&parsed("a\"b\"(c)")), "(a AND b AND [c])");
    assert_eq!(shape(&parsed("\"f o o\"bar")), "(f o o AND bar)");
    assert_eq!(shape(&parsed("(x)(y)")), "([x] AND [y])");
    assert_eq!(term("a\\\"b"), Term::Text("a\"b".into()));
    assert_eq!(term("a-b"), Term::Text("a-b".into()));
}

// R1028, R1029, R1030, G6, G7.
#[test]
fn escapes_colons_and_hyphens() {
    assert_eq!(term("3\\:30"), Term::Text("3:30".into()));
    assert_eq!(term("w:3:30"), Term::Word("3:30".into()));
    assert_eq!(term("\\-term"), Term::Text("-term".into()));
    assert_eq!(term("\"-term\""), Term::Text("-term".into()));
    // `\\`, `\*` and `\_` stay escaped for the wildcard matcher.
    assert_eq!(
        term("\\\\\\*\\_\\\"\\:\\(\\)\\-"),
        Term::Text("\\\\\\*\\_\":()-".into())
    );
    assert_eq!(term("a\\\\b"), Term::Text("a\\\\b".into()));
    // Q, D and R readings.
    assert_eq!(term("re:a\\\"b\\d"), Term::Regex("a\"b\\d".into()));
    assert_eq!(
        term("dupe:5,a\\\\\\\"b\\x"),
        Term::Duplicates {
            notetype: meguri_model::ids::NotetypeId(5),
            text: "a\\\"b\\x".into()
        }
    );
    assert_eq!(term("preset:a\\-b\\x"), Term::Preset("a\\-b\\x".into()));
    // Field names are U-read; mode prefixes are matched as written.
    assert_eq!(
        term("f\\(x\\):re:a\\x"),
        Term::Field {
            name: "f(x)".into(),
            mode: TextMode::Regex,
            text: "a\\x".into()
        }
    );
    assert_eq!(
        term("f:RE:x"),
        Term::Field {
            name: "f".into(),
            mode: TextMode::Normal,
            text: "RE:x".into()
        }
    );
    assert_eq!(
        term("tag:re\\:x"),
        Term::Tag {
            mode: TextMode::Normal,
            text: "re:x".into()
        }
    );
}

// R1028, E2, E5: the escape's own characters.
#[test]
fn unknown_escapes_report_the_escape_and_its_range() {
    let cases: [(&str, &str, std::ops::Range<usize>); 9] = [
        ("\\x", "\\x", 0..2),
        ("abc\\", "\\", 3..4),
        ("foo\\ bar baz", "\\ bar baz", 3..12),
        ("  a\\\u{3000}b  c ", "\\ b  c", 3..9),
        ("\"a\\ b\"", "\\ ", 2..4),
        ("\\x\\y", "\\x", 0..2),
        ("a\\\\\\x", "\\x", 3..5),
        ("é deck:a\\%", "\\%", 8..10),
        ("x fi\\eld:re:\\d", "\\e", 4..6),
    ];
    for (text, provided, range) in cases {
        let e = error(text);
        assert_eq!(
            (e.kind(), e.range()),
            (
                &ErrorKind::UnknownEscape {
                    provided: provided.into()
                },
                range
            ),
            "{text:?}"
        );
    }
}

// R1032, G6: unreserved keys are field searches; reserved keys are compared
// as typed.
#[test]
fn any_other_key_is_a_field_search() {
    let field = |name: &str, text: &str| Term::Field {
        name: name.into(),
        mode: TextMode::Normal,
        text: text.into(),
    };
    assert_eq!(term("rid:1:2"), field("rid", "1:2"));
    assert_eq!(term("has\\-cd:x"), field("has-cd", "x"));
    assert_eq!(term("Has-CD:x"), Term::HasCustomData("x".into()));
    assert_eq!(term("de\\:ck:x"), field("de:ck", "x"));
    assert_eq!(term("\"a:b c\""), field("a", "b c"));
    assert_eq!(term("\"a\\:b c\""), Term::Text("a:b c".into()));
}

// R1031, G8.
#[test]
fn keys_ignore_ascii_case_and_fixed_values_do_not() {
    assert_eq!(term("DECK:x"), Term::Deck("x".into()));
    assert_eq!(
        term("Tag:y"),
        Term::Tag {
            mode: TextMode::Normal,
            text: "y".into()
        }
    );
    assert_eq!(term("Is:new"), Term::State(CardState::New));
    for state in CardState::ALL {
        assert_eq!(term(&format!("is:{}", state.word())), Term::State(state));
    }
    assert_eq!(
        error("is:NEW").kind(),
        &ErrorKind::InvalidState { provided: "NEW".into() }
    );
    assert_eq!(term("deck:current"), Term::Deck("current".into()));
}

// G8, G9: values of reserved keys.
#[test]
fn reserved_key_values() {
    assert_eq!(term("card:0"), Term::Template(TemplateRef::Ordinal(0)));
    assert_eq!(term("card:+2"), Term::Template(TemplateRef::Ordinal(1)));
    assert_eq!(term("card:65536"), Term::Template(TemplateRef::Name("65536".into())));
    assert_eq!(term("card:\\-1"), Term::Template(TemplateRef::Name("-1".into())));
    assert_eq!(term("flag:+7"), Term::Flag(7));
    assert_eq!(term("added:0"), Term::Added(1));
    assert_eq!(term("resched:0"), Term::Rescheduled(0));
    assert_eq!(
        term("rated:0:4"),
        Term::Rated {
            days: 1,
            button: Some(4)
        }
    );
    assert_eq!(term("mid:-5"), Term::NotetypeId(meguri_model::ids::NotetypeId(-5)));
    assert_eq!(
        term("NID:007,\u{661}"),
        Term::Ids {
            kind: IdKind::Notes,
            list: "007,\u{661}".into()
        }
    );
    assert_eq!(
        term("prop:ivl>=+5"),
        Term::Property {
            property: Property::Interval(5),
            comparison: Comparison::GreaterOrEqual
        }
    );
    assert_eq!(
        term("prop:rated=-1:4"),
        Term::Property {
            property: Property::Rated {
                days: -1,
                button: Some(4)
            },
            comparison: Comparison::Equal
        }
    );
    assert_eq!(
        term("prop:cdn:k1!=1e3"),
        Term::Property {
            property: Property::CustomNumber {
                key: "k1".into(),
                value: Float(1000.0)
            },
            comparison: Comparison::NotEqual
        }
    );
    assert_eq!(
        term("prop:cds:k=a\\:b"),
        Term::Property {
            property: Property::CustomString {
                key: "k".into(),
                value: "a\\:b".into()
            },
            comparison: Comparison::Equal
        }
    );
    // G8.1: a prefix of the value is the property, so `stability` is `s`.
    assert_eq!(
        error("prop:stability>1").kind(),
        &ErrorKind::InvalidOperator { provided: "s".into() }
    );
    match term("prop:ease>nan") {
        Term::Property {
            property: Property::Ease(Float(x)),
            ..
        } => assert!(x.is_nan()),
        other => panic!("{other:?}"),
    }
}

/// `(kind, provided, context)` of a text's error.
fn details(text: &str) -> (String, Option<String>, Option<String>) {
    let e = error(text);
    (
        e.code().to_owned(),
        e.kind().provided().map(str::to_owned),
        e.kind().context().map(str::to_owned),
    )
}

// R1075, E1, E2: each category, with the details Anki reports.
#[test]
fn every_parse_error_category_is_reachable() {
    let cases: [(&str, &str, Option<&str>, Option<&str>); 21] = [
        ("and a", "misplaced_and", None, None),
        ("a or", "misplaced_or", None, None),
        ("()", "empty_group", None, None),
        (")", "unopened_group", None, None),
        ("(a", "unclosed_group", None, None),
        ("\"\"", "empty_quote", None, None),
        ("\"abc", "unclosed_quote", None, None),
        (":x", "missing_key", None, None),
        ("\\x", "unknown_escape", Some("\\x"), None),
        ("is:foo", "invalid_state", Some("foo"), None),
        ("flag:9", "invalid_flag", None, None),
        ("prop:foo>1", "invalid_property", Some("foo>1"), None),
        ("prop:ivl~1", "invalid_operator", Some("ivl"), None),
        ("prop:ease>x", "invalid_number", Some("x"), Some("ease>x")),
        ("mid:x", "invalid_whole_number", Some("x"), Some("mid:")),
        ("prop:ivl>x", "invalid_positive_whole_number", Some("x"), Some("ivl>x")),
        (
            "prop:rated=1",
            "invalid_negative_whole_number",
            Some("1"),
            Some("rated=1"),
        ),
        ("rated:1:5", "invalid_answer_button", Some("5"), Some("1:5")),
        ("nid:1,a", "invalid_id_list", None, Some("nid:")),
        ("dupe:1", "invalid_dupe", None, None),
        ("added:x", "invalid_positive_whole_number", Some("x"), Some("added:")),
    ];
    for (text, kind, provided, context) in cases {
        assert_eq!(
            details(text),
            (kind.to_owned(), provided.map(str::to_owned), context.map(str::to_owned)),
            "{text:?}"
        );
        let e = error(text);
        assert!(e.kind().is_parse_error(), "{text:?}");
    }
}

// E4: the first error from the left, item errors before sequence ends.
#[test]
fn errors_are_found_from_left_to_right() {
    let cases = [
        ("( is:foo", "invalid_state"),
        ("(a and)", "misplaced_and"),
        ("a or )", "misplaced_or"),
        ("a ) (b", "unopened_group"),
        ("(()", "empty_group"),
        ("((a)", "unclosed_group"),
        ("\"a\\\"", "unclosed_quote"),
        ("a\"\"", "empty_quote"),
        ("\"x\":y", "missing_key"),
        ("- OR", "misplaced_or"),
        ("a or and b", "misplaced_and"),
        ("a and or b", "misplaced_or"),
        ("dupe:x", "invalid_whole_number"),
        ("f\\x:\\y", "unknown_escape"),
        ("rated:x:5", "invalid_positive_whole_number"),
        ("-)", "unopened_group"),
        ("-(", "unclosed_group"),
    ];
    for (text, kind) in cases {
        assert_eq!(error(text).code(), kind, "{text:?}");
    }
}

// E2: the offending text is `provided` when the kind has it, otherwise the
// text the range covers.
#[test]
fn the_offending_text_is_provided_or_the_covered_text() {
    let cases = [
        ("prop:ivl>x", "x"),
        ("( is:foo", "foo"),
        ("foo\\ bar", "\\ bar"),
        ("a or", "or"),
        ("x ( )", "( )"),
        ("é -( )", "( )"),
        ("a \"bc  ", "\"bc"),
        ("a\tk:\"\" b", "\"\""),
        ("-NID:1,a", "NID:1,a"),
        ("dupe:1", "dupe:1"),
        ("flag:9 x", "flag:9"),
    ];
    for (text, offending) in cases {
        assert_eq!(error(text).offending_text(text), offending, "{text:?}");
    }
}

// E5: Meguri's ranges, character offsets of the caller's text.
#[test]
fn error_ranges_follow_e5() {
    let cases: [(&str, &str, std::ops::Range<usize>); 16] = [
        ("x and", "misplaced_and", 2..5),
        ("OR x", "misplaced_or", 0..2),
        ("x ( )", "empty_group", 2..5),
        ("é -( )", "empty_group", 3..6),
        ("a ) b", "unopened_group", 2..3),
        ("(a (b)", "unclosed_group", 0..1),
        ("x (a (b", "unclosed_group", 2..3),
        ("x -(a", "unclosed_group", 3..4),
        ("\t\"\" a", "empty_quote", 1..3),
        ("a k:\"\"", "empty_quote", 4..6),
        ("a \"bc  ", "unclosed_quote", 2..5),
        (":x", "missing_key", 0..2),
        ("\t-is:foo", "invalid_state", 2..8),
        ("a \"is:foo\"", "invalid_state", 2..10),
        ("é k:\"v\" prop:ivl>x", "invalid_positive_whole_number", 8..18),
        ("NID:\"1 2\"", "invalid_id_list", 0..9),
    ];
    for (text, kind, range) in cases {
        let e = error(text);
        assert_eq!((e.code(), e.range()), (kind, range), "{text:?}");
    }
}

// E7: Meguri's wording, the E2 texts with backticks turned to apostrophes.
#[test]
fn messages_name_the_offending_text() {
    let cases = [
        (
            "a or",
            "Invalid search: an `or` must stand between two search terms. To search for the word, put it in double quotes: `\"or\"`.",
        ),
        (
            "()",
            "Invalid search: there is nothing to search for between `(` and `)`. To search for brackets, put them in double quotes: `\"( )\"`.",
        ),
        (
            "\"\"",
            "Invalid search: there is nothing to search for between the double quotes `\"\"`. To search for double quotes, write `\\\"\\\"`.",
        ),
        (
            "a\\`b",
            "Invalid search: the escape `\\'` does not exist. To search for a backslash, write `\\\\`.",
        ),
        ("is:a`b", "Invalid search: `is:` does not accept `a'b`."),
        (
            "prop:ivl~1",
            "Invalid search: `prop:ivl` needs one of the comparisons `=`, `!=`, `<`, `>`, `<=` or `>=`.",
        ),
        (
            "added:`",
            "Invalid search: expected a whole number of 0 or more in `added:`, found `'`.",
        ),
        (
            "rated:1:5",
            "Invalid search: expected an answer button from 1 to 4 in `1:5`, found `5`.",
        ),
        ("NID:1,a", "Invalid search: `NID:` takes only ids separated by commas."),
        (
            "flag:8",
            "Invalid search: `flag:` needs a flag number: 1 red, 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple or 0 for no flag.",
        ),
    ];
    for (text, message) in cases {
        assert_eq!(error(text).message(), message, "{text:?}");
    }
    // The texts themselves keep their backticks; only displays change them.
    assert_eq!(error("is:a`b").kind().provided(), Some("a`b"));
}

/// E7's block, read from the spec: each `<label>: <text>` line, the label
/// being the kind or `search_failed (<cause>)`.
fn spec_messages() -> Vec<(String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../specs/search/errors.md");
    let spec = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (_, e7) = spec.split_once("## E7.").expect("errors.md has E7");
    let (_, block) = e7.split_once("```text\n").expect("E7 holds a text block");
    let (block, _) = block.split_once("```").expect("E7's block is closed");
    block
        .lines()
        .map(|line| {
            let (label, text) = line.split_once(": ").expect("a `<kind>: <text>` line");
            (label.to_owned(), text.to_owned())
        })
        .collect()
}

fn plan_error(text: &str) -> SearchError {
    use meguri_search::{BuildError, FoldMode, Plan, SearchContext, SearchMode};
    let context = SearchContext {
        now: 0,
        today: 0,
        next_day_at: 86_400,
        learn_ahead_secs: SearchContext::DEFAULT_LEARN_AHEAD_SECS,
        current_deck: meguri_model::ids::DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    };
    match Plan::build(&parsed(text), SearchMode::Cards, &context) {
        Err(BuildError::Search(e)) => e,
        other => panic!("{text:?}: {other:?}"),
    }
}

// R1075, E7: every message character for character as the spec writes it,
// `{provided}` and `{context}` being E2's texts with backticks turned to
// apostrophes. A kind the spec adds fails here until it has a case.
#[test]
fn every_message_is_the_spec_text() {
    use meguri_search::SearchFailure;
    // (label, error, provided, context): the texts from E2's rules.
    let cases: Vec<(&str, SearchError, &str, &str)> = vec![
        ("misplaced_and", error("and a"), "", ""),
        ("misplaced_or", error("a or"), "", ""),
        ("empty_group", error("()"), "", ""),
        ("unopened_group", error(")"), "", ""),
        ("unclosed_group", error("(a"), "", ""),
        ("empty_quote", error("\"\""), "", ""),
        ("unclosed_quote", error("\"abc"), "", ""),
        ("missing_key", error(":x"), "", ""),
        ("unknown_escape", error("a\\`b"), "\\`", ""),
        ("invalid_state", error("is:a`b"), "a`b", ""),
        ("invalid_flag", error("flag:9"), "", ""),
        ("invalid_property", error("prop:f`>1"), "f`>1", ""),
        ("invalid_operator", error("prop:cdn:ab~1"), "cdn:ab", ""),
        ("invalid_number", error("prop:ease>`"), "`", "ease>`"),
        ("invalid_whole_number", error("dupe:x`,a"), "x`", "x`,a"),
        ("invalid_positive_whole_number", error("added:x"), "x", "added:"),
        (
            "invalid_negative_whole_number",
            error("prop:resched=1"),
            "1",
            "resched=1",
        ),
        ("invalid_answer_button", error("prop:rated=-1:5"), "5", "rated=-1:5"),
        ("invalid_id_list", error("NID:1,a"), "", "NID:"),
        ("invalid_dupe", error("dupe:1"), "", ""),
        ("malformed", SearchError::new(ErrorKind::Malformed, 0..1), "", ""),
        ("search_failed (negated operator word)", plan_error("a -OR b"), "", ""),
        ("search_failed (id list)", plan_error("Cid:\u{661}"), "", "Cid:"),
        (
            "search_failed (database refusal)",
            SearchError::new(ErrorKind::SearchFailed(SearchFailure::DatabaseRefusal), 0..1),
            "",
            "",
        ),
    ];
    let spec = spec_messages();
    let mut labels: Vec<&str> = spec.iter().map(|(label, _)| label.as_str()).collect();
    // `{pattern}` and `{reason}` belong to text-matching.md (U81).
    labels.retain(|label| *label != "invalid_regex");
    assert_eq!(labels, cases.iter().map(|case| case.0).collect::<Vec<_>>());
    for (label, error, provided, context) in cases {
        let (_, text) = spec.iter().find(|(l, _)| l == label).unwrap();
        let expected = format!(
            "Invalid search: {}",
            text.replace("{provided}", &provided.replace('`', "'"))
                .replace("{context}", &context.replace('`', "'"))
        );
        assert_eq!(error.message(), expected, "{label}");
        assert_eq!(error.to_string(), expected, "{label}");
    }
    let regex = SearchError::new(
        ErrorKind::InvalidRegex {
            pattern: "a(".into(),
            reason: "the group is not closed".into(),
        },
        0..4,
    );
    let (_, text) = spec.iter().find(|(l, _)| l == "invalid_regex").unwrap();
    assert_eq!(
        regex.message(),
        format!(
            "Invalid search: {}",
            text.replace("{pattern}", "a(")
                .replace("{reason}", "the group is not closed")
        )
    );
}

// R1025, E6: no depth or length exhausts the stack.
#[test]
fn deep_nesting_and_long_searches_never_fail_for_their_size() {
    let depth = 200_000;
    let deep = format!("{}a{}", "(".repeat(depth), ")".repeat(depth));
    let query = parsed(&deep);
    let canonical = query.canonical_text();
    assert_eq!(
        canonical,
        format!("{}a{}", "(".repeat(depth - 1), ")".repeat(depth - 1))
    );
    drop(query);

    let negations = format!("{}a{}", "-(".repeat(depth), ")".repeat(depth));
    assert_eq!(parsed(&negations).canonical_text(), negations);

    let unclosed = format!("{}a", "(".repeat(depth));
    let e = error(&unclosed);
    assert_eq!((e.code(), e.range()), ("unclosed_group", 0..1));

    let long = vec!["a"; 300_000].join(" or ");
    assert_eq!(parsed(&long).canonical_text(), vec!["a"; 300_000].join(" OR "));
}

/// SplitMix64, so the generated cases are the same on every run.
struct Seq(u64);

impl Seq {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

const PIECES: [&str; 48] = [
    "a",
    "dog",
    "é",
    "日本",
    "\u{1F600}",
    " ",
    "  ",
    "\t",
    "\u{3000}",
    "\u{A0}",
    "(",
    ")",
    "\"",
    "\\",
    "-",
    ":",
    "and",
    "or",
    "AND",
    "Or",
    "deck:",
    "is:",
    "new",
    "prop:",
    "ivl",
    ">",
    "=",
    "1",
    "-1",
    "x",
    ",",
    "nid:",
    "re:",
    "nc:",
    "tag:",
    "dupe:",
    "card:",
    "flag:",
    "rated:",
    "*",
    "_",
    "`",
    "\u{200B}",
    "cdn:",
    "cds:",
    "has-cd:",
    "preset:",
    "\n",
];

// E5, E6: every text parses or gives an error whose range is a non-empty
// part of it, and every parsed text has a canonical text.
#[test]
fn generated_texts_parse_or_fail_with_a_range_inside_the_text() {
    let mut seq = Seq(0x5EA2_C0DE);
    for _ in 0..20_000 {
        let pieces = seq.below(16);
        let text: String = (0..pieces).map(|_| PIECES[seq.below(PIECES.len())]).collect();
        let length = text.chars().count();
        match parse(&text) {
            Ok(query) => {
                let canonical = query.canonical_text();
                assert_eq!(canonical_text(&text).as_deref(), Ok(canonical.as_str()));
            }
            Err(e) => {
                let range = e.range();
                assert!(range.start < range.end && range.end <= length, "{text:?}: {e:?}");
                assert!(e.kind().is_parse_error() && e.message().starts_with("Invalid search: "));
                if let Some(provided) = e.kind().provided()
                    && e.code() == "unknown_escape"
                {
                    let covered: String = text.chars().skip(range.start).take(range.len()).collect();
                    let normalized: String = covered
                        .chars()
                        .map(|c| if c.is_whitespace() { ' ' } else { c })
                        .collect();
                    assert_eq!(normalized, provided, "{text:?}");
                }
            }
        }
    }
}
