//! P16/R11 files owned by the collection replacement intent. These are never
//! J4 post-image stages, and no recovery decision is made here from DB hashes.
use super::{MediaError, MediaStore, Result, case_fold::*, folder, journal_db};
use crate::atomic_file::{self, AtomicFile, Purpose};
use serde::{Deserialize, Serialize};
use sha1::Digest as _;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingMedia {
    pub name: String,
    pub length: u64,
    pub sha1: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportMediaCounts {
    pub incoming: u64,
    pub decoded_bytes: u64,
    pub reused: u64,
    pub added: u64,
    pub trashed: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportMediaBackup {
    pub name: String,
    pub size_bytes: u64,
    pub instant_ms: i64,
    pub snapshot_modified_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Bytes {
    length: u64,
    sha1: String,
    sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Version {
    name: String,
    bytes: Bytes,
    trash: String,
    id: i64,
    time: i64,
    native: (u64, u64),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Action {
    incoming: IncomingMedia,
    bytes: Bytes,
    reuse: Option<String>,
    old: Vec<Version>,
    undo: Version,
}
/// Immutable recovery authority plus durable per-file progress, embedded in R6.
/// Paths are single owned components or validated J3 trash identities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionMediaIntent {
    root: String,
    root_native: (u64, u64),
    manifest: Bytes,
    journal_ready: bool,
    progress: u64,
    pub counts: ImportMediaCounts,
    pub backup: Option<ImportMediaBackup>,
    #[serde(default)]
    initial_admission: Option<(String, Bytes)>,
    #[serde(default)]
    initial_admission_native: Option<(u64, u64)>,
}

#[derive(Serialize, Deserialize)]
struct CollectionMediaPlan {
    snapshot: Bytes,
    original: BTreeMap<String, Bytes>,
    actions: Vec<Action>,
    previous_journal_absent: bool,
}

#[derive(Debug)]
pub struct MediaPreparationFailure {
    pub error: MediaError,
    pub recovery_reference: Option<String>,
}
impl From<MediaError> for MediaPreparationFailure {
    fn from(error: MediaError) -> Self {
        Self {
            error,
            recovery_reference: None,
        }
    }
}
impl From<std::io::Error> for MediaPreparationFailure {
    fn from(error: std::io::Error) -> Self {
        MediaError::from(error).into()
    }
}

/// Save the initial journal and file identities before the store can change
/// them. If J4 moves a stage into trash, these saved facts must still establish
/// the earlier state; otherwise admission stays closed for recovery.
pub struct InitialMediaState {
    directory: Option<tempfile::TempDir>,
    reference: String,
    journal: PathBuf,
    journal_native: (u64, u64),
    absent: bool,
    files: BTreeMap<PathBuf, Bytes>,
    native_files: BTreeMap<PathBuf, (u64, u64)>,
    created_roots: Vec<(PathBuf, u64, u64)>,
}
impl InitialMediaState {
    pub fn begin(collection: &Path, cancel: &dyn Fn() -> bool) -> std::result::Result<Self, MediaPreparationFailure> {
        let root = collection.parent().unwrap_or(Path::new("."));
        Self::check_pending(root, None)?;
        let mut created_roots = Reservations::default();
        for name in ["collection.media", "media.trash", "media.staging"] {
            let path = root.join(name);
            match fs::create_dir(&path) {
                Ok(()) => {
                    created_roots.add(&path)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => folder::ensure_directory(&path)?,
                Err(e) => return Err(e.into()),
            }
        }
        let directory = tempfile::Builder::new()
            .prefix("r6-admission-")
            .tempdir_in(root.join("media.staging"))?;
        let reference = directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .ok_or_else(invalid)?
            .to_owned();
        let journal = root.join("meguri-media.db");
        let (absent, m) = match OpenOptions::new().write(true).create_new(true).open(&journal) {
            Ok(file) => match file.metadata() {
                Ok(m) => (true, m),
                Err(error) => {
                    // The newly claimed path cannot be proved safe to unlink.
                    created_roots.0.clear();
                    let _ = directory.keep();
                    return Err(MediaPreparationFailure {
                        error: error.into(),
                        recovery_reference: Some(reference),
                    });
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (false, open(&journal)?.metadata()?),
            Err(e) => return Err(e.into()),
        };
        let mut state = Self {
            directory: Some(directory),
            reference,
            journal,
            journal_native: (m.dev(), m.ino()),
            absent,
            files: BTreeMap::new(),
            native_files: BTreeMap::new(),
            created_roots: std::mem::take(&mut created_roots.0),
        };
        let captured: Result<()> = (|| {
            let mut owner_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(state.directory.as_ref().unwrap().path().join("ownership.json"))?;
            serde_json::to_writer(
                &mut owner_file,
                &serde_json::json!({
                    "journalAbsent": state.absent, "journalNative": state.journal_native
                }),
            )
            .map_err(|_| invalid())?;
            owner_file.flush()?;
            atomic_file::full_sync(&owner_file)?;
            atomic_file::sync_directory(state.directory.as_ref().unwrap().path())?;
            atomic_file::sync_directory(&root.join("media.staging"))?;
            atomic_file::full_sync(&open(&state.journal)?)?;
            atomic_file::sync_directory(root)?;
            for name in ["collection.media", "media.trash", "media.staging"] {
                Self::capture(
                    &root.join(name),
                    state.directory.as_ref().unwrap().path(),
                    cancel,
                    &mut state.files,
                    &mut state.native_files,
                )?;
            }
            let mut journal_images = BTreeMap::new();
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let path = folder::sidecar(&state.journal, suffix);
                if let Some(b) = present_checked(&path, cancel)? {
                    let name = format!("journal{suffix}");
                    copy_new_checked(&path, &state.directory.as_ref().unwrap().path().join(&name), &b, cancel)?;
                    journal_images.insert(name, b);
                }
            }
            let manifest = serde_json::json!({
                "journalAbsent": state.absent, "journalNative": state.journal_native,
                "files": state.files, "nativeFiles": state.native_files, "journalImages": journal_images,
            });
            let path = state.directory.as_ref().unwrap().path().join("admission.json");
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            serde_json::to_writer(&mut file, &manifest).map_err(|_| invalid())?;
            file.flush()?;
            atomic_file::full_sync(&file)?;
            atomic_file::sync_directory(state.directory.as_ref().unwrap().path())?;
            atomic_file::sync_directory(&root.join("media.staging"))?;
            Ok(())
        })();
        if let Err(error) = captured {
            // Before any store call only our new empty journal can differ.
            if state.absent && state.remove_journal().is_err() {
                let recovery_reference = Some(state.reference.clone());
                state.retain();
                return Err(MediaPreparationFailure {
                    error,
                    recovery_reference,
                });
            }
            return Err(error.into());
        }
        Ok(state)
    }
    fn capture(
        path: &Path,
        excluded: &Path,
        cancel: &dyn Fn() -> bool,
        files: &mut BTreeMap<PathBuf, Bytes>,
        identities: &mut BTreeMap<PathBuf, (u64, u64)>,
    ) -> Result<()> {
        for entry in fs::read_dir(path)? {
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            let entry = entry?;
            if entry.path() == excluded {
                continue;
            }
            let m = entry.path().symlink_metadata()?;
            if m.is_dir() {
                Self::capture(&entry.path(), excluded, cancel, files, identities)?;
            } else {
                files.insert(entry.path(), bytes_checked(&entry.path(), cancel)?);
                identities.insert(entry.path(), (m.dev(), m.ino()));
            }
        }
        Ok(())
    }
    pub(super) fn check_pending(root: &Path, allowed: Option<&str>) -> Result<()> {
        let entries = match fs::read_dir(root.join("media.staging")) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            if (name.to_string_lossy().starts_with("r6-admission-") && name.to_str() != allowed)
                || (name.to_string_lossy().starts_with("r6-")
                    && entry.path().join("unpublished.json").symlink_metadata().is_ok())
            {
                return Err(MediaError::InvalidManifest(format!(
                    "initial media recovery required: {}",
                    name.to_string_lossy()
                )));
            }
        }
        Ok(())
    }
    pub fn open_store(
        &self,
        collection: &Path,
        clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
        random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<MediaStore> {
        if self.absent {
            for suffix in ["-wal", "-shm", "-journal"] {
                match folder::sidecar(&self.journal, suffix).symlink_metadata() {
                    Ok(_) => return Err(invalid()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        MediaStore::open_checked(
            collection,
            clock,
            random,
            cancel,
            Some(&self.reference),
            self.absent.then_some(self.journal_native),
        )
    }
    pub fn journal_was_absent(&self) -> bool {
        self.absent
    }
    pub fn attach(&self, intent: &mut CollectionMediaIntent) -> Result<()> {
        let metadata = fs::symlink_metadata(self.directory.as_ref().ok_or_else(invalid)?.path())?;
        if !metadata.is_dir() {
            return Err(invalid());
        }
        intent.initial_admission = Some((
            self.reference.clone(),
            bytes(
                &self
                    .directory
                    .as_ref()
                    .ok_or_else(invalid)?
                    .path()
                    .join("admission.json"),
            )?,
        ));
        intent.initial_admission_native = Some((metadata.dev(), metadata.ino()));
        Ok(())
    }
    pub fn recovery_reference(&self) -> &str {
        &self.reference
    }
    pub fn retain(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = directory.keep();
        }
    }
    fn remove_journal(&self) -> Result<()> {
        let m = open(&self.journal)?.metadata()?;
        if (m.dev(), m.ino()) != self.journal_native {
            return Err(invalid());
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            if folder::sidecar(&self.journal, suffix).symlink_metadata().is_ok() {
                return Err(invalid());
            }
        }
        meguri_model::faultpoint!("replace.media.initial_restore")?;
        fs::remove_file(&self.journal)?;
        atomic_file::sync_directory(self.journal.parent().unwrap())?;
        Ok(())
    }
    pub fn restore_unpublished(&mut self, store: Option<&MediaStore>) -> Result<()> {
        // Existing journal images remain durable evidence rather than guessing
        // whether SQLite/J4 changed them during a failed open or recovery.
        if !self.absent {
            return Err(invalid());
        }
        let root = self.journal.parent().unwrap();
        let mut found = BTreeMap::new();
        let mut native = BTreeMap::new();
        for name in ["collection.media", "media.trash", "media.staging"] {
            Self::capture(
                &root.join(name),
                self.directory.as_ref().ok_or_else(invalid)?.path(),
                &|| false,
                &mut found,
                &mut native,
            )?;
        }
        if found != self.files || native != self.native_files {
            return Err(invalid());
        }
        if let Some(store) = store
            && !store.try_close()?
        {
            return Err(invalid());
        }
        self.remove_journal()
    }
}
impl Drop for InitialMediaState {
    fn drop(&mut self) {
        if self.directory.is_some() {
            // Only empty roots created by this admission, at the same identity.
            drop(self.directory.take());
            for (path, dev, ino) in self.created_roots.iter().rev() {
                if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == (*dev, *ino)) {
                    let _ = fs::remove_dir(path);
                }
            }
        }
    }
}

/// Admission, not a mutex guard. Ordinary APIs refuse while it is held; the
/// replacement uses the same shared connection. Never invokes client code.
pub struct MediaImportOwner {
    store: MediaStore,
    resolved: bool,
}
impl MediaImportOwner {
    pub fn resolved(&mut self) {
        self.resolved = true;
    }
    pub fn recovery_required(&mut self) {
        self.resolved = false;
    }
    pub fn restore_initial_journal_absence(&self) -> Result<()> {
        self.store.restore_collection_import_journal_absence()
    }
}
impl Drop for MediaImportOwner {
    fn drop(&mut self) {
        if self.resolved {
            self.store.inner.importing.store(false, Ordering::Release);
        }
    }
}
fn invalid() -> MediaError {
    MediaError::InvalidManifest("collection media recovery evidence is missing, changed or conflicting".into())
}
pub fn validate_incoming_name(name: &str) -> Result<()> {
    if !normalized(name) || ignored(name) || meguri_text::normalize::nfc(name) != name {
        return Err(MediaError::InvalidName);
    }
    Ok(())
}
fn open(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 {
        return Err(invalid());
    }
    Ok(file)
}
fn bytes(path: &Path) -> Result<Bytes> {
    bytes_checked(path, &|| false)
}
fn bytes_checked(path: &Path, cancel: &dyn Fn() -> bool) -> Result<Bytes> {
    meguri_model::faultpoint!("replace.media.hash_chunk")?;
    let mut file = open(path)?;
    let before = file.metadata()?;
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0; 65536];
    let mut length = 0u64;
    loop {
        meguri_model::faultpoint!("replace.media.hash_chunk")?;
        if cancel() {
            return Err(MediaError::Cancelled);
        }
        let n = file.read(&mut buffer)?;
        if before.len() >= 1024 * 1024
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "collection.media")
        {
            meguri_model::faultpoint!("replace.media.old_hash_chunk")?;
        }
        if before.len() >= 1024 * 1024
            && path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .is_some_and(|n| n == "media.trash")
        {
            meguri_model::faultpoint!("replace.media.trash_hash_chunk")?;
        }
        if cancel() {
            return Err(MediaError::Cancelled);
        }
        if n == 0 {
            break;
        }
        length = length.checked_add(n as u64).ok_or_else(invalid)?;
        sha1.update(&buffer[..n]);
        sha256.update(&buffer[..n]);
    }
    let after = file.metadata()?;
    let bound = fs::symlink_metadata(path)?;
    if before.dev() != bound.dev()
        || before.ino() != bound.ino()
        || before.len() != length
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(invalid());
    }
    Ok(Bytes {
        length,
        sha1: format!("{:x}", sha1.finalize()),
        sha256: sha256.finalize().into(),
    })
}
fn present(path: &Path) -> Result<Option<Bytes>> {
    present_checked(path, &|| false)
}
fn present_checked(path: &Path, cancel: &dyn Fn() -> bool) -> Result<Option<Bytes>> {
    match fs::symlink_metadata(path) {
        Ok(_) => bytes_checked(path, cancel).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn require(path: &Path, expected: &Bytes) -> Result<()> {
    require_checked(path, expected, &|| false)
}
fn require_checked(path: &Path, expected: &Bytes, cancel: &dyn Fn() -> bool) -> Result<()> {
    if bytes_checked(path, cancel)? != *expected {
        return Err(invalid());
    }
    Ok(())
}
fn copy_new_checked(source: &Path, target: &Path, expected: &Bytes, cancel: &dyn Fn() -> bool) -> Result<()> {
    if bytes_checked(source, cancel)? != *expected {
        return Err(invalid());
    }
    let mut input = open(source)?;
    let mut out = AtomicFile::new(target, Purpose::Media)?;
    let mut buffer = [0; 65536];
    loop {
        if cancel() {
            return Err(MediaError::Cancelled);
        }
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        out.file_mut().write_all(&buffer[..n])?;
    }
    let published = out.publish_new(|path| {
        if bytes_checked(path, cancel).map_err(storage)? != *expected {
            return Err(storage(invalid()));
        }
        Ok(())
    });
    if let Err(e) = published {
        return Err(if cancel() { MediaError::Cancelled } else { e.into() });
    }
    require_checked(target, expected, cancel)
}
fn storage(e: MediaError) -> crate::StorageError {
    crate::StorageError::Io { detail: e.to_string() }
}
fn checkpoint(db: &rusqlite::Connection, path: &Path) -> Result<()> {
    let (busy, _, _): (i64, i64, i64) = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    if busy != 0 {
        return Err(invalid());
    }
    atomic_file::full_sync(&open(path)?)?;
    Ok(())
}
fn inventory(store: &MediaStore) -> Result<BTreeMap<String, Bytes>> {
    inventory_checked(store, &|| false)
}
fn inventory_checked(store: &MediaStore, cancel: &dyn Fn() -> bool) -> Result<BTreeMap<String, Bytes>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(store.media_dir())? {
        if cancel() {
            return Err(MediaError::Cancelled);
        }
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        #[cfg(target_vendor = "apple")]
        let name = meguri_text::normalize::nfc(&name).into_owned();
        if entry.file_type()?.is_symlink() {
            return Err(MediaError::InvalidName);
        }
        if entry.file_type()?.is_file() && normalized(&name) && !ignored(&name) {
            if meguri_text::normalize::nfc(&name) != name {
                return Err(MediaError::InvalidName);
            }
            if result.insert(name, bytes_checked(&entry.path(), cancel)?).is_some() {
                return Err(MediaError::NameTaken);
            }
        }
    }
    Ok(result)
}
#[derive(Default)]
struct Reservations(Vec<(PathBuf, u64, u64)>);
impl Reservations {
    fn add(&mut self, path: &Path) -> Result<(u64, u64)> {
        let m = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(error) => {
                self.0.push((path.to_owned(), 0, 0));
                return Err(error.into());
            }
        };
        self.0.push((path.to_owned(), m.dev(), m.ino()));
        Ok((m.dev(), m.ino()))
    }
    fn cleanup(&mut self) -> Result<()> {
        meguri_model::faultpoint!("replace.media.unpublished_cleanup")?;
        for (path, dev, ino) in &self.0 {
            match fs::symlink_metadata(path) {
                Ok(m) if m.is_dir() && m.dev() == *dev && m.ino() == *ino => {
                    fs::remove_dir(path)?;
                    atomic_file::sync_directory(path.parent().ok_or_else(invalid)?)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(invalid()),
            }
        }
        self.0.clear();
        Ok(())
    }
}
impl Drop for Reservations {
    fn drop(&mut self) {
        for (path, dev, ino) in &self.0 {
            if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.dev() == *dev && m.ino() == *ino) {
                // remove_dir refuses a nonempty or replaced reservation.
                let _ = fs::remove_dir(path);
            }
        }
    }
}
impl MediaStore {
    /// Initial-entry rollback only, under its media owner and collection lock.
    /// Close the shared connection, then remove only its proven native file.
    fn restore_collection_import_journal_absence(&self) -> Result<()> {
        let identity = open(&self.inner.journal)?.metadata()?;
        if (identity.dev(), identity.ino()) != self.inner.journal_native {
            return Err(invalid());
        }
        let mut guard = self.inner.db.lock().map_err(|_| invalid())?;
        if let Some(db) = guard.take()
            && let Err((db, error)) = db.close()
        {
            *guard = Some(db);
            return Err(error.into());
        }
        let bound = fs::symlink_metadata(&self.inner.journal)?;
        if !bound.is_file() || bound.nlink() != 1 || bound.dev() != identity.dev() || bound.ino() != identity.ino() {
            return Err(invalid());
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let path = folder::sidecar(&self.inner.journal, suffix);
            match path.symlink_metadata() {
                Ok(_) => return Err(invalid()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        fs::remove_file(&self.inner.journal)?;
        atomic_file::sync_directory(self.inner.journal.parent().unwrap())?;
        Ok(())
    }
    pub fn preflight_collection_import(
        &self,
        incoming: &[IncomingMedia],
        provider: Option<&dyn crate::space::SpaceProvider>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<()> {
        let overflow = || MediaError::Io("collection media preflight overflow".into());
        let device = fs::metadata(self.media_dir())?.dev();
        for path in [
            self.staging_dir(),
            self.trash_dir(),
            self.inner.journal.parent().unwrap(),
        ] {
            if fs::symlink_metadata(path)?.file_type().is_symlink() || fs::metadata(path)?.dev() != device {
                return Err(overflow());
            }
        }
        let mut needed = 16u64 * 1024 * 1024;
        for m in incoming {
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            needed = needed
                .checked_add(m.length.checked_mul(2).ok_or_else(overflow)?)
                .and_then(|n| n.checked_add(4096))
                .ok_or_else(overflow)?;
        }
        for entry in fs::read_dir(self.media_dir())? {
            meguri_model::faultpoint!("replace.media.capacity_entry")?;
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            let entry = entry?;
            let m = entry.path().symlink_metadata()?;
            if m.is_file() {
                needed = needed
                    .checked_add(m.len())
                    .and_then(|n| n.checked_add(4096))
                    .ok_or_else(overflow)?;
            }
        }
        for path in [self.inner.journal.clone(), folder::sidecar(&self.inner.journal, "-wal")] {
            match fs::symlink_metadata(path) {
                Ok(m) if m.is_file() => {
                    needed = needed
                        .checked_add(m.len().checked_mul(3).ok_or_else(overflow)?)
                        .ok_or_else(overflow)?;
                }
                Ok(_) => return Err(overflow()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        if cancel() {
            return Err(MediaError::Cancelled);
        }
        crate::space::require_space(
            provider.unwrap_or(&crate::space::StatvfsSpace),
            self.media_dir(),
            needed,
        )
        .map_err(MediaError::from)?;
        if cancel() {
            return Err(MediaError::Cancelled);
        }
        Ok(())
    }
    pub fn own_collection_import(&self) -> Result<MediaImportOwner> {
        // Wait for an admitted journal mutation before excluding subsequent
        // APIs. The engine separately holds the real shared media-job lease.
        let _db = self.db()?;
        if self.inner.importing.swap(true, Ordering::AcqRel) {
            return Err(invalid());
        }
        Ok(MediaImportOwner {
            store: self.clone(),
            resolved: true,
        })
    }
    pub fn prepare_collection_import(
        &self,
        stage: &Path,
        incoming: Vec<IncomingMedia>,
        backup: Option<ImportMediaBackup>,
        cancel: &dyn Fn() -> bool,
        previous_journal_absent: bool,
    ) -> std::result::Result<CollectionMediaIntent, MediaPreparationFailure> {
        let directory = tempfile::Builder::new().prefix("r6-").tempdir_in(self.staging_dir())?;
        let root = directory
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(invalid)?
            .to_owned();
        let mut reservations = Reservations::default();
        let prepared: Result<CollectionMediaIntent> = (|| {
            let original = inventory_checked(self, cancel)?;
            let db = self.import_db()?;
            let pending: i64 = db.query_row("SELECT COUNT(*) FROM staged_ops", [], |r| r.get(0))?;
            if pending != 0 {
                return Err(invalid());
            }
            checkpoint(&db, &self.inner.journal)?;
            let mut id: i64 = db.query_row("SELECT COALESCE(MAX(id),0) FROM trash_entries", [], |r| r.get(0))?;
            drop(db);
            let snapshot = bytes_checked(&self.inner.journal, cancel)?;
            copy_new_checked(
                &self.inner.journal,
                &directory.path().join("old-journal.db"),
                &snapshot,
                cancel,
            )?;

            let mut reserve = |name: &str, b: &Bytes| -> Result<Version> {
                // Reserve an unused J3 identity without publishing a trash row or
                // touching old journal state before the R6 intent is durable.
                loop {
                    id = id.checked_add(1).ok_or_else(invalid)?;
                    let time = self.inner.clock.now_ms();
                    let parent = format!("{time}-{id}");
                    match fs::create_dir(self.trash_dir().join(&parent)) {
                        Ok(()) => {
                            let native = reservations.add(&self.trash_dir().join(&parent))?;
                            atomic_file::sync_directory(self.trash_dir())?;
                            return Ok(Version {
                                name: name.into(),
                                bytes: b.clone(),
                                trash: format!("{parent}/{name}"),
                                id,
                                time,
                                native,
                            });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(e) => return Err(e.into()),
                    }
                }
            };
            let mut keys = BTreeSet::new();
            let mut actions = Vec::new();
            let mut counts = ImportMediaCounts::default();
            for (index, item) in incoming.into_iter().enumerate() {
                validate_incoming_name(&item.name)?;
                if !keys.insert(case_key(&item.name)) {
                    return Err(MediaError::NameTaken);
                }
                let source = stage.join(format!("media-{index}"));
                let b = bytes_checked(&source, cancel)?;
                if b.length != item.length || b.sha1 != item.sha1 {
                    return Err(invalid());
                }
                copy_new_checked(&source, &directory.path().join(index.to_string()), &b, cancel)?;
                counts.incoming += 1;
                counts.decoded_bytes = counts.decoded_bytes.checked_add(b.length).ok_or_else(invalid)?;
                let variants: Vec<_> = original
                    .iter()
                    .filter(|(n, _)| case_key(n) == case_key(&item.name))
                    .collect();
                // An exact-name alias the store cannot enumerate must refuse; it
                // must not become an overwrite on a case-insensitive volume.
                if present_checked(&self.media_dir().join(&item.name), cancel)?.is_some() && variants.is_empty() {
                    return Err(MediaError::NameTaken);
                }
                let reuse = variants
                    .iter()
                    .find(|(n, old)| **n == item.name && old.sha1 == b.sha1)
                    .or_else(|| variants.iter().find(|(_, old)| old.sha1 == b.sha1))
                    .map(|(n, _)| (*n).clone());
                let mut old = Vec::new();
                if reuse.is_some() {
                    counts.reused += 1;
                } else {
                    // Detect nonregular entries before any member is published.
                    if self.media_dir().join(&item.name).symlink_metadata().is_ok()
                        && !original.contains_key(&item.name)
                        && variants.is_empty()
                    {
                        return Err(MediaError::NameTaken);
                    }
                    for (name, old_bytes) in variants {
                        old.push(reserve(name, old_bytes)?);
                    }
                    counts.trashed += old.len() as u64;
                    counts.added += 1;
                }
                let undo = reserve(&item.name, &b)?;
                actions.push(Action {
                    incoming: item,
                    bytes: b,
                    reuse,
                    old,
                    undo,
                });
            }
            let plan = CollectionMediaPlan {
                snapshot,
                original,
                actions,
                previous_journal_absent,
            };
            let manifest_path = directory.path().join("plan.json");
            let mut manifest_file = OpenOptions::new().write(true).create_new(true).open(&manifest_path)?;
            serde_json::to_writer(&mut manifest_file, &plan).map_err(|e| match e.io_error_kind() {
                Some(kind) => MediaError::from(std::io::Error::new(kind, "collection media manifest write failed")),
                None => invalid(),
            })?;
            manifest_file.flush()?;
            atomic_file::full_sync(&manifest_file)?;
            drop(manifest_file);
            let manifest = bytes_checked(&manifest_path, cancel)?;
            atomic_file::sync_directory(directory.path())?;
            atomic_file::sync_directory(self.staging_dir())?;
            let root_metadata = fs::symlink_metadata(directory.path())?;
            let intent = CollectionMediaIntent {
                root: root.clone(),
                root_native: (root_metadata.dev(), root_metadata.ino()),
                manifest,
                journal_ready: false,
                progress: 0,
                counts,
                backup,
                initial_admission: None,
                initial_admission_native: None,
            };
            // PreparedMedia owns these unpublished artifacts until R6 handoff.
            meguri_model::faultpoint!("replace.media.prepared")?;
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            Ok(intent)
        })();
        match prepared {
            Ok(intent) => {
                reservations.0.clear();
                let _ = directory.keep();
                Ok(intent)
            }
            Err(error) => {
                if reservations.cleanup().is_err() {
                    // Keep the exact reservation identities with the unpublished
                    // payloads. No ordinary sweep owns this R6 evidence.
                    let evidence = directory.path().join("unpublished.json");
                    let saved = (|| -> Result<()> {
                        let mut file = OpenOptions::new().write(true).create_new(true).open(evidence)?;
                        serde_json::to_writer(&mut file, &reservations.0).map_err(|_| invalid())?;
                        file.flush()?;
                        atomic_file::full_sync(&file)?;
                        atomic_file::sync_directory(directory.path())?;
                        atomic_file::sync_directory(self.staging_dir())?;
                        Ok(())
                    })();
                    // A failed evidence sync is also uncertain; preserve all
                    // bytes and keep admission closed in the engine.
                    let _ = saved;
                    reservations.0.clear();
                    let _ = directory.keep();
                    return Err(MediaPreparationFailure {
                        error,
                        recovery_reference: Some(root),
                    });
                }
                match directory.close() {
                    Ok(()) => Err(error.into()),
                    Err(_) => Err(MediaPreparationFailure {
                        error,
                        recovery_reference: Some(root),
                    }),
                }
            }
        }
    }
}
impl CollectionMediaIntent {
    pub fn validate_initial_admission(&self, collection: &Path) -> Result<Option<&str>> {
        let Some((reference, expected)) = &self.initial_admission else {
            return Ok(None);
        };
        if !safe_component(reference) || !reference.starts_with("r6-admission-") {
            return Err(invalid());
        }
        let directory = collection
            .parent()
            .unwrap_or(Path::new("."))
            .join("media.staging")
            .join(reference);
        let metadata = fs::symlink_metadata(&directory)?;
        if !metadata.is_dir()
            || self
                .initial_admission_native
                .is_some_and(|n| n != (metadata.dev(), metadata.ino()))
            || bytes(&directory.join("admission.json"))? != *expected
        {
            return Err(invalid());
        }
        Ok(Some(reference))
    }
    pub fn open_recovery_store(
        &self,
        collection: &Path,
        clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
        random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
    ) -> Result<MediaStore> {
        let allowed = self.validate_initial_admission(collection)?;
        MediaStore::open_checked(collection, clock, random, &|| false, allowed, None)
    }
    /// The replacement owner must first prove durable R6 completion from its
    /// retained operation/publication record. No store open is needed here.
    pub fn cleanup_initial_admission(&self, collection: &Path) -> Result<()> {
        if let Some((reference, expected)) = &self.initial_admission {
            if !safe_component(reference) || !reference.starts_with("r6-admission-") {
                return Err(invalid());
            }
            let staging = collection.parent().unwrap_or(Path::new(".")).join("media.staging");
            folder::ensure_directory(&staging)?;
            let path = staging.join(reference);
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
                Ok(m) if !m.is_dir() || self.initial_admission_native.is_some_and(|n| n != (m.dev(), m.ino())) => {
                    return Err(invalid());
                }
                Ok(_) => {}
            }
            match present(&path.join("admission.json"))? {
                Some(found) if found == *expected => {}
                None if self.initial_admission_native.is_some() => {}
                _ => return Err(invalid()),
            }
            // The manifest binds the journal snapshots. Preserve unexpected
            // files instead of recursively deleting ambiguous evidence.
            for entry in fs::read_dir(&path)? {
                let entry = entry?;
                if !entry.file_type()?.is_file()
                    || !matches!(
                        entry.file_name().to_str(),
                        Some(
                            "admission.json"
                                | "ownership.json"
                                | "journal"
                                | "journal-wal"
                                | "journal-shm"
                                | "journal-journal"
                        )
                    )
                {
                    return Err(invalid());
                }
            }
            meguri_model::faultpoint!("replace.media.admission.before_cleanup")?;
            for entry in fs::read_dir(&path)? {
                let entry = entry?;
                if entry.file_name() != "admission.json" {
                    fs::remove_file(entry.path())?;
                    meguri_model::faultpoint!("replace.media.admission.during_cleanup")?;
                }
            }
            match fs::remove_file(path.join("admission.json")) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            atomic_file::sync_directory(&path)?;
            meguri_model::faultpoint!("replace.media.admission.after_manifest_cleanup")?;
            fs::remove_dir(&path)?;
            atomic_file::sync_directory(&staging)?;
            meguri_model::faultpoint!("replace.media.admission.after_cleanup")?;
        }
        Ok(())
    }
    pub fn recovery_reference(&self) -> &str {
        &self.root
    }
    fn directory(&self, store: &MediaStore) -> Result<PathBuf> {
        if !safe_component(&self.root) || !self.root.starts_with("r6-") {
            return Err(invalid());
        }
        let path = store.staging_dir().join(&self.root);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() || (metadata.dev(), metadata.ino()) != self.root_native {
            return Err(invalid());
        }
        Ok(path)
    }
    fn validate(&self, store: &MediaStore) -> Result<(PathBuf, CollectionMediaPlan)> {
        self.validate_checked(store, &|| false)
    }
    fn validate_checked(
        &self,
        store: &MediaStore,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(PathBuf, CollectionMediaPlan)> {
        let path = self.directory(store)?;
        if bytes_checked(&path.join("plan.json"), cancel)? != self.manifest {
            return Err(invalid());
        }
        let plan: CollectionMediaPlan =
            serde_json::from_reader(open(&path.join("plan.json"))?).map_err(|_| invalid())?;
        if bytes_checked(&path.join("old-journal.db"), cancel)? != plan.snapshot {
            return Err(invalid());
        }
        for name in plan.original.keys() {
            validate_incoming_name(name)?;
        }
        let mut keys = BTreeSet::new();
        let mut identities = BTreeSet::new();
        let mut counts = ImportMediaCounts::default();
        for (i, action) in plan.actions.iter().enumerate() {
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            if !keys.insert(case_key(&action.incoming.name))
                || action.incoming.length > u64::from(u32::MAX)
                || action.incoming.length != action.bytes.length
                || action.incoming.sha1 != action.bytes.sha1
                || action.undo.name != action.incoming.name
                || action.undo.bytes != action.bytes
            {
                return Err(invalid());
            }
            counts.incoming += 1;
            counts.decoded_bytes = counts
                .decoded_bytes
                .checked_add(action.bytes.length)
                .ok_or_else(invalid)?;
            if let Some(reused) = &action.reuse {
                validate_incoming_name(reused)?;
                if case_key(reused) != case_key(&action.incoming.name)
                    || plan.original.get(reused).is_none_or(|b| b.sha1 != action.bytes.sha1)
                    || !action.old.is_empty()
                {
                    return Err(invalid());
                }
                counts.reused += 1;
            } else {
                counts.added += 1;
                counts.trashed += action.old.len() as u64;
            }
            for old in &action.old {
                if plan.original.get(&old.name) != Some(&old.bytes)
                    || case_key(&old.name) != case_key(&action.incoming.name)
                {
                    return Err(invalid());
                }
            }
            validate_incoming_name(&action.incoming.name)?;
            if bytes_checked(&path.join(i.to_string()), cancel)? != action.bytes {
                return Err(invalid());
            }
            for v in action.old.iter().chain(std::iter::once(&action.undo)) {
                if !identities.insert(v.id)
                    || !safe_component(&v.name)
                    || v.id <= 0
                    || v.trash != format!("{}-{}/{}", v.time, v.id, v.name)
                {
                    return Err(invalid());
                }
                let parent = store.trash_dir().join(format!("{}-{}", v.time, v.id));
                let m = fs::symlink_metadata(parent)?;
                if !m.is_dir() || (m.dev(), m.ino()) != v.native {
                    return Err(invalid());
                }
                if let Some(found) = present_checked(&store.trash_dir().join(&v.trash), cancel)?
                    && found != v.bytes
                {
                    return Err(invalid());
                }
            }
        }
        if counts != self.counts {
            return Err(invalid());
        }
        Ok((path, plan))
    }
    fn trash(store: &MediaStore, v: &Version, source: &Path) -> Result<()> {
        Self::trash_checked(store, v, source, &|| false)
    }
    fn trash_checked(store: &MediaStore, v: &Version, source: &Path, cancel: &dyn Fn() -> bool) -> Result<()> {
        let target = store.trash_dir().join(&v.trash);
        if let Some(found) = present_checked(&target, cancel)? {
            if found != v.bytes {
                return Err(invalid());
            }
            if present_checked(source, cancel)?.is_some() {
                return Err(invalid());
            }
        } else {
            require_checked(source, &v.bytes, cancel)?;
            atomic_file::full_sync(&open(source)?)?;
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            atomic_file::move_new(source, &target)?;
        }
        Self::record_trash(store, v)
    }
    fn record_trash(store: &MediaStore, v: &Version) -> Result<()> {
        let db = store.import_db()?;
        db.execute("INSERT INTO trash_entries(id,name,sha1,size,trashed_ms,path) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING", rusqlite::params![v.id,v.name,v.bytes.sha1,journal_db::sqlite_integer(v.bytes.length)?,v.time,v.trash])?;
        let recorded: (String, String, String) =
            db.query_row("SELECT name,sha1,path FROM trash_entries WHERE id=?1", [v.id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        if recorded != (v.name.clone(), v.bytes.sha1.clone(), v.trash.clone()) {
            return Err(invalid());
        }
        Ok(())
    }
    /// Called off the collection writer, with no foreign progress callbacks.
    /// Each boundary persists in the existing R6 record outside journal locks.
    pub fn publish(
        &mut self,
        store: &MediaStore,
        mut record: impl FnMut(&Self) -> Result<()>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<()> {
        let (directory, plan) = self.validate_checked(store, cancel)?;
        for i in 0..plan.actions.len() {
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            let a = &plan.actions[i];
            if let Some(name) = &a.reuse {
                if bytes_checked(&store.media_dir().join(name), cancel)?
                    != *plan.original.get(name).ok_or_else(invalid)?
                {
                    return Err(invalid());
                }
                continue;
            }
            let target = store.media_dir().join(&a.incoming.name);
            for old in &a.old {
                let source = store.media_dir().join(&old.name);
                if present_checked(&store.trash_dir().join(&old.trash), cancel)?.is_some() {
                    require_checked(&store.trash_dir().join(&old.trash), &old.bytes, cancel)?;
                    // Its original name may now hold the candidate bytes.
                    if let Some(found) = present_checked(&source, cancel)?
                        && (old.name != a.incoming.name || found != a.bytes)
                    {
                        return Err(invalid());
                    }
                    Self::record_trash(store, old)?;
                } else {
                    Self::trash_checked(store, old, &source, cancel)?;
                }
                meguri_model::faultpoint!("replace.media.after_trash")?;
                self.progress = self.progress.checked_add(1).ok_or_else(invalid)?;
                record(self)?;
            }
            match present_checked(&target, cancel)? {
                Some(found) if found == a.bytes => {}
                Some(_) => return Err(invalid()),
                None => copy_new_checked(&directory.join(i.to_string()), &target, &a.bytes, cancel)?,
            }
            meguri_model::faultpoint!("replace.media.after_publish")?;
            self.progress = self.progress.checked_add(1).ok_or_else(invalid)?;
            record(self)?;
        }
        let expected = plan.expected();
        if inventory_checked(store, cancel)? != expected {
            return Err(invalid());
        }
        if !self.journal_ready {
            let mut db = store.import_db()?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            super::feed::replace_identity(&tx, store.inner.random.as_ref())?;
            tx.execute_batch("DELETE FROM media; DELETE FROM media_downloads; DELETE FROM meta WHERE key IN ('baseline_owner','folder_mtime'); UPDATE meta SET value='0' WHERE key='last_usn';")?;
            // Counters remain monotonic to invalidate old acknowledgements.
            let epoch = super::sync_state::epoch(&tx)?.checked_add(1).ok_or_else(invalid)?;
            journal_db::sqlite_integer(epoch)?;
            tx.execute(
                "UPDATE meta SET value=?1 WHERE key='baseline_epoch'",
                [epoch.to_string()],
            )?;
            for (name, b) in &expected {
                if cancel() {
                    return Err(MediaError::Cancelled);
                }
                folder::live(
                    &tx,
                    name,
                    b.sha1.clone(),
                    &fs::symlink_metadata(store.media_dir().join(name))?,
                    true,
                )?;
            }
            journal_db::set_marker(&tx, Some(&folder::stamp(&fs::metadata(store.media_dir())?)))?;
            super::feed::complete_registration(&tx)?;
            tx.commit()?;
            checkpoint(&db, &store.inner.journal)?;
            drop(db);
            meguri_model::faultpoint!("replace.media.after_journal")?;
            self.journal_ready = true;
            record(self)?;
        }
        drop(expected);
        drop(plan);
        self.verify_checked(store, cancel)
    }
    fn verify_checked(&self, store: &MediaStore, cancel: &dyn Fn() -> bool) -> Result<()> {
        let (_, plan) = self.validate_checked(store, cancel)?;
        let expected = plan.expected();
        if !self.journal_ready || inventory_checked(store, cancel)? != expected {
            return Err(invalid());
        }
        let db = store.import_db()?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0))?;
        if u64::try_from(count).ok() != Some(expected.len() as u64) {
            return Err(invalid());
        }
        let mut query = db.prepare(&format!("SELECT {} FROM media ORDER BY name", journal_db::COLUMNS))?;
        for row in query.query_map([], journal_db::row)? {
            if cancel() {
                return Err(MediaError::Cancelled);
            }
            let e = row?;
            let b = expected.get(&e.name).ok_or_else(invalid)?;
            if !e.live
                || e.sha1.as_ref() != Some(&b.sha1)
                || e.size != b.length
                || e.pending_download
                || (e.syncable() && !e.dirty)
            {
                return Err(invalid());
            }
        }
        let absent: i64 = db.query_row("SELECT (SELECT COUNT(*) FROM media_downloads)+(SELECT COUNT(*) FROM meta WHERE key='baseline_owner' OR (key='last_usn' AND value!='0'))", [], |r| r.get(0))?;
        if absent != 0 {
            return Err(invalid());
        }
        Ok(())
    }
    fn verify_initial_rollback(&self, store: &MediaStore) -> Result<()> {
        let Some((reference, expected)) = &self.initial_admission else {
            return Ok(());
        };
        if !safe_component(reference) || !reference.starts_with("r6-admission-") {
            return Err(invalid());
        }
        let manifest = store.staging_dir().join(reference).join("admission.json");
        if bytes(&manifest)? != *expected {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_reader(open(&manifest)?).map_err(|_| invalid())?;
        // A prior journal/J4 image has independent pre-admission authority.
        // Keep that evidence if a rollback cannot prove its exact preimage.
        if value.get("journalAbsent").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(invalid());
        }
        let original: BTreeMap<PathBuf, Bytes> =
            serde_json::from_value(value.get("files").cloned().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let identities: BTreeMap<PathBuf, (u64, u64)> =
            serde_json::from_value(value.get("nativeFiles").cloned().ok_or_else(invalid)?).map_err(|_| invalid())?;
        for (path, expected) in original {
            let m = open(&path)?.metadata()?;
            if identities.get(&path) != Some(&(m.dev(), m.ino())) || bytes(&path)? != expected {
                return Err(invalid());
            }
        }
        // Additional operation-owned trash is retained by R6 rollback. It is
        // never evidence that an original staged or media path survived.
        Ok(())
    }
    pub fn rollback(&self, store: &MediaStore) -> Result<()> {
        let (directory, plan) = self.validate(store)?;
        for a in plan.actions.iter().rev() {
            if a.reuse.is_some() {
                continue;
            }
            let target = store.media_dir().join(&a.incoming.name);
            if let Some(found) = present(&target)? {
                let was_old = a.old.iter().any(|v| v.name == a.incoming.name && v.bytes == found);
                if !was_old {
                    if found != a.bytes {
                        return Err(invalid());
                    }
                    Self::trash(store, &a.undo, &target)?;
                    meguri_model::faultpoint!("replace.media.rollback.after_trash")?;
                }
            }
            for old in &a.old {
                let target = store.media_dir().join(&old.name);
                match present(&target)? {
                    Some(found) if found == old.bytes => {}
                    Some(_) => return Err(invalid()),
                    None => {
                        let saved = store.trash_dir().join(&old.trash);
                        require(&saved, &old.bytes)?;
                        atomic_file::move_new(&saved, &target)?;
                    }
                }
            }
        }
        if inventory(store)? != plan.original {
            return Err(invalid());
        }
        // SQLite's backup API restores the entire own journal, including prior
        // downloads, baseline, counters and trash rows. No foreign DB is opened.
        let source = rusqlite::Connection::open_with_flags(
            crate::connections::file_uri(&directory.join("old-journal.db"), Some("mode=ro&immutable=1")),
            crate::connections::READ_ONLY,
        )?;
        let mut db = store.import_db()?;
        let backup = rusqlite::backup::Backup::new(&source, &mut db)?;
        loop {
            match backup.step(128)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                _ => return Err(invalid()),
            }
        }
        drop(backup);
        // An R6 backup prepared before J6 may contain a v2 journal. Restoring
        // it must not leave this already-open store without its feed schema.
        let restored_version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if restored_version == 2 {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            super::feed::migrate(&tx, store.inner.random.as_ref())?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        checkpoint(&db, &store.inner.journal)?;
        // Restored media/baseline rows are the snapshot's exact preimages.
        // Newly published bytes remain visible in J3 trash even in this same
        // process, without waiting for a subsequent open's reconciliation.
        for a in &plan.actions {
            for v in a.old.iter().chain(std::iter::once(&a.undo)) {
                if present(&store.trash_dir().join(&v.trash))?.is_some() {
                    db.execute("INSERT INTO trash_entries(id,name,sha1,size,trashed_ms,path) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING", rusqlite::params![v.id,v.name,v.bytes.sha1,journal_db::sqlite_integer(v.bytes.length)?,v.time,v.trash])?;
                }
            }
        }
        checkpoint(&db, &store.inner.journal)?;
        drop(db);
        self.verify_initial_rollback(store)?;
        if plan.previous_journal_absent {
            store.restore_collection_import_journal_absence()?;
        }
        meguri_model::faultpoint!("replace.media.rollback.after_journal")?;
        Ok(())
    }
    pub fn discard_unpublished(&self, store: &MediaStore) -> Result<()> {
        let (path, _) = self.validate(store)?;
        self.cleanup(store)?;
        fs::remove_file(path.join("plan.json"))?;
        fs::remove_dir(&path)?;
        atomic_file::sync_directory(store.staging_dir())?;
        Ok(())
    }
    /// Only after R6 durably completes. Keep trash and backup mapping forever;
    /// explicit Empty Trash alone may delete retained media versions.
    pub fn cleanup(&self, store: &MediaStore) -> Result<()> {
        let (path, plan) = self.validate(store)?;
        let mut names = BTreeSet::new();
        names.insert("old-journal.db".to_owned());
        for i in 0..plan.actions.len() {
            names.insert(i.to_string());
        }
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || !names.contains(&entry.file_name().to_string_lossy().into_owned())
                    && entry.file_name() != "plan.json"
            {
                return Err(invalid());
            }
        }
        for name in names {
            fs::remove_file(path.join(name))?;
        }
        for a in &plan.actions {
            for v in a.old.iter().chain(std::iter::once(&a.undo)) {
                let parent = store.trash_dir().join(format!("{}-{}", v.time, v.id));
                match fs::remove_dir(parent) {
                    Ok(()) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                        ) => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        atomic_file::sync_directory(store.staging_dir())?;
        Ok(())
    }
}

impl CollectionMediaPlan {
    fn expected(&self) -> BTreeMap<String, Bytes> {
        let mut expected = self.original.clone();
        for a in &self.actions {
            if a.reuse.is_none() {
                for old in &a.old {
                    expected.remove(&old.name);
                }
                expected.insert(a.incoming.name.clone(), a.bytes.clone());
            }
        }
        expected
    }
}
