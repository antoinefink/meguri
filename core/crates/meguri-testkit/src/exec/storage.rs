//! The storage area's adapters (U22, U23): `open` (storage-ops.md P1),
//! `db_exec` and `set_config` (record-ops.md W1, W4), the observations
//! `schema_objects` (P3) and `db_dump` (observations.md O8), the handoff
//! adapters that make Meguri a side of live chains (dsl.md D9.2), and the
//! schema guard the runner asserts around every Meguri operation
//! (`storage/schema-18` S6.2).
//!
//! A session's collection lives in a temporary session directory as
//! `collection.anki2`, opened through meguri-storage's open gate. While the
//! engine and `CoreHost::open` do not exist (U24, U38), the gate is the
//! collection open.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{ConfigValue, OpenError, OpenOptions, OpenedCollection, StampMode, open_collection};
use serde_json::{Map, Number, Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{DUMP_TABLES, LoadedScenario, ObservationArgs, ObservationRequest, SqlParam, Step};

pub(super) fn register(registry: &mut Registry) {
    registry.op_rejecting("open", &["file_too_old", "file_too_new", "v1_scheduler"], open);
    registry.op("db_exec", db_exec);
    registry.op("set_config", set_config);
    registry.observation("schema_objects", schema_objects);
    registry.observation("db_dump", db_dump);
    registry.handoff(hand_out, hand_in);
    registry.finish(finish);
}

const COLLECTION: &str = "collection.anki2";

/// A session's collection files and the open collection.
#[derive(Default)]
pub struct Storage {
    // Fields drop in order: the collection closes before its directory goes.
    collection: Option<OpenedCollection>,
    /// `sqlite_master` after the last open or `db_exec`, which every later
    /// Meguri operation must leave as it is.
    baseline: Objects,
    dir: Option<TempDir>,
}

impl Storage {
    pub fn is_open(&self) -> bool {
        self.collection.is_some()
    }

    /// The open collection, for the adapters of later areas.
    pub fn collection(&self) -> Option<&OpenedCollection> {
        self.collection.as_ref()
    }

    fn dir(&mut self) -> Result<PathBuf, ExecError> {
        if self.dir.is_none() {
            let dir = tempfile::Builder::new()
                .prefix("meguri-session-")
                .tempdir()
                .map_err(|e| ExecError::new("internal_error", format!("creating a session directory: {e}")))?;
            self.dir = Some(dir);
        }
        Ok(self.dir.as_ref().map(|d| d.path().to_path_buf()).unwrap_or_default())
    }

    /// Closes the collection as at a handoff (`storage/open-and-connections`
    /// C10).
    fn close(&mut self) -> Result<(), ExecError> {
        if let Some(collection) = self.collection.take() {
            collection
                .close()
                .map_err(|e| ExecError::new("core_error", format!("closing the collection: {e}")))?;
        }
        Ok(())
    }

    fn opened(&mut self, collection: OpenedCollection) -> Result<(), ExecError> {
        self.baseline = object_set(&collection)?;
        self.collection = Some(collection);
        Ok(())
    }

    /// S6.2: the objects are the ones after the last open or `db_exec`, each
    /// with the definition it had then: a column added to a table or an index
    /// defined again under its name adds to the collection as much as a new
    /// object (R2016).
    fn guard(&self) -> Result<(), ExecError> {
        let Some(collection) = &self.collection else {
            return Ok(());
        };
        let now = object_set(collection)?;
        if now == self.baseline {
            return Ok(());
        }
        let added: Vec<_> = now.keys().filter(|k| !self.baseline.contains_key(*k)).collect();
        let removed: Vec<_> = self.baseline.keys().filter(|k| !now.contains_key(*k)).collect();
        let redefined: Vec<_> = now
            .iter()
            .filter_map(|(k, v)| self.baseline.get(k).filter(|was| *was != v).map(|_| k))
            .collect();
        Err(ExecError::new(
            "schema_guard",
            format!(
                "sqlite_master changed (storage/schema-18 S6.2): added {added:?}, removed {removed:?}, redefined {redefined:?}"
            ),
        ))
    }
}

impl fmt::Debug for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Storage")
            .field("open", &self.collection.is_some())
            .field("dir", &self.dir.as_ref().map(TempDir::path))
            .finish()
    }
}

/// `sqlite_master`: `(type, name)` to `(tbl_name, sql)` as stored.
type Objects = BTreeMap<(String, String), (String, Option<String>)>;

fn object_set(collection: &OpenedCollection) -> Result<Objects, ExecError> {
    Ok(collection
        .writer()
        .schema_objects()
        .map_err(core)?
        .into_iter()
        .map(|o| ((o.kind, o.name), (o.tbl_name, o.sql)))
        .collect())
}

