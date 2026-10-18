use meguri_testkit::{
    exec::{Executor, MeguriExecutor},
    scenario,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

#[test]
fn the_ten_thousand_pair_corpus_matches_every_recorded_integer_delta() {
    let loaded = scenario::load(&conformance().join("scenarios/fp-probe/corpus/fuzz-table.json")).unwrap();
    let actual = MeguriExecutor::new().run(&loaded).unwrap();
    let golden: Value = serde_json::from_str(
        &fs::read_to_string(conformance().join("goldens/fp-probe/corpus/fuzz-table.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual["steps"][0]["fuzz_table"]["rows"].as_array().unwrap().len(),
        10_000
    );
    assert_eq!(actual, golden["observations"]);
}

#[test]
fn observing_synthetic_ids_and_counts_does_not_change_the_stored_card() {
    let original: Value = serde_json::from_str(
        &fs::read_to_string(conformance().join("scenarios/fp-probe/corpus/fuzz-table.json")).unwrap(),
    )
    .unwrap();
    let mut value = original;
    value["id"] = json!("sched/fuzz/observation-is-read-only");
    value["steps"] = json!([
        {"observe": ["ids", {"card": ["#c1"]}]},
        {"observe": [{"fuzz_table": {"card":"#c1", "pairs":[[1,0],[2,4294967294u64]], "intervals":[2,3,7,36500]}}]},
        {"observe": ["ids", {"card": ["#c1"]}]}
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scenario.json");
    fs::write(&path, value.to_string()).unwrap();
    let loaded = scenario::load(&path).unwrap();
    let actual = MeguriExecutor::new().run(&loaded).unwrap();
    assert_eq!(actual["steps"][0], actual["steps"][2]);
    assert_eq!(actual["steps"][1]["fuzz_table"]["rows"].as_array().unwrap().len(), 2);
}
