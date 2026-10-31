//! Ordinary payload files. Paths carry storage ownership, never ACK authority.
use super::{
    BindingId, Error, InboxEntry, InboxReason, InboxValue, StoredInboxEntry, entity_codec as c, incoming, workflow,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

fn directory(root: &Path, binding: BindingId, purpose: &str) -> PathBuf {
    let owner = data_encoding::HEXLOWER.encode(&binding.0);
    root.join("meguri-cloud-staging").join(format!("pa-{owner}-{purpose}"))
}

fn ensure_directory(path: &Path) -> Result<(), Error> {
    // The collection root is already owned by the host. Refuse links at each
    // staging component rather than letting a stored path select a new root.
    let parent = path.parent().ok_or(Error::InvalidData)?;
    for dir in [parent, path] {
        match fs::create_dir(dir) {
            Ok(()) => File::open(dir.parent().ok_or(Error::InvalidData)?)?.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        if !fs::symlink_metadata(dir)?.is_dir() {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<Vec<u8>, Error> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > c::MAX_PAYLOAD_BYTES {
        return Err(Error::InvalidData);
    }
    let mut bytes = Vec::new();
    (&mut file).take(c::MAX_PAYLOAD_BYTES + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 > c::MAX_PAYLOAD_BYTES
        || bytes.len() as u64 != before.len()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Error::InvalidData);
    }
    Ok(bytes)
}

fn write(root: &Path, binding: BindingId, purpose: &str, bytes: &[u8]) -> Result<tempfile::TempPath, Error> {
    c::validate_payload_size(bytes.len() as u64).map_err(|_| Error::InvalidData)?;
    let dir = directory(root, binding, purpose);
    ensure_directory(&dir)?;
    let mut file = tempfile::Builder::new().prefix("payload-").tempfile_in(&dir)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    File::open(&dir)?.sync_all()?;
    Ok(file.into_temp_path())
}

pub(super) fn upload(root: &Path, binding: BindingId, bytes: &[u8]) -> Result<tempfile::TempPath, Error> {
    write(root, binding, "upload", bytes)
}

// Preparation can run outside tracking access. Sweeps must not remove its
// uncommitted copies; the lease ends only after ownership commit or cleanup.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IntakeLease {
    Preparing,
    Published,
    Retired,
}
static PREPARING: Mutex<BTreeMap<PathBuf, IntakeLease>> = Mutex::new(BTreeMap::new());
struct IntakeFile {
    file: Option<tempfile::TempPath>,
    path: PathBuf,
}
impl Drop for IntakeFile {
    fn drop(&mut self) {
        // Keep sweep protection until the TempPath has actually been removed.
        drop(self.file.take());
        PREPARING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.path);
    }
}

fn write_intake(root: &Path, binding: BindingId, bytes: &[u8]) -> Result<IntakeFile, Error> {
    c::validate_payload_size(bytes.len() as u64).map_err(|_| Error::InvalidData)?;
    let dir = directory(root, binding, "inbox");
    ensure_directory(&dir)?;
    // Only creation and lease registration serialize with a sweep. Bytes,
    // hashing/decoding by intake, and durability I/O hold no shared guard.
    let mut preparing = PREPARING.lock().unwrap_or_else(PoisonError::into_inner);
    let file = tempfile::Builder::new().prefix("payload-").tempfile_in(&dir)?;
    let (mut file, path) = file.into_parts();
    preparing.insert(path.to_path_buf(), IntakeLease::Preparing);
    let owned = IntakeFile {
        path: path.to_path_buf(),
        file: Some(path),
    };
    drop(preparing);
    file.write_all(bytes)?;
    file.sync_all()?;
    File::open(&dir)?.sync_all()?;
    Ok(owned)
}

/// A failed tracking commit drops these files. A successful commit releases
/// only the files actually named by durable inbox rows (stale fetches may drop).
pub(super) struct Intake {
    pub entries: Vec<InboxEntry>,
    pub refused: Vec<incoming::RefusedEntry>,
    files: Vec<IntakeFile>,
}
impl Intake {
    /// Update lease metadata while tracking access protects the committed inbox paths.
    pub fn publish(&self, live: &[StoredInboxEntry]) {
        let paths = live_paths(live);
        let mut preparing = PREPARING.lock().unwrap_or_else(PoisonError::into_inner);
        for owned in &self.files {
            if let Some(lease) = preparing.get_mut(&owned.path) {
                *lease = if paths.contains(&owned.path) {
                    IntakeLease::Published
                } else {
                    IntakeLease::Retired
                };
            }
        }
    }

    /// Explicit reads finish outside runtime/tracking access. A sweep may have
    /// retired a published reference while this owner was waiting to finish.
    pub fn finish(self) -> Result<(), Error> {
        for mut owned in self.files {
            let mut preparing = PREPARING.lock().unwrap_or_else(PoisonError::into_inner);
            if preparing.get(&owned.path) == Some(&IntakeLease::Published) {
                owned
                    .file
                    .take()
                    .ok_or(Error::InvalidData)?
                    .keep()
                    .map_err(|error| Error::from(error.error))?;
                // Release atomically with keep: a later sweep must see a
                // finished file, rather than retire an already-kept lease.
                preparing.remove(&owned.path);
            }
            drop(preparing);
            drop(owned);
        }
        Ok(())
    }

    pub fn commit(self, live: &[StoredInboxEntry]) -> Result<(), Error> {
        self.publish(live);
        self.finish()
    }
}

pub(super) fn intake(
    root: &Path,
    binding: BindingId,
    key: &[u8; 32],
    records: &[workflow::FetchedRecord],
    at: i64,
) -> Result<Intake, Error> {
    let mut output = Intake {
        entries: Vec::new(),
        refused: Vec::new(),
        files: Vec::new(),
    };
    for input in records {
        let record = &input.record;
        if record.name == "epoch" {
            if input.asset_path.is_some() {
                return Err(Error::InvalidData);
            }
            continue;
        }
        let (mut entries, mut refused) = incoming::classify(key, std::slice::from_ref(record), at);
        // Structural generation intake is deliberately outside this adapter.
        let ordinary = matches!(c::Entity::decode(&record.envelope),
            Ok(c::DecodeOutcome::Supported(ref entity)) if entity.fields.generation.is_none());
        if ordinary && entries.first().is_some_and(|entry| entry.reason == InboxReason::Asset) {
            // Source failures are retryable asset facts. Destination failures
            // propagate: without durable storage no subsequent cursor is safe.
            if let Some(bytes) = input.asset_path.as_deref().and_then(|path| read(path).ok()) {
                match c::receive(key, &record.name, &record.envelope, c::AssetInput::Bytes(&bytes)) {
                    Ok(received) => {
                        let valid = received.payload.as_ref().is_some_and(|payload| {
                            payload.kind() == received.entity.fields.kind
                                && (payload.kind() == c::Kind::MediaName
                                    || super::row_payload::payload_key(payload).ok().as_ref()
                                        == Some(&received.entity.fields.key))
                        });
                        if valid {
                            let owned = write_intake(root, binding, &bytes)?;
                            let entry = entries.first_mut().ok_or(Error::InvalidData)?;
                            entry.reason = InboxReason::Ready;
                            let InboxValue::Record { staged_asset, .. } = &mut entry.value else {
                                return Err(Error::InvalidData);
                            };
                            *staged_asset = Some(owned.path.to_string_lossy().into_owned());
                            output.files.push(owned);
                        } else {
                            entries.clear();
                            refused.push(refusal(record, &received.entity, at));
                        }
                    }
                    Err(c::ReceiveError::Refused(_)) => {
                        entries.clear();
                        if let Ok(c::DecodeOutcome::Supported(entity)) = c::Entity::decode(&record.envelope) {
                            refused.push(refusal(record, &entity, at));
                        }
                    }
                    Err(_) => {}
                }
            }
        }
        output.entries.extend(entries);
        output.refused.extend(refused);
    }
    Ok(output)
}

fn refusal(record: &workflow::Saved, entity: &c::Entity, at: i64) -> incoming::RefusedEntry {
    incoming::RefusedEntry {
        record_name: record.name.clone(),
        reason: "invalid_body".into(),
        envelope: super::OpaqueBytes(record.envelope.clone()),
        origin: Some(entity.fields.origin),
        erev: Some(entity.fields.erev),
        seen_at_ms: at,
    }
}

fn live_paths(entries: &[StoredInboxEntry]) -> BTreeSet<PathBuf> {
    entries
        .iter()
        .filter_map(|entry| match &entry.entry.value {
            InboxValue::Record {
                staged_asset: Some(path),
                ..
            } => Some(PathBuf::from(path)),
            _ => None,
        })
        .collect()
}

fn owned(root: &Path, binding: BindingId, path: &Path) -> Result<(), Error> {
    let dir = directory(root, binding, "inbox");
    if path.parent() != Some(dir.as_path())
        || !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("payload-"))
        || !fs::symlink_metadata(dir.parent().ok_or(Error::InvalidData)?)?.is_dir()
        || !fs::symlink_metadata(&dir)?.is_dir()
    {
        return Err(Error::InvalidData);
    }
    Ok(())
}

