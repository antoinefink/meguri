//! The `text` observation kind on Meguri's executor (observations.md O9).
//! Expected values come from the spec tables in specs/text/.

use std::path::Path;

use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::scenario;
use serde_json::{Value, json};

fn run(dir: &Path, steps: &str) -> Result<Value, meguri_testkit::exec::ExecError> {
    let path = dir.join("x.yaml");
    let text = format!("id: text/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps:\n{steps}");
    std::fs::write(&path, text).unwrap();
    let loaded = scenario::load(&path).unwrap_or_else(|e| panic!("{e}"));
    MeguriExecutor::new().run(&loaded)
}

#[test]
fn requests_in_one_step_merge_by_function() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        r#"  - observe:
      - text: {fns: [checksum], inputs: ["<b>私</b>", ""]}
      - text: {fns: [field_empty, sort_field], inputs: ["&nbsp;", "<br/> <div></div>\n"]}
  - observe:
      - text: {fns: [unicase_compare], inputs: [[a, B], [Straße, STRASSE], [Z, "["]]}
      - text: {fns: [media_refs, iri_encode], inputs: []}
"#,
    )
    .unwrap();
    assert_eq!(
        out,
        json!({"steps": [
            {"text": {
                "checksum": [1270766077u32, 3661210606u32],
                "field_empty": [false, true],
                "sort_field": [" ", " \n"],
            }},
            {"text": {"unicase_compare": [-1, 0, 1], "media_refs": [], "iri_encode": []}},
        ]})
    );
}

#[test]
fn saved_functions_normalize_first_and_the_others_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        "  - observe:\n      - text: {fns: [sort_field, checksum, strip_markup, field_empty], inputs: [\"e\\u0301\\u0001\", \"<b\\rr>\"]}\n",
    )
    .unwrap();
    assert_eq!(
        out["steps"][0]["text"],
        json!({
            // As saved, the second input is `<br>`: an empty sort field,
            // with the checksum of the empty string.
            "sort_field": ["\u{e9}", ""],
            "checksum": [3205873265u32, 3661210606u32],
            "strip_markup": ["e\u{301}\u{1}", ""],
            "field_empty": [false, true],
        })
    );
}

#[test]
fn a_function_twice_in_one_step_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let err = run(
        dir.path(),
        "  - observe:\n      - text: {fns: [checksum], inputs: [a]}\n      - text: {fns: [checksum], inputs: [b]}\n",
    )
    .unwrap_err();
    assert_eq!((err.kind.as_str(), err.step), ("invalid_scenario", Some(0)));
}

#[test]
fn media_filename_refuses_names_that_lowercasing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        "  - observe:\n      - text: {fns: [media_filename], inputs: [\"con.jpg\", \"a:b?.png\"]}\n",
    )
    .unwrap();
    assert_eq!(out["steps"][0]["text"]["media_filename"], json!(["con_.jpg", "ab.png"]));
    let err = run(
        dir.path(),
        "  - observe:\n      - text: {fns: [media_filename], inputs: [\"Photo.JPG\"]}\n",
    )
    .unwrap_err();
    assert_eq!(err.kind, "invalid_scenario");
    assert!(err.detail.contains("Photo.JPG"), "{err}");
}
