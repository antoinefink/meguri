use super::{MediaEntry, MediaError, MediaStore, MediaSyncState, Result, case_fold::*, journal_db};
use crate::atomic_file::{AtomicFile, Purpose};
use rusqlite::Connection;
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeSet,
    fs::{self, Metadata, OpenOptions},
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{fs::MetadataExt, fs::OpenOptionsExt},
    path::Path,
    sync::atomic::Ordering,
};

pub(super) fn ensure_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(MediaError::InvalidName),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(fs::create_dir(path)?),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn probe_case_sensitivity(path: &Path) -> Result<bool> {
    let probe = tempfile::Builder::new()
        .prefix(".meguri-tmp-media-case-")
        .tempfile_in(path)?;
    let name = probe
        .path()
        .file_name()
        .ok_or(MediaError::InvalidName)?
        .to_string_lossy();
    Ok(!path.join(name.to_uppercase()).exists())
}
pub(super) fn sidecar(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}
pub(super) fn stamp(m: &Metadata) -> String {
    format!("{}:{}", m.mtime(), m.mtime_nsec())
}
pub(super) fn regular(path: &Path) -> Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(m) => Ok(m.is_file().then_some(m)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}
pub(super) fn hash_file(path: &Path) -> Result<String> {
    hash_file_cancellable(path, || true)
}
fn hash_file_cancellable(path: &Path, mut keep_going: impl FnMut() -> bool) -> Result<String> {
    let mut file = open_regular(path)?;
    let mut hash = Sha1::new();
    let mut bytes = [0_u8; 65_536];
    loop {
        if !keep_going() {
            return Err(MediaError::Cancelled);
        }
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn open_regular(path: &Path) -> Result<fs::File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(MediaError::InvalidName);
    }
    Ok(file)
}
fn same_stat(entry: &MediaEntry, m: &Metadata) -> bool {
    entry.live
        && entry.size == m.len()
        && entry.mtime_secs == m.mtime()
        && i64::from(entry.mtime_nanos) == m.mtime_nsec()
}
pub(super) fn live(db: &Connection, name: &str, hash: String, m: &Metadata, force_dirty: bool) -> Result<()> {
    let old = journal_db::entry(db, name)?;
    let syncable = (1..=104_857_600).contains(&m.len());
    if !syncable && old.as_ref().is_some_and(|o| o.sync_state == MediaSyncState::Awaiting) {
        return Ok(());
    }
    let state = if syncable {
        MediaSyncState::Live
    } else if old
        .as_ref()
        .is_some_and(|o| matches!(o.sync_state, MediaSyncState::Live | MediaSyncState::Deleted))
    {
        MediaSyncState::Deleted
    } else {
        MediaSyncState::None
    };
    let dirty = old.as_ref().is_some_and(|o| o.dirty)
        || (syncable
            && (force_dirty
                || old
                    .as_ref()
                    .is_none_or(|o| o.sync_state != MediaSyncState::Live || o.sha1.as_ref() != Some(&hash))))
        || old
            .as_ref()
            .is_some_and(|o| o.sync_state == MediaSyncState::Live && !syncable);
    let entry = MediaEntry {
        name: name.into(),
        sha1: Some(hash),
        size: m.len(),
        mtime_secs: m.mtime(),
        mtime_nanos: m.mtime_nsec() as u32,
        live: true,
        dirty,
        change_counter: 0,
        pending_download: old.as_ref().is_some_and(|o| o.pending_download),
        sync_state: state,
    };
    journal_db::save(db, &entry)
}
pub(super) fn deleted(db: &Connection, name: &str) -> Result<()> {
    let Some(mut entry) = journal_db::entry(db, name)? else {
        return Ok(());
    };
    if !entry.live {
        return Ok(());
    }
    if entry.sync_state == MediaSyncState::None {
        return journal_db::remove(db, name);
    }
    if entry.sync_state == MediaSyncState::Live {
        entry.sync_state = MediaSyncState::Deleted;
        entry.dirty = true;
    }
    entry.live = false;
    entry.sha1 = None;
    entry.size = 0;
    entry.mtime_secs = 0;
    entry.mtime_nanos = 0;
    journal_db::save(db, &entry)
}

