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
    let mut statement = conn.prepare(sql)?;
    let columns = statement.column_count();
    if !statement.readonly() || !(1..=2).contains(&columns) {
        return Err(StorageError::Mode {
            detail: "a search must be a read-only id query of one or two columns".into(),
        });
    }
    let mut rows = statement.query(params_from_iter(params))?;
    let mut values = [0i64; 2];
    while let Some(row) = rows.next()? {
        for (index, value) in values.iter_mut().enumerate().take(columns) {
            *value = row.get::<_, i64>(index)?;
        }
        if !visit(&values[..columns]) {
            break;
        }
    }
    Ok(())
}
