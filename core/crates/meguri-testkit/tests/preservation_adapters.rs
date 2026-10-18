//! Preservation contracts N4–N5: raw bytes, logical IDs and strict arguments.
use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor, OpResult, Session};
use meguri_testkit::scenario::{self, LoadedScenario, Scenario, Step};
use serde_json::json;

fn load(dir: &Path, steps: &str) -> LoadedScenario {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    fs::create_dir_all(dir.join("fixtures/preservation")).unwrap();
    fs::copy(
        root.join("fixtures/preservation/kitchen-sink.anki2"),
        dir.join("fixtures/preservation/kitchen-sink.anki2"),
    )
    .unwrap();
    fs::create_dir_all(dir.join("scenarios/preservation")).unwrap();
    let path = dir.join("scenarios/preservation/test.yaml");
    fs::write(&path, format!("id: preservation/test\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps:\n  - open: {{fixture: preservation/kitchen-sink.anki2}}\n{steps}")).unwrap();
    scenario::load(&path).unwrap()
}

fn logical_ids(s: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    Ok(OpResult::Output(json!({"notes": s.notes, "cards": s.cards})))
}

#[test]
fn adopt_numbers_sorted_unbound_rows_once_without_changing_stored_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        concat!(
            "  - observe: [{db_dump: {raw: true}}]\n",
            "  - adopt: {}\n  - check_db: {}\n  - adopt: {}\n",
            "  - observe: [{db_dump: {raw: true}}]\n",
            "  - db_exec: {sql: \"INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data) SELECT 1, 'new-guid', mid, mod, usn, tags, flds, sfld, csum, flags, data FROM notes LIMIT 1\"}\n",
            "  - adopt: {}\n  - check_db: {}\n"
        ),
    );
    let mut registry = MeguriExecutor::core_registry();
    registry.op("check_db", logical_ids);
    let out = MeguriExecutor::with(registry).run(&loaded).unwrap();
    let before = &out["steps"][1]["db_dump"];
    assert_eq!(before, &out["steps"][5]["db_dump"]);
    let ids = &out["steps"][3]["check_db"];
    for table in ["notes", "cards"] {
        let mut expected: Vec<i64> = before[table]
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.parse().unwrap())
            .collect();
        expected.sort();
        assert_eq!(ids[table], json!(expected));
        assert_eq!(out["steps"][2]["adopt"][table], expected.len());
    }
    assert_eq!(out["steps"][4], json!({"adopt": {"notes": 0, "cards": 0}}));
    assert_eq!(out["steps"][7], json!({"adopt": {"notes": 1, "cards": 0}}));
    let mut expected = ids["notes"].as_array().unwrap().clone();
    expected.push(json!(1));
    assert_eq!(out["steps"][8]["check_db"]["notes"], json!(expected));
}

#[test]
fn raw_dump_keeps_guid_and_random_field_and_template_blob_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "  - observe: [{db_dump: {raw: true}}]\n  - observe: [db_dump]\n",
    );
    let out = MeguriExecutor::new().run(&loaded).unwrap();
    let raw = &out["steps"][1]["db_dump"];
    let plain = &out["steps"][2]["db_dump"];
    for row in raw["notes"].as_object().unwrap().values() {
        assert_ne!(row["guid"], "#random");
    }
    for row in plain["notes"].as_object().unwrap().values() {
        assert_eq!(row["guid"], "#random");
    }
    for table in ["fields", "templates"] {
        assert!(
            raw[table]
                .as_object()
                .unwrap()
                .iter()
                .any(|(key, row)| row["config"] != plain[table][key]["config"])
        );
    }
    for table in ["col", "cards", "decks", "deck_config", "config", "notetypes"] {
        assert_eq!(raw[table], plain[table]);
    }
}

#[test]
fn preservation_arguments_are_strict_and_adopt_is_forbidden_in_actor_scenarios() {
    let parse = |step: &str, actors: &str| {
        let text = format!(
            "id: preservation/args\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{actors}steps: [{step}]\n"
        );
        Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    for step in [
        "{adopt: {}}",
        "{observe: [{db_dump: {raw: true}}]}",
        "{observe: [{db_dump: {raw: false, tables: [notes]}}]}",
    ] {
        assert!(parse(step, "").is_ok(), "{step}");
    }
    for step in [
        "{adopt: []}",
        "{adopt: {extra: 1}}",
        "{observe: [{db_dump: {raw: null}}]}",
        "{observe: [{db_dump: {raw: 1}}]}",
        "{observe: [{db_dump: {raw: \"true\"}}]}",
        "{observe: [{db_dump: {raw: true, extra: true}}]}",
    ] {
        assert!(parse(step, "").is_err(), "{step}");
    }
    assert!(parse("{adopt: {}}", "actors: [meguri]\n").is_err());
    assert!(parse("{observe: [{db_dump: {raw: true}}]}", "actors: [meguri]\n").is_ok());
}

#[test]
fn blob_decode_isolates_invalid_notetype_children_without_rewriting_anything() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(dir.path(), concat!(
        "  - db_exec: {sql: \"UPDATE fields SET config = X'00' WHERE ntid = (SELECT id FROM notetypes WHERE name = 'Vocab') AND ord = 0\"}\n",
        "  - observe: [{db_dump: {raw: true}}]\n",
        "  - observe: [{blob_decode: {notetypes: [Vocab, Formula], decks: [Cram], presets: [Steady]}}]\n",
        "  - observe: [{db_dump: {raw: true}}]\n"));
    let out = MeguriExecutor::new().run(&loaded).unwrap();
    assert_eq!(out["steps"][3]["blob_decode"], json!({"notetypes": {"Vocab": "invalid_blob", "Formula": "ok"}, "decks": {"Cram": "ok"}, "presets": {"Steady": "ok"}}));
    assert_eq!(out["steps"][2]["db_dump"], out["steps"][4]["db_dump"]);
}

#[test]
fn blob_decode_does_not_misclassify_non_blob_columns_missing_names_or_missing_deck_kind() {
    for (sql, observation, kind) in [
        ("UPDATE deck_config SET config = 'text' WHERE name = 'Steady'", "{blob_decode: {presets: [Steady]}}", "core_error"),
        ("UPDATE decks SET kind = X'' WHERE name = 'Cram'", "{blob_decode: {decks: [Cram]}}", "core_error"),
        ("SELECT 1", "{blob_decode: {notetypes: [Absent]}}", "unknown_ref"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(dir.path(), &format!("  - db_exec: {{sql: \"{sql}\"}}\n  - observe: [{observation}]\n"));
        assert_eq!(MeguriExecutor::new().run(&loaded).unwrap_err().kind, kind);
    }
}

#[test]
fn stored_card_template_cases_wait_for_their_own_capability() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/scenarios");
    let stored = scenario::load(&root.join("preservation/render/every-card.yaml")).unwrap();
    assert!(stored.scenario.needs().observations.contains("template.card"));
    let new_note = scenario::load(&root.join("render/template/blank.yaml")).unwrap();
    assert!(!new_note.scenario.needs().observations.contains("template.card"));
    assert!(!MeguriExecutor::core_registry().capabilities().observations.contains("template.card"));
    let mut registry = MeguriExecutor::core_registry();
    registry.observation_part("template.card");
    assert!(registry.capabilities().observations.contains("template.card"));
}
