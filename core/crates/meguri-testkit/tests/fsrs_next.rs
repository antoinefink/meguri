//! The `fsrs_next` observation (specs/conformance/float-determinism.md F3,
//! F5): what the scenario model accepts, Meguri's adapter against the
//! oracle's values, and its comparison tolerances (observations.md O7).

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::observe;
use meguri_testkit::scenario;
use serde_json::{Value, json};

fn repo_conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn edges_value() -> Value {
    let text = fs::read_to_string(repo_conformance().join("scenarios/fp-probe/next-states-edges.yaml")).unwrap();
    scenario::parse_yaml(&text).unwrap()
}

/// Loads a scenario value through the scenario loader, as T0 does.
fn load(value: &Value) -> Result<scenario::LoadedScenario, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.json");
    fs::write(&path, serde_json::to_string(value).unwrap()).unwrap();
    scenario::load(&path).map_err(|e| e.to_string())
}

#[test]
fn row_4_of_the_edges_equals_the_next_states_of_the_smoke_golden_at_step_6() {
    // F3.5: input 4 is the review card of smoke/fsrs-new-card step 6.
    let loaded = load(&edges_value()).unwrap();
    let observations = MeguriExecutor::new().run(&loaded).unwrap();
    let row = observations
        .pointer("/steps/0/fsrs_next/rows/4")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let golden =
        observe::parse_json(&fs::read_to_string(repo_conformance().join("goldens/smoke/fsrs-new-card.json")).unwrap())
            .unwrap();
    let states = golden.pointer("/observations/steps/6/next_states/#c1").unwrap();
    let mut expected = Vec::new();
    for (rating, days) in [
        ("again", "/review/scheduled_days"),
        ("hard", "/scheduled_days"),
        ("good", "/scheduled_days"),
        ("easy", "/scheduled_days"),
    ] {
        let memory = if rating == "again" {
            "/review/memory_state"
        } else {
            "/memory_state"
        };
        expected.push(states.pointer(&format!("/{rating}{days}")).unwrap().clone());
        expected.push(states.pointer(&format!("/{rating}{memory}/stability")).unwrap().clone());
        expected.push(
            states
                .pointer(&format!("/{rating}{memory}/difficulty"))
                .unwrap()
                .clone(),
        );
    }
    assert_eq!(row, expected);
}

#[test]
fn every_row_has_twelve_values_days_as_integers_and_memory_as_f32_text() {
    let loaded = load(&edges_value()).unwrap();
    let observations = MeguriExecutor::new().run(&loaded).unwrap();
    let rows = observations
        .pointer("/steps/0/fsrs_next/rows")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 24);
    for row in rows {
        let row = row.as_array().unwrap();
        assert_eq!(row.len(), observe::FSRS_NEXT_COLUMNS);
        for (column, value) in row.iter().enumerate() {
            if column.is_multiple_of(3) {
                assert!(value.as_u64().is_some_and(|d| d >= 1), "{row:?}");
            } else {
                let x = value.as_f64().unwrap();
                assert!(value.as_number().unwrap().is_f64(), "{row:?}");
                assert_eq!(observe::f32_value(x as f32).unwrap(), *value);
            }
        }
    }
    // Input 5 has the maximum interval 1: Hard, Good and Easy are 1 day.
    let five: Vec<u64> = [3, 6, 9].iter().map(|c| rows[5][c].as_u64().unwrap()).collect();
    assert_eq!(five, [1, 1, 1]);
}

fn with_request(edit: impl FnOnce(&mut Value)) -> Result<scenario::LoadedScenario, String> {
    let mut value = edges_value();
    edit(value.pointer_mut("/steps/0/observe/0/fsrs_next").unwrap());
    load(&value)
}

