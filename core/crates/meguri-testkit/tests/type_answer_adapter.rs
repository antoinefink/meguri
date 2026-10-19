//! Execute U64's prepared goldens through the production providers, even
//! before the host activates their catalog entries. Never turn failures pending.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe,
    scenario::{self, ReviewHtmlCase, TypeAnswerRequest},
    suite::Suite,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, out);
        } else if path.file_stem().is_some_and(|n| n != "area")
            && path
                .extension()
                .is_some_and(|x| x == "yaml" || x == "json" || x == "yml")
        {
            out.push(path);
        }
    }
}
#[test]
fn every_prepared_u64_scenario_runs_and_matches_its_golden() {
    let root = root();
    let suite = Suite::open(&root).unwrap();
    let mut paths = Vec::new();
    for dir in [
        "render/type-answer",
        "render/media-escape",
        "render/image-occlusion/render",
    ] {
        files(&root.join("scenarios").join(dir), &mut paths);
    }
    paths.sort();
    assert_eq!(paths.len(), 25, "keep the full prepared corpus in this test");
    let executor = MeguriExecutor::new();
    let mut corpus_pairs = std::collections::BTreeSet::new();
    for path in paths {
        let loaded = scenario::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if loaded.scenario.id.starts_with("render/type-answer/corpus/") {
            for step in &loaded.scenario.steps {
                if let scenario::Step::Observe(requests) = step {
                    for request in requests {
                        if let scenario::ObservationArgs::Mapping(map) = &request.args {
                            let request = TypeAnswerRequest::from_mapping(map).unwrap();
                            assert_eq!(request.fns.len(), 2);
                            for pair in request.inputs {
                                assert!(corpus_pairs.insert(pair), "distinct corpus pair");
                            }
                        }
                    }
                }
            }
        }
        let actual = executor
            .run(&loaded)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let golden: Value = observe::parse_json(
            &std::fs::read_to_string(root.join("goldens").join(format!("{}.json", loaded.scenario.id))).unwrap(),
        )
        .unwrap();
        let comparison = diff::compare(
            &loaded.scenario.id,
            &golden["observations"],
            &actual,
            &CompareOptions {
                set: &loaded.scenario.compare.set,
                deviations: &suite.deviations,
                ..Default::default()
            },
        );
        assert!(
            comparison.differences.is_empty(),
            "{}: {:?}",
            loaded.scenario.id,
            comparison.differences
        );
    }
    assert_eq!(corpus_pairs.len(), 3000);
    assert_eq!(
        corpus_pairs.iter().filter(|(e, _)| e.chars().count() >= 200).count(),
        159
    );
}
#[test]
fn request_shapes_are_exact() {
    for request in [
        json!({"fns":[],"inputs":[]}),
        json!({"fns":["compare","compare"],"inputs":[]}),
        json!({"fns":["compare_ci"],"inputs":[]}),
        json!({"fns":["compare"],"inputs":[["x"]]}),
        json!({"fns":["compare"],"inputs":[["x",null]]}),
        json!({"fns":["compare"],"inputs":[[1,"x"]]}),
        json!({"fns":["compare"],"inputs":[["x","x","x"]]}),
        json!({"fns":["compare"],"inputs":[],"x":1}),
    ] {
        assert!(
            TypeAnswerRequest::from_mapping(request.as_object().unwrap()).is_err(),
            "{request}"
        );
    }
    for cases in [
        json!([]),
        json!([{}]),
        json!([{"card":"#n1"}]),
        json!([{"card":"#c1","typed":null}]),
        json!([{"card":"#c1","x":0}]),
        json!(["#c1"]),
    ] {
        assert!(ReviewHtmlCase::from_list(cases.as_array().unwrap()).is_err(), "{cases}");
    }
}
fn run(value: Value) -> Result<Value, meguri_testkit::exec::ExecError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.json");
    std::fs::write(&path, value.to_string()).unwrap();
    MeguriExecutor::new().run(&scenario::load(&path).unwrap())
}
#[test]
fn pure_comparison_needs_no_collection_and_disjoint_functions_merge() {
    let out = run(
        json!({"id":"render/pure-type-answer", "clock":{"tz":"UTC","start":"2030-01-15T10:00:00"}, "steps":[{"observe":[
            {"type_answer":{"fns":["compare"],"inputs":[["ka","ka"]]}},
            {"type_answer":{"fns":["compare_nc"],"inputs":[]}}
        ]}]}),
    )
    .unwrap();
    assert_eq!(
        out,
        json!({"steps":[{"type_answer":{"compare":["<code id=typeans><span class=typeGood>ka</span></code>"],"compare_nc":[]}}]})
    );
    let err = run(json!({"id":"render/duplicate-type-answer", "clock":{"tz":"UTC","start":"2030-01-15T10:00:00"}, "steps":[{"observe":[
        {"type_answer":{"fns":["compare"],"inputs":[]}}, {"type_answer":{"fns":["compare"],"inputs":[]}}
    ]}]})).unwrap_err();
    assert_eq!(err.kind, "invalid_scenario");
}
#[test]
fn repeated_review_html_requests_are_rejected_when_loading() {
    let value = json!({"id":"render/duplicate-review-html", "clock":{"tz":"UTC","start":"2030-01-15T10:00:00"}, "steps":[{"observe":[
        {"review_html":[{"card":"#c1"}]}, {"review_html":[{"card":"#c1"}]}
    ]}]});
    assert!(scenario::Scenario::from_value(&value).is_err());
}
