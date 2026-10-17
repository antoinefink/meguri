//! conformance/image-occlusion-ops J1–J3: strict requests and merged outputs.
use meguri_testkit::{
    exec::{Executor, MeguriExecutor},
    scenario::{self, ImageOcclusionRequest},
};
use serde_json::json;

#[test]
fn tags_must_be_plain_integers_and_parse_inputs_must_survive_note_save() {
    for request in [
        json!({"fns":["fields"],"inputs":[[1.0]]}),
        json!({"fns":["fields"],"inputs":[[true]]}),
        json!({"fns":["fields"],"inputs":[[-1]]}),
        json!({"fns":["fields"],"inputs":[[4294967296u64]]}),
        json!({"fns":["fields"],"inputs":[[]]}),
        json!({"fns":["fields","parse"],"inputs":[]}),
        json!({"fns":["parse","parse"],"inputs":[]}),
        json!({"fns":["unknown"],"inputs":[]}),
        json!({"fns":[],"inputs":[]}),
        json!({"fns":["parse"],"inputs":[1]}),
        json!({"fns":["parse"],"inputs":["e\u{301}"]}),
        json!({"fns":["parse"],"inputs":["a\rb"]}),
        json!({"fns":["parse"],"inputs":[],"extra":1}),
    ] {
        assert!(
            ImageOcclusionRequest::from_mapping(request.as_object().unwrap()).is_err(),
            "{request}"
        );
    }
    for request in [
        json!({"fns":["fields"],"inputs":[[0,null,4294967295u64]]}),
        json!({"fns":["parse"],"inputs":["é\t\n\u{7f}"]}),
        json!({"fns":["parse"],"inputs":[]}),
    ] {
        assert!(
            ImageOcclusionRequest::from_mapping(request.as_object().unwrap()).is_ok(),
            "{request}"
        );
    }
}

fn run(steps: serde_json::Value) -> Result<serde_json::Value, meguri_testkit::exec::ExecError> {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("io.json");
    std::fs::write(
        &path,
        json!({"id":"render/io","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":steps}).to_string(),
    )
    .unwrap();
    let loaded = scenario::load(&path).unwrap();
    MeguriExecutor::new().run(&loaded)
}

#[test]
fn two_functions_merge_without_a_collection_and_duplicate_outputs_are_refused() {
    let out = run(json!([{"observe":[
        {"image_occlusion":{"fns":["parse"],"inputs":["{{c1::image-occlusion:rect:oi=0:oi=1}}"]}},
        {"image_occlusion":{"fns":["fields"],"inputs":[[3,2,1,0],[0,1,2]]}}
    ]}]))
    .unwrap();
    assert_eq!(
        out["steps"][0]["image_occlusion"],
        json!({
            "parse":[{"hide_all":false,"numbers":[{"number":1,"shapes":[{"shape":"rect","props":[["oi","0"],["oi","1"]]}]}]}],
            "fields":[{"occlusions":3,"image":2,"header":1,"back_extra":0},null]
        })
    );
    let err = run(json!([{"observe":[
        {"image_occlusion":{"fns":["parse"],"inputs":[]}},
        {"image_occlusion":{"fns":["parse"],"inputs":[]}}
    ]}]))
    .unwrap_err();
    assert_eq!(err.kind, "invalid_scenario");
}

#[test]
fn scenario_loading_rejects_float_and_boolean_tags_before_execution() {
    for tag in ["1.0", "true"] {
        let text = format!(
            "id: render/invalid-io\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps:\n  - observe: [{{image_occlusion: {{fns: [fields], inputs: [[{tag}]]}}}}]\n"
        );
        let value = scenario::parse_yaml(&text).unwrap();
        assert!(
            scenario::Scenario::from_value(&value)
                .unwrap_err()
                .to_string()
                .contains("plain u32 integers")
        );
    }
}
