//! The oracle executor's worker pool (specs/conformance/dsl.md D8), driven
//! against a stand-in worker that speaks the protocol (conformance/schema/
//! protocol.json). `tools/mg conformance --live` drives the real image.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{Executor, OracleExecutor};
use meguri_testkit::scenario;
use meguri_testkit::suite::OraclePin;

/// A worker that answers hello and run lines. It reports its own TZ and
/// process id, fails a scenario containing "teleport" with unsupported_op at
/// step 1, and exits on one containing "die". Its lock is all `a`s, except
/// $LOCK_<zone> (Asia/Kathmandu: LOCK_Asia_Kathmandu) or $DRIVER_<zone>
/// when set.
const WORKER: &str = r#"
zone=$(printf '%s' "$TZ" | tr '/' '_')
lock=$(eval "printf '%s' \"\${LOCK_$zone:-$(printf 'a%.0s' $(seq 64))}\"")
driver=$(eval "printf '%s' \"\${DRIVER_$zone:-$(printf 'b%.0s' $(seq 64))}\"")
while IFS= read -r line; do
  case "$line" in
    *'"cmd":"hello"'*)
      printf '{"ok":true,"protocol":1,"anki_version":"26.9.3","lock_sha256":"%s","driver_sha256":"%s","tz":"%s"}\n' \
        "$lock" "$driver" "$TZ" ;;
    *exact*)
      printf '{"ok":true,"scenario_id":"x","elapsed_ms":1,"observations":{"steps":[{"stats":{"retention":39.430133835633676}}]}}\n' ;;
    *teleport*)
      printf '{"ok":false,"error":{"kind":"unsupported_op","op":"teleport","step":1}}\n' ;;
    *'"die"'*)
      exit 3 ;;
    *'"cmd":"run"'*)
      printf '{"ok":true,"scenario_id":"x","elapsed_ms":1,"observations":{"steps":[{"timing":{"tz":"%s","pid":%s}}]}}\n' "$TZ" "$$" ;;
  esac
done
"#;

fn worker_command(dir: &Path) -> Vec<String> {
    worker_command_with(dir, &[])
}

/// The stand-in worker with extra environment (`NAME=value`).
fn worker_command_with(dir: &Path, env: &[&str]) -> Vec<String> {
    let script = dir.join("worker.sh");
    fs::write(&script, WORKER).unwrap();
    ["env", "TZ={tz}"]
        .iter()
        .chain(env)
        .chain(&["sh"])
        .map(|s| (*s).to_owned())
        .chain([script.display().to_string()])
        .collect()
}

fn pin(lock: char) -> OraclePin {
    OraclePin {
        anki_version: "26.9.3".to_owned(),
        lock_sha256: lock.to_string().repeat(64),
    }
}

fn scenario_in(dir: &Path, name: &str, tz: &str, step: &str) -> scenario::LoadedScenario {
    let path: PathBuf = dir.join(format!("{name}.yaml"));
    fs::write(
        &path,
        format!("id: pool/{name}\nclock: {{tz: {tz}, start: \"2030-01-15T10:00:00\"}}\nsteps: [{step}]\n"),
    )
    .unwrap();
    scenario::load(&path).unwrap()
}

#[test]
fn scenarios_run_on_workers_of_their_own_time_zone() {
    let dir = tempfile::tempdir().unwrap();
    let pool = OracleExecutor::new(worker_command(dir.path()), 2);
    let paris = scenario_in(dir.path(), "a", "Europe/Paris", "{observe: [timing]}");
    let kathmandu = scenario_in(dir.path(), "b", "Asia/Kathmandu", "{observe: [timing]}");

    let first = pool.run(&paris).unwrap();
    let second = pool.run(&kathmandu).unwrap();
    let third = pool.run(&paris).unwrap();
    assert_eq!(first["steps"][0]["timing"]["tz"], "Europe/Paris");
    assert_eq!(second["steps"][0]["timing"]["tz"], "Asia/Kathmandu");
    // The Paris worker stays alive and serves the next Paris scenario.
    assert_eq!(first["steps"][0]["timing"]["pid"], third["steps"][0]["timing"]["pid"]);
    assert_ne!(first["steps"][0]["timing"]["pid"], second["steps"][0]["timing"]["pid"]);

    let hello = pool.hello().unwrap();
    assert_eq!(hello.anki_version, "26.9.3");
    assert_eq!(hello.lock_sha256, "a".repeat(64));
}

#[test]
fn errors_are_structured_and_a_dead_worker_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let pool = OracleExecutor::new(worker_command(dir.path()), 1);
    let unknown = scenario_in(
        dir.path(),
        "c",
        "UTC",
        "{observe: [timing]}, {db_exec: {sql: teleport}}",
    );
    let err = pool.run(&unknown).unwrap_err();
    assert_eq!(err.kind, "unsupported_op");
    assert_eq!(err.step, Some(1));

    let dying = scenario_in(dir.path(), "d", "UTC", "{db_exec: {sql: die}}");
    let err = pool.run(&dying).unwrap_err();
    assert_eq!(err.kind, "worker_died");

    // With one worker allowed, a scenario in another zone still runs: the
    // idle worker of the first zone gives way.
    let fine = scenario_in(dir.path(), "e", "Europe/Paris", "{observe: [timing]}");
    assert_eq!(pool.run(&fine).unwrap()["steps"][0]["timing"]["tz"], "Europe/Paris");
    let utc = scenario_in(dir.path(), "f", "UTC", "{observe: [timing]}");
    assert_eq!(pool.run(&utc).unwrap()["steps"][0]["timing"]["tz"], "UTC");
}

