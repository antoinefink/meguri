//! U28's recorded scenarios must pass even before activation manifests change.
use meguri_testkit::{
    exec::MeguriExecutor,
    suite::{Status, Suite},
};
use std::path::Path;

#[test]
fn decks_and_new_collection_match_recorded_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let mut failures = Vec::new();
    for file in suite
        .discover(&[
            "decks/".into(),
            "storage/new-collection/".into(),
            "sched/day/".into(),
            "sched/unbury-on-rollover/".into(),
            "browser/config/new-collection".into(),
        ])
        .unwrap()
    {
        if file.rel.contains("/handoff") || file.rel.contains("legacy-timing-after-answer") {
            continue;
        }
        let outcome = suite.check(&file, &executor);
        if outcome.status != Status::Pass {
            failures.push(outcome.render_text());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
