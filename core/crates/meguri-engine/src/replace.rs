//! Verified replacement is the swap step of the existing lifecycle, never a
//! second writer lifecycle. Backup/inspection/confirmation belong to consumers.
use crate::device_state::{self, AccountBinding};
use crate::{Collection, EngineError, Exclusive, SlotGuard};
pub use meguri_storage::verified_replace::{ReplaceError as StagingError, ReplacementStage};
use meguri_storage::{OpenOptions, StorageError, WriteScope};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplaceReason {
    Restore,
    ColpkgImport,
    FullDownload,
    FullUpload,
    CloudAdopt,
}
impl ReplaceReason {
    pub(crate) fn package(self) -> bool {
        matches!(self, Self::Restore | Self::ColpkgImport)
    }
    pub(crate) fn requires_publication_decision(self) -> bool {
        matches!(self, Self::Restore | Self::ColpkgImport | Self::FullDownload)
    }
    pub(crate) fn refuses_active_review(self) -> bool {
        matches!(
            self,
            Self::ColpkgImport | Self::FullDownload | Self::FullUpload | Self::CloudAdopt
        )
    }
    pub(crate) fn long_op(self) -> crate::LongOp {
        match self {
            Self::CloudAdopt => crate::LongOp::CloudAdopt,
            Self::Restore => crate::LongOp::Restore,
            Self::ColpkgImport => crate::LongOp::ColpkgImport,
            Self::FullDownload | Self::FullUpload => crate::LongOp::OneWaySync,
        }
    }
}
#[derive(Default)]
struct RestorePreparation {
    raw: Option<device_state::RawRestoreAuthority>,
    safety_backup: Option<device_state::ReplacementBackup>,
}
/// Caller-owned adoption identities and the complete explicit cloud/local decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudAdoptInput {
    pub operation: device_state::PublicationId,
    pub candidate: device_state::Publication,
}
impl CloudAdoptInput {
    pub(crate) fn validate(&self) -> Result<(), EngineError> {
        if self.candidate.decision != device_state::PublicationDecision::Clear
            || !self.candidate.binding.certain()
            || self.operation == self.candidate.identity
        {
            return Err(EngineError::InvalidArgument {
                message: "adoption requires distinct caller identities and an explicit clear decision".into(),
            });
        }
        Ok(())
    }
}
pub(crate) fn require_adopt_input(reason: ReplaceReason, input: Option<&CloudAdoptInput>) -> Result<(), EngineError> {
    match (reason, input) {
        (ReplaceReason::CloudAdopt, Some(input)) => input.validate(),
        (ReplaceReason::CloudAdopt, None) | (_, Some(_)) => Err(EngineError::InvalidArgument {
            message: "CloudAdopt requires caller operation, candidate identity and decision".into(),
        }),
        (_, None) => Ok(()),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceError {
    NotExclusive,
    Staging(StagingError),
    Lifecycle(EngineError),
    /// The rename succeeded. The candidate file stands even if opening a
    /// writer failed; Some names the successfully reopened process generation.
    AfterActivation {
        generation: Option<crate::Generation>,
        source: Box<ReplaceError>,
    },
    MediaRecoveryRequired {
        recovery_reference: String,
        counts: meguri_storage::media::ImportMediaCounts,
        source: Box<ReplaceError>,
    },
}
impl std::fmt::Display for ReplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReplaceError {}
impl From<EngineError> for ReplaceError {
    fn from(e: EngineError) -> Self {
        Self::Lifecycle(e)
    }
}
impl From<StagingError> for ReplaceError {
    fn from(e: StagingError) -> Self {
        Self::Staging(e)
    }
}
impl From<StorageError> for ReplaceError {
    fn from(e: StorageError) -> Self {
        Self::Staging(e.into())
    }
}
impl From<std::io::Error> for ReplaceError {
    fn from(e: std::io::Error) -> Self {
        Self::Staging(e.into())
    }
}

impl SlotGuard {
    /// Minimal verified entry. Collection import and manual transfers refuse an
    /// active review before closing admission, and after the exclusive drain.
    /// The legacy closure entry retains its ability to replace a session.
    pub fn replace_verified(
        &self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
    ) -> Result<Collection, ReplaceError> {
        require_adopt_input(reason, None)?;
        if self.op() != reason.long_op() {
            return Err(ReplaceError::NotExclusive);
        }
        self.check_verified_review(reason)?;
        let mut exclusive = self.enter_exclusive(true)?;
        exclusive.replace_verified(stage, reason, change)
    }
}
impl Exclusive<'_> {
    /// Validates while the live collection remains open, then reuses L8's
    /// drain/close/swap/reopen/generation/event/provider protocol. An Err after
    /// rename can leave the new generation active: inspect collection()/state.
    pub fn replace_verified(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
    ) -> Result<Collection, ReplaceError> {
        self.replace_verified_before_commit(stage, reason, change, || Ok(()))
    }

    /// Runs `before_commit` after candidate preparation, outside core locks and
    /// reader leases, immediately before closing the live connections for swap.
    /// Media entry remains cancellable until rename; restoration always runs
    /// to completion.
    pub fn replace_verified_before_commit(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
        before_commit: impl FnOnce() -> Result<(), EngineError>,
    ) -> Result<Collection, ReplaceError> {
        self.replace_verified_with_raw(stage, reason, change, || before_commit().map(|()| None), None)
    }

    /// A retained corrupt owner consumes the same verified T7 raw-pair
    /// authority as initial Restore. Other replacement reasons cannot use it.
    pub fn restore_verified_before_commit(
        &mut self,
        stage: ReplacementStage,
        before_commit: impl FnOnce() -> Result<Option<device_state::RawRestoreAuthority>, EngineError>,
    ) -> Result<Collection, ReplaceError> {
        self.replace_verified_with_raw(stage, ReplaceReason::Restore, |_| Ok(()), before_commit, None)
    }

    /// T8 carries its actual forced-provider receipt through the R6 intent and
    /// activation. Unknown/raw witnesses are not assigned a collection receipt.
    pub fn restore_verified_with_backup_before_commit(
        &mut self,
        stage: ReplacementStage,
        before_commit: impl FnOnce() -> Result<
            (
                Option<device_state::RawRestoreAuthority>,
                Option<crate::ports::BackupReceipt>,
            ),
            EngineError,
        >,
    ) -> Result<Collection, ReplaceError> {
        self.replace_verified_with_preparation(
            stage,
            ReplaceReason::Restore,
            |_| Ok(()),
            || {
                let (raw, receipt) = before_commit()?;
                Ok(RestorePreparation {
                    raw,
                    safety_backup: receipt.map(Into::into),
                })
            },
            None,
        )
    }

    /// Adopts the verified staged collection with exactly the caller's decision.
    pub fn replace_verified_cloud_adopt(
        &mut self,
        stage: ReplacementStage,
        input: CloudAdoptInput,
    ) -> Result<Collection, ReplaceError> {
        self.replace_verified_with_raw(stage, ReplaceReason::CloudAdopt, |_| Ok(()), || Ok(None), Some(input))
    }

    fn replace_verified_with_raw(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
        before_commit: impl FnOnce() -> Result<Option<device_state::RawRestoreAuthority>, EngineError>,
        cloud: Option<CloudAdoptInput>,
    ) -> Result<Collection, ReplaceError> {
        self.replace_verified_with_preparation(
            stage,
            reason,
            change,
            || {
                before_commit().map(|raw| RestorePreparation {
                    raw,
                    safety_backup: None,
                })
            },
            cloud,
        )
    }
    fn replace_verified_with_preparation(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
        before_commit: impl FnOnce() -> Result<RestorePreparation, EngineError>,
        cloud: Option<CloudAdoptInput>,
    ) -> Result<Collection, ReplaceError> {
        require_adopt_input(reason, cloud.as_ref())?;
        let environment = self.verified_environment(reason)?;
        let live = self.collection().path().to_owned();
        let options = OpenOptions::new(environment.clock.as_ref(), environment.random.as_ref())
            .initializers(environment.initializers.clone());
        meguri_storage::verified_replace::prepare(
            &stage,
            &live,
            &options,
            environment.clock.as_ref(),
            reason.package(),
            change,
        )?;
        self.activate_finalised(stage, reason, before_commit, None, cloud)
    }

    /// Activates an O6.2 artifact after independently verifying its immutable
    /// identity, device, finalisation, gate and integrity. This path never
    /// opens a candidate writer or runs preparation a second time.
    pub fn activate_prepared_upload(
        &mut self,
        artifact: PreparedUpload,
        before_commit: impl FnOnce() -> Result<(), EngineError>,
    ) -> Result<Collection, ReplaceError> {
        self.verified_environment(ReplaceReason::FullUpload)?;
        artifact.check(self.collection().path())?;
        self.activate_finalised(
            artifact.stage,
            ReplaceReason::FullUpload,
            || before_commit().map(|()| RestorePreparation::default()),
            Some(artifact.identity),
            None,
        )
    }

    fn activate_finalised(
        &mut self,
        mut stage: ReplacementStage,
        reason: ReplaceReason,
        before_commit: impl FnOnce() -> Result<RestorePreparation, EngineError>,
        identity: Option<crate::device_state::FileIdentity>,
        cloud: Option<CloudAdoptInput>,
    ) -> Result<Collection, ReplaceError> {
        let environment = self.verified_environment(reason)?;
        // Admission is privileged and the writer drained. Recheck under the
        // lifecycle/session locks after preparation and the unlocked hook.
        self.verified_environment(reason)?;
        let media = if let Some(incoming) = stage.take_local_media() {
            if reason != ReplaceReason::ColpkgImport {
                return Err(ReplaceError::NotExclusive);
            }
            if stage.media_backup.is_none() {
                return Err(EngineError::Unsupported {
                    feature: "collection media import requires its forced backup receipt".into(),
                }
                .into());
            }
            stage.media_previous_journal_absent = false;
            let service = crate::media::MediaService::open(self.collection(), environment.clock.clone())?;
            let lease = service.try_job(crate::media::MediaJobKind::CollectionImport)?;
            // The shared host store recovered before Ready. Preparation
            // refuses any remaining J4 stage without mutating its evidence.
            Some(PreparedMedia::new(
                service.store().clone(),
                &stage,
                incoming,
                Some(lease),
            )?)
        } else {
            None
        };
        let preparation = match before_commit().map_err(ReplaceError::from).and_then(|preparation| {
            let raw = &preparation.raw;
            self.verified_environment(reason)?;
            if raw.is_some()
                && (reason != ReplaceReason::Restore
                    || !self
                        .collection()
                        .core
                        .exec
                        .corrupt
                        .load(std::sync::atomic::Ordering::Acquire))
            {
                return Err(EngineError::unavailable("retained WAL authority requires a corrupt Restore owner").into());
            }
            Ok(preparation)
        }) {
            Ok(preparation) => preparation,
            Err(error) => {
                return Err(match media {
                    Some(media) => media.abort(error),
                    None => error,
                });
            }
        };
        let RestorePreparation { raw, safety_backup } = preparation;
        let media_cleanup = media.as_ref().map(|m| m.cleanup_error.clone());
        let media_recovered = media.as_ref().map(|m| m.recovered.clone());
        let threads = self.replacement_threads();
        let error = Arc::new(Mutex::new(None));
        let record = error.clone();
        let activated = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tracked = activated.clone();
        let outcome = activated.clone();
        let old_generation = self.collection().generation();
        if let Err(error) = meguri_model::faultpoint!("replace.media.before_swap_dispatch") {
            return Err(match media {
                Some(media) => media.abort(error.into()),
                None => error.into(),
            });
        }
        let result = self.replace_tracked(
            move |live| {
                if let Some(expected) = &identity {
                    meguri_storage::one_way::verify_prepared_upload(&stage, live).map_err(|e| {
                        *record.lock().unwrap() = Some(ReplaceError::Staging(e.clone()));
                        EngineError::Storage(StorageError::Io {
                            detail: "prepared upload verification failed".into(),
                        })
                    })?;
                    if device_state::fingerprint(&stage.path())?.as_ref() != Some(expected) {
                        return Err(EngineError::Storage(StorageError::Io {
                            detail: "prepared upload identity changed".into(),
                        }));
                    }
                }
                run_swap(
                    live,
                    stage,
                    reason,
                    environment.binding(),
                    environment.random,
                    environment.clock,
                    tracked,
                    media,
                    threads,
                    raw,
                    cloud,
                    safety_backup,
                )
                .map_err(|e| {
                    *record.lock().unwrap() = Some(e.clone());
                    EngineError::Storage(StorageError::Io { detail: e.to_string() })
                })
            },
            activated,
            media_recovered,
        );
        if let Some(error) = media_cleanup.and_then(|e| e.lock().unwrap().take()) {
            return Err(error);
        }
        let failure = match (result, error.lock().unwrap().take()) {
            (Ok(collection), None) => return Ok(collection),
            (_, Some(error)) => error,
            (Err(error), None) => ReplaceError::Lifecycle(error),
        };
        if outcome.load(std::sync::atomic::Ordering::Acquire) {
            let generation =
                (self.collection().generation() != old_generation).then_some(self.collection().generation());
            Err(ReplaceError::AfterActivation {
                generation,
                source: Box::new(failure),
            })
        } else {
            Err(failure)
        }
    }
}

struct SwapContext<'a> {
    binding: &'a AccountBinding,
    random: &'a dyn meguri_model::rng::RandomSource,
    verified_at: i64,
    cloud: Option<CloudAdoptInput>,
    safety_backup: Option<device_state::ReplacementBackup>,
}