#[test]
fn a_worker_command_that_cannot_start_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let pool = OracleExecutor::new(vec![dir.path().join("no-such-worker").display().to_string()], 1);
    let s = scenario_in(dir.path(), "g", "UTC", "{observe: [timing]}");
    assert_eq!(pool.run(&s).unwrap_err().kind, "worker_start");
}

#[test]
fn every_worker_must_run_the_pinned_oracle() {
    let dir = tempfile::tempdir().unwrap();
    let utc = scenario_in(dir.path(), "h", "UTC", "{observe: [timing]}");
    let kathmandu = scenario_in(dir.path(), "i", "Asia/Kathmandu", "{observe: [timing]}");

    // The first worker already runs another lock than the pin.
    let pool = OracleExecutor::new(worker_command(dir.path()), 2).expecting(pin('c'));
    let err = pool.run(&utc).unwrap_err();
    assert_eq!(err.kind, "oracle_mismatch");
    assert!(err.detail.contains("conformance/oracle.json"), "{}", err.detail);

    // A later worker of another zone runs another lock.
    let other_lock = format!("LOCK_Asia_Kathmandu={}", "d".repeat(64));
    let pool = OracleExecutor::new(worker_command_with(dir.path(), &[&other_lock]), 2).expecting(pin('a'));
    assert!(pool.run(&utc).is_ok());
    assert_eq!(pool.run(&kathmandu).unwrap_err().kind, "oracle_mismatch");
    assert_eq!(pool.hello().unwrap().lock_sha256, "a".repeat(64));

    // Without a pin, a later worker must still match the first one's driver.
    let other_driver = format!("DRIVER_Asia_Kathmandu={}", "e".repeat(64));
    let pool = OracleExecutor::new(worker_command_with(dir.path(), &[&other_driver]), 2);
    assert!(pool.run(&utc).is_ok());
    let err = pool.run(&kathmandu).unwrap_err();
    assert_eq!(err.kind, "oracle_mismatch");
    assert!(err.detail.contains("driver"), "{}", err.detail);
}

#[test]
fn responses_keep_seventeen_digit_floats_exact() {
    let dir = tempfile::tempdir().unwrap();
    let pool = OracleExecutor::new(worker_command(dir.path()), 1);
    let s = scenario_in(dir.path(), "exact", "UTC", "{observe: [timing]}");
    let out = pool.run(&s).unwrap();
    let x = out["steps"][0]["stats"]["retention"].as_f64().unwrap();
    assert_eq!(x.to_bits(), 39.430133835633676_f64.to_bits());
}

#[test]
fn exclusive_checkouts_keep_factory_identity_and_quarantine_after_end() {
    use meguri_testkit::exec::SegmentExecutor;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("interactive.py");
    fs::write(&script, r#"
import json,sys
for line in sys.stdin:
    cmd=json.loads(line)['cmd']
    if cmd=='exit': sys.exit(7)
    if cmd=='hello':
        print(json.dumps({'ok':True,'anki_version':'26.9.3','lock_sha256':'a'*64,'driver_sha256':'b'*64,'tz':'UTC'}),flush=True)
    else: print('{"ok":true}',flush=True)
"#).unwrap();
    let pool = OracleExecutor::new(
        vec![
            "python3".into(),
            "-I".into(),
            "-B".into(),
            script.display().to_string(),
            "{worker}".into(),
        ],
        4,
    );
    let mut active = Vec::new();
    for _ in 0..4 {
        let session = pool.interactive("UTC").unwrap();
        let identity = session.worker_identity().unwrap().to_owned();
        assert_eq!(identity.len(), 32);
        assert!(active.iter().all(|(id, _)| id != &identity));
        active.push((identity, session));
    }
    let (failed, mut session) = active.remove(0);
    session.quarantine();
    session.request(&json!({"cmd":"end"})).unwrap();
    drop(session);
    let replacement = pool.interactive("UTC").unwrap();
    assert_ne!(replacement.worker_identity().unwrap(), failed);
    assert!(
        active
            .iter()
            .all(|(id, _)| id != replacement.worker_identity().unwrap())
    );
    drop(replacement);
    // A peer remained active throughout; safe successful end reuses its exact
    // identity. Quarantining A never consumed all four pool slots.
    let (safe, mut session) = active.remove(0);
    session.request(&json!({"cmd":"end"})).unwrap();
    drop(session);
    let mut reused = pool.interactive("UTC").unwrap();
    assert_eq!(reused.worker_identity().unwrap(), safe);
    assert_eq!(reused.request(&json!({"cmd":"exit"})).unwrap_err().kind, "worker_died");
    drop(reused);
    assert_ne!(pool.interactive("UTC").unwrap().worker_identity().unwrap(), safe);
}
