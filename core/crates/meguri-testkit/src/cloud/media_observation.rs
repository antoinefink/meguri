//! Read-only media evidence at a caller-owned, idle device barrier.
//!
//! Bytes come from the retained Host's MediaStore descriptor, not its journal
//! hashes or CloudKit payloads. This is neither a Q5 verdict nor install/trash
//! evidence. Metadata vectors are complete public API readouts, without inspect
//! caps; only the byte buffer and each file read have bounds.
use meguri_engine::media::MediaService;
use meguri_host::CoreHost;
use meguri_storage::media::{MediaEntry, MediaSyncState};
use meguri_sync::cloud::{self, entity_codec as codec};
use serde::{Deserialize, Serialize};
use sha1::{Digest as _, Sha1};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt, fs, io::Read, os::unix::fs::MetadataExt, path::Path};

pub const READ_BUFFER_BYTES: usize = 65_536;
pub const MAX_FILE_BYTES: u64 = cloud::media::MAX_ASSET_BYTES;

/// Unavailable evidence must remain distinct from an empty observed list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Missing {
    RuntimeMissingNameList,
    /// Entity pages expose dirty marks, not withheld/superseded/file_missing
    /// ledger dispositions or the policy's sendable/pending classification.
    RuntimeMediaNameDisposition,
    InstallAndTrashHistory,
    /// open_file guards its directory and terminal entry with NOFOLLOW, but
    /// exposes no directory descriptor pinned to the original owner's inode.
    OriginalRootPinnedRead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    OwnerUnavailable,
    UnsafeRoot,
    RootChanged,
    UnsafeName,
    UnsafeFile,
    FileRead,
    FileChanged,
    FileTooLarge,
    MetadataRead,
    MetadataChanged,
    RuntimeRead,
    RuntimeChanged,
}
impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never forward storage/OS errors, which may contain paths or names.
        write!(f, "media observation {self:?}")
    }
}
impl std::error::Error for Refusal {}
pub type Result<T> = std::result::Result<T, Refusal>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bytes {
    pub sha256: [u8; 32],
    pub sha1: [u8; 20],
    pub size: u64,
    pub read_calls: u64,
    pub largest_read: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileObservation {
    Present(Bytes),
    /// A filesystem observation only; never Runtime mediaMissing.
    Absent,
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalMetadata {
    pub sha1: Option<[u8; 20]>,
    pub size: u64,
    pub live: bool,
    /// Manual-transfer pending state, not CloudKit pending state.
    pub manual_pending: bool,
    pub manual_sync_state: ManualSyncState,
    pub pending_download: bool,
    pub change_counter: u64,
    pub tracked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManualSyncState {
    None,
    Live,
    Deleted,
    Awaiting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameObservation {
    pub name_sha256: [u8; 32],
    pub case_key_sha256: [u8; 32],
    pub journal: Option<JournalMetadata>,
    /// Lookup may use a different stored case spelling.
    pub stored_name_sha256: Option<[u8; 32]>,
    pub file: FileObservation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScopeObservation {
    NoRuntime,
    Unbound,
    Bound {
        scope_sha256: [u8; 32],
        account_sha256: [u8; 32],
        binding_sha256: [u8; 32],
        epoch_sha256: [u8; 32],
        state: cloud::BindingState,
        stored_usable: bool,
        identity_verified: bool,
        scan_required: bool,
        scan_running: bool,
        scan_drained: bool,
        tracking_stopped: bool,
    },
}

/// Canonical local entity metadata; no envelope, asset or content is retained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityObservation {
    pub case_key_sha256: [u8; 32],
    pub name_sha256: [u8; 32],
    pub sha256: [u8; 32],
    pub sha1: [u8; 20],
    pub size: u64,
    pub tombstone: bool,
    pub local_rev: u64,
    pub acked_rev: u64,
    pub has_dirty_mark: bool,
    pub in_flight: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub generation: u64,
    pub scope: ScopeObservation,
    pub names: Vec<NameObservation>,
    pub entities: Vec<EntityObservation>,
    pub missing: [Missing; 4],
}

/// Retain this before fault mutations. No caller-provided root or expected
/// manifest can enter an observation. Drop it before closing/replacing Host.
pub struct MediaObserver<'host> {
    host: &'host CoreHost,
    media: MediaService,
    generation: u64,
    collection_native: (u64, u64),
    directory_native: (u64, u64),
    media_native: (u64, u64),
}
impl fmt::Debug for MediaObserver<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MediaObserver")
    }
}

fn native(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}
fn directory_native(path: &Path) -> Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Refusal::UnsafeRoot)?;
    if !metadata.is_dir() {
        return Err(Refusal::UnsafeRoot);
    }
    Ok(native(&metadata))
}
fn digest(bytes: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn safe_name(name: &str) -> bool {
    // lookup also validates; this check prevents diagnostic path construction
    // for a malformed managed name. It grants no filesystem read authority.
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}
fn journal(entry: &MediaEntry, tracked: bool) -> Result<JournalMetadata> {
    let sha1 = entry
        .sha1
        .as_ref()
        .map(|value| {
            let mut bytes = [0; 20];
            if value.len() != 40 || !value.is_ascii() {
                return Err(Refusal::MetadataRead);
            }
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&value[2 * index..2 * index + 2], 16).map_err(|_| Refusal::MetadataRead)?;
            }
            Ok(bytes)
        })
        .transpose()?;
    Ok(JournalMetadata {
        sha1,
        size: entry.size,
        live: entry.live,
        manual_pending: entry.dirty,
        manual_sync_state: match entry.sync_state {
            MediaSyncState::None => ManualSyncState::None,
            MediaSyncState::Live => ManualSyncState::Live,
            MediaSyncState::Deleted => ManualSyncState::Deleted,
            MediaSyncState::Awaiting => ManualSyncState::Awaiting,
        },
        pending_download: entry.pending_download,
        change_counter: entry.change_counter,
        tracked,
    })
}

