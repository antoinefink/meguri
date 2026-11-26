//! Read-only M5 case fallback for file export. Never opens/initializes a
//! MediaStore, registers files, or changes the journal.
use crate::StorageError;
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn tracked_export_variants(
    directory: &Path,
    candidates: &BTreeSet<String>,
    check: &(dyn Fn() -> bool + Sync),
) -> Result<BTreeMap<String, Vec<String>>, StorageError> {
    let path = directory.join("meguri-media.db");
    match path.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.is_file() => {
            return Err(StorageError::Io {
                detail: "media journal is not a regular collection-owned file".into(),
            });
        }
        _ => {}
    }
    // NOFOLLOW also closes the check/open race for this collection-owned DB.
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let mut out = BTreeMap::new();
    let mut statement = db.prepare("SELECT name FROM media WHERE live=1 AND case_key=?1 ORDER BY name")?;
    for name in candidates {
        if check() {
            return Err(StorageError::Io {
                detail: "export cancelled".into(),
            });
        }
        let rows = statement.query_map([super::case_fold::case_key(name)], |r| r.get::<_, String>(0))?;
        let variants: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
        if !variants.is_empty() {
            out.insert(name.clone(), variants);
        }
    }
    Ok(out)
}
