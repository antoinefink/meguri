//! The storage area's adapters (U22, U23): `open` (storage-ops.md P1),
//! `db_exec` and `set_config` (record-ops.md W1, W4), `setup.config`
//! (dsl.md D6.1, written as `set_config` writes), the observations
//! `schema_objects` (P3) and `db_dump` (observations.md O8), the handoff
//! adapters that make Meguri a side of live chains (dsl.md D9.2), and the
//! schema guard the runner asserts around every Meguri operation
//! (`storage/schema-18` S6.2).
//!
//! A session's collection lives in a temporary session directory as
//! `collection.anki2`. Protocol storage opens remain raw; engine-owned
//! operations close that writer before promoting to CoreHost (host H4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use meguri_engine::{Collection, ReaderLease};
use meguri_host::{CoreHost, HostConfig};
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{ConfigValue, OpenError, OpenOptions, OpenedCollection, open_collection};
use meguri_storage::{Reader, ReaderRole};
use serde_json::{Map, Number, Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{DUMP_TABLES, LoadedScenario, ObservationArgs, ObservationRequest, Setup, SqlParam, Step};

pub(super) fn register(registry: &mut Registry) {
    registry.new_collection(new_collection);
    registry.op_rejecting("open", &["file_too_old", "file_too_new", "v1_scheduler"], open);
    registry.op("db_exec", db_exec);
    registry.op("adopt", adopt);
    registry.op("set_config", set_config);
    registry.op("remove_config", remove_config);
    registry.setup("config", setup_config);
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
    collection: Option<SessionCollection>,
    media: Option<meguri_storage::media::MediaStore>,
    /// `sqlite_master` after the last open or `db_exec`, which every later
    /// Meguri operation must leave as it is.
    baseline: Objects,
    dir: Option<TempDir>,
    host_data: Option<TempDir>,
    backup: std::sync::Arc<super::BackupWitness>,
    record_changes: bool,
    changes: Vec<std::sync::Arc<meguri_engine::Event>>,
    subscription: Option<meguri_engine::Subscription>,
}

/// A protocol storage open stays raw. Engine-owned operations promote it once,
/// closing that writer before the hosted writer starts (engine/host H4).
pub enum SessionCollection {
    Raw(Box<OpenedCollection>),
    Hosted {
        host: Box<CoreHost>,
        collection: Collection,
    },
}

pub enum SessionReader<'a> {
    Raw(Reader),
    Hosted(ReaderLease<'a>),
}
impl std::ops::Deref for SessionReader<'_> {
    type Target = Reader;
    fn deref(&self) -> &Reader {
        match self {
            Self::Raw(reader) => reader,
            Self::Hosted(reader) => reader,
        }
    }
}

impl SessionCollection {
    pub fn path(&self) -> &Path {
        match self {
            Self::Raw(raw) => raw.path(),
            Self::Hosted { collection, .. } => collection.path(),
        }
    }
    pub fn reader(&self) -> Result<SessionReader<'_>, ExecError> {
        match self {
            Self::Raw(raw) => raw
                .reader_factory()
                .open(ReaderRole::Query)
                .map(SessionReader::Raw)
                .map_err(core),
            Self::Hosted { collection, .. } => collection
                .reader(ReaderRole::Query)
                .map(SessionReader::Hosted)
                .map_err(core),
        }
    }
    pub(super) fn with_snapshot<T>(
        &self,
        read: impl FnOnce(&meguri_storage::ReadSnapshot<'_>) -> Result<T, ExecError>,
    ) -> Result<T, ExecError> {
        match self {
            Self::Raw(raw) => {
                let reader = raw.reader_factory().open(ReaderRole::Query).map_err(core)?;
                let snapshot = reader.begin_snapshot().map_err(core)?;
                read(&snapshot)
            }
            Self::Hosted { collection, .. } => {
                let reader = collection.reader(ReaderRole::Query).map_err(core)?;
                let snapshot = reader.snapshot().map_err(core)?;
                read(&snapshot)
            }
        }
    }
    fn close(self) -> Result<(), ExecError> {
        match self {
            Self::Raw(raw) => raw.close().map(|_| ()).map_err(core),
            Self::Hosted { host, collection } => {
                drop(collection);
                host.close().map_err(core)
            }
        }
    }
}

