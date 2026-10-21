//! Full scenarios, without truncating at an answer or treating pending as pass.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::{Path, PathBuf};
fn cases(path: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            cases(&path, out);
        } else {
            let name = path.file_name().unwrap().to_str().unwrap();
            if name != "area.yaml" && name != "area.yml" && scenario::SUFFIXES.iter().any(|s| name.ends_with(s)) {
                out.push(path);
            }
        }
    }
}
#[test]
fn complete_answer_families_and_standalone_dependent_regressions_match_whole_results() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let mut paths = Vec::new();
    for area in ["answer", "bury", "leech", "revlog", "loadbalancer"] {
        cases(&root.join("scenarios/sched").join(area), &mut paths);
    }
    paths.sort();
    let ids = paths
        .iter()
        .map(|p| scenario::load(p).unwrap().scenario.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "sched/answer/custom-data",
            "sched/answer/deck-counters",
            "sched/answer/new-card-ratings",
            "sched/answer/positions-normal",
            "sched/answer/state-kinds",
            "sched/answer/unchanged-counters",
            "sched/bury/ae22-undo",
            "sched/bury/answer-kinds",
            "sched/bury/sibling-states",
            "sched/leech/ae25-tag-only",
            "sched/leech/suspend",
            "sched/leech/thresholds",
            "sched/leech/zero-threshold",
            "sched/loadbalancer/cache-update",
            "sched/loadbalancer/counted-rows",
            "sched/loadbalancer/crafted-days",
            "sched/loadbalancer/easy-days",
            "sched/loadbalancer/plain-over-90",
            "sched/loadbalancer/siblings",
            "sched/revlog/kept-rows"
        ]
    );
    for id in [
        "undo/revlog-taken-id",
        "sched/day/legacy-timing-after-answer",
        "sched/fsrs/ae23-answer",
        "sched/fsrs/input-boundaries",
        "sched/steps/rollover-answer",
        "sched/memory/on-the-fly",
        "sched/data/answer-data",
        "sched/data/answer-keeps-cd",
        "sched/data/deck-counters",
        "sched/data/desired-retention",
        "sched/data/missing-preset",
        "sched/data/preset-bounds",
        "sched/data/preset-keep",
        "sched/compat/custom-scheduling-ignored",
        "sched/compat/re-enable",
        "sched/compat/unsupported-preset-kept",
    ] {
        let mut found = Vec::new();
        let parent = root.join("scenarios").join(id).parent().unwrap().to_path_buf();
        cases(&parent, &mut found);
        let matching = found
            .into_iter()
            .filter(|p| scenario::load(p).unwrap().scenario.id == id)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{id}: exactly one supported scenario suffix");
        paths.push(matching[0].clone());
    }
    // Preserve the complete filtered-leg case as pending. It is never a
    // positive product result and its runtime refusal remains an error.
    let mut data_paths = Vec::new();
    cases(&root.join("scenarios/sched/data"), &mut data_paths);
    let mixed = data_paths
        .iter()
        .map(|p| scenario::load(p).unwrap())
        .find(|s| s.scenario.id == "sched/data/positions")
        .expect("preserved mixed positions case");
    assert_eq!(mixed.scenario.steps.len(), 19);
    assert!(mixed.scenario.tags.iter().any(|tag| tag == "needs:answer.filtered"));
    assert!(mixed.scenario.needs().ops.contains("answer.filtered"));
    assert!(!executor.capabilities().ops.contains("answer.filtered"));
    assert_eq!(
        executor.missing(&mixed.scenario.needs()).ops,
        ["answer.filtered".to_owned()].into()
    );
    assert!(
        !paths
            .iter()
            .any(|p| scenario::load(p).unwrap().scenario.id == mixed.scenario.id)
    );
    // The remaining two requested regressions use the live-chain and actor
    // runners (enable-fsrs-live and keys-survive-sync). Their reference legs
    // and sync providers belong to the integration harness, not Executor::run.
    assert_eq!(paths.len(), 36);
    for path in paths {
        let loaded = scenario::load(&path).unwrap();
        let id = &loaded.scenario.id;
        let expected = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(
            expected["scenario_sha256"].as_str(),
            Some(loaded.sha256.as_str()),
            "{id}: frozen input"
        );
        assert!(
            executor.missing(&loaded.scenario.needs()).is_empty(),
            "{id}: missing capability"
        );
        let actual = executor.run(&loaded).unwrap_or_else(|e| panic!("{id}: {e}"));
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
        assert!(result.differences.is_empty(), "{id}: {:?}", result.differences);
    }
}
