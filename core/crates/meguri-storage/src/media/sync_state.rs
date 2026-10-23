//! Durable journal boundaries for the future selected-media caller. No network
//! policy lives here. The caller must admit the M5 operation, cancel/drain prior
//! jobs on identity change, and hold its media-job exclusion across reset or
//! each page/batch. These methods never grant consent or start work.
use super::{
    MediaEntry, MediaError, MediaStore, MediaSyncState, PendingChange, Result, case_fold::*, folder, journal_db,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::MetadataExt,
};

/// Supply the opaque M6 identifier issued for the configured account and
/// endpoint. Matching is case-sensitive. Its producer resolves approved aliases
/// and must keep raw addresses, account names and authentication material out
/// of this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaBaselineOwner(String);
impl MediaBaselineOwner {
    pub fn new(reference: impl Into<String>) -> Self {
        Self(reference.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaSyncBaseline {
    pub owner: Option<MediaBaselineOwner>,
    pub usn: u64,
    /// Changes on every reset, including a same-owner sanity reset.
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaDownloadIntent {
    pub name: String,
    /// Unknown only for migrated v1 download bookkeeping.
    pub sha1: Option<String>,
    pub usn: Option<u64>,
}
/// Decisions are supplied by the protocol caller against current journal
/// stamps. A stale page fails atomically, so the caller can decide again.
#[derive(Clone, Debug)]
pub enum MediaPageDecision {
    Clean {
        name: String,
        stamp: u64,
    },
    Remove {
        name: String,
        stamp: u64,
    },
    Download {
        name: String,
        sha1: String,
        usn: u64,
        stamp: Option<u64>,
    },
}
#[derive(Clone, Debug)]
pub struct MediaDownloadRecord {
    pub intent: MediaDownloadIntent,
    /// Already published via the store's file/staging APIs. None is Y9's
    /// ignored/transform-empty case. Renamed files retain their pending state.
    pub stored_name: Option<String>,
}
fn invalid(message: &str) -> MediaError {
    MediaError::Database(message.into())
}
fn meta(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}
fn put_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT INTO meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
fn number(db: &Connection, key: &str) -> Result<u64> {
    meta(db, key)?
        .unwrap_or_else(|| "0".into())
        .parse()
        .map_err(|_| invalid("invalid media metadata integer"))
}
pub(super) fn epoch(db: &Connection) -> Result<u64> {
    number(db, "baseline_epoch")
}
fn baseline(db: &Connection) -> Result<MediaSyncBaseline> {
    Ok(MediaSyncBaseline {
        owner: meta(db, "baseline_owner")?.map(MediaBaselineOwner),
        usn: number(db, "last_usn")?,
        epoch: number(db, "baseline_epoch")?,
    })
}
fn require_baseline(db: &Connection, expected: &MediaSyncBaseline) -> Result<()> {
    if expected.owner.is_none() || baseline(db)? != *expected {
        return Err(invalid("media baseline changed or is unbound"));
    }
    Ok(())
}
fn require_stamp(db: &Connection, name: &str, stamp: Option<u64>) -> Result<Option<MediaEntry>> {
    let entry = journal_db::entry(db, name)?;
    if entry.as_ref().map(|e| e.change_counter) != stamp {
        return Err(invalid("media entry changed"));
    }
    Ok(entry)
}
fn hash_valid(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn journal_name(name: &str) -> bool {
    safe_component(name) && meguri_text::normalize::nfc(name) == name
}
fn downloads(db: &Connection) -> Result<Vec<MediaDownloadIntent>> {
    Ok(db
        .prepare("SELECT name,sha1,usn FROM media_downloads ORDER BY usn,name")?
        .query_map([], |r| {
            Ok(MediaDownloadIntent {
                name: r.get(0)?,
                sha1: r.get(1)?,
                usn: r
                    .get::<_, Option<i64>>(2)?
                    .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, v)))
                    .transpose()?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?)
}
fn intent(db: &Connection, name: &str) -> Result<Option<MediaDownloadIntent>> {
    Ok(db
        .query_row("SELECT name,sha1,usn FROM media_downloads WHERE name=?1", [name], |r| {
            Ok(MediaDownloadIntent {
                name: r.get(0)?,
                sha1: r.get(1)?,
                usn: r
                    .get::<_, Option<i64>>(2)?
                    .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, v)))
                    .transpose()?,
            })
        })
        .optional()?)
}
fn forget_download(db: &Connection, name: &str) -> Result<()> {
    db.execute("DELETE FROM media_downloads WHERE name=?1", [name])?;
    if let Some(mut entry) = journal_db::entry(db, name)? {
        if entry.sync_state == MediaSyncState::Awaiting {
            journal_db::remove(db, name)?;
        } else if entry.pending_download {
            entry.pending_download = false;
            journal_db::save(db, &entry)?;
        }
    }
    Ok(())
}
/// The cursor cannot pass a durable unsaved download (including unknown v1
/// intents). Listed waterline survives a restart; it is not the applied USN.
fn advance_cursor(db: &Connection, high: u64) -> Result<u64> {
    journal_db::sqlite_integer(high)?;
    let old = number(db, "last_usn")?;
    let listed = number(db, "listed_usn")?.max(high);
    put_meta(db, "listed_usn", &listed.to_string())?;
    let first: Option<i64> = db.query_row("SELECT MIN(COALESCE(usn,0)) FROM media_downloads", [], |r| r.get(0))?;
    let bound = first
        .map(|v| u64::try_from(v).map(|v| v.saturating_sub(1)))
        .transpose()
        .map_err(|_| invalid("invalid media download USN"))?
        .unwrap_or(listed);
    if bound < old {
        return Err(invalid("unsaved media lies below applied cursor"));
    }
    let next = listed.min(bound).max(old);
    put_meta(db, "last_usn", &next.to_string())?;
    Ok(next)
}
fn reset(db: &mut Connection, owner: Option<&MediaBaselineOwner>) -> Result<()> {
    let tx = db.savepoint()?;
    let epoch = journal_db::next_counter(&tx)?;
    for mut entry in journal_db::entries(&tx)? {
        if !entry.live {
            journal_db::remove(&tx, &entry.name)?;
            continue;
        }
        entry.sync_state = MediaSyncState::None;
        entry.dirty = false;
        entry.pending_download = false;
        journal_db::save(&tx, &entry)?;
    }
    tx.execute("DELETE FROM media_downloads", [])?;
    put_meta(&tx, "last_usn", "0")?;
    put_meta(&tx, "listed_usn", "0")?;
    put_meta(&tx, "baseline_epoch", &epoch.to_string())?;
    journal_db::set_marker(&tx, None)?;
    match owner {
        Some(owner) => put_meta(&tx, "baseline_owner", owner.as_str())?,
        None => {
            tx.execute("DELETE FROM meta WHERE key='baseline_owner'", [])?;
        }
    }
    meguri_model::faultpoint!("media.sync.reset.before_commit")?;
    tx.commit()?;
    Ok(())
}
impl MediaStore {
    pub fn sync_baseline(&self) -> Result<MediaSyncBaseline> {
        baseline(&*self.db()?)
    }
    pub fn sync_entry(&self, name: &str) -> Result<Option<MediaEntry>> {
        journal_db::entry(&*self.db()?, name)
    }
    /// Unknown/changed owner resets and binds in one transaction. Caller must
    /// obtain consent and drain old jobs first. Same owner preserves progress.
    pub fn bind_sync_owner(&self, owner: &MediaBaselineOwner) -> Result<bool> {
        let mut db = self.db()?;
        if baseline(&db)?.owner.as_ref() == Some(owner) {
            return Ok(false);
        }
        reset(&mut db, Some(owner))?;
        Ok(true)
    }
    /// Same-owner sanity reset; never permission to run another operation.
    pub fn reset_sync(&self) -> Result<()> {
        let mut db = self.db()?;
        let owner = baseline(&db)?.owner;
        reset(&mut db, owner.as_ref())
    }
    /// Logout invalidates the binding without touching files or manual history.
    pub fn invalidate_sync_owner(&self) -> Result<()> {
        reset(&mut *self.db()?, None)
    }
    pub fn download_intents(&self) -> Result<Vec<MediaDownloadIntent>> {
        downloads(&*self.db()?)
    }
    /// Includes case variants, through the awaiting-only index. Filesystem
    /// lookup remains separate: an externally appeared file can already render.
    pub fn is_awaiting(&self, name: &str) -> Result<bool> {
        Ok(self.db()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM media INDEXED BY media_download WHERE sync_state=3 AND case_key=?1)",
            [case_key(&meguri_text::normalize::nfc(name))],
            |r| r.get(0),
        )?)
    }
    pub fn awaiting_names(&self) -> Result<Vec<String>> {
        Ok(self
            .db()?
            .prepare("SELECT name FROM media INDEXED BY media_download WHERE sync_state=3 ORDER BY case_key,name")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }
    /// Pending check and count share one read snapshot for Y11's sanity check.
    /// None asks the caller to finish the push before sending sanity.
    pub fn clean_live_count(&self) -> Result<Option<u64>> {
        let db = self.db()?;
        let tx = db.unchecked_transaction()?;
        let count = if journal_db::pending_any(&tx)? {
            None
        } else {
            Some(tx.query_row("SELECT COUNT(*) FROM media WHERE sync_state=1", [], |r| {
                journal_db::unsigned(r, 0)
            })?)
        };
        tx.commit()?;
        Ok(count)
    }
    pub fn live_count(&self) -> Result<u64> {
        Ok(self
            .db()?
            .query_row("SELECT COUNT(*) FROM media WHERE sync_state=1", [], |r| {
                journal_db::unsigned(r, 0)
            })?)
    }
    /// Caller validates server ordering/USNs and supplies the complete page's
    /// decisions, including re-decisions for duplicate server names. Storage
    /// validates stamps and pins the cursor below every unsaved file.
    pub fn commit_media_page(
        &self,
        expected: &MediaSyncBaseline,
        highest_usn: u64,
        decisions: &[MediaPageDecision],
    ) -> Result<u64> {
        let mut db = self.db()?;
        require_baseline(&db, expected)?;
        if highest_usn < expected.usn {
            return Err(invalid("media page precedes cursor"));
        }
        // Validate all stamps before any file moves.
        let mut names = BTreeSet::new();
        for decision in decisions {
            let (name, stamp) = match decision {
                MediaPageDecision::Clean { name, stamp } | MediaPageDecision::Remove { name, stamp } => {
                    (name, Some(*stamp))
                }
                MediaPageDecision::Download { name, stamp, .. } => (name, *stamp),
            };
            if name.is_empty() || !names.insert(name) {
                return Err(invalid("empty or duplicate media page decision"));
            }
            let entry = require_stamp(&db, name, stamp)?;
            if matches!(decision, MediaPageDecision::Clean { .. })
                && entry.as_ref().is_none_or(|e| e.sync_state != MediaSyncState::Live)
            {
                return Err(invalid("only a saved live file can be confirmed"));
            }
            if let MediaPageDecision::Download { sha1, usn, .. } = decision {
                if !hash_valid(sha1) || *usn <= expected.usn || *usn > highest_usn {
                    return Err(invalid("invalid media download intent"));
                }
            }
        }
        // Files are independently recoverable in the existing self-describing
        // trash if a later journal transaction fails or the process stops.
        for decision in decisions {
            if let MediaPageDecision::Remove { name, .. } = decision {
                if let Some(entry) = journal_db::entry(&db, name)?.filter(|e| e.live) {
                    if !normalized(name) || ignored(name) {
                        return Err(MediaError::InvalidName);
                    }
                    if let Some(m) = folder::regular(&self.media_dir().join(name))? {
                        self.move_to_trash_locked(
                            &mut db,
                            name,
                            entry.sha1.as_deref().ok_or_else(|| invalid("missing media hash"))?,
                            m.len(),
                            &self.media_dir().join(name),
                            false,
                        )?;
                    }
                }
            }
        }
        let tx = db.savepoint()?;
        for decision in decisions {
            match decision {
                MediaPageDecision::Clean { name, stamp } => {
                    // The sole stamp-preserving entry write (T8).
                    tx.execute(
                        "UPDATE media SET dirty=0 WHERE name=?1 AND change_counter=?2",
                        params![name, journal_db::sqlite_integer(*stamp)?],
                    )?;
                    forget_download(&tx, name)?;
                }
                MediaPageDecision::Remove { name, .. } => {
                    journal_db::remove(&tx, name)?;
                    tx.execute("DELETE FROM media_downloads WHERE name=?1", [name])?;
                }
                MediaPageDecision::Download { name, sha1, usn, .. } => {
                    tx.execute("INSERT INTO media_downloads VALUES(?1,?2,?3) ON CONFLICT(name) DO UPDATE SET sha1=excluded.sha1,usn=excluded.usn", params![name,sha1,journal_db::sqlite_integer(*usn)?])?;
                    if journal_name(name) {
                        let mut entry = journal_db::entry(&tx, name)?.unwrap_or(MediaEntry {
                            name: name.clone(),
                            sha1: None,
                            size: 0,
                            mtime_secs: 0,
                            mtime_nanos: 0,
                            live: false,
                            dirty: false,
                            change_counter: 0,
                            pending_download: false,
                            sync_state: MediaSyncState::Awaiting,
                        });
                        if !entry.live {
                            entry.sync_state = MediaSyncState::Awaiting;
                            entry.dirty = false;
                            entry.sha1 = None;
                        }
                        entry.pending_download = true;
                        journal_db::save(&tx, &entry)?;
                    }
                }
            }
        }
        let usn = advance_cursor(&tx, highest_usn)?;
        meguri_model::faultpoint!("media.sync.page.before_commit")?;
        tx.commit()?;
        Ok(usn)
    }
    /// Y5 empty-page cleanup. The caller supplies all names seen during this
    /// listing, including names that did not require a download.
    pub fn finish_media_listing(&self, expected: &MediaSyncBaseline, listed_names: &BTreeSet<String>) -> Result<u64> {
        let mut db = self.db()?;
        require_baseline(&db, expected)?;
        let tx = db.savepoint()?;
        for old in downloads(&tx)? {
            if !listed_names.contains(&old.name) {
                forget_download(&tx, &old.name)?;
            }
        }
        let usn = advance_cursor(&tx, expected.usn)?;
        tx.commit()?;
        Ok(usn)
    }
    /// Y10.4: caller passes only accepted changes whose streamed bytes matched
    /// their recorded hashes. A stale stamp stays pending. Server no-ops/gaps
    /// preserve the old USN; clean tombstones remain durable.
    pub fn commit_media_upload(
        &self,
        expected: &MediaSyncBaseline,
        accepted: &[PendingChange],
        server_usn: u64,
    ) -> Result<u64> {
        let mut db = self.db()?;
        require_baseline(&db, expected)?;
        journal_db::sqlite_integer(server_usn)?;
        let mut names = BTreeSet::new();
        if accepted.iter().any(|c| !names.insert(&c.name)) {
            return Err(invalid("duplicate accepted media change"));
        }
        let tx = db.savepoint()?;
        for change in accepted {
            tx.execute(
                "UPDATE media SET dirty=0 WHERE name=?1 AND change_counter=?2 AND dirty=1",
                params![change.name, journal_db::sqlite_integer(change.change_counter)?],
            )?;
        }
        let usn = if expected.usn.checked_add(accepted.len() as u64) == Some(server_usn) {
            advance_cursor(&tx, server_usn)?
        } else {
            expected.usn
        };
        meguri_model::faultpoint!("media.sync.upload.before_commit")?;
        tx.commit()?;
        Ok(usn)
    }
    /// Y8.5 journal boundary for files already durably published. Hashing is
    /// outside the journal lock; re-stat and stamp checks reject concurrent
    /// writes. All records and their cursor advance commit together.
    pub fn commit_media_downloads(&self, expected: &MediaSyncBaseline, records: &[MediaDownloadRecord]) -> Result<u64> {
        let mut groups: BTreeMap<&str, Vec<&MediaDownloadRecord>> = BTreeMap::new();
        let mut names = BTreeSet::new();
        for record in records {
            if !names.insert(&record.intent.name) {
                return Err(invalid("duplicate recorded download"));
            }
            if let Some(name) = record.stored_name.as_deref() {
                if !normalized(name) || ignored(name) {
                    return Err(MediaError::InvalidName);
                }
                groups.entry(name).or_default().push(record);
            } else {
                let transformed = meguri_text::media_name::safe_media_name(&record.intent.name);
                if !ignored(&record.intent.name) && !transformed.is_empty() && !ignored(&transformed) {
                    return Err(invalid("download requires a saved file"));
                }
            }
        }
        // A name cannot be both a saved destination and a renamed original
        // awaiting deletion in this batch.
        for record in records {
            if record.stored_name.as_ref() != Some(&record.intent.name)
                && journal_name(&record.intent.name)
                && groups.contains_key(record.intent.name.as_str())
            {
                return Err(invalid("conflicting download destinations"));
            }
        }
        let old_entries = {
            let db = self.db()?;
            require_baseline(&db, expected)?;
            for record in records {
                if intent(&db, &record.intent.name)?.as_ref() != Some(&record.intent) {
                    return Err(invalid("download intent changed"));
                }
            }
            groups
                .keys()
                .map(|name| journal_db::entry(&db, name))
                .collect::<Result<Vec<_>>>()?
        };
        let mut observations = Vec::new();
        for ((name, group), old) in groups.into_iter().zip(old_entries) {
            let path = self.media_dir().join(name);
            let m = folder::regular(&path)?.ok_or_else(|| invalid("download file not saved"))?;
            if !(1..=104_857_600).contains(&m.len()) {
                return Err(invalid("download file is not syncable"));
            }
            let hash = folder::hash_file(&path)?;
            if group.iter().any(|record| record.intent.sha1.as_ref() != Some(&hash)) {
                return Err(invalid("download file hash differs"));
            }
            let has_exact = group.iter().any(|record| record.intent.name == name);
            observations.push((name, old, m, hash, has_exact));
        }
        let mut db = self.db()?;
        require_baseline(&db, expected)?;
        let tx = db.savepoint()?;
        for record in records {
            if intent(&tx, &record.intent.name)?.as_ref() != Some(&record.intent) {
                return Err(invalid("download intent changed"));
            }
        }
        // Revalidate each shared row and file before any journal mutation.
        for (name, old, m, _, _) in &observations {
            if journal_db::entry(&tx, name)? != *old {
                return Err(invalid("download entry changed"));
            }
            let after =
                folder::regular(&self.media_dir().join(name))?.ok_or_else(|| invalid("download disappeared"))?;
            if m.ino() != after.ino()
                || m.dev() != after.dev()
                || m.len() != after.len()
                || folder::stamp(m) != folder::stamp(&after)
            {
                return Err(invalid("download file changed"));
            }
        }
        for (name, old, m, hash, has_exact) in observations {
            let entry = MediaEntry {
                name: name.into(),
                sha1: Some(hash),
                size: m.len(),
                mtime_secs: m.mtime(),
                mtime_nanos: m.mtime_nsec() as u32,
                live: true,
                dirty: !has_exact && old.as_ref().is_none_or(|e| e.dirty),
                change_counter: 0,
                pending_download: false,
                sync_state: MediaSyncState::Live,
            };
            journal_db::save(&tx, &entry)?;
        }
        for record in records {
            if record.stored_name.as_ref() != Some(&record.intent.name) && journal_name(&record.intent.name) {
                let mut original = journal_db::entry(&tx, &record.intent.name)?.unwrap_or(MediaEntry {
                    name: record.intent.name.clone(),
                    sha1: None,
                    size: 0,
                    mtime_secs: 0,
                    mtime_nanos: 0,
                    live: false,
                    dirty: true,
                    change_counter: 0,
                    pending_download: false,
                    sync_state: MediaSyncState::Deleted,
                });
                if original.syncable() {
                    if folder::regular(&self.media_dir().join(&original.name))?.is_some() {
                        return Err(invalid("renamed download still has a syncable original"));
                    }
                    original.live = false;
                    original.sha1 = None;
                    original.size = 0;
                    original.mtime_secs = 0;
                    original.mtime_nanos = 0;
                }
                original.sync_state = MediaSyncState::Deleted;
                original.dirty = true;
                original.pending_download = false;
                journal_db::save(&tx, &original)?;
            }
            tx.execute("DELETE FROM media_downloads WHERE name=?1", [&record.intent.name])?;
        }
        let usn = advance_cursor(&tx, expected.usn)?;
        meguri_model::faultpoint!("media.sync.downloads.before_commit")?;
        tx.commit()?;
        Ok(usn)
    }
}
