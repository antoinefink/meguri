//! Live collection snapshot for package/container P9–P11, RF2.3 and one-way O6.2.
//! U32 owns the staging directory, its namespace and failure cleanup.

use std::path::{Path, PathBuf};

use crate::{
    Reader, ReaderRole, StorageError,
    collection_package::finalise_snapshot,
    connections::{file_uri, sibling},
};

impl Reader {
    /// P10 metadata for a live package writer: the connected main database's
    /// path and its current database + WAL size. No read transaction is opened.
    /// The caller holds the engine lease and preflights before staging writes.
    pub fn live_snapshot_source(&self) -> Result<(PathBuf, u64), StorageError> {
        if self.role() != ReaderRole::Maintenance {
            return Err(StorageError::Mode {
                detail: "live package snapshots require the Maintenance reader".into(),
            });
        }
        self.snapshot_source()
    }

    fn snapshot_source(&self) -> Result<(PathBuf, u64), StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }
        let source = self
            .conn
            .path()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| StorageError::Io {
                detail: "live collection has no database path".into(),
            })?;
        let source = PathBuf::from(source);
        let database_bytes = std::fs::metadata(&source)?.len();
        let wal_bytes = match std::fs::metadata(sibling(&source, "-wal")) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let size = database_bytes.checked_add(wal_bytes).ok_or_else(|| StorageError::Io {
            detail: "live collection size overflow".into(),
        })?;
        Ok((source, size))
    }

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
        let destination = staging_directory.join("snapshot.anki2");
        self.snapshot_live_to(&destination)?;
        Ok(destination)
    }

    /// O6.2 capture uses only the dedicated Sync reader after the caller's
    /// drain, outside a snapshot and under its engine lease. The package
    /// export API above remains Maintenance-only.
    pub(crate) fn snapshot_full_upload_to(
        &self,
        stage: &crate::verified_replace::ReplacementStage,
    ) -> Result<PathBuf, crate::verified_replace::ReplaceError> {
        if self.role() != ReaderRole::Sync {
            return Err(StorageError::Mode {
                detail: "full-upload snapshots require the Sync reader".into(),
            }
            .into());
        }
        let (source, _) = self.snapshot_source()?;
        crate::verified_replace::check_directory(stage, &source)?;
        self.snapshot_live_to(&stage.path())?;
        Ok(source)
    }

    // Both role-specific entries validate their role before reaching this
    // shared capture/finalisation machinery.
    fn snapshot_live_to(&self, destination: &Path) -> Result<(), StorageError> {
        if !self.conn.is_autocommit() {
            return Err(StorageError::TransactionOpen);
        }

        // Reject adjacent state files before SQLite opens the output.
        for suffix in ["-wal", "-shm", "-journal"] {
            match sibling(destination, suffix).symlink_metadata() {
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
            .open(destination)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    StorageError::DestinationExists
                } else {
                    error.into()
                }
            })?;
        drop(claim);
        self.conn
            .execute("VACUUM main INTO ?1", [file_uri(destination, Some("mode=rw"))])?;
        finalise_snapshot(destination)
    }
}
