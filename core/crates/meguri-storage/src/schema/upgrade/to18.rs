//! Step 18 (`storage/schema-upgrade` U15): `graves` rebuilt with a primary
//! key; of rows repeating an `(oid, type)`, the first one written wins.
//!
//! The old rows wait in a TEMP table (SQLite's temp schema, never the file)
//! instead of a renamed `graves`: renaming a table rewrites the views and
//! triggers that name it to the new name, and those must keep reading the
//! rebuilt `graves`. Foreign indexes and triggers on `graves` come back after
//! the rows (U12).

use rusqlite::Connection;

use super::{StepResult, foreign_dependents, restore};
use crate::schema::ddl18;

pub(super) fn run(conn: &Connection) -> StepResult {
    let foreign = foreign_dependents(conn, "graves")?;
    conn.execute_batch(&format!(
        "CREATE TEMP TABLE graves_before_18 AS SELECT oid, type, usn FROM main.graves ORDER BY rowid;
         DROP TABLE main.graves;
         {};
         INSERT OR IGNORE INTO main.graves (oid, type, usn) SELECT oid, type, usn FROM temp.graves_before_18 ORDER BY rowid;
         DROP TABLE temp.graves_before_18;
         {};",
        ddl18("graves"),
        ddl18("idx_graves_pending"),
    ))?;
    restore(conn, &foreign)
}
