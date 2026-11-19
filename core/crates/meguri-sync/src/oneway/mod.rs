//! Explicit self-hosted collection transfers. Construction/history are offline;
//! only prepare/confirm/recovery calls supplied by a caller can issue requests.
mod files;
mod media_stage;
use crate::policy::*;
use crate::{Endpoint, HostKey, Job, JobOptions, Method, Payload, ServerKind, SyncTransport};
pub use media_stage::{MediaConfirmation, MediaPreview};
use meguri_engine::{
    CancelToken, Collection, Engine, EngineError, LifecycleState, OpenConfig, ProgressPhase, ProgressSink, SlotGuard,
    device_state::DeviceState,
    ports::{BackupService, TransferBackup},
    replace::{PreparedUpload, ReplaceReason, ReplacementStage},
};
use meguri_model::{clock::Clock, rng::RandomSource, time::EpochMillis};
use meguri_storage::{ReadRows, StorageError, atomic_file, paths::CollectionPaths, space::SpaceProvider};
use std::{
    fmt, fs,
    future::Future,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

type Result<T> = std::result::Result<T, TransferError>;
/// Server text is a bounded, explicitly requested display payload; Debug and
/// Display never include it, raw transport errors, bodies or credentials.
#[derive(Clone)]
pub enum TransferError {
    Policy(PolicyError),
    Busy,
    Stale(Boundary),
    Artifact,
    Cancelled,
    Interrupted,
    SessionActive,
    Unavailable,
    DiskFull {
        needed: Option<u64>,
        available: Option<u64>,
    },
    SafetyBackup,
    InvalidDownload(InvalidDownload),
    Storage,
    Server(crate::JobError),
    Replacement {
        activated: bool,
        generation: Option<u64>,
    },
    NeedsDecision(Boundary),
    MediaIncomplete {
        operation: OperationId,
        generation: Option<u64>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidDownload {
    Corrupt,
    TooOld(i64),
    TooNew(i64),
    V1Scheduler,
    NotACollection,
    UpgradeFailed(i64),
}
impl fmt::Debug for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MediaIncomplete { operation, generation } => f
                .debug_struct("MediaIncomplete")
                .field("operation", operation)
                .field("generation", generation)
                .finish(),
            Self::Server(e) => f.debug_tuple("Server").field(&e.kind.id()).finish(),
            Self::Policy(e) => f.debug_tuple("Policy").field(e).finish(),
            Self::Stale(b) | Self::NeedsDecision(b) => f.debug_tuple("DecisionRequired").field(b).finish(),
            Self::Replacement { activated, generation } => f
                .debug_struct("Replacement")
                .field("activated", activated)
                .field("generation", generation)
                .finish(),
            Self::InvalidDownload(reason) => f.debug_tuple("InvalidDownload").field(reason).finish(),
            Self::DiskFull { needed, available } => f
                .debug_struct("DiskFull")
                .field("needed", needed)
                .field("available", available)
                .finish(),
            other => f.write_str(match other {
                Self::Busy => "Busy",
                Self::Artifact => "ArtifactChanged",
                Self::Cancelled => "Cancelled",
                Self::Interrupted => "Interrupted",
                Self::SessionActive => "SessionActive",
                Self::Unavailable => "Unavailable",
                Self::SafetyBackup => "SafetyBackupFailed",
                _ => "LocalStorage",
            }),
        }
    }
}
impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for TransferError {}
impl From<PolicyError> for TransferError {
    fn from(e: PolicyError) -> Self {
        Self::Policy(e)
    }
}
impl From<StorageError> for TransferError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::InsufficientSpace { required, available } => Self::DiskFull {
                needed: Some(required),
                available: Some(available),
            },
            StorageError::Sqlite { code: 13, .. } => Self::DiskFull {
                needed: None,
                available: None,
            },
            _ => Self::Storage,
        }
    }
}
impl From<std::io::Error> for TransferError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::StorageFull {
            Self::DiskFull {
                needed: None,
                available: None,
            }
        } else {
            Self::Storage
        }
    }
}
impl From<EngineError> for TransferError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::SessionActive => Self::SessionActive,
            EngineError::Cancelled => Self::Cancelled,
            EngineError::Busy { .. } => Self::Busy,
            EngineError::StaleGeneration { .. } => Self::Stale(Boundary::LocalChanged),
            EngineError::Storage(e) => e.into(),
            EngineError::SafetyBackupFailed(_) => Self::SafetyBackup,
            _ => Self::Unavailable,
        }
    }
}
impl From<crate::JobError> for TransferError {
    fn from(e: crate::JobError) -> Self {
        match e.kind {
            crate::ErrorKind::Cancelled => Self::Cancelled,
            crate::ErrorKind::Interrupted => Self::Interrupted,
            _ => Self::Server(e),
        }
    }
}

/// Transient caller/Keychain inputs. No username/key is copied into history.
pub struct TransferAccess {
    account: AccountRef,
    configured: Endpoint,
    options: JobOptions,
    key: HostKey,
    transport: Option<Box<dyn SyncTransport>>,
}
impl fmt::Debug for TransferAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransferAccess(..)")
    }
}
impl TransferAccess {
    pub fn new(configured: Endpoint, username: &str, key: HostKey, options: JobOptions) -> Result<Self> {
        if options.kind != ServerKind::SelfHosted {
            return Err(TransferError::Unavailable);
        }
        let account =
            AccountRef::derive(&configured, username).ok_or(TransferError::Stale(Boundary::IdentityChanged))?;
        if options.base != configured {
            let approved = crate::follow_redirect(&configured, ServerKind::SelfHosted, options.base.as_str())
                .map_err(|_| TransferError::Stale(Boundary::IdentityChanged))?;
            if approved != options.base {
                return Err(TransferError::Stale(Boundary::IdentityChanged));
            }
        }
        Ok(Self {
            account,
            configured,
            options,
            key,
            transport: None,
        })
    }
    /// Host job cancellation must be the same token the transport observes.
    pub fn with_cancel(mut self, cancel: Arc<CancelToken>) -> Self {
        self.options = self.options.cancel(cancel);
        self
    }
    #[cfg(feature = "test-hooks")]
    pub fn with_transport_for_testing(mut self, transport: Box<dyn SyncTransport>) -> Self {
        self.transport = Some(transport);
        self
    }
    fn job(&mut self, random: &dyn RandomSource, progress: &ProgressSink) -> Result<Job> {
        let mut job = match self.transport.take() {
            Some(t) => Job::with_transport(&self.options, Some(self.key.clone()), random, t),
            None => Job::new(&self.options, Some(self.key.clone()), random),
        }
        .map_err(|e| TransferError::Server(crate::JobError::from_transport(e, None, ServerKind::SelfHosted)))?;
        job.set_progress(progress.clone());
        Ok(job)
    }
}

