//! Phase 1 of the open gate, the probe (`storage/open-and-connections`
//! C2.1): a read-only look at `col.ver` and the scheduler marker that writes
//! nothing and creates no file next to a file it may refuse.

use std::path::Path;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension};
use serde_json::value::RawValue;

use crate::conn_init;
use crate::connections::{READ_ONLY, file_uri, sibling};
use crate::error::OpenError;
use crate::schema::upgrade::legacy_json;

/// What the probe found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probed {
    pub version: i64,
    pub scheduler: Scheduler,
}

/// The scheduler marker (C2.2): the JSON value of the config key `schedVer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheduler {
    /// The value is the JSON integer 2: the v2 or v3 scheduler.
    V2,
    /// Anything else, or no value: the v1 scheduler.
    V1,
    /// At version 11, `col.conf` is not a JSON object.
    ConfNotAnObject,
    /// Not read: the version is refused anyway.
    NotRead,
}

/// Why the probe stopped.
#[derive(Debug)]
pub(crate) enum ProbeError {
    /// A hot rollback journal: SQLite cannot read the file read-only (C3).
    HotJournal,
    Open(OpenError),
}

impl From<OpenError> for ProbeError {
    fn from(error: OpenError) -> ProbeError {
        ProbeError::Open(error)
    }
}

/// SQLite's read-only rollback error (`SQLITE_READONLY_ROLLBACK`).
const READONLY_ROLLBACK: i32 = 776;

fn sqlite(error: &rusqlite::Error) -> ProbeError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) if failure.extended_code == READONLY_ROLLBACK => {
            ProbeError::HotJournal
        }
        other => ProbeError::Open(OpenError::from_sqlite(other)),
    }
}

/// The probe's URI query: `immutable=1` unless a `-wal` file (any size) or a
/// non-empty `-journal` file lies next to the file, which `immutable` would
/// ignore.
pub(crate) fn probe_query(path: &Path) -> &'static str {
    let wal = sibling(path, "-wal").exists();
    let journal = std::fs::metadata(sibling(path, "-journal")).is_ok_and(|m| m.len() > 0);
    if wal || journal {
        "mode=ro"
    } else {
        "mode=ro&immutable=1"
    }
}

/// Opens `path` read-only as the probe does, with the collation registered.
pub(crate) fn open_read_only(path: &Path) -> Result<Connection, ProbeError> {
    let conn =
        Connection::open_with_flags(file_uri(path, Some(probe_query(path))), READ_ONLY).map_err(|e| sqlite(&e))?;
    conn_init::probe(&conn).map_err(|detail| OpenError::ConnectionInit { detail })?;
    Ok(conn)
}

/// Phase 1: reads whether `col` exists, `ver` from its first row, and the
/// scheduler marker when the version may be opened. Closes before returning.
pub(crate) fn probe(path: &Path) -> Result<Probed, ProbeError> {
    let conn = open_read_only(path)?;
    let probed = read(&conn)?;
    conn.close().map_err(|(_, e)| sqlite(&e))?;
    Ok(probed)
}

fn has_table(conn: &Connection, name: &str) -> Result<bool, ProbeError> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1 COLLATE NOCASE",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(|e| sqlite(&e))
}

fn not_a_collection(missing: &[&str]) -> ProbeError {
    ProbeError::Open(OpenError::NotACollection {
        missing_tables: missing.iter().map(|t| (*t).to_owned()).collect(),
    })
}

fn read(conn: &Connection) -> Result<Probed, ProbeError> {
    if !has_table(conn, "col")? {
        return Err(not_a_collection(&["col"]));
    }
    let version = conn
        .query_row("SELECT ver FROM col LIMIT 1", [], |row| match row.get_ref(0)? {
            ValueRef::Integer(v) => Ok(Some(v)),
            _ => Ok(None),
        })
        .optional()
        .map_err(|e| sqlite(&e))?
        .flatten()
        .ok_or_else(|| not_a_collection(&[]))?;
    let scheduler = match version {
        11 => {
            let conf = conn
                .query_row("SELECT conf FROM col LIMIT 1", [], |row| {
                    Ok(text_bytes(row.get_ref(0)?))
                })
                .map_err(|e| sqlite(&e))?;
            conf_scheduler(conf.as_deref())
        }
        14..=18 => {
            if !has_table(conn, "config")? {
                return Err(not_a_collection(&["config"]));
            }
            let value = conn
                .query_row("SELECT val FROM config WHERE KEY = 'schedVer'", [], |row| {
                    Ok(text_bytes(row.get_ref(0)?))
                })
                .optional()
                .map_err(|e| sqlite(&e))?
                .flatten();
            if value.as_deref().is_some_and(is_json_two) {
                Scheduler::V2
            } else {
                Scheduler::V1
            }
        }
        _ => Scheduler::NotRead,
    };
    Ok(Probed { version, scheduler })
}

/// The bytes of a TEXT or BLOB value.
fn text_bytes(value: ValueRef<'_>) -> Option<Vec<u8>> {
    match value {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Some(bytes.to_vec()),
        _ => None,
    }
}

/// Whether JSON text is the integer 2 (`2.0`, `"2"` or `2e0` are not).
fn is_json_two(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| serde_json::from_str::<&RawValue>(text).ok())
        .is_some_and(|raw| raw.get() == "2")
}

/// The marker at version 11: the member `schedVer` of the JSON object in
/// `col.conf` (a repeated key keeps its last value).
fn conf_scheduler(conf: Option<&[u8]>) -> Scheduler {
    let members = conf
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(legacy_json::members);
    match members {
        None => Scheduler::ConfNotAnObject,
        Some(members) => match members.iter().find(|(key, _)| key == "schedVer") {
            Some((_, raw)) if raw.get() == "2" => Scheduler::V2,
            _ => Scheduler::V1,
        },
    }
}
