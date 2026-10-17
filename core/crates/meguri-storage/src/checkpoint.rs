//! The checkpoint primitive (`storage/open-and-connections` C7, R1454).

use crate::connections::Writer;
use crate::error::StorageError;

/// SQLite's answer to `PRAGMA wal_checkpoint(TRUNCATE)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointOutcome {
    /// A reader still held an older snapshot: the checkpoint could not
    /// finish.
    pub busy: bool,
    /// Frames in the log.
    pub log_frames: i64,
    /// Frames moved into the main file.
    pub checkpointed_frames: i64,
}

impl CheckpointOutcome {
    /// The main file holds every committed transaction and the `-wal` file
    /// is empty.
    pub fn succeeded(&self) -> bool {
        !self.busy
    }
}

impl Writer {
    /// Bound SQLite's reader-lock wait by the remaining suspend budget.
    pub fn checkpoint_with_timeout(&self, timeout: std::time::Duration) -> Result<CheckpointOutcome, StorageError> {
        let previous: u32 = self.conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0))?;
        self.conn.busy_timeout(timeout)?;
        let result = self.checkpoint();
        self.conn
            .busy_timeout(std::time::Duration::from_millis(u64::from(previous)))?;
        result
    }

    /// Runs `PRAGMA wal_checkpoint(TRUNCATE)` once, outside any transaction
    /// (inside one it fails with [`StorageError::TransactionOpen`] and does
    /// nothing). It does not retry when busy; the caller decides (U32). A
    /// caller that copies or sends the collection file itself copies only
    /// after a successful checkpoint.
    pub fn checkpoint(&self) -> Result<CheckpointOutcome, StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) =
            self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        Ok(CheckpointOutcome {
            busy: busy != 0,
            log_frames,
            checkpointed_frames,
        })
    }
}
