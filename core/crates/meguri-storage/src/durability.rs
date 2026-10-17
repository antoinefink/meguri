//! The writer's WAL flush (`storage/open-and-connections` C9.2).

use std::ptr;

use rusqlite::ffi;

use crate::{StorageError, Writer};

impl Writer {
    /// External commits only, unlike SQLITE_FCNTL_DATA_VERSION.
    pub fn external_data_version(&self) -> Result<i64, StorageError> {
        self.conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .map_err(Into::into)
    }
    pub fn is_autocommit(&self) -> bool {
        self.conn.is_autocommit()
    }

    /// The main database's change token, including this connection's own
    /// commits (unlike `PRAGMA data_version`). The executor uses it to notice
    /// writes by ops and maintenance without replacing SQLite's WAL hook or
    /// disabling its passive autocheckpoint.
    pub fn data_version(&self) -> Result<u32, StorageError> {
        let mut version = 0_u32;
        // SAFETY: the live connection is owned by the calling thread. SQLite
        // writes a u32 to the aligned, valid out-pointer and retains nothing.
        let code = unsafe {
            ffi::sqlite3_file_control(
                self.conn.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_DATA_VERSION,
                ptr::from_mut(&mut version).cast(),
            )
        };
        check(code)?;
        Ok(version)
    }

    /// Full-syncs the WAL itself, never a checkpoint. Call on the writer
    /// thread between ops, outside their timed write phases. Returns false
    /// if no WAL is open (there are no WAL frames to flush).
    ///
    /// SQLite's VFS handles Apple's full sync and the first directory sync.
    /// An error leaves coverage bookkeeping to the executor, which retries.
    pub fn sync_wal(&self) -> Result<bool, StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }
        #[cfg(feature = "test-hooks")]
        if self.fail_next_wal_sync.replace(false) {
            return Err(StorageError::Io {
                detail: "injected WAL sync failure".to_owned(),
            });
        }
        let mut file: *mut ffi::sqlite3_file = ptr::null_mut();
        // SAFETY: this live NO_MUTEX connection is used only by its owner.
        // JOURNAL_POINTER writes a borrowed sqlite3_file pointer; it remains
        // valid until the next SQLite call. No statement, transaction, close
        // or other connection access intervenes before xSync returns. Check
        // both pointers and the method before calling; retain none of them.
        unsafe {
            check(ffi::sqlite3_file_control(
                self.conn.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_JOURNAL_POINTER,
                ptr::from_mut(&mut file).cast(),
            ))?;
            // No WAL open yet: SQLite hands back the unopened rollback
            // journal handle, whose methods are null.
            let Some(methods) = file.as_ref().and_then(|file| file.pMethods.as_ref()) else {
                return Ok(false);
            };
            let sync = methods.xSync.ok_or_else(|| StorageError::Io {
                detail: "the WAL VFS has no xSync method".to_owned(),
            })?;
            check(sync(file, ffi::SQLITE_SYNC_FULL))?;
        }
        Ok(true)
    }

    /// Fail one flush before calling the VFS, to exercise retry coverage.
    #[cfg(feature = "test-hooks")]
    pub fn fail_next_wal_sync_for_test(&self) {
        self.fail_next_wal_sync.set(true);
    }
}

fn check(code: i32) -> Result<(), StorageError> {
    if code == ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(rusqlite::Error::SqliteFailure(ffi::Error::new(code), None).into())
    }
}
