//! Production V14 collection adoption. The platform supplies record replies;
//! Runtime owns the slot, unpublished stage, validation and durable transitions.
use super::*;
use crate::cloud::{
    adoption::*,
    control_codec::{DecodeOutcome, Epoch, EpochState, MediaRoot},
    publication::*,
    workflow::EntityFacts,
};
use meguri_engine::{
    Engine, LongOp, ReaderLease, SlotGuard,
    device_state::{AccountBinding, Publication, PublicationDecision, PublicationId},
    media::{MediaJobKind, MediaJobLease, MediaService},
    replace::{CloudAdoptInput, ReplaceReason, ReplacementStage},
};
use meguri_model::{
    col::Col,
    time::{EpochMillis, EpochSecs},
};
use meguri_storage::{
    atomic_file::{AtomicFile, Purpose},
    space::SpaceProvider,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};

pub(super) struct AdoptionOwner {
    slot: SlotGuard,
    generation: meguri_engine::Generation,
    pub(super) cleanup_evidence: Mutex<
        Option<(
            meguri_engine::OpenEvidence,
            Option<meguri_engine::device_state::CloudAdoptActivation>,
        )>,
    >,
    directory: PathBuf,
    stage: Mutex<Option<ReplacementStage>>,
    spool_sizes: Mutex<SpoolSizes>,
    next: Mutex<AdoptionNext>,
    // No page cursor or pending metadata survives this in-memory owner.
    pending_page: Mutex<Option<PendingPage>>,
    #[cfg(feature = "test-hooks")]
    next_contention: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    uncertain_activation: std::sync::atomic::AtomicBool,
}
enum AdoptionNext {
    Pending(AdoptionTask),
    Claimed,
    // No reply or local work can claim this state. Only abort cleanup may run.
    CleanupOnly,
}
struct PendingPage {
    records: VecDeque<StagingRecord>,
    next: Option<OpaqueBytes>,
}
fn payload_task(item: &StagingRecord) -> Result<AdoptionTask, RuntimeError> {
    Ok(AdoptionTask::FetchPayload {
        name: item.name.clone(),
        encrypted: OpaqueBytes(item.record.encrypted.clone().ok_or_else(refused)?),
        system_fields: item.record.system_fields.clone(),
    })
}
// Retain one adoption for close cleanup; this grants no collection access.
pub(super) struct SuspendedAdoptionCleanup {
    pub(super) op: AdoptOp,
    pub(super) incarnation: u64,
    pub(super) store: Arc<super::open::RetainedAdoptionStore>,
}
impl AdoptionOwner {
    fn set_next(&self, task: AdoptionTask) {
        *self.next.lock().unwrap_or_else(PoisonError::into_inner) = AdoptionNext::Pending(task);
    }
    fn claim(&self, task: &AdoptionTask) -> Result<(), RuntimeError> {
        self.claim_with_work(task, &crate::cloud::policy::PolicyWork::default())
    }
    fn claim_with_work(
        &self,
        task: &AdoptionTask,
        work: &crate::cloud::policy::PolicyWork,
    ) -> Result<(), RuntimeError> {
        #[cfg(feature = "test-hooks")]
        if matches!(self.next.try_lock(), Err(std::sync::TryLockError::WouldBlock))
            && let Some(sender) = self
                .next_contention
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        {
            let _ = sender.send(());
        }
        let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(&*next, AdoptionNext::Pending(pending) if pending == task) {
            return Err(RuntimeError::StaleIncarnation);
        }
        // After claim, restore the task or abort even if cancellation arrives.
        work.effect()?;
        *next = AdoptionNext::Claimed;
        Ok(())
    }
}
fn refused() -> RuntimeError {
    Error::InvalidData.into()
}
fn storage_io(e: std::io::Error) -> RuntimeError {
    meguri_storage::StorageError::from(e).into()
}
fn publication_id(id: [u8; 16]) -> PublicationId {
    PublicationId::from_bytes(id)
}
fn epoch(op: &AdoptOp) -> Result<Epoch, RuntimeError> {
    let record = op.epoch_record.as_ref().ok_or_else(refused)?;
    let DecodeOutcome::Supported(epoch) =
        Epoch::decode(record.encrypted.as_deref().ok_or_else(refused)?).map_err(|_| refused())?
    else {
        return Err(refused());
    };
    if epoch.epoch_id != op.epoch.0 || epoch.state != EpochState::Complete || epoch.ver != 18 {
        return Err(refused());
    }
    Ok(epoch)
}
fn root(op: &AdoptOp) -> Result<MediaRoot, RuntimeError> {
    let record = op.media_root.as_ref().ok_or_else(refused)?;
    let DecodeOutcome::Supported(root) =
        MediaRoot::decode(record.encrypted.as_deref().ok_or_else(refused)?).map_err(|_| refused())?
    else {
        return Err(refused());
    };
    Ok(root)
}
/// Inline spools retain their original JSON shape. Asset spools add only an
/// operation-relative filename; canonical bodies never enter spool JSON.
#[derive(serde::Serialize, serde::Deserialize)]
struct StagedRecord {
    name: String,
    envelope: Vec<u8>,
    system_fields: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_file: Option<String>,
}
fn saved(path: &Path) -> Result<StagedRecord, RuntimeError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(storage_io)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(storage_io)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(refused());
    }
    serde_json::from_slice(&bytes).map_err(|_| refused())
}
fn received_bytes(op: &AdoptOp, record: &StagedRecord, bytes: Option<&[u8]>) -> Result<codec::Received, RuntimeError> {
    let received = codec::receive(
        &epoch(op)?.record_key,
        &record.name,
        &record.envelope,
        bytes.map_or(codec::AssetInput::Absent, codec::AssetInput::Bytes),
    )
    .map_err(|_| refused())?;
    // K18.4 retains MediaName tombstones as ACKs with no collection row.
    // Other tombstone kinds and K19 generations remain unsupported.
    if received.entity.fields.generation.is_some()
        || (received.entity.fields.tombstone && received.entity.fields.kind != codec::Kind::MediaName)
        || received.payload.is_none()
        || received.entity.fields.erev == 0
        || record.system_fields.is_empty()
        || (bytes.is_some() && received.entity.payload.is_some())
    {
        #[cfg(feature = "test-hooks")]
        {
            let kind = match received.entity.fields.kind {
                codec::Kind::Note => "Note",
                codec::Kind::Card => "Card",
                codec::Kind::Revlog => "Revlog",
                codec::Kind::Deck => "Deck",
                codec::Kind::Preset => "Preset",
                codec::Kind::Notetype => "Notetype",
                codec::Kind::Tag => "Tag",
                codec::Kind::Config => "Config",
                codec::Kind::MediaName => "MediaName",
            };
            eprintln!(
                "cloud-adoption stage=received_validation record_kind={kind} generation_absent={} tombstone_absent={} payload_present={} revision_valid={} system_fields_present={} payload_form_valid={}",
                received.entity.fields.generation.is_none(),
                !received.entity.fields.tombstone,
                received.payload.is_some(),
                received.entity.fields.erev != 0,
                !record.system_fields.is_empty(),
                !(bytes.is_some() && received.entity.payload.is_some()),
            );
        }
        return Err(refused());
    }
    if let Some(payload) = &received.payload
        && crate::cloud::row_payload::payload_key(payload)? != received.entity.fields.key
    {
        return Err(refused());
    }
    Ok(received)
}
fn received(directory: &Path, op: &AdoptOp, record: &StagedRecord) -> Result<codec::Received, RuntimeError> {
    let bytes = record
        .payload_file
        .as_ref()
        .map(|name| {
            if name != &format!("{}.pa", record.name) || Path::new(name).components().count() != 1 {
                return Err(refused());
            }
            super::payload_asset::read(&directory.join("payloads").join(name)).map_err(RuntimeError::from)
        })
        .transpose()?;
    received_bytes(op, record, bytes.as_deref())
}
fn spool(directory: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory.join("incoming")).map_err(storage_io)? {
        let entry = entry.map_err(storage_io)?;
        if !entry.file_type().map_err(storage_io)?.is_file() {
            return Err(refused());
        }
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}
// Count actual entry inspections, including bounded stats, rather than logical
// writes. Thread-local so unrelated tests cannot change a witness's count.
#[cfg(feature = "test-hooks")]
thread_local! {
    static ADOPTION_SPACE_ENTRY_VISITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
#[cfg(feature = "test-hooks")]
fn space_entry_visit() {
    ADOPTION_SPACE_ENTRY_VISITS.with(|count| count.set(count.get() + 1));
}
fn space_metadata(path: &Path) -> Result<fs::Metadata, RuntimeError> {
    #[cfg(feature = "test-hooks")]
    space_entry_visit();
    fs::symlink_metadata(path).map_err(storage_io)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpoolStamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl SpoolStamp {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            bytes: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}
const SPOOLS: [&str; 2] = ["incoming", "payloads"];

// Only the private, non-cloneable ReplacementStage's exact claimed task writes
// these directories. SQL writes affect root files, never either spool. No
// owner/ledger survives reopen: recovery discards the unpublished stage.
struct SpoolSizes {
    root: (u64, u64),
    directories: [SpoolStamp; 2],
    files: BTreeMap<PathBuf, SpoolStamp>,
    bytes: u64,
    usable: bool,
}
fn regular_size(meta: &fs::Metadata) -> Result<u64, RuntimeError> {
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(refused());
    }
    Ok(meta.len())
}
impl SpoolSizes {
    fn publish(
        &mut self,
        file: AtomicFile,
        directory: &Path,
        spool: &str,
        name: &str,
        length: u64,
    ) -> Result<(), RuntimeError> {
        if let Err(error) = file.publish(|_| Ok(())) {
            // Rename may have succeeded before directory sync failed. Never
            // resume cached checks on that uncertain result; caller aborts.
            self.usable = false;
            return Err(error.into());
        }
        self.published(directory, spool, name, length)
    }
    fn read(directory: &Path) -> Result<Self, RuntimeError> {
        let root = space_metadata(directory)?;
        // Verify the TempDir's actual privacy, not just its reserved name.
        if !root.is_dir() || root.mode() & 0o077 != 0 {
            return Err(refused());
        }
        let mut directories = Vec::new();
        let mut files = BTreeMap::new();
        let mut bytes = 0_u64;
        for spool in SPOOLS {
            let path = directory.join(spool);
            let meta = space_metadata(&path)?;
            if !meta.is_dir() {
                return Err(refused());
            }
            directories.push(SpoolStamp::of(&meta));
            // Flat spools only: unexpected directories/types are refused,
            // never skipped or treated as empty (also bounds traversal depth).
            for entry in fs::read_dir(&path).map_err(storage_io)? {
                let entry = entry.map_err(storage_io)?;
                let meta = space_metadata(&entry.path())?;
                let size = regular_size(&meta)?;
                let limit = if spool == "incoming" {
                    8 * 1024 * 1024
                } else {
                    codec::MAX_PAYLOAD_BYTES
                };
                if size > limit {
                    return Err(refused());
                }
                bytes = bytes.checked_add(size).ok_or_else(refused)?;
                files.insert(Path::new(spool).join(entry.file_name()), SpoolStamp::of(&meta));
            }
            if directories.last() != Some(&SpoolStamp::of(&space_metadata(&path)?)) {
                return Err(refused());
            }
        }
        let sizes = Self {
            root: (root.dev(), root.ino()),
            directories: directories.try_into().map_err(|_| refused())?,
            files,
            bytes,
            usable: true,
        };
        sizes.current_size(directory)?;
        Ok(sizes)
    }
    fn current_size(&self, directory: &Path) -> Result<u64, RuntimeError> {
        let root = space_metadata(directory)?;
        if !self.usable || !root.is_dir() || root.mode() & 0o077 != 0 || self.root != (root.dev(), root.ino()) {
            return Err(refused());
        }
        let mut seen = [false; 2];
        let mut bytes = self.bytes;
        // Every existing capacity guard still enumerates/stats the flat root,
        // including SQLite, WAL, SHM, journal and any other regular root files.
        for entry in fs::read_dir(directory).map_err(storage_io)? {
            let entry = entry.map_err(storage_io)?;
            let meta = space_metadata(&entry.path())?;
            if let Some(index) = SPOOLS
                .iter()
                .position(|name| entry.file_name().as_os_str() == std::ffi::OsStr::new(name))
            {
                if !meta.is_dir() || SpoolStamp::of(&meta) != self.directories[index] {
                    return Err(refused());
                }
                seen[index] = true;
            } else {
                bytes = bytes.checked_add(regular_size(&meta)?).ok_or_else(refused)?;
            }
        }
        if seen != [true; 2] {
            return Err(refused());
        }
        Ok(bytes)
    }
    fn published(&mut self, directory: &Path, spool: &str, name: &str, length: u64) -> Result<(), RuntimeError> {
        // Called only after AtomicFile::publish succeeds. A failure here, or
        // an uncertain publish, makes the owner unusable until it is discarded.
        let was_usable = self.usable;
        self.usable = false;
        let index = SPOOLS.iter().position(|s| *s == spool).ok_or_else(refused)?;
        if !was_usable
            || !matches!(
                Path::new(name).components().next(),
                Some(std::path::Component::Normal(_))
            )
            || Path::new(name).components().count() != 1
        {
            return Err(refused());
        }
        let relative = Path::new(spool).join(name);
        let meta = space_metadata(&directory.join(&relative))?;
        if regular_size(&meta)? != length {
            return Err(refused());
        }
        let dir = space_metadata(&directory.join(spool))?;
        let prior = &self.directories[index];
        if !dir.is_dir() || (dir.dev(), dir.ino()) != (prior.device, prior.inode) {
            return Err(refused());
        }
        let bytes = self
            .bytes
            .checked_sub(self.files.get(&relative).map_or(0, |old| old.bytes))
            .and_then(|n| n.checked_add(length))
            .ok_or_else(refused)?;
        self.files.insert(relative, SpoolStamp::of(&meta));
        self.bytes = bytes;
        self.directories[index] = SpoolStamp::of(&dir);
        self.usable = true;
        Ok(())
    }
    fn reconcile(&self, directory: &Path) -> Result<(), RuntimeError> {
        let actual = Self::read(directory)?;
        if !self.usable
            || self.root != actual.root
            || self.directories != actual.directories
            || self.files != actual.files
            || self.bytes != actual.bytes
        {
            return Err(refused());
        }
        Ok(())
    }
}
fn check_space(space: &dyn SpaceProvider, directory: &Path, sizes: &SpoolSizes) -> Result<(), RuntimeError> {
    check_space_for(space, directory, 0, sizes)
}
fn check_space_for(
    space: &dyn SpaceProvider,
    directory: &Path,
    additional: u64,
    sizes: &SpoolSizes,
) -> Result<(), RuntimeError> {
    let required = sizes
        .current_size(directory)?
        .checked_add(additional)
        .ok_or_else(refused)?
        .checked_mul(2)
        .and_then(|n| n.checked_add(64 * 1024 * 1024))
        .ok_or_else(refused)?;
    meguri_storage::space::require_space(space, directory, required.max(512 * 1024 * 1024))?;
    Ok(())
}
pub(super) fn remove_adoption_stage(directory: &Path, op: &AdoptOp) -> Result<(), RuntimeError> {
    if !op.staging_name.starts_with(".meguri-stage-")
        || op.staging_name.len() <= ".meguri-stage-".len()
        || Path::new(&op.staging_name).components().count() != 1
    {
        return Err(refused());
    }
    let path = directory.join(&op.staging_name);
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path).map_err(storage_io),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(storage_io(e)),
        _ => Err(refused()),
    }
}
fn require_registered_media(media: &meguri_engine::media::MediaService) -> Result<(), RuntimeError> {
    if !media.store().media_feed_status().map_err(|_| refused())?.complete {
        return Err(refused());
    }
    Ok(())
}
fn inventory_registered_media<'a>(
    collection: &'a Collection,
    media: &MediaService,
) -> Result<(ReaderLease<'a>, MediaJobLease), RuntimeError> {
    // Pin the collection while admitting the media job and inventorying its
    // actual folder. Neither Runtime state nor tracking access is held here.
    let reader = collection.reader(ReaderRole::Cloud)?;
    let job = media.try_job(MediaJobKind::CheckMedia)?;
    media.check_generation(collection.generation().0)?;
    media.store().check_changes().map_err(|_| refused())?;
    require_registered_media(media)?;
    media.check_generation(collection.generation().0)?;
    Ok((reader, job))
}
impl Runtime {
    /// Fixed catalog boundary for the child crash harness only.
    #[cfg(feature = "test-hooks")]
    pub fn arm_adoption_activation_crash_for_testing(&self) {
        meguri_model::faultpoint::arm("cloud.adopt.after_activation", meguri_model::faultpoint::Fault::Abort);
    }
    fn adoption_clock(&self) -> Result<Arc<dyn meguri_model::clock::Clock>, RuntimeError> {
        self.adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData.into())
    }
    fn adoption_owner(&self) -> Result<Arc<AdoptionOwner>, RuntimeError> {
        self.adoption
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(RuntimeError::StaleIncarnation)
    }
    pub fn adoption_op(&self) -> Result<Option<AdoptOp>, RuntimeError> {
        self.read_recovery_op(TrackingStore::adoption_op)
    }
    /// A missing store is absent only for its live NoCollection owner and path.
    pub(super) fn read_recovery_op<T>(
        &self,
        query: impl FnOnce(&TrackingStore) -> Result<Option<T>, Error>,
    ) -> Result<Option<T>, RuntimeError> {
        let read = |s: &State| {
            with_tracking_access(|| match s.store.as_ref() {
                Some(store) => Ok(query(store)?),
                None if self.tracking_file_absent_locked()? => Ok(None),
                None => Err(Error::Database.into()),
            })
        };
        {
            let s = self.state();
            if s.store.is_some() {
                return read(&s);
            }
        }
        self.with_initial_adoption_state(|s| read(s))
    }
    /// Caller holds Runtime state and tracking access; never opens the file.
    fn tracking_file_absent_locked(&self) -> Result<bool, RuntimeError> {
        match self.directory.join(super::open::TRACKING_FILE).symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(storage_io(e)),
            Ok(_) => Ok(false),
        }
    }
    fn write_adoption(&self, op: &AdoptOp) -> Result<(), RuntimeError> {
        let mut s = self.state();
        with_tracking_access(|| Ok(s.store.as_mut().ok_or(Error::Database)?.update_adoption(op)?))
    }
    /// Called once by the newly retained Host, before discovery.
    pub fn recover_initial_cloud_adoption(
        &self,
        engine: &Engine,
        config: &meguri_engine::OpenConfig,
    ) -> Result<(), RuntimeError> {
        let slot = engine.acquire_slot(LongOp::CloudAdopt)?;
        let operation = {
            let _state = self.state();
            with_tracking_access(|| -> Result<Option<AdoptOp>, RuntimeError> {
                if self.tracking_file_absent_locked()? {
                    return Ok(None);
                }
                let mut store = match TrackingStore::open(&self.directory, self.inputs.scope.clone())? {
                    OpenOutcome::Opened(store) => store,
                    _ => return Err(Error::RecoveryRequired.into()),
                };
                store.install_runtime()?;
                Ok(store.adoption_op()?)
            })?
        };
        let expected = operation
            .as_ref()
            .filter(|op| op.file_before.is_none() && op.step == AdoptStep::Ready)
            .map(|op| CloudAdoptInput {
                operation: publication_id(op.id),
                candidate: op.candidate.clone(),
            });
        let prior = operation
            .as_ref()
            .and_then(|op| op.before.as_ref())
            .map(|before| &before.publication);
        let recovered = slot.recover_cloud_adopt_absence(
            &self.directory.join("collection.anki2"),
            config.clone(),
            expected.as_ref(),
            prior,
        )?;
        if recovered.is_some() {
            // The ordinary configured open step binds the actual activated
            // candidate and settles step 9 using its native R6 proof.
            return Ok(());
        }
        {
            let _state = self.state();
            if with_tracking_access(|| self.tracking_file_absent_locked())? {
                return Ok(());
            }
        }
        self.with_initial_adoption_state(|s| {
            with_tracking_access(|| {
                let mut store = match TrackingStore::open(&self.directory, self.inputs.scope.clone())? {
                    OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
                    _ => return Err(Error::RecoveryRequired.into()),
                };
                store.install_runtime()?;
                if let Some(op) = store.adoption_op()? {
                    if op.file_before.is_some() {
                        return Err(Error::RecoveryRefused.into());
                    }
                    // R6 has settled the old absent view. Keep its exact prior
                    // decision; only this operation's unpublished stage is retired.
                    let prior = meguri_engine::device_state::DeviceState::for_collection(
                        &self.directory.join("collection.anki2"),
                    )
                    .publication_decision()?;
                    if op.before.as_ref().is_some_and(|before| prior.as_ref() != Some(before)) {
                        return Err(Error::RecoveryRefused.into());
                    }
                    remove_adoption_stage(&self.directory, &op)?;
                    store.end_adoption(&op, None, None, None)?;
                }
                s.store = Some(store);
                Ok(())
            })
        })
    }
    fn with_initial_adoption_state<T>(
        &self,
        apply: impl FnOnce(&mut State) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let owner = self
            .no_collection_identity_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(RuntimeError::StaleIncarnation)?;
        let live = owner.live.upgrade().ok_or(RuntimeError::StaleIncarnation)?;
        let live = live.lock().unwrap_or_else(PoisonError::into_inner);
        let engine = owner.engine.upgrade().ok_or(RuntimeError::StaleIncarnation)?;
        let mut s = self.state();
        if !*live
            || engine.generation() != owner.generation
            || !matches!(engine.state(), meguri_engine::LifecycleState::NoCollection)
            || s.incarnation != owner.incarnation
            || s.bound.is_some()
            || s.cloud.generation.is_some()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        self.check_initial_path()?;
        apply(&mut s)
    }
    fn with_adoption_state<T>(
        &self,
        collection: Option<&Collection>,
        apply: impl FnOnce(&mut State) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if collection.is_none() {
            self.with_initial_adoption_state(apply)
        } else {
            apply(&mut self.state())
        }
    }
    fn check_initial_path(&self) -> Result<(), RuntimeError> {
        match self.directory.join("collection.anki2").symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(storage_io(e)),
            Ok(_) => Err(PublicationError::AuthorityChanged.into()),
        }
    }
    fn check_adoption_preview(&self, preview: &Preview, collection: Option<&Collection>) -> Result<(), RuntimeError> {
        if let Some(collection) = collection {
            return self.check_preview_authority(preview, collection);
        }
        if preview.native_file.is_some()
            || meguri_engine::device_state::DeviceState::for_collection(&self.directory.join("collection.anki2"))
                .publication_decision()?
                != preview.decision_before
            || self.publication_account()? != preview.account
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        self.with_initial_adoption_state(|_| Ok(()))
    }
    fn check_adoption_before(&self, op: &AdoptOp, collection: Option<&Collection>) -> Result<(), RuntimeError> {
        if self.publication_account()? != op.account {
            return Err(PublicationError::AuthorityChanged.into());
        }
        match (collection, &op.before, op.file_before) {
            (Some(collection), Some(before), Some(file)) => {
                meguri_engine::device_state::DeviceState::for_collection(collection.path()).settled_publication()?;
                if self.publication_decision()? != *before
                    || self.inputs.files.identity(collection.path())? != file
                    || !collection.is_current()
                {
                    return Err(PublicationError::AuthorityChanged.into());
                }
                Ok(())
            }
            (None, before, None) => {
                let prior =
                    meguri_engine::device_state::DeviceState::for_collection(&self.directory.join("collection.anki2"))
                        .publication_decision()?;
                if &prior != before {
                    return Err(PublicationError::AuthorityChanged.into());
                }
                self.with_initial_adoption_state(|_| Ok(()))
            }
            _ => Err(PublicationError::AuthorityChanged.into()),
        }
    }
    /// V14.2 steps 2–3. Network starts only when the host later drives work.
    pub fn confirm_cloud_adoption(
        &self,
        preview: &Preview,
        collection: Option<&Collection>,
        engine: &Engine,
    ) -> Result<[u8; 16], RuntimeError> {
        self.wait_incoming_idle();
        if preview.active.is_none() || preview.active_epoch.is_none() {
            return Err(PublicationError::NotOffered.into());
        }
        self.check_adoption_preview(preview, collection)?;
        let decision_before = preview.decision_before.as_ref();
        let native_file = preview.native_file;
        if let Some(collection) = collection {
            meguri_engine::device_state::DeviceState::for_collection(collection.path()).settled_publication()?;
        } else {
            self.with_initial_adoption_state(|s| {
                with_tracking_access(|| {
                    if s.store.is_none() {
                        s.store = Some(match TrackingStore::open(&self.directory, self.inputs.scope.clone())? {
                            OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
                            _ => return Err(Error::RecoveryRequired.into()),
                        });
                        s.store.as_mut().ok_or(Error::Database)?.install_runtime()?;
                    }
                    Ok(())
                })
            })?;
        }
        if !matches!(
            preview.decision,
            ConnectDecision::ConfirmAdoption | ConnectDecision::ChooseCollection | ConnectDecision::NoCollection
        ) {
            return Err(PublicationError::NotOffered.into());
        }
        {
            let s = self.state();
            if s.publication.preview.as_ref() != Some(&(preview.clone(), true)) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                if store.publication_op()?.is_some() || store.adoption_op()?.is_some() {
                    return Err(PublicationError::Busy.into());
                }
                Ok(())
            })?;
        }
        let slot = engine.acquire_slot(LongOp::CloudAdopt)?;
        slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        if collection.is_none() {
            let owner = self
                .no_collection_identity_owner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or(RuntimeError::StaleIncarnation)?;
            if !slot.belongs_to(&owner.engine.upgrade().ok_or(RuntimeError::StaleIncarnation)?) {
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        let stage = ReplacementStage::new(&self.directory)?;
        fs::create_dir(stage.directory().join("incoming")).map_err(storage_io)?;
        fs::create_dir(stage.directory().join("payloads")).map_err(storage_io)?;
        meguri_storage::atomic_file::sync_directory(stage.directory())?;
        let spool_sizes = SpoolSizes::read(stage.directory())?;
        let id = self.publication_random_id()?;
        let candidate = publication_id(self.publication_random_id()?);
        if decision_before.is_some_and(|d| candidate == d.publication.identity) || candidate == publication_id(id) {
            return Err(refused());
        }
        let binding_after = BindingId(self.publication_random_id()?);
        let binding = AccountBinding::Connected {
            account: String::from_utf8(preview.account.0.clone()).map_err(|_| refused())?,
            configuration: self.inputs.scope.container.as_str().into(),
            environment: match self.inputs.scope.environment {
                Environment::Development => "development",
                Environment::Production => "production",
            }
            .into(),
        };
        let mut op = AdoptOp {
            id,
            step: AdoptStep::Staging,
            binding_before: None,
            binding_after,
            account: preview.account.clone(),
            before: decision_before.cloned(),
            file_before: native_file,
            candidate: Publication {
                identity: candidate,
                binding,
                decision: PublicationDecision::Clear,
            },
            active: preview.active.clone().ok_or(PublicationError::NotOffered)?,
            epoch: preview.active_epoch.ok_or(PublicationError::NotOffered)?,
            epoch_record: None,
            media_root: None,
            staging_name: stage
                .directory()
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(refused)?
                .into(),
            started_at_ms: self.adapter_now()?,
            transport_wait: false,
            retry_after_ms: None,
        };
        self.check_adoption_preview(preview, collection)?;
        let cleanup_evidence = collection
            .map(|collection| -> Result<_, RuntimeError> {
                Ok((collection.open_evidence()?, collection.cloud_adopt_activation()?))
            })
            .transpose()?;
        let owner = Arc::new(AdoptionOwner {
            slot,
            generation: engine.generation(),
            cleanup_evidence: Mutex::new(cleanup_evidence),
            directory: stage.directory().to_owned(),
            stage: Mutex::new(Some(stage)),
            spool_sizes: Mutex::new(spool_sizes),
            next: Mutex::new(AdoptionNext::Pending(AdoptionTask::ReadActive)),
            pending_page: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            next_contention: Mutex::new(None),
            uncertain_activation: std::sync::atomic::AtomicBool::new(false),
        });
        let mut held = self.adoption.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_some() {
            return Err(PublicationError::Busy.into());
        }
        self.with_adoption_state(collection, |s| {
            if s.publication.preview.as_ref() != Some(&(preview.clone(), true)) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            if s.incoming.running {
                return Err(PublicationError::Busy.into());
            }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_mut().ok_or(Error::Database)?;
                op.binding_before = store
                    .publication_binding_for_account(&op.account)?
                    .map(|b| BindingBefore {
                        id: b.id,
                        state: b.facts.state,
                    });
                store.record_adoption(&op)?;
                Ok(())
            })?;
            *held = Some(owner);
            s.publication.preview = None;
            Self::adapter_close_exchange_locked(s);
            Ok(())
        })?;
        Ok(id)
    }
    /// A distinct durable adoption row drives the existing publication lane.
    #[cfg(feature = "test-hooks")]
    pub fn observe_adoption_next_contention_for_testing(
        &self,
        sender: std::sync::mpsc::Sender<()>,
    ) -> Result<(), RuntimeError> {
        *self
            .adoption_owner()?
            .next_contention
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(sender);
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub fn with_adoption_next_held_for_testing(&self, hold: impl FnOnce()) -> Result<(), RuntimeError> {
        let owner = self.adoption_owner()?;
        let _next = owner.next.lock().unwrap_or_else(PoisonError::into_inner);
        hold();
        Ok(())
    }
    pub fn next_adoption_work(&self, collection: Option<&Collection>) -> Result<Option<AdoptionWork>, RuntimeError> {
        self.next_adoption_work_with_work(collection, &mut crate::cloud::policy::PolicyWork::default())
    }
    pub fn next_adoption_work_with_work(
        &self,
        collection: Option<&Collection>,
        work: &mut crate::cloud::policy::PolicyWork,
    ) -> Result<Option<AdoptionWork>, RuntimeError> {
        work.check()?;
        let Some(op) = self.adoption_op()? else {
            return Ok(None);
        };
        if self.adoption.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            return Ok(None);
        }
        if matches!(
            self.inputs.identity.read_identity(&self.inputs.scope),
            IdentityStatus::ReadFailed
                | IdentityStatus::TemporarilyUnavailable
                | IdentityStatus::Restricted
                | IdentityStatus::Configuration
        ) {
            return Ok(None);
        }
        // Cancellation is not invalid recovery authority and must never enter
        // its destructive cleanup path. Recheck after every contended read.
        let authority = self.check_adoption_before(&op, collection);
        work.check()?;
        if authority.is_err() {
            self.cancel_cloud_adoption_with_work(op.id, collection, work)?;
            return Ok(None);
        }
        if op.transport_wait {
            return Ok(None);
        }
        let owner = self.adoption_owner()?;
        let next = match &*owner.next.lock().unwrap_or_else(PoisonError::into_inner) {
            AdoptionNext::Pending(task) => Some(task.clone()),
            AdoptionNext::Claimed | AdoptionNext::CleanupOnly => None,
        };
        if next == Some(AdoptionTask::Activate)
            && self
                .review
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .upgrade()
                .and_then(|r| r.active_session())
                .is_some()
        {
            return Ok(None);
        }
        work.check()?;
        Ok(next.map(|task| AdoptionWork { op: op.id, task }))
    }
    /// Only an explicit user Resume releases a live staging transport wait.
    pub fn resume_cloud_adoption_with_work(
        &self,
        id: [u8; 16],
        collection: Option<&Collection>,
        work: &mut crate::cloud::policy::PolicyWork,
    ) -> Result<bool, RuntimeError> {
        work.check()?;
        let mut op = self
            .adoption_op()?
            .filter(|op| op.id == id)
            .ok_or(RuntimeError::StaleIncarnation)?;
        let owner = self.adoption_owner()?;
        self.check_adoption_before(&op, collection)?;
        if op.step != AdoptStep::Staging || !op.transport_wait {
            return Err(PublicationError::NotOffered.into());
        }
        if !matches!(
            *owner.next.lock().unwrap_or_else(PoisonError::into_inner),
            AdoptionNext::Pending(AdoptionTask::Fetch { .. } | AdoptionTask::FetchPayload { .. })
        ) {
            return Err(RuntimeError::StaleIncarnation);
        }
        let now = self.adapter_now()?;
        if op.retry_after_ms.is_some_and(|deadline| now < deadline) {
            return Ok(false);
        }
        work.effect()?;
        op.transport_wait = false;
        op.retry_after_ms = None;
        self.write_adoption(&op)?;
        Ok(true)
    }
    /// Delivery owns no reader lease, Runtime/store guard or lifecycle lock.
    pub fn cloud_adoption_reply(
        &self,
        work: &AdoptionWork,
        reply: AdoptionReply,
        collection: Option<&Collection>,
        space: &dyn SpaceProvider,
    ) -> Result<(), RuntimeError> {
        self.cloud_adoption_reply_with_work(
            work,
            reply,
            collection,
            space,
            &mut crate::cloud::policy::PolicyWork::default(),
        )
    }
    fn stage_adoption_record(
        &self,
        owner: &AdoptionOwner,
        op: &AdoptOp,
        item: StagingRecord,
        space: &dyn SpaceProvider,
        diagnostic: &mut AdoptionDiagnosticRecorder,
    ) -> Result<(), RuntimeError> {
        diagnostic.enter(AdoptionDiagnosticStage::PayloadValidation);
        let mut record = StagedRecord {
            name: item.name,
            envelope: item.record.encrypted.ok_or_else(refused)?,
            system_fields: item.record.system_fields.0,
            payload_file: None,
        };
        // One source/image at a time. The caller retains its file
        // until this reply returns; only our copy is durable.
        if let Some(source) = item.payload_file {
            let codec::DecodeOutcome::Supported(entity) =
                codec::Entity::decode(&record.envelope).map_err(|_| refused())?
            else {
                return Err(refused());
            };
            if entity.payload.is_some()
                || entity.fields.generation.is_some()
                || (entity.fields.tombstone && entity.fields.kind != codec::Kind::MediaName)
                || codec::entity_name(&epoch(op)?.record_key, entity.fields.kind, &entity.fields.key)
                    .map_err(|_| refused())?
                    != record.name
            {
                return Err(refused());
            }
            diagnostic.enter(AdoptionDiagnosticStage::PayloadRead);
            let bytes = super::payload_asset::read(&source)?;
            diagnostic.enter(AdoptionDiagnosticStage::PayloadSpace);
            check_space_for(
                space,
                &owner.directory,
                bytes.len() as u64,
                &owner.spool_sizes.lock().unwrap_or_else(PoisonError::into_inner),
            )?;
            let name = format!("{}.pa", record.name);
            let path = owner.directory.join("payloads").join(&name);
            diagnostic.enter(AdoptionDiagnosticStage::PayloadWrite);
            let mut file = AtomicFile::new(&path, Purpose::Stage)?;
            file.file_mut().write_all(&bytes).map_err(storage_io)?;
            owner
                .spool_sizes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .publish(file, &owner.directory, "payloads", &name, bytes.len() as u64)?;
            record.payload_file = Some(name);
        }
        // Validate/decode the owned copy, never an SDK path.
        diagnostic.enter(AdoptionDiagnosticStage::ReceivedValidation);
        let incoming = received(&owner.directory, op, &record)?;
        // receive checked the HMAC name before it becomes a path.
        let path = owner.directory.join("incoming").join(&record.name);
        diagnostic.enter(AdoptionDiagnosticStage::RecordEncode);
        let bytes = serde_json::to_vec(&record).map_err(|_| refused())?;
        diagnostic.enter(AdoptionDiagnosticStage::RecordSpace);
        check_space_for(
            space,
            &owner.directory,
            bytes.len() as u64,
            &owner.spool_sizes.lock().unwrap_or_else(PoisonError::into_inner),
        )?;
        diagnostic.enter(AdoptionDiagnosticStage::RecordWrite);
        let mut file = AtomicFile::new(&path, Purpose::Stage)?;
        file.file_mut().write_all(&bytes).map_err(storage_io)?;
        owner
            .spool_sizes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .publish(file, &owner.directory, "incoming", &record.name, bytes.len() as u64)?;
        if let Some(payload) = incoming.payload
            && payload.kind() != codec::Kind::MediaName
        {
            diagnostic.enter(AdoptionDiagnosticStage::RowStage);
            let clock = self.adoption_clock()?;
            let stage = owner.stage.lock().unwrap_or_else(PoisonError::into_inner);
            meguri_storage::verified_replace::write_cloud_stage::<RuntimeError>(
                stage.as_ref().ok_or_else(refused)?,
                clock.as_ref(),
                |scope| {
                    crate::cloud::row_payload::stage_payload(scope, &payload)?;
                    Ok(())
                },
            )?;
        }
        diagnostic.enter(AdoptionDiagnosticStage::FinalSpace);
        check_space(
            space,
            &owner.directory,
            &owner.spool_sizes.lock().unwrap_or_else(PoisonError::into_inner),
        )?;
        Ok(())
    }
    fn finish_adoption_page(
        &self,
        op: &mut AdoptOp,
        next: Option<OpaqueBytes>,
        diagnostic: &mut AdoptionDiagnosticRecorder,
    ) -> Result<AdoptionTask, RuntimeError> {
        diagnostic.enter(AdoptionDiagnosticStage::PageFinish);
        if let Some(cursor) = next {
            Ok(AdoptionTask::Fetch { cursor: Some(cursor) })
        } else {
            op.step = AdoptStep::Staged;
            self.write_adoption(op)?;
            Ok(AdoptionTask::Validate)
        }
    }
    // Consume every record in arrival order. Only the current external record
    // suspends the page; its cursor stays provisional until the queue drains.
    fn advance_adoption_page(
        &self,
        owner: &AdoptionOwner,
        op: &mut AdoptOp,
        space: &dyn SpaceProvider,
        authority: &crate::cloud::policy::PolicyWork,
        diagnostic: &mut AdoptionDiagnosticRecorder,
    ) -> Result<AdoptionTask, RuntimeError> {
        loop {
            diagnostic.enter(AdoptionDiagnosticStage::PageAdvance);
            authority.check()?;
            let item = owner
                .pending_page
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .and_then(|page| page.records.front())
                .cloned();
            let Some(item) = item else {
                let next = owner
                    .pending_page
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                    .ok_or_else(refused)?
                    .next;
                return self.finish_adoption_page(op, next, diagnostic);
            };
            // Epoch records were checked against the read epoch at admission.
            if item.record_type != "MGEpoch" {
                diagnostic.enter(AdoptionDiagnosticStage::PageRecordValidation);
                let codec::DecodeOutcome::Supported(entity) =
                    codec::Entity::decode(item.record.encrypted.as_deref().ok_or_else(refused)?)
                        .map_err(|_| refused())?
                else {
                    return Err(refused());
                };
                if entity.payload.is_none() && item.payload_file.is_none() {
                    return payload_task(&item);
                }
                self.stage_adoption_record(owner, op, item, space, diagnostic)?;
            }
            owner
                .pending_page
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .ok_or_else(refused)?
                .records
                .pop_front();
        }
    }
    pub fn cloud_adoption_reply_with_work(
        &self,
        work: &AdoptionWork,
        reply: AdoptionReply,
        collection: Option<&Collection>,
        space: &dyn SpaceProvider,
        authority: &mut crate::cloud::policy::PolicyWork,
    ) -> Result<(), RuntimeError> {
        self.cloud_adoption_reply_with_diagnostic(
            work,
            reply,
            collection,
            space,
            authority,
            &mut AdoptionDiagnosticRecorder::new(&work.task),
        )
    }
    pub fn cloud_adoption_reply_with_diagnostic(
        &self,
        work: &AdoptionWork,
        reply: AdoptionReply,
        collection: Option<&Collection>,
        space: &dyn SpaceProvider,
        authority: &mut crate::cloud::policy::PolicyWork,
        diagnostic: &mut AdoptionDiagnosticRecorder,
    ) -> Result<(), RuntimeError> {
        authority.check()?;
        #[cfg(feature = "test-hooks")]
        let task = match &work.task {
            AdoptionTask::ReadActive => "ReadActive",
            AdoptionTask::ReadEpoch => "ReadEpoch",
            AdoptionTask::ReadMediaRoot => "ReadMediaRoot",
            AdoptionTask::Fetch { .. } => "Fetch",
            AdoptionTask::FetchPayload { .. } => "FetchPayload",
            AdoptionTask::Validate => "Validate",
            AdoptionTask::RecheckActive => "RecheckActive",
            AdoptionTask::Activate => "Activate",
        };
        #[cfg(feature = "test-hooks")]
        let mut stage = "operation";
        #[cfg(feature = "test-hooks")]
        let mut page = ("None", false, false, false, false, false);
        macro_rules! adoption_stage {
            ($value:literal, $stage:ident) => {
                diagnostic.enter(AdoptionDiagnosticStage::$stage);
                #[cfg(feature = "test-hooks")]
                {
                    stage = $value;
                }
            };
        }
        // Refusals before the exact owner/task claim have no failure evidence.
        let mut claimed = false;
        let mut failed_abort_owner = None;
        let result = (|| {
            let mut op = self
                .adoption_op()?
                .filter(|p| p.id == work.op)
                .ok_or(RuntimeError::StaleIncarnation)?;
            adoption_stage!("owner", Owner);
            let owner = self.adoption_owner()?;
            if op.transport_wait {
                return Err(RuntimeError::StaleIncarnation);
            }
            adoption_stage!("claim", Claim);
            owner.claim_with_work(&work.task, authority)?;
            claimed = true;
            #[cfg(feature = "test-hooks")]
            {
                let hook = self
                    .adoption_reply_claim_hook
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some(hook) = hook {
                    hook();
                }
            }
            if matches!(&reply, AdoptionReply::Failed) {
                diagnostic.enter(AdoptionDiagnosticStage::Transport);
                diagnostic.transport();
                self.abort_adoption(&op, collection)
                    .inspect_err(|error| {
                        diagnostic.cleanup(error);
                        failed_abort_owner = Some(owner.clone());
                    })?;
                return Err(refused());
            }
            let account_unavailable = matches!(
                self.inputs.identity.read_identity(&self.inputs.scope),
                IdentityStatus::ReadFailed
                    | IdentityStatus::TemporarilyUnavailable
                    | IdentityStatus::Restricted
                    | IdentityStatus::Configuration
            );
            if account_unavailable && !matches!(&reply, AdoptionReply::TransportFailed { .. }) {
                owner.set_next(work.task.clone());
                return Err(PublicationError::AccountUnavailable.into());
            }
            adoption_stage!("authority", Authority);
            // An already-issued failure may retain only wait bookkeeping while
            // account proof is unavailable. It grants no request/activation
            // authority: next work and explicit Resume still check it afresh.
            if !account_unavailable && let Err(error) = self.check_adoption_before(&op, collection) {
                if matches!(&reply, AdoptionReply::TransportFailed { .. }) {
                    self.abort_adoption(&op, collection)
                        .inspect_err(|error| {
                            diagnostic.cleanup(error);
                            failed_abort_owner = Some(owner.clone());
                        })?;
                    return Err(error);
                }
                // This pre-existing path discards the authority check error.
                // Only an abort failure contributes a category here.
                return self
                    .abort_adoption(&op, collection)
                    .inspect_err(|error| {
                        diagnostic.cleanup(error);
                        failed_abort_owner = Some(owner.clone());
                    });
            }
            if let AdoptionReply::TransportFailed {
                code,
                retry_after_duration_ms,
            } = &reply
            {
                let outcome = (|| {
                    // CKError networkUnavailable, networkFailure,
                    // serviceUnavailable, requestRateLimited, zoneBusy only.
                    if op.step != AdoptStep::Staging
                        || !matches!(*code, 3 | 4 | 6 | 7 | 23)
                        || !matches!(
                            work.task,
                            AdoptionTask::Fetch { .. } | AdoptionTask::FetchPayload { .. }
                        )
                    {
                        return Err(refused());
                    }
                    op.retry_after_ms = retry_after_duration_ms
                        .map(|duration| {
                            let duration = i64::try_from(duration).map_err(|_| refused())?;
                            self.adapter_now()?.checked_add(duration).ok_or_else(refused)
                        })
                        .transpose()?;
                    authority.effect()?;
                    op.transport_wait = true;
                    self.write_adoption(&op)?;
                    // No page or payload boundary has been accepted. The cursor
                    // and front metadata remain solely in this same owner.
                    owner.set_next(work.task.clone());
                    Ok(())
                })();
                if let Err(error) = outcome {
                    diagnostic.primary(&error);
                    self.abort_adoption(&op, collection)
                        .inspect_err(|error| {
                            diagnostic.cleanup(error);
                            failed_abort_owner = Some(owner.clone());
                        })?;
                    return Err(error);
                }
                return Ok(());
            }
            if matches!(&reply, AdoptionReply::Read(RecordRead::Failed))
                && matches!(
                    work.task,
                    AdoptionTask::ReadActive
                        | AdoptionTask::ReadEpoch
                        | AdoptionTask::ReadMediaRoot
                        | AdoptionTask::RecheckActive
                )
            {
                owner.set_next(work.task.clone());
                return Err(PublicationError::AccountUnavailable.into());
            }
            let outcome = (|| {
                let next = match (&work.task, reply) {
                    (
                        AdoptionTask::ReadActive | AdoptionTask::RecheckActive,
                        AdoptionReply::Read(RecordRead::Found(record)),
                    ) => {
                        adoption_stage!("active_validation", ActiveValidation);
                        if record != op.active {
                            return Err(PublicationError::AuthorityChanged.into());
                        }
                        if work.task == AdoptionTask::ReadActive {
                            AdoptionTask::ReadEpoch
                        } else {
                            AdoptionTask::Activate
                        }
                    }
                    (AdoptionTask::ReadEpoch, AdoptionReply::Read(RecordRead::Found(record))) => {
                        adoption_stage!("epoch_shape", EpochShape);
                        if record.system_fields.0.is_empty() {
                            return Err(refused());
                        }
                        op.epoch_record = Some(record);
                        adoption_stage!("epoch_validation", EpochValidation);
                        epoch(&op)?;
                        adoption_stage!("epoch_write", EpochWrite);
                        self.write_adoption(&op)?;
                        AdoptionTask::ReadMediaRoot
                    }
                    (AdoptionTask::ReadMediaRoot, AdoptionReply::Read(RecordRead::Found(record))) => {
                        adoption_stage!("media_root_shape", MediaRootShape);
                        if record.system_fields.0.is_empty() {
                            return Err(refused());
                        }
                        op.media_root = Some(record);
                        adoption_stage!("media_root_validation", MediaRootValidation);
                        root(&op)?;
                        adoption_stage!("media_root_write", MediaRootWrite);
                        self.write_adoption(&op)?;
                        adoption_stage!("media_root_epoch", MediaRootEpoch);
                        let source = epoch(&op)?;
                        let clock = self.adoption_clock()?;
                        let now = EpochMillis(self.adapter_now()?);
                        let col = Col {
                            version: i64::from(source.ver),
                            created: EpochSecs(source.crt),
                            modified: now,
                            schema_modified: now,
                            ..Col::default()
                        };
                        adoption_stage!("media_root_stage_create", MediaRootStageCreate);
                        meguri_storage::bootstrap::create::<RuntimeError>(
                            &owner.directory.join("collection.anki2"),
                            &col,
                            &[],
                            clock.as_ref(),
                            |_| Ok(()),
                        )?;
                        adoption_stage!("media_root_space", MediaRootSpace);
                        check_space(
                            space,
                            &owner.directory,
                            &owner.spool_sizes.lock().unwrap_or_else(PoisonError::into_inner),
                        )?;
                        AdoptionTask::Fetch { cursor: None }
                    }
                    (AdoptionTask::Fetch { .. }, AdoptionReply::Page { records, deleted, next }) => {
                        // A from-start snapshot may page while the server changes;
                        // deletion/control changes invalidate the candidate as a whole.
                        adoption_stage!("page_validation", PageValidation);
                        if !deleted.is_empty() {
                            return Err(refused());
                        }
                        if owner
                            .pending_page
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .is_some()
                            || records.len().checked_add(deleted.len()).ok_or_else(refused)? > 32
                        {
                            return Err(refused());
                        }
                        let metadata_bytes = records.iter().try_fold(0_usize, |total, item| {
                            total
                                .checked_add(item.name.len())
                                .and_then(|n| n.checked_add(item.record_type.len()))
                                .and_then(|n| n.checked_add(item.record.encrypted.as_ref().map_or(0, Vec::len)))
                                .and_then(|n| n.checked_add(item.record.system_fields.0.len()))
                                .filter(|n| *n <= 32 * 1024 * 1024)
                                .ok_or_else(refused)
                        })?;
                        if metadata_bytes > 32 * 1024 * 1024 {
                            return Err(refused());
                        }
                        let mut pending = VecDeque::new();
                        for item in records {
                            diagnostic.enter(AdoptionDiagnosticStage::PageRecordValidation);
                            #[cfg(feature = "test-hooks")]
                            {
                                stage = "page_record_validation";
                                page = (
                                    match item.record_type.as_str() {
                                        "MGEntity" => "MGEntity",
                                        "MGEpoch" => "MGEpoch",
                                        _ => "Other",
                                    },
                                    item.payload_file.is_some(),
                                    item.record.encrypted.is_some(),
                                    !item.record.system_fields.0.is_empty(),
                                    item.name == "epoch",
                                    Some(&item.record) == op.epoch_record.as_ref(),
                                );
                            }
                            if item.record_type == "MGEpoch" && item.name == "epoch" {
                                diagnostic.enter(AdoptionDiagnosticStage::PageEpochShape);
                                if item.payload_file.is_some() {
                                    return Err(refused());
                                }
                                diagnostic.enter(AdoptionDiagnosticStage::PageEpochData);
                                let expected = op.epoch_record.as_ref().ok_or_else(refused)?;
                                if item.record.encrypted != expected.encrypted {
                                    return Err(refused());
                                }
                                diagnostic.enter(AdoptionDiagnosticStage::PageEpochSystemFields);
                                if item.record.system_fields != expected.system_fields {
                                    return Err(refused());
                                }
                                pending.push_back(item);
                                continue;
                            }
                            if item.record_type != "MGEntity" {
                                return Err(refused());
                            }
                            let codec::DecodeOutcome::Supported(entity) =
                                codec::Entity::decode(item.record.encrypted.as_deref().ok_or_else(refused)?)
                                    .map_err(|_| refused())?
                            else {
                                return Err(refused());
                            };
                            // Core alone interprets the encrypted envelope. Authenticate
                            // its name and bounded external shape before retaining metadata.
                            if entity.fields.erev == 0
                                || entity.fields.generation.is_some()
                                || (entity.fields.tombstone && entity.fields.kind != codec::Kind::MediaName)
                                || item.record.system_fields.0.is_empty()
                                || codec::entity_name(&epoch(&op)?.record_key, entity.fields.kind, &entity.fields.key)
                                    .map_err(|_| refused())?
                                    != item.name
                            {
                                return Err(refused());
                            }
                            pending.push_back(item);
                        }
                        adoption_stage!("page_space", PageSpace);
                        check_space(
                            space,
                            &owner.directory,
                            &owner.spool_sizes.lock().unwrap_or_else(PoisonError::into_inner),
                        )?;
                        *owner.pending_page.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some(PendingPage { records: pending, next });
                        self.advance_adoption_page(&owner, &mut op, space, authority, diagnostic)?
                    }
                    (AdoptionTask::FetchPayload { .. }, AdoptionReply::Payload { record }) => {
                        diagnostic.enter(AdoptionDiagnosticStage::PayloadValidation);
                        // The claimed task serializes intake. Keep the first metadata
                        // record until its exact returned payload has been staged.
                        let expected = owner
                            .pending_page
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .as_ref()
                            .and_then(|page| page.records.front())
                            .cloned()
                            .ok_or_else(refused)?;
                        if record.name != expected.name
                            || record.record_type != expected.record_type
                            || record.record != expected.record
                            || record.payload_file.is_none()
                        {
                            return Err(refused());
                        }
                        self.stage_adoption_record(&owner, &op, record, space, diagnostic)?;
                        owner
                            .pending_page
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .as_mut()
                            .ok_or_else(refused)?
                            .records
                            .pop_front();
                        self.advance_adoption_page(&owner, &mut op, space, authority, diagnostic)?
                    }
                    _ => return Err(refused()),
                };
                diagnostic.enter(AdoptionDiagnosticStage::NextAuthority);
                authority.check()?;
                self.check_adoption_before(&op, collection)?;
                owner.set_next(next);
                Ok(())
            })();
            if let Err(error) = outcome {
                diagnostic.primary(&error);
                self.abort_adoption(&op, collection)
                    .inspect_err(|error| {
                        diagnostic.cleanup(error);
                        failed_abort_owner = Some(owner.clone());
                    })?;
                return Err(error);
            }
            Ok(())
        })();
        if claimed && let Err(error) = &result {
            diagnostic.primary(error);
        }
        #[cfg(feature = "test-hooks")]
        if result.is_err() {
            eprintln!(
                "cloud-adoption task={task} stage={stage} record_kind={} payload_source={} envelope_present={} system_fields_present={} epoch_name_valid={} epoch_record_matches={}",
                page.0, page.1, page.2, page.3, page.4, page.5,
            );
        }
        if let Some(owner) = failed_abort_owner {
            self.settle_failed_adoption_abort(&owner);
        }
        result
    }
    // Call only after the body, mandatory abort and any diagnostic capture
    // settle, with no further Core work. Host close also drains its jobs.
    // Exact retained ownership grants only cleanup, never task replay.
    fn settle_failed_adoption_abort(&self, owner: &Arc<AdoptionOwner>) {
        let retained = self.adoption.lock().unwrap_or_else(PoisonError::into_inner)
            .as_ref().is_some_and(|held| Arc::ptr_eq(held, owner));
        if retained {
            let mut next = owner.next.lock().unwrap_or_else(PoisonError::into_inner);
            if matches!(*next, AdoptionNext::Claimed)
                && !owner.uncertain_activation.load(std::sync::atomic::Ordering::Acquire)
            {
                *next = AdoptionNext::CleanupOnly;
            }
        }
    }
    fn abort_adoption(&self, op: &AdoptOp, collection: Option<&Collection>) -> Result<(), RuntimeError> {
        self.abort_adoption_with_work(op, collection, &crate::cloud::policy::PolicyWork::default())
    }
    fn abort_adoption_with_work(
        &self,
        op: &AdoptOp,
        collection: Option<&Collection>,
        work: &crate::cloud::policy::PolicyWork,
    ) -> Result<(), RuntimeError> {
        let owner = self.adoption.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let file = collection.map(|c| self.inputs.files.identity(c.path())).transpose()?;
        {
            let mut s = self.state();
            with_tracking_access(|| -> Result<(), RuntimeError> {
                work.effect()?;
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .end_adoption(op, None, file, None)?;
                Ok(())
            })?;
        }
        self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(collection) = collection {
            let owner = owner.ok_or(RuntimeError::StaleIncarnation)?;
            if !collection.is_current() || collection.generation() != owner.generation {
                return Err(RuntimeError::StaleIncarnation);
            }
            let (evidence, activation) = owner
                .cleanup_evidence
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or(RuntimeError::StaleIncarnation)?;
            let publication =
                meguri_engine::device_state::DeviceState::for_collection(collection.path()).settled_publication()?;
            self.bind_evidence(
                evidence,
                file.ok_or(RuntimeError::FileIdentityUnavailable)?,
                publication,
                activation,
            )?;
            self.refresh_identity()?;
            if op.binding_before.as_ref().is_some_and(|b| b.state == BindingState::Active) {
                match self.reconcile_binding(collection, &self.media_service(collection)?, Duration::from_secs(30)) {
                    Err(RuntimeError::Tracking(Error::ValidatedUnprovedLayout { .. })) => {},
                    Err(error) => return Err(error),
                    Ok(_) => {},
                }
            }
        }
        Ok(())
    }
    /// Host-only close cleanup after recovery-resource jobs drain. A suspended
    /// Runtime can read only its exact retained row through this capability.
    pub fn adoption_op_for_host_close(&self, engine: &Engine) -> Result<Option<AdoptOp>, RuntimeError> {
        let owner = self.adoption_owner()?;
        if !owner.slot.belongs_to(engine) || owner.generation != engine.generation() {
            return Err(RuntimeError::StaleIncarnation);
        }
        let s = self.state();
        if let Some(cleanup) = &s.suspended_adoption_cleanup {
            if cleanup.incarnation != s.incarnation || s.bound.is_some() {
                return Err(RuntimeError::StaleIncarnation);
            }
            return with_tracking_access(|| {
                let store =
                    TrackingStore::open_retained_adoption(&self.directory, self.inputs.scope.clone(), &cleanup.store)?;
                let op = store.adoption_op()?;
                if op.as_ref() != Some(&cleanup.op) {
                    return Err(RuntimeError::StaleIncarnation);
                }
                Ok(op)
            });
        }
        drop(s);
        self.adoption_op()
    }
    pub fn cancel_cloud_adoption_for_host_close(
        &self,
        id: [u8; 16],
        engine: &Engine,
        collection: Option<&Collection>,
    ) -> Result<(), RuntimeError> {
        let op = self
            .adoption_op_for_host_close(engine)?
            .filter(|op| op.id == id)
            .ok_or(PublicationError::NotOffered)?;
        let owner = self.adoption_owner()?;
        if matches!(
            *owner.next.lock().unwrap_or_else(PoisonError::into_inner),
            AdoptionNext::Claimed
        ) {
            return Err(PublicationError::Busy.into());
        }
        // Provider failure precedes the cleanup commit and preserves the lease
        // for retry, including after a drain deadline left admission closed.
        let file = collection.map(|c| self.inputs.files.identity(c.path())).transpose()?;
        let suspended = {
            let mut s = self.state();
            let suspended = s.suspended_adoption_cleanup.is_some();
            with_tracking_access(|| -> Result<(), RuntimeError> {
                if let Some(cleanup) = &s.suspended_adoption_cleanup {
                    if cleanup.incarnation != s.incarnation || cleanup.op != op || s.bound.is_some() {
                        return Err(RuntimeError::StaleIncarnation);
                    }
                    // Open only the original scoped tracking row. No schema
                    // installation, recovery, bind or collection handle access.
                    let mut store = TrackingStore::open_retained_adoption(
                        &self.directory,
                        self.inputs.scope.clone(),
                        &cleanup.store,
                    )?;
                    store.end_adoption(&op, None, file, None)?;
                } else {
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .end_adoption(&op, None, file, None)?;
                }
                Ok(())
            })?;
            s.suspended_adoption_cleanup = None;
            suspended
        };
        self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
        if !suspended && let Some(collection) = collection {
            if !collection.is_current() || collection.generation() != owner.generation {
                return Err(RuntimeError::StaleIncarnation);
            }
            let (evidence, activation) = owner
                .cleanup_evidence
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or(RuntimeError::StaleIncarnation)?;
            let publication =
                meguri_engine::device_state::DeviceState::for_collection(collection.path()).settled_publication()?;
            self.bind_evidence(
                evidence,
                file.ok_or(RuntimeError::FileIdentityUnavailable)?,
                publication,
                activation,
            )?;
            self.refresh_identity()?;
        }
        Ok(())
    }
    /// Live task authority, or the exact ownerless recovered Ready row below.
    /// A step or transport wait alone never establishes cancellation authority.
    pub fn cloud_adoption_cancellable(&self, id: [u8; 16]) -> Result<bool, RuntimeError> {
        Ok(self.cloud_adoption_cancellation(id)? != AdoptionCancellation::NotOffered)
    }
    pub fn cloud_adoption_cancellation(&self, id: [u8; 16]) -> Result<AdoptionCancellation, RuntimeError> {
        let owner = self.adoption.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(owner) = owner {
            if !self.adoption_op()?.is_some_and(|op| op.id == id) { return Ok(AdoptionCancellation::NotOffered); }
            let next = owner.next.lock().unwrap_or_else(PoisonError::into_inner);
            return Ok(if !matches!(*next, AdoptionNext::Claimed)
                && !owner.uncertain_activation.load(std::sync::atomic::Ordering::Acquire)
                { AdoptionCancellation::Live } else { AdoptionCancellation::NotOffered });
        }
        let observation = meguri_engine::device_state::DeviceState::for_collection(
            &self.directory.join("collection.anki2"),
        ).unproven_cloud_adoption()?;
        let held = self.adoption.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_some() { return Ok(AdoptionCancellation::NotOffered); }
        let s = self.state();
        with_tracking_access(|| {
            Ok(if Self::recovered_cancel_op(&s, id, observation.as_ref())?.is_some()
                { AdoptionCancellation::RecoveredCancelOnly } else { AdoptionCancellation::NotOffered })
        })
    }
    fn recovered_cancel_op(
        s: &State,
        id: [u8; 16],
        observation: Option<&meguri_engine::device_state::UnprovenCloudAdoption>,
    ) -> Result<Option<AdoptOp>, RuntimeError> {
        let Some(store) = &s.store else { return Ok(None) };
        let Some(op) = store.adoption_op()?.filter(|op| op.id == id) else { return Ok(None) };
        if !observation.is_some_and(|o| o.matches(publication_id(op.id), &op.candidate))
            || !store.recovered_adoption_cancellable(&op)?
            || !s.bound.as_ref().is_some_and(|b| b.binding.id == op.binding_after && !b.usable
                && b.binding.facts.state == BindingState::Isolated(IsolationReason::FileMismatch))
        { return Ok(None); }
        Ok(Some(op))
    }
    /// Host entry for a local Cancel after cold recovery. Live owners retain
    /// their original task/uncertain-activation restrictions and abort path.
    pub fn cancel_cloud_adoption_for_host(
        &self,
        id: [u8; 16],
        collection: Option<&Collection>,
        engine: &Engine,
        generation: meguri_engine::Generation,
    ) -> Result<(), RuntimeError> {
        if self.adoption.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
            return self.cancel_cloud_adoption(id, collection);
        }
        let (op, incarnation) = {
            let s = self.state();
            let op = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.adoption_op())?
                .filter(|op| op.id == id).ok_or(PublicationError::NotOffered)?;
            (op, s.incarnation)
        };
        let collection = collection.ok_or(RuntimeError::StaleIncarnation)?;
        if !collection.is_current() || collection.generation() != generation
            || collection.path() != self.directory.join("collection.anki2")
        { return Err(RuntimeError::StaleIncarnation); }
        let file = self.inputs.files.identity(collection.path())?;
        let slot = engine.acquire_slot(LongOp::CloudAdopt)?;
        slot.with_cloud_adoption_tracking_cleanup(generation, collection, || {
            // Actual file/sidecar evidence, with lifecycle serialized and no
            // Runtime/store locks or platform/account provider call.
            let observation = meguri_engine::device_state::DeviceState::for_collection(collection.path())
                .unproven_cloud_adoption()?;
            let native = fs::symlink_metadata(collection.path()).map_err(storage_io)?;
            if !native.file_type().is_file() || (native.dev(), native.ino()) != file.pair() {
                return Err(RuntimeError::StaleIncarnation);
            }
            let held = self.adoption.lock().unwrap_or_else(PoisonError::into_inner);
            if held.is_some() { return Err(PublicationError::Busy.into()); }
            let mut s = self.state();
            if s.incarnation != incarnation || !collection.is_current()
                || !s.bound.as_ref().is_some_and(|b| b.evidence.generation == generation && b.file == file)
            { return Err(RuntimeError::StaleIncarnation); }
            let next_incarnation = s.incarnation.checked_add(1).ok_or(Error::InvalidData)?;
            with_tracking_access(|| -> Result<(), RuntimeError> {
                if Self::recovered_cancel_op(&s, id, observation.as_ref())?.as_ref() != Some(&op) {
                    return Err(PublicationError::NotOffered.into());
                }
                s.store.as_mut().ok_or(Error::Database)?.cancel_recovered_adoption(&op)?;
                Ok(())
            })?;
            Self::adapter_close_exchange_locked(&mut s);
            s.incarnation = next_incarnation;
            s.bound = None;
            s.publication.preview = None;
            s.publication.resume = None;
            s.scan_required = true;
            s.whole_complete = false;
            s.note_scan = None;
            s.last_capture = None;
            s.publish_authority();
            Ok(())
        })
    }
    pub fn cancel_cloud_adoption(&self, id: [u8; 16], collection: Option<&Collection>) -> Result<(), RuntimeError> {
        self.cancel_cloud_adoption_with_work(id, collection, &crate::cloud::policy::PolicyWork::default())
    }
    fn cancel_cloud_adoption_with_work(
        &self,
        id: [u8; 16],
        collection: Option<&Collection>,
        work: &crate::cloud::policy::PolicyWork,
    ) -> Result<(), RuntimeError> {
        let op = self
            .adoption_op()?
            .filter(|p| p.id == id)
            .ok_or(PublicationError::NotOffered)?;
        let owner = self.adoption_owner()?;
        let pending = {
            let mut next = owner.next.lock().unwrap_or_else(PoisonError::into_inner);
            if owner.uncertain_activation.load(std::sync::atomic::Ordering::Acquire) {
                return Err(PublicationError::Busy.into());
            }
            // Claim the original pending task before releasing the guard; a
            // concurrent worker cannot enter activation during cancellation.
            if matches!(*next, AdoptionNext::Claimed) {
                return Err(PublicationError::Busy.into());
            }
            // CleanupOnly is consumed under the same guard as pending work;
            // concurrent cancellation stays Busy until this abort has settled.
            std::mem::replace(&mut *next, AdoptionNext::Claimed)
        };
        let result = self.abort_adoption_with_work(&op, collection, work);
        if result.is_err() {
            // Restore exactly the authority consumed, never a reply task for a
            // failed original abort. A further cleanup fault retains the slot.
            *owner.next.lock().unwrap_or_else(PoisonError::into_inner) = pending;
        }
        result
    }
    pub fn perform_cloud_adoption_local(
        &self,
        work: &AdoptionWork,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        space: &dyn SpaceProvider,
        backup: &dyn meguri_engine::ports::BackupService,
    ) -> Result<(), RuntimeError> {
        self.perform_adoption_local(work, Some(collection), Some(media), space, backup, None)
    }
    pub fn perform_initial_cloud_adoption_local(
        &self,
        work: &AdoptionWork,
        space: &dyn SpaceProvider,
        backup: &dyn meguri_engine::ports::BackupService,
        config: meguri_engine::OpenConfig,
    ) -> Result<(), RuntimeError> {
        self.perform_adoption_local(work, None, None, space, backup, Some(config))
    }
    fn perform_adoption_local(
        &self,
        work: &AdoptionWork,
        collection: Option<&Collection>,
        media: Option<&MediaService>,
        space: &dyn SpaceProvider,
        backup: &dyn meguri_engine::ports::BackupService,
        initial: Option<meguri_engine::OpenConfig>,
    ) -> Result<(), RuntimeError> {
        let mut op = self
            .adoption_op()?
            .filter(|p| p.id == work.op)
            .ok_or(RuntimeError::StaleIncarnation)?;
        let owner = self.adoption_owner()?;
        owner.claim(&work.task)?;
        if self.publication_account().is_err() {
            owner.set_next(work.task.clone());
            return Err(PublicationError::AccountUnavailable.into());
        }
        if self.check_adoption_before(&op, collection).is_err() {
            let result = self.abort_adoption(&op, collection);
            if result.is_err() {
                self.settle_failed_adoption_abort(&owner);
            }
            return result;
        }
        let result = match work.task {
            AdoptionTask::Validate => self.validate_adoption(&owner, &mut op, collection, media, space),
            AdoptionTask::Activate => match (collection, media, initial) {
                (Some(collection), Some(media), _) => self.activate_adoption(&owner, &op, collection, media, backup),
                (None, None, Some(config)) => self.activate_initial_adoption(&owner, &op, config),
                _ => Err(refused()),
            },
            _ => Err(refused()),
        };
        if matches!(&result, Err(RuntimeError::Engine(EngineError::SessionActive))) {
            owner.set_next(AdoptionTask::Activate);
            return Ok(());
        }
        if let Err(error) = result {
            // Never roll tracking back after a proven/uncertain R6 activation.
            let safe_to_abort = !owner.uncertain_activation.load(std::sync::atomic::Ordering::Acquire)
                && if let Some(collection) = collection {
                    self.publication_decision().is_ok_and(|d| Some(d) == op.before) && collection.is_current()
                } else {
                    owner.stage.lock().unwrap_or_else(PoisonError::into_inner).is_some()
                        || (self.check_initial_path().is_ok()
                            && meguri_engine::device_state::DeviceState::for_collection(
                                &self.directory.join("collection.anki2"),
                            )
                            .settled_publication()
                            .is_ok())
                };
            if safe_to_abort {
                if let Err(error) = self.abort_adoption(&op, collection) {
                    self.settle_failed_adoption_abort(&owner);
                    return Err(error);
                }
            } else {
                self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
            }
            return Err(error);
        }
        Ok(())
    }
    fn validate_adoption(
        &self,
        owner: &AdoptionOwner,
        op: &mut AdoptOp,
        collection: Option<&Collection>,
        media: Option<&meguri_engine::media::MediaService>,
        space: &dyn SpaceProvider,
    ) -> Result<(), RuntimeError> {
        let epoch = epoch(op)?;
        // Enumerate and compare every actual spool file before any Validate
        // reads/SQL preparation. Root/SQLite bytes are also inspected afresh.
        owner
            .spool_sizes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reconcile(&owner.directory)?;
        let paths = spool(&owner.directory)?;
        let mut keys = BTreeSet::new();
        let mut live_keys = BTreeSet::new();

        for path in &paths {
            let record = saved(path)?;
            let received = received(&owner.directory, op, &record)?;
            let key = (received.entity.fields.kind, received.entity.fields.key);
            if !keys.insert(key.clone()) {
                return Err(refused());
            }
            if !received.entity.fields.tombstone {
                live_keys.insert(key);
            }
        }
        // Missing-notetype graphs fail here, even when media is also pending.
        for path in &paths {
            let record = saved(path)?;
            let payload = received(&owner.directory, op, &record)?.payload.ok_or_else(refused)?;
            for reference in crate::cloud::publication_store::references(&payload)? {
                if !live_keys.contains(&reference) {
                    return Err(refused());
                }
            }
            match &payload {
                codec::Payload::Note(note) => {
                    let nt = load_notetype(owner, op, note.mid)?;
                    if note.nt_struct_gen != nt.struct_gen {
                        return Err(refused());
                    }
                    // The model's U+001F split includes trailing empty fields.
                    // SQL integrity and projection equality cannot enforce K21.2.
                    let fields = std::str::from_utf8(&note.flds).map_err(|_| refused())?;
                    if nt.fields.is_empty()
                        || fields.split(meguri_model::note::FIELD_SEPARATOR).count() != nt.fields.len()
                    {
                        return Err(refused());
                    }
                }
                codec::Payload::Card(card) => {
                    let name = codec::entity_name(&epoch.record_key, codec::Kind::Note, &codec::id_key(card.nid))
                        .map_err(|_| refused())?;
                    let note = received(
                        &owner.directory,
                        op,
                        &saved(&owner.directory.join("incoming").join(name))?,
                    )?
                    .payload
                    .ok_or_else(refused)?;
                    let codec::Payload::Note(note) = note else {
                        return Err(refused());
                    };
                    let nt = load_notetype(owner, op, note.mid)?;
                    if card.nt_struct_gen != nt.struct_gen {
                        return Err(refused());
                    }
                }
                _ => {}
            }
        }
        let _media = if let (Some(collection), Some(media)) = (collection, media) {
            self.settle_before_transition(collection, Duration::from_secs(30))?;
            let (reader, job) = inventory_registered_media(collection, media)?;
            drop(reader);
            Some(job)
        } else {
            None
        };
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        let random = self
            .random
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        check_space(
            space,
            &owner.directory,
            &owner.spool_sizes.lock().unwrap_or_else(PoisonError::into_inner),
        )?;
        // Gate, integrity, checkpoint and immutable integrity all precede ready.
        {
            let stage = owner.stage.lock().unwrap_or_else(PoisonError::into_inner);
            meguri_storage::verified_replace::prepare(
                stage.as_ref().ok_or_else(refused)?,
                &self.directory.join("collection.anki2"),
                &meguri_storage::OpenOptions::new(clock.as_ref(), random.as_ref()),
                clock.as_ref(),
                false,
                |_| Ok(()),
            )
            .map_err(|_| refused())?;
        }
        {
            let reader = meguri_storage::connections::ReaderFactory::recovery(
                &owner.directory.join("collection.anki2"),
                &meguri_storage::conn_init::Initializers::new(),
            )
            .open(ReaderRole::Cloud)?;
            let view = crate::cloud::row_payload::ReadView(&reader);
            for path in &paths {
                let payload = received(&owner.directory, op, &saved(path)?)?
                    .payload
                    .ok_or_else(refused)?;
                if payload.kind() == codec::Kind::MediaName {
                    continue;
                }
                let table = crate::cloud::row_payload::table(payload.kind()).ok_or_else(refused)?;
                let key = crate::cloud::row_payload::payload_key(&payload)?;
                let row_key = if (payload.kind() as u8) <= 6 {
                    let bytes: [u8; 8] = key.as_slice().try_into().map_err(|_| refused())?;
                    RowKey::Id(i64::from_be_bytes(bytes))
                } else {
                    RowKey::Text(key)
                };
                let row = reader.row_image(table, &row_key)?.ok_or_else(refused)?;
                let projected = crate::cloud::row_payload::project(&view, payload.kind(), &row, |id| {
                    load_notetype(owner, op, id).map_err(|_| Error::InvalidData)
                })?;
                if projected != payload {
                    return Err(refused());
                }
            }
        }
        self.check_adoption_before(op, collection)?;
        let media_key = RecordKey(root(op)?.media_key);
        let acknowledged_op = op.clone();
        let entities = paths.into_iter().map(|path| {
            let record = saved(&path).map_err(|_| Error::InvalidData)?;
            let received = received(&owner.directory, &acknowledged_op, &record).map_err(|_| Error::InvalidData)?;
            let payload = received.payload.ok_or(Error::InvalidData)?;
            let row_digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
            let payload_digest = payload.payload_digest().map_err(|_| Error::InvalidData)?;
            Ok(EntityFacts {
                name: record.name,
                kind: payload.kind(),
                key: received.entity.fields.key,
                origin: received.entity.fields.origin,
                sent: true,
                local_rev: 1,
                acked_rev: 1,
                tombstone_at: None,
                payload: payload.clone(),
                base_payload: None,
                mark: None,
                server_erev: received.entity.fields.erev,
                acked_system_fields: record.system_fields,
                acked_row_digest: row_digest,
                acked_payload_digest: payload_digest,
                acked_payload: Some(payload),
                acked_envelope: OpaqueBytes(record.envelope),
                in_flight: Vec::new(),
                observer_open: 0,
                observed_seq: 0,
                observed_generation: 0,
            })
        });
        self.check_adoption_before(op, collection)?;
        let checked = collection.map(|c| self.check_replacement_work(c)).transpose()?;
        let _lease = collection.map(|c| c.reader(ReaderRole::Cloud)).transpose()?;
        self.with_adoption_state(collection, |s| {
            if s.incoming.running || collection.zip(checked.as_ref()).is_some_and(|(c, proof)| c.last_seq() != proof.seq) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            with_tracking_access(|| {
                s.store.as_mut().ok_or(Error::Database)?.ready_adoption(
                    op,
                    RecordKey(epoch.record_key),
                    media_key,
                    epoch.crt,
                    entities,
                    checked.as_ref(),
                )
            })?;
            if let Some(bound) = &mut s.bound {
                bound.usable = false;
            }
            s.publish_authority();
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::AdoptionIsolationPublished);
            Ok(())
        })?;
        owner.set_next(AdoptionTask::RecheckActive);
        Ok(())
    }
    fn activate_initial_adoption(
        &self,
        owner: &AdoptionOwner,
        op: &AdoptOp,
        config: meguri_engine::OpenConfig,
    ) -> Result<(), RuntimeError> {
        self.check_adoption_before(op, None)?;
        owner.slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        let stage = owner
            .stage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(refused)?;
        let adopted = owner
            .slot
            .replace_verified_cloud_adopt_initial(
                &self.directory.join("collection.anki2"),
                config,
                stage,
                CloudAdoptInput {
                    operation: publication_id(op.id),
                    candidate: op.candidate.clone(),
                },
            )
            .map_err(|e| {
                if matches!(&e, meguri_engine::replace::ReplaceError::AfterActivation { .. }) {
                    owner
                        .uncertain_activation
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                match e {
                    meguri_engine::replace::ReplaceError::Lifecycle(error) => RuntimeError::Engine(error),
                    error => RuntimeError::Engine(engine_error(error)),
                }
            })?;
        owner
            .uncertain_activation
            .store(true, std::sync::atomic::Ordering::Release);
        // bind/open has already retained R6's native proof, not a guessed inode.
        let activation = adopted.cloud_adopt_activation()?.ok_or_else(refused)?;
        if !activation.matches_initial(publication_id(op.id), &op.candidate) {
            return Err(refused());
        }
        let file = self.inputs.files.identity(adopted.path())?;
        if activation.native_file() != (file.device, file.inode) {
            return Err(refused());
        }
        let durable_file = DurableFileIdentity::from_evidence(adopted.open_evidence()?.cloud_file, file)?;
        meguri_model::faultpoint!("cloud.adopt.after_activation").map_err(Error::from)?;
        {
            let mut s = self.state();
            if !with_tracking_access(|| {
                s.store.as_mut().ok_or(Error::Database)?.end_adoption(
                    op,
                    Some(&activation),
                    Some(file),
                    Some(durable_file),
                )
            })? {
                return Err(refused());
            }
        }
        self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.bind(&adopted)?;
        self.refresh_identity()?;
        Ok(())
    }
    fn activate_adoption(
        &self,
        owner: &AdoptionOwner,
        op: &AdoptOp,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        backup: &dyn meguri_engine::ports::BackupService,
    ) -> Result<(), RuntimeError> {
        self.settle_before_transition(collection, Duration::from_secs(30))?;
        owner.slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        let (media_reader, _media_job) = inventory_registered_media(collection, media)?;
        // Replacement closes every reader. Keep media-job exclusion through
        // activation, but release this generation pin before entering it.
        drop(media_reader);
        let mut exclusive = owner.slot.enter_exclusive(true)?;
        let working = exclusive.collection().clone();
        owner.slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        self.check_adoption_before(op, Some(exclusive.collection()))?;
        require_registered_media(media)?;
        let stage = owner
            .stage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(refused)?;
        let mut pin = None;
        let adopted = exclusive
            .replace_verified_cloud_adopt_with_backup(
                stage,
                CloudAdoptInput {
                    operation: publication_id(op.id),
                    candidate: op.candidate.clone(),
                },
                || {
                    // The package provider performs its P10 budget against this
                    // drained Working collection and retains the actual publication.
                    let guarded = backup.before_safety_operation(&working, &meguri_engine::CancelToken::new())?;
                    let receipt = guarded
                        .receipt
                        .clone()
                        .ok_or_else(|| engine_error("adoption safety backup unavailable"))?;
                    pin = Some(guarded);
                    self.check_adoption_before(op, Some(&working)).map_err(engine_error)?;
                    require_registered_media(media).map_err(engine_error)?;
                    Ok(receipt)
                },
            )
            .map_err(|e| match e {
                meguri_engine::replace::ReplaceError::Lifecycle(error) => RuntimeError::Engine(error),
                error => RuntimeError::Engine(engine_error(error)),
            })?;
        let decision = self.publication_decision()?;
        let evidence = meguri_engine::device_state::DeviceState::for_collection(adopted.path())
            .replacement_evidence_for(op.before.as_ref().ok_or_else(refused)?.publication.identity)?
            .ok_or_else(refused)?;
        let file = self.inputs.files.identity(adopted.path())?;
        let activation = adopted.cloud_adopt_activation()?.ok_or_else(refused)?;
        let durable_file = DurableFileIdentity::from_evidence(adopted.open_evidence()?.cloud_file, file)?;
        // As at startup, R6 may have completed this exact swap with Pending
        // after the platform account changed. Its typed proof admits only the
        // original certain bound candidate, with no pending reason. Finish
        // step 9 before refreshing the actual identity and applying V9.
        let mut candidate = op.candidate.clone();
        if candidate.decision == PublicationDecision::Clear
            && decision.publication.decision == PublicationDecision::Pending
            && decision.pending_reason.is_none()
        {
            candidate.decision = PublicationDecision::Pending;
        }
        if decision.publication != candidate
            || decision.pending_reason.is_some()
            || evidence.prior != op.before.as_ref().ok_or_else(refused)?.publication
            || evidence.candidate != op.candidate
            || evidence.reason != ReplaceReason::CloudAdopt
            || evidence.safety_backup.is_none()
            || !activation.matches(
                publication_id(op.id),
                &op.before.as_ref().ok_or_else(refused)?.publication,
                &op.candidate,
            )
            || activation.native_file() != (file.device, file.inode)
        {
            return Err(refused());
        }
        meguri_model::faultpoint!("cloud.adopt.after_activation").map_err(Error::from)?;
        {
            let mut s = self.state();
            if !with_tracking_access(|| {
                s.store.as_mut().ok_or(Error::Database)?.end_adoption(
                    op,
                    Some(&activation),
                    Some(file),
                    Some(durable_file),
                )
            })? {
                return Err(refused());
            }
        }
        self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.bind(&adopted)?;
        self.refresh_identity()?;
        drop(pin);
        // Root remains unconfirmed. The normal actual reconcile precedes the
        // first no-cursor SDK instance; its root fetch supplies the root proof.
        Ok(())
    }
}
fn load_notetype(owner: &AdoptionOwner, op: &AdoptOp, id: i64) -> Result<codec::Notetype, RuntimeError> {
    let name =
        codec::entity_name(&epoch(op)?.record_key, codec::Kind::Notetype, &codec::id_key(id)).map_err(|_| refused())?;
    let payload = received(
        &owner.directory,
        op,
        &saved(&owner.directory.join("incoming").join(name))?,
    )?
    .payload
    .ok_or_else(refused)?;
    match payload {
        codec::Payload::Notetype(nt) => Ok(nt),
        _ => Err(refused()),
    }
}

