//! J6 owns file-fact history; protocol state is deliberately absent here.
use super::{MediaError, MediaStore, Result, journal_db};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaFeedId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaAbsenceCause {
    Removed,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFileFacts {
    pub sha1: String,
    pub size: u64,
    pub mtime_secs: i64,
    pub mtime_nanos: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaFileState {
    Present(MediaFileFacts),
    Absent(MediaAbsenceCause),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFeedItem {
    pub name: String,
    pub change_counter: u64,
    pub state: MediaFileState,
    pub retained_removal: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFeedStatus {
    pub feed_id: MediaFeedId,
    pub counter: u64,
    pub floor: Option<u64>,
    pub complete: bool,
}

/// Owned values from one read transaction; no statement, transaction or lease
/// escapes the call. Completeness concerns registration, not consumer progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFeedSnapshot {
    pub status: MediaFeedStatus,
    pub items: Vec<MediaFeedItem>,
}

fn invalid() -> MediaError {
    MediaError::Database("invalid media feed metadata or cursor".into())
}

fn meta(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}

pub(super) fn status(db: &Connection) -> Result<MediaFeedStatus> {
    let id = meta(db, "feed_id")?.ok_or_else(invalid)?;
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut bytes = [0; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&id[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
    }
    let counter = journal_db::counter(db)?;
    let floor = meta(db, "cloud_feed_floor")?
        .map(|s| s.parse::<u64>().map_err(|_| invalid()))
        .transpose()?;
    if floor.is_some_and(|f| f > counter) {
        return Err(invalid());
    }
    let complete = match meta(db, "feed_complete")?.as_deref() {
        Some("0") => false,
        Some("1") => true,
        _ => return Err(invalid()),
    };
    Ok(MediaFeedStatus {
        feed_id: MediaFeedId(bytes),
        counter,
        floor,
        complete,
    })
}

fn cause(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<MediaAbsenceCause> {
    match row.get::<_, String>(index)?.as_str() {
        "removed" => Ok(MediaAbsenceCause::Removed),
        "missing" => Ok(MediaAbsenceCause::Missing),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn snapshot(db: &Connection, after: Option<u64>) -> Result<MediaFeedSnapshot> {
    let tx = db.unchecked_transaction()?;
    let status = status(&tx)?;
    if after.is_some_and(|c| c > status.counter) {
        return Err(invalid());
    }
    let after = after.map(journal_db::sqlite_integer).transpose()?;
    let items = tx
        .prepare(
            "SELECT name,change_counter,live,sha1,size,mtime_secs,mtime_nanos,absence_cause,0
         FROM media WHERE NOT EXISTS(SELECT 1 FROM media_feed_facts f WHERE f.name=media.name)
             AND (?1 IS NULL OR change_counter>?1)
         UNION ALL
         SELECT name,change_counter,live,sha1,size,mtime_secs,mtime_nanos,absence_cause,0
         FROM media_feed_facts WHERE ?1 IS NULL OR change_counter>?1
         UNION ALL
         SELECT name,change_counter,0,NULL,0,0,0,absence_cause,1
         FROM media_feed_removals WHERE ?1 IS NULL OR change_counter>?1
         ORDER BY 2,1,9",
        )?
        .query_map([after], |row| {
            Ok(MediaFeedItem {
                name: row.get(0)?,
                change_counter: journal_db::unsigned(row, 1)?,
                state: if row.get::<_, bool>(2)? {
                    MediaFileState::Present(MediaFileFacts {
                        sha1: row.get(3)?,
                        size: journal_db::unsigned(row, 4)?,
                        mtime_secs: row.get(5)?,
                        mtime_nanos: row.get(6)?,
                    })
                } else {
                    MediaFileState::Absent(cause(row, 7)?)
                },
                retained_removal: row.get(8)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    tx.commit()?;
    Ok(MediaFeedSnapshot { status, items })
}

impl MediaStore {
    pub fn media_feed_status(&self) -> Result<MediaFeedStatus> {
        let db = self.db()?;
        let tx = db.unchecked_transaction()?;
        let value = status(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// The caller must compare feed_id and floor with its committed cursor.
    /// A cursor below the floor has incomplete history and needs a snapshot.
    /// Facts can be unchanged; this is a current-state feed, not an event log.
    pub fn media_feed(&self, after: u64) -> Result<MediaFeedSnapshot> {
        snapshot(&*self.db()?, Some(after))
    }

    pub fn media_feed_snapshot(&self) -> Result<MediaFeedSnapshot> {
        snapshot(&*self.db()?, None)
    }

    /// Caller obligation (K10.3): use the minimum consumed counter for this
    /// identity over ALL active bindings in the tracking file, across builds,
    /// containers and environments, only AFTER committing tracking progress.
    /// This store cannot enumerate or validate those bindings. Initialize an
    /// unset floor at the current counter only when none has consumed this
    /// identity, BEFORE taking the new consumer's reconcile snapshot. Another
    /// new binding must leave an existing floor alone. Unset only when no
    /// binding remains, including paused bindings. This implements retention,
    /// not tracking's cross-build minimum algorithm.
    pub fn set_media_feed_floor(&self, expected: MediaFeedId, floor: Option<u64>) -> Result<()> {
        let mut db = self.db()?;
        let tx = db.savepoint()?;
        let current = status(&tx)?;
        if current.feed_id != expected {
            return Err(invalid());
        }
        if let Some(floor) = floor {
            if floor > current.counter
                || current.floor.is_some_and(|old| floor < old)
                || current.floor.is_none() && floor != current.counter
            {
                return Err(invalid());
            }
            tx.execute(
                "INSERT INTO meta VALUES('cloud_feed_floor',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [floor.to_string()],
            )?;
            tx.execute(
                "DELETE FROM media_feed_removals WHERE change_counter<=?1",
                [journal_db::sqlite_integer(floor)?],
            )?;
        } else {
            tx.execute("DELETE FROM meta WHERE key='cloud_feed_floor'", [])?;
            tx.execute("DELETE FROM media_feed_removals", [])?;
        }
        tx.commit()?;
        Ok(())
    }
}

pub(super) fn migrate(db: &Connection, random: &dyn meguri_model::rng::RandomSource) -> Result<()> {
    db.execute_batch(
        "ALTER TABLE media ADD COLUMN absence_cause TEXT NOT NULL DEFAULT 'missing' CHECK(absence_cause IN ('removed','missing'));
         ALTER TABLE media ADD COLUMN explicit_removal INTEGER NOT NULL DEFAULT 0 CHECK(explicit_removal IN (0,1));
         CREATE TABLE media_feed_facts(name TEXT PRIMARY KEY,sha1 TEXT,size INTEGER NOT NULL,
             mtime_secs INTEGER NOT NULL,mtime_nanos INTEGER NOT NULL,live INTEGER NOT NULL,
             change_counter INTEGER NOT NULL,absence_cause TEXT NOT NULL DEFAULT 'missing',
             explicit_removal INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX media_feed_facts_counter ON media_feed_facts(change_counter,name);
         CREATE TABLE media_feed_removals(name TEXT NOT NULL,change_counter INTEGER NOT NULL,
             absence_cause TEXT NOT NULL CHECK(absence_cause IN ('removed','missing')),PRIMARY KEY(name,change_counter));
         CREATE INDEX media_feed_counter ON media(change_counter,name);
         CREATE INDEX media_feed_removal_counter ON media_feed_removals(change_counter,name);
         CREATE INDEX media_feed_moves ON media(name) WHERE explicit_removal=1 AND live=1;
         CREATE TRIGGER media_feed_remove BEFORE DELETE ON media BEGIN
             SELECT CASE WHEN CAST((SELECT value FROM meta WHERE key='counter') AS INTEGER)=9223372036854775807
                 THEN RAISE(ABORT,'media counter exhausted') END;
             UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='counter';
             INSERT OR REPLACE INTO media_feed_removals(name,change_counter,absence_cause)
             SELECT OLD.name,
                 CASE WHEN OLD.live=1 THEN CAST((SELECT value FROM meta WHERE key='counter') AS INTEGER) ELSE OLD.change_counter END,
                 CASE WHEN OLD.live=1 THEN CASE WHEN OLD.explicit_removal=1 THEN 'removed' ELSE 'missing' END ELSE OLD.absence_cause END
             WHERE NOT EXISTS(SELECT 1 FROM media_feed_facts WHERE name=OLD.name)
                 AND EXISTS(SELECT 1 FROM meta WHERE key='cloud_feed_floor'
                 AND CAST(value AS INTEGER)<CASE WHEN OLD.live=1
                     THEN CAST((SELECT value FROM meta WHERE key='counter') AS INTEGER) ELSE OLD.change_counter END);
         END;",
    )?;
    replace_identity(db, random)?;
    if journal_db::marker(db)?.is_some() {
        complete_registration(db)?;
    }
    Ok(())
}

pub(super) fn replace_identity(db: &Connection, random: &dyn meguri_model::rng::RandomSource) -> Result<()> {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&random.next_u64().to_le_bytes());
    bytes[8..].copy_from_slice(&random.next_u64().to_le_bytes());
    let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    db.execute(
        "INSERT INTO meta VALUES('feed_id',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [id],
    )?;
    db.execute(
        "INSERT INTO meta VALUES('feed_complete','0') ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [],
    )?;
    db.execute("DELETE FROM meta WHERE key='cloud_feed_floor'", [])?;
    db.execute("DELETE FROM media_feed_removals", [])?;
    db.execute("DELETE FROM media_feed_facts", [])?;
    Ok(())
}

/// Only a completed full registration or whole-inventory publication may call
/// this, in the SAME transaction that records its folder marker.
pub(super) fn complete_registration(db: &Connection) -> Result<()> {
    if journal_db::marker(db)?.is_none() {
        return Err(invalid());
    }
    db.execute("UPDATE meta SET value='1' WHERE key='feed_complete'", [])?;
    Ok(())
}

/// Observe only an actual move out of collection.media, never staged bytes.
/// Some existing publishers defer dropping facts until their later transaction;
/// retain the reason across that boundary, without touching protocol state.
pub(super) fn observe_removal(db: &Connection, name: &str) -> Result<()> {
    db.execute_batch("SAVEPOINT media_feed_removal_observation")?;
    let result: Result<()> = (|| {
        let present: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM media WHERE name=?1 AND live=1
            UNION ALL SELECT 1 FROM media_feed_facts WHERE name=?1 AND live=1)",
            [name],
            |r| r.get(0),
        )?;
        if present {
            db.execute(
                "UPDATE media SET explicit_removal=1 WHERE name=?1 AND live=1",
                params![name],
            )?;
            db.execute(
                "UPDATE media_feed_facts SET explicit_removal=1 WHERE name=?1 AND live=1",
                [name],
            )?;
        } else {
            let counter = journal_db::next_counter(db)?;
            db.execute(
                "INSERT INTO media_feed_facts(name,size,mtime_secs,mtime_nanos,live,change_counter,absence_cause)
                VALUES(?1,0,0,0,0,?2,'removed') ON CONFLICT(name) DO UPDATE SET live=0,sha1=NULL,
                size=0,mtime_secs=0,mtime_nanos=0,change_counter=excluded.change_counter,
                absence_cause='removed',explicit_removal=0",
                params![name, journal_db::sqlite_integer(counter)?],
            )?;
        }
        Ok(())
    })();
    if result.is_err() {
        db.execute_batch("ROLLBACK TO media_feed_removal_observation")?;
    }
    db.execute_batch("RELEASE media_feed_removal_observation")?;
    result
}

/// An interrupted trash can be restored outside Meguri with identical stats.
/// The scan's unchanged-file fast path must clear that old move observation,
/// while never clearing one made by a concurrent write after its cutoff.
pub(super) fn observe_registered(
    db: &Connection,
    media_dir: &std::path::Path,
    found: &std::collections::BTreeSet<String>,
    cutoff: u64,
) -> Result<()> {
    let observed = db
        .prepare("SELECT name FROM media WHERE explicit_removal=1 AND live=1 AND change_counter<=?1")?
        .query_map([journal_db::sqlite_integer(cutoff)?], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for name in observed {
        // A failed page can move the file after the scan observed it, without
        // changing the protocol row's stamp. Recheck under the shared guard.
        if found.contains(&name) && super::folder::regular(&media_dir.join(&name)).is_ok_and(|m| m.is_some()) {
            db.execute("UPDATE media SET explicit_removal=0 WHERE name=?1", [name])?;
        }
    }
    Ok(())
}

/// T4.3 leaves an Awaiting manual row byte-for-byte unchanged for unsyncable
/// bytes. These physical facts therefore have their own stamp and lifetime.
pub(super) fn independent(db: &Connection, name: &str) -> Result<Option<(u64, Option<MediaFileFacts>)>> {
    Ok(db
        .query_row(
            "SELECT change_counter,live,sha1,size,mtime_secs,mtime_nanos FROM media_feed_facts WHERE name=?1",
            [name],
            |r| {
                Ok((
                    journal_db::unsigned(r, 0)?,
                    if r.get::<_, bool>(1)? {
                        Some(MediaFileFacts {
                            sha1: r.get(2)?,
                            size: journal_db::unsigned(r, 3)?,
                            mtime_secs: r.get(4)?,
                            mtime_nanos: r.get(5)?,
                        })
                    } else {
                        None
                    },
                ))
            },
        )
        .optional()?)
}

pub(super) fn independent_present(db: &Connection, name: &str, hash: &str, m: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    db.execute_batch("SAVEPOINT media_feed_facts_write")?;
    let result: Result<()> = (|| {
        let counter = journal_db::next_counter(db)?;
        db.execute("INSERT INTO media_feed_facts(name,sha1,size,mtime_secs,mtime_nanos,live,change_counter)
            VALUES(?1,?2,?3,?4,?5,1,?6) ON CONFLICT(name) DO UPDATE SET sha1=excluded.sha1,size=excluded.size,
            mtime_secs=excluded.mtime_secs,mtime_nanos=excluded.mtime_nanos,live=1,change_counter=excluded.change_counter,
            absence_cause='missing',explicit_removal=0",
            params![name,hash,journal_db::sqlite_integer(m.len())?,m.mtime(),m.mtime_nsec(),journal_db::sqlite_integer(counter)?])?;
        db.execute("DELETE FROM media_feed_removals WHERE name=?1", [name])?;
        Ok(())
    })();
    if result.is_err() {
        db.execute_batch("ROLLBACK TO media_feed_facts_write")?;
    }
    db.execute_batch("RELEASE media_feed_facts_write")?;
    result
}

pub(super) fn independent_absent(db: &Connection, name: &str) -> Result<()> {
    if independent(db, name)?.is_some_and(|(_, facts)| facts.is_some()) {
        db.execute_batch("SAVEPOINT media_feed_facts_absent")?;
        let result: Result<()> = (|| {
            let counter = journal_db::next_counter(db)?;
            db.execute(
                "UPDATE media_feed_facts SET live=0,sha1=NULL,size=0,mtime_secs=0,mtime_nanos=0,
                change_counter=?2,absence_cause=CASE WHEN explicit_removal=1 THEN 'removed' ELSE 'missing' END,
                explicit_removal=0 WHERE name=?1",
                params![name, journal_db::sqlite_integer(counter)?],
            )?;
            Ok(())
        })();
        if result.is_err() {
            db.execute_batch("ROLLBACK TO media_feed_facts_absent")?;
        }
        db.execute_batch("RELEASE media_feed_facts_absent")?;
        result?;
    }
    Ok(())
}

pub(super) fn independent_missing(
    db: &Connection,
    found: &std::collections::BTreeSet<String>,
    cutoff: u64,
) -> Result<()> {
    let names = db
        .prepare("SELECT name FROM media_feed_facts WHERE live=1 AND change_counter<=?1")?
        .query_map([journal_db::sqlite_integer(cutoff)?], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for name in names {
        if !found.contains(&name) {
            independent_absent(db, &name)?;
        }
    }
    Ok(())
}
