//! H3 held-build lifetime and L2's deliberately local legacy save.
use meguri_storage::ReadRows;
use meguri_testkit::{
    exec::{ExecError, Executor, MeguriExecutor, Session},
    scenario::{self, ObservationRequest},
};
use serde_json::{Value, json};

fn run(body: &str, executor: &MeguriExecutor) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.yaml");
    std::fs::write(
        &path,
        format!("id: answer/adapter-probe\nclock: {{tz: UTC, start: '2030-01-15T10:00:00'}}\n{body}"),
    )
    .unwrap();
    executor.run(&scenario::load(&path).unwrap()).unwrap()
}

fn witness(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let collection = session.engine_collection()?;
    Ok(match session.answer_cache.as_ref() {
        Some(cache) => {
            json!({"build": cache.identity().0, "enabled": cache.enabled(), "current": cache.is_current(&collection)})
        }
        None => Value::Null,
    })
}

#[test]
fn disabled_held_build_keeps_switch_changes_and_noops_until_a_real_mutation() {
    // Replace only the observation seam; every step and queue read is real.
    let areas = MeguriExecutor::AREAS
        .into_iter()
        .filter(|a| *a != "undo")
        .collect::<Vec<_>>();
    let mut registry = MeguriExecutor::area_registry(&areas);
    registry.observation("undo_status", witness);
    let executor = MeguriExecutor::with(registry);
    for mutation in [
        "  - legacy_update: {preset: Default, patch: {new: {perDay: 21}}}\n",
        "  - legacy_update: {deck: Inbox, patch: {desc: 'H3 deck witness'}}\n",
        "  - add_note: {notetype: Basic, deck: Inbox, fields: ['card mutation witness', 'real creation']}\n",
    ] {
        let out = run(
            &format!(
                r#"setup:
  config: {{fsrs: true, loadBalancerEnabled: false}}
  decks: [Inbox]
  notes: [{{notetype: Basic, deck: Inbox, fields: ['held probe', 'reverse probe']}}]
steps:
  - observe: [queue, undo_status]
  - set_config: {{key: loadBalancerEnabled, value: true}}
  - observe: [queue, undo_status]
  - set_flag: {{cards: ['#c1'], flag: 0}}
  - legacy_update: {{deck: Inbox, patch: {{}}}}
  - observe: [queue, undo_status]
{mutation}  - observe: [queue, undo_status]
"#
            ),
            &executor,
        );
        let steps = &out["steps"];
        let first = &steps[0]["undo_status"];
        assert_eq!(first["enabled"], false);
        assert_eq!(first["current"], true);
        assert_eq!(steps[2]["undo_status"], *first, "switch alone keeps the off build");
        assert_eq!(steps[5]["undo_status"], *first, "equal writes keep the off build");
        assert_eq!(steps[7]["undo_status"]["enabled"], true, "{mutation}");
        assert_eq!(steps[7]["undo_status"]["current"], true);
        assert_ne!(steps[7]["undo_status"]["build"], first["build"]);
    }
}

fn preset_stamp(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let rows = session
        .storage
        .collection()
        .ok_or_else(|| ExecError::new("test", "missing collection"))?
        .reader()?;
    let p = rows
        .deck_config(meguri_model::ids::DeckConfigId(1))
        .map_err(|e| ExecError::new("test", e.to_string()))?
        .unwrap();
    Ok(json!({
        "mtime": p.mtime.0,
        "usn": p.usn.0,
        "bytes": p.config.to_bytes().as_ref(),
        "max_interval": p.config.maximum_review_interval,
        "answer_cap": p.config.cap_answer_time_to_secs,
        "unknown": p.config.unknown_fields().iter().map(|field| field.bytes()).collect::<Vec<_>>(),
    }))
}

#[test]
fn empty_and_equal_legacy_preset_saves_stamp_locally_and_keep_raw_blob() {
    // Repeated false show_timer with an overlong zero leaves its value intact;
    // field 500 is an opaque unknown with an overlong value varint.
    let areas = MeguriExecutor::AREAS
        .into_iter()
        .filter(|a| *a != "undo")
        .collect::<Vec<_>>();
    let mut registry = MeguriExecutor::area_registry(&areas);
    registry.observation("undo_status", preset_stamp);
    let executor = MeguriExecutor::with(registry);
    for hosted in [false, true] {
        let promotion = if hosted { "  - observe: [queue]\n" } else { "" };
        let out = run(
            &format!(
                r#"steps:
  - db_exec: {{sql: "UPDATE deck_config SET config=CAST(config || X'C8018000A01F8700' AS BLOB), mtime_secs=1, usn=14 WHERE id=1"}}
{promotion}  - observe: [undo_status]
  - advance: 3s
  - legacy_update: {{preset: Default, patch: {{}}}}
  - observe: [undo_status]
  - advance: 2s
  - legacy_update: {{preset: Default, patch: {{new: {{perDay: 20}}}}}}
  - observe: [undo_status]
"#
            ),
            &executor,
        );
        let offset = usize::from(hosted);
        let steps = &out["steps"];
        let before = &steps[1 + offset]["undo_status"];
        let empty = &steps[4 + offset]["undo_status"];
        let equal = &steps[7 + offset]["undo_status"];
        assert_eq!(before["mtime"], 1);
        assert_eq!(before["usn"], 14);
        assert_eq!(empty["mtime"], 1_894_701_603_i64);
        assert_eq!(empty["usn"], -1);
        assert_eq!(equal["mtime"], 1_894_701_605_i64);
        assert_eq!(equal["usn"], -1);
        assert_eq!(before["max_interval"], 36500);
        assert_eq!(before["answer_cap"], 60);
        assert_eq!(before["unknown"], json!([[0xA0, 0x1F, 0x87, 0x00]]));
        assert_eq!(empty["bytes"], before["bytes"]);
        assert_eq!(equal["bytes"], before["bytes"]);
    }
}

