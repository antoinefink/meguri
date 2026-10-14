//! The upgrade of schemas 11, 14, 15, 16 and 17 to 18
//! (`storage/schema-upgrade`, R1478): every step above the file's version,
//! in order, inside one `IMMEDIATE` transaction on the writer, so a crash at
//! any point leaves the original as it was and the next open upgrades again
//! from the start (U3). The gate made the pre-upgrade copy before (U2).

pub(crate) mod legacy_json;
mod to14;
mod to15;
mod to16;
mod to17;
mod to18;

use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use rusqlite::Connection;

use crate::error::{OpenError, UpgradeStage};
use crate::schema::expected_objects::SchemaObject;
use crate::schema::refuse;

/// What the steps need from the caller.
pub(crate) struct UpgradeContext<'a> {
    /// "Now" for decks the upgrade renames (U7).
    pub clock: &'a dyn Clock,
    /// New ids for note types stored under id 0 (U6).
    pub random: &'a dyn RandomSource,
    /// Called with each step's number as it starts.
    pub progress: &'a dyn Fn(u32),
}

/// Why a step failed (U10): the text is for logs, never shown as is.
#[derive(Debug)]
pub(crate) struct StepError(pub String);

impl From<rusqlite::Error> for StepError {
    fn from(error: rusqlite::Error) -> StepError {
        StepError(error.to_string())
    }
}

impl From<meguri_model::ModelError> for StepError {
    fn from(error: meguri_model::ModelError) -> StepError {
        StepError(error.to_string())
    }
}

pub(crate) type StepResult = Result<(), StepError>;

const STEPS: [u32; 5] = [14, 15, 16, 17, 18];

/// Runs the upgrade from `from` (the probe's version) to 18 in one
/// transaction; any failure rolls back and fails with `UpgradeFailed`.
pub(crate) fn upgrade(conn: &Connection, from: i64, context: &UpgradeContext<'_>) -> Result<(), OpenError> {
    let failed = |stage: UpgradeStage, detail: String| OpenError::UpgradeFailed {
        version: from,
        stage,
        detail,
    };
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| failed(UpgradeStage::Step(first_step(from)), e.to_string()))?;
    match run_steps(conn, from, context) {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| {
            let _ = conn.execute_batch("ROLLBACK");
            failed(UpgradeStage::Step(18), e.to_string())
        }),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(match error {
                Failure::Step(step, StepError(detail)) => failed(UpgradeStage::Step(step), detail),
                Failure::Refused(error) => error,
            })
        }
    }
}

enum Failure {
    Step(u32, StepError),
    /// `col.ver` changed since the probe to a version C1 refuses.
    Refused(OpenError),
}

fn first_step(from: i64) -> u32 {
    STEPS.into_iter().find(|&s| i64::from(s) > from).unwrap_or(18)
}

fn run_steps(conn: &Connection, from: i64, context: &UpgradeContext<'_>) -> Result<(), Failure> {
    // C2.4 step 3: the version read again inside the transaction.
    let now: i64 = conn
        .query_row("SELECT ver FROM col LIMIT 1", [], |row| row.get(0))
        .map_err(|e| Failure::Step(first_step(from), e.into()))?;
    if now != from {
        return Err(match refuse::version(now) {
            Some(error) => Failure::Refused(error),
            None => Failure::Step(
                first_step(from),
                StepError(format!("col.ver changed from {from} to {now} during the open")),
            ),
        });
    }
    for step in STEPS {
        if i64::from(step) > from {
            (context.progress)(step);
            let result = match step {
                14 => to14::run(conn),
                15 => to15::run(conn, context),
                16 => to16::run(conn),
                17 => to17::run(conn),
                _ => to18::run(conn),
            };
            result
                .and_then(|()| set_version(conn, step))
                .map_err(|e| Failure::Step(step, e))?;
        }
        if step == 15 {
            // U3: after step 15's place in the sequence, before the commit.
            meguri_model::faultpoint!("storage.upgrade.in_transaction")
                .map_err(|e| Failure::Step(step, StepError(e.to_string())))?;
        }
    }
    Ok(())
}

/// Each step ends by setting `col.ver` to its number (U1).
fn set_version(conn: &Connection, step: u32) -> StepResult {
    conn.execute("UPDATE col SET ver = ?1", [step])?;
    Ok(())
}

/// The text of one `col` column (`conf`, `models`, ...), empty when NULL.
pub(super) fn col_text(conn: &Connection, column: &str) -> Result<String, StepError> {
    let bytes: Option<Vec<u8>> =
        conn.query_row(&format!("SELECT CAST({column} AS BLOB) FROM col LIMIT 1"), [], |row| {
            row.get(0)
        })?;
    String::from_utf8(bytes.unwrap_or_default()).map_err(|_| StepError(format!("col.{column} is not UTF-8")))
}

/// Sets one `col` JSON column to the empty string.
pub(super) fn clear_col(conn: &Connection, column: &str) -> StepResult {
    conn.execute(&format!("UPDATE col SET {column} = ''"), [])?;
    Ok(())
}

/// The foreign indexes and triggers of `table` (`storage/schema-18` S6), as
/// the statements stored for them, in the order they were created. Dropping
/// the table drops them, so a step that rebuilds it creates them again with
/// [`restore`] once the rows are back (U12: foreign objects are kept). Views
/// need nothing: a dropped table leaves the views that read it as they are.
pub(super) fn foreign_dependents(conn: &Connection, table: &str) -> Result<Vec<String>, StepError> {
    let mut select = conn.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_master
         WHERE type IN ('index', 'trigger') AND tbl_name = ?1 COLLATE NOCASE AND sql IS NOT NULL
         ORDER BY rowid",
    )?;
    let objects = select.query_map([table], |row| {
        Ok(SchemaObject {
            kind: row.get(0)?,
            name: row.get(1)?,
            tbl_name: row.get(2)?,
            sql: row.get(3)?,
        })
    })?;
    let mut statements = Vec::new();
    for object in objects {
        let object = object?;
        if !object.is_required() && !object.is_sqlite_own() {
            statements.extend(object.sql);
        }
    }
    Ok(statements)
}

/// Runs the statements [`foreign_dependents`] returned, unchanged. One that
/// fails fails the step (U10).
pub(super) fn restore(conn: &Connection, statements: &[String]) -> StepResult {
    for statement in statements {
        conn.execute_batch(statement)?;
    }
    Ok(())
}
