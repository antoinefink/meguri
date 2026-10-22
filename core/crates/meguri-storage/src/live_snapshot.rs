//! Live collection snapshot for package/container P9–P11 and RF2.3.
//! U32 owns the staging directory, its namespace and failure cleanup.

use std::path::{Path, PathBuf};

use crate::{
    Reader, ReaderRole, StorageError,
    collection_package::finalise_snapshot,
    connections::{file_uri, sibling},
};

impl Reader {
    /// Snapshot the live collection into `snapshot.anki2` in the caller's
    /// fresh U32 staging directory. The caller has preflighted P10, holds the
    /// appropriate engine lease and keeps this directory private and stable
    /// throughout the call. It releases that lease before replacement.
    ///
    /// Requires the shared Maintenance reader outside a read transaction.
    /// VACUUM establishes its own consistent read view, including committed
    /// WAL rows, without closing or checkpointing the source or its writer.
    /// The result uses P11's WAL format without adjacent SQLite state files.
    ///
    /// Existing destination entries (even empty files or dangling symlinks)
    /// or sidecar files cause refusal. Failed output stays
    /// unpublished inside the staging directory; the caller drops its U32
    /// handle to clean it up. This method never removes a path.
    pub fn snapshot_live(&self, staging_directory: &Path) -> Result<PathBuf, StorageError> {
        if self.role() != ReaderRole::Maintenance {
            return Err(StorageError::Mode {
                detail: "live package snapshots require the Maintenance reader".into(),
            });
        }
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }

        let destination = staging_directory.join("snapshot.anki2");
        // Reject adjacent state files before SQLite opens the output.
        for suffix in ["-wal", "-shm", "-journal"] {
            match sibling(&destination, suffix).symlink_metadata() {
                Ok(_) => return Err(StorageError::DestinationExists),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        // VACUUM accepts an empty output. Claim it exclusively first because
        // SQLite itself would also accept an existing empty user file.
        let claim = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    StorageError::DestinationExists
                } else {
                    error.into()
                }
            })?;
        drop(claim);
        self.conn
            .execute("VACUUM main INTO ?1", [file_uri(&destination, Some("mode=rw"))])?;
        finalise_snapshot(&destination)?;
        Ok(destination)
    }
}
