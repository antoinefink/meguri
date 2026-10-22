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

fn has_table(conn: &Connection, name: &str, error: fn(&rusqlite::Error) -> ProbeError) -> Result<bool, ProbeError> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1 COLLATE NOCASE",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(|e| error(&e))
}

fn not_a_collection(missing: &[&str]) -> ProbeError {
    ProbeError::Open(OpenError::NotACollection {
        missing_tables: missing.iter().map(|t| (*t).to_owned()).collect(),
    })
}

fn read(conn: &Connection) -> Result<Probed, ProbeError> {
    read_with(conn, sqlite)
}

fn read_version(conn: &Connection, error: fn(&rusqlite::Error) -> ProbeError) -> Result<i64, ProbeError> {
    if !has_table(conn, "col", error)? {
        return Err(not_a_collection(&["col"]));
    }
    conn.query_row("SELECT ver FROM col LIMIT 1", [], |row| match row.get_ref(0)? {
        ValueRef::Integer(v) => Ok(Some(v)),
        _ => Ok(None),
    })
    .optional()
    .map_err(|e| error(&e))?
    .flatten()
    .ok_or_else(|| not_a_collection(&[]))
}

fn read_with(conn: &Connection, error: fn(&rusqlite::Error) -> ProbeError) -> Result<Probed, ProbeError> {
    let version = read_version(conn, error)?;
    let scheduler = match version {
        11 => {
            let conf = conn
                .query_row("SELECT conf FROM col LIMIT 1", [], |row| {
                    Ok(text_bytes(row.get_ref(0)?))
                })
                .map_err(|e| error(&e))?;
            conf_scheduler(conf.as_deref())
        }
        14..=18 => {
            if !has_table(conn, "config", error)? {
                return Err(not_a_collection(&["config"]));
            }
            let value = conn
                .query_row("SELECT val FROM config WHERE KEY = 'schedVer'", [], |row| {
                    Ok(text_bytes(row.get_ref(0)?))
                })
                .optional()
                .map_err(|e| error(&e))?
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

/// P8 limits scheduler payloads selected during candidate validation. The caller must
/// keep this unpublished candidate exclusive until subsequent gate/open reads.
pub(crate) const CANDIDATE_PROBE_VALUE_LIMIT: i64 = 4 * 1024 * 1024;

fn candidate_sqlite(error: &rusqlite::Error) -> ProbeError {
    use rusqlite::ErrorCode;
    match error.sqlite_error_code() {
        Some(
            ErrorCode::SystemIoFailure
            | ErrorCode::CannotOpen
            | ErrorCode::PermissionDenied
            | ErrorCode::ReadOnly
            | ErrorCode::DiskFull
            | ErrorCode::DatabaseBusy
            | ErrorCode::DatabaseLocked,
        ) => sqlite(error),
        _ => ProbeError::Open(OpenError::Corrupt {
            detail: error.to_string(),
        }),
    }
}

fn stored_probe_columns(conn: &Connection, table: &str, columns: &[&str]) -> Result<(), ProbeError> {
    // A virtual/generated column could compute its contents before the byte
    // check. P8's metadata optimization requires ordinary stored columns.
    let ordinary: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_list WHERE schema = 'main' AND name = ?1 COLLATE NOCASE AND type = 'table')",
        [table], |row| row.get(0),
    ).map_err(|e| candidate_sqlite(&e))?;
    for column in columns {
        let stored: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_xinfo(?1, 'main') WHERE name = ?2 COLLATE NOCASE AND hidden = 0)",
            [table, *column], |row| row.get(0),
        ).map_err(|e| candidate_sqlite(&e))?;
        if !ordinary || !stored {
            return Err(ProbeError::Open(OpenError::Corrupt {
                detail: "candidate probe requires stored columns".into(),
            }));
        }
    }
    Ok(())
}

fn check_candidate_values(conn: &Connection, version: i64) -> Result<(), ProbeError> {
    // Keep octet_length's argument a direct table column: SQLite uses stored
    // byte-count metadata for TEXT/BLOB, without fetching overflow contents.
    let sql = match version {
        11 => {
            stored_probe_columns(conn, "col", &["conf"])?;
            "SELECT octet_length(conf) FROM col LIMIT 1"
        }
        14..=18 => {
            if !has_table(conn, "config", candidate_sqlite)? {
                return Err(not_a_collection(&["config"]));
            }
            stored_probe_columns(conn, "config", &["key", "val"])?;
            "SELECT octet_length(val) FROM config WHERE KEY = 'schedVer' LIMIT 1"
        }
        _ => return Ok(()),
    };
    let bytes = conn
        .query_row(sql, [], |row| row.get::<_, Option<i64>>(0))
        .optional()
        .map_err(|e| candidate_sqlite(&e))?
        .flatten();
    if bytes.is_some_and(|n| !(0..=CANDIDATE_PROBE_VALUE_LIMIT).contains(&n)) {
        return Err(ProbeError::Open(OpenError::Corrupt {
            detail: "candidate probe value byte limit".into(),
        }));
    }
    Ok(())
}

