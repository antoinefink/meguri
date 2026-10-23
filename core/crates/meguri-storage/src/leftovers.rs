//! Sweeps only reserved unpublished filenames, without following symlinks (F4).
use crate::StorageError;
use std::path::{Path, PathBuf};

pub fn cleanup(directory: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() && entry.file_name() == "media.staging" {
            // Startup cleanup runs with collection advisory ownership. Media
            // recovery can overlap downloads, so it leaves active copy directories
            // untouched.
            for copy in std::fs::read_dir(entry.path())? {
                let copy = copy?;
                if copy.file_type()?.is_dir()
                    && copy.file_name().to_str().is_some_and(|name| {
                        [".meguri-stage-", ".meguri-tmp-media-"]
                            .iter()
                            .any(|prefix| name.strip_prefix(*prefix).is_some_and(|suffix| !suffix.is_empty()))
                    })
                {
                    std::fs::remove_dir_all(copy.path())?;
                    removed.push(copy.path());
                }
            }
            continue;
        }
        if kind.is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.strip_prefix(".meguri-stage-").is_some_and(|s| !s.is_empty()))
        {
            // remove_dir_all never follows symlinks, including links inside
            // this reserved directory. Never recurse into arbitrary folders.
            std::fs::remove_dir_all(entry.path())?;
            removed.push(entry.path());
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if reserved(name) {
            std::fs::remove_file(entry.path())?;
            removed.push(entry.path());
        }
    }
    removed.sort();
    Ok(removed)
}
fn reserved(name: &str) -> bool {
    ["replace", "stage", "export", "backup", "media", "sidecar"]
        .iter()
        .any(|purpose| {
            name.strip_prefix(&format!(".meguri-tmp-{purpose}-"))
                .is_some_and(|s| !s.is_empty())
        })
        || (name.starts_with("meguri-sync-stage-") && name.ends_with(".tmp"))
        || (name.starts_with("meguri-export-") && name.ends_with(".partial"))
        || (name.starts_with("meguri-backup-") && name.ends_with(".partial"))
}

/// Launch cleanup sweeps this namespace at the collection root and inside
/// media.staging. Consumers keep only unpublished, independently owned bytes.
pub fn staging_directory(directory: &Path) -> Result<tempfile::TempDir, StorageError> {
    Ok(tempfile::Builder::new()
        .prefix(".meguri-stage-")
        .tempdir_in(directory)?)
}

/// Media contains published user files. Sweep only reserved regular files
/// at its top level, and never follow a link used as the media directory.
pub fn cleanup_media(directory: &Path) -> Result<Vec<PathBuf>, StorageError> {
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.file_name().to_str().is_some_and(reserved) {
            std::fs::remove_file(entry.path())?;
            removed.push(entry.path());
        }
    }
    removed.sort();
    Ok(removed)
}
