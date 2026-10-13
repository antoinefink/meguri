//! The differential corpus (U20): every generated input of
//! `conformance/scenarios/text/corpus/` through meguri-text, compared with
//! the oracle's goldens. Zero mismatches, on macOS and in Linux Docker.
//!
//! The functions are composed as observations.md O8 defines them: "as saved"
//! means the field save normalization (checksum-and-sort-field.md C1, with
//! note text normalization on) comes first.

mod common;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use meguri_text::{checksum, field_empty, fold, html, iri, media_name, media_refs, normalize};
use serde_json::{Value, json};

fn evaluate(function: &str, input: &Value) -> Value {
    if function == "unicase_compare" {
        let pair = input.as_array().expect("a pair");
        let ordering = fold::compare_folded(pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        return json!(match ordering {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        });
    }
    let text = input.as_str().expect("a string input");
    let saved = || normalize::field_as_saved(text, true);
    match function {
        "strip_markup" => json!(html::strip_markup(text)),
        "strip_markup_keep_media" => json!(html::strip_markup_keep_media(text)),
        "text_line" => json!(html::text_line(text)),
        "text_line_keep_media" => json!(html::text_line_keep_media(text)),
        "media_refs" => json!(media_refs::references(text)),
        "iri_encode" => json!(iri::encode_for_display(text)),
        "iri_decode" => json!(iri::decode_for_saving(text)),
        "media_filename" => json!(media_name::safe_media_name(text)),
        "sort_field" => json!(checksum::sort_text(&saved())),
        "checksum" => json!(checksum::note_checksum(&saved())),
        "field_empty" => json!(field_empty::counts_as_empty(&saved())),
        other => panic!("unknown text function {other}"),
    }
}

#[test]
fn the_corpus_matches_the_oracle_goldens() {
    let requests = common::corpus();
    let mut inputs = 0;
    let mut checked: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatched: BTreeMap<String, usize> = BTreeMap::new();
    let mut first = Vec::new();
    for request in &requests {
        inputs += request.inputs.len();
        for function in &request.fns {
            let expected = request.expected[function.as_str()]
                .as_array()
                .unwrap_or_else(|| panic!("{}: no golden outputs for {function}", request.scenario));
            assert_eq!(expected.len(), request.inputs.len(), "{} {function}", request.scenario);
            for (index, (input, want)) in request.inputs.iter().zip(expected).enumerate() {
                *checked.entry(function.clone()).or_default() += 1;
                let got = evaluate(function, input);
                if &got != want {
                    *mismatched.entry(function.clone()).or_default() += 1;
                    if first.len() < 10 {
                        first.push(format!(
                            "{} {function} #{index}: input {input}\n  expected {want}\n  actual   {got}",
                            request.scenario
                        ));
                    }
                }
            }
        }
    }
    assert!(
        mismatched.is_empty(),
        "mismatches per function {mismatched:?} of {checked:?}; first:\n{}",
        first.join("\n")
    );
    // 48,000 field texts, 2,000 file names and 2,000 unicase pairs.
    assert!(inputs >= 52_000, "only {inputs} corpus inputs");
    for function in [
        "strip_markup",
        "strip_markup_keep_media",
        "text_line",
        "text_line_keep_media",
        "media_refs",
        "iri_encode",
        "iri_decode",
        "media_filename",
        "sort_field",
        "checksum",
        "field_empty",
        "unicase_compare",
    ] {
        assert!(
            checked.get(function).copied().unwrap_or(0) >= 2_000,
            "{function}: {checked:?}"
        );
    }
}