impl Storage {
    pub(super) fn media(
        &mut self,
        clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    ) -> Result<meguri_storage::media::MediaStore, ExecError> {
        if let Some(SessionCollection::Hosted { host, .. }) = &self.collection {
            return host.media().map(|service| service.store().clone()).map_err(core);
        }
        if self.media.is_none() {
            let path = self
                .collection()
                .ok_or_else(|| invalid("no collection is open"))?
                .path();
            self.media = Some(meguri_storage::media::MediaStore::open(path, clock).map_err(core)?);
        }
        Ok(self.media.as_ref().expect("media store was initialized").clone())
    }
    pub fn is_open(&self) -> bool {
        self.collection.is_some()
    }

    /// The open collection, for the adapters of later areas.
    pub fn collection(&self) -> Option<&SessionCollection> {
        self.collection.as_ref()
    }

    pub(super) fn promote(
        &mut self,
        clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
        random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
    ) -> Result<bool, ExecError> {
        match self.collection.as_ref() {
            Some(SessionCollection::Hosted { .. }) => return Ok(false),
            None => return Err(invalid("no collection is open")),
            Some(SessionCollection::Raw(_)) => {}
        }
        self.close()?;
        if self.host_data.is_none() {
            self.host_data = Some(tempfile::tempdir().map_err(core)?);
        }
        let data = self.host_data.as_ref().expect("host data was initialized").path();
        let backups = data.join("backups");
        fs::create_dir_all(&backups).map_err(core)?;
        let directory = self
            .dir
            .as_ref()
            .expect("an open collection owns its directory")
            .path()
            .to_path_buf();
        let config = HostConfig::new(
            directory,
            backups,
            data.join("app-group/widget.snapshot"),
            clock,
            random,
            std::sync::Arc::new(meguri_storage::space::StatvfsSpace),
        )
        .with_backup_for_testing(self.backup.clone());
        let host = CoreHost::open(config).map_err(core)?;
        let collection = host.collection().map_err(core)?;
        self.backup.bind(collection.clone());
        if self.record_changes {
            self.subscription = Some(host.engine().stream().subscribe());
        }
        self.collection = Some(SessionCollection::Hosted {
            host: Box::new(host),
            collection,
        });
        self.guard()?;
        Ok(true)
    }

    pub(super) fn host(&self) -> Result<&CoreHost, ExecError> {
        match self.collection.as_ref() {
            Some(SessionCollection::Hosted { host, .. }) => Ok(host),
            _ => Err(invalid("the collection is not hosted")),
        }
    }

    pub(super) fn current_collection(&mut self) -> Result<Collection, ExecError> {
        match self.collection.as_mut() {
            Some(SessionCollection::Hosted { host, collection }) => {
                *collection = host.collection().map_err(core)?;
                self.backup.bind(collection.clone());
                Ok(collection.clone())
            }
            _ => Err(invalid("the collection is not hosted")),
        }
    }

    /// Number of pre-delete backup hooks reached by this conformance run.
    pub fn backup_calls(&self) -> usize {
        self.backup.calls()
    }
    pub fn backup_checks(&self) -> Vec<(meguri_engine::Seq, i64)> {
        self.backup.checks()
    }
    pub(super) fn record_changes(&mut self) {
        self.record_changes = true;
        self.backup.record_checks();
    }
    fn collect_changes(&mut self) {
        if let Some(subscription) = &self.subscription {
            while let Some(event) = subscription.try_recv() {
                if matches!(event.kind, meguri_engine::EventKind::Changes(_)) {
                    self.changes.push(event);
                }
            }
        }
    }
    /// Opt-in diagnostics never enter scenario observations or goldens.
    pub fn take_changes(&mut self) -> Vec<std::sync::Arc<meguri_engine::Event>> {
        self.collect_changes();
        std::mem::take(&mut self.changes)
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
        self.media = None;
        if let Some(collection) = self.collection.take() {
            collection
                .close()
                .map_err(|e| ExecError::new("core_error", format!("closing the collection: {e}")))?;
        }
        self.collect_changes();
        self.subscription = None;
        Ok(())
    }

