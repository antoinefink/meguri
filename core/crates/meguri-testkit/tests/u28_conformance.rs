//! U28's recorded scenarios must pass even before activation manifests change.
use meguri_testkit::{
    exec::MeguriExecutor,
    suite::{Platform, Status, Suite},
};
use std::path::Path;

#[test]
fn decks_and_new_collection_match_recorded_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::fixture_harness();
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

#[test]
fn setup_presets_match_schema11_goldens_on_both_lanes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let executor = MeguriExecutor::fixture_harness();
    let filters = [
        "preset-values",
        "preset-rejections",
        "preset-other",
        "preset-part-defaults",
        "other-out-of-range",
    ]
    .map(|name| format!("model/schema11/{name}"));
    let mut failures = Vec::new();
    for platform in [Platform::Linux, Platform::MacOs] {
        let suite = Suite::open(&root).unwrap().on_platform(platform);
        let files = suite.discover(&filters).unwrap();
        assert_eq!(files.len(), 5);
        for file in files {
            let outcome = suite.check(&file, &executor);
            // This area is not active in T0 yet: require an actual pass.
            if outcome.status != Status::Pass {
                failures.push(format!("{platform:?}: {}", outcome.render_text()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
