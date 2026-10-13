//! `meguri-cli conformance` through its binary (specs/conformance/dsl.md D8,
//! D12), on the committed conformance tree.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn capabilities_list_what_meguri_runs_and_the_whole_language() {
    let out = cli(&["conformance", "capabilities"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["supports"]["ops"], serde_json::json!(["advance", "set_clock"]));
    assert_eq!(json["language"]["ops"].as_array().unwrap().len(), 18);
    assert!(
        json["language"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "next_states")
    );
}

#[test]
fn exec_prints_observations_and_reports_pending_scenarios() {
    let empty = conformance().join("scenarios/smoke/runner-empty.yaml");
    let out = cli(&["conformance", "exec", empty.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "{\n  \"steps\": []\n}\n");

    let fsrs = conformance().join("scenarios/smoke/fsrs-new-card.yaml");
    let out = cli(&["conformance", "exec", fsrs.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("smoke/fsrs-new-card: pending (needs ops: answer"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn diff_and_run_compare_with_the_goldens() {
    let dir = conformance();
    let empty = dir.join("scenarios/smoke/runner-empty.yaml");
    let out = cli(&[
        "conformance",
        "diff",
        "--conformance",
        dir.to_str().unwrap(),
        empty.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", text(&out.stdout));
    assert_eq!(text(&out.stdout), "smoke/runner-empty: pass [active]\n");

    let report = tempfile::tempdir().unwrap();
    let path = report.path().join("t0.json");
    let out = cli(&[
        "conformance",
        "run",
        "--conformance",
        dir.to_str().unwrap(),
        "--report",
        path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let json: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["failed"], 0);
    assert_eq!(json["totals"]["pass"], 1);
    assert!(json["scenarios"].as_array().unwrap().len() >= 8);

    // A filter that matches nothing is a failure, not a silent pass.
    let out = cli(&[
        "conformance",
        "run",
        "--conformance",
        dir.to_str().unwrap(),
        "sched/nothing",
    ]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn to_json_prints_parsed_values_and_flags_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("a.yaml");
    let bad = dir.path().join("b.yaml");
    std::fs::write(&good, "a: 017\nb: yes\n").unwrap();
    std::fs::write(&bad, "a: &x 1\nb: *x\n").unwrap();
    let out = cli(&["conformance", "to-json", good.to_str().unwrap(), bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let lines: Vec<Value> = text(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["value"], serde_json::json!({"a": 17, "b": "yes"}));
    assert!(lines[1]["error"].as_str().unwrap().contains("alias"));
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(cli(&["conformance", "frobnicate"]).status.code(), Some(2));
    assert_eq!(cli(&["conformance", "run", "--jobs"]).status.code(), Some(2));
    assert_eq!(cli(&[]).status.code(), Some(2));
}
