//! T1 live chains (specs/conformance/dsl.md D9, D10): a live scenario's
//! segments run on the oracle and Meguri in turn, through a handoff directory
//! both see, and the chain's observations are compared with the golden the
//! oracle recorded running every segment itself. The oracle side is a
//! stand-in worker speaking conformance/schema/protocol.json.

use std::fs;
use std::path::Path;

use meguri_testkit::exec::{ExecError, MeguriExecutor, OpResult, OracleExecutor, Registry, Session};
use meguri_testkit::scenario::Step;
use meguri_testkit::suite::{LiveRun, OraclePin, Status, Suite};
use serde_json::json;

const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";

/// 2030-01-15T10:00:00Z, one day later, and one hour after that.
const START: i64 = 1_894_701_600;
const AFTER_ORACLE: i64 = START + 86_400;
const AFTER_MEGURI: i64 = AFTER_ORACLE + 3_600;

const SCENARIO: &str = r#"id: live/round-trip
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
steps:
  - advance: 1d         # the oracle
  - handoff: meguri
  - advance: 1h         # Meguri
  - handoff: oracle
  - observe: [timing]   # the oracle, on the collection Meguri wrote
"#;

/// The oracle side. A plain run (no `steps` range) answers what the oracle
/// recorded running everything itself. A first segment writes its
/// "collection" into the handoff directory and hands over one day later. A
/// resumed segment opens the collection it was handed (Anki refuses one that
/// is not a collection) and observes the clock it was handed. The request's
/// keys come sorted (serde_json), so the top-level `handoff` and `steps`
/// are found by their position.
const WORKER: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"cmd":"hello"'*)
      printf '{"ok":true,"protocol":1,"anki_version":"26.9.3","lock_sha256":"%s","driver_sha256":"%s","tz":"%s"}\n' \
        "$LOCK" "$(printf 'b%.0s' $(seq 64))" "$TZ" ;;
    *'"resume":'*)
      dir=$(printf '%s' "$line" | sed -n 's/.*"resume":{"cards":\[[^]]*\],"dir":"\([^"]*\)".*/\1/p')
      now=$(printf '%s' "$line" | sed -n 's/.*"resume":{[^}]*"now":\([0-9]*\)}.*/\1/p')
      if [ "$(cat "$HANDOFF/$dir/collection.anki2" 2>/dev/null)" = meguri ]; then
        printf '{"ok":true,"scenario_id":"live/round-trip","elapsed_ms":1,"observations":{"steps":[{"timing":{"now":%s}}]}}\n' "$now"
      else
        printf '{"ok":false,"error":{"kind":"anki_error","op":"open","exception":"DBError","step":4}}\n'
      fi ;;
    *'"steps":['[0-9]*)
      dir=$(printf '%s' "$line" | sed -n 's/^{"cmd":"run","handoff":"\([^"]*\)".*/\1/p')
      mkdir -p "$HANDOFF/$dir" && printf oracle >"$HANDOFF/$dir/collection.anki2"
      printf '{"ok":true,"scenario_id":"live/round-trip","elapsed_ms":1,"observations":{"steps":[{}]},"state":{"dir":"%s","now":%s,"notes":[],"cards":[]}}\n' \
        "$dir" "$((START + 86400))" ;;
    *'"cmd":"run"'*)
      printf '{"ok":true,"scenario_id":"live/round-trip","elapsed_ms":1,"observations":{"steps":[{},{},{},{},{"timing":{"now":%s}}]}}\n' \
        "$((START + 86400 + 3600))" ;;
  esac
done
"#;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A tree with the round-trip scenario and the golden the oracle records for
/// it (handoffs only close and reopen the collection there).
fn tree(active: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}\n"),
    );
    write(root, "deviations.yaml", "deviations: []\n");
    write(root, "scenarios/live/round-trip.yaml", SCENARIO);
    if active {
        write(root, "scenarios/live/area.yaml", "area: live\nactive: [round-trip]\n");
    }
    let loaded = meguri_testkit::scenario::load(&root.join("scenarios/live/round-trip.yaml")).unwrap();
    let golden = json!({
        "scenario_id": "live/round-trip", "scenario_sha256": loaded.sha256, "tz": "UTC",
        "oracle": {"anki_version": "26.9.3", "lock_sha256": LOCK, "driver_sha256": "b".repeat(64)},
        "clock": {"start": "2030-01-15T10:00:00", "mode": "frozen"},
        "observations": {"steps": [{}, {}, {}, {}, {"timing": {"now": AFTER_MEGURI}}]},
    });
    write(
        root,
        "goldens/live/round-trip.json",
        &serde_json::to_string_pretty(&golden).unwrap(),
    );
    dir
}

fn oracle(dir: &Path, handoff: &Path) -> OracleExecutor {
    let script = dir.join("worker.sh");
    fs::write(&script, WORKER).unwrap();
    let command = vec![
        "env".to_owned(),
        "TZ={tz}".to_owned(),
        format!("LOCK={LOCK}"),
        format!("START={START}"),
        format!("HANDOFF={}", handoff.display()),
        "sh".to_owned(),
        script.display().to_string(),
    ];
    OracleExecutor::new(command, 2).expecting(OraclePin {
        anki_version: "26.9.3".to_owned(),
        lock_sha256: LOCK.to_owned(),
    })
}

/// Meguri's side of the stand-in "collection": it opens what the oracle wrote
/// and writes its own.
fn open_from_oracle(_: &mut Session<'_>, dir: &Path) -> Result<(), ExecError> {
    match fs::read_to_string(dir.join("collection.anki2")) {
        Ok(text) if text == "oracle" => Ok(()),
        other => Err(ExecError::new("anki_error", format!("not a collection: {other:?}"))),
    }
}

