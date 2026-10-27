//! H1 scenarios use the real host backup writer and T2 policy in both executor paths.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::Path;
#[test]
fn complete_backup_trigger_scenarios_match_frozen_goldens_and_deviations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executors = [
        ("production", MeguriExecutor::new()),
        ("fixture_harness/CLI", MeguriExecutor::fixture_harness()),
    ];
    for name in [
        "change-check",
        "change-check-undo",
        "foreign-entry",
        "recent-entry",
        "forced-unchanged",
        "reopen",
    ] {
        let loaded = scenario::load(&root.join(format!("scenarios/package/backup-triggers/{name}.yaml"))).unwrap();
        let id = &loaded.scenario.id;
        let expected = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(expected["scenario_sha256"].as_str(), Some(loaded.sha256.as_str()));
        for (mode, executor) in &executors {
            assert!(
                executor.missing(&loaded.scenario.needs()).is_empty(),
                "{id} ({mode}): real capability"
            );
            let actual = executor.run(&loaded).unwrap_or_else(|e| panic!("{id} ({mode}): {e}"));
            let result = diff::compare(
                id,
                &expected["observations"],
                &actual,
                &CompareOptions {
                    set: &loaded.scenario.compare.set,
                    deviations: &suite.deviations,
                    overlay: &[],
                },
            );
            assert!(result.differences.is_empty(), "{id} ({mode}): {:?}", result.differences);
        }
    }
}