pub(super) fn read_owned(root: &Path, binding: BindingId, path: &str) -> Result<Vec<u8>, Error> {
    let path = Path::new(path);
    owned(root, binding, path)?;
    read(path)
}

pub(super) fn receive_owned(
    root: &Path,
    binding: BindingId,
    key: &[u8; 32],
    name: &str,
    envelope: &[u8],
    staged_asset: Option<&str>,
) -> Result<c::Received, c::ReceiveError> {
    let bytes = staged_asset.map(|path| read_owned(root, binding, path));
    let asset = match &bytes {
        Some(Ok(bytes)) => c::AssetInput::Bytes(bytes),
        Some(Err(_)) => c::AssetInput::Unreadable,
        None => c::AssetInput::Absent,
    };
    c::receive(key, name, envelope, asset)
}

/// Remove the previous path after its inbox reference has been replaced.
/// Generated paths are unique and cannot be reused by later intake, allowing
/// removal outside runtime and tracking access.
pub(super) fn discard_replaced(
    root: &Path,
    binding: BindingId,
    old: &StoredInboxEntry,
    live: &[StoredInboxEntry],
) -> Result<(), Error> {
    let InboxValue::Record {
        staged_asset: Some(path),
        ..
    } = &old.entry.value
    else {
        return Ok(());
    };
    let path = Path::new(path);
    if live_paths(live).contains(path) {
        return Ok(());
    }
    owned(root, binding, path)?;
    match fs::remove_file(path) {
        Ok(()) => File::open(path.parent().ok_or(Error::InvalidData)?)?.sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Requires tracking access. Remove only this binding's unused inbox files;
/// pending intents retain their rows until collection and WAL work settles.
pub(super) fn sweep(root: &Path, binding: BindingId, live: &[StoredInboxEntry]) -> Result<(), Error> {
    let dir = directory(root, binding, "inbox");
    match fs::symlink_metadata(&dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() => return Err(Error::InvalidData),
        Ok(_) => {}
    }
    let paths = live_paths(live);
    let mut preparing = PREPARING.lock().unwrap_or_else(PoisonError::into_inner);
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        owned(root, binding, &path)?;
        if !paths.contains(&path) {
            match preparing.get_mut(&path) {
                Some(lease) if *lease == IntakeLease::Published => *lease = IntakeLease::Retired,
                Some(_) => {}
                None => fs::remove_file(path)?,
            }
        }
    }
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// Called only before installing an SDK, after the previous SDK has drained.
pub(super) fn sweep_uploads(root: &Path, binding: BindingId, leased: &BTreeSet<PathBuf>) -> Result<(), Error> {
    let dir = directory(root, binding, "upload");
    match fs::symlink_metadata(&dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() => return Err(Error::InvalidData),
        Ok(_) => {}
    }
    if !fs::symlink_metadata(dir.parent().ok_or(Error::InvalidData)?)?.is_dir() {
        return Err(Error::InvalidData);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("payload-"))
        {
            return Err(Error::InvalidData);
        }
        if !leased.contains(&entry.path()) {
            fs::remove_file(entry.path())?;
        }
    }
    File::open(dir)?.sync_all()?;
    Ok(())
}
