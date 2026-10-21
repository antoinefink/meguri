//! Run the complete frozen U41 ordered-id corpus regardless of area
//! activation. An absent golden or missing adapter fails this proof test;
//! pending is never accepted as pass. Production adapters never read goldens.
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
            if name != "area.yaml"
                && name != "area.yml"
                && scenario::SUFFIXES.iter().any(|suffix| name.ends_with(*suffix))
            {
                out.push(path);
            }
        }
    }
}
#[test]
fn all_queue_and_count_cases_match_full_ordered_identity_kind_and_counts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let mut paths = Vec::new();
    cases(&root.join("scenarios/sched/queue"), &mut paths);
    cases(&root.join("scenarios/sched/counts"), &mut paths);
    paths.sort();
    let inventory = paths
        .iter()
        .map(|p| p.strip_prefix(root.join("scenarios/sched")).unwrap().to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        inventory,
        [
            "counts/ae84-kathmandu.yaml",
            "counts/deck-page-difference.yaml",
            "counts/default-only.yaml",
            "counts/default-shown.yaml",
            "counts/finished.yaml",
            "counts/limits-tree.yaml",
            "counts/tree-order.yaml",
            "queue/bury/interday-siblings.yaml",
            "queue/bury/mixed-presets.yaml",
            "queue/bury/new-siblings.yaml",
            "queue/bury/review-siblings.yaml",
            "queue/gather/card-states.yaml",
            "queue/gather/missing-preset.yaml",
            "queue/gather/nested-filtered.yaml",
            "queue/gather/preview-in-normal-deck.yaml",
            "queue/learn-ahead/ready-in.yaml",
            "queue/learn-ahead/window.yaml",
            "queue/limits/ae21.yaml",
            "queue/limits/interday-first.yaml",
            "queue/limits/intraday-uncapped.yaml",
            "queue/limits/new-capped-by-reviews.yaml",
            "queue/limits/parent-limits.yaml",
            "queue/limits/today-only.yaml",
            "queue/mix/interday-review.yaml",
            "queue/mix/new-review.yaml",
            "queue/mix/spread-sizes.yaml",
            "queue/order/mixed-sixty.json",
            "queue/order/new-gather-ascending.yaml",
            "queue/order/new-gather-deck-random-notes.yaml",
            "queue/order/new-gather-deck.yaml",
            "queue/order/new-gather-descending.yaml",
            "queue/order/new-gather-random-cards.yaml",
            "queue/order/new-gather-random-notes.yaml",
            "queue/order/review-orders.yaml",
            "queue/order/ties.yaml",
        ]
    );
    for path in paths {
        let loaded = scenario::load(&path).unwrap();
        let id = &loaded.scenario.id;
        let golden = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(
            golden["scenario_sha256"].as_str(),
            Some(loaded.sha256.as_str()),
            "{id}: frozen inputs"
        );
        let missing = executor.missing(&loaded.scenario.needs());
        assert!(missing.is_empty(), "{id}: missing {missing:?}");
        let observed = executor.run(&loaded).unwrap_or_else(|e| panic!("{id}: {e}"));
        let comparison = diff::compare(
            id,
            &golden["observations"],
            &observed,
            &CompareOptions {
                set: &loaded.scenario.compare.set,
                deviations: &suite.deviations,
                overlay: &[],
            },
        );
        assert!(comparison.differences.is_empty(), "{id}: {:?}", comparison.differences);
    }
}
