//! SQL-only, between-operation maintenance (optimize-integrity M1/M8).
use crate::{Reader, StorageError, Writer};
use rusqlite::Connection;

// The callback is installed and removed on the connection's owner thread. No
// interrupt handle escapes: a late cancel cannot interrupt the next operation.
struct Progress<'a>(&'a Connection);
impl Drop for Progress<'_> {
    fn drop(&mut self) {
        // SAFETY: the borrowed connection is live and no statement is running.
        unsafe { rusqlite::ffi::sqlite3_progress_handler(self.0.handle(), 0, None, std::ptr::null_mut()) };
    }
}
unsafe extern "C" fn cancelled(context: *mut std::ffi::c_void) -> std::ffi::c_int {
    // SAFETY: with_cancel retains this borrowed callback until Progress drops.
    let callback = unsafe { &*context.cast::<&dyn Fn() -> bool>() };
    i32::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if meguri_model::faultpoint!("storage.maintenance.progress").is_err() {
                return true;
            }
            callback()
        }))
        .unwrap_or(true),
    )
}
pub(crate) fn with_cancel<T>(
    conn: &Connection,
    cancel: &dyn Fn() -> bool,
    work: impl FnOnce() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    let callback = cancel;
    // SAFETY: SQLite invokes synchronously on this thread. The stack reference
    // outlives the registration, including unwinding; Progress always removes it.
    unsafe {
        rusqlite::ffi::sqlite3_progress_handler(
            conn.handle(),
            1_000,
            Some(cancelled),
            std::ptr::from_ref(&callback).cast_mut().cast(),
        );
    }
    let _progress = Progress(conn);
    if cancel() {
        return Err(StorageError::Sqlite {
            code: 9,
            extended_code: 9,
            message: "maintenance cancelled".into(),
        });
    }
    work()
}
struct TempStore<'a>(&'a Connection);
impl Drop for TempStore<'_> {
    fn drop(&mut self) {
        // Also restore on a panic. Normal paths propagate restoration failure.
        let _ = self.0.execute_batch("PRAGMA temp_store=MEMORY");
    }
}
impl Writer {
    pub fn vacuum(&self, cancel: &dyn Fn() -> bool) -> Result<(), StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }
        self.conn.execute_batch("PRAGMA temp_store=FILE")?;
        let restore = TempStore(&self.conn);
        let result = with_cancel(&self.conn, cancel, || {
            meguri_model::faultpoint!("storage.maintenance.vacuum")?;
            self.conn.execute_batch("VACUUM").map_err(Into::into)
        });
        // Remove the progress callback before restoring the connection setting.
        self.conn.execute_batch("PRAGMA temp_store=MEMORY")?;
        drop(restore);
        result
    }
    pub fn analyze(&self, cancel: &dyn Fn() -> bool) -> Result<(), StorageError> {
        self.maintenance_statement("ANALYZE", cancel)
    }
    /// Optional U128 building block; automatic optimize never calls it.
    pub fn reindex(&self, cancel: &dyn Fn() -> bool) -> Result<(), StorageError> {
        self.maintenance_statement("REINDEX", cancel)
    }
    fn maintenance_statement(&self, sql: &str, cancel: &dyn Fn() -> bool) -> Result<(), StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }
        with_cancel(&self.conn, cancel, || self.conn.execute_batch(sql).map_err(Into::into))
    }
}
impl Reader {
    pub fn quick_check_cancellable(
        &self,
        cancel: &dyn Fn() -> bool,
    ) -> Result<crate::integrity::IntegrityReport, StorageError> {
        with_cancel(&self.conn, cancel, || {
            meguri_model::faultpoint!("storage.maintenance.quick_check")?;
            self.quick_check()
        })
    }
}
