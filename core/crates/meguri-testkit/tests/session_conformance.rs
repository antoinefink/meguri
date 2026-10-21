//! Literal complete observations through the E2 held production queue seam.
//! This test does not activate an area or alter any frozen inputs/outputs.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::{Path, PathBuf};

fn cases(path: &Path, paths: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            cases(&path, paths);
        } else {
            let name = path.file_name().unwrap().to_str().unwrap();
            if name != "area.yaml" && name != "area.yml" && scenario::SUFFIXES.iter().any(|s| name.ends_with(s)) {
                paths.push(path);
            }
        }
    }
}

#[test]
fn all_eight_session_cases_match_literal_whole_observations_including_mixed_40_full_82_answers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let mut paths = Vec::new();
    cases(&root.join("scenarios/sched/session"), &mut paths);
    paths.sort();
    let ids = paths
        .iter()
        .map(|p| scenario::load(p).unwrap().scenario.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "sched/session/cutoff/refresh-when-empty",
            "sched/session/drain/mixed-40",
            "sched/session/head/learning-anywhere",
            "sched/session/requeue/after-next-learning",
            "sched/session/requeue/front-rules",
            "sched/session/requeue/main-not-empty",
            "sched/session/rollover/ae13",
            "sched/session/undo/answer-undo-redo"
        ]
    );
    for path in paths {
        let loaded = scenario::load(&path).unwrap();
        let id = &loaded.scenario.id;
        let expected = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(
            expected["scenario_sha256"].as_str(),
            Some(loaded.sha256.as_str()),
            "{id}: frozen input"
        );
        if id == "sched/session/drain/mixed-40" {
            assert_eq!(
                loaded
                    .scenario
                    .steps
                    .iter()
                    .filter(|s| matches!(s, scenario::Step::Answer(_)))
                    .count(),
                82
            );
        }
        assert!(
            executor.missing(&loaded.scenario.needs()).is_empty(),
            "{id}: no missing adapters"
        );
        let actual = executor.run(&loaded).unwrap_or_else(|e| panic!("{id}: {e}"));
        // The session corpus authorizes no transform (P11 / D13).
        assert_eq!(actual, expected["observations"], "{id}: every literal observation");
        let compared = diff::compare(
            id,
            &expected["observations"],
            &actual,
            &CompareOptions {
                set: &loaded.scenario.compare.set,
                deviations: &suite.deviations,
                overlay: &[],
            },
        );
        assert!(compared.differences.is_empty(), "{id}: {:?}", compared.differences);
    }
}
