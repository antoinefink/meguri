//! The `render/cloze` goldens, checked against the cloze functions directly
//! (`specs/conformance/cloze-ops.md` Z2): every requested function on every
//! input, compared with the oracle's output string for string.
//!
//! T0 runs these scenarios once the testkit has the `cloze` observation
//! kind; drop this file then. A golden whose scenario hash differs from the
//! scenario's is refused, so an edited scenario cannot pass on old outputs.

use std::path::{Path, PathBuf};

use meguri_template::cloze::{self, Side};
use meguri_testkit::scenario::{parse_json, parse_yaml, scenario_sha256};
use serde_json::{Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Z2: one function on one input.
fn evaluate(function: &str, input: &Value) -> Value {
    if function == "numbers" {
        let text = input.as_str().expect("numbers takes strings");
        return json!(cloze::numbers(text).into_iter().collect::<Vec<u16>>());
    }
    let [Value::String(text), card] = input.as_array().expect("[text, card] pair").as_slice() else {
        panic!("{input} is not a [text, card] pair");
    };
    let card = card
        .as_u64()
        .and_then(|card| u32::try_from(card).ok())
        .filter(|card| (1..=65535).contains(card))
        .unwrap_or_else(|| panic!("{card} is not a card number"));
    json!(match function {
        "front" => cloze::render(text, card, Side::Front),
        "back" => cloze::render(text, card, Side::Back),
        "only_front" => cloze::render_only(text, card, Side::Front),
        "only_back" => cloze::render_only(text, card, Side::Back),
        "typing" => cloze::type_answer(text, card),
        other => panic!("{other} is not a cloze function"),
    })
}

/// Every mismatch of one scenario, as `function input: expected / actual`.
fn mismatches(scenario: &Value, golden: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let steps = scenario["steps"].as_array().expect("steps");
    for (index, step) in steps.iter().enumerate() {
        let expected = &golden["observations"]["steps"][index]["cloze"];
        for request in step["observe"].as_array().expect("an observe step") {
            let request = &request["cloze"];
            let inputs = request["inputs"].as_array().expect("inputs");
            for function in request["fns"].as_array().expect("fns") {
                let function = function.as_str().expect("a function name");
                let outputs = expected[function]
                    .as_array()
                    .expect("one golden output list per function");
                assert_eq!(
                    outputs.len(),
                    inputs.len(),
                    "step {index} {function}: one output per input"
                );
                for (input, want) in inputs.iter().zip(outputs) {
                    let got = evaluate(function, input);
                    if &got != want {
                        found.push(format!(
                            "step {index} {function} {input}:\n  oracle {want}\n  meguri {got}"
                        ));
                    }
                }
            }
        }
    }
    found
}

#[test]
fn every_render_cloze_golden_matches() {
    let root = conformance();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("scenarios/render/cloze"))
        .expect("scenarios/render/cloze")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "yaml"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 11, "render/cloze has the eleven scenarios of cloze.md C14");

    let mut failures = Vec::new();
    for path in &paths {
        let scenario = parse_yaml(&read(path)).expect("a scenario");
        let id = scenario["id"].as_str().expect("an id");
        let golden = parse_json(&read(&root.join(format!("goldens/{id}.json")))).expect("a golden");
        let sha256 = scenario_sha256(&scenario).expect("a scenario hash");
        assert_eq!(golden["scenario_sha256"], json!(sha256), "{id}: the golden is stale");
        failures.extend(mismatches(&scenario, &golden).into_iter().map(|m| format!("{id} {m}")));
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
