//! Lifetime advisory ownership, retained across SQLite closes (F1).
use crate::OpenError;
use std::fs::{File, OpenOptions};
use std::path::Path;

#[derive(Debug)]
pub struct CollectionLock {
    _file: File,
}
impl CollectionLock {
    pub fn acquire(directory: &Path) -> Result<Self, OpenError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("meguri.lock"))
            .map_err(|e| OpenError::Io { detail: e.to_string() })?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(OpenError::OpenInAnotherProgram),
            Err(std::fs::TryLockError::Error(e)) => Err(OpenError::Io { detail: e.to_string() }),
        }
    }
}
