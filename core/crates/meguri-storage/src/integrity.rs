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

// Fixed Q5 inspection for generated smoke tests. The caller retains the owned
// read snapshot, so integrity, schema and references see the same WAL view.
#[cfg(feature = "test-hooks")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmokeIntegrity {
    pub integrity_ok: bool,
    pub schema_ok: bool,
    pub missing_card_notes: u64,
    pub missing_card_decks: u64,
    pub missing_note_types: u64,
    pub missing_field_types: u64,
    pub wrong_field_counts: u64,
}
#[cfg(feature = "test-hooks")]
impl crate::snapshot::ReadSnapshot<'_> {
    pub fn smoke_integrity(&self) -> Result<SmokeIntegrity, StorageError> {
        let conn = self.conn();
        let integrity_ok = check(conn, "PRAGMA main.integrity_check")?.is_ok();
        let objects = crate::schema::expected_objects::check(&self.reader().schema_objects()?);
        let version: i64 = conn.query_row("SELECT ver FROM col WHERE id=1", [], |r| r.get(0))?;
        let count = |sql: &str| -> Result<u64, StorageError> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
        Ok(SmokeIntegrity {
            integrity_ok,
            schema_ok: version == crate::schema::SCHEMA_VERSION
                && objects.missing_tables.is_empty()
                && objects.missing_indexes.is_empty()
                && objects.foreign.is_empty(),
            missing_card_notes: count(
                "SELECT count(*) FROM cards c LEFT JOIN notes n ON n.id=c.nid WHERE n.id IS NULL",
            )?,
            missing_card_decks: count(
                "SELECT count(*) FROM cards c WHERE NOT EXISTS (SELECT 1 FROM decks d WHERE d.id=c.did) OR (c.odid != 0 AND NOT EXISTS (SELECT 1 FROM decks d WHERE d.id=c.odid))",
            )?,
            missing_note_types: count(
                "SELECT count(*) FROM notes n LEFT JOIN notetypes t ON t.id=n.mid WHERE t.id IS NULL",
            )?,
            missing_field_types: count(
                "SELECT (SELECT count(*) FROM fields f WHERE NOT EXISTS (SELECT 1 FROM notetypes t WHERE t.id=f.ntid)) + (SELECT count(*) FROM templates f WHERE NOT EXISTS (SELECT 1 FROM notetypes t WHERE t.id=f.ntid))",
            )?,
            wrong_field_counts: count(
                "SELECT count(*) FROM notes n WHERE (length(CAST(n.flds AS BLOB))-length(CAST(replace(n.flds,char(31),'') AS BLOB))+1) != (SELECT count(*) FROM fields f WHERE f.ntid=n.mid)",
            )?,
        })
    }
}
