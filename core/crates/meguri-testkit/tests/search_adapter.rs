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

// Q5, Q6 with U81's plain text: in cards mode a note term skips the card
// whose note is gone; in notes mode it returns a note without cards.
#[test]
fn text_terms_read_the_note_rows() {
    let out = run(concat!(
        "  - observe:\n",
        "      - search:\n",
        "          - {query: dog}\n",
        "          - {query: dog, kind: notes}\n",
        "          - {query: cat, kind: notes}\n",
        "          - {query: cat}\n",
    ));
    assert_eq!(
        out["steps"][5]["search"],
        json!([{"ids": [201]}, {"ids": [101]}, {"ids": [102]}, {"ids": []}])
    );
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
        .filter(|area| *area != "storage")
        .collect();
    let without_new_collection = MeguriExecutor::with(MeguriExecutor::area_registry(&areas));
    let error = without_new_collection.run(&loaded).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("invalid_scenario", Some(0)));
}

// state-and-flags S1, S3: real day context, scenario time and configured
// learn-ahead. Only one note exists among the matching cards.
#[test]
fn due_search_uses_requested_day_clock_scenario_now_and_configured_learn_ahead() {
    let out = run(r#"  - db_exec: {sql: "UPDATE col SET crt=1894680000"}
  - set_config: {key: rollover, value: 4}
  - set_config: {key: collapseTime, value: 60}
  - db_exec: {sql: "UPDATE cards SET queue=1, due=1894701660 WHERE id=201"}
  - db_exec: {sql: "UPDATE cards SET queue=2, due=0 WHERE id=203"}
  - db_exec: {sql: "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data) VALUES (202,101,1,1,0,0,0,4,1894701661,0,0,0,0,0,0,0,0,'')"}
  - observe:
      - search: [{query: 'is:due'}, {query: 'is:due', kind: notes}]
  - set_config: {key: collapseTime, value: 0}
  - observe:
      - search: [{query: 'is:due'}]
  - set_clock: '2030-01-15T10:01:01'
  - observe:
      - search: [{query: 'is:due'}]
  - db_exec: {sql: "UPDATE cards SET queue=2, due=1 WHERE id=201"}
  - observe:
      - search: [{query: 'is:due'}]
  - set_clock: '2030-01-16T04:00:00'
  - observe:
      - search: [{query: 'is:due'}]
"#);
    let searches = out["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|step| step.get("search"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        searches,
        vec![
            json!([{"ids": [201, 203]}, {"ids": [101]}]),
            json!([{"ids": [203]}]),
            json!([{"ids": [201, 202, 203]}]),
            json!([{"ids": [202, 203]}]),
            json!([{"ids": [201, 202, 203]}]),
        ]
    );
}

// S2-S3: a buried-only plan must not even request day maintenance. These
// old buried rows would be unburied by a compatibility day query.
#[test]
fn buried_search_reads_no_day_and_preserves_cards_config_and_collection() {
    let out = run(
        r#"  - db_exec: {sql: "UPDATE cards SET queue=CASE id WHEN 201 THEN -2 ELSE -3 END"}
  - db_exec: {sql: "DELETE FROM config WHERE key='lastUnburied'"}
  - observe: [{db_dump: {tables: [cards, config, col]}}]
  - observe:
      - search: [{query: 'is:buried'}, {query: '-is:buried'}, {query: 'is:buried', kind: notes}]
  - observe: [{db_dump: {tables: [cards, config, col]}}]
"#,
    );
    let steps = out["steps"].as_array().unwrap();
    let dumps = steps.iter().filter_map(|step| step.get("db_dump")).collect::<Vec<_>>();
    assert_eq!(dumps.len(), 2);
    assert_eq!(dumps[0], dumps[1]);
    let searches = steps.iter().find_map(|step| step.get("search")).unwrap();
    assert_eq!(searches, &json!([{"ids": [201, 203]}, {"ids": []}, {"ids": [101]}]));
}