#[cfg(all(test, feature = "test-hooks"))]
mod capacity_entry_visits {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    pub(super) struct Capacity {
        pub available: AtomicU64,
        pub calls: AtomicUsize,
    }
    impl Capacity {
        pub(super) fn unlimited() -> Self {
            Self {
                available: AtomicU64::new(u64::MAX),
                calls: AtomicUsize::new(0),
            }
        }
    }
    impl SpaceProvider for Capacity {
        fn available_bytes(&self, _: &Path) -> Result<u64, meguri_storage::StorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.available.load(Ordering::SeqCst))
        }
    }
    pub(super) fn new_stage() -> (tempfile::TempDir, ReplacementStage) {
        let parent = tempfile::tempdir().unwrap();
        let stage = ReplacementStage::new(parent.path()).unwrap();
        // Inspect the actual constructor result before even creating children.
        let root = fs::symlink_metadata(stage.directory()).unwrap();
        assert!(root.is_dir());
        assert_eq!(root.mode() & 0o077, 0, "stage must be private before its first write");
        fs::create_dir(stage.directory().join("incoming")).unwrap();
        fs::create_dir(stage.directory().join("payloads")).unwrap();
        (parent, stage)
    }
    fn staging_entry_visits(count: usize) -> u64 {
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        fs::write(stage.path(), b"synthetic candidate bytes").unwrap();
        let space = Capacity::unlimited();
        ADOPTION_SPACE_ENTRY_VISITS.with(|visits| visits.set(0));
        let mut sizes = SpoolSizes::read(directory).unwrap();
        for index in 0..count {
            // Actual AtomicFile temp/write/publish, two pre-write guards and
            // the post-row guard, as in external-payload staging. No timing.
            for spool in ["payloads", "incoming"] {
                let name = format!("entity-{index}");
                let bytes = b"synthetic immutable spool bytes";
                check_space_for(&space, directory, bytes.len() as u64, &sizes).unwrap();
                let mut file = AtomicFile::new(&directory.join(spool).join(&name), Purpose::Stage).unwrap();
                file.file_mut().write_all(bytes).unwrap();
                sizes
                    .publish(file, directory, spool, &name, bytes.len() as u64)
                    .unwrap();
            }
            check_space(&space, directory, &sizes).unwrap();
        }
        sizes.reconcile(directory).unwrap();
        assert_eq!(space.calls.load(Ordering::SeqCst), 3 * count);
        ADOPTION_SPACE_ENTRY_VISITS.with(std::cell::Cell::get)
    }
    #[test]
    fn adoption_space_entry_visits_scale_linearly() {
        let small = staging_entry_visits(32);
        let large = staging_entry_visits(64);
        eprintln!("adoption capacity entry visits: N={small}, 2N={large}");
        assert!(small >= 6 * 32, "must observe real per-write filesystem inspections");
        assert!(large > small);
        assert!(large <= small * 5 / 2, "whole-spool rewalk: N={small}, 2N={large}");
    }
}