fn core(error: impl fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn collection<'s>(session: &'s Session<'_>) -> Result<&'s OpenedCollection, ExecError> {
    session
        .storage
        .collection
        .as_ref()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))
}

/// `conformance/fixtures/` of the checkout the scenario file is in: the
/// file is `<conformance>/scenarios/<id>.<ext>`.
fn fixtures_dir(loaded: &LoadedScenario) -> Option<PathBuf> {
    let mut dir = loaded.path.parent()?;
    for _ in 1..loaded.scenario.id.split('/').count() {
        dir = dir.parent()?;
    }
    Some(dir.parent()?.join("fixtures"))
}

/// P1: closes any open collection, replaces the session's collection files
/// with a copy of the fixture and opens it through the gate.
fn open(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Open(spec) = step else {
        return Err(ExecError::new("internal_error", "open adapter called for another op"));
    };
    let fixture = fixtures_dir(session.scenario)
        .map(|dir| dir.join(spec.fixture.as_str()))
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            invalid(format!(
                "no fixture file conformance/fixtures/{}",
                spec.fixture.as_str()
            ))
        })?;
    session.storage.close()?;
    let dir = session.storage.dir()?;
    let path = dir.join(COLLECTION);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let file = dir.join(format!("{COLLECTION}{suffix}"));
        if file.exists() {
            fs::remove_file(&file).map_err(|e| ExecError::new("internal_error", format!("{}: {e}", file.display())))?;
        }
    }
    fs::copy(&fixture, &path).map_err(|e| ExecError::new("fixture_error", format!("{}: {e}", fixture.display())))?;
    let opened = open_collection(&path, &OpenOptions::new(&*session.clock, &*session.random));
    match opened {
        Ok(collection) => {
            session.storage.opened(collection)?;
            Ok(OpResult::Done)
        }
        Err(OpenError::TooOld { .. }) => Ok(OpResult::Rejected("file_too_old")),
        Err(OpenError::TooNew { .. }) => Ok(OpResult::Rejected("file_too_new")),
        Err(OpenError::V1Scheduler) => Ok(OpResult::Rejected("v1_scheduler")),
        Err(other) => Err(core(other)),
    }
}

/// W1: one statement on the collection's writer, committed at once.
fn db_exec(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::DbExec(args) = step else {
        return Err(ExecError::new(
            "internal_error",
            "db_exec adapter called for another op",
        ));
    };
    let params: Vec<SqlValue> = args
        .params
        .iter()
        .flatten()
        .map(|param| match param {
            SqlParam::Text(text) => SqlValue::Text(text.clone()),
            SqlParam::Integer(value) => SqlValue::Integer(*value),
            SqlParam::Real(value) => SqlValue::Real(*value),
            SqlParam::Null => SqlValue::Null,
        })
        .collect();
    // A statement may change the schema; Meguri's operations before it may
    // not.
    session.storage.guard()?;
    let rows = collection(session)?
        .writer()
        .execute_raw(&args.sql, &params)
        .map_err(core)?;
    let baseline = object_set(collection(session)?)?;
    session.storage.baseline = baseline;
    // W1's reset after a write has nothing to clear: the session runs no
    // engine yet, so it keeps no cached view and no undo history (U38).
    Ok(OpResult::Output(json!({ "rows": rows_value(&rows) })))
}

/// A scenario's JSON value as a config value (W4): a number with a fraction
/// or an exponent is a float, one without an integer.
fn config_value(value: &Value) -> Result<ConfigValue, ExecError> {
    Ok(match value {
        Value::Null => ConfigValue::Null,
        Value::Bool(b) => ConfigValue::Bool(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => ConfigValue::from(i),
            (None, Some(u), _) => ConfigValue::from(u),
            (None, None, Some(f)) => {
                ConfigValue::float(f).ok_or_else(|| invalid(format!("set_config value {n} is not finite")))?
            }
            _ => return Err(invalid(format!("set_config value {n}"))),
        },
        Value::String(text) => ConfigValue::String(text.clone()),
        Value::Array(items) => ConfigValue::Array(items.iter().map(config_value).collect::<Result<_, _>>()?),
        Value::Object(members) => ConfigValue::Object(
            members
                .iter()
                .map(|(name, member)| Ok((name.clone(), config_value(member)?)))
                .collect::<Result<_, ExecError>>()?,
        ),
    })
}

