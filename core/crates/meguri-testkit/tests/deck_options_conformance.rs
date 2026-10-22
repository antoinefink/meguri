//! Complete frozen comparisons: no prefix replay or fixture-only Save API.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::Path;
#[test]
fn every_recorded_u49_save_scenario_and_memory_recompute_matches_all_steps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    for id in [
        "sched/options/remap-steps",
        "sched/options/undo-save",
        "sched/options/insertion-order-switch",
        "sched/options/missing-preset",
        "sched/options/insertion-order-option",
        "sched/options/deck-limits",
        "sched/options/subtree",
        "sched/options/remove-preset",
        "sched/options/globals",
        "sched/options/prefs-rollover",
        "sched/memory/recompute",
    ] {
        let loaded = scenario::load(&root.join(format!("scenarios/{id}.yaml"))).unwrap();
        let expected = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(expected["scenario_sha256"].as_str(), Some(loaded.sha256.as_str()));
        assert!(executor.missing(&loaded.scenario.needs()).is_empty(), "{id}");
        let actual = executor.run(&loaded).unwrap();
        assert_eq!(actual["steps"].as_array().unwrap().len(), loaded.scenario.steps.len());
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
#[test]
fn neutral_extended_save_and_preferences_inputs_have_real_capabilities_and_strict_shapes() {
    let executor = MeguriExecutor::new();
    let value=scenario::parse_yaml("id: extended\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsetup:\n  presets: [{name: Other}]\n  decks: [{name: Parent}, {name: 'Parent::Child', preset: Other}]\nsteps:\n  - save_deck_options: {deck: Parent, subtree: true, limits: {new: 7, new_today: 50}, globals: {apply_all_parent_limits: true}, options: {insertion_order: random}, remove: [Other]}\n  - save_deck_options: {deck: Parent, limits: {new_today: null}}\n  - set_scheduling_prefs: {learn_ahead_minutes: 33, load_balancer: false}\n  - save_deck_options: {deck: Parent, remove: [Default], preset: Default}\n").unwrap();
    // Removing the selected preset is an invalid scenario; test its schema
    // independently, then execute the valid complete sequence through U49.
    let mut value = value;
    value["steps"].as_array_mut().unwrap().pop();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("extended.json");
    std::fs::write(&path, value.to_string()).unwrap();
    let loaded = scenario::load(&path).unwrap();
    assert!(executor.missing(&loaded.scenario.needs()).is_empty());
    let actual = executor.run(&loaded).unwrap();
    assert_eq!(actual["steps"].as_array().unwrap().len(), 3);
    for patch in [
        serde_json::json!({"set_scheduling_prefs":{}}),
        serde_json::json!({"set_scheduling_prefs":{"rollover":24}}),
        serde_json::json!({"save_deck_options":{"deck":"Parent","limits":{"new":10000}}}),
        serde_json::json!({"save_deck_options":{"deck":"Parent","remove":["Other","Other"]}}),
    ] {
        value["steps"] = serde_json::json!([patch]);
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(scenario::load(&path).is_err());
    }
}
