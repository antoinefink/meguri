// SPDX-License-Identifier: MIT
mod owned;
mod root;
mod semantic;
use crate::{Error, Result};
pub use root::{AccountStores, DataRoot};
use rusqlite::{Connection, OpenFlags};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub struct Metadata {
    root: Arc<DataRoot>,
    capacity: Arc<Semaphore>,
}
impl Metadata {
    pub fn new(root: Arc<DataRoot>) -> Self {
        Self {
            root,
            capacity: Arc::new(Semaphore::new(8)),
        }
    }
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::new("storage_unavailable"))?;
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            root.validate()?;
            let mut connection = open(root.database(), false)?;
            f(&mut connection)
        })
        .await
        .map_err(|_| Error::new("storage_unavailable"))?
    }
    pub async fn files<T: Send + 'static>(
        &self,
        f: impl FnOnce(&DataRoot) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::new("storage_unavailable"))?;
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            root.validate()?;
            f(&root)
        })
        .await
        .map_err(|_| Error::new("storage_unavailable"))?
    }
}
pub(super) fn inspect(path: PathBuf) -> Result<Connection> {
    root::regular(&path, true)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut auxiliary = path.as_os_str().to_os_string();
        auxiliary.push(suffix);
        root::regular(std::path::Path::new(&auxiliary), false)?;
    }
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?)
}
pub(crate) fn open(path: PathBuf, create: bool) -> Result<Connection> {
    root::regular(&path, !create)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut auxiliary = path.as_os_str().to_os_string();
        auxiliary.push(suffix);
        root::regular(std::path::Path::new(&auxiliary), false)?;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | if create {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
    let connection = Connection::open_with_flags(&path, flags)?;
    // SQLite's auxiliary files live in an owner-only validated directory.
    root::private_file(&path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "trusted_schema", false)?;
    let mode: String = connection.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |r| r.get(0))?;
    let fk: i64 = connection.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
    let busy: i64 = connection.pragma_query_value(None, "busy_timeout", |r| r.get(0))?;
    if mode != "wal" || synchronous != 2 || fk != 1 || busy != 5000 {
        return Err(Error::new("storage_unavailable"));
    }
    Ok(connection)
}

#[cfg(test)]
mod tests;
