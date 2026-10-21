//! U48 seed: complete scheduling operations and card-data rewrites through
//! the product executor, with pending capabilities and partial runs failing.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::{Path, PathBuf};

const SCHED_OPS_IDS: [&str; 12] = [
    "sched/ops/bury/cards-and-notes",
    "sched/ops/bury/unbury-deck",
    "sched/ops/empty-filtered/restore",
    "sched/ops/grade-now/four-cards",
    "sched/ops/reset/end-of-queue",
    "sched/ops/reset/restore-position",
    "sched/ops/set-due/ae26",
    "sched/ops/set-due/card-kinds",
    "sched/ops/set-due/context",
    "sched/ops/set-due/parse",
    "sched/ops/set-due/range",
    "sched/ops/suspend/cards-and-notes",
];
const CARD_DATA_IDS: [&str; 4] = [
    "model/card-data/lenient",
    "model/card-data/out-of-range",
    "model/card-data/rewrite",
    "model/card-data/unknown-keys",
];

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
fn all_twelve_sched_ops_and_four_card_data_cases_match_complete_frozen_observations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    // Keep the production backup provider; this test needs no fixture witness.
    let executor = MeguriExecutor::new();
    let mut scenarios = Vec::new();
    for (family, expected_ids) in [
        ("sched/ops", SCHED_OPS_IDS.as_slice()),
        ("model/card-data", CARD_DATA_IDS.as_slice()),
    ] {
        let mut paths = Vec::new();
        cases(&root.join("scenarios").join(family), &mut paths);
        paths.sort();
        let loaded = paths
            .iter()
            .map(|path| scenario::load(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
            .collect::<Vec<_>>();
        let ids = loaded.iter().map(|s| s.scenario.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, expected_ids, "{family}: every complete scenario, exactly once");
        scenarios.extend(loaded);
    }

    let mut failures = Vec::new();
    for loaded in scenarios {
        let id = &loaded.scenario.id;
        let expected = parse_json(
            &std::fs::read_to_string(root.join(format!("goldens/{id}.json")))
                .unwrap_or_else(|e| panic!("{id}: read frozen golden: {e}")),
        )
        .unwrap_or_else(|e| panic!("{id}: parse frozen golden: {e}"));
        assert_eq!(
            expected["scenario_sha256"].as_str(),
            Some(loaded.sha256.as_str()),
            "{id}: frozen input"
        );
        let observations = expected.get("observations").expect("golden observations");
        assert_eq!(
            observations["steps"].as_array().expect("golden steps").len(),
            loaded.scenario.steps.len(),
            "{id}: golden covers every step"
        );
        let missing = executor.missing(&loaded.scenario.needs());
        if !missing.is_empty() {
            failures.push(format!("{id}: missing capabilities: {}", missing.summary()));
        }
        // Attempt every full case even when capabilities are missing, so the
        // first RED also records runtime errors. Every error remains a failure.
        match executor.run(&loaded) {
            Err(error) => failures.push(format!("{id}: execution failed: {error}")),
            Ok(actual) => {
                if actual["steps"].as_array().map(Vec::len) != Some(loaded.scenario.steps.len()) {
                    failures.push(format!("{id}: execution did not return every step"));
                }
                let compared = diff::compare(
                    id,
                    observations,
                    &actual,
                    &CompareOptions {
                        set: &loaded.scenario.compare.set,
                        deviations: &suite.deviations,
                        overlay: &[],
                    },
                );
                if !compared.differences.is_empty() {
                    failures.push(format!("{id}: {:?}", compared.differences));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
