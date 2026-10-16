//! The undo scenario `undo/config-writes` (`specs/engine/undo.md` A2, A6,
//! A7; `specs/conformance/undo-ops.md` V1 to V3) played on the engine's undo
//! and compared with the oracle's golden.
//!
//! The runner's undo adapter (`undo-ops.md` V6) waits on the conformance
//! session running ops through the engine (U38's `CoreHost::open`) and on the
//! fresh collection a scenario starts from (U28); the area's other scenarios
//! also need the note ops of U27 and the answers of U44. Until then this test
//! reads the scenario file and plays its steps here, as V1 to V3 say Meguri's
//! adapters do: a copy of the schema-18 fixture with `col.mod` 0 stands in
//! for the fresh collection (the golden's first read shows `col.mod` 0 and
//! none of the scenario's keys), and the maintenance lane runs `db_exec` on
//! the writer connection (`record-ops.md` W1). A scenario step this file
//! does not play fails the test.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::Harness;
use meguri_engine::executor::catalog;
use meguri_engine::undo::names::UPDATE_CONFIG;
use meguri_engine::{ClearCause, Collection, DisabledReason, EngineError, OpSpec, StepKind, UndoScope, UndoStatus};
use meguri_model::changes::OpSource;
use meguri_storage::ConfigValue;
use meguri_storage::raw::SqlValue;
use meguri_testkit::diff;
use meguri_testkit::scenario::{self, ObservationKind, Step};
use serde_json::{Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// W1: one statement on the writer connection, committed at once. One that
/// does not begin with `select` empties both histories.
fn db_exec(c: &Collection, sql: &str) -> Value {
    let statement = sql.to_owned();
    let rows = c
        .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), move |writer| {
            Ok(writer.execute_raw(&statement, &[])?)
        })
        .wait()
        .unwrap();
    if !sql.trim_start().to_ascii_lowercase().starts_with("select") {
        c.clear_undo(ClearCause::ExternalChange).unwrap();
    }
    let rows: Vec<Value> = rows
        .rows
        .iter()
        .map(|row| Value::Array(row.iter().map(sql_value).collect()))
        .collect();
    json!({ "rows": rows })
}

fn sql_value(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(i) => json!(i),
        SqlValue::Text(text) => json!(text),
        other => panic!("undo/config-writes reads no {other:?}"),
    }
}

/// W4, or with `undoable` V3.1: one config key through the config API.
fn set_config(c: &Collection, key: &str, value: &Value, undoable: bool) {
    let key = key.to_owned();
    let value = ConfigValue::parse(value.to_string().as_bytes()).expect("a JSON value");
    let spec = if undoable {
        OpSpec::named("set_config", OpSource::UserUndoable).undo_step(UPDATE_CONFIG, StepKind::Other)
    } else {
        OpSpec::named("set_config", OpSource::UserNonUndoable).skip_undo()
    };
    c.run(spec, move |ctx| {
        ctx.scope().set_config(&key, &value)?;
        Ok(())
    })
    .unwrap();
}

/// V3.2: the key removed through the config API, an "Update Config" step.
fn remove_config(c: &Collection, key: &str) {
    let key = key.to_owned();
    c.run(
        OpSpec::named("remove_config", OpSource::UserUndoable).undo_step(UPDATE_CONFIG, StepKind::Other),
        move |ctx| {
            ctx.scope().remove_config(&key)?;
            Ok(())
        },
    )
    .unwrap();
}

/// V1: undo or redo with scope `Global`. Only a refusal by the step's own
/// check is `changed_externally`; every other refusal is the empty one.
fn restore(c: &Collection, op: &str) -> Value {
    let (status, empty) = match op {
        "undo" => (c.peek_undo(UndoScope::Global), "nothing_to_undo"),
        _ => (c.peek_redo(UndoScope::Global), "nothing_to_redo"),
    };
    if status.unwrap().name().is_none() {
        return json!({ op: { "error": empty } });
    }
    let done = match op {
        "undo" => c.undo(UndoScope::Global).map(drop),
        _ => c.redo(UndoScope::Global).map(drop),
    };
    match done {
        Ok(()) => json!({}),
        Err(EngineError::UndoUnavailable {
            reason: DisabledReason::ChangedExternally,
        }) => json!({ op: { "error": "changed_externally" } }),
        Err(EngineError::UndoUnavailable { .. }) => json!({ op: { "error": empty } }),
        Err(error) => panic!("{op} failed: {error}"),
    }
}

/// V2, V4: the step ids an undo and a redo would apply now. A V4 id is its
/// name's words in lower case joined by `_` ("Update Config":
/// `update_config`).
fn undo_status(c: &Collection) -> Value {
    let id = |status: UndoStatus| match status.name() {
        Some(name) => json!(name.to_lowercase().replace([' ', '/'], "_")),
        None => Value::Null,
    };
    json!({
        "undo": id(c.peek_undo(UndoScope::Global).unwrap()),
        "redo": id(c.peek_redo(UndoScope::Global).unwrap()),
    })
}

#[test]
fn config_writes_undo_and_redo_as_the_oracle_records() {
    let loaded = scenario::load(&conformance().join("scenarios/undo/config-writes.yaml")).unwrap();
    let golden: Value = meguri_testkit::observe::parse_json(
        &std::fs::read_to_string(conformance().join("goldens/undo/config-writes.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        golden["scenario_sha256"].as_str(),
        Some(loaded.sha256.as_str()),
        "the golden is fresh"
    );
    assert!(loaded.scenario.setup.sections().is_empty());
    assert_eq!(golden["clock"]["start"], json!("2030-01-15T10:00:00"));
    assert_eq!(golden["tz"], json!("UTC"));

    let h = Harness::open();
    let c = &h.collection;
    db_exec(c, "UPDATE col SET mod = 0");
    let mut steps = Vec::new();
    for step in &loaded.scenario.steps {
        let out = match step {
            Step::Advance(duration) => {
                h.clock.step_ms(duration.seconds() * 1000);
                json!({})
            }
            Step::DbExec(args) => {
                assert!(args.params.is_none(), "no parameters in this scenario");
                json!({ "db_exec": db_exec(c, &args.sql) })
            }
            Step::SetConfig(args) => {
                set_config(c, &args.key, &args.value, args.undoable);
                json!({})
            }
            Step::RemoveConfig(args) => {
                remove_config(c, &args.key);
                json!({})
            }
            Step::Undo(_) => restore(c, "undo"),
            Step::Redo(_) => restore(c, "redo"),
            Step::Observe(requests) => {
                let [request] = requests.as_slice() else {
                    panic!("one observation per step here")
                };
                assert_eq!(request.kind, ObservationKind::UndoStatus);
                json!({ "undo_status": undo_status(c) })
            }
            other => panic!("this test does not play {}", other.name()),
        };
        steps.push(out);
    }
    let differences = diff::exact_differences(&golden["observations"], &json!({ "steps": steps }));
    assert!(
        differences.is_empty(),
        "differences from the golden: {:#?}",
        differences
            .iter()
            .map(|d| (&d.pointer, &d.expected, &d.actual))
            .collect::<Vec<_>>()
    );
}
