//! The storage area's adapters on Meguri's executor (specs/conformance/
//! storage-ops.md P1 to P3, observations.md O8; storage/schema-18 S6.2):
//! what they do beyond what the committed storage goldens show. Scenarios
//! live in a temporary conformance tree with copies of the committed
//! fixtures.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor, OpResult, SegmentExecutor, SegmentRequest, Session};
use meguri_testkit::scenario::{self, LoadedScenario, Step};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn repo_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage")
}

/// A scenario file `scenarios/<id>.yaml` in a temporary conformance tree
/// whose `fixtures/storage/` holds the named fixture copies.
fn load(dir: &Path, id: &str, body: &str, fixtures: &[&str]) -> LoadedScenario {
    let storage = dir.join("fixtures/storage");
    fs::create_dir_all(&storage).unwrap();
    for name in fixtures {
        fs::copy(repo_fixtures().join(name), storage.join(name)).unwrap();
    }
    let path = dir.join("scenarios").join(format!("{id}.yaml"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!("id: {id}\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{body}"),
    )
    .unwrap();
    scenario::load(&path).unwrap()
}

#[test]
fn after_a_refused_open_only_another_open_may_run() {
    let dir = tempfile::tempdir().unwrap();
    let ok = load(
        dir.path(),
        "storage/again",
        "steps:\n  - open: {fixture: storage/ver-10-wal.anki2}\n  - open: {fixture: storage/schema-18.anki2}\n  - observe: [schema_objects]\n",
        &["ver-10-wal.anki2", "schema-18.anki2"],
    );
    let out = MeguriExecutor::new().run(&ok).unwrap();
    assert_eq!(out["steps"][0], json!({"open": {"error": "file_too_old"}}));
    assert_eq!(out["steps"][1], json!({}));
    assert!(out["steps"][2]["schema_objects"].as_array().unwrap().len() >= 28);

    let bad = load(
        dir.path(),
        "storage/after-refusal",
        "steps:\n  - open: {fixture: storage/ver-13-delete.anki2}\n  - observe: [schema_objects]\n",
        &["ver-13-delete.anki2"],
    );
    let error = MeguriExecutor::new().run(&bad).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("invalid_scenario", Some(1)));
}

#[test]
fn a_missing_fixture_is_an_invalid_scenario() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "storage/missing",
        "steps:\n  - open: {fixture: storage/nowhere.anki2}\n",
        &[],
    );
    let error = MeguriExecutor::new().run(&loaded).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("invalid_scenario", Some(0)));
}

#[test]
fn db_exec_binds_parameters_by_type_and_reports_values_by_storage_class() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "storage/params",
        concat!(
            "steps:\n  - open: {fixture: storage/schema-18.anki2}\n",
            "  - db_exec: {sql: \"SELECT ?, ?, ?, ?, typeof(?), 1.0, X'00ff'\", params: [a, 7, 1.5, null, 2.0]}\n",
            "  - db_exec: {sql: \"CREATE TABLE addon (x)\"}\n",
            "  - observe: [{db_dump: {tables: [notes]}}]\n",
        ),
        &["schema-18.anki2"],
    );
    let out = MeguriExecutor::new().run(&loaded).unwrap();
    let digest: String = Sha256::digest([0x00, 0xff])
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        out["steps"][1]["db_exec"]["rows"],
        json!([["a", 7, 1.5, null, "real", 1.0, {"sha256": digest}]])
    );
    assert!(out["steps"][1]["db_exec"]["rows"][0][5].is_f64(), "REAL 1 is 1.0");
    // A statement may change the schema: the guard takes it as the new
    // baseline.
    assert_eq!(out["steps"][2], json!({"db_exec": {"rows": []}}));
    let notes = out["steps"][3]["db_dump"]["notes"].as_object().unwrap();
    assert!(!notes.is_empty());
    assert!(notes.values().all(|n| n["guid"] == "#random"));
}

/// An op of a later area that creates a table: the guard must stop the run.
fn add_table(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    let collection = session.storage.collection().unwrap();
    collection
        .writer()
        .execute_raw("CREATE TABLE sneaked_in (x)", &[])
        .map_err(|e| ExecError::new("core_error", e.to_string()))?;
    Ok(OpResult::Done)
}

#[test]
fn an_op_that_changes_the_schema_fails_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = MeguriExecutor::core_registry();
    registry.op("check_db", add_table);
    let meguri = MeguriExecutor::with(registry);
    let at_end = load(
        dir.path(),
        "storage/guard-end",
        "steps:\n  - open: {fixture: storage/schema-18.anki2}\n  - check_db: {}\n",
        &["schema-18.anki2"],
    );
    assert_eq!(meguri.run(&at_end).unwrap_err().kind, "schema_guard");
    let before_db_exec = load(
        dir.path(),
        "storage/guard-db-exec",
        "steps:\n  - open: {fixture: storage/schema-18.anki2}\n  - check_db: {}\n  - db_exec: {sql: \"SELECT 1\"}\n",
        &["schema-18.anki2"],
    );
    let error = meguri.run(&before_db_exec).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("schema_guard", Some(2)));
}

