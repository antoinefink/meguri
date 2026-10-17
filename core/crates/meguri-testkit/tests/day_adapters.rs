//! The day clock's adapters on Meguri's executor (`specs/sched/day-clock.md`
//! D14.2): `timing`, `reopen`, and what `set_config`, `db_exec` and `open`
//! do to the cached day. The day scenarios themselves start from a new
//! collection, which the executor cannot make yet (U28); this scenario opens
//! the schema-18 fixture instead and sets its `crt` and creation offset. The
//! expected values are the spec's worked examples (D3.1, D6, D7).

use std::fs;
use std::path::Path;

use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::scenario;
use serde_json::json;

const SCENARIO: &str = r#"id: sched/adapters
clock: {tz: Europe/Paris, start: "2030-01-15T10:00:00", mode: step}
steps:
  - open: {fixture: storage/schema-18.anki2}
  - db_exec: {sql: "UPDATE col SET crt = 1894676400"}
  - db_exec: {sql: "UPDATE config SET val = CAST('-60' AS BLOB) WHERE KEY = 'creationOffset'"}
  - observe: [timing]
  - db_exec: {sql: "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY IN ('lastUnburied', 'localOffset') ORDER BY KEY"}
  - set_clock: "2030-01-20T10:00:00"
  - observe: [timing]
  - db_exec: {sql: "SELECT id, queue, mod, usn FROM cards WHERE id = 1788249600246"}
  - set_config: {key: rollover, value: 12}
  - db_exec: {sql: "SELECT usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY = 'lastUnburied'"}
  - set_clock: "2030-01-18T10:00:00"
  - observe: [timing]
  - reopen: {}
  - observe: [timing]
"#;

#[test]
fn timing_reopen_and_the_cached_day_follow_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("fixtures/storage");
    fs::create_dir_all(&storage).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        storage.join("schema-18.anki2"),
    )
    .unwrap();
    let path = dir.path().join("scenarios/sched/adapters.yaml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, SCENARIO).unwrap();
    let loaded = scenario::load(&path).unwrap();

    let executor = MeguriExecutor::new();
    assert!(executor.missing(&loaded.scenario.needs()).is_empty());
    let out = executor.run(&loaded).unwrap();
    let steps = &out["steps"];
    let timing = |days: u32, next: i64, now: i64| json!({"timing": {"days_elapsed": days, "next_day_at": next, "now": now, "utc_offset_secs": 3600}});
    // D3.1: day 0 after creation at 04:00 +01:00; the first query writes
    // localOffset and leaves lastUnburied unwritten (0 is not below 0).
    assert_eq!(steps[3], timing(0, 1_894_762_800, 1_894_698_000));
    assert_eq!(
        steps[4],
        json!({"db_exec": {"rows": [["localOffset", -1, 1_894_698_000, "-60"]]}})
    );
    // Day 5: the fixture's user-buried new card is back in the new queue
    // with its mod and usn (D7).
    assert_eq!(steps[6], timing(5, 1_895_194_800, 1_895_130_000));
    assert_eq!(
        steps[7],
        json!({"db_exec": {"rows": [[1_788_249_600_246_i64, 0, 1_788_681_600, -1]]}})
    );
    // rollover 12 at 10:00 computes afresh at once (D8): day 4, which is
    // not more than a week below lastUnburied 5, so nothing is written.
    assert_eq!(steps[9], json!({"db_exec": {"rows": [[-1, 1_895_130_000, "5"]]}}));
    // The clock moved back: the cached day 4 holds until 12:00 on the 20th
    // (D6), and a reopen computes afresh: day 2 until 12:00 on the 18th.
    assert_eq!(steps[11], timing(4, 1_895_137_200, 1_894_957_200));
    assert_eq!(steps[13], timing(2, 1_894_964_400, 1_894_957_200));
}
