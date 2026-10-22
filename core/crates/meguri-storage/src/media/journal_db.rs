use super::{Result, case_fold::case_key};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;

pub(super) const COLUMNS: &str = "name,sha1,size,mtime_secs,mtime_nanos,live,dirty,change_counter,pending_download";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaEntry {
    pub name: String,
    pub sha1: Option<String>,
    pub size: u64,
    pub mtime_secs: i64,
    pub mtime_nanos: u32,
    pub live: bool,
    pub dirty: bool,
    pub change_counter: u64,
    pub pending_download: bool,
}
impl MediaEntry {
    pub fn syncable(&self) -> bool {
        self.live && (1..=104_857_600).contains(&self.size)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingChange {
    pub name: String,
    pub sha1: Option<String>,
    pub change_counter: u64,
}
pub(super) fn open(path: &Path) -> Result<Connection> {
    const APPLICATION_ID: i64 = 0x4d47524d;
    const VERSION: i64 = 1;
    if path.try_exists()? {
        // Refusals must not switch journal mode, recover a foreign database,
        // or add our tables to a file owned by another application.
        let probe = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let application_id: i64 = probe.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = probe.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if application_id != APPLICATION_ID || version != VERSION {
            return Err(super::MediaError::Database(
                "unsupported media journal format or version".into(),
            ));
        }
    }
    let db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    #[cfg(target_vendor = "apple")]
    db.execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        INSERT OR IGNORE INTO meta VALUES('counter','0'),('last_usn','0');
        CREATE TABLE IF NOT EXISTS media(
            name TEXT PRIMARY KEY,case_key TEXT NOT NULL,sha1 TEXT,size INTEGER NOT NULL,
            mtime_secs INTEGER NOT NULL,mtime_nanos INTEGER NOT NULL,live INTEGER NOT NULL,
            dirty INTEGER NOT NULL,change_counter INTEGER NOT NULL,pending_download INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS media_case ON media(case_key) WHERE live=1;
        CREATE INDEX IF NOT EXISTS media_dirty ON media(name) WHERE dirty=1;
        CREATE INDEX IF NOT EXISTS media_download ON media(name) WHERE pending_download=1;
        CREATE TABLE IF NOT EXISTS staged_ops(id INTEGER PRIMARY KEY AUTOINCREMENT,refs TEXT NOT NULL DEFAULT '[]');
        CREATE TABLE IF NOT EXISTS staged_files(op_id INTEGER NOT NULL,idx INTEGER NOT NULL,name TEXT NOT NULL,
            sha1 TEXT NOT NULL,size INTEGER NOT NULL,path TEXT NOT NULL,replace_existing INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(op_id,idx));
        CREATE INDEX IF NOT EXISTS staged_name ON staged_files(name);
        CREATE TABLE IF NOT EXISTS trash_entries(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,
            sha1 TEXT NOT NULL,size INTEGER NOT NULL,trashed_ms INTEGER NOT NULL,path TEXT UNIQUE);
        PRAGMA application_id=1296519757;
        PRAGMA user_version=1;
        COMMIT;",
    )?;
    Ok(db)
}
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaEntry> {
    Ok(MediaEntry {
        name: row.get(0)?,
        sha1: row.get(1)?,
        size: unsigned(row, 2)?,
        mtime_secs: row.get(3)?,
        mtime_nanos: row.get(4)?,
        live: row.get(5)?,
        dirty: row.get(6)?,
        change_counter: unsigned(row, 7)?,
        pending_download: row.get(8)?,
    })
}
pub(super) fn counter(db: &Connection) -> Result<u64> {
    db.query_row("SELECT value FROM meta WHERE key='counter'", [], |r| {
        r.get::<_, String>(0)
    })?
    .parse()
    .map_err(|_| super::MediaError::Database("invalid media change counter".into()))
}
pub(super) fn entry(db: &Connection, name: &str) -> Result<Option<MediaEntry>> {
    Ok(db
        .query_row(&format!("SELECT {COLUMNS} FROM media WHERE name=?1"), [name], row)
        .optional()?)
}
pub(super) fn variants(db: &Connection, name: &str) -> Result<Vec<MediaEntry>> {
    Ok(db
        .prepare(&format!(
            "SELECT {COLUMNS} FROM media WHERE live=1 AND case_key=?1 ORDER BY name"
        ))?
        .query_map([case_key(name)], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub(super) fn entries(db: &Connection) -> Result<Vec<MediaEntry>> {
    Ok(db
        .prepare(&format!("SELECT {COLUMNS} FROM media ORDER BY name"))?
        .query_map([], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
/// Spelling and case key are journal bytes, not reconstructed by clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedMediaName {
    pub name: String,
    pub case_key: String,
}
pub(super) fn tracked_names(db: &Connection) -> Result<Vec<TrackedMediaName>> {
    Ok(db
        .prepare("SELECT name,case_key FROM media WHERE live=1 AND pending_download=0 ORDER BY name")?
        .query_map([], |row| {
            Ok(TrackedMediaName {
                name: row.get(0)?,
                case_key: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub(super) fn save(db: &mut Connection, value: &MediaEntry) -> Result<()> {
    let tx = db.savepoint()?;
    tx.execute("UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='counter'", [])?;
    let counter = counter(&tx)?;
    tx.execute(
        "INSERT INTO media(name,case_key,sha1,size,mtime_secs,mtime_nanos,live,dirty,change_counter,pending_download)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(name) DO UPDATE SET
        case_key=excluded.case_key,sha1=excluded.sha1,size=excluded.size,mtime_secs=excluded.mtime_secs,
        mtime_nanos=excluded.mtime_nanos,live=excluded.live,dirty=excluded.dirty,change_counter=excluded.change_counter,
        pending_download=excluded.pending_download",
        params![
            value.name,
            case_key(&value.name),
            value.sha1,
            sqlite_integer(value.size)?,
            value.mtime_secs,
            value.mtime_nanos,
            value.live,
            value.dirty,
            sqlite_integer(counter)?,
            value.pending_download
        ],
    )?;
    tx.commit()?;
    Ok(())
}
pub(super) fn pending(db: &Connection) -> Result<Vec<PendingChange>> {
    Ok(db
        .prepare(
            "SELECT name,CASE WHEN live=1 AND size BETWEEN 1 AND 104857600 THEN sha1 ELSE NULL END,change_counter
        FROM media INDEXED BY media_dirty WHERE dirty=1 ORDER BY name",
        )?
        .query_map([], |r| {
            Ok(PendingChange {
                name: r.get(0)?,
                sha1: r.get(1)?,
                change_counter: unsigned(r, 2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub(super) fn marker(db: &Connection) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key='folder_mtime'", [], |r| r.get(0))
        .optional()?)
}
pub(super) fn set_marker(db: &Connection, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => {
            db.execute(
                "INSERT INTO meta VALUES('folder_mtime',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [value],
            )?;
        }
        None => {
            db.execute("DELETE FROM meta WHERE key='folder_mtime'", [])?;
        }
    }
    Ok(())
}

pub(super) fn sqlite_integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| super::MediaError::Database("media integer exceeds SQLite range".into()))
}
pub(super) fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
