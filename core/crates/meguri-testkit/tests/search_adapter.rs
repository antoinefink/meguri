//! The `search` and `canonical_search` observation kinds on Meguri's executor
//! (`specs/conformance/search-ops.md`). The committed search goldens need
//! setup sections Meguri lacks or terms of later units, so these runs open
//! a storage fixture, replace its rows with known ones through `db_exec` and
//! expect what S1-S3 and `specs/search/query-model.md` Q2, Q5 and Q6 say of
//! those rows.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{Executor, MeguriExecutor};
use meguri_testkit::scenario::{self, LoadedScenario};
use serde_json::{Value, json};

fn repo_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage")
}

/// A scenario `scenarios/<id>.yaml` in a temporary conformance tree whose
/// `fixtures/storage/` holds a copy of `schema-18.anki2`.
fn load(dir: &Path, id: &str, body: &str) -> LoadedScenario {
    let storage = dir.join("fixtures/storage");
    fs::create_dir_all(&storage).unwrap();
    fs::copy(repo_fixtures().join("schema-18.anki2"), storage.join("schema-18.anki2")).unwrap();
    let path = dir.join("scenarios").join(format!("{id}.yaml"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!("id: {id}\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{body}"),
    )
    .unwrap();
    scenario::load(&path).unwrap_or_else(|e| panic!("{e}"))
}

/// Notes 101 and 102 with cards 201 (note 101) and 203 (note 103, which is
/// gone): note 102 has no cards (Q6).
const ROWS: &str = r#"steps:
  - open: {fixture: storage/schema-18.anki2}
  - db_exec: {sql: "DELETE FROM cards"}
  - db_exec: {sql: "DELETE FROM notes"}
  - db_exec: {sql: "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data) VALUES (102, 'b', 1, 0, 0, '', 'cat', 'cat', 0, 0, ''), (101, 'a', 1, 0, 0, '', 'dog', 'dog', 0, 0, '')"}
  - db_exec: {sql: "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) VALUES (203, 103, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, ''), (201, 101, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, '')"}
"#;

fn run(body: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(dir.path(), "search/adapter", &format!("{ROWS}{body}"));
    MeguriExecutor::new().run(&loaded).unwrap_or_else(|e| panic!("{e}"))
}

// S1, Q2, Q6: rows no logical id names come back as real ids, ascending;
// the whole collection keeps cards without a note and notes without cards.
#[test]
fn the_whole_collection_returns_every_row_of_the_mode() {
    let out = run(concat!(
        "  - observe:\n",
        "      - search:\n",
        "          - {query: ''}\n",
        "          - {query: \"\\t\\u3000\", kind: notes}\n",
        "          - {query: '(  )', kind: cards}\n",
    ));
    assert_eq!(
        out["steps"][5]["search"],
        json!([{"ids": [201, 203]}, {"ids": [101, 102]}, {"error": "empty_group"}])
    );
}

// S1, S3, errors.md E2, E3: parse errors with their texts, backticks turned
// to apostrophes; run errors without texts.
#[test]
fn errors_come_back_as_outputs() {
    let out = run(concat!(
        "  - observe:\n",
        "      - search:\n",
        "          - {query: 'is:a`b'}\n",
        "          - {query: 'prop:ivl>x', kind: notes}\n",
        "          - {query: 'NID:1,a'}\n",
        "          - {query: '(a'}\n",
        "          - {query: '-and'}\n",
        "          - {query: 'a -OR b', kind: notes}\n",
        "          - {query: \"nid:\\u0661\"}\n",
        "          - {query: ''}\n",
    ));
    assert_eq!(
        out["steps"][5]["search"],
        json!([
            {"error": "invalid_state", "provided": "a'b"},
            {"error": "invalid_positive_whole_number", "provided": "x", "context": "ivl>x"},
            {"error": "invalid_id_list", "context": "NID:"},
            {"error": "unclosed_group"},
            {"error": "search_failed"},
            {"error": "search_failed"},
            {"error": "search_failed"},
            {"ids": [201, 203]},
        ])
    );
}

// S2: canonical texts and parse errors, with or without a collection.
#[test]
fn canonical_search_writes_canonical_texts() {
    let expected = json!([
        {"text": "dog cat OR fish"},
        {"text": "deck:*"},
        {"text": "- "},
        {"error": "unknown_escape", "provided": "\\'"},
        {"error": "misplaced_or"},
    ]);
    let observe = "  - observe:\n      - canonical_search: ['dog cat or  fish', '', '-and', 'a\\`b', 'a or']\n";
    let out = run(observe);
    assert_eq!(out["steps"][5]["canonical_search"], expected);

    let dir = tempfile::tempdir().unwrap();
    let loaded = load(dir.path(), "search/no-collection", &format!("steps:\n{observe}"));
    let out = MeguriExecutor::new().run(&loaded).unwrap();
    assert_eq!(out["steps"][0]["canonical_search"], expected);
}

// A term whose matching unit has not landed is a run error, never a guess.
#[test]
fn terms_of_later_units_are_run_errors() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{ROWS}  - observe:\n      - search: [{{query: 'dog'}}]\n");
    let loaded = load(dir.path(), "search/stub", &body);
    let error = MeguriExecutor::new().run(&loaded).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("unsupported_observation", Some(5)));
}

// S1: `search` needs the scenario's collection. The run starts from the
// new collection (dsl.md D8); an executor without one has none open.
#[test]
fn search_without_a_collection_is_an_invalid_scenario() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "search/nothing-open",
        "steps:\n  - observe:\n      - search: [{query: ''}]\n",
    );
    let areas: Vec<&str> = MeguriExecutor::AREAS
        .into_iter()
        .filter(|area| *area != "notetypes")
        .collect();
    let without_new_collection = MeguriExecutor::with(MeguriExecutor::area_registry(&areas));
    let error = without_new_collection.run(&loaded).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("invalid_scenario", Some(0)));
}
