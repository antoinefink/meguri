use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{BuildScope, Error, TrackingStore, store};

pub const TRACKING_FILE: &str = "meguri-cloud.db";
pub(super) const SCHEMA_VERSION: i64 = 2;

/// Plain open refuses damage; explicit recovery must finish before use.
/// Missing creates an empty store, without inferring any server facts.
pub enum OpenOutcome {
    Missing(TrackingStore),
    Opened(TrackingStore),
    Damaged,
    NewerSchema { found: i64 },
}

pub(super) enum Probe {
    Supported,
    Damaged,
    Newer(i64),
}

pub(super) fn open(directory: &Path, scope: BuildScope) -> Result<OpenOutcome, Error> {
    super::recovery::require_settled(directory)?;
    open_current(directory, scope)
}

pub(super) fn open_current(directory: &Path, scope: BuildScope) -> Result<OpenOutcome, Error> {
    let path = directory.join(TRACKING_FILE);
    let exists = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => true,
        Ok(_) => return Ok(OpenOutcome::Damaged),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if exists {
        match probe(&path)? {
            Probe::Damaged => return Ok(OpenOutcome::Damaged),
            Probe::Newer(found) => return Ok(OpenOutcome::NewerSchema { found }),
            Probe::Supported => {}
        }
    }
    // Only the private copy has been opened by SQLite until the version and
    // required rows have been checked. Opening even read-only at the original
    // path could change its sidecars or attempt WAL recovery.
    let flags = if exists {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let mut connection = Connection::open_with_flags(&path, flags)?;
    // Server facts must survive the device cache before dependent work (K10.1, V3).
    #[cfg(target_vendor = "apple")]
    connection.execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    if !exists {
        let transaction = connection.transaction()?;
        transaction.execute_batch(store::V1_SCHEMA)?;
        transaction.execute_batch(store::V2_SCHEMA)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    if exists {
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 1 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(store::V2_SCHEMA)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
    }
    let store = TrackingStore {
        connection,
        scope,
        directory: directory.to_owned(),
    };
    Ok(if exists {
        OpenOutcome::Opened(store)
    } else {
        OpenOutcome::Missing(store)
    })
}

pub(super) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub(super) fn probe(path: &Path) -> Result<Probe, Error> {
    let private = tempfile::tempdir()?;
    let copy = private.path().join(TRACKING_FILE);
    std::fs::copy(path, &copy)?;
    match std::fs::copy(sidecar(path, "-wal"), sidecar(&copy, "-wal")) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // Do not copy SHM: SQLite reconstructs the private WAL index. It never
    // shares the original's locks, nor recovers/checkpoints the original WAL.
    let checked = (|| -> Result<Probe, Error> {
        let connection = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Ok(Probe::Newer(version));
        }
        if !(1..=SCHEMA_VERSION).contains(&version) {
            return Ok(Probe::Damaged);
        }
        let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Ok(Probe::Damaged);
        }
        store::validate(&connection, version)?;
        Ok(Probe::Supported)
    })();
    match checked {
        Ok(result) => Ok(result),
        Err(_) => Ok(Probe::Damaged),
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildScope, OpenOutcome, TrackingStore};
    use crate::cloud::{ContainerId, Environment};

    fn scope() -> BuildScope {
        BuildScope {
            container: ContainerId::new("test.container").unwrap(),
            environment: Environment::Development,
        }
    }

    fn assert_durable_settings(store: &TrackingStore) {
        // These settings belong to this connection; a separate SQL connection
        // would not establish the durability of the production store.
        let connection = &store.connection;
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 2, "tracking commits require synchronous=FULL");
        #[cfg(target_vendor = "apple")]
        for pragma in ["fullfsync", "checkpoint_fullfsync"] {
            let enabled: i64 = connection.pragma_query_value(None, pragma, |row| row.get(0)).unwrap();
            assert_eq!(enabled, 1, "{pragma} must flush the device cache");
        }
    }

    #[test]
    fn newly_created_tracking_connection_uses_durable_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("expected missing tracking file"),
        };
        assert_durable_settings(&store);
    }

    #[test]
    fn reopened_tracking_connection_uses_durable_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("expected missing tracking file"),
        };
        drop(store);
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Opened(store) => store,
            _ => panic!("expected supported tracking file"),
        };
        assert_durable_settings(&store);
    }
}
