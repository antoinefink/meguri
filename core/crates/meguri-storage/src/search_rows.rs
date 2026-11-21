//! Id statements of Browse searches (`browser/result-sets` H3): one
//! read-only statement whose integer columns are visited row by row, under a
//! progress handler that stops it with SQLite's interrupt once the caller's
//! cancel test says so (a superseded request, H3). The handler is installed
//! and removed on the connection's thread around one statement only, so a
//! late cancel never interrupts the next statement.

use rusqlite::{Connection, params_from_iter};

use crate::error::StorageError;
use crate::rows::Cell;

/// SQLite virtual machine steps between two cancel tests.
const STEPS: std::ffi::c_int = 1_000;

struct Handler<'a>(&'a Connection);

impl Drop for Handler<'_> {
    fn drop(&mut self) {
        // SAFETY: the connection is live and the statement has been reset or
        // dropped before this guard (declared before the statement).
        unsafe { rusqlite::ffi::sqlite3_progress_handler(self.0.handle(), 0, None, std::ptr::null_mut()) };
    }
}

unsafe extern "C" fn stop(context: *mut std::ffi::c_void) -> std::ffi::c_int {
    // SAFETY: `visit` keeps the borrowed test alive until the handler is
    // removed.
    let cancel = unsafe { &*context.cast::<&dyn Fn() -> bool>() };
    i32::from(std::panic::catch_unwind(std::panic::AssertUnwindSafe(cancel)).unwrap_or(true))
}

fn interrupted() -> StorageError {
    StorageError::Sqlite {
        code: 9,
        extended_code: 9,
        message: "the search was cancelled".into(),
    }
}

pub(crate) fn visit(
    conn: &Connection,
    sql: &str,
    params: &[Cell],
    cancel: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&[i64]) -> bool,
) -> Result<(), StorageError> {
    visit_integers(conn, sql, params, 2, cancel, visit)
}

/// The most integer columns one count statement may select
/// ([`visit_counts`]).
pub const COUNT_COLUMNS: usize = 1000;

/// A read-only statement of Browse's filter counts (`search/filter-tokens`
/// T12.4) whose columns, at most [`COUNT_COLUMNS`], are integers: grouped
/// keys and their counts, or one count per value. Cancelled as [`visit`] is.
pub(crate) fn visit_counts(
    conn: &Connection,
    sql: &str,
    params: &[Cell],
    cancel: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&[i64]) -> bool,
) -> Result<(), StorageError> {
    visit_integers(conn, sql, params, COUNT_COLUMNS, cancel, visit)
}

/// Every note's id, `mid` (none when not an integer) and `tags` as text
/// (`search/filter-tokens` T12.4's tag and note type counts): TEXT read as
/// `meguri_regex` reads it (invalid UTF-8 replaced), INTEGER and REAL as
/// SQLite's text for them, NULL and BLOB as none, which no tag term matches.
/// Cancelled as [`visit`] is.
pub(crate) fn visit_note_tags(
    conn: &Connection,
    cancel: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(i64, Option<i64>, Option<&str>) -> bool,
) -> Result<(), StorageError> {
    with_cancel(conn, cancel, || {
        let mut statement = conn.prepare(
            "SELECT id, mid, CASE WHEN typeof(tags) IN ('integer', 'real') THEN CAST(tags AS TEXT) ELSE tags END FROM notes",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id = row.get::<_, i64>(0)?;
            let mid = match row.get_ref(1)? {
                rusqlite::types::ValueRef::Integer(mid) => Some(mid),
                _ => None,
            };
            let keep = match row.get_ref(2)? {
                rusqlite::types::ValueRef::Text(bytes) => visit(id, mid, Some(&String::from_utf8_lossy(bytes))),
                _ => visit(id, mid, None),
            };
            if !keep {
                break;
            }
        }
        Ok(())
    })
}

/// Every deck's id and `name` as text, as the deck terms' `meguri_regex`
/// reads it (`search/filter-tokens` T12.4's deck counts; see
/// [`visit_note_tags`] for the text).
pub(crate) fn deck_names(conn: &Connection) -> Result<Vec<(i64, Option<String>)>, StorageError> {
    names(conn, "decks")
}

/// Every note type's id and `name` as text (T12.4's note type counts; the
/// `unicase` comparison reads text only).
pub(crate) fn notetype_names(conn: &Connection) -> Result<Vec<(i64, Option<String>)>, StorageError> {
    names(conn, "notetypes")
}

fn names(conn: &Connection, table: &str) -> Result<Vec<(i64, Option<String>)>, StorageError> {
    let mut statement = conn.prepare(&format!(
        "SELECT id, CASE WHEN typeof(name) IN ('integer', 'real') THEN CAST(name AS TEXT) ELSE name END FROM {table}"
    ))?;
    let mut rows = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let name = match row.get_ref(1)? {
            rusqlite::types::ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        };
        out.push((row.get::<_, i64>(0)?, name));
    }
    Ok(out)
}

/// Runs `work` under the progress handler that stops SQLite once `cancel`
/// says so; a cancel seen before it starts is the interrupt too.
fn with_cancel<T>(
    conn: &Connection,
    cancel: &dyn Fn() -> bool,
    work: impl FnOnce() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    if cancel() {
        return Err(interrupted());
    }
    let test = cancel;
    // SAFETY: SQLite calls the handler synchronously on this thread; `test`
    // outlives the registration, which `_handler` removes on every path.
    unsafe {
        rusqlite::ffi::sqlite3_progress_handler(
            conn.handle(),
            STEPS,
            Some(stop),
            std::ptr::from_ref(&test).cast_mut().cast(),
        );
    }
    let _handler = Handler(conn);
    work()
}

fn visit_integers(
    conn: &Connection,
    sql: &str,
    params: &[Cell],
    most: usize,
    cancel: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&[i64]) -> bool,
) -> Result<(), StorageError> {
    with_cancel(conn, cancel, || {
        let mut statement = conn.prepare(sql)?;
        let columns = statement.column_count();
        if !statement.readonly() || !(1..=most).contains(&columns) {
            return Err(StorageError::Mode {
                detail: format!("a search must be a read-only integer query of one to {most} columns"),
            });
        }
        let mut rows = statement.query(params_from_iter(params))?;
        let mut values = vec![0i64; columns];
        while let Some(row) = rows.next()? {
            for (index, value) in values.iter_mut().enumerate() {
                *value = row.get::<_, i64>(index)?;
            }
            if !visit(&values) {
                break;
            }
        }
        Ok(())
    })
}
