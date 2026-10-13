//! The oracle executor's worker pool (specs/conformance/dsl.md D8), driven
//! against a stand-in worker that speaks the protocol (conformance/schema/
//! protocol.json). `tools/mg conformance --live` drives the real image.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{Executor, OracleExecutor};
use meguri_testkit::scenario;

/// A worker that answers hello and run lines. It reports its own TZ and
/// process id, fails a scenario containing "teleport" with unsupported_op at
/// step 1, and exits on one containing "die".
const WORKER: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"cmd":"hello"'*)
      printf '{"ok":true,"protocol":1,"anki_version":"26.9.3","lock_sha256":"%s","driver_sha256":"%s","tz":"%s"}\n' \
        "$(printf 'a%.0s' $(seq 64))" "$(printf 'b%.0s' $(seq 64))" "$TZ" ;;
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
    let script = dir.join("worker.sh");
    fs::write(&script, WORKER).unwrap();
    ["env", "TZ={tz}", "sh"]
        .iter()
        .map(|s| (*s).to_owned())
        .chain([script.display().to_string()])
        .collect()
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
