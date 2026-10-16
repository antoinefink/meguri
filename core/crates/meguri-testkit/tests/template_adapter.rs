//! The `template` observation kind on Meguri's executor
//! (specs/conformance/render-ops.md Q1). Expected values come from Q1.4-Q1.6
//! and the examples of specs/render/.

use std::path::Path;

use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor};
use meguri_testkit::scenario::{self, TemplateCase, TemplateRequest};
use serde_json::{Value, json};

fn write(dir: &Path, steps: &str) -> std::path::PathBuf {
    let path = dir.join("x.yaml");
    let text = format!("id: render/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps:\n{steps}");
    std::fs::write(&path, text).unwrap();
    path
}

fn run(steps: &str) -> Result<Value, ExecError> {
    let dir = tempfile::tempdir().unwrap();
    let loaded = scenario::load(&write(dir.path(), steps)).unwrap_or_else(|e| panic!("{e}"));
    MeguriExecutor::new().run(&loaded)
}

fn load_error(steps: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    match scenario::load(&write(dir.path(), steps)) {
        Ok(_) => panic!("loaded: {steps}"),
        Err(e) => e.to_string(),
    }
}

fn request(cases: &str) -> String {
    format!(
        "  - observe:\n      - template:\n          notetype: {{name: Probe, fields: [Front, Back]}}\n          cases:\n{cases}"
    )
}

// Q1.4, Q1.5: both sides, a blank question, an error on each side.
#[test]
fn cases_evaluate_side_by_side() {
    let out = run(&request(
        r#"            - {front: "{{Front}}", back: "{{FrontSide}}<hr>{{Back}}", fields: [ein, zwei]}
            - {front: "{{Front}}", back: "{{Nope}}", fields: ["", zwei]}
            - {front: "{{Nope}}", back: "{{Back}}", fields: [ein]}
            - {front: "{{Front}}", back: "{{#Back}}", fields: [ein]}
            - {front: "{{Back}}|{{Card}}|{{Tags}}|{{c2}}", ord: 1, name: Reverse, fields: {Back: zwei}, tags: [de, b]}
"#,
    ))
    .unwrap();
    assert_eq!(
        out,
        json!({"steps": [{"template": [
            {"question": "ein", "answer": "ein<hr>zwei", "empty": false},
            // A blank question hides the answer's error (T9).
            {"question": "", "answer": null, "empty": true},
            {"error": {"kind": "unknown_field", "side": "question"}},
            {"error": {"kind": "unclosed_conditional", "side": "answer"}},
            {"question": "zwei|Reverse|de b|1", "answer": "", "empty": false},
        ]}]})
    );
}

#[test]
fn a_case_without_back_or_name_takes_the_defaults() {
    let out = run(&request(
        "            - {front: \"{{Card}}{{Deck}}{{CardFlag}}{{CardID}}\", ord: 4}\n",
    ))
    .unwrap();
    assert_eq!(
        out["steps"][0]["template"],
        json!([{"question": "Card 5(Deck)flag00", "answer": "", "empty": false}])
    );
}

#[test]
fn requests_resolve_field_values_to_field_order() {
    let parsed = scenario::parse_yaml(
        "{notetype: {name: P, fields: [A, B, C]}, cases: [{front: x, fields: {C: c, A: a}}, {front: y, fields: [a]}]}",
    )
    .unwrap();
    let request = TemplateRequest::from_mapping(parsed.as_object().unwrap()).unwrap();
    let values: Vec<Vec<String>> = request
        .cases
        .iter()
        .map(|case| match case {
            TemplateCase::NewNote(case) => case.fields.clone(),
            TemplateCase::Card { .. } => panic!("not a card case"),
        })
        .collect();
    assert_eq!(values, [vec!["a", "", "c"], vec!["a", "", ""]]);
}