fn write_collection(_: &mut Session<'_>, dir: &Path) -> Result<(), ExecError> {
    fs::write(dir.join("collection.anki2"), "meguri").map_err(|e| ExecError::new("handoff_error", e.to_string()))
}

fn write_garbage(_: &mut Session<'_>, dir: &Path) -> Result<(), ExecError> {
    fs::write(dir.join("collection.anki2"), "corrupt").map_err(|e| ExecError::new("handoff_error", e.to_string()))
}

/// An `advance` that moves the clock twice as far: Meguri getting time wrong.
fn advance_twice(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    if let Step::Advance(by) = step {
        session.clock.advance(*by);
        session.clock.advance(*by);
    }
    Ok(OpResult::Done)
}

fn meguri_with(out: meguri_testkit::exec::HandoffFn) -> MeguriExecutor {
    let mut registry = MeguriExecutor::core_registry();
    registry.handoff(out, open_from_oracle);
    MeguriExecutor::with(registry)
}

#[test]
fn a_chain_hands_the_collection_both_ways_and_passes_against_the_golden() {
    let dir = tree(false);
    let handoff = tempfile::tempdir().unwrap();
    let oracle = oracle(dir.path(), handoff.path());
    let meguri = meguri_with(write_collection);
    let live = LiveRun {
        oracle: &oracle,
        meguri: &meguri,
        handoff_root: Some(handoff.path()),
    };
    let report = Suite::open(dir.path()).unwrap().run_live(&[], &live, "test");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Pass, "{}", report.render_text());
    assert!(outcome.live);
    assert!(report.failures().is_empty());
    // Both sides wrote their collection where the other read it.
    let read = |rel: &str| fs::read_to_string(handoff.path().join(rel)).unwrap();
    assert_eq!(read("live/round-trip/1/collection.anki2"), "oracle");
    assert_eq!(read("live/round-trip/3/collection.anki2"), "meguri");
}

#[test]
fn what_the_oracle_observes_of_meguris_collection_is_compared() {
    let dir = tree(false);
    let handoff = tempfile::tempdir().unwrap();
    let oracle = oracle(dir.path(), handoff.path());
    let mut registry = Registry::default();
    registry.op("advance", advance_twice);
    registry.handoff(write_collection, open_from_oracle);
    let meguri = MeguriExecutor::with(registry);
    let live = LiveRun {
        oracle: &oracle,
        meguri: &meguri,
        handoff_root: Some(handoff.path()),
    };
    let report = Suite::open(dir.path()).unwrap().run_live(&[], &live, "test");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Fail, "{}", report.render_text());
    let first = outcome.first_divergence.as_ref().unwrap();
    assert_eq!((first.step, first.pointer.as_str()), (4, "/steps/4/timing/now"));
    assert_eq!(
        (first.expected.clone(), first.actual.clone()),
        (json!(AFTER_MEGURI), json!(AFTER_MEGURI + 3_600))
    );
    assert!(outcome.message.contains("live chain"), "{}", outcome.message);
    // Not active, still a T1 failure.
    assert!(!outcome.active);
    assert_eq!(report.failures().len(), 1);
}

#[test]
fn an_oracle_that_refuses_meguris_collection_fails_t1() {
    let dir = tree(false);
    let handoff = tempfile::tempdir().unwrap();
    let oracle = oracle(dir.path(), handoff.path());
    let meguri = meguri_with(write_garbage);
    let live = LiveRun {
        oracle: &oracle,
        meguri: &meguri,
        handoff_root: Some(handoff.path()),
    };
    let report = Suite::open(dir.path()).unwrap().run_live(&[], &live, "test");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Error, "{}", report.render_text());
    assert!(outcome.message.contains("anki_error"), "{}", outcome.message);
    assert!(outcome.message.contains("oracle side"), "{}", outcome.message);
    assert_eq!(report.failures().len(), 1);
    assert_eq!(report.to_json()["scenarios"][0]["fails"], true);
}

#[test]
fn a_chain_meguri_cannot_join_yet_is_pending_and_fails_only_when_active() {
    let handoff = tempfile::tempdir().unwrap();
    for active in [false, true] {
        let dir = tree(active);
        let oracle = oracle(dir.path(), handoff.path());
        let meguri = MeguriExecutor::new();
        let live = LiveRun {
            oracle: &oracle,
            meguri: &meguri,
            handoff_root: Some(handoff.path()),
        };
        let report = Suite::open(dir.path()).unwrap().run_live(&[], &live, "test");
        let outcome = &report.outcomes[0];
        assert_eq!(outcome.status, Status::Pending, "{}", report.render_text());
        assert!(outcome.missing.as_ref().unwrap().ops.contains("handoff"));
        assert_eq!(report.failures().len(), usize::from(active));
    }
}

#[test]
fn without_a_handoff_directory_a_chain_is_an_error() {
    let dir = tree(false);
    let handoff = tempfile::tempdir().unwrap();
    let oracle = oracle(dir.path(), handoff.path());
    let meguri = meguri_with(write_collection);
    let live = LiveRun {
        oracle: &oracle,
        meguri: &meguri,
        handoff_root: None,
    };
    let report = Suite::open(dir.path()).unwrap().run_live(&[], &live, "test");
    assert_eq!(report.outcomes[0].status, Status::Error);
    assert!(report.outcomes[0].message.contains("handoff directory"));
}
