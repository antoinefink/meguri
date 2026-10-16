//! Read snapshots on a reader (`storage/open-and-connections` C7): one read
//! transaction that sees the collection as of its first read, whatever the
//! writer commits meanwhile, until it is dropped. The engine plans heavy ops
//! on them (`engine/op-model` O6). A snapshot holds the `-wal` from being
//! checkpointed past it, so it lives only as long as the work that needs it.

use rusqlite::Connection;

use crate::connections::Reader;
use crate::error::StorageError;

/// An open read transaction on a reader. Dropping it ends the transaction.
#[derive(Debug)]
pub struct ReadSnapshot<'r> {
    reader: &'r Reader,
}

impl Reader {
    /// Begins a read transaction and makes its first read, which fixes the
    /// view. Fails with [`StorageError::TransactionOpen`] when the reader is
    /// already inside one.
    pub fn begin_snapshot(&self) -> Result<ReadSnapshot<'_>, StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }
        self.conn.execute_batch("BEGIN DEFERRED")?;
        let pinned = self
            .conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0));
        if let Err(error) = pinned {
            let _ = self.conn.execute_batch("ROLLBACK");
            return Err(error.into());
        }
        Ok(ReadSnapshot { reader: self })
    }
}

impl ReadSnapshot<'_> {
    pub(crate) fn conn(&self) -> &Connection {
        &self.reader.conn
    }

    pub fn reader(&self) -> &Reader {
        self.reader
    }
}

impl Drop for ReadSnapshot<'_> {
    fn drop(&mut self) {
        // A read transaction writes nothing; ending it releases the view.
        let _ = self.reader.conn.execute_batch("ROLLBACK");
    }
}
