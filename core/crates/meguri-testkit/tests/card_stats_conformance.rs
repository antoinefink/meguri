//! `stats/card-info` C14.1 through Meguri's `card_stats` adapter
//! (`conformance/stats-ops` H9): every recorded scenario against its golden,
//! with the deviations of C13.4 applied.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::Path;

const SCENARIOS: [&str; 18] = [
    "ae46-no-state",
    "back-to-back-answers",
    "cloze-type",
    "custom-data",
    "due-estimate-night",
    "edge-values",
    "filtered-card",
    "intraday-learning",
    "memory-and-retention",
    "mixed-history",
    "new-card",
    "no-item",
    "no-lrt-elapsed",
    "params",
    "reset-position",
    "review-card",
    "times",
    "zone-dst",
];

#[test]
fn every_recorded_card_info_scenario_matches_its_golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let mut failures = Vec::new();
    for name in SCENARIOS {
        let id = format!("stats/card-info/{name}");
        let loaded = scenario::load(&root.join(format!("scenarios/{id}.yaml"))).unwrap();
        let expected = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(expected["scenario_sha256"].as_str(), Some(loaded.sha256.as_str()), "{id}: stale golden");
        let missing = executor.missing(&loaded.scenario.needs());
        if !missing.is_empty() {
            failures.push(format!("{id}: missing {missing:?}"));
            continue;
        }
        let actual = match executor.run(&loaded) {
            Ok(actual) => actual,
            Err(error) => {
                failures.push(format!("{id}: run error {error}"));
                continue;
            }
        };
        let observed = |value: &serde_json::Value| {
            value["steps"]
                .as_array()
                .map_or(0, |steps| steps.iter().filter(|step| step.get("card_stats").is_some()).count())
        };
        assert!(observed(&actual) > 0, "{id}: no card_stats observation ran");
        assert_eq!(observed(&actual), observed(&expected["observations"]), "{id}");
        let compared = diff::compare(
            &id,
            &expected["observations"],
            &actual,
            &CompareOptions {
                set: &loaded.scenario.compare.set,
                deviations: &suite.deviations,
                overlay: &[],
            },
        );
        if !compared.differences.is_empty() {
            failures.push(format!("{id}: {:#?}", compared.differences));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