    fn opened(&mut self, collection: OpenedCollection) -> Result<(), ExecError> {
        let collection = SessionCollection::Raw(Box::new(collection));
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

fn object_set(collection: &SessionCollection) -> Result<Objects, ExecError> {
    Ok(collection
        .reader()?
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

fn collection<'s>(session: &'s Session<'_>) -> Result<&'s SessionCollection, ExecError> {
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
    // Another file: the cached day goes (D6); the open gate runs no day
    // query (`sched/day-clock` D14.2).
    session.day.clear();
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
    let rows = match collection(session)? {
        SessionCollection::Raw(raw) => raw.writer().execute_raw(&args.sql, &params).map_err(core)?,
        SessionCollection::Hosted { collection, .. } => {
            let sql = args.sql.clone();
            collection
                .submit_maintenance(
                    meguri_engine::executor::catalog::ANALYZE,
                    std::time::Duration::from_millis(10),
                    move |writer| Ok(writer.execute_raw(&sql, &params)?),
                )
                .wait()
                .map_err(core)?
        }
    };
    let baseline = object_set(collection(session)?)?;
    session.storage.baseline = baseline;
    // W1's reset after a write: the cached day goes (`sched/day-clock` D6).
    // The session keeps no other view and no undo history yet (U38).
    if !args.sql.trim_start().to_ascii_lowercase().starts_with("select") {
        session.day.clear();
        if let Some(SessionCollection::Hosted { host, collection }) = session.storage.collection() {
            collection
                .clear_undo(meguri_engine::ClearCause::ExternalChange)
                .map_err(core)?;
            collection.drop_cached_day().map_err(core)?;
            host.derived()
                .invalidate(meguri_engine::derived_host::Invalidation::ExternalChange);
        }
    }
    Ok(OpResult::Output(json!({ "rows": db_exec_rows(&rows) })))
}

/// A scenario's JSON value as a config value (W4): a number with a fraction
/// or an exponent is a float, one without an integer.
pub(super) fn config_value(value: &Value) -> Result<ConfigValue, ExecError> {
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
    write_config_mode(session, &args.key, &args.value, args.undoable)?;
    Ok(OpResult::Done)
}

fn remove_config(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::RemoveConfig(args) = step else {
        return Err(invalid("expected remove_config"));
    };
    let collection = session.engine_collection()?;
    let key = args.key.clone();
    collection
        .run(config_op(true), move |ctx| {
            ctx.scope().remove_config(&key)?;
            Ok(())
        })
        .map_err(core)?;
    Ok(OpResult::Done)
}

fn config_op(undoable: bool) -> meguri_engine::OpSpec {
    use meguri_engine::{OpSource, OpSpec, StepKind, undo::names::UPDATE_CONFIG};
    let source = if undoable {
        OpSource::UserUndoable
    } else {
        OpSource::UserNonUndoable
    };
    let spec = OpSpec::named("set_config", source);
    if undoable {
        spec.undo_step(UPDATE_CONFIG, StepKind::Other)
    } else {
        spec.skip_undo()
    }
}

/// dsl.md D6.1: the mapping's keys in sorted order, each written as
/// `set_config` writes it (W4).
fn setup_config(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    let mut entries: Vec<(&String, &Value)> = setup.config.iter().flatten().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in entries {
        write_config(session, key, value)?;
    }
    Ok(())
}

/// W4's write of one key: through the config API, as an operation of its
/// own that keeps no undo entry.
fn write_config(session: &mut Session<'_>, key: &str, value: &Value) -> Result<(), ExecError> {
    write_config_mode(session, key, value, false)
}

fn write_config_mode(session: &mut Session<'_>, key: &str, value: &Value, undoable: bool) -> Result<(), ExecError> {
    let value = config_value(value)?;
    let collection = session.engine_collection()?;
    let name = key.to_owned();
    let written = collection
        .run(config_op(undoable), move |ctx| {
            Ok(ctx.scope().set_config(&name, &value)?)
        })
        .map_err(core)?
        .value;
    // The engine's day clock computes afresh right after a change of its
    // keys (`sched/day-clock` D8, D14.2).
    if written.changed() && matches!(key, "rollover" | "creationOffset") {
        session.day.clear();
        super::sched::day_query_now(session)?;
    }
    Ok(())
}

/// B11: the product bootstrap, then the normal open and its first day query.
fn new_collection(session: &mut Session<'_>) -> Result<(), ExecError> {
    session.storage.close()?;
    let path = session.storage.dir()?.join(COLLECTION);
    meguri_engine::bootstrap::create(&path, session.clock.as_ref(), session.random.as_ref()).map_err(core)?;
    let collection = open_collection(&path, &OpenOptions::new(&*session.clock, &*session.random)).map_err(core)?;
    session.storage.opened(collection)?;
    session.day.clear();
    super::sched::day_query_now(session)?;
    Ok(())
}

/// sync-ops.md S6: closes the session's collection and opens it again from
/// its file through the gate.
pub(super) fn reopen_collection(session: &mut Session<'_>) -> Result<(), ExecError> {
    collection(session)?;
    session.storage.close()?;
    let path = session.storage.dir()?.join(COLLECTION);
    let collection = open_collection(&path, &OpenOptions::new(&*session.clock, &*session.random))
        .map_err(|e| core(format!("reopening the collection: {e}")))?;
    session.storage.opened(collection)
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

/// W1's output rows: each row the list of its values by storage class.
pub fn db_exec_rows(rows: &RawRows) -> Value {
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
    let mut objects = collection(session)?.reader()?.schema_objects().map_err(core)?;
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

/// N5: append only unbound row identities, in stored-ID order.
fn adopt(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    let ids = |table: &str| -> Result<Vec<i64>, ExecError> {
        let rows = collection(session)?
            .reader()?
            .execute_raw(&format!("SELECT id FROM {table} ORDER BY id"), &[])
            .map_err(core)?;
        rows.rows
            .into_iter()
            .map(|row| match row.as_slice() {
                [SqlValue::Integer(id)] => Ok(*id),
                _ => Err(core("a stored identity is not an integer")),
            })
            .collect()
    };
    let notes = ids("notes")?;
    let cards = ids("cards")?;
    let append = |bound: &mut Vec<i64>, ids: Vec<i64>| {
        let mut known: BTreeSet<i64> = bound.iter().copied().collect();
        let before = bound.len();
        bound.extend(ids.into_iter().filter(|id| known.insert(*id)));
        bound.len() - before
    };
    Ok(OpResult::Output(
        json!({"notes": append(&mut session.notes, notes), "cards": append(&mut session.cards, cards)}),
    ))
}

/// O8: one table's rows, as `SELECT * FROM <table>` returns them, keyed by
/// their primary key and with the random values hidden.
pub fn db_dump_table(table: &str, rows: &RawRows) -> Result<Value, ExecError> {
    db_dump_table_inner(table, rows, false)
}

/// N4: one stored table, with every GUID and blob byte preserved.
pub fn db_dump_table_raw(table: &str, rows: &RawRows) -> Result<Value, ExecError> {
    db_dump_table_inner(table, rows, true)
}

fn db_dump_table_inner(table: &str, rows: &RawRows, raw: bool) -> Result<Value, ExecError> {
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
                ("notes", "guid", _) if !raw => Value::String("#random".to_owned()),
                ("fields", "config", SqlValue::Blob(bytes)) if !raw => digest_without_field(bytes, 9),
                ("templates", "config", SqlValue::Blob(bytes)) if !raw => digest_without_field(bytes, 8),
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
    let raw = matches!(&request.args, ObservationArgs::Mapping(map) if map.get("raw") == Some(&Value::Bool(true)));
    let tables: Vec<&str> = match &request.args {
        ObservationArgs::Mapping(map) => match map.get("tables") {
            Some(Value::Array(tables)) => tables.iter().filter_map(Value::as_str).collect(),
            _ => DUMP_TABLES.to_vec(),
        },
        _ => DUMP_TABLES.to_vec(),
    };
    collection.with_snapshot(|snapshot| {
        let mut out = Map::new();
        for table in tables {
            if !DUMP_TABLES.contains(&table) {
                return Err(invalid(format!("db_dump has no table {table}")));
            }
            let rows = snapshot
                .execute_raw(&format!("SELECT * FROM {table}"), &[])
                .map_err(core)?;
            out.insert(table.to_owned(), db_dump_table_inner(table, &rows, raw)?);
        }
        Ok(Value::Object(out))
    })
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

/// F1: handoffs share collection media, not Meguri's journal or recovery state.
fn copy_handoff(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if matches!(
            entry.file_name().to_str(),
            Some("meguri-media.db" | "meguri-media.db-wal" | "meguri-media.db-shm" | "media.trash" | "media.staging")
        ) {
            continue;
        }
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
    copy_handoff(&from, dir).map_err(|e| ExecError::new("handoff_error", format!("{}: {e}", dir.display())))
}

/// D9.2, taking over: the handed directory copied into a new session
/// directory and its collection opened through the gate.
fn hand_in(session: &mut Session<'_>, dir: &Path) -> Result<(), ExecError> {
    let to = session.storage.dir()?;
    copy_handoff(dir, &to).map_err(|e| ExecError::new("handoff_error", format!("{}: {e}", dir.display())))?;
    let collection = open_collection(
        &to.join(COLLECTION),
        &OpenOptions::new(&*session.clock, &*session.random),
    )
    .map_err(|e| ExecError::new("handoff_error", format!("opening the handed-over collection: {e}")))?;
    session.day.clear();
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
    fn media_handoff_copies_files_without_journal_or_recovery_state() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        fs::write(from.path().join("collection.anki2"), b"collection").unwrap();
        fs::create_dir(from.path().join("collection.media")).unwrap();
        fs::write(
            from.path().join("collection.media/meguri-media.db"),
            b"ordinary attachment",
        )
        .unwrap();
        for name in ["meguri-media.db", "meguri-media.db-wal", "meguri-media.db-shm"] {
            fs::write(from.path().join(name), b"private sync state").unwrap();
        }
        for name in ["media.trash", "media.staging"] {
            fs::create_dir(from.path().join(name)).unwrap();
            fs::write(from.path().join(name).join("held.png"), b"recovery").unwrap();
        }
        copy_handoff(from.path(), to.path()).unwrap();
        assert_eq!(fs::read(to.path().join("collection.anki2")).unwrap(), b"collection");
        assert_eq!(
            fs::read(to.path().join("collection.media/meguri-media.db")).unwrap(),
            b"ordinary attachment"
        );
        assert_eq!(fs::read_dir(to.path()).unwrap().count(), 2);
    }

    /// dsl.md D6.1 written as W4 writes: each key a local change at the
    /// clock's instant that moves `col.mod`, and a `rollover` change computes
    /// the day afresh at once (`sched/day-clock` D8, D14.2). The day is
    /// D12.2's example: `crt` 2030-01-15 04:00 +01:00, rollover 12 at
    /// 2030-01-20 10:00 in Paris is day 4 until 12:00.
    #[test]
    fn setup_config_writes_each_key_as_set_config_does() {
        use std::sync::Arc;

        use meguri_model::clock::UtcOffset;
        use meguri_model::rng::SeededRandom;

        use crate::exec::clock::ScenarioClock;

        assert!(
            crate::exec::MeguriExecutor::new()
                .capabilities()
                .setup
                .contains("config")
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scenarios/sched/setup-config.yaml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "id: sched/setup-config\nclock: {tz: Europe/Paris, start: \"2030-01-20T10:00:00\", mode: step}\n\
             setup:\n  config: {rollover: 12, collapseTime: 600}\n",
        )
        .unwrap();
        let loaded = crate::scenario::load(&path).unwrap();
        let mut session = Session {
            decks: Default::default(),
            scenario: &loaded,
            clock: Arc::new(ScenarioClock::new("Europe/Paris", "2030-01-20T10:00:00").unwrap()),
            random: Arc::new(SeededRandom::new(1)),
            notes: Vec::new(),
            cards: Vec::new(),
            actor: None,
            storage: Storage::default(),
            day: Default::default(),
        };
        let file = session.storage.dir().unwrap().join(COLLECTION);
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
            &file,
        )
        .unwrap();
        let opened = open_collection(&file, &OpenOptions::new(&*session.clock, &*session.random)).unwrap();
        session.storage.opened(opened).unwrap();
        let sql = |session: &mut Session<'_>, sql: &str| {
            let step: Step = serde_json::from_value(json!({"db_exec": {"sql": sql}})).unwrap();
            let OpResult::Output(value) = db_exec(session, &step).unwrap() else {
                panic!("expected query rows");
            };
            value["rows"].clone()
        };
        sql(&mut session, "UPDATE col SET crt = 1894676400");
        sql(
            &mut session,
            "UPDATE config SET val = CAST('-60' AS BLOB) WHERE KEY = 'creationOffset'",
        );

        setup_config(&mut session, &loaded.scenario.setup).unwrap();
        assert_eq!(
            sql(
                &mut session,
                "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY IN ('collapseTime', \
                 'rollover') ORDER BY KEY"
            ),
            json!([
                ["collapseTime", -1, 1_895_130_000, "600"],
                ["rollover", -1, 1_895_130_000, "12"]
            ])
        );
        assert_eq!(
            sql(&mut session, "SELECT mod FROM col"),
            json!([[1_895_130_000_000_i64]])
        );
        let day = session
            .day
            .valid_at(1_895_130_000, UtcOffset::from_seconds(3600).unwrap())
            .expect("the day query ran");
        assert_eq!((day.today, day.next_day_at), (4, 1_895_137_200));
    }

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
