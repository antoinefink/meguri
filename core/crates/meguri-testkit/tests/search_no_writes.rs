//! A search never writes, sorted or not (`specs/conformance/browser-ops.md`
//! B1): on Meguri's executor, searches whose sort key or term reads the day
//! leave buried cards buried and store no timing key, although the day has
//! moved past `lastUnburied` and a day query would unbury them
//! (`sched/day-clock` D7). The schema-18 fixture's `crt` and creation offset
//! are set as `day_adapters.rs` sets them.
//!
//! A browser scenario's searches run through the core's result sets
//! (`browser/result-sets` H16), on the hosted engine, whose open runs the
//! day check (D8). So the first search promotes the collection on the
//! creation day, which unburies the fixture's buried cards; the scenario then
//! buries cards again and moves the clock five days on.

use std::fs;
use std::path::Path;

use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::scenario;
use serde_json::{Value, json};

const SCENARIO: &str = r#"id: browser/no-writes
clock: {tz: Europe/Paris, start: "2030-01-15T10:00:00"}
steps:
  - open: {fixture: storage/schema-18.anki2}
  - db_exec: {sql: "UPDATE col SET crt = 1894676400"}
  - db_exec: {sql: "UPDATE config SET val = CAST('-60' AS BLOB) WHERE KEY = 'creationOffset'"}
  - observe:
      - search:
          - {query: "is:buried"}
  - db_exec: {sql: "UPDATE cards SET queue = -3 WHERE id IN (SELECT id FROM cards ORDER BY id LIMIT 2)"}
  - set_clock: "2030-01-20T10:00:00"
  - db_exec: {sql: "SELECT id, queue, mod, usn FROM cards WHERE queue IN (-2, -3) ORDER BY id"}
  - db_exec: {sql: "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY IN ('lastUnburied', 'localOffset', 'rollover') ORDER BY KEY"}
  - observe:
      - search:
          - {query: "is:buried", order: {column: cardDue}}
          - {query: "is:buried", order: {column: cardDue, reverse: true}}
          - {query: "is:buried -prop:due>100000", order: {column: cardMod}}
          - {query: "is:buried -is:due"}
  - db_exec: {sql: "SELECT id, queue, mod, usn FROM cards WHERE queue IN (-2, -3) ORDER BY id"}
  - db_exec: {sql: "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY IN ('lastUnburied', 'localOffset', 'rollover') ORDER BY KEY"}
"#;

fn sorted_ids(value: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = value["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_i64().expect("fixture cards have no logical ids"))
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn searches_that_read_the_day_leave_buried_cards_and_timing_keys_alone_b1() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("fixtures/storage");
    fs::create_dir_all(&storage).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        storage.join("schema-18.anki2"),
    )
    .unwrap();
    let path = dir.path().join("scenarios/browser/no-writes.yaml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, SCENARIO).unwrap();
    let loaded = scenario::load(&path).unwrap();
    let executor = MeguriExecutor::new();
    assert!(executor.missing(&loaded.scenario.needs()).is_empty());
    let out = executor.run(&loaded).unwrap();
    let steps = &out["steps"];

    let buried = steps[6]["db_exec"]["rows"].as_array().unwrap().clone();
    assert!(!buried.is_empty(), "the fixture holds buried cards");
    let buried_ids: Vec<i64> = buried.iter().map(|row| row[0].as_i64().unwrap()).collect();
    let searches = steps[8]["search"].as_array().unwrap();
    // Every search still finds the buried cards, sorted or not, whether the
    // sort key (Due) or a term (`prop:due`, `is:due`) reads the day.
    for search in searches {
        assert_eq!(sorted_ids(search), buried_ids, "{search}");
    }
    // Ascending and descending Due are each other's reverse (S8).
    let mut descending = searches[1]["ids"].as_array().unwrap().clone();
    descending.reverse();
    assert_eq!(searches[0]["ids"].as_array().unwrap(), &descending);
    // Nothing was written: queues, stamps and timing keys as before.
    assert_eq!(steps[9], json!({"db_exec": {"rows": buried}}));
    assert_eq!(steps[10], steps[7]);
}