enum Publication {
    Temporary(AtomicFile),
    Existing(std::path::PathBuf),
}
impl Publication {
    fn publish(self, destination: &Path) -> std::result::Result<(), crate::StorageError> {
        match self {
            Self::Temporary(file) => file.publish_new(|_| Ok(())),
            Self::Existing(source) => crate::atomic_file::move_new(&source, destination),
        }
    }
}

impl MediaStore {
    /// A matching folder marker performs no enumeration or hashing.
    pub fn check_changes(&self) -> Result<()> {
        self.check_changes_report().map(|_| ())
    }
    pub fn check_changes_report(&self) -> Result<super::MediaCheckReport> {
        let current = stamp(&fs::metadata(self.media_dir())?);
        if journal_db::marker(&*self.db()?)?.as_deref() == Some(&current) {
            let mut report = super::MediaCheckReport::started(self.inner.clock.now_ms());
            report.completed = true;
            self.finish_check_report(&mut report)?;
            return Ok(report);
        }
        self.register_full_report(|_, _| true)
    }
    /// The callback cancels by returning false; completed records remain valid.
    pub fn register_full(&self, progress: impl FnMut(usize, usize) -> bool) -> Result<()> {
        self.register_full_report(progress).map(|_| ())
    }
    pub fn register_full_report(
        &self,
        mut progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<super::MediaCheckReport> {
        // Refuse suspended stores before touching the folder or callbacks.
        drop(self.db()?);
        let mut report = super::MediaCheckReport::started(self.inner.clock.now_ms());
        let result = self.scan(&mut progress, &mut report);
        report.completed = result.is_ok();
        self.finish_check_report(&mut report)?;
        result?;
        Ok(report)
    }
    fn scan(
        &self,
        progress: &mut impl FnMut(usize, usize) -> bool,
        report: &mut super::MediaCheckReport,
    ) -> Result<()> {
        let before = stamp(&fs::metadata(self.media_dir())?);
        let (cutoff, epoch) = {
            let db = self.db()?;
            let cutoff = journal_db::counter(&db)?;
            meguri_model::faultpoint!("media.scan.after_cutoff")?;
            (cutoff, super::sync_state::epoch(&db)?)
        };
        self.inner.scans.fetch_add(1, Ordering::Relaxed);
        report.listed = true;
        let files = fs::read_dir(self.media_dir())?.collect::<std::io::Result<Vec<_>>>()?;
        let total = files.len();
        let mut found = BTreeSet::new();
        let mut stable = true;
        for (index, file) in files.into_iter().enumerate() {
            if !progress(index, total) {
                return Err(MediaError::Cancelled);
            }
            report.seen += 1;
            let disk = file.file_name();
            let Some(disk) = disk.to_str() else {
                report.skipped += 1;
                continue;
            };
            let name = meguri_text::normalize::nfc(disk).into_owned();
            if !normalized(&name) || ignored(&name) {
                report.skipped += 1;
                continue;
            }
            let Some(m) = regular(&file.path())? else {
                report.skipped += 1;
                continue;
            };
            let old = journal_db::entry(&*self.db()?, &name)?;
            let mut path = file.path();
            if name != disk && !old.as_ref().is_some_and(|e| same_stat(e, &m)) {
                let canonical = self.media_dir().join(&name);
                match crate::atomic_file::move_new(&path, &canonical) {
                    Ok(()) => {
                        path = canonical;
                        report.renamed += 1;
                    }
                    Err(_) => {
                        // Apple can resolve both spellings to the same inode.
                        if regular(&canonical)?.is_some_and(|c| c.ino() == m.ino() && c.dev() == m.dev()) {
                            path = canonical;
                        } else {
                            report.skipped += 1;
                            continue;
                        }
                    }
                }
            }
            found.insert(name.clone());
            let same = old.as_ref().is_some_and(|o| same_stat(o, &m));
            if same
                && old
                    .as_ref()
                    .is_some_and(|o| o.sync_state == MediaSyncState::Live || !(1..=104_857_600).contains(&m.len()))
            {
                continue;
            }
            // No lock across hashing or the external progress callback.
            let hash = if same {
                old.as_ref()
                    .and_then(|e| e.sha1.clone())
                    .ok_or_else(|| MediaError::Database("missing media hash".into()))?
            } else {
                self.inner.hashes.fetch_add(1, Ordering::Relaxed);
                report.hashed += 1;
                meguri_model::faultpoint!("media.scan.before_hash")?;
                match hash_file_cancellable(&path, || progress(index, total)) {
                    Ok(hash) => hash,
                    Err(MediaError::Cancelled) => return Err(MediaError::Cancelled),
                    Err(_) if regular(&path)?.is_none() => {
                        found.remove(&name);
                        continue;
                    }
                    Err(error) => return Err(MediaError::Io(format!("{name}: {error}"))),
                }
            };
            let Some(after) = regular(&path)? else {
                found.remove(&name);
                continue;
            };
            if stamp(&after) != stamp(&m) || after.len() != m.len() || after.ino() != m.ino() || after.dev() != m.dev()
            {
                stable = false;
                continue;
            }
            let db = self.db()?;
            if journal_db::entry(&db, &name)? != old {
                stable = false;
                continue;
            }
            live(&db, &name, hash.clone(), &after, false)?;
            let new = journal_db::entry(&db, &name)?;
            if let Some(new) = new.filter(|e| old.as_ref().is_none_or(|o| o.change_counter != e.change_counter)) {
                if new.sync_state == MediaSyncState::Live {
                    if old.as_ref().is_none_or(|e| e.sync_state != MediaSyncState::Live) {
                        report.added += 1;
                    } else if old.as_ref().and_then(|e| e.sha1.as_ref()) != Some(&hash) {
                        report.changed += 1;
                    }
                } else if old.as_ref().is_some_and(|e| e.sync_state == MediaSyncState::Live) {
                    report.removed += 1;
                }
            }
        }
        if !progress(total, total) {
            return Err(MediaError::Cancelled);
        }
        let mut db = self.db()?;
        let tx = db.savepoint()?;
        for entry in journal_db::entries(&tx)? {
            if entry.live && entry.change_counter <= cutoff && !found.contains(&entry.name) {
                if entry.sync_state == MediaSyncState::Live {
                    report.removed += 1;
                }
                deleted(&tx, &entry.name)?;
            }
        }
        // Save the time read BEFORE listing, even if a rename moved it.
        // A racing reset must retain its unset marker.
        if stable && super::sync_state::epoch(&tx)? == epoch {
            journal_db::set_marker(&tx, Some(&before))?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn lookup(&self, name: &str) -> Result<Option<String>> {
        let name = meguri_text::normalize::nfc(name);
        if !safe_component(&name) || ignored(&name) {
            return Ok(None);
        }
        let exact = regular(&self.media_dir().join(name.as_ref()))?.is_some();
        if exact && self.inner.case_sensitive {
            return Ok(Some(name.into_owned()));
        }
        let db = self.db()?;
        let mut variants = Vec::new();
        for entry in journal_db::variants(&db, &name)? {
            if regular(&self.media_dir().join(&entry.name))?.is_some() {
                variants.push(entry);
            }
        }
        if exact && variants.is_empty() {
            return Ok(Some(name.into_owned()));
        }
        let key = case_key(&name);
        Ok(variants
            .iter()
            .find(|e| e.name == name)
            .or_else(|| variants.iter().find(|e| e.name == key))
            .or_else(|| variants.first())
            .map(|e| e.name.clone()))
    }
    /// Renderers read the returned descriptor, never reopen the resolved path.
    /// Symlinks and non-regular files swapped in after lookup are refused.
    pub fn open_file(&self, name: &str) -> Result<Option<fs::File>> {
        let Some(name) = self.lookup(name)? else {
            return Ok(None);
        };
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(self.media_dir())?;
        let name = std::ffi::CString::new(name).map_err(|_| MediaError::InvalidName)?;
        // The component was validated by lookup. Both the directory and
        // terminal entry are opened without following symbolic links.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: openat returned a new descriptor owned only by this File.
        let file = unsafe { fs::File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(MediaError::InvalidName);
        }
        Ok(Some(file))
    }
    pub(super) fn candidates(&self, db: &mut Connection, name: &str) -> Result<Vec<(String, String)>> {
        let mut files = journal_db::variants(db, name)?;
        files.sort_by(|a, b| a.name.cmp(&b.name));
        let mut result = Vec::new();
        for entry in files {
            let path = self.media_dir().join(&entry.name);
            let Some(m) = regular(&path)? else {
                deleted(db, &entry.name)?;
                continue;
            };
            let hash = if same_stat(&entry, &m) {
                entry.sha1.unwrap_or_default()
            } else {
                self.inner.hashes.fetch_add(1, Ordering::Relaxed);
                let hash = hash_file(&path)?;
                live(db, &entry.name, hash.clone(), &m, false)?;
                hash
            };
            result.push((entry.name, hash));
        }
        let mut q = db.prepare("SELECT name,sha1 FROM staged_files ORDER BY name")?;
        for value in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (other, hash) = value?;
            if case_key(&other) == case_key(name) {
                result.push((other, hash));
            }
        }
        Ok(result)
    }
    pub(super) fn choose(&self, db: &mut Connection, desired: &str, hash: &str) -> Result<(String, bool)> {
        let name = meguri_text::media_name::safe_media_name(desired);
        if name.is_empty() {
            return Err(MediaError::InvalidName);
        }
        if ignored(&name) {
            return Err(MediaError::IgnoredName);
        }
        let lower = lowercase_candidate(&name);
        let mut variants = self.candidates(db, &name)?;
        for value in self.candidates(db, &lower)? {
            if !variants.contains(&value) {
                variants.push(value);
            }
        }
        let same = variants
            .iter()
            .filter(|(_, h)| h == hash)
            .map(|(n, _)| n)
            .min_by_key(|n| {
                (
                    if **n == name {
                        0
                    } else if **n == lower {
                        1
                    } else {
                        2
                    },
                    n.as_bytes(),
                )
            });
        if let Some(same) = same {
            return Ok((same.clone(), false));
        }
        if !variants.iter().any(|(n, _)| case_key(n) == case_key(&lower)) {
            return Ok((lower, true));
        }
        Ok((hash_name(&lower, hash), true))
    }
    pub(super) fn write_bytes(&self, destination: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = AtomicFile::new(destination, Purpose::Media)?;
        file.file_mut().write_all(bytes)?;
        file.publish_new(|_| Ok(())).map_err(MediaError::from)
    }
    fn install_locked(
        &self,
        db: &mut Connection,
        name: &str,
        bytes: &[u8],
        replacements: &[(String, String)],
    ) -> Result<()> {
        let target = self.media_dir().join(name);
        let marker = stamp(&fs::metadata(self.media_dir())?);
        let mut file = AtomicFile::new(&target, Purpose::Media)?;
        file.file_mut().write_all(bytes)?;
        self.install_publication_locked(
            db,
            name,
            hash_bytes(bytes),
            Publication::Temporary(file),
            replacements,
            &marker,
        )
    }
    pub(super) fn install_existing_locked(
        &self,
        db: &mut Connection,
        name: &str,
        source: &Path,
        hash: &str,
        replacements: &[(String, String)],
    ) -> Result<()> {
        let marker = stamp(&fs::metadata(self.media_dir())?);
        self.install_publication_locked(
            db,
            name,
            hash.to_owned(),
            Publication::Existing(source.to_owned()),
            replacements,
            &marker,
        )
    }
    fn install_publication_locked(
        &self,
        db: &mut Connection,
        name: &str,
        hash: String,
        publication: Publication,
        replacements: &[(String, String)],
        marker: &str,
    ) -> Result<()> {
        let target = self.media_dir().join(name);
        db.execute_batch("SAVEPOINT media_install")?;
        let outcome = (|| {
            let mut moved = Vec::new();
            let result = (|| {
                for (old, old_hash) in replacements {
                    let Some(m) = regular(&self.media_dir().join(old))? else {
                        continue;
                    };
                    let entry = self.trash_one_locked(db, old, old_hash, &m)?;
                    moved.push((old.clone(), entry));
                }
                publication.publish(&target)?;
                live(db, name, hash.clone(), &fs::symlink_metadata(&target)?, true)?;
                self.advance_marker(db, marker)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    db.execute_batch("RELEASE media_install")?;
                    Ok(())
                }
                Err(error) => {
                    let published = !matches!(error, MediaError::NameTaken)
                        && regular(&target)?.is_some()
                        && hash_file(&target)? == hash;
                    if published {
                        // A rename can succeed before directory sync fails. Keep
                        // both new bytes and the old version's trash metadata.
                        let recorded = live(db, name, hash, &fs::symlink_metadata(&target)?, true);
                        db.execute_batch("RELEASE media_install")?;
                        journal_db::set_marker(db, None)?;
                        recorded?;
                    } else {
                        let mut restored = true;
                        for (old, entry) in moved.into_iter().rev() {
                            if self.restore_original_locked(db, &entry, &old).is_err() {
                                restored = false;
                            }
                        }
                        if restored {
                            db.execute_batch("ROLLBACK TO media_install; RELEASE media_install")?;
                        } else {
                            db.execute_batch("RELEASE media_install")?;
                            journal_db::set_marker(db, None)?;
                            return Err(MediaError::Io(
                                "media publication failed; prior bytes remain in the trash".into(),
                            ));
                        }
                    }
                    Err(error)
                }
            }
        })();
        // No error path may leave a journal transaction open for later ops.
        if !db.is_autocommit() {
            db.execute_batch("ROLLBACK")?;
        }
        outcome
    }
    pub fn add(&self, desired: &str, bytes: &[u8]) -> Result<String> {
        self.check_changes()?;
        let hash = hash_bytes(bytes);
        let hashed = hash_name(
            &lowercase_candidate(&meguri_text::media_name::safe_media_name(desired)),
            &hash,
        );
        // M9: decide again after registration, then use the hash name.
        for attempt in 0..3 {
            let result = {
                let mut db = self.db()?;
                let (name, write) = if attempt == 2 {
                    (hashed.clone(), true)
                } else {
                    self.choose(&mut db, desired, &hash)?
                };
                if !write && regular(&self.media_dir().join(&name))?.is_some() {
                    return Ok(name);
                }
                let mut replacements = Vec::new();
                if name == hashed {
                    let mut variants = self.candidates(&mut db, &name)?;
                    if regular(&self.media_dir().join(&name))?.is_some() && !variants.iter().any(|(n, _)| n == &name) {
                        variants.push((name.clone(), hash_file(&self.media_dir().join(&name))?));
                    }
                    if let Some((found, _)) = variants
                        .iter()
                        .filter(|(_, h)| h == &hash)
                        .min_by_key(|(n, _)| (n != &name, n.as_bytes()))
                        && regular(&self.media_dir().join(found))?.is_some()
                    {
                        return Ok(found.clone());
                    }
                    replacements = variants.into_iter().filter(|(_, h)| h != &hash).collect();
                }
                self.install_locked(&mut db, &name, bytes, &replacements).map(|_| name)
            };
            match result {
                Err(MediaError::NameTaken) if attempt < 2 => self.register_full(|_, _| true)?,
                other => return other,
            }
        }
        Err(MediaError::NameTaken)
    }
    pub fn ensure_static_media(&self, name: &str, bytes: &[u8]) -> Result<String> {
        if !normalized(name) {
            return Err(MediaError::InvalidName);
        }
        if ignored(name) {
            return Err(MediaError::IgnoredName);
        }
        self.check_changes()?;
        let mut db = self.db()?;
        let hash = hash_bytes(bytes);
        let candidates = self.candidates(&mut db, name)?;
        if let Some((found, _)) = candidates.iter().find(|(_, h)| h == &hash)
            && regular(&self.media_dir().join(found))?.is_some()
        {
            return Ok(found.clone());
        }
        if candidates
            .iter()
            .any(|(n, h)| h != &hash && !self.media_dir().join(n).exists())
        {
            return Err(MediaError::NameTaken);
        }
        let replacements = candidates.into_iter().filter(|(_, h)| h != &hash).collect::<Vec<_>>();
        self.install_locked(&mut db, name, bytes, &replacements)?;
        Ok(name.into())
    }
    pub(super) fn advance_marker(&self, db: &Connection, before: &str) -> Result<()> {
        if journal_db::marker(db)?.as_deref() == Some(before) {
            journal_db::set_marker(db, Some(&stamp(&fs::metadata(self.media_dir())?)))?;
        }
        Ok(())
    }
}
