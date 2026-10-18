use meguri_testkit::{
    exec::{Executor, MeguriExecutor},
    scenario,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}
#[test]
fn pure_next_states_and_labels_match_all_recorded_observations() {
    let mut checked = 0;
    for area in ["fsrs", "steps", "labels"] {
        let mut paths = fs::read_dir(root().join("scenarios/sched").join(area))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let mut value = scenario::parse_yaml(&fs::read_to_string(&path).unwrap()).unwrap();
            let steps = value["steps"].as_array_mut().unwrap();
            if let Some(index) = steps.iter().position(|s| s.get("answer").is_some()) {
                steps.truncate(index);
            }
            if !steps
                .iter()
                .any(|s| s.to_string().contains("next_states") || s.to_string().contains("answer_labels"))
            {
                continue;
            }
            let expected_steps = steps.len();
            checked += 1;
            let dir = tempfile::tempdir().unwrap();
            let run_path = dir.path().join("scenario.json");
            fs::write(&run_path, value.to_string()).unwrap();
            let loaded = scenario::load(&run_path).unwrap();
            let actual = MeguriExecutor::new().run(&loaded).unwrap();
            let golden: Value = serde_json::from_str(
                &fs::read_to_string(root().join(format!("goldens/sched/{area}/{name}.json"))).unwrap(),
            )
            .unwrap();
            assert_eq!(actual["steps"].as_array().unwrap().len(), expected_steps);
            let mut expected = golden["observations"].clone();
            expected["steps"].as_array_mut().unwrap().truncate(expected_steps);
            assert_eq!(actual, expected, "{area}/{name}");
        }
    }
    assert_eq!(checked, 18);
}
