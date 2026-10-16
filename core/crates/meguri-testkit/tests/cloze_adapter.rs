//! The `cloze` observation kind on Meguri's executor
//! (specs/conformance/cloze-ops.md Z1, Z2). Expected values come from the
//! tables of specs/render/cloze.md.

use std::path::Path;

use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor};
use meguri_testkit::scenario::{self, ClozeFunction, ClozeInputs, ClozeRequest};
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

fn observe(request: &str) -> String {
    format!("  - observe:\n      - cloze: {request}\n")
}

// Z1, Z2: one array per function, one output per input in input order; the
// outputs are C6's, C9's and C10's.
#[test]
fn every_function_runs_on_every_input() {
    let out = run(&observe(
        r#"{fns: [front, back, only_front, only_back, typing], inputs: [["{{c1::Paris::capital}} is in {{c2::France}}", 1], ["{{c1,2::x}}", 2], ["{{c2::x}}", 1]]}"#,
    ))
    .unwrap();
    assert_eq!(
        out["steps"][0]["cloze"],
        json!({
            "front": [
                r#"<span class="cloze" data-cloze="Paris" data-ordinal="1">[capital]</span> is in <span class="cloze-inactive" data-ordinal="2">France</span>"#,
                r#"<span class="cloze" data-cloze="x" data-ordinal="1,2">[...]</span>"#,
                "",
            ],
            "back": [
                r#"<span class="cloze" data-ordinal="1">Paris</span> is in <span class="cloze-inactive" data-ordinal="2">France</span>"#,
                r#"<span class="cloze" data-ordinal="1,2">x</span>"#,
                "",
            ],
            "only_front": ["capital", "...", ""],
            "only_back": ["Paris", "x", ""],
            "typing": ["Paris", "x", ""],
        })
    );
}

// C5: numbers take strings, ascending, 0 left out; requests of one step
// with different functions merge (O2).
#[test]
fn numbers_take_strings_and_requests_merge() {
    let out = run(r#"  - observe:
      - cloze: {fns: [numbers], inputs: ["{{c1::a}}{{c3::b}}{{c0::l}}{{c700::z}}", "{{c0,3,4::x}}", "{{c1::open", ""]}
      - cloze: {fns: [typing], inputs: [["{{c1::a}} {{c1::a}} {{c1::b}}", 1]]}
"#)
    .unwrap();
    assert_eq!(
        out,
        json!({"steps": [{"cloze": {
            "numbers": [[1, 3, 700], [3, 4], [], []],
            "typing": ["a, a, b"],
        }}]})
    );
}

#[test]
fn an_empty_input_list_gives_empty_outputs() {
    let out = run(&observe("{fns: [front], inputs: []}")).unwrap();
    assert_eq!(out["steps"][0]["cloze"], json!({"front": []}));
}

// Z1: a function requested twice in one step names the same outputs twice.
#[test]
fn a_function_twice_in_one_step_is_invalid() {
    let err = run(
        "  - observe:\n      - cloze: {fns: [front], inputs: [[a, 1]]}\n      - cloze: {fns: [front, back], inputs: [[b, 1]]}\n",
    )
    .unwrap_err();
    assert_eq!((err.kind.as_str(), err.step), ("invalid_scenario", Some(0)));
}

// Z1 errors: every malformed request is refused when the scenario loads.
#[test]
fn malformed_requests_are_invalid_scenarios() {
    for (request, why) in [
        ("{fns: [front]}", "exactly {fns, inputs}"),
        ("{fns: [front], inputs: [], extra: 1}", "exactly {fns, inputs}"),
        ("{fns: [], inputs: []}", "at least one function"),
        ("{fns: front, inputs: []}", "list of function names"),
        ("{fns: [render], inputs: []}", "not a cloze function"),
        ("{fns: [back, back], inputs: []}", "appears twice"),
        ("{fns: [numbers, front], inputs: []}", "no other function"),
        ("{fns: [numbers], inputs: [[a, 1]]}", "numbers inputs are strings"),
        ("{fns: [front], inputs: a}", "inputs is a list"),
        ("{fns: [front], inputs: [a]}", "pairs"),
        ("{fns: [front], inputs: [[a]]}", "pairs"),
        ("{fns: [front], inputs: [[a, 1, 2]]}", "pairs"),
        ("{fns: [front], inputs: [[1, 1]]}", "pairs"),
        ("{fns: [front], inputs: [[a, 0]]}", "from 1 to 65535"),
        ("{fns: [front], inputs: [[a, 65536]]}", "from 1 to 65535"),
        ("{fns: [front], inputs: [[a, -1]]}", "from 1 to 65535"),
        ("{fns: [front], inputs: [[a, 1.0]]}", "from 1 to 65535"),
        ("{fns: [front], inputs: [[a, \"1\"]]}", "from 1 to 65535"),
        ("{fns: [front], inputs: [[a, true]]}", "from 1 to 65535"),
        ("{fns: [front], inputs: [[a, null]]}", "from 1 to 65535"),
        ("[\"#c1\"]", "does not take these arguments"),
    ] {
        let error = load_error(&observe(request));
        assert!(error.contains(why), "{request}: {error}");
    }
}

#[test]
fn requests_read_card_numbers_to_the_top_of_the_range() {
    let parsed =
        scenario::parse_yaml(r#"{fns: [typing, only_back], inputs: [["{{c65535::x}}", 65535], [y, 1]]}"#).unwrap();
    let request = ClozeRequest::from_mapping(parsed.as_object().unwrap()).unwrap();
    assert_eq!(request.fns, [ClozeFunction::Typing, ClozeFunction::OnlyBack]);
    assert_eq!(
        request.inputs,
        ClozeInputs::Cards(vec![("{{c65535::x}}".to_owned(), 65535), ("y".to_owned(), 1)])
    );
    let out = run(&observe(r#"{fns: [typing], inputs: [["{{c65535::x}}", 65535]]}"#)).unwrap();
    assert_eq!(out["steps"][0]["cloze"]["typing"], json!(["x"]));
}

// Z4: the render area offers the kind, so its scenarios are runnable.
#[test]
fn the_executor_offers_the_kind() {
    assert!(
        MeguriExecutor::new().capabilities().observations.contains("cloze"),
        "the render area registers cloze"
    );
}
