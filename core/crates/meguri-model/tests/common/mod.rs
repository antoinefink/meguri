//! Shared by the model tests: conformance files, hex and JSON helpers.

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

pub fn conformance_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

pub fn yaml_file(rel: &str) -> Value {
    let path = conformance_dir().join(rel);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    meguri_testkit::scenario::parse_yaml(&text).unwrap()
}

/// A scenario by id, parsed (YAML 1.2 core schema).
pub fn scenario(id: &str) -> Value {
    yaml_file(&format!("scenarios/{id}.yaml"))
}

/// A golden by scenario id.
pub fn golden(id: &str) -> Value {
    let path = conformance_dir().join(format!("goldens/{id}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse_json(&text)
}

/// JSON with numbers kept as integers or floats as written.
pub fn parse_json(text: &str) -> Value {
    meguri_testkit::observe::parse_json(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// The output of step `i` of a golden.
pub fn step(golden: &Value, i: usize) -> &Value {
    &golden["observations"]["steps"][i]
}

/// The rows a `db_exec` step read.
pub fn rows(golden: &Value, i: usize) -> &Vec<Value> {
    step(golden, i)["db_exec"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("step {i} is not a db_exec read"))
}

/// One `legacy_json` object of a step.
pub fn object<'a>(golden: &'a Value, i: usize, list: &str, name: &str) -> &'a Value {
    let value = &step(golden, i)["legacy_json"][list][name];
    assert!(value.is_object(), "step {i}: no {list} object {name:?}");
    value
}

/// Upper- or lower-case hex, as SQLite's hex() and blob literals write it.
pub fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd hex length: {text}");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// RFC 7396 JSON merge patch (specs/conformance/model-ops.md L2).
pub fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Default::default());
    }
    let object = target.as_object_mut().unwrap();
    for (key, value) in patch {
        if value.is_null() {
            object.remove(key);
        } else {
            merge_patch(object.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

/// Compares two JSON values exactly, numbers by kind and value
/// (specs/conformance/observations.md O7), and lists the differences.
pub fn assert_same_json(actual: &Value, expected: &Value, what: &str) {
    let differences = meguri_testkit::diff::exact_differences(expected, actual);
    assert!(
        differences.is_empty(),
        "{what}: {differences:#?}\nactual:   {actual}\nexpected: {expected}"
    );
}
