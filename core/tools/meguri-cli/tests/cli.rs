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
    assert_eq!(json["language"]["ops"].as_array().unwrap().len(), 19);
    assert!(
        json["language"]["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "handoff")
    );
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
    assert_eq!(json["tier"], "t0");
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

const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";

/// A temporary tree with the committed timing-tz-utc scenario and golden,
/// inactive.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, from) in [
        ("oracle.json", "oracle.json"),
        ("deviations.yaml", "deviations.yaml"),
        (
            "scenarios/smoke/timing-tz-utc.yaml",
            "scenarios/smoke/timing-tz-utc.yaml",
        ),
        ("goldens/smoke/timing-tz-utc.json", "goldens/smoke/timing-tz-utc.json"),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(conformance().join(from), path).unwrap();
    }
    dir
}

/// A stand-in oracle worker that answers every run with `observations`.
fn worker(dir: &Path, observations: &str, lock: &str) -> String {
    let script = dir.join("worker.sh");
    std::fs::write(
        &script,
        format!(
            r#"while IFS= read -r line; do
  case "$line" in
    *'"cmd":"hello"'*) printf '{{"ok":true,"protocol":1,"anki_version":"26.9.3","lock_sha256":"{lock}","driver_sha256":"%s","tz":"%s"}}\n' "$(printf 'b%.0s' $(seq 64))" "$TZ" ;;
    *) printf '%s\n' '{{"ok":true,"scenario_id":"x","elapsed_ms":1,"observations":{observations}}}' ;;
  esac
done
"#
        ),
    )
    .unwrap();
    serde_json::to_string(&["env", "TZ={tz}", "sh", script.to_str().unwrap()]).unwrap()
}

fn run_live(root: &Path, cmd: &str) -> (Option<i32>, Value) {
    let report = root.join("t1.json");
    let out = cli(&[
        "conformance",
        "run",
        "--live",
        "--conformance",
        root.to_str().unwrap(),
        "--oracle-cmd",
        cmd,
        "--report",
        report.to_str().unwrap(),
    ]);
    let json = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    (out.status.code(), json)
}

#[test]
fn run_live_fails_on_oracle_drift_or_errors_in_scenarios_that_are_not_active() {
    let dir = tree();
    let root = dir.path();
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("goldens/smoke/timing-tz-utc.json")).unwrap()).unwrap();
    let recorded = serde_json::to_string(&golden["observations"]).unwrap();

    let (code, json) = run_live(root, &worker(root, &recorded, LOCK));
    assert_eq!(code, Some(0), "{json}");
    assert_eq!(
        (json["tier"].as_str(), json["scenarios"][0]["active"].as_bool()),
        (Some("t1"), Some(false))
    );
    assert_eq!(json["oracle"]["lock_sha256"], LOCK);

    // The oracle no longer reproduces the golden of an inactive scenario.
    let drifted = recorded.replace("\"now\":", "\"now\":1,\"was\":");
    assert_ne!(drifted, recorded);
    let (code, json) = run_live(root, &worker(root, &drifted, LOCK));
    assert_eq!(code, Some(1), "{json}");
    assert_eq!(json["scenarios"][0]["status"], "fail");
    assert_eq!(json["scenarios"][0]["fails"], true);

    // Workers of another oracle than the pin: every scenario is an error.
    let (code, json) = run_live(root, &worker(root, &recorded, &"0".repeat(64)));
    assert_eq!(code, Some(1), "{json}");
    assert_eq!(json["scenarios"][0]["status"], "error");
    assert!(
        json["scenarios"][0]["message"]
            .as_str()
            .unwrap()
            .contains("oracle_mismatch")
    );
}

#[test]
fn run_fails_on_an_inactive_scenario_that_breaks_the_schema() {
    let dir = tree();
    let root = dir.path();
    let path = root.join("scenarios/sched/filtered.yaml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "id: sched/filtered\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{build_filtered: {}}]\n",
    )
    .unwrap();
    let out = cli(&["conformance", "run", "--conformance", root.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("sched/filtered: invalid"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn goldens_check_lists_each_changed_value_with_before_and_after() {
    let dir = tree();
    let root = dir.path();
    let base = tempfile::tempdir().unwrap();
    let golden_path = root.join("goldens/smoke/timing-tz-utc.json");
    let mut golden: Value = serde_json::from_str(&std::fs::read_to_string(&golden_path).unwrap()).unwrap();
    std::fs::create_dir_all(base.path().join("smoke")).unwrap();
    std::fs::write(base.path().join("smoke/timing-tz-utc.json"), golden.to_string()).unwrap();
    let before = golden["observations"]["steps"][0]["timing"]["now"].clone();
    golden["observations"]["steps"][0]["timing"]["now"] = serde_json::json!(7);
    std::fs::write(&golden_path, golden.to_string()).unwrap();
    let report = root.join("fresh.json");
    let out = cli(&[
        "conformance",
        "goldens-check",
        "--conformance",
        root.to_str().unwrap(),
        "--base",
        base.path().to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains(&format!("smoke/timing-tz-utc /steps/0/timing/now: {before} -> 7")),
        "{stderr}"
    );
    let json: Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let value = &json["changed"][0]["values"][0];
    assert_eq!(
        (value["before"].clone(), value["after"].clone()),
        (before, serde_json::json!(7))
    );
    assert_eq!(json["changed"][0]["count"], 1);
}