#[test]
fn requests_outside_f5_1_make_the_scenario_invalid() {
    assert!(with_request(|_| {}).is_ok());
    type Edit = Box<dyn Fn(&mut Value)>;
    let breaks: Vec<(&str, Edit)> = vec![
        ("extra member", Box::new(|r| r["extra"] = json!(1))),
        ("no parameter set", Box::new(|r| r["param_sets"] = json!({}))),
        (
            "18 values",
            Box::new(|r| r["param_sets"]["fsrs4-a"] = json!(vec![0.5; 18])),
        ),
        (
            "upper-case name",
            Box::new(|r| {
                let set = r["param_sets"]["default"].take();
                r["param_sets"]["Default"] = set;
            }),
        ),
        ("unknown set", Box::new(|r| r["inputs"][0]["params"] = json!("fsrs9"))),
        ("no inputs", Box::new(|r| r["inputs"] = json!([]))),
        (
            "1,001 inputs",
            Box::new(|r| {
                let first = r["inputs"][0].clone();
                r["inputs"] = Value::Array(vec![first; 1001]);
            }),
        ),
        (
            "missing member",
            Box::new(|r| {
                r["inputs"][0].as_object_mut().unwrap().remove("max_ivl");
            }),
        ),
        ("unknown member", Box::new(|r| r["inputs"][0]["lrt"] = json!(1))),
        ("s below 0.001", Box::new(|r| r["inputs"][0]["s"] = json!(0.0009))),
        ("d above 10", Box::new(|r| r["inputs"][0]["d"] = json!(10.5))),
        ("dr of 1", Box::new(|r| r["inputs"][0]["dr"] = json!(1))),
        (
            "elapsed days written as 1.0",
            Box::new(|r| r["inputs"][0]["elapsed_days"] = json!(1.0)),
        ),
        (
            "reps above 4,294,967,294",
            Box::new(|r| r["inputs"][0]["reps"] = json!(4_294_967_295u64)),
        ),
        ("card id 0", Box::new(|r| r["inputs"][0]["card_id"] = json!(0))),
        (
            "scheduled days 0",
            Box::new(|r| r["inputs"][0]["scheduled_days"] = json!(0)),
        ),
        (
            "max_ivl above 36,500",
            Box::new(|r| r["inputs"][0]["max_ivl"] = json!(36501)),
        ),
    ];
    for (what, edit) in breaks {
        let error = with_request(edit).err().unwrap_or_else(|| panic!("{what} loaded"));
        assert!(error.contains("fsrs_next"), "{what}: {error}");
    }
    // Integers and fractions both read as f32 (json-numbers N2).
    assert!(with_request(|r| r["inputs"][0]["s"] = json!(36500)).is_ok());
    assert!(with_request(|r| r["inputs"][0]["d"] = json!(1)).is_ok());
}

#[test]
fn an_observe_step_holds_at_most_one_fsrs_next() {
    let mut value = edges_value();
    let request = value.pointer("/steps/0/observe/0").unwrap().clone();
    value["steps"][0]["observe"] = json!([request.clone(), request]);
    let error = load(&value).err().unwrap();
    assert!(error.contains("at most once"), "{error}");
    // In two steps it is fine, and it needs no collection.
    let mut value = edges_value();
    let step = value["steps"][0].clone();
    value["steps"] = json!([step.clone(), {"advance": "1d"}, step]);
    let loaded = load(&value).unwrap();
    let observations = MeguriExecutor::new().run(&loaded).unwrap();
    assert_eq!(observations["steps"][0], observations["steps"][2]);
}

#[test]
fn memory_columns_have_the_memory_state_tolerance_and_days_none() {
    let rows = |days: u64, s: f64| json!({"steps": [{"fsrs_next": {"rows": [[1, 0.6369821, 7.3921676, 10, 9.240417, 4.7481427, days, s, 2.1041174, 26, 23.900333, 1.0]]}}]});
    let plain = CompareOptions::default();
    let expected = rows(15, 13.836124);
    assert!(
        diff::compare("fp/x", &expected, &rows(15, 13.836124 * (1.0 + 5e-6)), &plain)
            .differences
            .is_empty()
    );
    let far = diff::compare("fp/x", &expected, &rows(15, 13.836124 * (1.0 + 2e-5)), &plain);
    assert_eq!(far.differences[0].pointer, "/steps/0/fsrs_next/rows/0/7");
    let day = diff::compare("fp/x", &expected, &rows(16, 13.836124), &plain);
    assert_eq!(day.differences[0].pointer, "/steps/0/fsrs_next/rows/0/6");
}
