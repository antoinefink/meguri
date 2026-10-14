//! Raw SQL on a collection's connections, for tests and the conformance
//! runner only (feature `test-hooks`, KTD18): `db_exec` and `db_dump`
//! (`specs/conformance/storage-ops.md` P2, `observations.md` O8) and the
//! crate's own tests. Never in Release.

use rusqlite::types::{Value, ValueRef};
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
        let mut values = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            values.push(match row.get_ref(index)? {
                ValueRef::Null => SqlValue::Null,
                ValueRef::Integer(v) => SqlValue::Integer(v),
                ValueRef::Real(v) => SqlValue::Real(v),
                ValueRef::Text(bytes) => SqlValue::Text(String::from_utf8_lossy(bytes).into_owned()),
                ValueRef::Blob(bytes) => SqlValue::Blob(bytes.to_vec()),
            });
        }
        out.push(values);
    }
    Ok(RawRows { columns, rows: out })
}