/// A bounded probe for a private/stable candidate; ordinary collection opens
/// retain their existing behavior. All shape errors keep typed SQLite sources
/// until classification, so SQL errors cannot masquerade as filesystem I/O.
pub(crate) fn probe_candidate(path: &Path) -> Result<Probed, ProbeError> {
    let conn = open_read_only(path)?;
    let probed = read_candidate(&conn)?;
    conn.close().map_err(|(_, e)| candidate_sqlite(&e))?;
    Ok(probed)
}

fn read_candidate(conn: &Connection) -> Result<Probed, ProbeError> {
    if has_table(conn, "col", candidate_sqlite)? {
        stored_probe_columns(conn, "col", &["ver"])?;
        // typeof on a direct stored column uses its record header, so a
        // malformed version never loads a TEXT/BLOB overflow payload.
        let integer = conn
            .query_row("SELECT typeof(ver) = 'integer' FROM col LIMIT 1", [], |row| {
                row.get::<_, bool>(0)
            })
            .optional()
            .map_err(|e| candidate_sqlite(&e))?;
        if integer != Some(true) {
            return Err(not_a_collection(&[]));
        }
    }
    let version = read_version(conn, candidate_sqlite)?;
    check_candidate_values(conn, version)?;
    read_with(conn, candidate_sqlite)
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

#[cfg(test)]
mod candidate_tests {
    use super::*;

    fn small_probe(version: i64, value: rusqlite::types::Value) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE col (ver INTEGER, conf); CREATE TABLE config (key TEXT PRIMARY KEY, val)")
            .unwrap();
        conn.execute("INSERT INTO col VALUES (?1, ?2)", rusqlite::params![version, value])
            .unwrap();
        conn.execute("INSERT INTO config VALUES ('schedVer', ?1)", [value])
            .unwrap();
        conn
    }

    #[test]
    fn bounded_inputs_keep_ordinary_probe_storage_class_and_content_behavior() {
        use rusqlite::types::Value;
        let values = [
            Value::Null,
            Value::Integer(2),
            Value::Real(2.0),
            Value::Text("2".into()),
            Value::Blob(b"2".to_vec()),
            Value::Text("{\"schedVer\":2}".into()),
            Value::Blob(b"{\"schedVer\":2}".to_vec()),
            Value::Text("invalid json".into()),
            Value::Blob(vec![0xff]),
        ];
        for version in [11, 14, 18] {
            for value in &values {
                let conn = small_probe(version, value.clone());
                check_candidate_values(&conn, version).unwrap();
                assert_eq!(read_with(&conn, candidate_sqlite).unwrap(), read(&conn).unwrap());
            }
            let conn = small_probe(version, Value::Null);
            conn.execute("DELETE FROM config", []).unwrap();
            check_candidate_values(&conn, version).unwrap();
            assert_eq!(read_with(&conn, candidate_sqlite).unwrap(), read(&conn).unwrap());
        }
    }

    #[test]
    fn probe_limit_counts_utf8_bytes_instead_of_characters() {
        // Each character takes two UTF-8 bytes; neither expression uses SQL
        // length(TEXT), which would walk and decode the stored contents.
        let conn = small_probe(18, rusqlite::types::Value::Text("é".repeat(2 * 1024 * 1024)));
        check_candidate_values(&conn, 18).unwrap();
        conn.execute("UPDATE config SET val = val || 'x'", []).unwrap();
        assert!(matches!(
            check_candidate_values(&conn, 18),
            Err(ProbeError::Open(OpenError::Corrupt { .. }))
        ));
    }

    #[test]
    fn generated_probe_values_are_rejected_before_evaluation() {
        for version in [11, 18] {
            let conn = Connection::open_in_memory().unwrap();
            let schema = if version == 11 {
                "CREATE TABLE col (ver INTEGER, conf GENERATED ALWAYS AS (zeroblob(67108864))); INSERT INTO col (ver) VALUES (11)"
            } else {
                "CREATE TABLE col (ver INTEGER); INSERT INTO col VALUES (18); CREATE TABLE config (key TEXT, val GENERATED ALWAYS AS (zeroblob(67108864))); INSERT INTO config (key) VALUES ('schedVer')"
            };
            conn.execute_batch(schema).unwrap();
            assert!(
                matches!(check_candidate_values(&conn, version), Err(ProbeError::Open(OpenError::Corrupt { detail })) if detail == "candidate probe requires stored columns")
            );
        }
    }

    fn cache_misses(conn: &Connection) -> i32 {
        let (mut current, mut highwater) = (0, 0);
        // SAFETY: the connection remains live and exclusively used by this
        // test; both SQLite output pointers address initialized local storage.
        let result = unsafe {
            rusqlite::ffi::sqlite3_db_status(
                conn.handle(),
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_MISS,
                &mut current,
                &mut highwater,
                0,
            )
        };
        assert_eq!(result, rusqlite::ffi::SQLITE_OK);
        current
    }

    #[test]
    fn production_size_check_does_not_fetch_oversized_text_or_blob_overflow_pages() {
        for version in [11, 14, 18] {
            for blob in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("candidate.db");
                let conn = Connection::open(&path).unwrap();
                conn.execute_batch("PRAGMA page_size=4096; CREATE TABLE col (ver INTEGER, conf); CREATE TABLE config (key TEXT PRIMARY KEY, val)").unwrap();
                let value = if blob {
                    "zeroblob(67108864)"
                } else {
                    "CAST(zeroblob(67108864) AS TEXT)"
                };
                let sql = if version == 11 {
                    format!("INSERT INTO col VALUES (11, {value})")
                } else {
                    format!("INSERT INTO col VALUES ({version}, NULL); INSERT INTO config VALUES ('schedVer', {value})")
                };
                conn.execute_batch(&sql).unwrap();
                conn.close().unwrap();
                assert!(std::fs::metadata(&path).unwrap().len() > 64 * 1024 * 1024);
                let conn = open_read_only(&path).unwrap();
                // With mmap disabled all payload reads must pass the pager.
                // Reopening with minimal cache space exposes overflow-page reads.
                conn.execute_batch("PRAGMA mmap_size=0; PRAGMA cache_size=-64").unwrap();
                let before = cache_misses(&conn);
                assert_eq!(read_version(&conn, candidate_sqlite).unwrap(), version);
                assert!(matches!(
                    check_candidate_values(&conn, version),
                    Err(ProbeError::Open(OpenError::Corrupt { .. }))
                ));
                let reads = cache_misses(&conn) - before;
                assert!(reads < 16, "metadata-only bound fetched {reads} pages");
            }
        }
    }

    #[test]
    fn candidate_version_metadata_refuses_large_noninteger_values_with_bounded_pager_reads() {
        for blob in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("candidate.db");
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA page_size=4096; CREATE TABLE col (ver INTEGER)")
                .unwrap();
            let value = if blob {
                "zeroblob(67108864)"
            } else {
                "CAST(zeroblob(67108864) AS TEXT)"
            };
            conn.execute_batch(&format!("INSERT INTO col VALUES ({value})"))
                .unwrap();
            conn.close().unwrap();
            assert!(std::fs::metadata(&path).unwrap().len() > 64 * 1024 * 1024);
            let conn = open_read_only(&path).unwrap();
            conn.execute_batch("PRAGMA mmap_size=0; PRAGMA cache_size=-64").unwrap();
            let before = cache_misses(&conn);
            assert!(matches!(
                read_candidate(&conn),
                Err(ProbeError::Open(OpenError::NotACollection { missing_tables })) if missing_tables.is_empty()
            ));
            let reads = cache_misses(&conn) - before;
            assert!(reads < 16, "version metadata check fetched {reads} pages");
            conn.close().unwrap();
            assert!(matches!(
                probe_candidate(&path),
                Err(ProbeError::Open(OpenError::NotACollection { missing_tables })) if missing_tables.is_empty()
            ));
        }
    }

    #[test]
    fn candidate_version_metadata_keeps_missing_empty_and_noninteger_refusals() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(matches!(
            read_candidate(&conn),
            Err(ProbeError::Open(OpenError::NotACollection { missing_tables })) if missing_tables == ["col"]
        ));
        conn.execute_batch("CREATE TABLE col (ver INTEGER)").unwrap();
        for value in [None, Some("NULL"), Some("2.5"), Some("'invalid'"), Some("X'32'")] {
            conn.execute("DELETE FROM col", []).unwrap();
            if let Some(value) = value {
                conn.execute_batch(&format!("INSERT INTO col VALUES ({value})"))
                    .unwrap();
            }
            assert!(matches!(
                read_candidate(&conn),
                Err(ProbeError::Open(OpenError::NotACollection { missing_tables })) if missing_tables.is_empty()
            ));
        }
    }
}
