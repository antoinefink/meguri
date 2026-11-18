//! Atomic creation of the schema-18 file (storage/new-collection B1-B2).
//! The engine supplies its initial records; SQL and file publication stay
//! here. The destination is never a partial collection and never replaced.
use crate::{Record, StampMode, StorageError, Table, WriteScope};
use meguri_model::{clock::Clock, col::Col, config::ConfigEntry};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

fn io(error: std::io::Error) -> StorageError {
    StorageError::Io {
        detail: format!("creating collection: {error}"),
    }
}

pub fn create<E: From<StorageError>>(
    path: &Path,
    col: &Col,
    config: &[ConfigEntry],
    clock: &dyn Clock,
    fill: impl FnOnce(&mut WriteScope<'_>) -> Result<(), E>,
) -> Result<(), E> {
    let exists = || StorageError::BuilderRefused {
        reason: crate::error::BuilderRefusal::Exists,
    };
    if ["", "-wal", "-shm", "-journal"]
        .iter()
        .any(|suffix| crate::connections::sibling(path, suffix).symlink_metadata().is_ok())
    {
        return Err(exists().into());
    }
    // Claim a private sibling with O_EXCL. No random or wall-clock source.
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut n = 0_u64;
    let (temp, claim) = loop {
        let candidate = dir.join(format!(".meguri-bootstrap-{}-{n}", std::process::id()));
        match crate::write_scope::Claim::take(&candidate) {
            Ok(claim) => break (candidate, claim),
            Err(StorageError::BuilderRefused { .. }) => n += 1,
            Err(error) => return Err(error.into()),
        }
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(&temp, flags)
        .map_err(StorageError::from)
        .map_err(E::from)?;
    crate::conn_init::probe(&conn).map_err(|detail| E::from(StorageError::ConnectionInit { detail }))?;
    conn.execute_batch("PRAGMA page_size = 4096; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;")
        .map_err(StorageError::from)
        .map_err(E::from)?;
    #[cfg(target_vendor = "apple")]
    conn.execute_batch("PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;")
        .map_err(StorageError::from)
        .map_err(E::from)?;
    let scope = WriteScope::begin(&conn, StampMode::CallerSupplied, clock, None).map_err(E::from)?;
    for statement in crate::schema::ddl18_statements() {
        conn.execute_batch(statement)
            .map_err(StorageError::from)
            .map_err(E::from)?;
    }
    crate::rows::insert(&conn, Table::Col, &col.to_cells()).map_err(E::from)?;
    for entry in config {
        crate::rows::insert(&conn, Table::Config, &entry.to_cells()).map_err(E::from)?;
    }
    conn.execute_batch("ANALYZE;")
        .map_err(StorageError::from)
        .map_err(E::from)?;
    let mut scope = scope;
    fill(&mut scope)?;
    meguri_model::faultpoint!("storage.bootstrap.in_transaction")
        .map_err(io)
        .map_err(E::from)?;
    scope.commit().map_err(E::from)?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(StorageError::from)
        .map_err(E::from)?;
    conn.close().map_err(|(_, e)| E::from(StorageError::from(e)))?;
    std::fs::File::open(&temp)
        .and_then(|f| f.sync_all())
        .map_err(io)
        .map_err(E::from)?;
    meguri_model::faultpoint!("storage.bootstrap.before_publish")
        .map_err(io)
        .map_err(E::from)?;
    // link is an atomic, no-clobber publication on the same filesystem.
    std::fs::hard_link(&temp, path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                exists()
            } else {
                io(e)
            }
        })
        .map_err(E::from)?;
    std::fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(io)
        .map_err(E::from)?;
    drop(claim); // remove staging file and its private sidecars
    Ok(())
}