// Q1.6: refused when the scenario loads.
#[test]
fn malformed_requests_are_invalid_scenarios() {
    let with = |notetype: &str, cases: &str| {
        format!("  - observe:\n      - template: {{notetype: {notetype}, cases: [{cases}]}}\n")
    };
    let nt = "{name: P, fields: [Front, Back]}";
    for (steps, why) in [
        (
            "  - observe:\n      - template: {cases: [{front: x}]}\n".to_owned(),
            "needs notetype",
        ),
        (
            format!("  - observe:\n      - template: {{notetype: {nt}, cases: []}}\n"),
            "non-empty list",
        ),
        (with(nt, "{card: \"#c1\", front: x}"), "only for new-note cases"),
        (with(nt, "{front: x, ord: 500}"), "ord is an integer from 0 to 499"),
        (with(nt, "{front: x, ord: -1}"), "ord is an integer"),
        (with(nt, "{front: x, fields: [a, b, c]}"), "3 field values for 2 fields"),
        (with(nt, "{front: x, fields: {Extra: e}}"), "not a field"),
        (with(nt, "{front: x, name: \"\"}"), "name is a non-empty string"),
        (with(nt, "{front: x, tags: [1]}"), "tags is a list of strings"),
        (with(nt, "{back: x}"), "front is required"),
        (with(nt, "{front: x, deck: D}"), "a new-note case takes"),
        (with("{name: P, fields: [Front, front]}", "{front: x}"), "unicase"),
        (with("{name: P, fields: [\"a:b\"]}", "{front: x}"), "holds one of"),
        (with("{name: P, fields: [\"#a\"]}", "{front: x}"), "starts with"),
        (with("{name: P, fields: [\" a\"]}", "{front: x}"), "starts with"),
        (
            with("{name: P, fields: [\"a \"]}", "{front: x}"),
            "ends with white space",
        ),
        (with("{name: P, fields: [\"e\\u0301\"]}", "{front: x}"), "is not NFC"),
        (
            with("{name: P, fields: []}", "{front: x}"),
            "non-empty list of field names",
        ),
        (with("{name: \"\", fields: [A]}", "{front: x}"), "must be non-empty"),
        (
            with("{name: P, kind: basic, fields: [A]}", "{front: x}"),
            "standard or cloze",
        ),
        (
            format!(
                "  - observe:\n      - template: {{notetype: {nt}, cases: [{{front: x}}]}}\n      - template: {{notetype: {nt}, cases: [{{front: y}}]}}\n"
            ),
            "more than once in one step",
        ),
    ] {
        let err = load_error(&steps);
        assert!(err.contains(why), "{steps}: {err}");
    }
}

// Q3: card cases wait for notes and cards in the executor.
#[test]
fn work_of_later_units_is_refused_not_guessed() {
    let card = "  - observe:\n      - template: {cases: [{card: \"#c1\", front: \"{{Deck}}\"}]}\n";
    assert_eq!(run(card).unwrap_err().kind, "unsupported_observation");
}

// filters.md F8, render/cloze C1, C6, C9: the cloze filters render for the
// case's card number (ord + 1) and side; on a standard note type the blank
// check is T11.2's, so an empty cloze output is no blank card (R975). The
// type-in marker for cloze runs no cloze filter (F1).
#[test]
fn cloze_filters_render_for_the_card_and_side() {
    let out = run(&request(
        r#"            - {front: "{{cloze:Front}}", back: "{{FrontSide}}|{{cloze:Front}}", fields: ["{{c1::a::h}} {{c2::b}}"]}
            - {front: "{{cloze:Front}}", back: "{{cloze:Front}}", ord: 2, fields: ["{{c1::a::h}} {{c2::b}}"]}
            - {front: "{{text:cloze-only:Front}}", back: "{{text:cloze-only:Front}}", ord: 1, fields: ["{{c2::<b>x</b>}}"]}
            - {front: "{{Front}}{{type:cloze:Back}}", fields: ["{{c1::f}}", "{{c1::g}}"]}
"#,
    ))
    .unwrap();
    let front = r#"<span class="cloze" data-cloze="a" data-ordinal="1">[h]</span> <span class="cloze-inactive" data-ordinal="2">b</span>"#;
    let back =
        r#"<span class="cloze" data-ordinal="1">a</span> <span class="cloze-inactive" data-ordinal="2">b</span>"#;
    assert_eq!(
        out["steps"][0]["template"],
        json!([
            {"question": front, "answer": format!("{front}|{back}"), "empty": false},
            {"question": "", "answer": "", "empty": false},
            {"question": "...", "answer": "x", "empty": false},
            {"question": "{{c1::f}}[[type:cloze:Back]]", "answer": "", "empty": false},
        ])
    );
}

/// A `template` request on a cloze note type named `name` with `fields`.
fn cloze_request(name: &str, fields: &str, cases: &str) -> String {
    format!(
        "  - observe:\n      - template:\n          notetype: {{name: \"{name}\", kind: cloze, fields: [{fields}]}}\n          cases:\n{cases}"
    )
}

