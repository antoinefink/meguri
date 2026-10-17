//! Sweeps only reserved unpublished filenames, without following symlinks (F4).
use crate::StorageError;
use std::path::{Path, PathBuf};

pub fn cleanup(directory: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
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

/// The only namespace launch cleanup recursively sweeps. U124a and import
/// consumers keep all unpublished package entries inside this directory.
pub fn staging_directory(directory: &Path) -> Result<tempfile::TempDir, StorageError> {
    Ok(tempfile::Builder::new()
        .prefix(".meguri-stage-")
        .tempdir_in(directory)?)
}
