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
        let backups = resolve_missing_directory(backups)?;
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

// Resolve existing symlinks before comparing roots, without creating backup
// directories ahead of the package writer's capacity check.
fn resolve_missing_directory(path: &Path) -> Result<PathBuf, StorageError> {
    let mut ancestor = path;
    let mut tail = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(mut resolved) => {
                for component in tail.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A dangling symlink is an occupied entry, never a missing parent.
                match ancestor.symlink_metadata() {
                    Ok(_) => return Err(e.into()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                tail.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| StorageError::Io {
                            detail: "invalid backup directory".into(),
                        })?
                        .to_owned(),
                );
                ancestor = ancestor
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
            }
            Err(e) => return Err(e.into()),
        }
    }
}
