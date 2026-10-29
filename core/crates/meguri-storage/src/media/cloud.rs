//! Cloud exact-name publication. All filesystem mutations use the journal's
//! normal publication/trash machinery, while the caller owns the cloud intent.
use super::{
    MediaError, MediaStore, Result,
    case_fold::{hash_name, ignored, lowercase_candidate, normalized},
    folder::{self, DownloadSource},
    journal_db,
};
use std::os::unix::fs::MetadataExt;
use std::{fs, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CloudMoveEvidence {
    source: (u64, u64),
    destination: Option<(u64, u64)>,
}
fn native(path: &Path) -> Result<Option<(u64, u64)>> {
    Ok(folder::regular(path)?.map(|m| (m.dev(), m.ino())))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloudInstall {
    Installed,
    CaseCollision { name: String },
    Changed,
}
fn matches_content(path: &Path, sha256: &[u8; 32], size: u64) -> Result<bool> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let before = file.metadata()?;
    if !before.is_file() || before.len() != size {
        return Ok(false);
    }
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > size {
            return Ok(false);
        }
        digest.update(&buffer[..count]);
    }
    let found: [u8; 32] = digest.finalize().into();
    Ok(total == size && found == *sha256 && folder::unchanged_file(&before, &file.metadata()?))
}
fn move_content(path: &Path, expected_sha1: &str) -> Result<Option<([u8; 32], u64)>> {
    use sha1::Digest as _;
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let before = file.metadata()?;
    if !before.is_file() {
        return Ok(None);
    }
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > before.len() {
            return Ok(None);
        }
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
    }
    if total != before.len()
        || format!("{:x}", sha1.finalize()) != expected_sha1
        || !folder::unchanged_file(&before, &file.metadata()?)
        || folder::regular(path)?.is_none_or(|m| !folder::unchanged_file(&before, &m))
    {
        return Ok(None);
    }
    Ok(Some((sha256.finalize().into(), total)))
}
impl MediaStore {
    pub fn cloud_trash(&self, name: &str, sha1: &str, sha256: &[u8; 32], size: u64) -> Result<bool> {
        if !normalized(name) || ignored(name) {
            return Err(MediaError::InvalidName);
        }
        self.check_changes()?;
        let mut db = self.db()?;
        let source = self.media_dir().join(name);
        if !matches_content(&source, sha256, size)? {
            return Ok(false);
        }
        let verify = |path: &Path| matches_content(path, sha256, size);
        match self.move_to_trash_checked_locked(&mut db, name, sha1, size, &source, true, &mut false, Some(&verify)) {
            Ok(_) => {
                journal_db::set_marker(&db, None)?;
                Ok(true)
            }
            Err(MediaError::NameTaken) => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub fn cloud_trash_path(&self, id: i64) -> Option<std::path::PathBuf> {
        self.trash_entries()
            .ok()?
            .into_iter()
            .find(|t| t.id == id)
            .map(|t| self.trash_dir().join(t.path))
    }
    /// K9.3 refuses another spelling even when its bytes are identical. The
    /// independent staged copy is synced before any old bytes can move.
    pub fn cloud_install(&self, name: &str, staged: &Path, expected_sha1: Option<&str>) -> Result<CloudInstall> {
        if !normalized(name) || ignored(name) {
            return Err(MediaError::InvalidName);
        }
        let mut source = DownloadSource::open(staged)?;
        let copy = source.stage(self.staging_dir())?;
        self.check_changes()?;
        let mut db = self.db()?;
        let candidates = self.candidates(&mut db, name)?;
        if let Some((other, _)) = candidates.iter().find(|(n, _)| n != name) {
            return Ok(CloudInstall::CaseCollision { name: other.clone() });
        }
        let actual = candidates.iter().find(|(n, _)| n == name).map(|(_, h)| h.as_str());
        if actual != expected_sha1 {
            return Ok(CloudInstall::Changed);
        }
        if actual == Some(source.hash.as_str()) {
            return Ok(CloudInstall::Installed);
        }
        match self.install_existing_locked(&mut db, name, &copy.path, &source.hash, &candidates) {
            Ok(()) => Ok(CloudInstall::Installed),
            Err(MediaError::NameTaken) => Ok(CloudInstall::Changed),
            Err(error) => Err(error),
        }
    }
    /// M4's actual hash-name lookup, including an existing case variant.
    pub fn cloud_hash_name(&self, name: &str, sha1: &str) -> Result<String> {
        self.check_changes()?;
        let candidate = hash_name(&lowercase_candidate(name), sha1);
        let candidate = candidate.trim_end_matches([' ', '.']);
        let mut db = self.db()?;
        let (name, _) = self.choose(&mut db, candidate, sha1)?;
        Ok(name)
    }
    /// A move to a previously planned M4 name. Destination bytes are never
    /// replaced. Re-registration after a crash observes the real rename.
    pub fn cloud_move(&self, old: &str, new: &str, expected_sha1: &str) -> Result<bool> {
        let Some(evidence) = self.cloud_move_evidence(old, new, expected_sha1)? else {
            return Ok(false);
        };
        self.cloud_move_guarded(old, new, expected_sha1, &evidence)
    }
    pub fn cloud_move_evidence(&self, old: &str, new: &str, sha1: &str) -> Result<Option<CloudMoveEvidence>> {
        if !normalized(old) || !normalized(new) || old == new || ignored(new) {
            return Err(MediaError::InvalidName);
        }
        self.check_changes()?;
        let _db = self.db()?;
        let source = self.media_dir().join(old);
        let target = self.media_dir().join(new);
        let Some(identity) = native(&source)? else {
            return Ok(None);
        };
        if folder::hash_file(&source)? != sha1 {
            return Ok(None);
        }
        let destination = native(&target)?;
        if destination.is_some() && folder::hash_file(&target)? != sha1 {
            return Ok(None);
        }
        Ok(Some(CloudMoveEvidence {
            source: identity,
            destination,
        }))
    }
    pub fn cloud_move_completed(&self, old: &str, new: &str, sha1: &str, evidence: &CloudMoveEvidence) -> Result<bool> {
        if !normalized(old) || !normalized(new) || old == new || ignored(new) {
            return Err(MediaError::InvalidName);
        }
        let _db = self.db()?;
        let target = self.media_dir().join(new);
        if native(&self.media_dir().join(old))? == Some(evidence.source) {
            return Ok(false);
        }
        if native(&target)?.is_none() || move_content(&target, sha1)?.is_none() {
            return Ok(false);
        }
        if let Some(destination) = evidence.destination {
            if native(&target)? != Some(destination) {
                return Ok(false);
            }
            // Trash retains the original inode and its synced removal receipt.
            // A namesake with equal bytes cannot establish this move.
            let entries = super::trash::cloud_move_entries(&_db)?;
            for entry in entries.into_iter().filter(|e| e.name == old && e.sha1 == sha1) {
                let retained = self.trash_dir().join(&entry.path);
                if native(&retained)? == Some(evidence.source) && move_content(&retained, sha1)?.is_some() {
                    return Ok(true);
                }
            }
            Ok(false)
        } else {
            Ok(native(&target)? == Some(evidence.source))
        }
    }
    pub fn cloud_move_guarded(
        &self,
        old: &str,
        new: &str,
        expected_sha1: &str,
        evidence: &CloudMoveEvidence,
    ) -> Result<bool> {
        if !normalized(old) || !normalized(new) || old == new || ignored(new) {
            return Err(MediaError::InvalidName);
        }
        self.check_changes()?;
        let mut db = self.db()?;
        let source = self.media_dir().join(old);
        let target = self.media_dir().join(new);
        if native(&source)? != Some(evidence.source) || native(&target)? != evidence.destination {
            return Ok(false);
        }
        let Some((sha256, size)) = move_content(&source, expected_sha1)? else {
            return Ok(false);
        };
        let verify_source = |path: &Path| -> Result<bool> {
            Ok(native(path)? == Some(evidence.source)
                && matches_content(path, &sha256, size)?
                && native(path)? == Some(evidence.source))
        };
        if evidence.destination.is_some() {
            let verify = |path: &Path| -> Result<bool> {
                Ok(verify_source(path)?
                    && native(&target)? == evidence.destination
                    && matches_content(&target, &sha256, size)?
                    && native(&target)? == evidence.destination)
            };
            if !verify(&source)? {
                return Ok(false);
            }
            match self.move_to_trash_checked_locked(
                &mut db,
                old,
                expected_sha1,
                size,
                &source,
                true,
                &mut false,
                Some(&verify),
            ) {
                Ok(_) => {}
                Err(MediaError::NameTaken) => return Ok(false),
                Err(error) => return Err(error),
            }
        } else {
            use std::os::unix::fs::OpenOptionsExt;
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&source)?;
            crate::atomic_file::full_sync(&file)?;
            meguri_model::faultpoint!("media.cloud_move.before_move")?;
            if !verify_source(&source)? || native(&target)?.is_some() {
                return Ok(false);
            }
            let outcome = crate::atomic_file::move_new(&source, &target);
            if native(&target)? != Some(evidence.source) {
                outcome?;
                return Ok(false);
            }
            meguri_model::faultpoint!("media.cloud_move.after_move_before_verify")?;
            let checked = verify_source(&target);
            if !matches!(checked, Ok(true)) {
                // Never overwrite a new source name or move a replacement destination.
                if native(&target)? != Some(evidence.source) {
                    return Err(MediaError::Io("changed move destination requires recovery".into()));
                }
                crate::atomic_file::move_new(&target, &source).map_err(|_| {
                    MediaError::Io("changed media retained at move destination; restoration needs recovery".into())
                })?;
                let actual_hash = folder::hash_file(&source)?;
                folder::live(&db, old, actual_hash, &fs::symlink_metadata(&source)?, false)?;
                journal_db::set_marker(&db, None)?;
                checked?;
                return Ok(false);
            }
            outcome?;
            super::feed::observe_removal(&db, old)?;
            folder::deleted(&db, old)?;
            folder::live(&db, new, expected_sha1.into(), &fs::symlink_metadata(&target)?, true)?;
        }
        journal_db::set_marker(&db, None)?;
        Ok(true)
    }
}
