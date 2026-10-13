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
/// "collection" into the handoff directory and hands over one day later; like
/// the driver, it refuses a handoff directory that already exists. With
/// `SYNC` set, first segments wait (up to 2 s) until two of them are in
/// flight. A resumed segment opens the collection it was handed (Anki refuses
/// one that is not a collection) and observes the clock it was handed. The
/// request's keys come sorted (serde_json), so the top-level `handoff` and
/// `steps` are found by their position.
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
      if [ -n "${SYNC:-}" ]; then
        touch "$SYNC/$(printf '%s' "$dir" | tr '/@' '__')"
        i=0
        while [ "$(ls "$SYNC" | wc -l)" -lt 2 ] && [ "$i" -lt 100 ]; do sleep 0.02; i=$((i + 1)); done
      fi
      if [ -e "$HANDOFF/$dir" ]; then
        printf '{"ok":false,"error":{"kind":"handoff_error","detail":"the handoff directory already exists"}}\n'
      else
        mkdir -p "$HANDOFF/$dir" && printf oracle >"$HANDOFF/$dir/collection.anki2"
        printf '{"ok":true,"scenario_id":"live/round-trip","elapsed_ms":1,"observations":{"steps":[{}]},"state":{"dir":"%s","now":%s,"notes":[],"cards":[]}}\n' \
          "$dir" "$((START + 86400))"
      fi ;;
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
    let dir = tree_of(&["live/round-trip"]);
    if active {
        write(
            dir.path(),
            "scenarios/live/area.yaml",
            "area: live\nactive: [round-trip]\n",
        );
    }
    dir
}

/// A tree with the round-trip scenario under each of these ids.
fn tree_of(ids: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}\n"),
    );
    write(root, "deviations.yaml", "deviations: []\n");
    for id in ids {
        let rel = format!("scenarios/{id}.yaml");
        write(root, &rel, &SCENARIO.replace("live/round-trip", id));
        let loaded = meguri_testkit::scenario::load(&root.join(&rel)).unwrap();
        let golden = json!({
            "scenario_id": id, "scenario_sha256": loaded.sha256, "tz": "UTC",
            "oracle": {"anki_version": "26.9.3", "lock_sha256": LOCK, "driver_sha256": "b".repeat(64)},
            "clock": {"start": "2030-01-15T10:00:00", "mode": "frozen"},
            "observations": {"steps": [{}, {}, {}, {}, {"timing": {"now": AFTER_MEGURI}}]},
        });
        write(
            root,
            &format!("goldens/{id}.json"),
            &serde_json::to_string_pretty(&golden).unwrap(),
        );
    }
    dir
}

fn oracle(dir: &Path, handoff: &Path) -> OracleExecutor {
    oracle_synced(dir, handoff, None)
}

/// With `sync`, first segments wait for each other there (WORKER).
fn oracle_synced(dir: &Path, handoff: &Path, sync: Option<&Path>) -> OracleExecutor {
    let script = dir.join("worker.sh");
    fs::write(&script, WORKER).unwrap();
    let mut command = vec![
        "env".to_owned(),
        "TZ={tz}".to_owned(),
        format!("LOCK={LOCK}"),
        format!("START={START}"),
        format!("HANDOFF={}", handoff.display()),
    ];
    if let Some(sync) = sync {
        command.push(format!("SYNC={}", sync.display()));
    }
    command.extend(["sh".to_owned(), script.display().to_string()]);
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
    assert_eq!(read("live/round-trip@1/collection.anki2"), "oracle");
    assert_eq!(read("live/round-trip@3/collection.anki2"), "meguri");
}

/// D1 allows both `live/rt` and `live/rt/1`, which sort next to each other,
/// so T1 runs their chains at the same time; the oracle refuses a handoff
/// directory that exists, and each side reads what it was handed.
#[test]
fn chains_whose_ids_nest_as_paths_run_at_once_in_their_own_handoff_directories() {
    let ids = ["live/rt", "live/rt/1"];
    for _ in 0..5 {
        let dir = tree_of(&ids);
        let handoff = tempfile::tempdir().unwrap();
        let sync = tempfile::tempdir().unwrap();
        let oracle = oracle_synced(dir.path(), handoff.path(), Some(sync.path()));
        let meguri = meguri_with(write_collection);
        let live = LiveRun {
            oracle: &oracle,
            meguri: &meguri,
            handoff_root: Some(handoff.path()),
        };
        let report = Suite::open(dir.path()).unwrap().run_live(&[], &live, "test");
        assert_eq!(report.outcomes.len(), 2);
        for outcome in &report.outcomes {
            assert_eq!(outcome.status, Status::Pass, "{}", report.render_text());
        }
        // Both chains' first segments reached the oracle.
        assert_eq!(fs::read_dir(sync.path()).unwrap().count(), 2);
        let list = |rel: &str| {
            let mut names: Vec<String> = fs::read_dir(handoff.path().join(rel))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(list("live"), ["rt", "rt@1", "rt@3"]);
        assert_eq!(list("live/rt"), ["1@1", "1@3"]);
        for id in ids {
            for (step, side) in [(1, "oracle"), (3, "meguri")] {
                let rel = format!("{id}@{step}");
                assert_eq!(list(&rel), ["collection.anki2"], "{rel}");
                assert_eq!(
                    fs::read_to_string(handoff.path().join(&rel).join("collection.anki2")).unwrap(),
                    side
                );
            }
        }
    }
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