fn swap(
    live: &Path,
    stage: ReplacementStage,
    reason: ReplaceReason,
    context: SwapContext<'_>,
    activated: &std::sync::atomic::AtomicBool,
    recovery: (Option<&mut PreparedMedia>, Option<device_state::RawRestoreAuthority>),
) -> Result<(), ReplaceError> {
    let SwapContext {
        binding,
        random,
        verified_at,
        cloud,
        safety_backup,
    } = context;
    let (mut media, raw) = recovery;
    use meguri_storage::{atomic_file, verified_replace as storage};
    let cancel = media
        .as_ref()
        .and_then(|m| m.cancel.clone())
        .or_else(|| stage.media_cancel.clone());
    let cancelled = || cancel.as_ref().is_some_and(|c| c());
    if cancelled() {
        return Err(EngineError::Cancelled.into());
    }
    storage::check_device(&stage, live)?;
    meguri_model::faultpoint!("replace.before_live_wal_check")?;
    let retained_wal = raw.is_some();
    if !retained_wal {
        storage::check_live_wal(live)?;
    }
    // Every replacement retires queued tokens. Manual Import requires a new
    // publication decision; Export preserves the exact prior decision.
    let intent = media.as_ref().map(|m| m.intent.clone());
    device_state::begin_with_cloud(
        (live, &stage.path()),
        reason,
        binding,
        random,
        (intent, raw),
        cloud,
        safety_backup,
        &cancelled,
        || {
            if let Some(m) = media.as_mut() {
                m.published = true;
                m.owner.recovery_required();
                m.recovered.store(false, std::sync::atomic::Ordering::Release);
            }
        },
    )
    .map_err(|e| {
        if cancelled() {
            ReplaceError::from(EngineError::Cancelled)
        } else {
            e.into()
        }
    })?;
    device_state::record_verified_check(live, verified_at)?;
    meguri_model::faultpoint!("replace.after_pause_recorded")?;
    if retained_wal {
        device_state::retire_restore_wal(live)?;
    }
    storage::check_live_wal(live)?;
    storage::remove_empty_live_sidecars(live)?;
    meguri_model::faultpoint!("replace.after_live_wal_removed")?;
    if let Some(media) = media.as_mut() {
        device_state::publish_media(
            live,
            &mut media.intent,
            &media.store,
            media.cancel.as_deref().unwrap_or(&|| false),
        )
        .map_err(|e| {
            if e == meguri_storage::media::MediaError::Cancelled {
                ReplaceError::Lifecycle(EngineError::Cancelled)
            } else {
                ReplaceError::Lifecycle(EngineError::Media(e))
            }
        })?;
        if media.cancel.as_ref().is_some_and(|cancel| cancel()) {
            return Err(EngineError::Cancelled.into());
        }
    }
    device_state::verify_swap_files(live, &stage.path(), &cancelled).map_err(|e| {
        if cancelled() {
            ReplaceError::from(EngineError::Cancelled)
        } else {
            e.into()
        }
    })?;
    meguri_model::faultpoint!("replace.media.after_final_database_verification")?;
    if cancelled() {
        return Err(EngineError::Cancelled.into());
    }
    storage::check_live_wal(live)?;
    std::fs::rename(stage.path(), live)?;
    activated.store(true, std::sync::atomic::Ordering::Release);
    meguri_model::faultpoint!("replace.after_rename")?;
    atomic_file::sync_directory(live.parent().unwrap_or(Path::new(".")))?;
    meguri_model::faultpoint!("replace.after_dir_sync")?;
    // Media completion is reconciled by run_swap before any writer reopens.
    if media.is_none() {
        if retained_wal {
            device_state::finish_restore_wal(live, binding)?;
        } else {
            device_state::activate(live, binding)?;
        }
    }
    // TempDir removes only the operation-owned directory, best effort.
    drop(stage);
    Ok(())
}

