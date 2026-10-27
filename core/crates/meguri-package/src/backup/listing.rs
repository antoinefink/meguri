//! Passive, no-follow directory listing (B7).
use super::naming::{BackupKind, WallTime, parse_name};
use std::{fs, io, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupEntry {
    pub name: String,
    pub kind: BackupKind,
    pub wall_time: WallTime,
    pub size_bytes: u64,
}

pub fn list(directory: &Path) -> io::Result<Vec<BackupEntry>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((kind, wall_time)) = parse_name(&name) else {
            continue;
        };
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if metadata.file_type().is_file() {
            out.push(BackupEntry {
                name,
                kind,
                wall_time,
                size_bytes: metadata.len(),
            });
        }
    }
    out.sort_unstable_by(|a, b| b.wall_time.cmp(&a.wall_time).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}
