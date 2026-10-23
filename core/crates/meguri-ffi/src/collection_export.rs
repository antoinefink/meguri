//! RF3: owned collection-only output and bounded atomic destination publication.
use crate::{CoreError, JobHandle, callback, jobs, runtime};
use meguri_engine::{CancelToken, Engine, EngineError, JobKind, ProgressPhase};
use meguri_storage::{
    StorageError,
    atomic_file::{AtomicFile, Purpose},
    space::{SpaceProvider, require_space},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Flow;
impl Flow {
    fn reserve() -> Result<Self, CoreError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| Self)
            .map_err(|_| CoreError::Busy {
                running_op: "collection export".into(),
            })
    }
}
impl Drop for Flow {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CollectionExportMetadata {
    pub operation_id: u64,
    pub source_generation: u64,
    pub admission_seq: u64,
    pub snapshot_modified_ms: i64,
    pub byte_length: u64,
    pub suggested_name: String,
    pub sha256: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CollectionExportFailure {
    pub cause: CoreError,
    /// A complete new destination may be visible. Never claim the old file survived.
    pub publication_uncertain: bool,
    pub destination: Option<String>,
}
impl From<EngineError> for CollectionExportFailure {
    fn from(error: EngineError) -> Self {
        Self {
            cause: error.into(),
            publication_uncertain: false,
            destination: None,
        }
    }
}
fn core(error: impl std::fmt::Display) -> EngineError {
    EngineError::Internal {
        message: error.to_string(),
    }
}

struct ArtifactState {
    discarded: bool,
    leases: usize,
    cleaning: bool,
    flow: Option<Flow>,
}
struct ArtifactOwner {
    engine: Engine,
    directory: PathBuf,
    path: PathBuf,
    source: PathBuf,
    protected_directories: Vec<PathBuf>,
    space: Arc<dyn SpaceProvider>,
    metadata: CollectionExportMetadata,
    state: Mutex<ArtifactState>,
    removal: Mutex<meguri_host::CollectionExportRemoval>,
}
impl ArtifactOwner {
    fn acquire(self: &Arc<Self>) -> Result<Arc<CollectionExportLease>, CoreError> {
        check_generation(self).map_err(CoreError::from)?;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.discarded {
            return Err(CoreError::InvalidArgument {
                message: "export was discarded".into(),
            });
        }
        state.leases += 1;
        Ok(Arc::new(CollectionExportLease {
            owner: self.clone(),
            native: Mutex::new(None),
            prepared: AtomicBool::new(false),
        }))
    }
    fn cleanup(self: &Arc<Self>) -> Option<meguri_engine::JobHandle> {
        let run = {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if !s.discarded || s.leases != 0 || s.cleaning || s.flow.is_none() {
                false
            } else {
                s.cleaning = true;
                true
            }
        };
        if !run {
            return None;
        }
        let owner = self.clone();
        struct Reset(Arc<ArtifactOwner>);
        impl Drop for Reset {
            fn drop(&mut self) {
                self.0.state.lock().unwrap_or_else(PoisonError::into_inner).cleaning = false;
            }
        }
        let reset = Reset(owner.clone());
        // No I/O in a short exported call, no lock held across filesystem work.
        Some(
            self.engine
                .jobs()
                .start("discard collection export", self.engine.generation(), None, move |_| {
                    let _reset = reset;
                    let removed = owner
                        .removal
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&owner.directory, &owner.protected_directories);
                    let mut s = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                    if removed.is_ok() {
                        s.flow.take();
                    }
                    removed
                }),
        )
    }
}
/// Drop alone deliberately retains output after ambiguous process/provider death.
#[derive(uniffi::Object)]
pub struct CollectionExportArtifact {
    owner: Arc<ArtifactOwner>,
}
#[uniffi::export]
impl CollectionExportArtifact {
    pub fn metadata(&self) -> Result<CollectionExportMetadata, CoreError> {
        callback::enter("CollectionExportArtifact.metadata");
        Ok(self.owner.metadata.clone())
    }
    pub fn acquire(&self) -> Result<CollectionExportLeaseJob, CoreError> {
        callback::enter("CollectionExportArtifact.acquire");
        let slot = self.owner.engine.reserve_collection_export_handoff()?;
        let lease = self.owner.acquire()?; // generation checked AFTER admission ownership
        *lease.native.lock().unwrap_or_else(PoisonError::into_inner) = Some(slot);
        let worker = lease.clone();
        let job = self.owner.engine.start_job(JobKind::Export, move |job| {
            job.cancel.check()?;
            meguri_host::collection_export_provider_state(&worker.owner.directory, true)?;
            worker.prepared.store(true, Ordering::Release);
            Ok(())
        })?;
        Ok(CollectionExportLeaseJob {
            job: jobs::wrap(job),
            lease,
        })
    }
    pub fn discard(&self) -> Result<Option<JobHandle>, CoreError> {
        callback::enter("CollectionExportArtifact.discard");
        self.owner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .discarded = true;
        Ok(self.owner.cleanup().map(jobs::wrap))
    }
}
/// A native provider holds this object until its real completion, even after dismissal.
#[derive(uniffi::Object)]
pub struct CollectionExportLease {
    owner: Arc<ArtifactOwner>,
    native: Mutex<Option<meguri_engine::SlotGuard>>,
    prepared: AtomicBool,
}
#[derive(Clone, uniffi::Record)]
pub struct CollectionExportLeaseJob {
    pub job: JobHandle,
    pub lease: Arc<CollectionExportLease>,
}
#[uniffi::export]
impl CollectionExportLease {
    pub fn path(&self) -> Result<String, CoreError> {
        callback::enter("CollectionExportLease.path");
        if !self.prepared.load(Ordering::Acquire) {
            return Err(CoreError::Busy {
                running_op: "native export ownership preparation".into(),
            });
        }
        Ok(self.owner.path.to_string_lossy().into_owned())
    }
    /// Call only after actual native completion/cancellation, never sheet dismissal.
    /// A lost lease leaves the durable provider marker ambiguous on restart.
    pub fn finish(self: Arc<Self>) -> Result<JobHandle, CoreError> {
        callback::enter("CollectionExportLease.finish");
        if self.native.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            // Completion can be retried after a Swift tracking error lost its receipt.
            return self
                .owner
                .engine
                .start_job(JobKind::Export, |_| Ok(()))
                .map(jobs::wrap)
                .map_err(Into::into);
        }
        self.prepared
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| CoreError::InvalidArgument {
                message: "native export lease is not active".into(),
            })?;
        struct Terminal {
            lease: Arc<CollectionExportLease>,
            completed: bool,
        }
        impl Drop for Terminal {
            fn drop(&mut self) {
                if !self.completed {
                    self.lease.prepared.store(true, Ordering::Release);
                }
            }
        }
        // Capture the guard itself: a queued closure may never enter its body.
        let terminal = Terminal {
            lease: self.clone(),
            completed: false,
        };
        let job = self.owner.engine.start_job(JobKind::Export, move |_| {
            let mut terminal = terminal;
            meguri_host::collection_export_provider_state(&terminal.lease.owner.directory, false)?;
            terminal
                .lease
                .native
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            terminal.completed = true;
            Ok(())
        })?;
        Ok(jobs::wrap(job))
    }
}
impl Drop for CollectionExportLease {
    fn drop(&mut self) {
        self.owner.state.lock().unwrap_or_else(PoisonError::into_inner).leases -= 1;
        let _ = self.owner.cleanup();
    }
}
struct Cell<T>(Mutex<Option<Result<T, CollectionExportFailure>>>);
struct Completion<T>(Arc<Cell<T>>);
impl<T> Completion<T> {
    fn finish(self, value: Result<T, CollectionExportFailure>) {
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
    }
}
impl<T> Drop for Completion<T> {
    fn drop(&mut self) {
        let mut result = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        if result.is_none() {
            *result = Some(Err(if std::thread::panicking() {
                core("export worker panicked")
            } else {
                EngineError::Cancelled
            }
            .into()));
        }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CollectionExportOutcome {
    Pending,
    Ready { artifact: Arc<CollectionExportArtifact> },
    Failed { failure: CollectionExportFailure },
}
#[derive(uniffi::Object)]
pub struct CollectionExportResult {
    cell: Arc<Cell<Arc<CollectionExportArtifact>>>,
}
#[uniffi::export]
impl CollectionExportResult {
    pub fn outcome(&self) -> Result<CollectionExportOutcome, CoreError> {
        callback::enter("CollectionExportResult.outcome");
        Ok(match &*self.cell.0.lock().unwrap_or_else(PoisonError::into_inner) {
            None => CollectionExportOutcome::Pending,
            Some(Ok(artifact)) => CollectionExportOutcome::Ready {
                artifact: artifact.clone(),
            },
            Some(Err(failure)) => CollectionExportOutcome::Failed {
                failure: failure.clone(),
            },
        })
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CollectionExportJob {
    pub job: JobHandle,
    pub result: Arc<CollectionExportResult>,
}
#[uniffi::export]
pub fn start_collection_export() -> Result<CollectionExportJob, CoreError> {
    callback::enter("start_collection_export");
    let host = runtime::host()?;
    let collection = host.collection()?; // Ready independently of FSRS study eligibility.
    let generation = collection.generation().0;
    let admission_seq = collection.last_seq().0;
    let source = collection.path().to_owned();
    let flow = Flow::reserve()?;
    let engine = runtime::engine()?.clone();
    let worker_engine = engine.clone();
    let result = Arc::new(CollectionExportResult {
        cell: Arc::new(Cell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    let job = engine.start_job(JobKind::Export, move |job| {
        let outcome = prepare_export(
            &host,
            collection,
            worker_engine,
            source,
            generation,
            admission_seq,
            flow,
            job.id.0,
            &job.cancel,
            &job.progress,
        );
        let status = outcome.as_ref().map(|_| ()).map_err(Clone::clone);
        completion.finish(outcome.map_err(Into::into));
        status
    })?;
    Ok(CollectionExportJob {
        job: jobs::wrap(job),
        result,
    })
}
// Admission facts are captured by the short starter; snapshot facts come from the writer.
#[allow(clippy::too_many_arguments)]
fn prepare_export(
    host: &meguri_host::CoreHost,
    collection: meguri_engine::Collection,
    worker_engine: Engine,
    source: PathBuf,
    generation: u64,
    admission_seq: u64,
    flow: Flow,
    operation_id: u64,
    cancel: &CancelToken,
    progress: &meguri_engine::ProgressSink,
) -> Result<Arc<CollectionExportArtifact>, EngineError> {
    progress.set_phase(ProgressPhase::Named("Flushing saved reviews"));
    let output = host.write_collection_export(&collection, cancel, progress)?;
    // Writer's Maintenance lease is gone before hashing and Ready.
    progress.set_phase(ProgressPhase::Named("Verifying completed export"));
    let digest = digest(&output.path, cancel);
    let (byte_length, sha256) = match digest {
        Ok(value) if collection.is_current() => value,
        other => {
            let _ = meguri_host::remove_collection_export(&output.directory, &output.protected_directories);
            return Err(other.err().unwrap_or(EngineError::StaleGeneration {
                planned: collection.generation(),
                current: collection.current_generation(),
            }));
        }
    };
    let artifact = Arc::new(CollectionExportArtifact {
        owner: Arc::new(ArtifactOwner {
            engine: worker_engine,
            directory: output.directory,
            path: output.path,
            source,
            protected_directories: output.protected_directories,
            space: output.space,
            metadata: CollectionExportMetadata {
                operation_id,
                source_generation: generation,
                admission_seq,
                snapshot_modified_ms: output.written.snapshot_modified.0,
                byte_length,
                suggested_name: "Meguri-Collection.colpkg".into(),
                sha256: sha256.to_vec(),
            },
            removal: Mutex::new(meguri_host::CollectionExportRemoval::default()),
            state: Mutex::new(ArtifactState {
                discarded: false,
                leases: 0,
                cleaning: false,
                flow: Some(flow),
            }),
        }),
    });
    drop(collection);
    // Generation can advance after the reader drains, before delivery.
    let current = artifact.owner.engine.generation().0;
    if current != generation {
        let _ = meguri_host::remove_collection_export(&artifact.owner.directory, &artifact.owner.protected_directories);
        return Err(EngineError::StaleGeneration {
            planned: meguri_engine::Generation(generation),
            current: meguri_engine::Generation(current),
        });
    }
    Ok(artifact)
}
fn digest(path: &Path, cancel: &CancelToken) -> Result<(u64, [u8; 32]), EngineError> {
    let mut file = File::open(path).map_err(core)?;
    let mut hash = Sha256::new();
    let mut length = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancel.check()?;
        let n = file.read(&mut buffer).map_err(core)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        length += n as u64;
    }
    Ok((length, hash.finalize().into()))
}
fn resolved(path: &Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
}
fn alias(a: &Path, b: &Path) -> bool {
    resolved(a).zip(resolved(b)).is_some_and(|(a, b)| a == b)
        || fs::metadata(a)
            .ok()
            .zip(fs::metadata(b).ok())
            .is_some_and(|(a, b)| a.dev() == b.dev() && a.ino() == b.ino())
}
fn check_generation(owner: &ArtifactOwner) -> Result<(), EngineError> {
    let collection = owner.engine.collection()?;
    if collection.generation().0 != owner.metadata.source_generation {
        return Err(EngineError::StaleGeneration {
            planned: meguri_engine::Generation(owner.metadata.source_generation),
            current: collection.generation(),
        });
    }
    Ok(())
}
fn check_destination(owner: &ArtifactOwner, destination: &Path) -> Result<(), EngineError> {
    let mut protected = vec![owner.source.clone(), owner.path.clone()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut path = owner.source.as_os_str().to_owned();
        path.push(suffix);
        protected.push(path.into());
    }
    if protected.iter().any(|path| alias(path, destination))
        || resolved(destination).is_some_and(|path| {
            owner
                .protected_directories
                .iter()
                .chain(std::iter::once(&owner.directory))
                .any(|directory| directory.canonicalize().ok().is_some_and(|dir| path.starts_with(dir)))
        })
    {
        return Err(EngineError::InvalidArgument {
            message: "destination aliases private collection/export files".into(),
        });
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionExportSaveOutcome {
    Pending,
    Saved,
    Failed { failure: CollectionExportFailure },
}
#[derive(uniffi::Object)]
pub struct CollectionExportSaveResult {
    cell: Arc<Cell<()>>,
}
#[uniffi::export]
impl CollectionExportSaveResult {
    pub fn outcome(&self) -> Result<CollectionExportSaveOutcome, CoreError> {
        callback::enter("CollectionExportSaveResult.outcome");
        Ok(match &*self.cell.0.lock().unwrap_or_else(PoisonError::into_inner) {
            None => CollectionExportSaveOutcome::Pending,
            Some(Ok(())) => CollectionExportSaveOutcome::Saved,
            Some(Err(failure)) => CollectionExportSaveOutcome::Failed {
                failure: failure.clone(),
            },
        })
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CollectionExportSaveJob {
    pub job: JobHandle,
    pub result: Arc<CollectionExportSaveResult>,
}
#[uniffi::export]
pub fn start_collection_export_save(
    artifact: Arc<CollectionExportArtifact>,
    destination: String,
    final_validation_destination: Option<String>,
) -> Result<CollectionExportSaveJob, CoreError> {
    callback::enter("start_collection_export_save");
    let lease = artifact.owner.acquire()?;
    let result = Arc::new(CollectionExportSaveResult {
        cell: Arc::new(Cell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    let job = runtime::engine()?.start_job(JobKind::Export, move |job| {
        let outcome = save_validating_final(
            &lease.owner,
            Path::new(&destination),
            final_validation_destination.as_deref().map(Path::new),
            &job.cancel,
        );
        // Release destination's reader before terminal result delivery.
        drop(lease);
        let status = outcome.as_ref().map(|_| ()).map_err(|failure| core(&failure.cause));
        completion.finish(outcome);
        status
    })?;
    Ok(CollectionExportSaveJob {
        job: jobs::wrap(job),
        result,
    })
}
/// All app-controlled destination writes pass through this bounded loop.
fn copy_bounded(
    source: &mut impl Read,
    destination: &mut impl Write,
    expected: u64,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    let mut copied = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancel.check()?;
        let n = source.read(&mut buffer).map_err(core)?;
        if n == 0 {
            break;
        }
        copied = copied
            .checked_add(n as u64)
            .ok_or_else(|| core("export length overflow"))?;
        if copied > expected {
            return Err(core("export artifact length changed"));
        }
        destination.write_all(&buffer[..n]).map_err(core)?;
    }
    if copied != expected {
        return Err(core("export artifact length changed"));
    }
    Ok(())
}
#[cfg(any(test, feature = "test-hooks"))]
fn save(owner: &ArtifactOwner, destination: &Path, cancel: &CancelToken) -> Result<(), CollectionExportFailure> {
    save_validating_final(owner, destination, None, cancel)
}
// Native publication stages on the destination volume. Validate its actual final
// URL too, on this worker, before any copy and again before staged publication.
fn save_validating_final(
    owner: &ArtifactOwner,
    destination: &Path,
    final_validation_destination: Option<&Path>,
    cancel: &CancelToken,
) -> Result<(), CollectionExportFailure> {
    let validate = || -> Result<(), EngineError> {
        check_generation(owner)?;
        check_destination(owner, destination)?;
        if let Some(final_destination) = final_validation_destination {
            check_destination(owner, final_destination)?;
        }
        Ok(())
    };
    let before = || -> Result<AtomicFile, EngineError> {
        cancel.check()?;
        validate()?;
        require_space(
            owner.space.as_ref(),
            destination.parent().ok_or_else(|| core("destination has no parent"))?,
            owner.metadata.byte_length,
        )
        .map_err(EngineError::from)?;
        let mut output = AtomicFile::new(destination, Purpose::Export).map_err(EngineError::from)?;
        let mut source = File::open(&owner.path).map_err(core)?;
        copy_bounded(&mut source, output.file_mut(), owner.metadata.byte_length, cancel)?;
        Ok(output)
    };
    let output = before().map_err(CollectionExportFailure::from)?;
    // Record the verified temporary inode, so errors after rename can be
    // distinguished from all pre-publication failures (including disk full).
    let identity = fs::metadata(output.path()).map_err(|e| CollectionExportFailure::from(core(e)))?;
    let mut verification_error = None;
    let mut verified = false;
    let published = output.publish(|path| {
        let checked = (|| -> Result<(), EngineError> {
            let (length, hash) = digest(path, cancel)?;
            if length != owner.metadata.byte_length || hash.as_slice() != owner.metadata.sha256 {
                return Err(core("destination length or SHA-256 mismatch"));
            }
            validate()?;
            cancel.check()
        })();
        if checked.is_ok() {
            verified = true;
        }
        checked.map_err(|error| {
            verification_error = Some(error);
            StorageError::Io {
                detail: "export destination verification failed".into(),
            }
        })
    });
    match published {
        Ok(()) => Ok(()), // publication wins a late cancellation request
        Err(error) => {
            let uncertain = verified
                && fs::metadata(destination)
                    .map(|m| m.dev() == identity.dev() && m.ino() == identity.ino())
                    .unwrap_or(true);
            Err(CollectionExportFailure {
                cause: verification_error.unwrap_or_else(|| error.into()).into(),
                publication_uncertain: uncertain,
                destination: Some(destination.to_string_lossy().into_owned()),
            })
        }
    }
}

/// Focused E6 integration seam: a stable sparse file through the actual save
/// preflight/copy/verification/publication path. Never exported through UniFFI.
#[cfg(feature = "test-hooks")]
pub fn save_file_for_memory_regression(
    host: &meguri_host::CoreHost,
    source: &Path,
    destination: &Path,
) -> Result<(), CollectionExportFailure> {
    let collection = host.collection()?;
    let (byte_length, hash) = digest(source, &CancelToken::new())?;
    let owner = ArtifactOwner {
        engine: host.engine().clone(),
        directory: source.parent().unwrap().to_owned(),
        path: source.to_owned(),
        source: collection.path().to_owned(),
        protected_directories: vec![collection.path().parent().unwrap().to_owned()],
        space: Arc::new(meguri_storage::space::StatvfsSpace),
        metadata: CollectionExportMetadata {
            operation_id: 1,
            source_generation: collection.generation().0,
            admission_seq: collection.last_seq().0,
            snapshot_modified_ms: 0,
            byte_length,
            suggested_name: "Meguri-Collection.colpkg".into(),
            sha256: hash.to_vec(),
        },
        state: Mutex::new(ArtifactState {
            discarded: false,
            leases: 0,
            cleaning: false,
            flow: None,
        }),
        removal: Mutex::new(meguri_host::CollectionExportRemoval::default()),
    };
    save(&owner, destination, &CancelToken::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_engine::{EventKind, OpSource, OpSpec, ProgressSink};
    use meguri_host::{CoreHost, HostConfig};
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        ids::DeckId,
        rng::SeededRandom,
        time::EpochMillis,
    };
    use meguri_storage::{ConfigValue, space::StatvfsSpace};
    use std::time::Duration;
    static SERIAL: Mutex<()> = Mutex::new(());
    const NOW: i64 = 1_894_701_600_000;
    static CANCEL_AT_PUBLICATION: Mutex<Option<Arc<CancelToken>>> = Mutex::new(None);
    fn cancel_at_publication() {
        if let Some(token) = CANCEL_AT_PUBLICATION.lock().unwrap().take() {
            token.cancel();
        }
    }
    fn host(root: &Path) -> (CoreHost, HostConfig) {
        let config = HostConfig::new(
            root.join("collection"),
            root.join("backups"),
            root.join("widget"),
            Arc::new(FixedClock::new(NOW, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(23)),
            Arc::new(StatvfsSpace),
        );
        fs::create_dir_all(&config.collection_directory).unwrap();
        meguri_engine::bootstrap::create(
            &config.collection_directory.join("collection.anki2"),
            config.clock.as_ref(),
            config.random.as_ref(),
        )
        .unwrap();
        (CoreHost::open(config.clone()).unwrap(), config)
    }
    fn prepare(host: &CoreHost) -> Arc<CollectionExportArtifact> {
        let c = host.collection().unwrap();
        let generation = c.generation().0;
        let seq = c.last_seq().0;
        let source = c.path().to_owned();
        prepare_export(
            host,
            c,
            host.engine().clone(),
            source,
            generation,
            seq,
            Flow::reserve().unwrap(),
            41,
            &CancelToken::new(),
            &ProgressSink::new(),
        )
        .unwrap()
    }
    fn acquire_native(artifact: &CollectionExportArtifact) -> Arc<CollectionExportLease> {
        let events = artifact.owner.engine.stream().subscribe();
        let started = artifact.acquire().unwrap();
        loop {
            let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
            if matches!(event.kind, EventKind::JobFinished { id, .. } if id.0 == started.job.id) {
                break;
            }
        }
        assert!(started.lease.path().is_ok());
        started.lease
    }
    fn finish_native(lease: Arc<CollectionExportLease>) {
        let events = lease.owner.engine.stream().subscribe();
        let job = lease.finish().unwrap();
        loop {
            let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
            if matches!(event.kind, EventKind::JobFinished { id, .. } if id.0 == job.id) {
                break;
            }
        }
    }
    fn drain_cleanup(artifact: &CollectionExportArtifact) {
        let events = artifact.owner.engine.stream().subscribe();
        if let Some(job) = artifact.discard().unwrap() {
            loop {
                let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
                if matches!(event.kind, EventKind::JobFinished { id, .. } if id.0 == job.id) {
                    break;
                }
            }
        }
    }
    fn inspect(path: &Path, config: &HostConfig) -> meguri_package::InspectedCollection {
        meguri_package::inspect_collection_package(
            path,
            &meguri_package::CollectionPackageInspectContext {
                collection_directory: &config.collection_directory,
                live_collection_size: 0,
                reserved_space_bytes: 0,
                clock: config.clock.as_ref(),
                random: config.random.as_ref(),
                space: config.disk_space_provider.as_ref(),
                is_cancelled: &|| false,
            },
        )
        .unwrap()
    }
    fn add_cards(host: &CoreHost) {
        let c = host.collection().unwrap();
        c.run(OpSpec::named("export fixture policy", OpSource::Maintenance), |ctx| {
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
            ctx.scope()
                .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
        let kind = meguri_engine::notetypes::add_stock(
            &c,
            meguri_engine::notetypes::StockKind::Basic,
            Some("Public export fixture".into()),
        )
        .unwrap()
        .value
        .id;
        for i in 0..3 {
            meguri_engine::notes::add(
                &c,
                meguri_engine::notes::AddNote {
                    notetype_id: kind,
                    fields: vec![format!("public front {i}"), "public back".into()],
                    tags: vec![],
                    deck_id: DeckId(1),
                    guid: None,
                    update_add_defaults: false,
                },
                OpSource::UserUndoable,
            )
            .unwrap();
        }
    }
    #[test]
    fn captured_review_reopens_and_later_answers_do_not_change_ready_file() {
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, config) = host(root.path());
        add_cards(&host);
        let meguri_engine::session::StartResult::Started { session_id, head, .. } =
            host.review().start(DeckId(1), None).unwrap()
        else {
            panic!("real session")
        };
        let first = &head.items[0];
        host.review()
            .answer(
                session_id,
                first.card_id,
                meguri_sched::answer::Rating::Easy,
                first.state_token,
                EpochMillis(NOW),
                42,
            )
            .unwrap();
        let admitted = host.collection().unwrap().last_seq().0;
        let artifact = prepare(&host);
        let metadata = artifact.metadata().unwrap();
        assert_eq!(metadata.admission_seq, admitted);
        assert_eq!(metadata.operation_id, 41);
        assert_eq!(metadata.suggested_name, "Meguri-Collection.colpkg");
        let inspected = inspect(&artifact.owner.path, &config);
        assert_eq!(inspected.facts().revlog, 1);
        assert_eq!(inspected.facts().cards, 3);
        assert_eq!(
            meguri_storage::collection_package::snapshot_modified(&inspected.collection_path())
                .unwrap()
                .0,
            metadata.snapshot_modified_ms
        );
        drop(inspected);
        let second = host.review().head(session_id, 1).unwrap();
        let card = &second.items[0];
        host.review()
            .answer(
                session_id,
                card.card_id,
                meguri_sched::answer::Rating::Easy,
                card.state_token,
                EpochMillis(NOW),
                43,
            )
            .unwrap();
        host.review().flush_for_background().unwrap();
        let (length, hash) = digest(&artifact.owner.path, &CancelToken::new()).unwrap();
        assert_eq!(length, metadata.byte_length);
        assert_eq!(hash.as_slice(), metadata.sha256);
        assert_eq!(inspect(&artifact.owner.path, &config).facts().revlog, 1);
        let destination = root.path().join("saved.colpkg");
        save(&artifact.owner, &destination, &CancelToken::new()).unwrap();
        assert_eq!(inspect(&destination, &config).facts().revlog, 1);
        drain_cleanup(&artifact);
        assert!(destination.is_file());
        assert!(!artifact.owner.path.exists());
        host.review().end(session_id).unwrap();
        host.close().unwrap();
    }
    #[test]
    fn destination_alias_cancel_disk_full_and_post_rename_failure_preserve_truthful_outcomes() {
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, _) = host(root.path());
        let artifact = prepare(&host);
        let destination = root.path().join("saved.colpkg");
        fs::write(&destination, b"old file").unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(
            !save(&artifact.owner, &destination, &cancel)
                .unwrap_err()
                .publication_uncertain
        );
        assert_eq!(fs::read(&destination).unwrap(), b"old file");
        let hardlink = root.path().join("alias.colpkg");
        fs::hard_link(&artifact.owner.path, &hardlink).unwrap();
        for path in [
            &artifact.owner.source,
            &artifact.owner.path,
            &hardlink,
            &artifact.owner.source.with_extension("anki2-wal"),
        ] {
            assert!(save(&artifact.owner, path, &CancelToken::new()).is_err());
        }
        for point in [
            "storage.atomic.after_write",
            "storage.atomic.before_rename",
            "storage.atomic.after_rename",
        ] {
            meguri_model::faultpoint::arm(
                point,
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::StorageFull),
            );
            let error = save(&artifact.owner, &destination, &CancelToken::new()).unwrap_err();
            meguri_model::faultpoint::disarm(point);
            assert_eq!(error.publication_uncertain, point.ends_with("after_rename"));
            if error.publication_uncertain {
                assert_eq!(
                    digest(&destination, &CancelToken::new()).unwrap().1.as_slice(),
                    artifact.owner.metadata.sha256
                );
            } else {
                assert_eq!(fs::read(&destination).unwrap(), b"old file");
            }
        }
        fs::write(&destination, b"old file").unwrap();
        for (point, published) in [
            ("storage.atomic.after_sync", false),
            ("storage.atomic.after_rename", true),
        ] {
            let cancel = CancelToken::new();
            *CANCEL_AT_PUBLICATION.lock().unwrap() = Some(cancel.clone());
            meguri_model::faultpoint::arm(point, meguri_model::faultpoint::Fault::Callback(cancel_at_publication));
            let result = save(&artifact.owner, &destination, &cancel);
            meguri_model::faultpoint::disarm(point);
            assert_eq!(result.is_ok(), published);
            if !published {
                assert!(!result.unwrap_err().publication_uncertain);
                assert_eq!(fs::read(&destination).unwrap(), b"old file");
            }
        }
        // Retry uses the same immutable file, without snapshotting again.
        save(&artifact.owner, &destination, &CancelToken::new()).unwrap();
        drain_cleanup(&artifact);
        host.close().unwrap();
    }
    #[test]
    fn staged_save_validates_final_aliases_and_preflights_only_authorized_staging() {
        use std::os::unix::fs::symlink;
        struct Capacity {
            paths: Mutex<Vec<PathBuf>>,
            bytes: std::sync::atomic::AtomicU64,
            redirect: Mutex<Option<(PathBuf, PathBuf)>>,
        }
        impl SpaceProvider for Capacity {
            fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError> {
                self.paths.lock().unwrap().push(directory.to_owned());
                if let Some((source, target)) = self.redirect.lock().unwrap().take() {
                    std::os::unix::fs::symlink(source, target)?;
                }
                Ok(self.bytes.load(Ordering::SeqCst))
            }
        }
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, config) = host(root.path());
        let mut artifact = prepare(&host);
        fs::create_dir_all(&config.backups_directory).unwrap();
        let capacity = Arc::new(Capacity {
            paths: Mutex::new(Vec::new()),
            bytes: std::sync::atomic::AtomicU64::new(0),
            redirect: Mutex::new(None),
        });
        Arc::get_mut(&mut Arc::get_mut(&mut artifact).unwrap().owner)
            .unwrap()
            .space = capacity.clone();
        let staging_directory = root.path().join("authorized-replacement");
        fs::create_dir(&staging_directory).unwrap();
        let staged = staging_directory.join("staged.colpkg");
        let artifact_link = root.path().join("artifact-alias.colpkg");
        fs::hard_link(&artifact.owner.path, &artifact_link).unwrap();
        let source_link = root.path().join("source-alias.colpkg");
        fs::hard_link(&artifact.owner.source, &source_link).unwrap();
        let sidecar = PathBuf::from(format!("{}-journal", artifact.owner.source.display()));
        fs::write(&sidecar, b"protected sidecar").unwrap();
        let sidecar_link = root.path().join("sidecar-alias.colpkg");
        fs::hard_link(&sidecar, &sidecar_link).unwrap();
        let directory_link = root.path().join("collection-alias");
        symlink(&config.collection_directory, &directory_link).unwrap();
        let symbolic_artifact = root.path().join("artifact-symlink.colpkg");
        symlink(&artifact.owner.path, &symbolic_artifact).unwrap();
        let source_before = fs::read(&artifact.owner.source).unwrap();
        let output_before = fs::read(&artifact.owner.path).unwrap();
        for final_destination in [
            artifact.owner.source.clone(),
            artifact.owner.path.clone(),
            artifact_link,
            source_link,
            sidecar_link,
            symbolic_artifact,
            PathBuf::from(format!("{}-wal", artifact.owner.source.display())),
            PathBuf::from(format!("{}-shm", artifact.owner.source.display())),
            directory_link.join("new.colpkg"),
            config.backups_directory.join("new.colpkg"),
            artifact.owner.directory.join("new.colpkg"),
        ] {
            let failure =
                save_validating_final(&artifact.owner, &staged, Some(&final_destination), &CancelToken::new())
                    .unwrap_err();
            assert!(matches!(failure.cause, CoreError::InvalidArgument { .. }));
            assert!(!failure.publication_uncertain);
            assert_eq!(fs::read_dir(&staging_directory).unwrap().count(), 0);
        }
        assert!(
            capacity.paths.lock().unwrap().is_empty(),
            "refuse before preflight/copy"
        );
        assert_eq!(fs::read(&artifact.owner.source).unwrap(), source_before);
        assert_eq!(fs::read(&artifact.owner.path).unwrap(), output_before);
        assert_eq!(fs::read(&sidecar).unwrap(), b"protected sidecar");
        let final_destination = root.path().join("user.colpkg");
        fs::write(&final_destination, b"user original").unwrap();
        assert!(
            save_validating_final(&artifact.owner, &staged, Some(&final_destination), &CancelToken::new(),).is_err()
        );
        assert!(!staged.exists());
        capacity.bytes.store(u64::MAX, Ordering::SeqCst);
        save_validating_final(&artifact.owner, &staged, Some(&final_destination), &CancelToken::new()).unwrap();
        assert_eq!(*capacity.paths.lock().unwrap(), vec![staging_directory.clone(); 2]);
        assert_eq!(fs::read(&staged).unwrap(), output_before);
        assert_eq!(fs::read(&final_destination).unwrap(), b"user original");
        fs::remove_file(&staged).unwrap();
        let late_alias = root.path().join("late-alias.colpkg");
        *capacity.redirect.lock().unwrap() = Some((artifact.owner.path.clone(), late_alias.clone()));
        let failure =
            save_validating_final(&artifact.owner, &staged, Some(&late_alias), &CancelToken::new()).unwrap_err();
        assert!(matches!(failure.cause, CoreError::InvalidArgument { .. }));
        assert!(!failure.publication_uncertain && !staged.exists());
        assert_eq!(fs::read_link(&late_alias).unwrap(), artifact.owner.path);
        assert_eq!(fs::read(&artifact.owner.path).unwrap(), output_before);
        fs::remove_file(&sidecar).unwrap();
        drain_cleanup(&artifact);
        host.close().unwrap();
    }
    #[test]
    fn discard_waits_for_provider_read_and_restart_retains_ambiguous_output() {
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, config) = host(root.path());
        let artifact = prepare(&host);
        let lease = acquire_native(&artifact);
        let directory = artifact.owner.directory.clone();
        artifact.discard().unwrap();
        artifact.discard().unwrap();
        assert!(artifact.acquire().is_err());
        assert!(directory.is_dir());
        assert_eq!(
            fs::metadata(lease.path().unwrap()).unwrap().len(),
            artifact.owner.metadata.byte_length
        );
        let events = host.engine().stream().subscribe();
        drop(lease);
        loop {
            let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
            if matches!(event.kind, EventKind::JobFinished { .. }) && !directory.exists() {
                break;
            }
        }
        let next = prepare(&host);
        let path = next.owner.path.clone();
        let lease = acquire_native(&next);
        // Releasing this reference supplies no evidence that the provider has stopped.
        drop(lease);
        drop(next);
        assert!(path.is_file());
        host.close().unwrap();
        let host = CoreHost::open(config).unwrap();
        assert!(path.is_file());
        let c = host.collection().unwrap();
        let error = host
            .write_collection_export(&c, &CancelToken::new(), &ProgressSink::new())
            .err()
            .unwrap();
        assert!(matches!(error, EngineError::Busy { .. }));
        host.close().unwrap();
    }
    #[test]
    fn native_picker_owns_replacement_admission_until_real_completion() {
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, _) = host(root.path());
        let artifact = prepare(&host);
        let generation = host.engine().generation();
        let lease = acquire_native(&artifact);
        // While a destination is being selected, collection replacement must wait.
        assert!(matches!(
            host.engine().acquire_slot(meguri_engine::LongOp::ColpkgImport),
            Err(EngineError::Busy { .. })
        ));
        assert!(matches!(host.close(), Err(EngineError::Busy { .. })));
        assert_eq!(host.engine().generation(), generation);
        // Delayed native completion must leave the captured file accessible.
        assert_eq!(
            fs::metadata(lease.path().unwrap()).unwrap().len(),
            artifact.owner.metadata.byte_length
        );
        finish_native(lease.clone());
        drop(lease);
        let slot = host.engine().acquire_slot(meguri_engine::LongOp::ColpkgImport).unwrap();
        drop(slot);
        drain_cleanup(&artifact);
        host.close().unwrap();
    }
    #[test]
    fn queued_native_finish_cancel_or_suspend_restores_retry_before_terminal_delivery() {
        use std::{
            future::Future,
            pin::Pin,
            sync::{Barrier, mpsc},
            task::{Context, Wake, Waker},
        };
        struct Cancelled(mpsc::Sender<()>);
        impl Wake for Cancelled {
            fn wake(self: Arc<Self>) {
                self.0.send(()).unwrap();
            }
        }
        let _serial = SERIAL.lock().unwrap();
        for suspend in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (host, config) = host(root.path());
            let artifact = prepare(&host);
            let lease = acquire_native(&artifact);
            let engine = host.engine().clone();
            let events = engine.stream().subscribe();
            let release = Arc::new(Barrier::new(config.engine.job_workers + 1));
            let (entered, started) = mpsc::channel();
            for _ in 0..config.engine.job_workers {
                let entered = entered.clone();
                let release = release.clone();
                engine
                    .jobs()
                    .start("occupy export worker", engine.generation(), None, move |_| {
                        entered.send(()).unwrap();
                        release.wait();
                        Ok(())
                    });
            }
            for _ in 0..config.engine.job_workers {
                started.recv_timeout(Duration::from_secs(20)).unwrap();
            }
            let finish = lease.clone().finish().unwrap();
            assert!(!lease.prepared.load(Ordering::Acquire));
            let cancellation = engine
                .jobs()
                .running()
                .into_iter()
                .find(|job| job.id.0 == finish.id)
                .unwrap()
                .cancel;
            let (signal, cancelled) = mpsc::channel();
            let waker = Waker::from(Arc::new(Cancelled(signal)));
            let mut future = cancellation.cancelled();
            assert!(
                Pin::new(&mut future)
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            let suspending = if suspend {
                let engine = engine.clone();
                Some(std::thread::spawn(move || engine.suspend(Duration::from_secs(20))))
            } else {
                assert!(engine.jobs().cancel(meguri_engine::JobId(finish.id)));
                None
            };
            cancelled.recv_timeout(Duration::from_secs(20)).unwrap();
            release.wait();
            loop {
                let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
                if let EventKind::JobFinished { id, outcome } = &event.kind {
                    if id.0 == finish.id {
                        assert_eq!(outcome, &meguri_engine::JobOutcome::Cancelled);
                        break;
                    }
                }
            }
            if let Some(suspending) = suspending {
                assert_eq!(
                    suspending.join().unwrap().unwrap(),
                    meguri_engine::SuspendOutcome::Suspended
                );
                // An admission failure also drops the captured terminal guard.
                assert!(lease.clone().finish().is_err());
                assert!(lease.prepared.load(Ordering::Acquire));
                engine.resume().unwrap();
            }
            assert!(lease.path().is_ok());
            assert!(matches!(
                engine.acquire_slot(meguri_engine::LongOp::ColpkgImport),
                Err(EngineError::Busy { .. })
            ));
            assert!(
                fs::read_to_string(artifact.owner.directory.join("ownership"))
                    .unwrap()
                    .contains("\nprovider\n")
            );
            finish_native(lease.clone());
            assert!(lease.native.lock().unwrap().is_none());
            assert!(
                fs::read_to_string(artifact.owner.directory.join("ownership"))
                    .unwrap()
                    .contains("\nsafe\n")
            );
            drop(lease);
            let slot = engine.acquire_slot(meguri_engine::LongOp::ColpkgImport).unwrap();
            drop(slot);
            drain_cleanup(&artifact);
            host.close().unwrap();
        }
    }
    #[test]
    fn native_finish_failure_or_panic_keeps_ownership_retryable() {
        fn fail_worker() {
            panic!("synthetic native completion panic");
        }
        let _serial = SERIAL.lock().unwrap();
        for fault in [
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            meguri_model::faultpoint::Fault::Callback(fail_worker),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (host, _) = host(root.path());
            let artifact = prepare(&host);
            let lease = acquire_native(&artifact);
            let events = host.engine().stream().subscribe();
            let point = "storage.atomic.before_rename";
            meguri_model::faultpoint::arm(point, fault);
            let finish = lease.clone().finish().unwrap();
            loop {
                let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
                if let EventKind::JobFinished { id, outcome } = &event.kind {
                    if id.0 == finish.id {
                        assert!(matches!(outcome, meguri_engine::JobOutcome::Failed { .. }));
                        break;
                    }
                }
            }
            meguri_model::faultpoint::disarm(point);
            assert!(lease.prepared.load(Ordering::Acquire));
            assert!(lease.native.lock().unwrap().is_some());
            finish_native(lease.clone());
            // Retrying a completed finish also resolves a lost tracking receipt.
            finish_native(lease.clone());
            drop(lease);
            drain_cleanup(&artifact);
            host.close().unwrap();
        }
    }
    #[test]
    fn discard_retries_marker_unlink_and_post_unlink_sync_failures_before_new_export() {
        let _serial = SERIAL.lock().unwrap();
        for point in [
            "host.collection_export.after_marker_unlink",
            "host.collection_export.before_removal_root_sync",
        ] {
            let root = tempfile::tempdir().unwrap();
            let (host, config) = host(root.path());
            let artifact = prepare(&host);
            let backup = host
                .backup()
                .before_colpkg_import(&host.collection().unwrap(), &CancelToken::new())
                .unwrap();
            let backup = config.backups_directory.join(backup.name);
            let bytes = fs::read(&backup).unwrap();
            let events = host.engine().stream().subscribe();
            meguri_model::faultpoint::arm(
                point,
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
            let failed = artifact.discard().unwrap().unwrap();
            loop {
                let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
                if let EventKind::JobFinished { id, outcome } = &event.kind {
                    if id.0 == failed.id {
                        assert!(matches!(outcome, meguri_engine::JobOutcome::Failed { .. }));
                        break;
                    }
                }
            }
            meguri_model::faultpoint::disarm(point);
            assert!(artifact.owner.state.lock().unwrap().flow.is_some());
            assert!(Flow::reserve().is_err());
            assert_eq!(
                artifact.owner.directory.exists(),
                point.ends_with("after_marker_unlink")
            );
            drain_cleanup(&artifact);
            assert!(artifact.owner.state.lock().unwrap().flow.is_none());
            assert!(artifact.discard().unwrap().is_none());
            assert!(!artifact.owner.directory.exists());
            assert_eq!(fs::read(&backup).unwrap(), bytes);
            let next = prepare(&host);
            drain_cleanup(&next);
            host.close().unwrap();
        }
    }
    #[test]
    fn ready_close_reopen_recovers_only_demonstrably_safe_output() {
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, config) = host(root.path());
        let artifact = prepare(&host);
        let path = artifact.owner.path.clone();
        drop(artifact);
        host.close().unwrap();
        let host = CoreHost::open(config).unwrap();
        assert!(path.is_file(), "opening preserves the completed export");
        let next = prepare(&host);
        assert!(!path.exists());
        assert!(next.owner.path.is_file());
        drain_cleanup(&next);
        host.close().unwrap();
    }
    #[test]
    fn queued_completion_fallback_and_flush_failure_never_report_ready() {
        let _serial = SERIAL.lock().unwrap();
        let cell = Arc::new(Cell::<()>(Mutex::new(None)));
        drop(Completion(cell.clone()));
        assert!(matches!(
            &*cell.0.lock().unwrap(),
            Some(Err(CollectionExportFailure {
                cause: CoreError::Cancelled,
                ..
            }))
        ));
        let root = tempfile::tempdir().unwrap();
        let (host, _) = host(root.path());
        add_cards(&host);
        let c = host.collection().unwrap();
        c.submit_maintenance(
            meguri_engine::executor::catalog::WAL_CHECKPOINT,
            Duration::ZERO,
            |writer| {
                writer.fail_next_wal_sync_for_test();
                Ok(())
            },
        )
        .wait()
        .unwrap();
        assert!(
            host.write_collection_export(&c, &CancelToken::new(), &ProgressSink::new())
                .is_err()
        );
        assert!(!root.path().join("CollectionExports/owned").exists());
        host.close().unwrap();
    }
    #[test]
    fn replacement_invalidates_destination_confirmation_without_reading_old_collection() {
        let _serial = SERIAL.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (host, config) = host(root.path());
        let artifact = prepare(&host);
        let inspection = meguri_package::entry::inspect_entry(
            host.engine(),
            &artifact.owner.path,
            &config.entry_inputs(),
            CancelToken::new(),
            &ProgressSink::new(),
        )
        .unwrap();
        let slot = host.engine().acquire_slot(meguri_engine::LongOp::ColpkgImport).unwrap();
        inspection
            .activate_ready(&slot, host.backup().as_ref(), &CancelToken::new(), &ProgressSink::new())
            .unwrap();
        drop(slot);
        assert!(matches!(artifact.acquire(), Err(CoreError::StaleGeneration { .. })));
        let staged = root.path().join("staged.colpkg");
        let final_destination = root.path().join("user.colpkg");
        let failure =
            save_validating_final(&artifact.owner, &staged, Some(&final_destination), &CancelToken::new()).unwrap_err();
        assert!(matches!(failure.cause, CoreError::StaleGeneration { .. }));
        assert!(!staged.exists() && !final_destination.exists());
        assert!(artifact.owner.path.is_file());
        drain_cleanup(&artifact);
        host.close().unwrap();
    }
    #[test]
    fn gigabyte_stream_uses_bounded_writes_and_detects_length_changes_and_cancellation() {
        struct Sink {
            bytes: u64,
            largest_write: usize,
        }
        impl Write for Sink {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.bytes += buffer.len() as u64;
                self.largest_write = self.largest_write.max(buffer.len());
                Ok(buffer.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let size = 1024 * 1024 * 1024;
        let mut source = std::io::repeat(3).take(size);
        let mut sink = Sink {
            bytes: 0,
            largest_write: 0,
        };
        copy_bounded(&mut source, &mut sink, size, &CancelToken::new()).unwrap();
        assert_eq!(sink.bytes, size);
        assert!(sink.largest_write <= 64 * 1024);
        assert!(
            copy_bounded(
                &mut std::io::repeat(0).take(9),
                &mut std::io::sink(),
                8,
                &CancelToken::new()
            )
            .is_err()
        );
        assert!(copy_bounded(&mut std::io::empty(), &mut std::io::sink(), 8, &CancelToken::new()).is_err());
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(matches!(
            copy_bounded(&mut std::io::repeat(0), &mut std::io::sink(), size, &cancel),
            Err(EngineError::Cancelled)
        ));
    }
}