/// W4: one config key through the config API (`storage/config-table` C4),
/// as an operation of its own that keeps no undo entry: a local change,
/// written only when the value differs. With `undoable` (undo-ops.md V3.1)
/// the write is an "Update Config" undo step, which needs the engine's
/// history: refused until the session runs ops through the engine (U38's
/// `CoreHost::open`), so no scenario passes with the step left out.
fn set_config(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::SetConfig(args) = step else {
        return Err(ExecError::new(
            "internal_error",
            "set_config adapter called for another op",
        ));
    };
    if args.undoable {
        return Err(ExecError::new(
            "unsupported_op",
            "set_config with undoable: true records an undo step; the session has no engine yet (U38)",
        ));
    }
    let value = config_value(&args.value)?;
    let clock = session.clock.clone();
    let collection = collection(session)?;
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, &*clock, None)
        .map_err(core)?;
    scope.set_config(&args.key, &value).map_err(core)?;
    scope.commit().map_err(core)?;
    Ok(OpResult::Done)
}

/// A stored value by its storage class (P2.2, O8).
fn sql_value(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(i) => Value::from(*i),
        SqlValue::Real(f) => Number::from_f64(*f).map_or(Value::Null, Value::Number),
        SqlValue::Text(s) => Value::String(s.clone()),
        SqlValue::Blob(bytes) => digest_value(bytes),
    }
}

fn digest_value(bytes: &[u8]) -> Value {
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    json!({ "sha256": hex })
}

fn rows_value(rows: &RawRows) -> Value {
    Value::Array(
        rows.rows
            .iter()
            .map(|row| Value::Array(row.iter().map(sql_value).collect()))
            .collect(),
    )
}