/// Concrete composition inputs, wired by CoreHost from its existing providers.
#[derive(Clone)]
pub struct TransferInputs {
    pub engine: Engine,
    /// Canonical paths bound once by the host for all transfer I/O.
    pub paths: CollectionPaths,
    /// Original configured roots, retained only for layout validation.
    pub collection_directory: PathBuf,
    pub backups_directory: PathBuf,
    pub state_directory: PathBuf,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub space: Arc<dyn SpaceProvider>,
    pub backup: Arc<dyn BackupService>,
    pub initial_open: OpenConfig,
}
#[derive(Clone)]
pub struct ManualTransfers {
    inputs: TransferInputs,
    policy: TransferPolicy,
    process: [u64; 2],
}
/// In-process network work owns a flag, not a held mutex. Persisted policy is
/// the cross-restart guard; account changes have no clearing operation.
static RUNNING: AtomicBool = AtomicBool::new(false);
struct Flight;
impl Flight {
    fn acquire() -> Result<Self> {
        RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| TransferError::Busy)
    }
}
impl Drop for Flight {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

/// This value carries the saved preview revision into the final decision.
/// Only this service can issue it after recording a preview.
#[derive(Clone, Debug)]
pub struct Confirmation {
    id: OperationId,
    revision: u64,
    recovery: bool,
}
#[derive(Clone, Debug)]
pub struct Preview {
    pub operation: Operation,
    pub confirmation: Confirmation,
    pub pending_local_work: bool,
}
impl ManualTransfers {
    /// Offline. A single installation-private state directory must be reused
    /// across collection, endpoint, account, key and process changes.
    pub fn new(inputs: TransferInputs) -> Self {
        let process = [inputs.random.next_u64(), inputs.random.next_u64()];
        let policy = TransferPolicy::new(&inputs.state_directory);
        Self {
            inputs,
            policy,
            process,
        }
    }
    pub fn policy(&self) -> &TransferPolicy {
        &self.policy
    }
    /// Offline local retry, serialized with every verified replacement and new
    /// admission by the existing lifecycle slot. Never opens or contacts a server.
    pub fn reconcile_local_replacement(&self, slot: &SlotGuard) -> Result<Option<Operation>> {
        let _flight = Flight::acquire()?;
        self.reconcile_replacement_under_slot(slot)
    }
    fn reconcile_replacement_under_slot(&self, slot: &SlotGuard) -> Result<Option<Operation>> {
        if !slot.belongs_to(&self.inputs.engine) || !slot.op().replaces_collection() {
            return Err(TransferError::Unavailable);
        }
        // A process counter or an absent collection cannot supply R6 authority.
        let collection = match self.inputs.engine.collection() {
            Ok(collection) => collection,
            Err(_) => return Ok(None),
        };
        let _reader = collection.reader(meguri_storage::ReaderRole::Query)?;
        if collection.path() != self.live() {
            return Err(TransferError::Unavailable);
        }
        let operation = match self.policy.active()? {
            Some(operation) if can_supersede_partial_media(&operation) => operation,
            _ => return Ok(None),
        };
        let Some(selected_publication) = operation.resulting_publication else {
            return Ok(None);
        };
        let device = DeviceState::for_collection(collection.path());
        let Some(successor) = device.local_replacement_successor(selected_publication)? else {
            return Ok(None);
        };
        self.policy
            .retire_replaced_media(&operation, &successor)
            .map(Some)
            .map_err(Into::into)
    }
    fn live(&self) -> PathBuf {
        self.inputs.paths.collection()
    }
    fn owned(&self, operation: &Operation) -> Result<PathBuf> {
        if !files::valid_name(operation.id.as_str()) {
            return Err(TransferError::Artifact);
        }
        Ok(self.inputs.state_directory.join(operation.id.as_str()))
    }
    fn candidate_path(&self, operation: &Operation) -> Result<PathBuf> {
        let candidate = operation.candidate.as_ref().ok_or(TransferError::Artifact)?;
        if !files::valid_name(&candidate.name) {
            return Err(TransferError::Artifact);
        }
        Ok(self.owned(operation)?.join(&candidate.name))
    }
    fn eligibility(&self, slot: &SlotGuard, direction: Direction) -> Result<()> {
        if !slot.belongs_to(&self.inputs.engine) || slot.op() != meguri_engine::LongOp::OneWaySync {
            return Err(TransferError::Unavailable);
        }
        match (direction, self.inputs.engine.state()) {
            (_, LifecycleState::Ready { .. }) => {}
            (Direction::Import, LifecycleState::NoCollection | LifecycleState::Failed { .. }) => {}
            _ => return Err(TransferError::Unavailable),
        }
        slot.check_verified_review(reason(direction))?;
        Ok(())
    }
    fn capture(&self, collection: Option<&Collection>) -> Result<(LocalGuard, Option<LocalFacts>)> {
        let publication = DeviceState::for_collection(&self.live()).publication()?;
        match collection {
            Some(c) => {
                let lease = c.reader(meguri_storage::ReaderRole::Sync)?;
                let before = c.transfer_admission()?;
                if before.pending {
                    return Err(TransferError::Stale(Boundary::LocalChanged));
                }
                let facts = {
                    let snapshot = lease.snapshot()?;
                    if snapshot.seq().0 != before.committed {
                        return Err(TransferError::Stale(Boundary::LocalChanged));
                    }
                    match snapshot.transfer_facts() {
                        Ok(facts) => Some(LocalFacts::from(facts)),
                        Err(StorageError::Sqlite { code: 11 | 26, .. }) => None,
                        Err(error) => return Err(error.into()),
                    }
                };
                let after = c.transfer_admission()?;
                if before != after {
                    return Err(TransferError::Stale(Boundary::LocalChanged));
                }
                Ok((
                    LocalGuard {
                        process: self.process,
                        generation: c.generation().0,
                        admitted: after.admitted,
                        committed: after.committed,
                        publication,
                        closed_main: None,
                        closed_wal: None,
                    },
                    facts,
                ))
            }
            None => Ok((
                LocalGuard {
                    process: self.process,
                    generation: self.inputs.engine.generation().0,
                    admitted: 0,
                    committed: 0,
                    publication,
                    closed_main: optional_identity(&self.live())?,
                    closed_wal: optional_identity(&self.live().with_file_name("collection.anki2-wal"))?,
                },
                None,
            )),
        }
    }
    fn same_local(&self, operation: &Operation, c: Option<&Collection>) -> Result<()> {
        let (guard, facts) = self.capture(c)?;
        if operation.guard.as_ref() != Some(&guard) || operation.local != facts {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        Ok(())
    }
    fn access(&self, operation: &Operation, access: &TransferAccess) -> Result<()> {
        if operation.account != access.account || operation.configured_endpoint != access.configured.as_str() {
            return Err(TransferError::Stale(Boundary::IdentityChanged));
        }
        Ok(())
    }
    fn network<T>(&self, access: &TransferAccess, work: impl Future<Output = Result<T>>) -> Result<T> {
        // The outer runtime token is deliberately inert: each Job request has
        // the caller's real token. Never drop a just-received OK between its
        // interpretation and durable recording/activation because cancel raced.
        let _ = access;
        crate::transport::runtime::block_on(self.inputs.engine.threads(), &CancelToken::default(), async {
            Ok(work.await)
        })
        .map_err(|_| TransferError::Storage)?
    }
    fn meta(&self, access: &TransferAccess, job: &mut Job) -> Result<RemoteFacts> {
        let outcome = self.network(access, async {
            crate::status::probe(job, self.inputs.clock.as_ref())
                .await
                .map_err(Into::into)
        })?;
        Ok(RemoteFacts::from(&outcome.server))
    }
    fn stage(&self, phase: &'static str, progress: &ProgressSink) {
        progress.set_phase(ProgressPhase::Named(phase));
    }

    /// Caller explicitly authorizes preparation, including Export's safety
    /// backup and server-copy download (which changes the server's mod/usn).
    /// This returns final facts/artifact identities; it cannot dispatch upload.
    pub fn prepare(
        &self,
        slot: &SlotGuard,
        access: TransferAccess,
        direction: Direction,
        scope: Scope,
        progress: &ProgressSink,
    ) -> Result<Preview> {
        self.prepare_identified(slot, access, direction, scope, progress, |_| {})
    }
    /// Reports only the id committed by policy admission. This Rust-only sink
    /// runs after policy storage releases its lock and before requests start.
    pub fn prepare_identified(
        &self,
        slot: &SlotGuard,
        mut access: TransferAccess,
        direction: Direction,
        scope: Scope,
        progress: &ProgressSink,
        identify: impl FnOnce(&OperationId),
    ) -> Result<Preview> {
        let _flight = Flight::acquire()?;
        self.eligibility(slot, direction)?;
        let cancel = access.options.cancel.clone();
        cancel.check()?;
        let current = self.inputs.engine.collection().ok();
        self.check_layout()?;
        self.preflight(direction, None)?;
        files::directory(&self.inputs.state_directory)?;
        self.reconcile_replacement_under_slot(slot)?;
        let now = self.inputs.clock.now_ms();
        let operation = Operation {
            id: OperationId(String::new()),
            account: access.account.clone(),
            configured_endpoint: access.configured.as_str().into(),
            target_endpoint: access.options.base.as_str().into(),
            direction,
            scope,
            phase: Phase::Preparing,
            acceptance: Acceptance::NotSent,
            boundary: None,
            guard: None,
            recovery_guard: None,
            recovery_local: None,
            local: None,
            remote: None,
            candidate: None,
            activation_identity: None,
            original: None,
            safety_backups: Vec::new(),
            server_copy: None,
            prepared_at: now,
            actual_generation: current.as_ref().map(|c| c.generation().0),
            collection_complete: false,
            resulting_publication: None,
            media: MediaOutcome::default(),
            receipt: None,
            revision: 0,
        };
        let mut op = self.policy.begin(operation, self.inputs.random.next_u64(), now)?;
        identify(&op.id);
        let outcome = (|| {
            files::directory(&self.owned(&op)?)?;
            self.cancel_media_jobs(&cancel)?;
            meguri_model::faultpoint!("oneway.before_exclusive")?;
            if current.is_some() {
                let exclusive = slot.enter_exclusive(true)?;
                slot.check_verified_review(reason(direction))?; // AFTER drain, before ANY prep/request.
                meguri_model::faultpoint!("oneway.after_exclusive")?;
                let c = exclusive.collection();
                let (guard, local) = self.capture(Some(c))?;
                if direction == Direction::Export && local.is_none() {
                    return Err(TransferError::Unavailable);
                }
                op.guard = Some(guard);
                op.local = local;
                self.prepare_inside(&mut op, Some(c), &mut access, &cancel, progress)?;
            } else {
                let _ownership = meguri_storage::lockfile::CollectionLock::acquire(&self.inputs.paths.directory)
                    .map_err(|_| TransferError::Unavailable)?;
                slot.check_verified_review(reason(direction))?;
                let (guard, local) = self.capture(None)?;
                op.guard = Some(guard);
                op.local = local;
                self.prepare_inside(&mut op, None, &mut access, &cancel, progress)?;
            }
            Ok(preview(&op, false))
        })();
        if let Err(error) = &outcome {
            self.failed(&mut op, error);
        }
        outcome
    }
    fn prepare_inside(
        &self,
        op: &mut Operation,
        collection: Option<&Collection>,
        access: &mut TransferAccess,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> Result<()> {
        let mut job = access.job(self.inputs.random.as_ref(), progress)?;
        if op.direction == Direction::Export {
            self.stage("backup", progress);
            self.backup(op, collection, cancel)?;
        }
        let _before = self.meta(access, &mut job)?;
        let stage = ReplacementStage::new(&self.inputs.paths.directory)?;
        self.stage(
            if op.direction == Direction::Export {
                "server_copy"
            } else {
                "downloading"
            },
            progress,
        );
        self.download(access, &mut job, &stage.path(), op.direction)?;
        self.stage("checking", progress);
        if op.direction == Direction::Export {
            let cancelled = || cancel.is_cancelled();
            let context = meguri_package::CollectionPackageWriteContext {
                collection_directory: &self.inputs.paths.directory,
                purpose: atomic_file::Purpose::Export,
                finalisation: meguri_package::Finalisation::ExclusiveCreate,
                clock: self.inputs.clock.as_ref(),
                random: self.inputs.random.as_ref(),
                space: self.inputs.space.as_ref(),
                is_cancelled: &cancelled,
            };
            let copy = meguri_package::preserve_server_copy(&stage.path(), &self.inputs.paths.backups, &context)
                .map_err(package_error)?;
            op.server_copy = Some(ServerCopy {
                artifact: files::artifact(&copy.path)?,
                kind: if copy.kind == meguri_package::ServerCopyKind::Verified {
                    CopyKind::VerifiedPackage
                } else {
                    CopyKind::UnverifiedRaw
                },
                published_at: copy.instant,
                served_facts: copy.facts.map(Into::into),
            });
            self.policy.save(op)?;
            let remote = self.meta(access, &mut job)?;
            if let Some(served) = op.server_copy.as_ref().and_then(|c| c.served_facts.as_ref()) {
                same_served(served, &remote)?;
            }
            op.remote = Some(remote);
            op.target_endpoint = job.base().as_str().into();
            drop(stage);
            cancel.check()?;
            self.stage("preparing", progress);
            let stage = ReplacementStage::new(&self.inputs.paths.directory)?;
            let c = collection.ok_or(TransferError::Unavailable)?;
            self.same_local(op, Some(c))?;
            {
                let reader = c.reader(meguri_storage::ReaderRole::Sync)?;
                reader
                    .prepare_full_upload(&stage, EpochMillis(op.prepared_at), self.inputs.random.as_ref())
                    .map_err(staging_error)?;
            }
            let path = self.owned(op)?.join("candidate.anki2");
            op.candidate = Some(files::retain(&stage.path(), &path)?);
        } else {
            cancel.check()?;
            // Actual open/upgrade, integrity, ls=mod and finalisation before
            // safety backup. Refused download never adds a safety backup.
            let options = meguri_storage::OpenOptions::new(self.inputs.clock.as_ref(), self.inputs.random.as_ref());
            meguri_storage::verified_replace::prepare(
                &stage,
                &self.live(),
                &options,
                self.inputs.clock.as_ref(),
                false,
                |scope| {
                    let mut col = scope.col()?;
                    col.last_sync = col.modified;
                    scope.put_col(&col).map(|_| ())
                },
            )
            .map_err(staging_error)?;
            let remote = self.meta(access, &mut job)?;
            if let Some(served) = meguri_storage::one_way::verify_server_copy(&stage.path()).map_err(staging_error)? {
                same_served(&served.into(), &remote)?;
            }
            op.remote = Some(remote);
            op.target_endpoint = job.base().as_str().into();
            op.candidate = Some(files::retain(&stage.path(), &self.owned(op)?.join("candidate.anki2"))?);
        }
        self.same_local(op, collection)?;
        cancel.check()?;
        op.phase = Phase::Preview;
        self.policy.save(op)?;
        Ok(())
    }

    /// Final destructive authority. A token is consumed against its durable
    /// revision. Identity, local admitted/committed work, artifacts and fresh
    /// observed remote facts are checked again before upload/local activation.
    pub fn confirm(
        &self,
        slot: &SlotGuard,
        mut access: TransferAccess,
        confirmation: Confirmation,
        progress: &ProgressSink,
    ) -> Result<Operation> {
        let _flight = Flight::acquire()?;
        let mut op = self.policy.get(&confirmation.id)?;
        if op.revision != confirmation.revision || op.phase != Phase::Preview {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        self.eligibility(slot, op.direction)?;
        self.access(&op, &access)?;
        let cancel = access.options.cancel.clone();
        cancel.check()?;
        let current = self.inputs.engine.collection().ok();
        self.preflight(op.direction, None)?;
        let outcome = (|| {
            if confirmation.recovery {
                if !recoverable(&op) {
                    return Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown));
                }
                self.same_recovery_local(&op, current.as_ref())?;
            } else {
                if op.acceptance != Acceptance::NotSent {
                    return Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown));
                }
                self.same_local(&op, current.as_ref())?;
            }
            self.cancel_media_jobs(&cancel)?;
            if current.is_some() {
                let mut exclusive = slot.enter_exclusive(true)?;
                slot.check_verified_review(reason(op.direction))?;
                let mut upload_job = None;
                if confirmation.recovery {
                    self.same_recovery_local(&op, Some(exclusive.collection()))?;
                    if self.activation_proven(&op)? {
                        op.actual_generation = Some(exclusive.collection().generation().0);
                        return self.complete_collection(&mut op);
                    }
                    // A fresh recovery decision explicitly discards the shown
                    // local work. Protect it before activating the accepted file.
                    if op.direction == Direction::Export {
                        self.stage("backup", progress);
                        self.backup(&mut op, Some(exclusive.collection()), &cancel)?;
                    }
                    if op.direction == Direction::Import {
                        self.recheck_remote(&op, &mut access, progress)?;
                    }
                } else {
                    self.same_local(&op, Some(exclusive.collection()))?;
                    upload_job = Some(self.recheck_remote(&op, &mut access, progress)?);
                }
                let stage = self.restore_candidate(&op)?;
                if op.direction == Direction::Export {
                    let candidate = op.candidate.as_ref().ok_or(TransferError::Artifact)?;
                    let prepared = PreparedUpload::verify(
                        stage,
                        &self.live(),
                        candidate.identity.length,
                        candidate.identity.sha256,
                    )
                    .map_err(replacement_error)?;
                    if !confirmation.recovery {
                        self.send_upload(
                            &mut op,
                            &access,
                            upload_job.take().ok_or(TransferError::Unavailable)?,
                            progress,
                        )?;
                    }
                    // Known OK: only local activation remains. Cancellation is
                    // past its point of no return and never reuploads this file.
                    op.activation_identity = op.candidate.as_ref().map(|a| a.identity.clone());
                    op.phase = Phase::Activating;
                    self.policy.save(&mut op)?;
                    progress.set_phase(ProgressPhase::Committing);
                    let result = exclusive.activate_prepared_upload(prepared, || Ok(()));
                    self.activation_result(&mut op, result)?;
                } else {
                    self.stage("backup", progress);
                    self.backup(&mut op, Some(exclusive.collection()), &cancel)?;
                    cancel.check()?;
                    self.stage("replacing", progress);
                    let candidate_path = stage.path();
                    let result = exclusive.replace_verified_before_commit(
                        stage,
                        ReplaceReason::FullDownload,
                        |scope| {
                            let mut col = scope.col()?;
                            col.last_sync = col.modified;
                            scope.put_col(&col).map(|_| ())
                        },
                        || {
                            cancel.check()?;
                            progress.set_phase(ProgressPhase::Committing);
                            op.activation_identity = Some(files::identity(&candidate_path).map_err(worker_error)?);
                            op.phase = Phase::Activating;
                            self.policy
                                .save(&mut op)
                                .map_err(|_| worker_error(TransferError::Storage))?;
                            Ok(())
                        },
                    );
                    self.activation_result(&mut op, result)?;
                }
            } else {
                if op.direction != Direction::Import {
                    return Err(TransferError::Unavailable);
                }
                if confirmation.recovery {
                    self.check_absent_recovery_source(&op)?;
                    self.same_recovery_local(&op, None)?;
                } else {
                    self.same_local(&op, None)?;
                }
                self.recheck_remote(&op, &mut access, progress)?;
                let stage = self.restore_candidate(&op)?;
                // The initial replacement owns the advisory lock during this
                // hook. Its full gate/finalisation runs BEFORE the raw backup.
                let candidate_path = stage.path();
                let result = slot.replace_verified_initial_before_commit(
                    &self.live(),
                    self.inputs.initial_open.clone(),
                    stage,
                    ReplaceReason::FullDownload,
                    |scope| {
                        let mut col = scope.col()?;
                        col.last_sync = col.modified;
                        scope.put_col(&col).map(|_| ())
                    },
                    || {
                        if confirmation.recovery {
                            self.check_absent_recovery_source(&op).map_err(worker_error)?;
                            self.same_recovery_local(&op, None).map_err(worker_error)?;
                        } else {
                            self.same_local(&op, None).map_err(worker_error)?;
                        }
                        self.backup(&mut op, None, &cancel).map_err(worker_error)?;
                        cancel.check()?;
                        progress.set_phase(ProgressPhase::Committing);
                        op.activation_identity = Some(files::identity(&candidate_path).map_err(worker_error)?);
                        op.phase = Phase::Activating;
                        self.policy.save(&mut op).map_err(|_| {
                            EngineError::Storage(StorageError::Io {
                                detail: "manual Import intent was not durable".into(),
                            })
                        })?;
                        Ok(())
                    },
                );
                self.activation_result(&mut op, result)?;
            }
            self.complete_collection(&mut op)
        })();
        if let Err(error) = &outcome {
            self.failed(&mut op, error);
        }
        outcome
    }
    fn recheck_remote(&self, op: &Operation, access: &mut TransferAccess, progress: &ProgressSink) -> Result<Job> {
        self.access(op, access)?;
        let mut job = access.job(self.inputs.random.as_ref(), progress)?;
        let remote = self.meta(access, &mut job)?;
        if op.remote != Some(remote) || op.target_endpoint != job.base().as_str() {
            return Err(TransferError::Stale(Boundary::RemoteChanged));
        }
        // Reuse this exact job/session/approved route for the subsequent upload.
        // The configured identity remains unchanged.
        Ok(job)
    }
    fn restore_candidate(&self, op: &Operation) -> Result<ReplacementStage> {
        let source = self.candidate_path(op)?;
        let expected = op.candidate.as_ref().ok_or(TransferError::Artifact)?;
        files::check(&source, expected)?;
        if let Some(original) = &op.original {
            files::check(&self.owned(op)?.join(&original.name), original)?;
        }
        for backup in &op.safety_backups {
            if !files::valid_name(&backup.name) {
                return Err(TransferError::Artifact);
            }
            files::check(&self.inputs.paths.backups.join(&backup.name), backup)?;
        }
        if let Some(copy) = &op.server_copy {
            if !files::valid_name(&copy.artifact.name) {
                return Err(TransferError::Artifact);
            }
            files::check(&self.inputs.paths.backups.join(&copy.artifact.name), &copy.artifact)?;
        }
        let stage = ReplacementStage::new(&self.inputs.paths.directory)?;
        files::retain(&source, &stage.path())?;
        // Only this disposable activation inode becomes writable. The durable
        // candidate remains sealed, and changing its copy's mode must keep bytes exact.
        fs::set_permissions(stage.path(), fs::Permissions::from_mode(0o600))?;
        files::check(&stage.path(), expected)?;
        atomic_file::full_sync(&fs::File::open(stage.path())?)?;
        Ok(stage)
    }
    fn send_upload(
        &self,
        op: &mut Operation,
        access: &TransferAccess,
        mut job: Job,
        progress: &ProgressSink,
    ) -> Result<()> {
        self.access(op, access)?;
        let cancel = access.options.cancel.clone();
        cancel.check()?;
        let candidate = op.candidate.as_ref().ok_or(TransferError::Artifact)?;
        let source = files::FileStream::new(&self.candidate_path(op)?, candidate.identity.clone())?;
        let total = candidate.identity.length;
        if job.base().as_str() != op.target_endpoint {
            return Err(TransferError::Stale(Boundary::IdentityChanged));
        }
        // ALL fallible local preflight above this line. Once intent is durable,
        // an interrupted dispatch is conservatively a possibly accepted upload.
        op.phase = Phase::ConfirmedIntent;
        op.acceptance = Acceptance::PossiblyAccepted;
        self.policy.save(op)?;
        meguri_model::faultpoint!("oneway.upload.after_intent")?;
        op.phase = Phase::Transferring;
        self.policy.save(op)?;
        self.stage("uploading", progress);
        let answer = self.network(access, async {
            match job
                .request(
                    Method::Upload,
                    Payload::Stream {
                        source: Box::pin(source),
                        total: Some(total),
                    },
                )
                .await
            {
                Err(error) => {
                    let refused = matches!(
                        &error,
                        crate::SyncError::Status {
                            code: 400 | 403 | 407 | 409 | 413 | 501,
                            ..
                        }
                    );
                    let preflight = matches!(
                        &error,
                        crate::SyncError::InvalidEndpoint(_)
                            | crate::SyncError::InvalidInput(_)
                            | crate::SyncError::InvalidProxy(_)
                    );
                    Ok(UploadAnswer::Failure {
                        error: job.error_for(Method::Upload, error),
                        refused,
                        preflight,
                    })
                }
                Ok(mut body) => {
                    self.stage("checking", progress);
                    let mut prefix = Vec::new();
                    let mut length = 0u64;
                    loop {
                        match body.chunk().await {
                            Ok(Some(piece)) => {
                                length = length.saturating_add(piece.len() as u64);
                                let room = 16_384 - prefix.len();
                                prefix.extend_from_slice(&piece[..piece.len().min(room)]);
                            }
                            Ok(None) => break,
                            Err(error) => {
                                return Ok(UploadAnswer::Failure {
                                    error: job.error_for(Method::Upload, error),
                                    refused: false,
                                    preflight: false,
                                });
                            }
                        }
                    }
                    if length == 2 && prefix == b"OK" {
                        Ok(UploadAnswer::Ok)
                    } else {
                        Ok(UploadAnswer::Failure {
                            error: crate::JobError::new(
                                crate::ErrorKind::ServerMessage {
                                    text: String::from_utf8_lossy(&prefix).into_owned(),
                                },
                                Some(Method::Upload),
                                "upload refused",
                            ),
                            refused: true,
                            preflight: false,
                        })
                    }
                }
            }
        })?;
        match answer {
            UploadAnswer::Ok => {
                op.acceptance = Acceptance::ExactOk;
                op.phase = Phase::RemoteAccepted;
                self.policy.save(op)?; // MUST precede every local activation.
                meguri_model::faultpoint!("oneway.upload.after_ok")?;
                Ok(())
            }
            UploadAnswer::Failure {
                error,
                refused,
                preflight,
            } => {
                if refused {
                    op.acceptance = Acceptance::Refused;
                } else if preflight {
                    op.acceptance = Acceptance::NotSent;
                }
                Err(error.into())
            }
        }
    }
    fn backup(&self, op: &mut Operation, collection: Option<&Collection>, cancel: &CancelToken) -> Result<()> {
        let local_absent = if op.recovery_guard.is_some() {
            op.recovery_local.is_none()
        } else {
            op.local.is_none()
        };
        let backup = self
            .inputs
            .backup
            .before_manual_transfer(if local_absent { None } else { collection }, cancel)?;
        let names = match backup {
            TransferBackup::Collection(receipt) => vec![receipt.name],
            TransferBackup::Raw { main, wal, .. } => std::iter::once(main).chain(wal).collect(),
            TransferBackup::Absent => Vec::new(),
        };
        for name in names {
            if !files::valid_name(&name) {
                return Err(TransferError::Artifact);
            }
            let path = self.inputs.paths.backups.join(&name);
            let artifact = files::artifact(&path)?;
            // Retain independent original/backup bytes in the durable operation
            // namespace as well as the published backup. No retention/sweep owns it.
            if op.original.is_none() {
                op.original = Some(files::retain(&path, &self.owned(op)?.join("original.backup"))?);
            }
            op.safety_backups.push(artifact);
        }
        self.policy.save(op)?;
        meguri_model::faultpoint!("oneway.after_backup")?;
        Ok(())
    }
    fn download(&self, access: &TransferAccess, job: &mut Job, destination: &Path, direction: Direction) -> Result<()> {
        self.network(access, async {
            let mut body = job
                .request(Method::Download, Payload::empty())
                .await
                .map_err(|e| job.error_for(Method::Download, e))?;
            self.preflight(direction, Some(body.declared_size()))?;
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
            while let Some(piece) = body.chunk().await.map_err(|e| job.error_for(Method::Download, e))? {
                file.write_all(&piece)?;
            }
            atomic_file::full_sync(&file)?;
            atomic_file::sync_directory(destination.parent().ok_or(TransferError::Artifact)?)?;
            Ok(())
        })
    }
    fn activation_result(
        &self,
        op: &mut Operation,
        result: std::result::Result<Collection, meguri_engine::replace::ReplaceError>,
    ) -> Result<()> {
        match result {
            Ok(c) => {
                op.actual_generation = Some(c.generation().0);
                Ok(())
            }
            Err(error) => {
                if let meguri_engine::replace::ReplaceError::AfterActivation { generation, .. } = &error {
                    op.actual_generation = generation.map(|g| g.0);
                }
                // Preparation I/O keeps its storage category. Once the commit
                // hook has saved activation intent, swap I/O is a typed
                // replacement failure; failed() retains the fresh-decision
                // fence and advances the operation revision for recovery.
                if op.phase == Phase::Activating
                    && matches!(
                        &error,
                        meguri_engine::replace::ReplaceError::Staging(
                            meguri_storage::verified_replace::ReplaceError::Io(_)
                        )
                    )
                {
                    return Err(TransferError::Replacement {
                        activated: false,
                        generation: None,
                    });
                }
                Err(replacement_error(error))
            }
        }
    }
    fn complete_collection(&self, op: &mut Operation) -> Result<Operation> {
        // Clears legacy manual state only; R6's cloud decision is untouched.
        DeviceState::for_collection(&self.live()).record_manual_transfer_success(
            (op.direction == Direction::Export).then_some(EpochMillis(op.prepared_at)),
        )?;
        op.collection_complete = true;
        op.resulting_publication = DeviceState::for_collection(&self.live())
            .publication()?
            .map(|p| p.identity);
        op.phase = Phase::CollectionComplete;
        self.policy.save(op)?;
        if op.scope == Scope::CollectionAndMedia {
            op.phase = Phase::MediaIncomplete;
            op.boundary = None;
            op.media.stage = MediaStage::AwaitingConsent;
            self.policy.save(op)?;
        } else {
            self.policy.finish(op, self.inputs.clock.now_ms())?;
        }
        Ok(op.clone())
    }
    /// Explicit offline recovery preparation. A possible upload without durable
    /// OK has no protocol proof of completion: preserve evidence and overlap.
    /// Known OK permits a NEW local discard decision, never another upload.
    pub fn prepare_recovery(
        &self,
        slot: &SlotGuard,
        id: &OperationId,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> Result<Preview> {
        let _flight = Flight::acquire()?;
        let mut op = self.policy.get(id)?;
        cancel.check()?;
        self.eligibility(slot, op.direction)?;
        if op.collection_complete && op.scope == Scope::CollectionAndMedia {
            return Err(TransferError::NeedsDecision(Boundary::MediaProviderUnavailable));
        }
        if !recoverable(&op) {
            let boundary = if op.acceptance == Acceptance::PossiblyAccepted {
                Boundary::RemoteCompletionUnknown
            } else {
                Boundary::PreparationInterrupted
            };
            if self.policy.active()?.as_ref().map(|active| &active.id) == Some(&op.id) {
                op.phase = Phase::NeedsDecision;
                op.boundary = Some(boundary);
                self.policy.save(&mut op)?;
            }
            return Err(TransferError::NeedsDecision(boundary));
        }
        self.stage("checking", progress);
        let (guard, local) = if self.inputs.engine.collection().is_ok() {
            let exclusive = slot.enter_exclusive(true)?;
            slot.check_verified_review(reason(op.direction))?;
            let _verified = self.restore_candidate(&op)?;
            cancel.check()?;
            self.capture(Some(exclusive.collection()))?
        } else {
            let _ownership = meguri_storage::lockfile::CollectionLock::acquire(&self.inputs.paths.directory)
                .map_err(|_| TransferError::Unavailable)?;
            slot.check_verified_review(reason(op.direction))?;
            self.check_absent_recovery_source(&op)?;
            let _verified = self.restore_candidate(&op)?;
            cancel.check()?;
            self.capture(None)?
        };
        op.recovery_guard = Some(guard);
        op.recovery_local = local;
        op.phase = Phase::Preview;
        op.boundary = Some(Boundary::LocalChanged);
        self.policy.save(&mut op)?;
        Ok(preview(&op, true))
    }
    // Limit recovery without a writer to a first Import whose original source
    // was absent and is still absent. R6 reconciliation and binding validation
    // run inside initial replacement, before its ownership-held commit hook.
    fn check_absent_recovery_source(&self, op: &Operation) -> Result<()> {
        let original = op.guard.as_ref().ok_or(TransferError::Stale(Boundary::LocalChanged))?;
        if op.direction != Direction::Import
            || op.acceptance != Acceptance::NotSent
            || op.activation_identity.is_none()
            || op.local.is_some()
            || op.actual_generation.is_some()
            || original.closed_main.is_some()
            || original.closed_wal.is_some()
        {
            return Err(TransferError::NeedsDecision(Boundary::ActivationInterrupted));
        }
        let (current, _) = self.capture(None)?;
        if current.closed_main.is_some()
            || current.closed_wal.is_some()
            || original
                .publication
                .as_ref()
                .is_some_and(|prior| current.publication.as_ref() != Some(prior))
        {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        Ok(())
    }
    fn same_recovery_local(&self, op: &Operation, c: Option<&Collection>) -> Result<()> {
        let (guard, facts) = self.capture(c)?;
        if op.recovery_guard.as_ref() != Some(&guard) || op.recovery_local != facts {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        Ok(())
    }
    fn activation_proven(&self, op: &Operation) -> Result<bool> {
        let device = DeviceState::for_collection(&self.live());
        let Some(evidence) = device.replacement_evidence()? else {
            return Ok(false);
        };
        let candidate = op.candidate.as_ref().ok_or(TransferError::Artifact)?;
        let expected = op.activation_identity.as_ref().unwrap_or(&candidate.identity);
        let prior_matches = op.guard.as_ref().is_some_and(|guard| match &guard.publication {
            Some(prior) => prior == &evidence.prior,
            None => {
                guard.closed_main.as_ref().map(|f| (f.length, f.sha256))
                    == evidence.old_file.as_ref().map(|f| (f.length, f.sha256))
            }
        });
        Ok(evidence.reason == reason(op.direction)
            && prior_matches
            && evidence.candidate_file.length == expected.length
            && evidence.candidate_file.sha256 == expected.sha256
            && device.publication()?.as_ref() == Some(&evidence.candidate))
    }
    /// Explicit cancellation/dismissal of a no-upload preview only. Unknown or
    /// known-accepted incomplete operations cannot be cleared by this action.
    pub fn cancel_prepared(&self, id: &OperationId, interrupted: bool) -> Result<Operation> {
        let _flight = Flight::acquire()?;
        let mut op = self.policy.get(id)?;
        if !matches!(op.acceptance, Acceptance::NotSent | Acceptance::Refused)
            || op.phase == Phase::Activating
            || op.activation_identity.is_some()
            || op.collection_complete
        {
            return Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown));
        }
        op.phase = Phase::Cancelled;
        op.boundary = Some(if interrupted {
            Boundary::Interrupted
        } else {
            Boundary::Cancelled
        });
        self.policy.end_before_activation(&mut op)?;
        Ok(op)
    }
    fn failed(&self, op: &mut Operation, error: &TransferError) {
        if matches!(error, TransferError::MediaIncomplete { .. }) {
            return;
        }
        let boundary = match error {
            TransferError::Stale(b) | TransferError::NeedsDecision(b) => *b,
            TransferError::Artifact => Boundary::ArtifactChanged,
            TransferError::Cancelled => Boundary::Cancelled,
            TransferError::Interrupted => Boundary::Interrupted,
            TransferError::InvalidDownload(_) => Boundary::InvalidDownload,
            TransferError::Server(_) => Boundary::Server,
            _ => Boundary::Storage,
        };
        op.boundary = Some(boundary);
        if op.phase == Phase::Activating || op.acceptance == Acceptance::ExactOk {
            op.phase = Phase::NeedsDecision;
            op.boundary = Some(Boundary::ActivationInterrupted);
            let _ = self.policy.save(op);
        } else if op.acceptance == Acceptance::PossiblyAccepted {
            op.phase = Phase::NeedsDecision;
            op.boundary = Some(Boundary::RemoteCompletionUnknown);
            let _ = self.policy.save(op);
        } else if matches!(error, TransferError::Stale(_) | TransferError::NeedsDecision(_)) {
            op.phase = Phase::NeedsDecision;
            let _ = self.policy.save(op);
        } else {
            op.phase = if matches!(error, TransferError::Cancelled | TransferError::Interrupted) {
                Phase::Cancelled
            } else {
                Phase::Failed
            };
            let _ = self.policy.end_before_activation(op);
        }
    }
    fn cancel_media_jobs(&self, cancel: &CancelToken) -> Result<()> {
        let events = self.inputs.engine.stream().subscribe();
        loop {
            let running: Vec<_> = self
                .inputs
                .engine
                .jobs()
                .running()
                .into_iter()
                .filter(|job| job.name == meguri_engine::JobKind::MediaSync.name())
                .collect();
            if running.is_empty() {
                return Ok(());
            }
            for job in running {
                job.cancel.cancel();
            }
            cancel.check()?;
            events.recv_timeout(std::time::Duration::from_millis(100));
        }
    }
    fn check_layout(&self) -> Result<()> {
        let resolve = |path: &Path| -> Result<PathBuf> {
            if !path.is_absolute()
                || path.components().any(|c| {
                    matches!(c, std::path::Component::ParentDir)
                        || c.as_os_str()
                            .to_str()
                            .is_some_and(|s| s.starts_with(".meguri-stage-") || s.starts_with(".meguri-tmp-"))
                })
            {
                return Err(TransferError::Artifact);
            }
            let parent = ancestor(path)?;
            let suffix = path.strip_prefix(&parent).map_err(|_| TransferError::Artifact)?;
            Ok(parent.canonicalize()?.join(suffix))
        };
        let root = resolve(&self.inputs.state_directory)?;
        for (supplied, pinned) in [
            (&self.inputs.collection_directory, &self.inputs.paths.directory),
            (&self.inputs.backups_directory, &self.inputs.paths.backups),
        ] {
            // Validate the supplied spelling without making a mutable alias the
            // authority for transfer bytes or replacement destinations.
            let protected = resolve(supplied)?;
            if &protected != pinned || root.starts_with(&protected) || protected.starts_with(&root) {
                return Err(TransferError::Artifact);
            }
        }
        Ok(())
    }
    fn preflight(&self, direction: Direction, declared: Option<u64>) -> Result<()> {
        let s = file_size(&self.live())?
            .checked_add(file_size(&self.live().with_file_name("collection.anki2-wal"))?)
            .ok_or(TransferError::Storage)?;
        let b = if s == 0 { 0 } else { mul_add(s, 2, 4 * 1024 * 1024)? };
        // Includes independent durable candidate/original copies in addition to
        // O10's staging, compaction, package and possible schema-upgrade copies.
        let (local, backups) = match (direction, declared) {
            (Direction::Export, None) => (
                mul_add(s, 8, 0)?,
                b.checked_add(mul_add(s, 2, 4 * 1024 * 1024)?)
                    .ok_or(TransferError::Storage)?,
            ),
            (Direction::Import, None) => (mul_add(s, 4, 0)?, b),
            (Direction::Export, Some(d)) => (mul_add(d, 2, 0)?, mul_add(d, 2, 4 * 1024 * 1024)?),
            (Direction::Import, Some(d)) => (mul_add(d, 4, s)?, b),
        };
        let collection = ancestor(&self.inputs.paths.directory)?;
        let backup = ancestor(&self.inputs.paths.backups)?;
        let state = ancestor(&self.inputs.state_directory)?;
        // Add needs by actual filesystem, including the durable evidence root.
        let mut budgets = std::collections::BTreeMap::<u64, (PathBuf, u64)>::new();
        for (path, need) in [
            (collection, local),
            (backup, backups),
            (state, mul_add(declared.unwrap_or(s).max(s), 2, 4 * 1024 * 1024)?),
        ] {
            let entry = budgets.entry(fs::metadata(&path)?.dev()).or_insert((path, 0));
            entry.1 = entry.1.checked_add(need).ok_or(TransferError::Storage)?;
        }
        for (_, (path, need)) in budgets {
            meguri_storage::space::require_space(self.inputs.space.as_ref(), &path, need)?;
        }
        Ok(())
    }
}

enum UploadAnswer {
    Ok,
    Failure {
        error: crate::JobError,
        refused: bool,
        preflight: bool,
    },
}
fn preview(op: &Operation, recovery: bool) -> Preview {
    Preview {
        operation: op.clone(),
        confirmation: Confirmation {
            id: op.id.clone(),
            revision: op.revision,
            recovery,
        },
        pending_local_work: false,
    }
}
fn reason(direction: Direction) -> ReplaceReason {
    if direction == Direction::Import {
        ReplaceReason::FullDownload
    } else {
        ReplaceReason::FullUpload
    }
}
fn same_served(local: &LocalFacts, remote: &RemoteFacts) -> Result<()> {
    if local.modified != remote.modified
        || local.schema_modified != remote.schema_modified
        || local.usn != remote.usn
        || (local.cards == 0) != remote.empty
    {
        Err(TransferError::Stale(Boundary::RemoteChanged))
    } else {
        Ok(())
    }
}
fn ancestor(path: &Path) -> Result<PathBuf> {
    let mut path = path;
    loop {
        match path.symlink_metadata() {
            Ok(m) if m.file_type().is_dir() => return Ok(path.to_owned()),
            Ok(_) => return Err(TransferError::Artifact),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                path = path.parent().ok_or(TransferError::Artifact)?
            }
            Err(e) => return Err(e.into()),
        }
    }
}
fn file_size(path: &Path) -> Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => Ok(m.len()),
        Ok(_) => Err(TransferError::Artifact),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e.into()),
    }
}
fn mul_add(size: u64, times: u64, extra: u64) -> Result<u64> {
    size.checked_mul(times)
        .and_then(|s| s.checked_add(extra))
        .ok_or(TransferError::Storage)
}
fn staging_error(e: meguri_storage::verified_replace::ReplaceError) -> TransferError {
    use meguri_storage::{OpenError as O, verified_replace::ReplaceError as R};
    match e {
        R::StagingRefused(error) => TransferError::InvalidDownload(match error {
            O::TooOld { version } => InvalidDownload::TooOld(version),
            O::TooNew { version } => InvalidDownload::TooNew(version),
            O::V1Scheduler => InvalidDownload::V1Scheduler,
            O::NotACollection { .. } => InvalidDownload::NotACollection,
            O::UpgradeFailed { version, .. } => InvalidDownload::UpgradeFailed(version),
            _ => InvalidDownload::Corrupt,
        }),
        R::StagingCorrupt => TransferError::InvalidDownload(InvalidDownload::Corrupt),
        R::Io(e) | R::CallerChangeFailed(e) => e.into(),
        _ => TransferError::Storage,
    }
}
fn replacement_error(e: meguri_engine::replace::ReplaceError) -> TransferError {
    use meguri_engine::replace::ReplaceError as R;
    match e {
        R::AfterActivation { generation, .. } => TransferError::Replacement {
            activated: true,
            generation: generation.map(|g| g.0),
        },
        R::Lifecycle(e) => e.into(),
        R::Staging(e) => staging_error(e),
        _ => TransferError::Replacement {
            activated: false,
            generation: None,
        },
    }
}
fn package_error(e: meguri_package::CollectionPackageWriteError) -> TransferError {
    use meguri_package::CollectionPackageWriteError as E;
    match e {
        E::Cancelled => TransferError::Cancelled,
        E::DiskFull => TransferError::DiskFull {
            needed: None,
            available: None,
        },
        E::InsufficientSpace { needed, available } => TransferError::DiskFull {
            needed: Some(needed),
            available: Some(available),
        },
        _ => TransferError::Storage,
    }
}
pub fn worker_error(e: TransferError) -> EngineError {
    match e {
        TransferError::Cancelled => EngineError::Cancelled,
        TransferError::SessionActive => EngineError::SessionActive,
        _ => EngineError::Storage(StorageError::Io {
            detail: "manual transfer did not complete; read its typed result and durable history".into(),
        }),
    }
}

fn recoverable(op: &Operation) -> bool {
    op.acceptance == Acceptance::ExactOk
        || (op.direction == Direction::Import
            && op.acceptance == Acceptance::NotSent
            && op.activation_identity.is_some())
}
fn optional_identity(path: &Path) -> Result<Option<FileIdentity>> {
    match path.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(_) => files::identity(path).map(Some),
    }
}