// T10, render/cloze C15: a cloze note's card is blank when no value of its
// field map holds its cloze number (C5: nested clozes count, whatever the
// template shows).
#[test]
fn cloze_note_cards_are_blank_without_their_cloze_number() {
    let cloze = |cases: &str| cloze_request("C", "Text, Extra", cases);
    let out = run(&cloze(
        r#"            - {front: "{{cloze:Text}}", back: "{{cloze:Text}}<br>{{Extra}}", fields: ["{{c1::x}}", e]}
            - {front: "{{cloze:Text}}", back: "{{cloze:Text}}<br>{{Extra}}", ord: 1, fields: ["{{c1::x}}", e]}
            - {front: "{{cloze:Text}}", back: "{{Nope}}", ord: 1, fields: ["{{c1::x}}", "{{c2::y}}"]}
            - {front: "{{Extra}}", back: "{{cloze:Text}}", ord: 2, fields: ["{{c1::a {{c3::b}}}}", e]}
            - {front: "{{Extra}}", ord: 0, fields: ["{{c0::a {{c1::b}}}}", e]}
            - {front: "{{Extra}}", ord: 0, fields: ["{{c1::open", e]}
"#,
    ))
    .unwrap();
    assert_eq!(
        out["steps"][0]["template"],
        json!([
            {
                "question": r#"<span class="cloze" data-cloze="x" data-ordinal="1">[...]</span>"#,
                "answer": r#"<span class="cloze" data-ordinal="1">x</span><br>e"#,
                "empty": false,
            },
            {"question": "", "answer": null, "empty": true},
            // Extra holds c2, so the card is not blank and its answer errs.
            {"error": {"kind": "unknown_field", "side": "answer"}},
            {
                "question": "e",
                "answer": r#"<span class="cloze-inactive" data-ordinal="1">a <span class="cloze" data-ordinal="3">b</span></span>"#,
                "empty": false,
            },
            {"question": "e", "answer": "", "empty": false},
            {"question": "e", "answer": null, "empty": true},
        ])
    );
}

// render/cloze C15: special values count like fields (Tags, the note type
// name, the template name), and a note field named like a special replaces
// it in the check too.
#[test]
fn special_values_count_in_the_cloze_blank_check() {
    let out = run(&cloze_request(
        "Gap {{c5::type}}",
        "Text, Extra",
        r#"            - {front: "{{cloze:Text}}", back: "{{Tags}}", ord: 2, fields: ["{{c1::a}}", ""], tags: ["{{c3::t}}"]}
            - {front: "{{cloze:Text}}", back: "{{Card}}", ord: 3, name: "{{c4::card}}", fields: ["{{c1::a}}", ""]}
            - {front: "{{cloze:Text}}", back: "{{Type}}", ord: 4, fields: ["{{c1::a}}", ""]}
            - {front: "{{cloze:Text}}", back: "{{Type}}", ord: 5, fields: ["{{c1::a}}", ""]}
            - {front: "{{cloze:Text}}", back: "{{Deck}}|{{CardFlag}}|{{CardID}}|{{c2}}", ord: 1, fields: ["{{c1::a}}", ""]}
"#,
    ))
    .unwrap();
    assert_eq!(
        out["steps"][0]["template"],
        json!([
            {"question": "", "answer": "{{c3::t}}", "empty": false},
            {"question": "", "answer": "{{c4::card}}", "empty": false},
            {"question": "", "answer": "Gap {{c5::type}}", "empty": false},
            {"question": "", "answer": null, "empty": true},
            // (Deck), flag0, 0 and 1 hold no cloze.
            {"question": "", "answer": null, "empty": true},
        ])
    );
    let shadowed = run(&cloze_request(
        "Gap",
        "Text, Tags, Card",
        r#"            - {front: "{{cloze:Text}}", back: "{{Tags}}", ord: 2, fields: ["{{c1::a}}", "", ""], tags: ["{{c3::t}}"]}
            - {front: "{{cloze:Text}}", back: "{{Tags}}", ord: 2, fields: ["{{c1::a}}", "{{c3::f}}", ""], tags: [t]}
            - {front: "{{cloze:Text}}", back: "{{Card}}", ord: 3, name: "{{c4::card}}", fields: ["{{c1::a}}", "", ""]}
"#,
    ))
    .unwrap();
    assert_eq!(
        shadowed["steps"][0]["template"],
        json!([
            {"question": "", "answer": null, "empty": true},
            {"question": "", "answer": "{{c3::f}}", "empty": false},
            {"question": "", "answer": null, "empty": true},
        ])
    );
}
