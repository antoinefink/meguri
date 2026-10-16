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

// Q3: card cases wait for notes and cards in the executor; cloze note types
// and cloze filters for U61.
#[test]
fn work_of_later_units_is_refused_not_guessed() {
    let card = "  - observe:\n      - template: {cases: [{card: \"#c1\", front: \"{{Deck}}\"}]}\n";
    assert_eq!(run(card).unwrap_err().kind, "unsupported_observation");
    let cloze = "  - observe:\n      - template:\n          notetype: {name: C, kind: cloze, fields: [Text]}\n          cases: [{front: \"{{Text}}\"}]\n";
    assert_eq!(run(cloze).unwrap_err().kind, "unsupported_observation");
    for template in ["{{cloze:Front}}", "{{text:cloze-only:Front}}"] {
        let steps = request(&format!("            - {{front: \"{template}\"}}\n"));
        assert_eq!(run(&steps).unwrap_err().kind, "unsupported_observation", "{template}");
    }
    // The type-in marker for cloze runs no cloze filter (filters.md F1).
    let typed = run(&request(
        "            - {front: \"{{Front}}{{type:cloze:Back}}\", fields: [f]}\n",
    ))
    .unwrap();
    assert_eq!(typed["steps"][0]["template"][0]["question"], "f[[type:cloze:Back]]");
}