impl<'host> MediaObserver<'host> {
    pub fn new(host: &'host CoreHost) -> Result<Self> {
        let collection = host.collection().map_err(|_| Refusal::OwnerUnavailable)?;
        let evidence = collection.open_evidence().map_err(|_| Refusal::OwnerUnavailable)?;
        let media = host.media().map_err(|_| Refusal::OwnerUnavailable)?;
        let value = Self {
            host,
            generation: evidence.generation.0,
            collection_native: evidence.native_file,
            directory_native: directory_native(collection.path().parent().ok_or(Refusal::UnsafeRoot)?)?,
            media_native: directory_native(media.store().media_dir())?,
            media,
        };
        value.check_owner()?;
        Ok(value)
    }

    fn check_owner(&self) -> Result<()> {
        self.media
            .check_generation(self.generation)
            .map_err(|_| Refusal::OwnerUnavailable)?;
        let collection = self.host.collection().map_err(|_| Refusal::OwnerUnavailable)?;
        let metadata = fs::symlink_metadata(collection.path()).map_err(|_| Refusal::OwnerUnavailable)?;
        if !metadata.is_file() || native(&metadata) != self.collection_native {
            return Err(Refusal::OwnerUnavailable);
        }
        if collection.generation().0 != self.generation {
            return Err(Refusal::OwnerUnavailable);
        }
        if directory_native(collection.path().parent().ok_or(Refusal::UnsafeRoot)?)? != self.directory_native
            || directory_native(self.media.store().media_dir())? != self.media_native
        {
            return Err(Refusal::RootChanged);
        }
        Ok(())
    }

