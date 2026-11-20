//! The search-field editing services (`specs/search/editing-services.md`
//! D7): U88a's `classify_tail` (D5). Each D5.2 row, each form of D5.1 at,
//! before and after the cursor, AE42's core part, and D1's offsets.

use meguri_search::canonical_text;
use meguri_search::tail::{Incomplete, InvalidOffset, Tail, classify_tail};

/// The text with the cursor at `|`, or at the end when there is none.
fn at(marked: &str) -> (String, usize) {
    match marked.find('|') {
        Some(byte) => (marked.replacen('|', "", 1), marked[..byte].chars().count()),
        None => (marked.to_owned(), marked.chars().count()),
    }
}

fn tail(marked: &str) -> Tail {
    let (text, cursor) = at(marked);
    classify_tail(&text, cursor).unwrap()
}

fn incomplete(marked: &str) -> (Incomplete, std::ops::Range<usize>) {
    match tail(marked) {
        Tail::Incomplete { form, range } => (form, range),
        other => panic!("{marked:?}: {other:?}"),
    }
}

fn error(marked: &str) -> &'static str {
    match tail(marked) {
        Tail::Error(error) => error.code(),
        other => panic!("{marked:?}: {other:?}"),
    }
}

// D5.2, row by row.
#[test]
fn the_spec_examples_classify_as_d5_2_says() {
    use Incomplete::*;
    // AE42 (core part): incomplete while typing, the error on commit.
    let ae42 = "deck:Japanese tag:\"wk-lev";
    assert_eq!(incomplete(ae42), (UnclosedQuote, 18..25));
    let committed = canonical_text(ae42).unwrap_err();
    assert_eq!((committed.code(), committed.range()), ("unclosed_quote", 18..25));
    assert_eq!(error(&format!("|{ae42}")), "unclosed_quote");

    assert_eq!(incomplete("deck:"), (EmptyValue, 0..5));
    assert_eq!(tail("deck: "), Tail::Valid);
    for text in ["deck:a::", "tag:a::", "f:RE:"] {
        assert_eq!(tail(text), Tail::Valid, "{text:?}");
    }
    for text in ["f:re:", "tag:nc:", "\"dupe:1,|\"", "prop:cds:k!="] {
        assert_eq!(incomplete(text).0, EmptyValue, "{text:?}");
    }
    assert_eq!(incomplete("deck:| is:due"), (EmptyValue, 0..5));
    assert_eq!(incomplete("a -"), (DanglingMinus, 2..3));
    assert_eq!(incomplete("(a b"), (UnclosedGroup, 0..4));
    assert_eq!(incomplete("deck:x (|is:due"), (UnclosedGroup, 7..14));
    for text in ["a or", "a or ", "or", "a and or", "(a or |)"] {
        assert_eq!(incomplete(text).0, OperatorWord, "{text:?}");
    }
    for text in ["or ", "a and or ", "a or )"] {
        assert_eq!(error(text), "misplaced_or", "{text:?}");
    }
    assert_eq!(incomplete("abc\\"), (TrailingEscape, 3..4));
    for text in [
        "is:",
        "is:ne",
        "is:buried-",
        "flag:",
        "flag:+",
        "added:",
        "rated:1:",
        "mid:-",
        "nid:1,",
        "dupe:12",
        "prop:",
        "prop:iv",
        "prop:re",
        "prop:ivl",
        "prop:ivl!",
        "prop:ivl>",
        "prop:due=-",
        "prop:ease>1e",
        "prop:r<in",
        "prop:cdn:",
        "prop:rated=-1:",
    ] {
        let (form, range) = incomplete(text);
        assert_eq!(form, PartialValue, "{text:?}");
        assert_eq!(range, 0..text.chars().count(), "{text:?}");
    }
    let errors = [
        ("is:NEW", "invalid_state"),
        ("is:x", "invalid_state"),
        ("flag:8", "invalid_flag"),
        ("flag:1.", "invalid_flag"),
        ("rated:1:5", "invalid_answer_button"),
        ("added:-1", "invalid_positive_whole_number"),
        ("prop:IVL", "invalid_property"),
        ("prop:ivl>1.", "invalid_positive_whole_number"),
        ("prop:ivl>-", "invalid_positive_whole_number"),
        ("prop:rated=1", "invalid_negative_whole_number"),
        ("nid:1,,", "invalid_id_list"),
        ("dupe:x", "invalid_whole_number"),
        ("is:x deck:", "invalid_state"),
        (":", "missing_key"),
    ];
    for (text, code) in errors {
        assert_eq!(error(text), code, "{text:?}");
    }
    assert_eq!(incomplete("is:| flag:9"), (PartialValue, 0..3));
    assert_eq!(tail(""), Tail::Valid);
}

