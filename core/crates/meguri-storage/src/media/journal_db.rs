use super::{Result, case_fold::case_key};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;

pub(super) const COLUMNS: &str =
    "name,sha1,size,mtime_secs,mtime_nanos,live,dirty,change_counter,pending_download,sync_state";
/// File presence (`MediaEntry::live`) is independent of this protocol state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum MediaSyncState {
    None = 0,
    Live = 1,
    Deleted = 2,
    Awaiting = 3,
}
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
    pub sync_state: MediaSyncState,
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
    const VERSION: i64 = 2;
    let mut old_version = 0;
    if path.try_exists()? {
        // Probe a copy when SQLite might need sidecar recovery. Even a refused
        // foreign WAL database must not acquire new sidecars in its directory.
        let temporary = tempfile::tempdir()?;
        let probe_path = temporary.path().join("probe.db");
        std::fs::copy(path, &probe_path)?;
        for suffix in ["-wal", "-journal"] {
            let source = super::folder::sidecar(path, suffix);
            if source.try_exists()? {
                std::fs::copy(source, super::folder::sidecar(&probe_path, suffix))?;
            }
        }
        let probe = Connection::open(&probe_path)?;
        let application_id: i64 = probe.pragma_query_value(None, "application_id", |row| row.get(0))?;
        old_version = probe.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if application_id != APPLICATION_ID || !(1..=VERSION).contains(&old_version) {
            return Err(super::MediaError::Database(
                "unsupported media journal format or version".into(),
            ));
        }
    }
    let mut db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    #[cfg(target_vendor = "apple")]
    db.execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if old_version == 0 {
        tx.execute_batch(
            "CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            INSERT INTO meta VALUES('counter','0'),('last_usn','0');
            CREATE TABLE media(
                name TEXT PRIMARY KEY,case_key TEXT NOT NULL,sha1 TEXT,size INTEGER NOT NULL,
                mtime_secs INTEGER NOT NULL,mtime_nanos INTEGER NOT NULL,live INTEGER NOT NULL,
                dirty INTEGER NOT NULL,change_counter INTEGER NOT NULL,pending_download INTEGER NOT NULL DEFAULT 0,
                sync_state INTEGER NOT NULL DEFAULT 0 CHECK(sync_state BETWEEN 0 AND 3));
            CREATE INDEX media_case ON media(case_key) WHERE live=1;
            CREATE TABLE staged_ops(id INTEGER PRIMARY KEY AUTOINCREMENT,refs TEXT NOT NULL DEFAULT '[]');
            CREATE TABLE staged_files(op_id INTEGER NOT NULL,idx INTEGER NOT NULL,name TEXT NOT NULL,
                sha1 TEXT NOT NULL,size INTEGER NOT NULL,path TEXT NOT NULL,replace_existing INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(op_id,idx));
            CREATE INDEX staged_name ON staged_files(name);
            CREATE TABLE trash_entries(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,
                sha1 TEXT NOT NULL,size INTEGER NOT NULL,trashed_ms INTEGER NOT NULL,path TEXT UNIQUE);",
        )?;
    } else if old_version == 1 {
        tx.execute_batch(
            "ALTER TABLE media ADD COLUMN sync_state INTEGER NOT NULL DEFAULT 0 CHECK(sync_state BETWEEN 0 AND 3);
            UPDATE media SET sync_state=CASE
                WHEN pending_download=1 AND live=0 AND dirty=0 THEN 3
                WHEN live=1 AND size BETWEEN 1 AND 104857600 THEN 1
                WHEN live=0 OR dirty=1 THEN 2 ELSE 0 END;
            DROP INDEX media_dirty;
            DROP INDEX media_download;",
        )?;
    }
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS media_dirty ON media(change_counter,name) WHERE dirty=1;
        CREATE INDEX IF NOT EXISTS media_download ON media(case_key,name) WHERE sync_state=3;
        CREATE TABLE IF NOT EXISTS media_downloads(name TEXT PRIMARY KEY,sha1 TEXT,usn INTEGER);
        CREATE INDEX IF NOT EXISTS media_download_usn ON media_downloads(usn);
        INSERT OR IGNORE INTO meta VALUES('baseline_epoch','0');",
    )?;
    if old_version == 1 {
        // v1 had no server USN per download. Keep that unknown, pinning the
        // cursor until the caller resets/binds or re-lists this intent.
        tx.execute_batch("INSERT INTO media_downloads SELECT name,CASE WHEN live=0 THEN sha1 ELSE NULL END,NULL FROM media WHERE pending_download=1;")?;
    }
    if old_version == 1 {
        meguri_model::faultpoint!("media.sync.migration.before_commit")?;
    }
    tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
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
        sync_state: match row.get::<_, i64>(9)? {
            0 => MediaSyncState::None,
            1 => MediaSyncState::Live,
            2 => MediaSyncState::Deleted,
            3 => MediaSyncState::Awaiting,
            value => return Err(rusqlite::Error::IntegralValueOutOfRange(9, value)),
        },
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
        .prepare("SELECT name,case_key FROM media WHERE live=1 AND sync_state!=3 ORDER BY name")?
        .query_map([], |row| {
            Ok(TrackedMediaName {
                name: row.get(0)?,
                case_key: row.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub(super) fn save(db: &Connection, value: &MediaEntry) -> Result<()> {
    if meguri_text::normalize::nfc(&value.name) != value.name
        || value.dirty && !matches!(value.sync_state, MediaSyncState::Live | MediaSyncState::Deleted)
        || value.sync_state == MediaSyncState::Live && !value.syncable()
        || value.sync_state == MediaSyncState::Deleted && value.syncable()
        || value.sync_state == MediaSyncState::Awaiting && value.live
        || value.sync_state == MediaSyncState::None && !value.live
    {
        return Err(super::MediaError::Database("invalid media entry state".into()));
    }
    // T8 assigns the next global stamp when saving a row. This savepoint
    // also undoes that allocation if SQL fails; an outer transaction may
    // still discard the completed save.
    db.execute_batch("SAVEPOINT media_entry_write")?;
    let outcome = (|| {
        let counter = next_counter(db)?;
        db.execute(
            "INSERT INTO media(name,case_key,sha1,size,mtime_secs,mtime_nanos,live,dirty,change_counter,pending_download,sync_state)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(name) DO UPDATE SET
            case_key=excluded.case_key,sha1=excluded.sha1,size=excluded.size,mtime_secs=excluded.mtime_secs,
            mtime_nanos=excluded.mtime_nanos,live=excluded.live,dirty=excluded.dirty,change_counter=excluded.change_counter,
            pending_download=excluded.pending_download,sync_state=excluded.sync_state",
            params![value.name,case_key(&value.name),value.sha1,sqlite_integer(value.size)?,value.mtime_secs,
                value.mtime_nanos,value.live,value.dirty,sqlite_integer(counter)?,value.pending_download,value.sync_state as i64],
        )?;
        Ok(())
    })();
    if outcome.is_err() {
        db.execute_batch("ROLLBACK TO media_entry_write")?;
    }
    db.execute_batch("RELEASE media_entry_write")?;
    outcome
}
pub(super) fn next_counter(db: &Connection) -> Result<u64> {
    let next = counter(db)?
        .checked_add(1)
        .ok_or_else(|| super::MediaError::Database("media counter exhausted".into()))?;
    sqlite_integer(next)?;
    db.execute("UPDATE meta SET value=?1 WHERE key='counter'", [next.to_string()])?;
    Ok(next)
}
/// Deletions consume a global stamp too: removing and recreating a name cannot
/// make an in-flight acknowledgement match a different incarnation.
pub(super) fn remove(db: &Connection, name: &str) -> Result<()> {
    db.execute_batch("SAVEPOINT media_entry_remove")?;
    let outcome = (|| {
        if db.execute("DELETE FROM media WHERE name=?1", [name])? != 0 {
            next_counter(db)?;
        }
        Ok(())
    })();
    if outcome.is_err() {
        db.execute_batch("ROLLBACK TO media_entry_remove")?;
    }
    db.execute_batch("RELEASE media_entry_remove")?;
    outcome
}
pub(super) fn pending(db: &Connection, limit: u64) -> Result<Vec<PendingChange>> {
    Ok(db
        .prepare(
            "SELECT name,CASE WHEN sync_state=1 THEN sha1 ELSE NULL END,change_counter
        FROM media INDEXED BY media_dirty WHERE dirty=1 ORDER BY change_counter,name LIMIT ?1",
        )?
        .query_map([sqlite_integer(limit)?], |r| {
            Ok(PendingChange {
                name: r.get(0)?,
                sha1: r.get(1)?,
                change_counter: unsigned(r, 2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub(super) fn pending_any(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM media INDEXED BY media_dirty WHERE dirty=1)",
        [],
        |r| r.get(0),
    )?)
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
