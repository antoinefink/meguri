//! The private collection directory and separate host-supplied backups (F1).
use crate::StorageError;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct CollectionPaths {
    pub directory: PathBuf,
    pub backups: PathBuf,
}
impl CollectionPaths {
    pub fn new(directory: &Path, backups: &Path) -> Result<Self, StorageError> {
        let directory = directory.canonicalize()?;
        let backups = backups.canonicalize()?;
        if directory.starts_with(&backups) || backups.starts_with(&directory) {
            return Err(StorageError::Io {
                detail: "backups and collection require separate directories".into(),
            });
        }
        Ok(Self { directory, backups })
    }
    pub fn collection(&self) -> PathBuf {
        self.directory.join("collection.anki2")
    }
    pub fn media(&self) -> PathBuf {
        self.directory.join("collection.media")
    }
    pub fn media_journal(&self) -> PathBuf {
        self.directory.join("collection.media.db")
    }
    pub fn derived(&self) -> PathBuf {
        self.directory.join("meguri-derived.db")
    }
    pub fn sidecar(&self) -> PathBuf {
        self.directory.join("meguri-sidecar.json")
    }
}
