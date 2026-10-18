use super::{MediaError, MediaStore, Result, case_fold::*, folder, journal_db};
use crate::atomic_file::move_new;
use meguri_model::note::Note;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteReference {
    pub note_id: i64,
    pub mtime: i64,
    pub fields_sha256: String,
}
impl NoteReference {
    pub fn of(note: &Note) -> Self {
        Self {
            note_id: note.id.0,
            mtime: note.mtime.0,
            fields_sha256: Sha256::digest(note.fields.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        }
    }
    pub fn matches(&self, note: &Note) -> bool {
        self == &Self::of(note)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub published: usize,
    pub trashed: usize,
}
#[derive(Debug)]
pub struct StagedOp {
    store: MediaStore,
    id: i64,
    finished: bool,
}
#[derive(Debug)]
struct FileRecord {
    idx: i64,
    name: String,
    sha1: String,
    path: String,
    replace_existing: bool,
}
fn files(db: &Connection, id: i64) -> Result<Vec<FileRecord>> {
    Ok(db
        .prepare("SELECT idx,name,sha1,path,replace_existing FROM staged_files WHERE op_id=?1 ORDER BY idx")?
        .query_map([id], |r| {
            Ok(FileRecord {
                idx: r.get(0)?,
                name: r.get(1)?,
                sha1: r.get(2)?,
                path: r.get(3)?,
                replace_existing: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
fn validate(file: &FileRecord) -> Result<()> {
    if !safe_component(&file.name)
        || !safe_component(&file.path)
        || file.sha1.len() != 40
        || !file.sha1.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(MediaError::InvalidManifest("invalid staged file manifest".into()));
    }
    Ok(())
}
fn delete_manifest(db: &Connection, id: i64) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    tx.execute("DELETE FROM staged_files WHERE op_id=?1", [id])?;
    tx.execute("DELETE FROM staged_ops WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(())
}
impl StagedOp {
    pub fn add(&mut self, desired: &str, bytes: &[u8]) -> Result<String> {
        let mut db = self.store.db()?;
        let hash = folder::hash_bytes(bytes);
        let (mut name, write) = self.store.choose(&mut db, desired, &hash)?;
        if !write {
            let own: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM staged_files WHERE op_id=?1 AND name=?2)",
                params![self.id, name],
                |r| r.get(0),
            )?;
            if own || folder::regular(&self.store.media_dir().join(&name))?.is_some() {
                return Ok(name);
            }
            // Another uncommitted op owns the same bytes: keep our own copy,
            // so its rollback cannot remove this operation's attachment.
        }
        let hash_name = hash_name(
            &lowercase_candidate(&meguri_text::media_name::safe_media_name(desired)),
            &hash,
        );
        let mut target = self.store.media_dir().join(&name);
        if target.symlink_metadata().is_ok() && folder::regular(&target)?.is_none() {
            name = hash_name.clone();
            target = self.store.media_dir().join(&name);
        }
        let replace_existing = if folder::regular(&target)?.is_some() {
            if folder::hash_file(&target)? == hash {
                return Ok(name);
            }
            if name != hash_name {
                return Err(MediaError::NameTaken);
            }
            true
        } else {
            false
        };
        let index: i64 = db.query_row(
            "SELECT COALESCE(MAX(idx),0)+1 FROM staged_files WHERE op_id=?1",
            [self.id],
            |r| r.get(0),
        )?;
        let path = format!("{}-{index}--{name}", self.id);
        self.store.write_bytes(&self.store.staging_dir().join(&path), bytes)?;
        meguri_model::faultpoint!("media.stage.after_write")?;
        db.execute(
            "INSERT INTO staged_files(op_id,idx,name,sha1,size,path,replace_existing) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                self.id,
                index,
                name,
                hash,
                journal_db::sqlite_integer(bytes.len() as u64)?,
                path,
                replace_existing
            ],
        )?;
        Ok(name)
    }
    pub fn names(&self) -> Result<Vec<String>> {
        Ok(files(&*self.store.db()?, self.id)?
            .into_iter()
            .map(|f| f.name)
            .collect())
    }
    pub fn set_references(&mut self, references: &[NoteReference]) -> Result<()> {
        if references.is_empty()
            || references.iter().any(|r| {
                r.note_id <= 0 || r.fields_sha256.len() != 64 || !r.fields_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err(MediaError::InvalidManifest(
                "staged media needs valid note post-images".into(),
            ));
        }
        let refs = serde_json::to_string(references).map_err(|e| MediaError::InvalidManifest(e.to_string()))?;
        self.store
            .db()?
            .execute("UPDATE staged_ops SET refs=?1 WHERE id=?2", params![refs, self.id])?;
        Ok(())
    }
    /// Call only after the collection commit. An error retains recovery data.
    pub fn publish(mut self) -> Result<usize> {
        self.finished = true;
        self.store.publish_stage(self.id)
    }
    pub fn rollback(mut self) -> Result<()> {
        self.finished = true;
        self.store.abort_stage(self.id)
    }
}
impl Drop for StagedOp {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.store.abort_stage(self.id);
        }
    }
}
impl MediaStore {
    pub fn begin_staging(&self) -> Result<StagedOp> {
        self.check_changes()?;
        let db = self.db()?;
        db.execute("INSERT INTO staged_ops DEFAULT VALUES", [])?;
        Ok(StagedOp {
            store: self.clone(),
            id: db.last_insert_rowid(),
            finished: false,
        })
    }
    fn abort_stage(&self, id: i64) -> Result<()> {
        let db = self.db()?;
        for entry in fs::read_dir(self.staging_dir())? {
            let entry = entry?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|s| s.starts_with(&format!("{id}-")))
                && entry.file_type()?.is_file()
            {
                fs::remove_file(entry.path())?;
            }
        }
        crate::atomic_file::sync_directory(self.staging_dir())?;
        delete_manifest(&db, id)
    }
    fn publish_stage(&self, id: i64) -> Result<usize> {
        let mut db = self.db()?;
        let records = files(&db, id)?;
        if !records.is_empty() {
            let refs: String = db.query_row("SELECT refs FROM staged_ops WHERE id=?1", [id], |r| r.get(0))?;
            if refs == "[]" {
                return Err(MediaError::InvalidManifest("staged media has no commit witness".into()));
            }
        }
        let marker = folder::stamp(&fs::metadata(self.media_dir())?);
        meguri_model::faultpoint!("media.stage.before_publish")?;
        for file in &records {
            validate(file)?;
            let source = self.staging_dir().join(&file.path);
            let target = self.media_dir().join(&file.name);
            let already_present = folder::regular(&target)?.is_some() && folder::hash_file(&target)? == file.sha1;
            if !already_present {
                let Some(m) = folder::regular(&source)? else {
                    return Err(MediaError::InvalidManifest("staged media bytes are missing".into()));
                };
                if folder::hash_file(&source)? != file.sha1 {
                    return Err(MediaError::InvalidManifest("staged media digest mismatch".into()));
                }
                if let Some(old) = folder::regular(&target)? {
                    if !file.replace_existing {
                        return Err(MediaError::NameTaken);
                    }
                    self.trash_one_locked(&mut db, &file.name, &folder::hash_file(&target)?, &old)?;
                }
                let _ = m;
                move_new(&source, &target)?;
                meguri_model::faultpoint!("media.stage.after_publish")?;
            } else if folder::regular(&source)?.is_some() {
                fs::remove_file(&source)?;
            }
            folder::live(
                &mut db,
                &file.name,
                file.sha1.clone(),
                &fs::symlink_metadata(&target)?,
                !already_present,
            )?;
            db.execute(
                "DELETE FROM staged_files WHERE op_id=?1 AND idx=?2",
                params![id, file.idx],
            )?;
        }
        self.advance_marker(&db, &marker)?;
        delete_manifest(&db, id)?;
        Ok(records.len())
    }
    /// The caller checks each witness against rows in the open collection.
    pub fn recover(&self, mut matches: impl FnMut(&NoteReference) -> Result<bool>) -> Result<RecoveryReport> {
        let operations = {
            let db = self.db()?;
            db.prepare("SELECT id,refs FROM staged_ops ORDER BY id")?
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut report = RecoveryReport::default();
        for (id, refs) in operations {
            let refs: Vec<NoteReference> =
                serde_json::from_str(&refs).map_err(|e| MediaError::InvalidManifest(e.to_string()))?;
            let mut committed = !refs.is_empty();
            for reference in &refs {
                if !matches(reference)? {
                    committed = false;
                }
            }
            if committed {
                report.published += self.publish_stage(id)?;
            } else {
                let mut db = self.db()?;
                for file in files(&db, id)? {
                    validate(&file)?;
                    let source = self.staging_dir().join(&file.path);
                    if let Some(m) = folder::regular(&source)? {
                        self.move_to_trash_locked(
                            &mut db,
                            &file.name,
                            &folder::hash_file(&source)?,
                            m.len(),
                            &source,
                            false,
                        )?;
                        report.trashed += 1;
                    }
                }
                delete_manifest(&db, id)?;
            }
        }
        let mut db = self.db()?;
        let known: BTreeSet<String> = db
            .prepare("SELECT path FROM staged_files")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for entry in fs::read_dir(self.staging_dir())? {
            let entry = entry?;
            let Some(path) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if known.contains(&path) {
                continue;
            }
            let Some(m) = folder::regular(&entry.path())? else {
                continue;
            };
            let name = path.split_once("--").map_or(path.as_str(), |(_, name)| name);
            if !safe_component(name) {
                return Err(MediaError::InvalidManifest("invalid orphan staging name".into()));
            }
            self.move_to_trash_locked(
                &mut db,
                name,
                &folder::hash_file(&entry.path())?,
                m.len(),
                &entry.path(),
                false,
            )?;
            report.trashed += 1;
        }
        Ok(report)
    }
}
