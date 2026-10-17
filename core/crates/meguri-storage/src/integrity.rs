//! Read-only integrity primitives (F5); scheduling and policy belong to U132.
use crate::{
    Reader, StorageError,
    connections::{READ_ONLY, file_uri},
};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityReport {
    pub diagnostics: Vec<String>,
}
impl IntegrityReport {
    pub fn is_ok(&self) -> bool {
        self.diagnostics == ["ok"]
    }
}
fn check(conn: &rusqlite::Connection, sql: &str) -> Result<IntegrityReport, StorageError> {
    let result = (|| {
        let mut stmt = conn.prepare(sql)?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()
    })();
    match result {
        Ok(diagnostics) => Ok(IntegrityReport { diagnostics }),
        Err(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
            ) =>
        {
            Ok(IntegrityReport {
                diagnostics: vec![error.to_string()],
            })
        }
        Err(error) => Err(error.into()),
    }
}
impl Reader {
    pub fn quick_check(&self) -> Result<IntegrityReport, StorageError> {
        check(&self.conn, "PRAGMA main.quick_check")
    }
}
pub fn integrity_check(path: &Path) -> Result<IntegrityReport, StorageError> {
    let conn = rusqlite::Connection::open_with_flags(file_uri(path, Some("mode=ro")), READ_ONLY)?;
    crate::conn_init::probe(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    check(&conn, "PRAGMA main.integrity_check")
}
