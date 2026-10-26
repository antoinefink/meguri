//! RF2 maps jobs, results and ownership for the native entry flow. Start the
//! event stream before either job. During inspection, keep scoped file access
//! in a synchronous Foundation coordination worker and await Rust completion
//! from the async caller. The worker blocks until every terminal outcome has
//! been delivered. Cancellation also waits for terminal delivery before the
//! coordinated read and scoped access end. Read the URL from the coordinator.
use crate::{
    callback,
    collection::{CollectionLocation, DiskSpaceProvider, ForeignClock, ForeignSpace, HostClock},
    error::CoreError,
    jobs::{self, JobHandle},
    lifecycle::CollectionOpenFailure,
    runtime,
};
use meguri_engine::{
    EngineError, JobKind, ports,
    replace::{ReplaceError, StagingError},
};
use meguri_host::HostConfig;
use meguri_model::rng::SeededRandom;
use meguri_package::{
    CollectionPackageInspectError as PackageError,
    entry::{self, EntryError},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PackageFailure {
    TooNew,
    UnsupportedFormat,
    UnsupportedMedia,
    SchemaUnsupported { gate: CollectionOpenFailure },
    Corrupt { detail: String },
    InsufficientSpace { needed: u64, available: u64 },
    DiskFull,
    SourceNotFinalised,
    DestinationExists,
    Cancelled,
    Io { detail: String },
}
impl From<PackageError> for PackageFailure {
    fn from(package_failure: PackageError) -> Self {
        use PackageError as Input;
        match package_failure {
            Input::Cancelled => Self::Cancelled,
            Input::DestinationExists => Self::DestinationExists,
            Input::DiskFull => Self::DiskFull,
            Input::SourceNotFinalised => Self::SourceNotFinalised,
            Input::TooNew => Self::TooNew,
            Input::UnsupportedMedia => Self::UnsupportedMedia,
            Input::UnsupportedFormat => Self::UnsupportedFormat,
            Input::SchemaUnsupported { gate } => Self::SchemaUnsupported { gate: gate.into() },
            Input::InsufficientSpace { needed, available } => Self::InsufficientSpace { needed, available },
            Input::Corrupt { detail } => Self::Corrupt { detail },
            Input::Io { detail } => Self::Io { detail },
            export_failure @ (Input::ExportFormatLimit
            | Input::ExportSourceInvalid { .. }
            | Input::ExportSourceChanged) => {
                // RF2 and forced backup retain collection-only publication.
                Self::Io {
                    detail: export_failure.to_string(),
                }
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImportBackupReceipt {
    pub name: String,
    pub size_bytes: u64,
    pub instant_ms: i64,
    pub snapshot_modified_ms: i64,
}
impl From<ports::BackupReceipt> for ImportBackupReceipt {
    fn from(r: ports::BackupReceipt) -> Self {
        Self {
            name: r.name,
            size_bytes: r.size_bytes,
            instant_ms: r.instant.0,
            snapshot_modified_ms: r.snapshot_modified.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ImportBackupPublication {
    Unpublished,
    Uncertain { name: String, instant_ms: i64 },
    Durable { receipt: ImportBackupReceipt },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ImportBackupCause {
    Core { error: CoreError },
    Package { error: PackageFailure },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ImportStagingFailure {
    CrossDevice,
    StagingRefused { gate: CollectionOpenFailure },
    StagingCorrupt,
    CallerChangeFailed { error: CoreError },
    StagingNotFinalised,
    LiveWalNotEmpty,
    Io { detail: String },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ImportReplacementFailure {
    NotExclusive,
    Staging { failure: ImportStagingFailure },
    Core { error: CoreError },
    MediaRecoveryRequired { recovery_reference: String },
}
impl From<ReplaceError> for ImportReplacementFailure {
    fn from(error: ReplaceError) -> Self {
        match error {
            ReplaceError::NotExclusive => Self::NotExclusive,
            ReplaceError::Lifecycle(error) => Self::Core { error: error.into() },
            ReplaceError::AfterActivation { source, .. } => (*source).into(),
            ReplaceError::MediaRecoveryRequired { recovery_reference, .. } => {
                Self::MediaRecoveryRequired { recovery_reference }
            }
            ReplaceError::Staging(error) => Self::Staging {
                failure: match error {
                    StagingError::CrossDevice => ImportStagingFailure::CrossDevice,
                    StagingError::StagingRefused(gate) => ImportStagingFailure::StagingRefused { gate: gate.into() },
                    StagingError::StagingCorrupt => ImportStagingFailure::StagingCorrupt,
                    StagingError::CallerChangeFailed(error) => {
                        ImportStagingFailure::CallerChangeFailed { error: error.into() }
                    }
                    StagingError::StagingNotFinalised => ImportStagingFailure::StagingNotFinalised,
                    StagingError::LiveWalNotEmpty => ImportStagingFailure::LiveWalNotEmpty,
                    StagingError::Io(error) => ImportStagingFailure::Io {
                        detail: error.to_string(),
                    },
                },
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ImportIssue {
    Package {
        error: PackageFailure,
    },
    Core {
        error: CoreError,
    },
    StaleConfirmation,
    Consumed,
    Cancelled,
    Replacement {
        failure: ImportReplacementFailure,
    },
    SafetyBackup {
        cause: ImportBackupCause,
        publication: ImportBackupPublication,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImportFailure {
    pub issue: ImportIssue,
    /// True even if reopen failed. Candidate stands, publication remains fenced.
    pub candidate_active: bool,
    pub generation: Option<u64>,
    pub backup: Option<ImportBackupReceipt>,
    pub media_recovery_reference: Option<String>,
    pub media: Option<CollectionImportMedia>,
}
impl ImportFailure {
    fn simple(issue: ImportIssue) -> Self {
        Self {
            issue,
            candidate_active: false,
            generation: None,
            backup: None,
            media_recovery_reference: None,
            media: None,
        }
    }
    fn cancelled() -> Self {
        Self::simple(ImportIssue::Cancelled)
    }
}
fn backup_cause(c: ports::BackupFailureCause) -> ImportBackupCause {
    use ports::BackupFailureCause as B;
    let package = match c {
        B::Engine(e) => return ImportBackupCause::Core { error: (*e).into() },
        B::TooNew => PackageError::TooNew,
        B::UnsupportedFormat => PackageError::UnsupportedFormat,
        B::UnsupportedMedia => PackageError::UnsupportedMedia,
        B::SchemaUnsupported { gate } => PackageError::SchemaUnsupported { gate },
        B::Corrupt { detail } => PackageError::Corrupt { detail },
        B::InsufficientSpace { needed, available } => PackageError::InsufficientSpace { needed, available },
        B::DiskFull => PackageError::DiskFull,
        B::SourceNotFinalised => PackageError::SourceNotFinalised,
        B::DestinationExists => PackageError::DestinationExists,
        B::Cancelled => PackageError::Cancelled,
        B::Io { detail } => PackageError::Io { detail },
    };
    ImportBackupCause::Package { error: package.into() }
}
impl From<EntryError> for ImportFailure {
    fn from(e: EntryError) -> Self {
        match e {
            EntryError::AfterBackup { source, backup } => {
                let mut f = Self::from(*source);
                f.backup = Some(backup.into());
                f
            }
            EntryError::MediaOutcome {
                source,
                media,
                retained_reference,
            } => {
                let mut f = Self::from(*source);
                f.media = Some(media.into());
                f.media_recovery_reference = Some(retained_reference);
                f
            }
            EntryError::Cancelled => Self::cancelled(),
            EntryError::CancelledAfterBackup(receipt) => Self {
                backup: Some(receipt.into()),
                ..Self::cancelled()
            },
            EntryError::StaleConfirmation => Self::simple(ImportIssue::StaleConfirmation),
            EntryError::Package(error) => Self::simple(ImportIssue::Package { error: error.into() }),
            EntryError::Engine(EngineError::SafetyBackupFailed(failure)) => {
                let publication = match failure.publication {
                    ports::BackupPublication::Unpublished => ImportBackupPublication::Unpublished,
                    ports::BackupPublication::Uncertain { name, instant } => ImportBackupPublication::Uncertain {
                        name,
                        instant_ms: instant.0,
                    },
                    ports::BackupPublication::Durable(r) => ImportBackupPublication::Durable { receipt: r.into() },
                };
                Self::simple(ImportIssue::SafetyBackup {
                    cause: backup_cause(failure.cause),
                    publication,
                })
            }
            EntryError::Engine(error) => Self::simple(ImportIssue::Core { error: error.into() }),
            EntryError::Replacement { error, backup } => {
                fn media_facts(e: &ReplaceError) -> (Option<String>, Option<CollectionImportMedia>) {
                    match e {
                        ReplaceError::AfterActivation { source, .. } => media_facts(source),
                        ReplaceError::MediaRecoveryRequired {
                            recovery_reference,
                            counts,
                            ..
                        } => (Some(recovery_reference.clone()), Some(counts.clone().into())),
                        _ => (None, None),
                    }
                }
                let (media_recovery_reference, media) = media_facts(&error);
                let (active, generation) = match &error {
                    ReplaceError::AfterActivation { generation, .. } => (true, generation.map(|g| g.0)),
                    _ => (false, None),
                };
                Self {
                    issue: ImportIssue::Replacement { failure: error.into() },
                    candidate_active: active,
                    generation,
                    backup: backup.map(|receipt| (*receipt).into()),
                    media_recovery_reference,
                    media,
                }
            }
        }
    }
}
fn backup_cancelled(cause: &ports::BackupFailureCause) -> bool {
    match cause {
        ports::BackupFailureCause::Cancelled => true,
        ports::BackupFailureCause::Engine(e) => **e == EngineError::Cancelled,
        _ => false,
    }
}
fn job_failure(e: &EntryError) -> EngineError {
    // JobFinished still reports cancellation correctly; the result object keeps
    // the full backup cause and publication evidence for the native flow.
    match e {
        EntryError::AfterBackup { source, .. } => job_failure(source),
        EntryError::MediaOutcome { source, .. } => job_failure(source),
        EntryError::Cancelled => EngineError::Cancelled,
        EntryError::CancelledAfterBackup(_) => EngineError::Cancelled,
        EntryError::Engine(EngineError::SafetyBackupFailed(f)) if backup_cancelled(&f.cause) => EngineError::Cancelled,
        EntryError::Engine(e) => e.clone(),
        _ => EngineError::Internal { message: e.to_string() },
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CollectionInspectionFacts {
    pub notes: u64,
    pub cards: u64,
    pub reviews: u64,
    pub source_schema: i64,
    pub destination: String,
    pub replacing_existing: bool,
    pub source_sha256: Vec<u8>,
    pub media_count: u64,
    pub media_decoded_bytes: u64,
}
struct OwnedInspection {
    inspection: entry::EntryInspection,
    config: HostConfig,
}
#[derive(uniffi::Object)]
pub struct CollectionInspection {
    owned: Mutex<Option<OwnedInspection>>,
    facts: CollectionInspectionFacts,
}
#[uniffi::export]
impl CollectionInspection {
    pub fn facts(&self) -> Result<CollectionInspectionFacts, CoreError> {
        callback::enter("CollectionInspection.facts");
        Ok(self.facts.clone())
    }
    /// Invalidates confirmation and drops only this operation's unpublished files.
    pub fn discard(&self) -> Result<(), CoreError> {
        callback::enter("CollectionInspection.discard");
        let owned = self.owned.lock().unwrap_or_else(PoisonError::into_inner).take();
        drop(owned);
        Ok(())
    }
}
impl CollectionInspection {
    fn new(inspection: entry::EntryInspection, config: HostConfig) -> Arc<Self> {
        let f = inspection.facts();
        let facts = CollectionInspectionFacts {
            notes: f.source.notes,
            cards: f.source.cards,
            reviews: f.source.revlog,
            source_schema: f.source.ver_found,
            destination: f.destination.to_string_lossy().into_owned(),
            replacing_existing: f.replacing_existing,
            source_sha256: f.source_sha256.to_vec(),
            media_count: f.media_count,
            media_decoded_bytes: f.media_decoded_bytes,
        };
        Arc::new(Self {
            owned: Mutex::new(Some(OwnedInspection { inspection, config })),
            facts,
        })
    }
    fn consume(&self) -> Result<OwnedInspection, CoreError> {
        self.owned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| CoreError::InvalidArgument {
                message: "inspection has already been consumed or discarded".into(),
            })
    }
}

struct ResultCell<T>(Mutex<Option<Result<T, ImportFailure>>>);
struct Completion<T>(Arc<ResultCell<T>>);
impl<T> Completion<T> {
    fn finish(self, value: Result<T, ImportFailure>) {
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
    }
}
impl<T> Drop for Completion<T> {
    fn drop(&mut self) {
        let mut value = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        if value.is_none() {
            *value = Some(Err(if std::thread::panicking() {
                ImportFailure::simple(ImportIssue::Core {
                    error: CoreError::Internal {
                        message: "entry job panicked".into(),
                    },
                })
            } else {
                ImportFailure::cancelled()
            }));
        }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CollectionInspectOutcome {
    Pending,
    Ready { inspection: Arc<CollectionInspection> },
    Failed { failure: ImportFailure },
}
#[derive(uniffi::Object)]
pub struct CollectionInspectResult {
    cell: Arc<ResultCell<Arc<CollectionInspection>>>,
}
#[uniffi::export]
impl CollectionInspectResult {
    pub fn outcome(&self) -> Result<CollectionInspectOutcome, CoreError> {
        callback::enter("CollectionInspectResult.outcome");
        Ok(match &*self.cell.0.lock().unwrap_or_else(PoisonError::into_inner) {
            None => CollectionInspectOutcome::Pending,
            Some(Ok(inspection)) => CollectionInspectOutcome::Ready {
                inspection: inspection.clone(),
            },
            Some(Err(failure)) => CollectionInspectOutcome::Failed {
                failure: failure.clone(),
            },
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CollectionActivation {
    pub generation: u64,
    pub backup: Option<ImportBackupReceipt>,
    pub media: CollectionImportMedia,
    pub media_retention_reference: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CollectionImportMedia {
    pub incoming: u64,
    pub decoded_bytes: u64,
    pub reused: u64,
    pub added: u64,
    pub trashed: u64,
}
impl From<meguri_storage::media::ImportMediaCounts> for CollectionImportMedia {
    fn from(m: meguri_storage::media::ImportMediaCounts) -> Self {
        Self {
            incoming: m.incoming,
            decoded_bytes: m.decoded_bytes,
            reused: m.reused,
            added: m.added,
            trashed: m.trashed,
        }
    }
}
impl From<entry::Activation> for CollectionActivation {
    fn from(a: entry::Activation) -> Self {
        Self {
            generation: a.generation.0,
            backup: a.backup.map(Into::into),
            media: a.media.into(),
            media_retention_reference: a.media_retention_reference,
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CollectionActivationOutcome {
    Pending,
    Activated { activation: CollectionActivation },
    Failed { failure: ImportFailure },
}
#[derive(uniffi::Object)]
pub struct CollectionActivationResult {
    cell: Arc<ResultCell<CollectionActivation>>,
}
#[uniffi::export]
impl CollectionActivationResult {
    pub fn outcome(&self) -> Result<CollectionActivationOutcome, CoreError> {
        callback::enter("CollectionActivationResult.outcome");
        Ok(match &*self.cell.0.lock().unwrap_or_else(PoisonError::into_inner) {
            None => CollectionActivationOutcome::Pending,
            Some(Ok(a)) => CollectionActivationOutcome::Activated { activation: a.clone() },
            Some(Err(failure)) => CollectionActivationOutcome::Failed {
                failure: failure.clone(),
            },
        })
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CollectionInspectJob {
    pub job: JobHandle,
    pub result: Arc<CollectionInspectResult>,
}
#[derive(Clone, uniffi::Record)]
pub struct CollectionActivationJob {
    pub job: JobHandle,
    pub result: Arc<CollectionActivationResult>,
}

/// Source path remains caller-stabilized for the complete terminal job. This
/// short call never opens the source or performs parsing/provider work itself.
#[uniffi::export]
pub fn start_collection_inspection(
    source_path: String,
    location: CollectionLocation,
    clock: Arc<dyn HostClock>,
    disk_space: Arc<dyn DiskSpaceProvider>,
    random_seed: u64,
) -> Result<CollectionInspectJob, CoreError> {
    callback::enter("start_collection_inspection");
    let manual_transfer_directory = location.manual_transfer_directory.clone();
    let mut config = HostConfig::new(
        location.collection_directory.into(),
        location.backups_directory.into(),
        location.app_group_snapshot.into(),
        Arc::new(ForeignClock(clock)),
        Arc::new(SeededRandom::new(random_seed)),
        Arc::new(ForeignSpace(disk_space)),
    );
    config.manual_transfer_directory = manual_transfer_directory.into();
    let engine = runtime::engine()?.clone();
    let result = Arc::new(CollectionInspectResult {
        cell: Arc::new(ResultCell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    let worker_engine = engine.clone();
    let job = engine
        .jobs()
        .start("copy and inspect collection", engine.generation(), None, move |job| {
            let inspected = entry::inspect_entry(
                &worker_engine,
                &PathBuf::from(source_path),
                &config.entry_inputs(),
                job.cancel.clone(),
                &job.progress,
            )
            .and_then(public_entry_media_guard);
            let status = inspected.as_ref().map(|_| ()).map_err(job_failure);
            completion.finish(
                inspected
                    .map(|i| CollectionInspection::new(i, config))
                    .map_err(Into::into),
            );
            status
        });
    Ok(CollectionInspectJob {
        job: jobs::wrap(job),
        result,
    })
}
// Temporary app-entry boundary only. Root removes this single guard after
// backend verification and the native confirmation/result mapping are complete.
// Rust package and host entry paths deliberately exercise the full backend.
fn public_entry_media_guard(i: entry::EntryInspection) -> Result<entry::EntryInspection, EntryError> {
    if i.facts().media_count != 0 {
        return Err(EntryError::Package(PackageError::UnsupportedMedia));
    }
    Ok(i)
}
#[uniffi::export]
pub fn start_collection_activation(
    inspection: Arc<CollectionInspection>,
) -> Result<CollectionActivationJob, CoreError> {
    callback::enter("start_collection_activation");
    let relay = crate::progress::CollectionEntryRelay::new()?;
    let progress = meguri_engine::ProgressSink::new();
    progress.set_listener(relay.clone());
    let worker_progress = progress.clone();
    let owned = inspection.consume()?;
    let initial = owned.inspection.is_initial();
    let reservation = if initial { Some(runtime::reserve_open()?) } else { None };
    let host = if initial { None } else { Some(runtime::host()?) };
    let result = Arc::new(CollectionActivationResult {
        cell: Arc::new(ResultCell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    let worker_relay = relay.clone();
    let job = runtime::engine()?.start_job(JobKind::ColpkgImport, move |job| {
        let drain = worker_relay.drain_guard();
        let progress = worker_progress;
        let slot = job.slot().expect("ColpkgImport owns long-operation slot");
        let outcome = match reservation {
            Some(reservation) => {
                reservation.import_initial(owned.config, owned.inspection, slot, &job.cancel, &progress)
            }
            None => owned.inspection.activate_ready(
                slot,
                host.as_ref().expect("ready host").backup().as_ref(),
                &job.cancel,
                &progress,
            ),
        };
        let status = outcome.as_ref().map(|_| ()).map_err(job_failure);
        drop(drain);
        completion.finish(outcome.map(Into::into).map_err(Into::into));
        status
    })?;
    let mut job = jobs::wrap(job);
    job.progress = crate::progress::JobProgress::wrap_collection_entry(job.id, progress, relay);
    Ok(CollectionActivationJob { job, result })
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_engine::{CancelToken, EngineConfig, ProgressSink};
    use meguri_model::{clock::FixedClock, time::EpochMillis};
    use meguri_storage::{atomic_file::Purpose, space::StatvfsSpace};
    #[test]
    fn opaque_inspection_is_single_owned_and_discard_releases_only_its_files() {
        let root = tempfile::tempdir().unwrap();
        let collection = root.path().join("collection");
        std::fs::create_dir(&collection).unwrap();
        let config = HostConfig::new(
            collection.clone(),
            root.path().join("backups"),
            root.path().join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, meguri_model::clock::UtcOffset::UTC)),
            Arc::new(SeededRandom::new(42)),
            Arc::new(StatvfsSpace),
        );
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/fixtures/storage/schema-18.anki2");
        let source = root.path().join("input.colpkg");
        meguri_package::write_collection_package_from_standalone(
            &fixture,
            &source,
            &meguri_package::CollectionPackageWriteContext {
                collection_directory: &collection,
                purpose: Purpose::Export,
                finalisation: meguri_package::Finalisation::ExclusiveCreate,
                clock: config.clock.as_ref(),
                random: config.random.as_ref(),
                space: config.disk_space_provider.as_ref(),
                is_cancelled: &|| false,
            },
        )
        .unwrap();
        let process = meguri_host::ProcessCore::new(EngineConfig::default()).unwrap();
        let inspected = entry::inspect_entry(
            process.engine(),
            &source,
            &config.entry_inputs(),
            CancelToken::new(),
            &ProgressSink::new(),
        )
        .unwrap();
        let object = CollectionInspection::new(inspected, config);
        let alias = object.clone();
        let owned = object.consume().unwrap();
        assert!(alias.consume().is_err());
        alias.discard().unwrap();
        assert_eq!(std::fs::read_dir(&collection).unwrap().count(), 2);
        drop(owned);
        assert_eq!(std::fs::read_dir(&collection).unwrap().count(), 0);
        assert!(source.is_file());
    }
    #[test]
    fn importer_mapping_keeps_backup_cause_publication_and_cancellation() {
        let receipt = ports::BackupReceipt {
            name: "recoverable.colpkg".into(),
            size_bytes: 42,
            instant: EpochMillis(7),
            snapshot_modified: EpochMillis(6),
        };
        for publication in [
            ports::BackupPublication::Unpublished,
            ports::BackupPublication::Uncertain {
                name: receipt.name.clone(),
                instant: receipt.instant,
            },
            ports::BackupPublication::Durable(receipt.clone()),
        ] {
            let error = EntryError::Engine(EngineError::SafetyBackupFailed(Box::new(ports::SafetyBackupFailure {
                cause: ports::BackupFailureCause::Cancelled,
                publication: publication.clone(),
            })));
            assert_eq!(job_failure(&error), EngineError::Cancelled);
            let failure = ImportFailure::from(error);
            assert!(!failure.candidate_active);
            let ImportIssue::SafetyBackup {
                cause,
                publication: mapped,
            } = failure.issue
            else {
                panic!("typed safety failure")
            };
            assert_eq!(
                cause,
                ImportBackupCause::Package {
                    error: PackageFailure::Cancelled
                }
            );
            match (publication, mapped) {
                (ports::BackupPublication::Unpublished, ImportBackupPublication::Unpublished) => {}
                (
                    ports::BackupPublication::Uncertain { name, instant },
                    ImportBackupPublication::Uncertain {
                        name: actual,
                        instant_ms,
                    },
                ) => {
                    assert_eq!(actual, name);
                    assert_eq!(instant_ms, instant.0);
                }
                (ports::BackupPublication::Durable(r), ImportBackupPublication::Durable { receipt }) => {
                    assert_eq!(receipt, r.into())
                }
                _ => panic!("publication evidence lost"),
            }
        }
        let failure = ImportFailure::from(EntryError::Replacement {
            error: ReplaceError::AfterActivation {
                generation: Some(meguri_engine::Generation(9)),
                source: Box::new(ReplaceError::Lifecycle(EngineError::Internal {
                    message: "recovery error".into(),
                })),
            },
            backup: Some(Box::new(receipt)),
        });
        assert!(failure.candidate_active);
        assert_eq!(failure.generation, Some(9));
        assert!(failure.backup.is_some());
    }
    #[test]
    fn queued_cancel_completion_is_terminal_before_job_finished_delivery() {
        let cell = Arc::new(ResultCell::<CollectionActivation>(Mutex::new(None)));
        drop(Completion(cell.clone()));
        assert_eq!(*cell.0.lock().unwrap(), Some(Err(ImportFailure::cancelled())));
    }
    #[test]
    fn media_recovery_mapping_keeps_counts_reference_and_commit_side() {
        for committed in [false, true] {
            let counts = meguri_storage::media::ImportMediaCounts {
                incoming: 5,
                decoded_bytes: 123,
                reused: 2,
                added: 3,
                trashed: 1,
            };
            let error = ReplaceError::MediaRecoveryRequired {
                recovery_reference: "r6-synthetic".into(),
                counts: counts.clone(),
                source: Box::new(ReplaceError::Lifecycle(EngineError::Media(
                    meguri_storage::media::MediaError::DiskFull,
                ))),
            };
            let error = if committed {
                ReplaceError::AfterActivation {
                    generation: None,
                    source: Box::new(error),
                }
            } else {
                error
            };
            let mapped = ImportFailure::from(EntryError::Replacement { error, backup: None });
            assert_eq!(mapped.candidate_active, committed);
            assert_eq!(mapped.media_recovery_reference.as_deref(), Some("r6-synthetic"));
            assert_eq!(mapped.media, Some(counts.into()));
            assert!(matches!(
                mapped.issue,
                ImportIssue::Replacement {
                    failure: ImportReplacementFailure::MediaRecoveryRequired { .. }
                }
            ));
        }
    }
}