/// Ops of a later area that change a definition but no `(type, name)`.
fn add_column(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    raw(session, &["ALTER TABLE cards ADD COLUMN sneaked_in integer"])
}

fn redefine_index(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    raw(
        session,
        &[
            "DROP INDEX ix_cards_usn",
            "CREATE INDEX ix_cards_usn ON cards (usn, mod)",
        ],
    )
}

fn raw(session: &mut Session<'_>, statements: &[&str]) -> Result<OpResult, ExecError> {
    let writer = session.storage.collection().unwrap().writer();
    for sql in statements {
        writer
            .execute_raw(sql, &[])
            .map_err(|e| ExecError::new("core_error", e.to_string()))?;
    }
    Ok(OpResult::Done)
}

#[test]
fn an_op_that_adds_a_column_or_redefines_an_index_fails_the_run() {
    let dir = tempfile::tempdir().unwrap();
    for (name, adapter) in [
        (
            "column",
            add_column as fn(&mut Session<'_>, &Step) -> Result<OpResult, ExecError>,
        ),
        ("index", redefine_index),
    ] {
        let mut registry = MeguriExecutor::core_registry();
        registry.op("check_db", adapter);
        let meguri = MeguriExecutor::with(registry);
        let loaded = load(
            dir.path(),
            &format!("storage/guard-{name}"),
            "steps:\n  - open: {fixture: storage/schema-18.anki2}\n  - check_db: {}\n",
            &["schema-18.anki2"],
        );
        let error = meguri.run(&loaded).unwrap_err();
        assert_eq!(error.kind, "schema_guard", "{name}: {error:?}");
    }
}

#[test]
fn a_handoff_closes_the_collection_and_the_next_segment_opens_what_was_handed() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "storage/handed",
        concat!(
            "steps:\n  - open: {fixture: storage/schema-14.anki2}\n",
            "  - db_exec: {sql: \"UPDATE cards SET flags = 5\"}\n",
            "  - handoff: oracle\n",
            "  - db_exec: {sql: \"SELECT DISTINCT flags FROM cards\"}\n",
        ),
        &["schema-14.anki2"],
    );
    let handoffs = tempfile::tempdir().unwrap();
    let meguri = MeguriExecutor::new();
    let first = meguri
        .segment(
            &loaded,
            &SegmentRequest {
                resume: None,
                steps: 0..2,
                hand_off_to: Some("storage/handed@2".to_owned()),
                root: handoffs.path().to_path_buf(),
            },
        )
        .unwrap();
    let handoff = first.handoff.unwrap();
    let handed = handoffs.path().join(&handoff.dir);
    let mut files: Vec<String> = fs::read_dir(&handed)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    // Closed cleanly (no -wal or -shm), with the pre-upgrade copy.
    assert_eq!(files, ["collection.anki2", "collection.anki2.pre-upgrade-v14"]);

    let second = meguri
        .segment(
            &loaded,
            &SegmentRequest {
                resume: Some(handoff),
                steps: 3..4,
                hand_off_to: None,
                root: handoffs.path().to_path_buf(),
            },
        )
        .unwrap();
    assert_eq!(second.steps, [json!({"db_exec": {"rows": [[5]]}})]);
}

#[test]
fn schema_objects_are_sorted_with_normalized_text() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "storage/objects",
        concat!(
            "steps:\n  - open: {fixture: storage/schema-18.anki2}\n",
            "  - db_exec: {sql: \"CREATE TABLE addon (\\n  a text, -- note\\n\\tb   blob\\n)\"}\n",
            "  - observe: [schema_objects]\n",
        ),
        &["schema-18.anki2"],
    );
    let out = MeguriExecutor::new().run(&loaded).unwrap();
    let objects = out["steps"][2]["schema_objects"].as_array().unwrap();
    let keys: Vec<(String, String)> = objects
        .iter()
        .map(|o| {
            (
                o["type"].as_str().unwrap().to_owned(),
                o["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let addon = objects.iter().find(|o| o["name"] == "addon").unwrap();
    assert_eq!(
        addon,
        &json!({"type": "table", "name": "addon", "tbl_name": "addon", "sql": "CREATE TABLE addon ( a text, b blob )"})
    );
    assert!(objects.iter().all(|o| o["sql"].is_string() || o["sql"] == Value::Null));
}