/// P3.2: `--` comments removed to the end of their line, every run of
/// spaces, tabs, carriage returns and line feeds made one space, trimmed.
pub fn normalize_sql(sql: &str) -> String {
    let mut without_comments = String::with_capacity(sql.len());
    for (i, line) in sql.split('\n').enumerate() {
        if i > 0 {
            without_comments.push('\n');
        }
        without_comments.push_str(line.find("--").map_or(line, |at| &line[..at]));
    }
    let mut out = String::with_capacity(without_comments.len());
    let mut space = false;
    for c in without_comments.chars() {
        if matches!(c, ' ' | '\t' | '\r' | '\n') {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// P3: every object of the main database's `sqlite_master`, sorted by type,
/// then name.
fn schema_objects(session: &mut Session<'_>, _request: &ObservationRequest) -> Result<Value, ExecError> {
    let mut objects = collection(session)?.writer().schema_objects().map_err(core)?;
    objects.sort_by(|a, b| (a.kind.as_str(), a.name.as_str()).cmp(&(b.kind.as_str(), b.name.as_str())));
    Ok(Value::Array(
        objects
            .into_iter()
            .map(|o| {
                json!({
                    "type": o.kind,
                    "name": o.name,
                    "tbl_name": o.tbl_name,
                    "sql": o.sql.as_deref().map(normalize_sql),
                })
            })
            .collect(),
    ))
}

/// O8's key columns of each table.
fn key_columns(table: &str) -> &'static [&'static str] {
    match table {
        "graves" => &["oid", "type"],
        "fields" | "templates" => &["ntid", "ord"],
        "tags" => &["tag"],
        "config" => &["KEY"],
        _ => &["id"],
    }
}

/// The digest of a blob without every top-level protobuf field `number`
/// (O8), or of the stored bytes when it does not parse as protobuf fields.
fn digest_without_field(bytes: &[u8], number: u64) -> Value {
    match strip_field(bytes, number) {
        Some(kept) => digest_value(&kept),
        None => digest_value(bytes),
    }
}

fn varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// The blob as a sequence of fields (varint key with a field number of at
/// least 1 and wire type 0, 1, 2 or 5) ending exactly at its end, without
/// the fields numbered `number`; `None` when it does not parse.
fn strip_field(bytes: &[u8], number: u64) -> Option<Vec<u8>> {
    let mut kept = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let key = varint(bytes, &mut at)?;
        if key >> 3 == 0 {
            return None;
        }
        match key & 7 {
            0 => {
                varint(bytes, &mut at)?;
            }
            1 => at = at.checked_add(8)?,
            2 => {
                let len = usize::try_from(varint(bytes, &mut at)?).ok()?;
                at = at.checked_add(len)?;
            }
            5 => at = at.checked_add(4)?,
            _ => return None,
        }
        if at > bytes.len() {
            return None;
        }
        if key >> 3 != number {
            kept.extend_from_slice(&bytes[start..at]);
        }
    }
    Some(kept)
}

/// The text of a key column's value: integers in decimal.
fn key_text(value: &SqlValue) -> String {
    match value {
        SqlValue::Integer(i) => i.to_string(),
        SqlValue::Text(s) => s.clone(),
        SqlValue::Real(f) => f.to_string(),
        SqlValue::Null => "null".to_owned(),
        SqlValue::Blob(bytes) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn dump_table(collection: &OpenedCollection, table: &str) -> Result<Value, ExecError> {
    let rows = collection
        .writer()
        .execute_raw(&format!("SELECT * FROM {table}"), &[])
        .map_err(core)?;
    // A key column is found in any letter case: releases at schemas 14 to
    // 16 created config's as `key`.
    let keys: Vec<usize> = key_columns(table)
        .iter()
        .map(|key| {
            rows.columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(key))
                .ok_or_else(|| core(format!("table {table} has no column {key}")))
        })
        .collect::<Result<_, _>>()?;
    let mut out = Map::new();
    for row in &rows.rows {
        let key = keys.iter().map(|&i| key_text(&row[i])).collect::<Vec<_>>().join(",");
        let mut object = Map::new();
        for (column, value) in rows.columns.iter().zip(row) {
            let value = match (table, column.to_ascii_lowercase().as_str(), value) {
                ("notes", "guid", _) => Value::String("#random".to_owned()),
                ("fields", "config", SqlValue::Blob(bytes)) => digest_without_field(bytes, 9),
                ("templates", "config", SqlValue::Blob(bytes)) => digest_without_field(bytes, 8),
                _ => sql_value(value),
            };
            object.insert(column.clone(), value);
        }
        out.insert(key, Value::Object(object));
    }
    Ok(Value::Object(out))
}

/// O8: the stored rows of the twelve tables, or of the listed ones.
fn db_dump(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let collection = collection(session)?;
    let tables: Vec<&str> = match &request.args {
        ObservationArgs::Mapping(map) => match map.get("tables") {
            Some(Value::Array(tables)) => tables.iter().filter_map(Value::as_str).collect(),
            _ => DUMP_TABLES.to_vec(),
        },
        _ => DUMP_TABLES.to_vec(),
    };
    let mut out = Map::new();
    for table in tables {
        if !DUMP_TABLES.contains(&table) {
            return Err(invalid(format!("db_dump has no table {table}")));
        }
        out.insert(table.to_owned(), dump_table(collection, table)?);
    }
    Ok(Value::Object(out))
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// D9.2, handing over: the guard, the collection closed (C10), and the
/// session directory copied into `dir`.
fn hand_out(session: &mut Session<'_>, dir: &Path) -> Result<(), ExecError> {
    session.storage.guard()?;
    session.storage.close()?;
    let from = session.storage.dir()?;
    copy_tree(&from, dir).map_err(|e| ExecError::new("handoff_error", format!("{}: {e}", dir.display())))
}

/// D9.2, taking over: the handed directory copied into a new session
/// directory and its collection opened through the gate.
fn hand_in(session: &mut Session<'_>, dir: &Path) -> Result<(), ExecError> {
    let to = session.storage.dir()?;
    copy_tree(dir, &to).map_err(|e| ExecError::new("handoff_error", format!("{}: {e}", dir.display())))?;
    let collection = open_collection(
        &to.join(COLLECTION),
        &OpenOptions::new(&*session.clock, &*session.random),
    )
    .map_err(|e| ExecError::new("handoff_error", format!("opening the handed-over collection: {e}")))?;
    session.storage.opened(collection)
}

/// The end of a run on Meguri: the guard over the last operations.
fn finish(session: &mut Session<'_>) -> Result<(), ExecError> {
    session.storage.guard()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_text_normalizes_as_p3_2_says() {
        assert_eq!(
            normalize_sql("CREATE TABLE notes (\n  id integer PRIMARY KEY, -- the id\n\tsfld integer   NOT NULL\r\n)"),
            "CREATE TABLE notes ( id integer PRIMARY KEY, sfld integer NOT NULL )"
        );
        assert_eq!(normalize_sql("  x  "), "x");
    }

    #[test]
    fn a_field_is_stripped_only_from_blobs_that_parse() {
        // Field 1 varint, field 9 length-delimited, field 2 fixed32.
        let blob = [0x08, 0x01, 0x4a, 0x02, 0xaa, 0xbb, 0x15, 1, 2, 3, 4];
        assert_eq!(strip_field(&blob, 9).unwrap(), [0x08, 0x01, 0x15, 1, 2, 3, 4]);
        assert_eq!(strip_field(&blob, 7).unwrap(), blob);
        // Truncated payload, field number 0, wire type 3.
        assert_eq!(strip_field(&[0x4a, 0x05, 0x01], 9), None);
        assert_eq!(strip_field(&[0x00, 0x01], 9), None);
        assert_eq!(strip_field(&[0x0b], 9), None);
    }
}
