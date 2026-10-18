use meguri_testkit::scenario::{self, ObservationKind};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn value() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/scenarios/sched/labels/units.yaml");
    scenario::parse_yaml(&fs::read_to_string(path).unwrap()).unwrap()
}
fn load(value: &Value) -> Result<scenario::LoadedScenario, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scenario.json");
    fs::write(&path, value.to_string()).unwrap();
    scenario::load(&path).map_err(|e| e.to_string())
}

#[test]
fn answer_label_observations_have_a_typed_kind_and_load_recorded_scenarios() {
    let kind: ObservationKind = ObservationKind::from_name("answer_labels").unwrap();
    assert_eq!(kind.name(), "answer_labels");
    load(&value()).unwrap();
}

#[test]
fn answer_label_observations_reject_note_references() {
    let mut value = value();
    value["steps"][0]["observe"] = json!([{"answer_labels":["#n1"]}]);
    assert!(load(&value).is_err());
}
