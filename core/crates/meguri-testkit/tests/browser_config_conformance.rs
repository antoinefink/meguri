//! The `browser/config` scenario T0 does not run
//! (`conformance/scenarios/browser/config/`; `specs/browser/config-keys.md`
//! K13, `specs/conformance/browser-config-ops.md`), played through Meguri's
//! executor and its browser adapters and compared with the oracle's golden
//! as T0 compares it (dsl.md D10, observations.md O7). T0 runs the other
//! five, which are active: four open the schema-18 storage fixture, and
//! `new-collection` starts from the executor's new collection (dsl.md D8),
//! the note type area's stand-in (notetype-ops.md T10.2) until U28's
//! bootstrap, which holds K1's two new-collection keys.
//!
//! `browser/config/live-roundtrip` is a live chain (D9.2), which only T1
//! runs. Here Meguri runs every segment from its new collection, handing the
//! collection over to itself through the storage area's handoff adapters, as
//! the oracle ran every segment to record the golden. That shows Meguri
//! stores the bytes Anki stored at the end of each segment and reads every
//! key as Anki read them. A cargo test has no oracle worker: only T1
//! (`tools/mg conformance --live browser/config/live-roundtrip`) runs the
//! oracle's segments on the oracle, so each side opens the collection file
//! the other wrote.

use std::path::{Path, PathBuf};

use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::live::Chain;
use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::observe::parse_json;
use meguri_testkit::scenario::{self, Side};
use meguri_testkit::suite::Suite;
use serde_json::{Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// Runs `browser/config/<name>` on Meguri and compares it with its golden.
fn play(name: &str) {
    let id = format!("browser/config/{name}");
    let loaded = scenario::load(&conformance().join(format!("scenarios/{id}.yaml"))).unwrap();
    let golden: Value =
        parse_json(&std::fs::read_to_string(conformance().join(format!("goldens/{id}.json"))).unwrap()).unwrap();
    assert_eq!(
        golden["scenario_sha256"].as_str(),
        Some(loaded.sha256.as_str()),
        "the golden is fresh"
    );
    assert_eq!(golden["clock"]["mode"], json!("frozen"));

    assert!(
        loaded.scenario.is_live(),
        "{id}: T0 runs the scenarios without handoffs"
    );
    let executor = MeguriExecutor::new();
    let root = tempfile::tempdir().unwrap();
    let observed = Chain {
        meguri: &executor,
        oracle: &executor,
        root: root.path(),
    }
    .run(&loaded)
    .unwrap_or_else(|e| panic!("{id}: {e}"));

    let suite = Suite::open(&conformance()).unwrap();
    let comparison = diff::compare(
        &id,
        &golden["observations"],
        &observed,
        &CompareOptions {
            set: &loaded.scenario.compare.set,
            deviations: &suite.deviations,
            overlay: &[],
        },
    );
    assert!(
        comparison.differences.is_empty(),
        "{id}: differences from the golden: {:#?}",
        comparison
            .differences
            .iter()
            .map(|d| (&d.pointer, &d.expected, &d.actual))
            .collect::<Vec<_>>()
    );
    assert!(
        comparison.deviations_applied.is_empty(),
        "{id}: no deviation applies: {:?}",
        comparison.deviations_applied
    );
}

#[test]
fn live_roundtrip() {
    play("live-roundtrip");
}

/// Meguri's executor lacks nothing the browser/config scenarios use: the
/// new collection that `browser/config/new-collection` starts from, and the
/// live chain's Meguri segment, included.
#[test]
fn meguri_lacks_nothing_the_browser_scenarios_use() {
    let executor = MeguriExecutor::new();
    for entry in std::fs::read_dir(conformance().join("scenarios/browser/config")).unwrap() {
        let path = entry.unwrap().path();
        let loaded = scenario::load(&path).unwrap();
        let missing = if loaded.scenario.is_live() {
            executor.missing(&loaded.scenario.needs_on(Side::Meguri))
        } else {
            executor.missing(&loaded.scenario.needs())
        };
        assert!(missing.ops.is_empty(), "{}: {:?}", path.display(), missing.ops);
        assert!(
            missing.observations.is_empty(),
            "{}: {:?}",
            path.display(),
            missing.observations
        );
        assert!(missing.setup.is_empty(), "{}: {:?}", path.display(), missing.setup);
    }
}