    /// Call only while the device's producers are idle. Separate public reads
    /// do not form an atomic collection/journal/runtime/filesystem snapshot.
    pub fn observe(&self) -> Result<Observation> {
        self.check_owner()?;
        let tracked = self.media.tracked_names().map_err(|_| Refusal::MetadataRead)?;
        let entries = self.media.store().entries().map_err(|_| Refusal::MetadataRead)?;
        let tracked_by_name: BTreeMap<_, _> = tracked.names.iter().map(|name| (name.name.as_str(), name)).collect();
        let mut names = BTreeMap::new();
        for entry in &entries {
            let tracked_name = tracked_by_name.get(entry.name.as_str());
            let key = tracked_name
                .map(|name| name.case_key.clone())
                .unwrap_or_else(|| entry.name.to_lowercase());
            names.insert(entry.name.clone(), (key, Some(journal(entry, tracked_name.is_some())?)));
        }
        // The journal snapshot must not silently lose a tracked row.
        if tracked.names.iter().any(|name| !names.contains_key(&name.name)) {
            return Err(Refusal::MetadataChanged);
        }
        let (scope, entities) = self.runtime_entities(&mut names)?;
        let mut observed = Vec::with_capacity(names.len());
        for (name, (key, journal)) in names {
            self.check_owner()?;
            let (stored_name_sha256, file) = match self.read_file(&name) {
                Ok(value) => value,
                Err(reason) => (None, FileObservation::Refused(reason)),
            };
            self.check_owner()?;
            observed.push(NameObservation {
                name_sha256: digest(name.as_bytes()),
                case_key_sha256: digest(key.as_bytes()),
                journal,
                stored_name_sha256,
                file,
            });
        }
        if self.media.store().entries().map_err(|_| Refusal::MetadataRead)? != entries
            || self.media.tracked_names().map_err(|_| Refusal::MetadataRead)? != tracked
        {
            return Err(Refusal::MetadataChanged);
        }
        self.check_owner()?;
        Ok(Observation {
            generation: self.generation,
            scope,
            names: observed,
            entities,
            missing: [
                Missing::RuntimeMissingNameList,
                Missing::RuntimeMediaNameDisposition,
                Missing::InstallAndTrashHistory,
                Missing::OriginalRootPinnedRead,
            ],
        })
    }

    fn runtime_entities(
        &self,
        names: &mut BTreeMap<String, (String, Option<JournalMetadata>)>,
    ) -> Result<(ScopeObservation, Vec<EntityObservation>)> {
        let Some(runtime) = self.host.cloud_runtime() else {
            return Ok((ScopeObservation::NoRuntime, Vec::new()));
        };
        let status = runtime.status().map_err(|_| Refusal::RuntimeRead)?;
        let Some(binding) = status.binding else {
            return Ok((ScopeObservation::Unbound, Vec::new()));
        };
        if status.generation.map(|g| g.0) != Some(self.generation) {
            return Err(Refusal::RuntimeChanged);
        }
        let scope = ScopeObservation::Bound {
            scope_sha256: digest(serde_json::to_vec(&binding.scope).map_err(|_| Refusal::RuntimeRead)?),
            account_sha256: digest(&binding.facts.account.0),
            binding_sha256: digest(binding.id.0),
            epoch_sha256: digest(binding.facts.epoch.0),
            state: binding.facts.state,
            stored_usable: status.stored_usable,
            identity_verified: status.identity_verified,
            scan_required: status.scan_required,
            scan_running: status.scan_running,
            scan_drained: status.scan_drained,
            tracking_stopped: status.tracking_stopped,
        };
        let mut after: Option<Vec<u8>> = None;
        let mut entities = Vec::new();
        loop {
            let page = runtime
                .entity_facts_page(codec::Kind::MediaName, after.as_deref())
                .map_err(|_| Refusal::RuntimeRead)?;
            if page.is_empty() {
                break;
            }
            for entity in page {
                if after
                    .as_ref()
                    .is_some_and(|key| entity.key.as_slice() <= key.as_slice())
                {
                    return Err(Refusal::RuntimeChanged);
                }
                after = Some(entity.key.clone());
                let codec::Payload::MediaName(payload) = entity.payload else {
                    return Err(Refusal::RuntimeRead);
                };
                let name = std::str::from_utf8(&payload.name).map_err(|_| Refusal::UnsafeName)?;
                let key = std::str::from_utf8(&entity.key).map_err(|_| Refusal::UnsafeName)?;
                if !safe_name(name) || name.to_lowercase() != key {
                    return Err(Refusal::UnsafeName);
                }
                names.entry(name.to_owned()).or_insert_with(|| (key.to_owned(), None));
                entities.push(EntityObservation {
                    case_key_sha256: digest(&entity.key),
                    name_sha256: digest(&payload.name),
                    sha256: payload.sha256,
                    sha1: payload.sha1,
                    size: payload.size,
                    tombstone: entity.tombstone_at.is_some(),
                    local_rev: entity.local_rev,
                    acked_rev: entity.acked_rev,
                    has_dirty_mark: entity.mark.is_some(),
                    in_flight: entity.in_flight.len(),
                });
            }
        }
        let end = runtime.status().map_err(|_| Refusal::RuntimeRead)?;
        if end.binding.as_ref() != Some(&binding)
            || end.generation != status.generation
            || end.change_counter != status.change_counter
        {
            return Err(Refusal::RuntimeChanged);
        }
        Ok((scope, entities))
    }

