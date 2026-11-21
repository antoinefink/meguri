//! `stats/card-info` C14.1 through Meguri's `card_stats` adapter
//! (`conformance/stats-ops` H9): every recorded scenario against its golden,
//! with the deviations of C13.4 applied.
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    observe::parse_json,
    scenario,
    suite::Suite,
};
use std::path::Path;

const SCENARIOS: [&str; 18] = [
    "ae46-no-state",
    "back-to-back-answers",
    "cloze-type",
    "custom-data",
    "due-estimate-night",
    "edge-values",
    "filtered-card",
    "intraday-learning",
    "memory-and-retention",
    "mixed-history",
    "new-card",
    "no-item",
    "no-lrt-elapsed",
    "params",
    "reset-position",
    "review-card",
    "times",
    "zone-dst",
];

#[test]
fn every_recorded_card_info_scenario_matches_its_golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let mut failures = Vec::new();
    for name in SCENARIOS {
        let id = format!("stats/card-info/{name}");
        let loaded = scenario::load(&root.join(format!("scenarios/{id}.yaml"))).unwrap();
        let expected = parse_json(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
        assert_eq!(expected["scenario_sha256"].as_str(), Some(loaded.sha256.as_str()), "{id}: stale golden");
        let missing = executor.missing(&loaded.scenario.needs());
        if !missing.is_empty() {
            failures.push(format!("{id}: missing {missing:?}"));
            continue;
        }
        let actual = match executor.run(&loaded) {
            Ok(actual) => actual,
            Err(error) => {
                failures.push(format!("{id}: run error {error}"));
                continue;
            }
        };
        let observed = |value: &serde_json::Value| {
            value["steps"]
                .as_array()
                .map_or(0, |steps| steps.iter().filter(|step| step.get("card_stats").is_some()).count())
        };
        assert!(observed(&actual) > 0, "{id}: no card_stats observation ran");
        assert_eq!(observed(&actual), observed(&expected["observations"]), "{id}");
        let compared = diff::compare(
            &id,
            &expected["observations"],
            &actual,
            &CompareOptions {
                set: &loaded.scenario.compare.set,
                deviations: &suite.deviations,
                overlay: &[],
            },
        );
        if !compared.differences.is_empty() {
            failures.push(format!("{id}: {:#?}", compared.differences));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}


/// C1 (R1252, R1425) through the product entry point on a hosted engine:
/// reading Card Info and its dump for every card of the schema-18 fixture
/// leaves the collection file, its WAL and every card row and `col.mod` as
/// they were after the open.
#[test]
fn the_hosted_card_info_read_writes_nothing() {
    use meguri_model::{ids::CardId, rng::SeededRandom};
    use meguri_storage::{ReaderRole, raw::SqlValue};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let collection_dir = dir.path().join("collection");
    std::fs::create_dir(&collection_dir).unwrap();
    let file = collection_dir.join("collection.anki2");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &file,
    )
    .unwrap();
    let clock = Arc::new(meguri_testkit::exec::clock::ScenarioClock::new("Europe/Paris", "2030-01-15T10:00:00").unwrap());
    let host = meguri_host::CoreHost::open(meguri_host::HostConfig::new(
        collection_dir.clone(),
        dir.path().join("backups"),
        dir.path().join("widget.snapshot"),
        clock,
        Arc::new(SeededRandom::new(5)),
        Arc::new(meguri_storage::space::StatvfsSpace),
    ))
    .unwrap();
    let collection = host.collection().unwrap();
    let state = || {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        let rows = reader
            .execute_raw("SELECT (SELECT mod FROM col), id, mod, usn, data FROM cards ORDER BY id", &[])
            .unwrap()
            .rows;
        let wal = std::fs::metadata(collection_dir.join("collection.anki2-wal")).map(|m| m.len()).ok();
        (rows, std::fs::read(&file).unwrap(), wal)
    };
    let before = state();
    let ids: Vec<i64> = before
        .0
        .iter()
        .map(|row| match row[1] {
            SqlValue::Integer(id) => id,
            ref other => panic!("{other:?}"),
        })
        .collect();
    assert!(!ids.is_empty());
    for id in ids {
        let _ = meguri_stats::card_info(&collection, CardId(id)).unwrap();
        let _ = meguri_stats::card_info_debug_json(&collection, CardId(id)).unwrap();
    }
    assert_eq!(state(), before);
    drop(collection);
    host.close().unwrap();
}
