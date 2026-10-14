//! Raw SQL on a collection's connections, for tests and the conformance
//! runner only (feature `test-hooks`, KTD18): `db_exec` and `db_dump`
//! (`specs/conformance/storage-ops.md` P2, `observations.md` O8) and the
//! crate's own tests. Never in Release.

use rusqlite::types::{Type, Value, ValueRef};
use rusqlite::{Connection, params_from_iter};

use crate::connections::{Reader, Writer};
use crate::error::StorageError;

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
