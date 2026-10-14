//! Step 18 (`storage/schema-upgrade` U15): `graves` rebuilt with a primary
//! key; of rows repeating an `(oid, type)`, the first one written wins.

use rusqlite::Connection;

use super::StepResult;
use crate::schema::ddl18;

pub(super) fn run(conn: &Connection) -> StepResult {
    conn.execute_batch(&format!(
        "ALTER TABLE graves RENAME TO graves_old;
         {};
         INSERT OR IGNORE INTO graves (oid, type, usn) SELECT oid, type, usn FROM graves_old ORDER BY rowid;
         DROP TABLE graves_old;
         {};",
        ddl18("graves"),
        ddl18("idx_graves_pending"),
    ))?;
    Ok(())
}