// D5.1 forms with the cursor elsewhere than where they hold.
#[test]
fn each_form_needs_its_cursor() {
    use Incomplete::*;
    // empty_value: at the term's end, or just before its closing quote.
    assert_eq!(incomplete("deck:| x"), (EmptyValue, 0..5));
    assert_eq!(tail("|deck:"), Tail::Valid);
    assert_eq!(tail("deck: |x"), Tail::Valid);
    assert_eq!(incomplete("\"deck:|\"").0, EmptyValue);
    assert_eq!(incomplete("\"deck:\"").0, EmptyValue);
    assert_eq!(tail("\"deck|:\""), Tail::Valid);
    assert_eq!(incomplete("\"Extra Info:\"").0, EmptyValue);
    assert_eq!(incomplete("-deck:"), (EmptyValue, 0..6));
    assert_eq!(incomplete("x -tag:re:"), (EmptyValue, 2..10));
    assert_eq!(tail("deck:a"), Tail::Valid);
    assert_eq!(incomplete("w:"), (EmptyValue, 0..2));
    // dangling_minus: only a bare `-` ending at the cursor.
    assert_eq!(incomplete("a -| b"), (DanglingMinus, 2..3));
    assert_eq!(tail("a| -"), Tail::Valid);
    assert_eq!(tail("a \\-"), Tail::Valid);
    assert_eq!(tail("a \"-\""), Tail::Valid);
    // unclosed_quote and empty_quote: after the opening quote.
    assert_eq!(incomplete("x \"a|b").0, UnclosedQuote);
    assert_eq!(error("x |\"ab"), "unclosed_quote");
    assert_eq!(incomplete("x \"|\""), (EmptyQuote, 2..4));
    assert_eq!(error("x \"\"|"), "empty_quote");
    assert_eq!(error("x |\"\""), "empty_quote");
    // unclosed_group: after the `(` the error names, to the end of the text.
    assert_eq!(incomplete("a (b (c) d|"), (UnclosedGroup, 2..10));
    assert_eq!(error("|(a"), "unclosed_group");
    assert_eq!(incomplete("-(a b"), (UnclosedGroup, 1..5));
    // operator_word: spaces only after it, nothing after it in its sequence.
    assert_eq!(incomplete("a or  |"), (OperatorWord, 2..4));
    assert_eq!(incomplete("a or | "), (OperatorWord, 2..4));
    assert_eq!(error("a| or"), "misplaced_or");
    assert_eq!(incomplete("(x) and"), (OperatorWord, 4..7));
    assert_eq!(incomplete("-a and "), (OperatorWord, 3..6));
    assert_eq!(error("(and |"), "misplaced_and");
    assert_eq!(incomplete("x (and|"), (OperatorWord, 3..6));
    // trailing_escape: only the lone backslash ending at the cursor.
    assert_eq!(error("ab|c\\"), "unknown_escape");
    assert_eq!(error("a\\ b"), "unknown_escape");
    assert_eq!(error("a\\x"), "unknown_escape");
    // partial_value: unquoted, ending at the cursor.
    assert_eq!(error("is:n|e x"), "invalid_state");
    assert_eq!(incomplete("is:ne| x"), (PartialValue, 0..5));
    assert_eq!(incomplete("x -is:ne"), (PartialValue, 2..8));
    assert_eq!(error("is:\"ne\""), "invalid_state");
    assert_eq!(error("\"is:ne\""), "invalid_state");
    assert_eq!(error("|is:ne"), "invalid_state");
    // Some completions need a sign, leading zeros or a separator.
    for text in ["rated:1:0", "rated:+", "dupe:", "dupe:-", "prop:due<=+", "prop:rated<-0:", "nid:", "did:٣,"] {
        assert_eq!(incomplete(text).0, PartialValue, "{text:?}");
    }
    for text in ["rated::", "dupe:1x", "nid:,", "prop:resched=0:", "prop:ease>.e", "prop:pos=1e3", "flag:00x"] {
        assert!(matches!(tail(text), Tail::Error(_)), "{text:?}");
    }
}

// D1: a cursor past the end; J2's separators before anything reads.
#[test]
fn offsets_are_characters_of_the_given_text() {
    assert_eq!(classify_tail("abc", 4), Err(InvalidOffset));
    assert_eq!(classify_tail("日本", 2), Ok(Tail::Valid));
    assert_eq!(classify_tail("日本", 3), Err(InvalidOffset));
    // `deck:` then U+001F: the separator is a space, so the value is empty.
    assert_eq!(
        classify_tail("deck:\u{1f}", 5),
        Ok(Tail::Incomplete {
            form: Incomplete::EmptyValue,
            range: 0..5
        })
    );
    // The lead G1 trims: ranges stay offsets of the given text.
    assert_eq!(
        classify_tail("  \u{a0}is:ne", 8),
        Ok(Tail::Incomplete {
            form: Incomplete::PartialValue,
            range: 3..8
        })
    );
    assert_eq!(
        classify_tail("😀 deck:", 7),
        Ok(Tail::Incomplete {
            form: Incomplete::EmptyValue,
            range: 2..7
        })
    );
}
