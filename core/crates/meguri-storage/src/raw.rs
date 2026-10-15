//! Raw SQL on a collection's connections, for tests and the conformance
//! runner only (feature `test-hooks`, KTD18): `db_exec` and `db_dump`
//! (`specs/conformance/storage-ops.md` P2, `observations.md` O8) and the
//! crate's own tests. Never in Release.
//!
//! [`PlainDatabase`] is a collection file opened as a plain SQLite database,
//! outside the open gate, for test tools that must write nothing but their
//! own changes (the anonymizer, `specs/fixtures/anonymizer.md` A14.3).

use std::path::Path;

use rusqlite::types::{ToSqlOutput, Type, Value, ValueRef};
use rusqlite::{Connection, OpenFlags, ToSql, params_from_iter};

use crate::collation;
use crate::connections::{Reader, Writer};
use crate::error::StorageError;
use crate::snapshot::ReadSnapshot;

/// A value by its SQLite storage class.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    /// TEXT; bytes that are not UTF-8 are replaced.
    Text(String),
    Blob(Vec<u8>),
}

/// The rows a statement returned, with its column names as SQLite reports
/// them (for `SELECT *`, as the stored table definition declares them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}

impl Writer {
    /// Runs exactly one statement with positional parameters, outside any
    /// transaction of Meguri's (it commits at once), and returns its rows.
    pub fn execute_raw(&self, sql: &str, params: &[SqlValue]) -> Result<RawRows, StorageError> {
        run(&self.conn, sql, params)
    }
}

impl Reader {
    /// Runs exactly one statement on this reader and returns its rows.
    pub fn execute_raw(&self, sql: &str, params: &[SqlValue]) -> Result<RawRows, StorageError> {
        run(&self.conn, sql, params)
    }
}

impl ReadSnapshot<'_> {
    /// Runs exactly one statement inside the snapshot and returns its rows.
    pub fn execute_raw(&self, sql: &str, params: &[SqlValue]) -> Result<RawRows, StorageError> {
        run(self.conn(), sql, params)
    }
}

fn run(conn: &Connection, sql: &str, params: &[SqlValue]) -> Result<RawRows, StorageError> {
    let mut statement = conn.prepare(sql)?;
    let columns: Vec<String> = statement.column_names().into_iter().map(str::to_owned).collect();
    let values = params.iter().map(|p| match p {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(v) => Value::Integer(*v),
        SqlValue::Real(v) => Value::Real(*v),
        SqlValue::Text(v) => Value::Text(v.clone()),
        SqlValue::Blob(v) => Value::Blob(v.clone()),
    });
    let mut rows = statement.query(params_from_iter(values))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let cells = (0..columns.len())
            .map(|index| row.get_ref(index).and_then(stored))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out.push(cells);
    }
    Ok(RawRows { columns, rows: out })
}

/// One result value, by the storage class SQLite reports for it (P2.2).
fn stored(cell: ValueRef<'_>) -> rusqlite::Result<SqlValue> {
    let class = cell.data_type();
    Ok(match class {
        Type::Null => SqlValue::Null,
        Type::Integer => SqlValue::Integer(cell.as_i64()?),
        Type::Real => SqlValue::Real(cell.as_f64()?),
        Type::Blob => SqlValue::Blob(cell.as_blob()?.to_vec()),
        // TEXT is reported as a string, so bytes that are not UTF-8 are
        // replaced rather than refused.
        Type::Text => SqlValue::Text(String::from_utf8_lossy(cell.as_bytes()?).into_owned()),
    })
}

/// A stored value with its storage class and its exact bytes: TEXT that is
/// not UTF-8 stays as stored.
#[derive(Clone, Debug, PartialEq)]
pub enum RawValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl RawValue {
    fn of(cell: ValueRef<'_>) -> RawValue {
        match cell {
            ValueRef::Null => RawValue::Null,
            ValueRef::Integer(v) => RawValue::Integer(v),
            ValueRef::Real(v) => RawValue::Real(v),
            ValueRef::Text(bytes) => RawValue::Text(bytes.to_vec()),
            ValueRef::Blob(bytes) => RawValue::Blob(bytes.to_vec()),
        }
    }

    /// TEXT holding `text`.
    pub fn text(text: &str) -> RawValue {
        RawValue::Text(text.as_bytes().to_vec())
    }
}

impl ToSql for RawValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            RawValue::Null => ValueRef::Null,
            RawValue::Integer(v) => ValueRef::Integer(*v),
            RawValue::Real(v) => ValueRef::Real(*v),
            // Bound as TEXT byte for byte, whether or not it is UTF-8.
            RawValue::Text(bytes) => ValueRef::Text(bytes),
            RawValue::Blob(bytes) => ValueRef::Blob(bytes),
        }))
    }
}

/// A collection file opened as a plain SQLite database: read-write, never
/// created, with the `unicase` collation registered (`storage/schema-18`
/// S4) and nothing else. No open gate, no pragma, no upgrade: the caller's
/// statements are the only writes.
#[derive(Debug)]
pub struct PlainDatabase {
    conn: Connection,
}

impl PlainDatabase {
    /// Opens the file at `path`, which must exist. A file that is not an
    /// SQLite database opens; its first statement fails (code 26).
    pub fn open(path: &Path) -> Result<PlainDatabase, StorageError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;
        collation::register(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
        Ok(PlainDatabase { conn })
    }

    /// Runs one statement and returns the number of rows it changed.
    pub fn execute(&self, sql: &str, params: &[RawValue]) -> Result<usize, StorageError> {
        let mut statement = self.conn.prepare_cached(sql)?;
        Ok(statement.execute(params_from_iter(params.iter()))?)
    }

    /// Runs statements separated by `;`, without parameters.
    pub fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        Ok(self.conn.execute_batch(sql)?)
    }

    /// Runs one query and hands each row to `row`, in the order SQLite
    /// returns them.
    pub fn for_each(
        &self,
        sql: &str,
        params: &[RawValue],
        mut row: impl FnMut(Vec<RawValue>),
    ) -> Result<(), StorageError> {
        let mut statement = self.conn.prepare_cached(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query(params_from_iter(params.iter()))?;
        while let Some(r) = rows.next()? {
            let mut cells = Vec::with_capacity(columns);
            for index in 0..columns {
                cells.push(RawValue::of(r.get_ref(index)?));
            }
            row(cells);
        }
        Ok(())
    }

    /// Runs one query and returns its rows.
    pub fn rows(&self, sql: &str, params: &[RawValue]) -> Result<Vec<Vec<RawValue>>, StorageError> {
        let mut out = Vec::new();
        self.for_each(sql, params, |row| out.push(row))?;
        Ok(out)
    }

    /// Closes the connection, reporting a failure to close.
    pub fn close(self) -> Result<(), StorageError> {
        self.conn.close().map_err(|(_, e)| StorageError::from(e))
    }
}
