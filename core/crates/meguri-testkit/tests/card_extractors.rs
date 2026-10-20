//! Frozen U82a corpus through the actual observation provider (E1-E5).
use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::observe::parse_json;
use meguri_testkit::scenario::{self, Scenario};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn play(name: &str) {
    let id = format!("search/extractors/{name}");
    let loaded = scenario::load(&root().join(format!("scenarios/{id}.yaml"))).unwrap();
    let golden = parse_json(&std::fs::read_to_string(root().join(format!("goldens/{id}.json"))).unwrap()).unwrap();
    assert_eq!(golden["scenario_sha256"], json!(loaded.sha256));
    // Deliberately omit storage: a pure extractor observation needs no
    // collection, bootstrap, clock query or fixture implementation.
    let executor = MeguriExecutor::with(MeguriExecutor::area_registry(&["search"]));
    let actual = executor.run(&loaded).unwrap();
    let comparison = diff::compare(&id, &golden["observations"], &actual, &CompareOptions::default());
    assert!(comparison.differences.is_empty(), "{id}: {:#?}", comparison.differences);
}

#[test]
fn card_data() {
    play("card-data");
}
#[test]
fn arguments() {
    play("arguments");
}
#[test]
fn retrievability() {
    play("retrievability");
}
#[test]
fn relative() {
    play("relative");
}
#[test]
fn day_edges() {
    play("day-edges");
}

#[test]
fn tolerance_is_exactly_e4_and_only_for_r_rr() {
    for (field, expected) in [
        ("r", Some(1e-5)),
        ("rr", Some(1e-5)),
        ("s", None),
        ("d", None),
        ("dr", None),
        ("pos", None),
        ("id", None),
    ] {
        let pointer: Vec<String> = ["steps", "0", "card_extractors", "rows", "0", field]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(meguri_testkit::observe::tolerance(&pointer), expected);
    }
}

fn request(value: Value) -> Result<Scenario, scenario::FormatError> {
    Scenario::from_value(
        &json!({"id":"search/validation","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"observe":[{"card_extractors":value}]}]}),
    )
}

#[test]
fn observation_input_validation_matches_e1_e2() {
    let row = json!({"id":"x","data":"{}","due":35,"ivl":10,"today":30,"now":1897293600});
    assert!(request(json!({"inputs":[row.clone()]})).is_ok());
    for invalid in [
        json!({}),
        json!({"inputs":[]}),
        json!({"inputs":[row.clone(),row.clone()]}),
        json!({"inputs":[row.clone()],"extra":0}),
        json!({"inputs":vec![row.clone();1001]}),
    ] {
        assert!(request(invalid).is_err());
    }
    for value in [
        json!(true),
        json!([]),
        json!({"int":"9223372036854775808"}),
        json!({"int":"+1"}),
        json!({"int":"1e2"}),
        json!({"text_hex":"f"}),
        json!({"blob_hex":"gg"}),
        json!({"int":"1","extra":0}),
    ] {
        let mut bad = row.clone();
        bad["due"] = value;
        assert!(request(json!({"inputs":[bad]})).is_err());
    }
    for value in [
        json!({"int":"-9223372036854775808"}),
        json!({"int":"9223372036854775807"}),
        json!({"text_hex":"ff"}),
        json!({"blob_hex":""}),
        json!(35.0),
        json!(null),
    ] {
        let mut valid = row.clone();
        valid["data"] = value;
        assert!(request(json!({"inputs":[valid]})).is_ok());
    }
    let duplicate = json!({"id":"search/validation","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"observe":[{"card_extractors":{"inputs":[row.clone()]}},{"card_extractors":{"inputs":[row]}}]}]});
    assert!(Scenario::from_value(&duplicate).is_err());
}
