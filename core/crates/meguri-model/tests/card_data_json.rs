//! Card `data` text (specs/model/card-data-json.md): read, rebuilt and written
//! back as the oracle does, except that Meguri keeps unknown keys (R1461).
//! Inputs come from the card-data scenarios; expected texts from their goldens
//! and, for unknown keys, from the deviation entry.

mod common;

use common::{golden, scenario};
use meguri_model::card_data::{CardData, MemoryState};
use serde_json::Value;

/// The text each card's `data` holds before the rewrite, in card order:
/// the bound string, or `None` where the scenario stores another storage class
/// (which reads as empty text, C1).
fn stored_texts(id: &str) -> Vec<Option<String>> {
    scenario(id)["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|step| step.get("db_exec"))
        .filter(|exec| exec["sql"].as_str().unwrap().starts_with("UPDATE cards SET data"))
        .map(|exec| exec.get("params").map(|p| p[0].as_str().unwrap().to_owned()))
        .collect()
}

/// The `data` texts the golden's last step reads back, in card order.
fn rewritten_texts(golden: &Value) -> Vec<String> {
    let steps = golden["observations"]["steps"].as_array().unwrap();
    steps.last().unwrap()["db_exec"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[0].as_str().unwrap().to_owned())
        .collect()
}

fn rewrite(stored: &Option<String>) -> String {
    CardData::parse(stored.as_deref().unwrap_or("")).to_text()
}

fn assert_rewrites(id: &str, expected: &[String]) {
    let stored = stored_texts(id);
    assert_eq!(stored.len(), expected.len(), "{id}: one input per card");
    for (i, (input, want)) in stored.iter().zip(expected).enumerate() {
        assert_eq!(&rewrite(input), want, "{id} card {}: input {input:?}", i + 1);
    }
}

#[test]
fn rounding_and_number_text_match_the_oracle() {
    let id = "model/card-data/rewrite";
    assert_rewrites(id, &rewritten_texts(&golden(id)));
}

#[test]
fn values_that_do_not_fit_read_as_absent_as_in_the_oracle() {
    let id = "model/card-data/lenient";
    assert_rewrites(id, &rewritten_texts(&golden(id)));
}

/// The rows a deviation entry of `conformance/deviations.yaml` puts in place
/// of the golden's, after checking where it applies.
fn deviation_rows(id: &str, pointer: &str) -> Vec<String> {
    let deviations = common::yaml_file("deviations.yaml");
    let entry = deviations["deviations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id)
        .expect("the deviation entry");
    assert_eq!(entry["pointer"], pointer);
    entry["transform"]["replace"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[0].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn unknown_keys_survive_rewrite() {
    let id = "model/card-data/unknown-keys";
    assert_rewrites(id, &deviation_rows("card-data-unknown-keys", "/steps/7/db_exec/rows"));
}

#[test]
fn numbers_beyond_the_float_range_read_as_the_oracle_reads_them() {
    // json-numbers N10 and C3 rule 1: the first 21 cards, which the
    // golden's step 25 reads back; the last three are the deviation below.
    let id = "model/card-data/out-of-range";
    let g = golden(id);
    let expected: Vec<String> = common::rows(&g, 25)
        .iter()
        .map(|row| row[0].as_str().unwrap().to_owned())
        .collect();
    let stored = stored_texts(id);
    assert_eq!((stored.len(), expected.len()), (24, 21));
    for (i, (input, want)) in stored.iter().zip(&expected).enumerate() {
        assert_eq!(&rewrite(input), want, "{id} card {}: input {input:?}", i + 1);
    }
}

/// Deviation `card-data-unknown-keys-range`: the values of unknown keys are
/// kept as their text without a range check (C3 rule 7, N10).
#[test]
fn unknown_keys_skip_range_check() {
    let id = "model/card-data/out-of-range";
    let expected = deviation_rows("card-data-unknown-keys-range", "/steps/26/db_exec/rows");
    let stored = stored_texts(id);
    assert_eq!(expected.len(), 3);
    for (i, (input, want)) in stored[21..].iter().zip(&expected).enumerate() {
        assert_eq!(&rewrite(input), want, "{id} card {}: input {input:?}", i + 22);
    }
    let data = CardData::parse(r#"{"pos":5,"zz":-1e400}"#);
    assert_eq!(data.original_position, Some(5));
    assert_eq!(data.unknown_keys().collect::<Vec<_>>(), [("zz", "-1e400")]);
}

/// specs/model/json-numbers.md N10 table, through the `s` key: a number in
/// range keeps the memory state, one out of range empties the whole text.
#[test]
fn spec_range_table() {
    let ones = |zeros: usize| format!("1{}", "0".repeat(zeros));
    let cases = [
        ("1e400".to_owned(), false),
        ("0.1e310".to_owned(), false),
        ("1e99999999999".to_owned(), false),
        (ones(309), false),
        (ones(308), true),
        ("1.7976931348623157e308".to_owned(), true),
        ("1.7976931348623158e308".to_owned(), false),
        ("17976931348623157081e289".to_owned(), false),
        ("1797693134862315e293".to_owned(), true),
        ("1e-400".to_owned(), true),
        ("0e400".to_owned(), true),
        // The sign, an explicit exponent sign and a capital E change nothing.
        ("-1e400".to_owned(), false),
        ("1e+400".to_owned(), false),
        ("1E400".to_owned(), false),
        ("-0.0e400".to_owned(), true),
        ("1e-99999999999".to_owned(), true),
        // Fraction digits count against the exponent, leading zeros included.
        ("0.0001e312".to_owned(), true),
        ("0.0001e313".to_owned(), false),
    ];
    for (literal, in_range) in cases {
        let text = format!(r#"{{"s":{literal},"d":1}}"#);
        let data = CardData::parse(&text);
        assert_eq!(data.memory_state.is_some(), in_range, "{literal}");
        assert_eq!(data.to_text() == "{}", !in_range, "{literal}");
    }
}

/// specs/model/card-data-json.md C5, input and Meguri's output.
#[test]
fn spec_examples() {
    let cases = [
        (
            r#"{"s":123.45678,"d":1.234567,"dr":0.987654,"decay":0.123456}"#,
            r#"{"s":123.4568,"d":1.235,"dr":0.99,"decay":0.123}"#,
        ),
        (
            r#"{"zz":1,"cd":"{}","lrt":5,"decay":0.212,"dr":0.212,"d":0.212,"s":0.212,"pos":0}"#,
            r#"{"pos":0,"s":0.212,"d":0.212,"dr":0.21,"decay":0.212,"lrt":5,"zz":1}"#,
        ),
        (
            r#"{"s":2.5,"d":0.0015,"dr":0.125,"decay":0.0005}"#,
            r#"{"s":2.5,"d":0.002,"dr":0.13,"decay":0.001}"#,
        ),
        (r#"{"s":1e-7,"d":1e10}"#, r#"{"s":0.0,"d":10000000000.0}"#),
        (r#"{"s":1e40,"d":1}"#, r#"{"s":null,"d":1.0}"#),
        (r#"{"s":-0.00004,"d":-0.0004}"#, r#"{"s":-0.0,"d":-0.0}"#),
        (r#"{"s": 1 , "d" : 2}"#, r#"{"s":1.0,"d":2.0}"#),
        (r#"{"s":1.5}"#, "{}"),
        (r#"{"lrt":9223372036854775807}"#, r#"{"lrt":9223372036854775807}"#),
        (r#"{"lrt":9223372036854775808}"#, "{}"),
        (r#"{"cd":"{\"a\": 1}"}"#, r#"{"cd":"{\"a\": 1}"}"#),
        (r#"{"s":1,"s":2,"d":3}"#, "{}"),
        (r#"{"zz":1,"zz":2,"s":1,"d":2}"#, r#"{"s":1.0,"d":2.0,"zz":2}"#),
        ("garbage", "{}"),
        ("", "{}"),
        ("[1,2]", r#"{"pos":1}"#),
        ("[null,2,3]", r#"{"s":2.0,"d":3.0}"#),
        (r#"[1,2,3,4,5,6,"x",8]"#, "{}"),
        (
            r#"{"zz":{"b":1,"a":[1, 2]},"pos":1}"#,
            r#"{"pos":1,"zz":{"b":1,"a":[1, 2]}}"#,
        ),
        (r#"{"s":1.5,"zz":null}"#, r#"{"zz":null}"#),
        (r#"{"s":1e400,"d":1}"#, "{}"),
        (r#"{"pos":5,"cd":[1e400]}"#, "{}"),
        (r#"{"pos":5,"s":1,"d":1.7976931348623158e308}"#, "{}"),
        (r#"{"pos":5,"s":1e-400,"d":2}"#, r#"{"pos":5,"s":0.0,"d":2.0}"#),
        (
            r#"{"pos":5,"zz":1e400,"s":1.5,"d":2}"#,
            r#"{"pos":5,"s":1.5,"d":2.0,"zz":1e400}"#,
        ),
        ("[1,null,null,1e400]", "{}"),
    ];
    for (input, want) in cases {
        assert_eq!(CardData::parse(input).to_text(), want, "input {input}");
    }
}

#[test]
fn values_read_with_their_declared_types() {
    let data =
        CardData::parse(r#"{"pos":3,"s":0.212,"d":5,"dr":0.9,"decay":0.1542,"lrt":-5,"cd":"{\"x\":1}","q":[1]}"#);
    assert_eq!(data.original_position, Some(3));
    let memory = data.memory_state.unwrap();
    // The f32 nearest to 0.212, not a widened or rounded value.
    assert_eq!(memory.stability.to_bits(), 0.212f32.to_bits());
    assert_eq!(memory.difficulty, 5.0);
    assert_eq!(data.desired_retention, Some(0.9));
    assert_eq!(data.decay, Some(0.1542));
    assert_eq!(data.last_review_secs, Some(-5));
    assert_eq!(data.custom_data(), Some("{\"x\":1}"));
    assert_eq!(data.unknown_keys().collect::<Vec<_>>(), [("q", "[1]")]);
}

#[test]
fn written_values_are_rounded_only_in_the_text() {
    let mut data = CardData::default();
    assert_eq!(data.to_text(), "{}");
    data.memory_state = Some(MemoryState {
        stability: 11.262237,
        difficulty: 4.99951,
    });
    data.desired_retention = Some(0.875);
    data.last_review_secs = Some(1_894_701_600);
    assert_eq!(data.to_text(), r#"{"s":11.2622,"d":5.0,"dr":0.88,"lrt":1894701600}"#);
    assert_eq!(data.memory_state.unwrap().stability, 11.262237);
    // 0.212 read as an f32 and written back stays 0.212 (json-numbers N4).
    data.memory_state = Some(MemoryState {
        stability: 0.212,
        difficulty: 0.212,
    });
    data.desired_retention = None;
    data.last_review_secs = None;
    assert_eq!(data.to_text(), r#"{"s":0.212,"d":0.212}"#);
}

#[test]
fn strings_are_written_with_json_escapes_only() {
    let data = CardData::parse(r#"{"cd":"a\"b\\c\/d\u0001\u007fé\n"}"#);
    assert_eq!(data.custom_data(), Some("a\"b\\c/d\u{1}\u{7f}é\n"));
    assert_eq!(data.to_text(), "{\"cd\":\"a\\\"b\\\\c/d\\u0001\u{7f}é\\n\"}");
}

#[test]
fn an_unpaired_surrogate_makes_the_whole_text_invalid() {
    // json-numbers N9: escapes combine into surrogate pairs; an unpaired one
    // makes the text invalid, so it reads as no keys at all (C3 rule 1).
    for text in [
        r#"{"s":1,"d":2,"zz":"\ud800"}"#,
        r#"{"s":1,"d":2,"zz":["\udc00"]}"#,
        r#"{"s":1,"d":2,"zz":{"\ud800x":1}}"#,
        r#"{"s":1,"d":2,"cd":"\ud800"}"#,
    ] {
        assert_eq!(CardData::parse(text).to_text(), "{}", "{text}");
    }
    let paired = r#"{"zz":"😀 é"}"#;
    assert_eq!(CardData::parse(paired).to_text(), paired);
}
