use crate::{
    StorageError,
    connections::{READ_ONLY, file_uri},
};
use std::path::Path;

pub fn recorded_sizes(collection: &Path) -> Result<(u64, u64), StorageError> {
    use std::os::unix::fs::MetadataExt;
    let journal = collection.parent().unwrap_or(Path::new(".")).join("meguri-media.db");
    match journal.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.is_file() || m.nlink() != 1 => {
            return Err(StorageError::Io {
                detail: "invalid media journal binding".into(),
            });
        }
        Ok(_) => {}
    }
    let db = rusqlite::Connection::open_with_flags(file_uri(&journal, Some("mode=ro")), READ_ONLY)?;
    // Add in Rust: SQLite SUM is signed and can overflow before u64 does.
    fn sum(db: &rusqlite::Connection, sql: &str) -> Result<u64, StorageError> {
        let mut statement = db.prepare(sql)?;
        let mut rows = statement.query([])?;
        let mut total = 0u64;
        while let Some(row) = rows.next()? {
            let size = super::journal_db::unsigned(row, 0)?;
            total = total.checked_add(size).ok_or_else(|| StorageError::Io {
                detail: "media size overflow".into(),
            })?;
        }
        Ok(total)
    }
    db.execute_batch("BEGIN")?;
    Ok((
        sum(&db, "SELECT size FROM media WHERE live=1")?,
        sum(&db, "SELECT size FROM trash_entries WHERE path IS NOT NULL")?,
    ))
}