#[cfg(all(test, feature = "test-hooks"))]
mod capacity_tests {
    use super::capacity_entry_visits::{Capacity, new_stage};
    use super::*;
    use std::{os::unix::fs::symlink, sync::atomic::Ordering};

    #[test]
    fn adoption_space_real_stage_is_private_and_initial_reads_repeat() {
        use std::os::unix::fs::PermissionsExt;

        // new_stage checks privacy immediately after ReplacementStage::new,
        // before creating incoming/payloads. No candidate or spool files yet.
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        let sizes = SpoolSizes::read(directory).unwrap();
        for _ in 0..3 {
            let fresh = SpoolSizes::read(directory).unwrap();
            assert_eq!(fresh.current_size(directory).unwrap(), 0);
            fresh.reconcile(directory).unwrap();
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(SpoolSizes::read(directory).is_err());
        assert!(check_space(&Capacity::unlimited(), directory, &sizes).is_err());
    }

    fn publish(sizes: &mut SpoolSizes, directory: &Path, spool: &str, name: &str, bytes: &[u8]) {
        check_space_for(&Capacity::unlimited(), directory, bytes.len() as u64, sizes).unwrap();
        let mut file = AtomicFile::new(&directory.join(spool).join(name), Purpose::Stage).unwrap();
        file.file_mut().write_all(bytes).unwrap();
        sizes.publish(file, directory, spool, name, bytes.len() as u64).unwrap();
    }
    fn actual_bytes(directory: &Path) -> u64 {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    actual_bytes(&entry.path())
                } else {
                    meta.len()
                }
            })
            .sum()
    }
    fn exact_budget(sizes: &SpoolSizes, directory: &Path, additional: u64) {
        let required = (2 * (actual_bytes(directory) + additional) + 64 * 1024 * 1024).max(512 * 1024 * 1024);
        let space = Capacity::unlimited();
        space.available.store(required - 1, Ordering::SeqCst);
        let error = check_space_for(&space, directory, additional, sizes).unwrap_err();
        assert!(matches!(error,
            RuntimeError::Engine(EngineError::Storage(meguri_storage::StorageError::InsufficientSpace {
                required: found_required, available: found_available,
            })) if found_required == required && found_available == required - 1
        ));
        space.available.store(required, Ordering::SeqCst);
        check_space_for(&space, directory, additional, sizes).unwrap();
        assert_eq!(space.calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn adoption_space_replacements_and_inline_orphans_count_actual_files_and_temp_peak() {
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        let mut sizes = SpoolSizes::read(directory).unwrap();
        // Sparse logical bytes put the guard above its floor, below the
        // existing 256 MiB staging bound, without allocating that much RAM.
        fs::File::create(stage.path())
            .unwrap()
            .set_len(225 * 1024 * 1024)
            .unwrap();
        publish(
            &mut sizes,
            directory,
            "payloads",
            "entity.pa",
            b"original external body",
        );
        let mut record = StagedRecord {
            name: "entity".into(),
            envelope: vec![1; 21],
            system_fields: vec![2],
            payload_file: Some("entity.pa".into()),
        };
        publish(
            &mut sizes,
            directory,
            "incoming",
            "entity",
            &serde_json::to_vec(&record).unwrap(),
        );
        let before = sizes.current_size(directory).unwrap();
        exact_budget(&sizes, directory, 97);
        publish(&mut sizes, directory, "payloads", "entity.pa", &[3; 97]);
        assert_eq!(
            sizes.current_size(directory).unwrap(),
            before - b"original external body".len() as u64 + 97
        );
        record.payload_file = None;
        record.envelope = vec![4; 7];
        publish(
            &mut sizes,
            directory,
            "incoming",
            "entity",
            &serde_json::to_vec(&record).unwrap(),
        );
        assert!(
            saved(&directory.join("incoming/entity"))
                .unwrap()
                .payload_file
                .is_none()
        );
        assert_eq!(fs::metadata(directory.join("payloads/entity.pa")).unwrap().len(), 97);
        // Inline replacement leaves the actual orphan in the ledger.
        assert_eq!(sizes.current_size(directory).unwrap(), actual_bytes(directory));
        exact_budget(&sizes, directory, 0);
        sizes.reconcile(directory).unwrap();
        // A newly opened ledger must inventory pre-existing files, too.
        let reopened = SpoolSizes::read(directory).unwrap();
        assert_eq!(reopened.current_size(directory).unwrap(), actual_bytes(directory));
    }
    #[test]
    fn adoption_space_sqlite_growth_and_siblings_are_always_fresh() {
        use meguri_model::{
            clock::{FixedClock, UtcOffset},
            config::ConfigEntry,
        };
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        let sizes = SpoolSizes::read(directory).unwrap();
        let clock = FixedClock::new(1, UtcOffset::UTC);
        meguri_storage::bootstrap::create::<RuntimeError>(&stage.path(), &Col::default(), &[], &clock, |_| Ok(()))
            .unwrap();
        let before = sizes.current_size(directory).unwrap();
        meguri_storage::verified_replace::write_cloud_stage::<RuntimeError>(&stage, &clock, |scope| {
            scope.put_config_entry(&ConfigEntry {
                key: "capacity-synthetic".into(),
                value: serde_json::to_vec(&"x".repeat(128 * 1024)).unwrap(),
                ..ConfigEntry::default()
            })?;
            Ok(())
        })
        .unwrap();
        assert!(sizes.current_size(directory).unwrap() > before);
        assert_eq!(sizes.current_size(directory).unwrap(), actual_bytes(directory));
        // A separate ordinary flat root file retains the seeded capacity
        // fixture's padding behavior and moves the budget above the floor.
        fs::File::create(directory.join(".capacity-padding"))
            .unwrap()
            .set_len(225 * 1024 * 1024)
            .unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            let path = directory.join(format!("collection.anki2{suffix}"));
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .unwrap();
            file.set_len(file.metadata().unwrap().len() + 4096).unwrap();
            exact_budget(&sizes, directory, 0);
        }
        sizes.reconcile(directory).unwrap();
    }
    #[test]
    fn adoption_space_reconciliation_refuses_in_place_changes_and_untracked_entries() {
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        let mut sizes = SpoolSizes::read(directory).unwrap();
        publish(&mut sizes, directory, "incoming", "entity", b"original");
        // An in-place writer need not change its directory's metadata. The
        // full pre-Validate comparison still refuses the stale file size.
        fs::OpenOptions::new()
            .write(true)
            .open(directory.join("incoming/entity"))
            .unwrap()
            .set_len(19)
            .unwrap();
        assert!(sizes.reconcile(directory).is_err());
        let sizes = SpoolSizes::read(directory).unwrap();
        fs::write(directory.join("payloads/untracked"), b"unexpected").unwrap();
        assert!(sizes.reconcile(directory).is_err());
    }
    #[test]
    fn adoption_space_refuses_unknown_directories_links_and_special_types() {
        for kind in ["directory", "symlink", "hardlink", "socket", "root-directory"] {
            let (_parent, stage) = new_stage();
            let directory = stage.directory();
            let sizes = SpoolSizes::read(directory).unwrap();
            let path = directory.join("incoming/unexpected");
            match kind {
                "directory" => fs::create_dir(&path).unwrap(),
                "symlink" => symlink(directory.join("payloads"), &path).unwrap(),
                "hardlink" => {
                    let source = directory.join("owned");
                    fs::write(&source, b"independent required").unwrap();
                    fs::hard_link(source, &path).unwrap();
                }
                "socket" => {
                    let _socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
                }
                "root-directory" => fs::create_dir(directory.join("unknown")).unwrap(),
                _ => unreachable!(),
            }
            assert!(sizes.reconcile(directory).is_err(), "{kind}");
            assert!(SpoolSizes::read(directory).is_err(), "{kind}");
        }
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        let sizes = SpoolSizes::read(directory).unwrap();
        fs::rename(directory.join("incoming"), directory.join("retired")).unwrap();
        symlink(directory.join("retired"), directory.join("incoming")).unwrap();
        assert!(check_space(&Capacity::unlimited(), directory, &sizes).is_err());
    }
    #[test]
    fn adoption_space_failed_and_uncertain_publication_never_resume_cached_checks() {
        use meguri_model::faultpoint::{Fault, arm, disarm};
        for point in ["storage.stage.before_rename", "storage.atomic.after_rename"] {
            let (_parent, stage) = new_stage();
            let directory = stage.directory();
            let mut sizes = SpoolSizes::read(directory).unwrap();
            publish(&mut sizes, directory, "incoming", "entity", b"old");
            let mut file = AtomicFile::new(&directory.join("incoming/entity"), Purpose::Stage).unwrap();
            file.file_mut().write_all(b"replacement").unwrap();
            arm(point, Fault::IoError(std::io::ErrorKind::Other));
            let result = sizes.publish(file, directory, "incoming", "entity", 11);
            disarm(point);
            assert!(result.is_err());
            let expected: &[u8] = if point == "storage.atomic.after_rename" {
                b"replacement"
            } else {
                b"old"
            };
            assert_eq!(fs::read(directory.join("incoming/entity")).unwrap(), expected);
            assert_eq!(sizes.bytes, 3);
            assert!(check_space(&Capacity::unlimited(), directory, &sizes).is_err());
        }
    }
    #[test]
    fn adoption_space_refuses_overflow_oversize_and_nonprivate_stage() {
        use std::os::unix::fs::PermissionsExt;
        let (_parent, stage) = new_stage();
        let directory = stage.directory();
        let sizes = SpoolSizes::read(directory).unwrap();
        let space = Capacity::unlimited();
        // Overflow is rejected before asking the provider, even for an empty
        assert!(check_space_for(&space, directory, u64::MAX, &sizes).is_err());
        assert_eq!(space.calls.load(Ordering::SeqCst), 0);
        for (spool, limit) in [("incoming", 8 * 1024 * 1024), ("payloads", codec::MAX_PAYLOAD_BYTES)] {
            let path = directory.join(spool).join("oversize");
            fs::File::create(&path).unwrap().set_len(limit + 1).unwrap();
            assert!(SpoolSizes::read(directory).is_err());
            fs::remove_file(path).unwrap();
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(SpoolSizes::read(directory).is_err());
        assert!(check_space(&space, directory, &sizes).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_tombstone_received_validation_requires_original_system_fields_and_one_payload_form() {
        use crate::cloud::control_codec::EpochReason;
        let epoch = Epoch {
            epoch_id: [11; 16],
            record_key: [12; 32],
            crt: 1,
            ver: 18,
            state: EpochState::Complete,
            reason: EpochReason::Initial,
            published_at_ms: 1,
            counts: Vec::new(),
        };
        // Decoder input only: this operation is never installed or acknowledged.
        let op = AdoptOp {
            id: [13; 16],
            step: AdoptStep::Staging,
            binding_before: None,
            binding_after: BindingId([14; 16]),
            account: OpaqueBytes(vec![15]),
            before: None,
            file_before: None,
            candidate: Publication {
                identity: PublicationId::from_bytes([16; 16]),
                binding: AccountBinding::LocalOnly,
                decision: PublicationDecision::Clear,
            },
            active: ControlRecord {
                encrypted: None,
                system_fields: OpaqueBytes(vec![17]),
            },
            epoch: EpochId(epoch.epoch_id),
            epoch_record: Some(ControlRecord {
                encrypted: Some(epoch.encode().unwrap()),
                system_fields: OpaqueBytes(vec![18]),
            }),
            media_root: None,
            staging_name: ".meguri-stage-decoder".into(),
            started_at_ms: 1,
            transport_wait: false,
            retry_after_ms: None,
        };
        let payload = codec::Payload::MediaName(codec::MediaName {
            name: b"photo.jpg".to_vec(),
            sha256: [19; 32],
            sha1: [20; 20],
            size: 21,
        })
        .encode()
        .unwrap();
        let entity = codec::Entity::build(
            codec::EntityFields {
                kind: codec::Kind::MediaName,
                key: b"photo.jpg".to_vec(),
                origin: [22; 16],
                erev: 2,
                generation: None,
                tombstone: true,
                deleted_at_ms: 3,
                writer: [23; 16],
                base_sha256: Some([24; 32]),
            },
            &payload,
        )
        .unwrap();
        let mut record = StagedRecord {
            name: codec::entity_name(&epoch.record_key, codec::Kind::MediaName, b"photo.jpg").unwrap(),
            envelope: entity.encode().unwrap(),
            system_fields: vec![25],
            payload_file: None,
        };
        assert_eq!(received_bytes(&op, &record, None).unwrap().entity, entity);
        assert!(received_bytes(&op, &record, Some(&payload)).is_err());
        record.system_fields.clear();
        assert!(received_bytes(&op, &record, None).is_err());
        record.system_fields = vec![25];
        let mut asset_entity = entity.clone();
        asset_entity.payload = None;
        record.envelope = asset_entity.encode().unwrap();
        assert_eq!(
            received_bytes(&op, &record, Some(&payload)).unwrap().entity,
            asset_entity
        );
        assert!(received_bytes(&op, &record, None).is_err());
        let mut damaged = payload;
        damaged[0] ^= 1;
        assert!(received_bytes(&op, &record, Some(&damaged)).is_err());
    }
}
