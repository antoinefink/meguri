use meguri_testkit::{
    exec::MeguriExecutor,
    suite::{Status, Suite},
};
use std::path::Path;

#[test]
fn every_media_golden_runs_and_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let files = suite.discover(&["media".into()]).unwrap();
    assert!(files.len() >= 13);
    let mut failures = Vec::new();
    for file in files {
        let outcome = suite.check(&file, &executor);
        if outcome.status != Status::Pass {
            failures.push(outcome.render_text());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn media_model_rejects_shapes_and_names_the_schema_forbids() {
    use meguri_testkit::scenario::Scenario;
    use serde_json::json;
    let base = json!({"id":"media/model", "clock":{"tz":"UTC", "start":"2030-01-15T10:00:00"}});
    let mut invalid = Vec::new();
    for step in [
        json!({"add_media":{"name":"x.png","fill":[1,2]}}),
        json!({"media_outside":[["x.png", "text", null, null, null]]}),
        json!({"media_outside":[["x.png", "text"]]}),
        json!({"add_media":{"name":"x.png","text":"x","fill":{"size":1}}}),
        json!({"add_media":{"name":"x.png","fill":null}}),
        json!({"add_media":{"name":"x.png","fill":{"size":209715201}}}),
        json!({"media_outside":[{"name":"../x","text":"x"}]}),
        json!({"media_outside":[]}),
        json!({"media_outside":[{"name":"x","dir":false}]}),
    ] {
        let mut value = base.clone();
        value["steps"] = json!([step]);
        invalid.push(value);
    }
    for setup in [
        json!({"media_files":null}),
        json!({"media_files":[["x.png", "text", null, null, null]]}),
        json!({"media_files":[["x.png", "text"]]}),
        json!({"media_files":[{"name":"Cat.png","text":"x"},{"name":"cat.png","text":"y"}]}),
        json!({"media_files":[{"name":"é.png","text":"x"},{"name":"e\u{301}.png","text":"y"}]}),
    ] {
        let mut value = base.clone();
        value["setup"] = setup;
        invalid.push(value);
    }
    let accepted: Vec<_> = invalid
        .into_iter()
        .filter(|value| Scenario::from_value(value).is_ok())
        .collect();
    assert!(accepted.is_empty(), "invalid media scenarios accepted: {accepted:?}");
}

#[test]
fn media_sync_vocabulary_requires_exact_args_and_its_own_prior_login() {
    use meguri_testkit::scenario::{ObservationKind, Scenario, Step};
    use serde_json::{Value, json};
    let scenario = |steps: Value, actors: Option<Value>| {
        let mut value = json!({"id":"media/model", "clock":{"tz":"UTC", "start":"2030-01-15T10:00:00"}, "steps":steps});
        if let Some(actors) = actors {
            value["actors"] = actors;
        }
        Scenario::from_value(&value)
    };
    let actors = Some(json!(["client_a", "meguri"]));
    let login = json!({"sync":{"kind":"login"}});
    for op in ["media_reset", "media_sync"] {
        for bad in [
            Value::Null,
            json!([]),
            json!([{}]),
            json!(true),
            json!(""),
            json!({"extra":true}),
        ] {
            assert!(scenario(json!([login, {op:bad}]), actors.clone()).is_err(), "{op}");
        }
    }
    let parsed = scenario(json!([{"media_reset":{}}, {"observe":["media_quick_check"]}]), None).unwrap();
    assert_eq!(parsed.steps[0].name(), "media_reset");
    assert!(Step::NAMES.contains(&"media_reset"));
    assert!(Step::NAMES.contains(&"media_sync"));
    assert_eq!(
        ObservationKind::from_name("media_quick_check"),
        Some(ObservationKind::MediaQuickCheck)
    );
    assert!(parsed.needs().observations.contains("media_quick_check"));
    for bad in [
        json!({}),
        json!([]),
        json!(["#c0"]),
        json!(false),
        json!({"extra":true}),
    ] {
        assert!(scenario(json!([{"observe":[{"media_quick_check":bad}]}]), None).is_err());
    }
    assert!(scenario(json!([{"media_sync":{}}]), None).is_err());
    assert!(scenario(json!([{"media_sync":{}}, login]), actors.clone()).is_err());
    assert!(scenario(json!([login, {"actor":"meguri"}, {"media_sync":{}}]), actors.clone()).is_err());
    let parsed = scenario(
        json!([
            login, {"media_sync":{}}, {"actor":"meguri"}, login,
            {"media_sync":{}}, {"actor":"client_a"}, {"media_sync":{}}
        ]),
        actors,
    )
    .unwrap();
    assert!(!parsed.steps[1].is_global());
    assert_eq!(parsed.step_actors()[1].unwrap().as_str(), "client_a");
    assert_eq!(parsed.step_actors()[4].unwrap().as_str(), "meguri");
    assert_eq!(parsed.step_actors()[6].unwrap().as_str(), "client_a");
}

#[test]
fn published_media_reset_scenarios_parse_but_keep_media_sync_missing() {
    use meguri_testkit::{exec::Executor, scenario};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/scenarios/sync/media/tracking");
    let executor = MeguriExecutor::new();
    for name in ["reset-identical", "reset-pending"] {
        let loaded = scenario::load(&root.join(format!("{name}.yaml"))).unwrap();
        let missing = executor.missing(&loaded.scenario.needs_of_meguri_actors());
        assert!(missing.ops.contains("media_sync"), "{name}");
        assert!(!missing.ops.contains("media_reset"), "{name}");
        assert!(!missing.observations.contains("media_quick_check"), "{name}");
    }
}