    fn read_file(&self, name: &str) -> Result<(Option<[u8; 32]>, FileObservation)> {
        if !safe_name(name) {
            return Err(Refusal::UnsafeName);
        }
        let stored = self.media.store().lookup(name).map_err(|_| Refusal::FileRead)?;
        let Some(stored) = stored else {
            return match fs::symlink_metadata(self.media.store().media_dir().join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((None, FileObservation::Absent)),
                Ok(_) => Err(Refusal::UnsafeFile),
                Err(_) => Err(Refusal::FileRead),
            };
        };
        if !safe_name(&stored) {
            return Err(Refusal::UnsafeName);
        }
        let mut file = self
            .media
            .store()
            .open_file(&stored)
            .map_err(|_| Refusal::FileRead)?
            .ok_or(Refusal::FileChanged)?;
        let before = file.metadata().map_err(|_| Refusal::FileRead)?;
        if !before.is_file() {
            return Err(Refusal::UnsafeFile);
        }
        if before.len() > MAX_FILE_BYTES {
            return Err(Refusal::FileTooLarge);
        }
        let mut sha256 = Sha256::new();
        let mut sha1 = Sha1::new();
        let mut buffer = [0; READ_BUFFER_BYTES];
        let (mut size, mut read_calls, mut largest_read) = (0_u64, 0_u64, 0);
        loop {
            let limit = (MAX_FILE_BYTES + 1 - size).min(READ_BUFFER_BYTES as u64) as usize;
            let count = file.read(&mut buffer[..limit]).map_err(|_| Refusal::FileRead)?;
            read_calls += 1;
            largest_read = largest_read.max(count);
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > MAX_FILE_BYTES {
                return Err(Refusal::FileTooLarge);
            }
            sha256.update(&buffer[..count]);
            sha1.update(&buffer[..count]);
        }
        let end = file.metadata().map_err(|_| Refusal::FileRead)?;
        let named =
            fs::symlink_metadata(self.media.store().media_dir().join(&stored)).map_err(|_| Refusal::FileChanged)?;
        if native(&before) != native(&end)
            || before.len() != size
            || end.len() != size
            || !named.is_file()
            || native(&named) != native(&end)
            || named.len() != end.len()
            || named.mtime() != end.mtime()
            || named.mtime_nsec() != end.mtime_nsec()
            || named.ctime() != end.ctime()
            || named.ctime_nsec() != end.ctime_nsec()
            || before.mtime() != end.mtime()
            || before.mtime_nsec() != end.mtime_nsec()
            || before.ctime() != end.ctime()
            || before.ctime_nsec() != end.ctime_nsec()
            || self
                .media
                .store()
                .lookup(name)
                .map_err(|_| Refusal::FileRead)?
                .as_deref()
                != Some(stored.as_str())
        {
            return Err(Refusal::FileChanged);
        }
        Ok((
            Some(digest(stored.as_bytes())),
            FileObservation::Present(Bytes {
                sha256: sha256.finalize().into(),
                sha1: sha1.finalize().into(),
                size,
                read_calls,
                largest_read,
            }),
        ))
    }
}