pub(crate) struct PreparedMedia {
    store: meguri_storage::media::MediaStore,
    owner: meguri_storage::media::MediaImportOwner,
    _lease: Option<crate::media::MediaJobLease>,
    intent: meguri_storage::media::CollectionMediaIntent,
    pub(crate) recovered: Arc<std::sync::atomic::AtomicBool>,
    cancel: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    published: bool,
    cleanup_error: Arc<Mutex<Option<ReplaceError>>>,
    initial: Option<meguri_storage::media::InitialMediaState>,
}
fn media_error(error: meguri_storage::media::MediaError) -> EngineError {
    if error == meguri_storage::media::MediaError::Cancelled {
        EngineError::Cancelled
    } else {
        EngineError::Media(error)
    }
}
impl PreparedMedia {
    pub(crate) fn new_initial(
        live: &Path,
        clock: Arc<dyn meguri_model::clock::Clock>,
        random: Arc<dyn meguri_model::rng::RandomSource>,
        stage: &mut ReplacementStage,
        incoming: Vec<meguri_storage::media::IncomingMedia>,
    ) -> Result<Self, ReplaceError> {
        use meguri_storage::media::InitialMediaState;
        let cancel = stage.media_cancel.clone();
        let cancelled = || cancel.as_ref().is_some_and(|c| c());
        let initial = InitialMediaState::begin(live, &cancelled).map_err(|failure| {
            let source = ReplaceError::from(media_error(failure.error));
            match failure.recovery_reference {
                Some(recovery_reference) => ReplaceError::MediaRecoveryRequired {
                    recovery_reference,
                    counts: Default::default(),
                    source: Box::new(source),
                },
                None => source,
            }
        })?;
        stage.media_previous_journal_absent = initial.journal_was_absent();
        let store = match initial.open_store(live, clock, random, &cancelled) {
            Ok(store) => store,
            Err(error) => return Err(Self::initial_failure(initial, None, media_error(error).into())),
        };
        // Initial entry has no collection rows that can witness an old stage.
        if let Err(error) = store.recover_checked(|_| Ok(false), &cancelled) {
            return Err(Self::initial_failure(initial, Some(&store), media_error(error).into()));
        }
        match Self::new(store.clone(), stage, incoming, None) {
            Ok(mut media) => {
                if let Err(error) = initial.attach(&mut media.intent) {
                    // Keep the initial guard until unpublished cleanup finishes.
                    media.initial = Some(initial);
                    return Err(media.abort(media_error(error).into()));
                }
                media.initial = Some(initial);
                Ok(media)
            }
            Err(error) => Err(Self::initial_failure(initial, Some(&store), error)),
        }
    }
    fn initial_failure(
        mut initial: meguri_storage::media::InitialMediaState,
        store: Option<&meguri_storage::media::MediaStore>,
        source: ReplaceError,
    ) -> ReplaceError {
        if initial.restore_unpublished(store).is_ok() {
            return source;
        }
        if let Some(store) = store {
            let _ = store.try_close();
        }
        let recovery_reference = initial.recovery_reference().to_owned();
        initial.retain();
        ReplaceError::MediaRecoveryRequired {
            recovery_reference,
            counts: Default::default(),
            source: Box::new(source),
        }
    }
    pub(crate) fn abort(mut self, source: ReplaceError) -> ReplaceError {
        match self.discard() {
            Ok(()) => source,
            Err(error) => error,
        }
    }
    fn discard(&mut self) -> Result<(), ReplaceError> {
        let result = (|| {
            self.intent.discard_unpublished(&self.store).map_err(media_error)?;
            if let Some(initial) = self.initial.as_mut() {
                initial.restore_unpublished(Some(&self.store)).map_err(media_error)?;
            }
            drop(self.initial.take());
            self.recovered.store(true, std::sync::atomic::Ordering::Release);
            self.owner.resolved();
            Ok::<_, ReplaceError>(())
        })();
        // Once cleanup is uncertain, leave its remaining proof for recovery;
        // Drop must not retry the removal.
        self.published = true;
        if let Err(source) = result {
            self.owner.recovery_required();
            self.recovered.store(false, std::sync::atomic::Ordering::Release);
            let recovery_reference = if let Some(initial) = self.initial.as_mut() {
                let reference = initial.recovery_reference().to_owned();
                initial.retain();
                reference
            } else {
                self.intent.recovery_reference().to_owned()
            };
            return Err(ReplaceError::MediaRecoveryRequired {
                recovery_reference,
                counts: self.intent.counts.clone(),
                source: Box::new(source),
            });
        }
        Ok(())
    }
    pub(crate) fn new(
        store: meguri_storage::media::MediaStore,
        stage: &ReplacementStage,
        incoming: Vec<meguri_storage::media::IncomingMedia>,
        lease: Option<crate::media::MediaJobLease>,
    ) -> Result<Self, ReplaceError> {
        let mut owner = store.own_collection_import().map_err(EngineError::Media)?;
        let prepared = (|| {
            store
                .preflight_collection_import(
                    &incoming,
                    stage.media_space.as_deref(),
                    stage.media_cancel.as_deref().unwrap_or(&|| false),
                )
                .map_err(media_error)?;
            store
                .prepare_collection_import(
                    stage.directory(),
                    incoming,
                    stage.media_backup.clone(),
                    stage.media_cancel.as_deref().unwrap_or(&|| false),
                    stage.media_previous_journal_absent,
                )
                .map_err(|failure| {
                    let source = ReplaceError::from(media_error(failure.error));
                    match failure.recovery_reference {
                        Some(recovery_reference) => ReplaceError::MediaRecoveryRequired {
                            recovery_reference,
                            counts: Default::default(),
                            source: Box::new(source),
                        },
                        None => source,
                    }
                })
        })();
        let intent = match prepared {
            Ok(intent) => intent,
            Err(e) => {
                if matches!(e, ReplaceError::MediaRecoveryRequired { .. }) {
                    owner.recovery_required();
                } else {
                    owner.resolved();
                }
                return Err(e);
            }
        };
        *stage.media_counts().lock().unwrap() = intent.counts.clone();
        *stage.media_reference().lock().unwrap() = Some(intent.recovery_reference().to_owned());
        Ok(Self {
            store,
            owner,
            intent,
            _lease: lease,
            recovered: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            cancel: stage.media_cancel.clone(),
            published: false,
            cleanup_error: Default::default(),
            initial: None,
        })
    }
}
impl Drop for PreparedMedia {
    fn drop(&mut self) {
        if !self.published {
            if let Err(error) = self.discard() {
                *self.cleanup_error.lock().unwrap() = Some(error);
            }
        } else if let Some(initial) = self.initial.as_mut() {
            initial.retain();
        }
    }
}
/// The writer has closed its connection before invoking this. All hashing,
/// journal recovery and payload streaming run on a registered replacement
/// worker, never on the collection writer or under lifecycle locks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_swap(
    live: &Path,
    stage: ReplacementStage,
    reason: ReplaceReason,
    binding: AccountBinding,
    random: Arc<dyn meguri_model::rng::RandomSource>,
    clock: Arc<dyn meguri_model::clock::Clock>,
    activated: Arc<std::sync::atomic::AtomicBool>,
    mut media: Option<PreparedMedia>,
    threads: crate::ThreadRegistry,
    raw: Option<device_state::RawRestoreAuthority>,
    cloud: Option<CloudAdoptInput>,
    safety_backup: Option<device_state::ReplacementBackup>,
) -> Result<(), ReplaceError> {
    if media.is_none() {
        return swap(
            live,
            stage,
            reason,
            SwapContext {
                binding: &binding,
                random: random.as_ref(),
                verified_at: clock.now_ms(),
                cloud,
                safety_backup,
            },
            &activated,
            (None, raw),
        );
    }
    let cleanup_error = media.as_ref().unwrap().cleanup_error.clone();
    let live = live.to_owned();
    if let Err(error) = meguri_model::faultpoint!("replace.media.before_worker_spawn") {
        return Err(media.take().unwrap().abort(error.into()));
    }
    let worker = threads
        .spawn_owned(
            "collection-media-replacement",
            crate::Qos::UserInitiated,
            "writer",
            move |activity| {
                let _busy = activity.busy();
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    swap(
                        &live,
                        stage,
                        reason,
                        SwapContext {
                            binding: &binding,
                            random: random.as_ref(),
                            verified_at: clock.now_ms(),
                            cloud,
                            safety_backup,
                        },
                        &activated,
                        (media.as_mut(), raw),
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(meguri_storage::StorageError::Io {
                        detail: "collection media swap interrupted".into(),
                    }
                    .into())
                });
                let m = media.as_mut().expect("media worker owns prepared intent");
                if !m.published {
                    return match outcome {
                        Err(error) => Err(media.take().unwrap().abort(error)),
                        Ok(()) => unreachable!(),
                    };
                }
                let owns = device_state::owns_media_intent(&live, m.intent.recovery_reference()).map_err(|error| {
                    ReplaceError::MediaRecoveryRequired {
                        recovery_reference: m.intent.recovery_reference().to_owned(),
                        counts: m.intent.counts.clone(),
                        source: Box::new(error.into()),
                    }
                })?;
                if !owns {
                    m.published = false;
                    return match outcome {
                        Err(error) => Err(media.take().unwrap().abort(error)),
                        Ok(()) => unreachable!(),
                    };
                }
                match device_state::reconcile_with_media(&live, &binding, clock, random.clone(), Some(&m.store)) {
                    Ok(()) => {
                        match device_state::media_activation(&live, m.intent.recovery_reference()) {
                            Ok(Some(true)) => activated.store(true, std::sync::atomic::Ordering::Release),
                            Ok(None) if activated.load(std::sync::atomic::Ordering::Acquire) || outcome.is_ok() => {
                                return Err(ReplaceError::MediaRecoveryRequired {
                                    recovery_reference: m.intent.recovery_reference().to_owned(),
                                    counts: m.intent.counts.clone(),
                                    source: Box::new(
                                        EngineError::internal("candidate media activation evidence missing").into(),
                                    ),
                                });
                            }
                            Ok(_) => {}
                            Err(e) => {
                                return Err(ReplaceError::MediaRecoveryRequired {
                                    recovery_reference: m.intent.recovery_reference().to_owned(),
                                    counts: m.intent.counts.clone(),
                                    source: Box::new(e.into()),
                                });
                            }
                        }
                        drop(m.initial.take());
                        m.owner.resolved();
                        m.recovered.store(true, std::sync::atomic::Ordering::Release);
                        // R6 completion precedes cleanup. A cleanup failure only
                        // retains owned recovery bytes; it cannot undo activation.
                        let _ = m.intent.cleanup(&m.store);
                        outcome
                    }
                    Err(e) => Err(ReplaceError::MediaRecoveryRequired {
                        recovery_reference: m.intent.recovery_reference().to_owned(),
                        counts: m.intent.counts.clone(),
                        source: Box::new(ReplaceError::Staging(e.into())),
                    }),
                }
            },
        )
        .map_err(meguri_storage::StorageError::from)
        .map_err(EngineError::from)
        .map_err(|e| cleanup_error.lock().unwrap().take().unwrap_or_else(|| e.into()))?;
    let outcome = worker
        .join()
        .map_err(|p| ReplaceError::Lifecycle(EngineError::from_panic(p.as_ref())))?;
    if let Some(error) = cleanup_error.lock().unwrap().take() {
        return Err(error);
    }
    outcome
}

/// Owned, verified prepared bytes. Construction verifies the caller's recorded
/// byte identity; no arbitrary unchecked path or caller transform is accepted.
pub struct PreparedUpload {
    stage: ReplacementStage,
    identity: crate::device_state::FileIdentity,
}
impl PreparedUpload {
    pub fn verify(stage: ReplacementStage, live: &Path, length: u64, sha256: [u8; 32]) -> Result<Self, ReplaceError> {
        let artifact = Self {
            stage,
            identity: crate::device_state::FileIdentity { length, sha256 },
        };
        artifact.check(live)?;
        Ok(artifact)
    }
    fn check(&self, live: &Path) -> Result<(), ReplaceError> {
        meguri_storage::one_way::verify_prepared_upload(&self.stage, live)?;
        if device_state::fingerprint(&self.stage.path())
            .map_err(StagingError::from)?
            .as_ref()
            != Some(&self.identity)
        {
            return Err(StagingError::StagingNotFinalised.into());
        }
        Ok(())
    }
}
