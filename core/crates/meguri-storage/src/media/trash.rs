use super::{MediaError, MediaStore, Result, case_fold::*, folder, journal_db};
use crate::atomic_file::move_new;
use rusqlite::{Connection, params};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashEntry {
    pub id: i64,
    pub name: String,
    pub sha1: String,
    pub size: u64,
    pub trashed_ms: i64,
    pub(super) path: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrashSummary {
    pub entries: usize,
    pub bytes: u64,
}

fn identity(path: &str) -> Option<(i64, i64, &str)> {
    let (prefix, name) = path.split_once('/').or_else(|| path.split_once("--"))?;
    if !safe_component(prefix) || !safe_component(name) {
        return None;
    }
    let (time, id) = prefix.rsplit_once('-')?;
    let id: i64 = id.parse().ok()?;
    (id > 0).then_some((time.parse().ok()?, id, name))
}
fn remove_empty_parent(root: &Path, relative: &str) -> Result<()> {
    if let Some((parent, _)) = relative.split_once('/') {
        // The payload is already restored/deleted (or was never moved).
        if let Some(directory) = receipt_directory(&root.join(parent))? {
            match fs::remove_file(directory.join(REMOVAL_INTENT)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            match fs::remove_dir(directory) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(e) => return Err(e.into()),
            }
            crate::atomic_file::sync_directory(&root.join(parent))?;
        }
        match fs::remove_dir(root.join(parent)) {
            Ok(()) => crate::atomic_file::sync_directory(root)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound || e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn list(db: &Connection) -> Result<Vec<TrashEntry>> {
    let entries = db.prepare("SELECT id,name,sha1,size,trashed_ms,path FROM trash_entries WHERE path IS NOT NULL ORDER BY trashed_ms DESC,id DESC")?
        .query_map([], |r|Ok(TrashEntry{id:r.get(0)?,name:r.get(1)?,sha1:r.get(2)?,size:journal_db::unsigned(r,3)?,trashed_ms:r.get(4)?,path:r.get(5)?}))?
        .collect::<std::result::Result<Vec<_>,_>>()?;
    for entry in &entries {
        if !safe_component(&entry.name) || identity(&entry.path).is_none_or(|(_, _, name)| name != entry.name) {
            return Err(MediaError::InvalidManifest("invalid trash path".into()));
        }
    }
    Ok(entries)
}
pub(super) fn cloud_move_entries(db: &Connection) -> Result<Vec<TrashEntry>> {
    list(db)
}
impl MediaStore {
    pub fn trash_entries(&self) -> Result<Vec<TrashEntry>> {
        list(&*self.db()?)
    }
    pub fn trash_summary(&self) -> Result<TrashSummary> {
        let entries = self.trash_entries()?;
        Ok(TrashSummary {
            entries: entries.len(),
            bytes: entries.iter().map(|e| e.size).sum(),
        })
    }
    pub(super) fn trash_one_locked(
        &self,
        db: &mut Connection,
        name: &str,
        hash: &str,
        m: &fs::Metadata,
    ) -> Result<TrashEntry> {
        self.move_to_trash_locked(db, name, hash, m.len(), &self.media_dir().join(name), true)
    }
    pub(super) fn move_to_trash_locked(
        &self,
        db: &mut Connection,
        name: &str,
        hash: &str,
        size: u64,
        source: &Path,
        record_deletion: bool,
    ) -> Result<TrashEntry> {
        self.move_to_trash_observed_locked(db, name, hash, size, source, record_deletion, &mut false)
    }
    // Report a retained file move even if a subsequent directory sync fails.
    // A vanished source alone is not evidence: match the held inode in trash.
    #[allow(clippy::too_many_arguments)] // Extends the common move with its retained effect.
    pub(super) fn move_to_trash_observed_locked(
        &self,
        db: &mut Connection,
        name: &str,
        hash: &str,
        size: u64,
        source: &Path,
        record_deletion: bool,
        moved: &mut bool,
    ) -> Result<TrashEntry> {
        self.move_to_trash_checked_locked(db, name, hash, size, source, record_deletion, moved, None)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn move_to_trash_checked_locked(
        &self,
        db: &mut Connection,
        name: &str,
        hash: &str,
        size: u64,
        source: &Path,
        record_deletion: bool,
        moved: &mut bool,
        verify: Option<&dyn Fn(&Path) -> Result<bool>>,
    ) -> Result<TrashEntry> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(source)?;
        if !file.metadata()?.is_file() {
            return Err(MediaError::InvalidName);
        }
        crate::atomic_file::full_sync(&file)?;
        let now = self.inner.clock.now_ms();
        let entry = loop {
            db.execute(
                "INSERT INTO trash_entries(name,sha1,size,trashed_ms,path) VALUES(?1,?2,?3,?4,NULL)",
                params![name, hash, journal_db::sqlite_integer(size)?, now],
            )?;
            let id = db.last_insert_rowid();
            // The full original basename may already use NAME_MAX. Put the
            // identity in its parent so the bytes remain self-describing.
            let parent = format!("{now}-{id}");
            match fs::create_dir(self.trash_dir().join(&parent)) {
                Ok(()) => crate::atomic_file::sync_directory(self.trash_dir())?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    db.execute("DELETE FROM trash_entries WHERE id=?1", [id])?;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
            let path = format!("{parent}/{name}");
            db.execute("UPDATE trash_entries SET path=?1 WHERE id=?2", params![path, id])?;
            break TrashEntry {
                id,
                name: name.into(),
                sha1: hash.into(),
                size,
                trashed_ms: now,
                path,
            };
        };
        let destination = self.trash_dir().join(&entry.path);
        let identity = file.metadata()?;
        // SQL here may be inside a caller's savepoint. A separately synced
        // filesystem receipt must precede the rename; uncommitted SQL cannot
        // preserve its cause across rollback or process death.
        if source == self.media_dir().join(name) {
            self.prepare_removal_intent(db, &entry, &identity)?;
        }
        meguri_model::faultpoint!("media.trash.before_move")?;
        if let Some(verify) = verify {
            let checked: Result<bool> = (|| {
                Ok(
                    folder::regular(source)?.is_some_and(|m| folder::unchanged_file(&identity, &m))
                        && verify(source)?
                        && folder::unchanged_file(&identity, &file.metadata()?),
                )
            })();
            if !matches!(checked, Ok(true)) {
                db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
                remove_empty_parent(self.trash_dir(), &entry.path)?;
                checked?;
                return Err(MediaError::NameTaken);
            }
        }
        let outcome = move_new(source, &destination);
        *moved = fs::symlink_metadata(&destination)
            .is_ok_and(|m| m.is_file() && m.dev() == identity.dev() && m.ino() == identity.ino());
        if verify.is_some() && *moved {
            meguri_model::faultpoint!("media.trash.after_move_before_verify")?;
        }
        if let Some(verify) = verify.filter(|_| outcome.is_ok() || destination.exists()) {
            let checked = verify(&destination);
            if !*moved || !matches!(checked, Ok(true)) {
                // Restore with the observed facts; the expected version no longer owns these bytes.
                let metadata = fs::symlink_metadata(&destination)?;
                let actual_hash = folder::hash_file(&destination)?;
                let mut actual = entry.clone();
                actual.sha1 = actual_hash;
                actual.size = metadata.len();
                db.execute(
                    "UPDATE trash_entries SET sha1=?2,size=?3 WHERE id=?1",
                    params![actual.id, actual.sha1, journal_db::sqlite_integer(actual.size)?],
                )?;
                self.restore_original_locked(db, &actual, name).map_err(|_| {
                    MediaError::Io("changed media retained in trash; restoration needs recovery".into())
                })?;
                *moved = false;
                checked?;
                return Err(MediaError::NameTaken);
            }
        }
        // Deterministic crash boundary: durable move, no observation SQL yet.
        if *moved {
            meguri_model::faultpoint!("media.trash.after_move_before_observation")?;
        }
        let source_absent =
            *moved && fs::symlink_metadata(source).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        if *moved && source_absent && source == self.media_dir().join(name) {
            db.execute_batch("SAVEPOINT media_trash_cause")?;
            let recorded: Result<()> = (|| {
                super::feed::observe_removal(db, name)?;
                if outcome.is_ok() && record_deletion {
                    folder::deleted(db, name)?;
                }
                Ok(())
            })();
            if recorded.is_err() {
                db.execute_batch("ROLLBACK TO media_trash_cause")?;
            }
            db.execute_batch("RELEASE media_trash_cause")?;
            recorded?;
        }
        if let Err(e) = outcome {
            if source.exists() {
                db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
                remove_empty_parent(self.trash_dir(), &entry.path)?;
            }
            return Err(e.into());
        }
        Ok(entry)
    }
    pub(super) fn restore_original_locked(&self, db: &mut Connection, entry: &TrashEntry, name: &str) -> Result<()> {
        let target = self.media_dir().join(name);
        move_new(&self.trash_dir().join(&entry.path), &target)?;
        remove_empty_parent(self.trash_dir(), &entry.path)?;
        folder::live(db, name, entry.sha1.clone(), &fs::symlink_metadata(target)?, false)?;
        db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
        Ok(())
    }
    pub fn trash(&self, names: &[String]) -> Result<()> {
        self.check_changes()?;
        for requested in names {
            let found = self.lookup(requested)?;
            let mut db = self.db()?;
            let marker = folder::stamp(&fs::metadata(self.media_dir())?);
            if let Some(name) = found {
                if let Some(m) = folder::regular(&self.media_dir().join(&name))? {
                    let hash = folder::hash_file(&self.media_dir().join(&name))?;
                    self.trash_one_locked(&mut db, &name, &hash, &m)?;
                }
            } else {
                folder::deleted(&db, requested)?;
            }
            self.advance_marker(&db, &marker)?;
        }
        Ok(())
    }
    pub fn restore(&self) -> Result<()> {
        for entry in self.trash_entries()? {
            self.check_changes()?;
            let mut db = self.db()?;
            let source = self.trash_dir().join(&entry.path);
            if folder::regular(&source)?.is_none() {
                continue;
            }
            let marker = folder::stamp(&fs::metadata(self.media_dir())?);
            let (name, _write) = if self.candidates(&mut db, &entry.name)?.is_empty()
                && folder::regular(&self.media_dir().join(&entry.name))?.is_none()
            {
                (entry.name.clone(), true)
            } else {
                self.choose(&mut db, &entry.name, &entry.sha1)?
            };
            let target = self.media_dir().join(&name);
            if folder::regular(&target)?.is_some() && folder::hash_file(&target)? == entry.sha1 {
                fs::remove_file(&source)?;
            } else {
                let mut variants = self.candidates(&mut db, &name)?;
                if folder::regular(&target)?.is_some() && !variants.iter().any(|(n, _)| n == &name) {
                    variants.push((name.clone(), folder::hash_file(&target)?));
                }
                let expected_hash_name = hash_name(
                    &lowercase_candidate(&meguri_text::media_name::safe_media_name(&entry.name)),
                    &entry.sha1,
                );
                let replacements = if name == expected_hash_name {
                    variants
                        .into_iter()
                        .filter(|(_, h)| h != &entry.sha1)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let outcome = self.install_existing_locked(&mut db, &name, &source, &entry.sha1, &replacements);
                if !source.exists() {
                    db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
                }
                outcome?;
            }
            db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
            remove_empty_parent(self.trash_dir(), &entry.path)?;
            self.advance_marker(&db, &marker)?;
        }
        Ok(())
    }
    /// Only a user request calls this; no timer or open path empties the trash.
    pub fn empty_trash(&self) -> Result<()> {
        let db = self.db()?;
        for entry in list(&db)? {
            let path = self.trash_dir().join(&entry.path);
            if path.symlink_metadata().is_ok() {
                fs::remove_file(&path)?;
            }
            db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
            remove_empty_parent(self.trash_dir(), &entry.path)?;
        }
        crate::atomic_file::sync_directory(self.trash_dir())?;
        Ok(())
    }
    pub(super) fn reconcile_trash_checked(&self, cancel: &dyn Fn() -> bool) -> Result<()> {
        let db = self.db()?;
        for entry in list(&db)? {
            if cancel() {
                return Err(super::MediaError::Cancelled);
            }
            if folder::regular(&self.trash_dir().join(&entry.path))?.is_none() {
                db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
            }
        }
        let mut files = Vec::new();
        for file in fs::read_dir(self.trash_dir())? {
            if cancel() {
                return Err(super::MediaError::Cancelled);
            }
            let file = file?;
            let Some(name) = file.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if file.file_type()?.is_dir() {
                for child in fs::read_dir(file.path())? {
                    if cancel() {
                        return Err(super::MediaError::Cancelled);
                    }
                    let child = child?;
                    let Some(child_name) = child.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let path = format!("{name}/{child_name}");
                    if identity(&path).is_some() {
                        files.push(path);
                    }
                }
            } else if identity(&name).is_some() {
                // Preserve journals created with the earlier flat layout.
                files.push(name);
            }
        }
        files.sort_by_key(|name| {
            let key = identity(name)
                .map(|(time, id, _)| (time, id))
                .unwrap_or((i64::MIN, i64::MIN));
            (key, name.clone())
        });
        for path in files {
            let Some((time, _, name)) = identity(&path) else {
                continue;
            };
            let file = self.trash_dir().join(&path);
            let Some(m) = folder::regular(&file)? else {
                continue;
            };
            let exists: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM trash_entries WHERE path=?1)",
                [&path],
                |r| r.get(0),
            )?;
            if !exists {
                db.execute(
                    "INSERT INTO trash_entries(name,sha1,size,trashed_ms,path) VALUES(?1,?2,?3,?4,?5)",
                    params![
                        name,
                        folder::hash_file_cancellable(&file, || !cancel())?,
                        journal_db::sqlite_integer(m.len())?,
                        time,
                        path
                    ],
                )?;
            }
        }
        db.execute("DELETE FROM trash_entries WHERE path IS NULL", [])?;
        Ok(())
    }
}

const REMOVAL_DIRECTORIES: [&str; 2] = [".meguri-removal", ".meguri-removal-alt"];
const REMOVAL_INTENT: &str = "intent.json";
fn receipt_directory(parent: &Path) -> Result<Option<std::path::PathBuf>> {
    let mut found = None;
    for name in REMOVAL_DIRECTORIES {
        let path = parent.join(name);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
                if found.is_some() {
                    return Err(MediaError::InvalidManifest("ambiguous media removal receipt".into()));
                }
                found = Some(path);
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(found)
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalIntent {
    version: u8,
    origin: String,
    path: String,
    name: String,
    feed_id: [u8; 16],
    counter: u64,
    entry_counter: Option<u64>,
    journal_native: (u64, u64),
    source_native: (u64, u64),
    size: u64,
    mtime_secs: i64,
    mtime_nanos: i64,
}
impl MediaStore {
    pub(super) fn load_removal_intents(&self) -> Result<()> {
        let mut pending = self
            .inner
            .removal_intents
            .lock()
            .map_err(|_| MediaError::Database("media intent mutex poisoned".into()))?;
        for entry in fs::read_dir(self.trash_dir())? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && receipt_directory(&entry.path())?.is_some()
                && let Some(name) = entry.file_name().to_str()
                && safe_component(name)
            {
                pending.insert(name.to_owned());
            }
        }
        Ok(())
    }

    fn prepare_removal_intent(&self, db: &Connection, entry: &TrashEntry, m: &fs::Metadata) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let entry_counter = match super::feed::independent(db, &entry.name)? {
            Some((counter, _)) => Some(counter),
            None => journal_db::entry(db, &entry.name)?.map(|e| e.change_counter),
        };
        let counter = entry_counter.unwrap_or(journal_db::counter(db)?);
        let (parent, _) = entry.path.split_once('/').ok_or(MediaError::InvalidName)?;
        let receipt = RemovalIntent {
            version: 1,
            origin: "collection.media".into(),
            path: entry.path.clone(),
            name: entry.name.clone(),
            feed_id: super::feed::status(db)?.feed_id.0,
            counter,
            entry_counter,
            journal_native: self.inner.journal_native,
            source_native: (m.dev(), m.ino()),
            size: m.len(),
            mtime_secs: m.mtime(),
            mtime_nanos: m.mtime_nsec(),
        };
        let metadata_name = if entry.name == REMOVAL_DIRECTORIES[0] {
            REMOVAL_DIRECTORIES[1]
        } else {
            REMOVAL_DIRECTORIES[0]
        };
        let directory = self.trash_dir().join(parent).join(metadata_name);
        fs::create_dir(&directory)?;
        crate::atomic_file::sync_directory(&self.trash_dir().join(parent))?;
        let path = directory.join(REMOVAL_INTENT);
        self.inner
            .removal_intents
            .lock()
            .map_err(|_| MediaError::Database("media intent mutex poisoned".into()))?
            .insert(parent.into());
        let mut output = crate::atomic_file::AtomicFile::new(&path, crate::atomic_file::Purpose::Media)?;
        output
            .file_mut()
            .write_all(&serde_json::to_vec(&receipt).map_err(|e| MediaError::InvalidManifest(e.to_string()))?)?;
        output.publish_new(|_| Ok(()))?;
        Ok(())
    }

    /// Called under the shared journal guard before another operation may
    /// change a protocol stamp. Receipts remain until committed SQL or physical
    /// restoration makes them obsolete, so outer SQL rollback is harmless.
    pub(super) fn recover_removal_intents(&self, db: &Connection) -> Result<()> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let mut pending = self
            .inner
            .removal_intents
            .lock()
            .map_err(|_| MediaError::Database("media intent mutex poisoned".into()))?;
        let mut settled = Vec::new();
        for parent in pending.iter() {
            let parent_directory = self.trash_dir().join(parent);
            if !fs::symlink_metadata(&parent_directory).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
                settled.push(parent.clone());
                continue;
            }
            let Some(directory) = receipt_directory(&parent_directory)? else {
                settled.push(parent.clone());
                continue;
            };
            let path = directory.join(REMOVAL_INTENT);
            let file = match fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)
            {
                Ok(file) if file.metadata()?.is_file() && file.metadata()?.len() <= 16384 => file,
                Ok(_) => return Err(MediaError::InvalidManifest("invalid media removal receipt".into())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    settled.push(parent.clone());
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let intent: RemovalIntent =
                serde_json::from_reader(file).map_err(|e| MediaError::InvalidManifest(e.to_string()))?;
            if intent.version != 1
                || intent.origin != "collection.media"
                || !safe_component(&intent.name)
                || intent.entry_counter.is_some_and(|c| c != intent.counter)
                || intent.path != format!("{parent}/{}", intent.name)
                || identity(&intent.path).is_none_or(|(_, _, name)| name != intent.name)
            {
                return Err(MediaError::InvalidManifest(
                    "invalid media removal receipt ownership".into(),
                ));
            }
            let current = super::feed::status(db)?;
            let row = journal_db::entry(db, &intent.name)?;
            let independent = super::feed::independent(db, &intent.name)?;
            let stamp = independent
                .as_ref()
                .map(|(c, _)| *c)
                .or_else(|| row.as_ref().map(|e| e.change_counter));
            let already_observed: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM media WHERE name=?1 AND live=1 AND explicit_removal=1
                    UNION ALL SELECT 1 FROM media_feed_facts WHERE name=?1 AND live=1 AND explicit_removal=1)",
                [&intent.name],
                |r| r.get(0),
            )?;
            let moved = folder::regular(&self.trash_dir().join(&intent.path))?.is_some_and(|m| {
                (m.dev(), m.ino()) == intent.source_native
                    && m.len() == intent.size
                    && m.mtime() == intent.mtime_secs
                    && m.mtime_nsec() == intent.mtime_nanos
            });
            // A missing source, an old namesake in trash, or matching bytes
            // alone never prove a move of this receipt's original inode.
            let source_absent = match fs::symlink_metadata(self.media_dir().join(&intent.name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                Err(e) => return Err(e.into()),
                Ok(_) => false,
            };
            let valid = current.feed_id.0 == intent.feed_id
                && self.inner.journal_native == intent.journal_native
                && intent.counter <= current.counter
                && moved
                && source_absent;
            if valid && (stamp == intent.entry_counter || already_observed) {
                super::feed::observe_removal(db, &intent.name)?;
            } else if db.is_autocommit() {
                if !source_absent
                    && current.feed_id.0 == intent.feed_id
                    && self.inner.journal_native == intent.journal_native
                {
                    db.execute_batch("SAVEPOINT media_restored_cause")?;
                    let clear: Result<()> = (|| {
                        db.execute("UPDATE media SET explicit_removal=0 WHERE name=?1", [&intent.name])?;
                        db.execute(
                            "UPDATE media_feed_facts SET explicit_removal=0 WHERE name=?1",
                            [&intent.name],
                        )?;
                        Ok(())
                    })();
                    if clear.is_err() {
                        db.execute_batch("ROLLBACK TO media_restored_cause")?;
                    }
                    db.execute_batch("RELEASE media_restored_cause")?;
                    clear?;
                }
                // Never delete a receipt based on uncommitted SQL, which an
                // outer savepoint might still discard. Physical restoration
                // and a committed later stamp both invalidate this ownership.
                match fs::remove_file(&path) {
                    Ok(()) => {
                        fs::remove_dir(&directory)?;
                        crate::atomic_file::sync_directory(&parent_directory)?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                settled.push(parent.clone());
            }
        }
        for parent in settled {
            pending.remove(&parent);
        }
        Ok(())
    }
}

#[cfg(test)]
mod feed_rollback_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn media_feed_unregistered_original_move_recovers_before_complete_registration() {
        let root = tempfile::tempdir().unwrap();
        let random = Arc::new(meguri_model::rng::SeededRandom::new(7));
        let clock = Arc::new(meguri_model::clock::FixedClock::new(
            0,
            meguri_model::clock::UtcOffset::UTC,
        ));
        let media = MediaStore::open(&root.path().join("collection.anki2"), clock.clone(), random.clone()).unwrap();
        fs::write(media.media_dir().join("unregistered.png"), b"original").unwrap();
        {
            let mut db = media.db().unwrap();
            let m = fs::metadata(media.media_dir().join("unregistered.png")).unwrap();
            meguri_model::faultpoint::arm(
                "media.trash.after_move_before_observation",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
            let result = media.trash_one_locked(&mut db, "unregistered.png", &folder::hash_bytes(b"original"), &m);
            meguri_model::faultpoint::disarm("media.trash.after_move_before_observation");
            assert!(result.is_err());
            assert!(journal_db::entries(&db).unwrap().is_empty());
        }
        drop(media);
        let media = MediaStore::open(&root.path().join("collection.anki2"), clock, random).unwrap();
        let snapshot = media.media_feed_snapshot().unwrap();
        assert!(!snapshot.status.complete);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(
            snapshot.items[0].state,
            super::super::MediaFileState::Absent(super::super::MediaAbsenceCause::Removed)
        );
        assert_eq!(media.media_feed_snapshot().unwrap(), snapshot);
    }

    #[test]
    fn media_feed_outer_sql_rollback_keeps_durable_move_ownership() {
        let root = tempfile::tempdir().unwrap();
        let random = Arc::new(meguri_model::rng::SeededRandom::new(7));
        let clock = Arc::new(meguri_model::clock::FixedClock::new(
            0,
            meguri_model::clock::UtcOffset::UTC,
        ));
        let media = MediaStore::open(&root.path().join("collection.anki2"), clock.clone(), random.clone()).unwrap();
        media.add("a.png", b"original").unwrap();
        let status = media.media_feed_status().unwrap();
        media
            .set_media_feed_floor(status.feed_id, Some(status.counter))
            .unwrap();
        let before = media.entries().unwrap();
        {
            let mut db = media.db().unwrap();
            db.execute_batch("SAVEPOINT protocol_outer").unwrap();
            let m = fs::metadata(media.media_dir().join("a.png")).unwrap();
            media
                .trash_one_locked(&mut db, "a.png", before[0].sha1.as_deref().unwrap(), &m)
                .unwrap();
            assert!(!journal_db::entry(&db, "a.png").unwrap().unwrap().live);
            db.execute_batch("ROLLBACK TO protocol_outer; RELEASE protocol_outer")
                .unwrap();
            assert_eq!(journal_db::entries(&db).unwrap(), before);
            assert!(
                !db.query_row("SELECT explicit_removal FROM media WHERE name='a.png'", [], |r| r
                    .get::<_, bool>(0))
                    .unwrap()
            );
        }
        // Both trash SQL and cause SQL rolled back; only the synced filesystem
        // intent and the exact original inode survived.
        drop(media);
        let media = MediaStore::open(&root.path().join("collection.anki2"), clock, random).unwrap();
        media.register_full(|_, _| true).unwrap();
        let snapshot = media.media_feed_snapshot().unwrap();
        assert_eq!(
            snapshot.items[0].state,
            super::super::MediaFileState::Absent(super::super::MediaAbsenceCause::Removed)
        );
        assert!(snapshot.items[0].change_counter > status.counter);
        media.register_full(|_, _| true).unwrap();
        assert_eq!(media.media_feed_snapshot().unwrap(), snapshot);
    }
}