#[test]
fn empty_and_equal_legacy_preset_saves_store_read_bounds_over_invalid_raw_values() {
    let areas = MeguriExecutor::AREAS
        .into_iter()
        .filter(|a| *a != "undo")
        .collect::<Vec<_>>();
    let mut registry = MeguriExecutor::area_registry(&areas);
    registry.observation("undo_status", preset_stamp);
    let executor = MeguriExecutor::with(registry);
    for hosted in [false, true] {
        for patch in ["{}", "{rev: {maxIvl: 36500}, maxTaken: 60}"] {
            let promotion = if hosted { "  - observe: [queue]\n" } else { "" };
            let out = run(
                &format!(
                    r#"steps:
  - db_exec: {{sql: "UPDATE deck_config SET config=CAST(config || X'800100C00100A01F8700' AS BLOB), mtime_secs=1, usn=14 WHERE id=1"}}
{promotion}  - observe: [undo_status]
  - advance: 3s
  - legacy_update: {{preset: Default, patch: {patch}}}
  - observe: [undo_status]
"#
                ),
                &executor,
            );
            let offset = usize::from(hosted);
            let before = &out["steps"][1 + offset]["undo_status"];
            let saved = &out["steps"][4 + offset]["undo_status"];
            assert_eq!(before["max_interval"], 0);
            assert_eq!(before["answer_cap"], 0);
            assert_eq!(before["mtime"], 1);
            assert_eq!(before["usn"], 14);
            assert_eq!(saved["max_interval"], 36500, "hosted={hosted}, patch={patch}");
            assert_eq!(saved["answer_cap"], 60, "hosted={hosted}, patch={patch}");
            assert_eq!(saved["mtime"], 1_894_701_603_i64);
            assert_eq!(saved["usn"], -1);
            assert_ne!(saved["bytes"], before["bytes"]);
            assert_eq!(before["unknown"], json!([[0xA0, 0x1F, 0x87, 0x00]]));
            assert_eq!(saved["unknown"], before["unknown"]);
        }
    }
}

#[test]
fn unexpected_filtered_answer_without_declaration_is_a_run_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.yaml");
    std::fs::write(
        &path,
        r#"id: answer/unexpected-filtered
clock: {tz: UTC, start: '2030-01-15T10:00:00'}
setup:
  config: {fsrs: true}
  notes: [{notetype: Basic, fields: ['scope probe', 'normal study']}]
steps:
  - db_exec: {sql: "UPDATE cards SET odid=1"}
  - answer: {card: '#c1', rating: good}
"#,
    )
    .unwrap();
    let loaded = scenario::load(&path).unwrap();
    let executor = MeguriExecutor::new();
    assert!(executor.missing(&loaded.scenario.needs()).is_empty());
    let error = executor.run(&loaded).unwrap_err();
    assert_eq!(error.kind, "core_error");
    assert_eq!(error.step, Some(1));
    assert!(error.detail.contains("InFilteredDeck"));
}

#[test]
fn raw_noop_and_reopen_explicitly_release_the_disabled_build() {
    let areas = MeguriExecutor::AREAS
        .into_iter()
        .filter(|a| *a != "undo")
        .collect::<Vec<_>>();
    let mut registry = MeguriExecutor::area_registry(&areas);
    registry.observation("undo_status", witness);
    let executor = MeguriExecutor::with(registry);
    for reset in [
        "  - db_exec: {sql: 'UPDATE cards SET usn=usn WHERE 0'}\n",
        "  - reopen: {}\n",
    ] {
        let out = run(
            &format!(
                r#"setup:
  config: {{loadBalancerEnabled: false}}
steps:
  - observe: [queue, undo_status]
  - set_config: {{key: loadBalancerEnabled, value: true}}
{reset}  - observe: [undo_status]
  - observe: [queue, undo_status]
"#
            ),
            &executor,
        );
        let steps = &out["steps"];
        assert_eq!(steps[0]["undo_status"]["enabled"], false);
        assert_eq!(steps[3]["undo_status"], Value::Null);
        assert_eq!(steps[4]["undo_status"]["enabled"], true);
        assert_ne!(steps[4]["undo_status"]["build"], steps[0]["undo_status"]["build"]);
    }
}
