//! Every `browser/config` scenario (`conformance/scenarios/browser/config/`;
//! `specs/browser/config-keys.md`, `specs/conformance/browser-config-ops.md`)
//! run through Meguri's executor and its browser adapters, and compared with
//! the oracle's golden as T0 compares it (dsl.md D10, observations.md O7).
//!
//! T0 runs them once Meguri's executor has the new collection (U28) and the
//! config setup section; until then this file stands in for both, in front
//! of the scenario's own steps:
//!
//! - **The new collection** (dsl.md D6): `open` of the oracle-made schema-18
//!   storage fixture, a new collection of the pinned Anki.
//! - **`setup.config`** (D6.1): one `set_config` per key, in sorted key
//!   order, at `clock.start`.
//!
//! `browser/config/live-roundtrip` is a live chain (D9.2): here Meguri runs
//! every segment, handing the collection over to itself through the storage
//! area's handoff adapters, as the oracle ran every segment to record the
//! golden. The comparison shows that Meguri stores the bytes Anki stored at
//! the end of each segment and reads every key as Anki read them; a cargo
//! test has no oracle worker, so only T1 (`tools/mg conformance --live`),
//! running the oracle's segments on the oracle, shows each side opening the
//! collection file the other wrote.

use std::path::{Path, PathBuf};

use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::live::Chain;
use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::observe::parse_json;
use meguri_testkit::scenario::{self, LoadedScenario, OpenSpec, SetConfigArgs, Step};
use meguri_testkit::suite::Suite;
use serde_json::{Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// The committed scenario with the stand-ins in front of its steps, and how
/// many steps they are.
fn with_stand_ins(loaded: &LoadedScenario) -> (LoadedScenario, usize) {
    let mut played = loaded.clone();
    let mut front = vec![Step::Open(OpenSpec {
        fixture: "storage/schema-18.anki2".parse().unwrap(),
    })];
    if let Some(config) = played.scenario.setup.config.take() {
        let mut keys: Vec<(&String, &Value)> = config.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in keys {
            front.push(Step::SetConfig(SetConfigArgs {
                key: key.clone(),
                value: value.clone(),
                undoable: false,
            }));
        }
    }
    assert!(
        played.scenario.setup.sections().is_empty(),
        "the browser/config scenarios set up config only"
    );
    let prefix = front.len();
    played.scenario.steps.splice(0..0, front);
    (played, prefix)
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

    let (played, prefix) = with_stand_ins(&loaded);
    let executor = MeguriExecutor::new();
    let observed = if played.scenario.is_live() {
        let root = tempfile::tempdir().unwrap();
        Chain {
            meguri: &executor,
            oracle: &executor,
            root: root.path(),
        }
        .run(&played)
    } else {
        executor.run(&played)
    }
    .unwrap_or_else(|e| panic!("{id}: {e}"));
    let steps = observed["steps"].as_array().unwrap();
    assert!(
        steps[..prefix].iter().all(|s| s == &json!({})),
        "{id}: the stand-ins record nothing"
    );
    let observed = json!({ "steps": steps[prefix..] });

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
fn absent_defaults() {
    play("absent-defaults");
}

#[test]
fn live_roundtrip() {
    play("live-roundtrip");
}

#[test]
fn new_collection() {
    play("new-collection");
}

#[test]
fn other_keys_untouched() {
    play("other-keys-untouched");
}

#[test]
fn write_shapes() {
    play("write-shapes");
}

#[test]
fn wrong_shapes() {
    play("wrong-shapes");
}

/// T0 lists every browser/config scenario as pending until the new
/// collection exists (U28), and T1 can run the live chain: Meguri's
/// segments need only the browser, clock and storage adapters.
#[test]
fn meguri_needs_only_the_new_collection_and_setup() {
    let executor = MeguriExecutor::new();
    for entry in std::fs::read_dir(conformance().join("scenarios/browser/config")).unwrap() {
        let path = entry.unwrap().path();
        let loaded = scenario::load(&path).unwrap();
        let missing = if loaded.scenario.is_live() {
            executor.missing(&loaded.scenario.needs_on(scenario::Side::Meguri))
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
        let setup: Vec<&str> = missing.setup.iter().map(String::as_str).collect();
        if loaded.scenario.is_live() {
            assert!(setup.is_empty(), "{}: {setup:?}", path.display());
        } else {
            assert!(setup.contains(&"new_collection"), "{}: {setup:?}", path.display());
            assert!(
                setup.iter().all(|s| ["new_collection", "config"].contains(s)),
                "{}: {setup:?}",
                path.display()
            );
        }
    }
}
