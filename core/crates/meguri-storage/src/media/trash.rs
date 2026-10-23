use super::{MediaError, MediaStore, Result, case_fold::*, folder, journal_db};
use crate::atomic_file::move_new;
use rusqlite::{Connection, params};
use std::{fs, path::Path};

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
        use std::os::unix::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(source)?;
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
        if let Err(e) = move_new(source, &self.trash_dir().join(&entry.path)) {
            if source.exists() {
                db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
                remove_empty_parent(self.trash_dir(), &entry.path)?;
            }
            return Err(e.into());
        }
        if record_deletion {
            folder::deleted(db, name)?;
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
    pub(super) fn reconcile_trash(&self) -> Result<()> {
        let db = self.db()?;
        for entry in list(&db)? {
            if folder::regular(&self.trash_dir().join(&entry.path))?.is_none() {
                db.execute("DELETE FROM trash_entries WHERE id=?1", [entry.id])?;
            }
        }
        let mut files = Vec::new();
        for file in fs::read_dir(self.trash_dir())? {
            let file = file?;
            let Some(name) = file.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if file.file_type()?.is_dir() {
                for child in fs::read_dir(file.path())? {
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
                        folder::hash_file(&file)?,
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
