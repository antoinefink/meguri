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
