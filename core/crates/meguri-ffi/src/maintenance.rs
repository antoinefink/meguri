//! U133a: value outcomes owned by the caller, over the existing host and jobs.
use crate::{
    CoreError, JobHandle, callback,
    collection_entry::{ImportBackupPublication, ImportBackupReceipt},
    jobs, runtime,
};
use meguri_engine::{
    EngineError,
    maintenance::{BackupError, MaintenanceTrigger as Trigger},
    ports,
    replace::{ReplaceError, StagingError},
};
use meguri_package::{CollectionPackageWriteError as PackageError, restore::RestoreError};
use meguri_storage::{ReaderRole, StorageError};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaintenanceError {
    Cancelled,
    InsufficientSpace { needed: u64, available: u64 },
    Corrupt,
    TooNew,
    SchemaUnsupported,
    UnsupportedFormat,
    InvalidIds,
    SafetyBackupFailed,
    Busy { running_op: String },
    CollectionUnavailable,
    CollectionCorrupt,
    SessionActive,
    InvalidArgument { message: String },
    DiskFull,
    Io { detail: String },
    DestinationExists,
}
impl From<StorageError> for MaintenanceError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::InsufficientSpace { required, available } => Self::InsufficientSpace {
                needed: required,
                available,
            },
            StorageError::DestinationExists => Self::DestinationExists,
            StorageError::Sqlite { code: 11 | 26, .. } => Self::Corrupt,
            StorageError::Sqlite { code: 13, .. } => Self::DiskFull,
            StorageError::IdsExhausted { .. } => Self::InvalidIds,
            other => Self::Io {
                detail: other.to_string(),
            },
        }
    }
}
impl From<meguri_storage::OpenError> for MaintenanceError {
    fn from(e: meguri_storage::OpenError) -> Self {
        use meguri_storage::OpenError as O;
        match e {
            O::TooNew { .. } => Self::TooNew,
            O::TooOld { .. } | O::V1Scheduler => Self::SchemaUnsupported,
            O::Corrupt { .. } | O::NotACollection { .. } => Self::Corrupt,
            O::OpenInAnotherProgram => Self::Busy {
                running_op: "another program".into(),
            },
            O::NotFound => Self::CollectionUnavailable,
            other => Self::Io {
                detail: other.to_string(),
            },
        }
    }
}
impl From<EngineError> for MaintenanceError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::Cancelled => Self::Cancelled,
            EngineError::Busy { running_op } => Self::Busy { running_op },
            EngineError::CollectionUnavailable { .. } => Self::CollectionUnavailable,
            EngineError::CollectionCorrupt => Self::CollectionCorrupt,
            EngineError::SessionActive => Self::SessionActive,
            EngineError::InvalidArgument { message } => Self::InvalidArgument { message },
            EngineError::Storage(e) => e.into(),
            EngineError::Open(e) => e.into(),
            EngineError::SafetyBackupFailed(_) => Self::SafetyBackupFailed,
            EngineError::Unsupported { .. } => Self::UnsupportedFormat,
            other => Self::Io {
                detail: other.to_string(),
            },
        }
    }
}
impl From<PackageError> for MaintenanceError {
    fn from(e: PackageError) -> Self {
        match e {
            PackageError::Cancelled => Self::Cancelled,
            PackageError::TooNew => Self::TooNew,
            PackageError::SchemaUnsupported { .. } => Self::SchemaUnsupported,
            PackageError::UnsupportedFormat | PackageError::UnsupportedMedia => Self::UnsupportedFormat,
            PackageError::Corrupt { .. } => Self::Corrupt,
            PackageError::InsufficientSpace { needed, available } => Self::InsufficientSpace { needed, available },
            PackageError::DiskFull => Self::DiskFull,
            PackageError::DestinationExists => Self::DestinationExists,
            PackageError::Io { detail } => Self::Io { detail },
            other => Self::Io {
                detail: other.to_string(),
            },
        }
    }
}
fn backup_cause(c: ports::BackupFailureCause) -> MaintenanceError {
    use ports::BackupFailureCause as B;
    match c {
        B::Engine(e) => (*e).into(),
        B::TooNew => MaintenanceError::TooNew,
        B::SchemaUnsupported { .. } => MaintenanceError::SchemaUnsupported,
        B::UnsupportedFormat | B::UnsupportedMedia => MaintenanceError::UnsupportedFormat,
        B::Corrupt { .. } => MaintenanceError::Corrupt,
        B::InsufficientSpace { needed, available } => MaintenanceError::InsufficientSpace { needed, available },
        B::DiskFull => MaintenanceError::DiskFull,
        B::DestinationExists => MaintenanceError::DestinationExists,
        B::Cancelled => MaintenanceError::Cancelled,
        B::Io { detail } => MaintenanceError::Io { detail },
        B::SourceNotFinalised => MaintenanceError::Io {
            detail: "backup source is not finalised".into(),
        },
    }
}
fn publication(p: ports::BackupPublication) -> ImportBackupPublication {
    match p {
        ports::BackupPublication::Unpublished => ImportBackupPublication::Unpublished,
        ports::BackupPublication::Uncertain { name, instant } => ImportBackupPublication::Uncertain {
            name,
            instant_ms: instant.0,
        },
        ports::BackupPublication::Durable(receipt) => ImportBackupPublication::Durable {
            receipt: receipt.into(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RestoredBackup {
    pub name: String,
    pub safety_backup: Option<String>,
    pub generation: u64,
    pub cloud_publication_pending: bool,
    pub deadline_ms: Option<i64>,
}
impl From<meguri_package::restore::RestoredBackup> for RestoredBackup {
    fn from(r: meguri_package::restore::RestoredBackup) -> Self {
        Self {
            name: r.name,
            safety_backup: r.safety_backup,
            generation: r.generation.0,
            cloud_publication_pending: r.cloud_publication_pending,
            deadline_ms: r.deadline.map(|v| v.0),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MaintenanceFailure {
    pub cause: MaintenanceError,
    pub candidate_active: bool,
    pub generation: Option<u64>,
    pub safety_backup: Option<String>,
    pub restored: Option<RestoredBackup>,
    pub backup_publication: ImportBackupPublication,
}
impl MaintenanceFailure {
    fn simple(cause: MaintenanceError) -> Self {
        Self {
            cause,
            candidate_active: false,
            generation: None,
            safety_backup: None,
            restored: None,
            backup_publication: ImportBackupPublication::Unpublished,
        }
    }
    fn engine(e: EngineError) -> Self {
        match e {
            EngineError::SafetyBackupFailed(f) => Self {
                cause: backup_cause(f.cause),
                backup_publication: publication(f.publication),
                ..Self::simple(MaintenanceError::SafetyBackupFailed)
            },
            e => Self::simple(e.into()),
        }
    }
    fn terminal(&self) -> EngineError {
        match &self.cause {
            MaintenanceError::Cancelled => EngineError::Cancelled,
            MaintenanceError::Busy { running_op } => EngineError::Busy {
                running_op: running_op.clone(),
            },
            MaintenanceError::SessionActive => EngineError::SessionActive,
            _ => EngineError::Internal {
                message: format!("{:?}", self.cause),
            },
        }
    }
}
impl From<BackupError> for MaintenanceFailure {
    fn from(e: BackupError) -> Self {
        match e {
            BackupError::CollectionCorrupt => Self::simple(MaintenanceError::CollectionCorrupt),
            BackupError::Engine(e) => Self::engine(e),
            BackupError::Writer { cause, publication: p } => Self {
                cause: backup_cause(cause),
                backup_publication: publication(p),
                ..Self::simple(MaintenanceError::SafetyBackupFailed)
            },
        }
    }
}
fn replacement(e: ReplaceError) -> MaintenanceFailure {
    match e {
        ReplaceError::AfterActivation { generation, source } => MaintenanceFailure {
            candidate_active: true,
            generation: generation.map(|g| g.0),
            ..replacement(*source)
        },
        ReplaceError::MediaRecoveryRequired { source, .. } => replacement(*source),
        ReplaceError::Lifecycle(e) => MaintenanceFailure::engine(e),
        ReplaceError::NotExclusive => MaintenanceFailure::simple(MaintenanceError::InvalidArgument {
            message: "replacement requires exclusive ownership".into(),
        }),
        ReplaceError::Staging(e) => match e {
            StagingError::StagingRefused(e) => MaintenanceFailure::simple(e.into()),
            StagingError::StagingCorrupt => MaintenanceFailure::simple(MaintenanceError::Corrupt),
            StagingError::CallerChangeFailed(e) => MaintenanceFailure::engine(e.into()),
            StagingError::Io(e) => MaintenanceFailure::simple(e.into()),
            e => MaintenanceFailure::simple(MaintenanceError::Io { detail: e.to_string() }),
        },
    }
}
impl From<RestoreError> for MaintenanceFailure {
    fn from(e: RestoreError) -> Self {
        match e {
            RestoreError::Engine(e) => Self::engine(e),
            RestoreError::Package(e) => Self::simple(e.into()),
            RestoreError::Replacement { error, safety_backup } => Self {
                safety_backup,
                ..replacement(error)
            },
            RestoreError::AfterSafety { error, safety_backup } => Self {
                safety_backup,
                ..Self::from(*error)
            },
            RestoreError::AfterActivation { error, restored } => {
                let restored = RestoredBackup::from(*restored);
                Self {
                    candidate_active: true,
                    generation: Some(restored.generation),
                    safety_backup: restored.safety_backup.clone(),
                    restored: Some(restored),
                    ..Self::engine(error)
                }
            }
        }
    }
}

struct Cell<T>(Mutex<Option<Result<T, MaintenanceFailure>>>);
struct Completion<T>(Arc<Cell<T>>);
impl<T> Completion<T> {
    fn finish(self, value: Result<T, MaintenanceFailure>) -> Result<(), EngineError> {
        let terminal = value.as_ref().map(|_| ()).map_err(MaintenanceFailure::terminal);
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        terminal
    }
}
impl<T> Drop for Completion<T> {
    fn drop(&mut self) {
        let mut value = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        if value.is_none() {
            *value = Some(Err(MaintenanceFailure::simple(if std::thread::panicking() {
                MaintenanceError::Io {
                    detail: "maintenance worker panicked".into(),
                }
            } else {
                MaintenanceError::Cancelled
            })));
        }
    }
}
macro_rules! owned_result {
    ($result:ident, $outcome:ident, $job:ident, $value:ty, $variant:ident, $field:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
        pub enum $outcome {
            Pending,
            $variant { $field: $value },
            Failed { failure: MaintenanceFailure },
        }
        #[derive(uniffi::Object)]
        pub struct $result {
            cell: Arc<Cell<$value>>,
        }
        #[uniffi::export]
        impl $result {
            pub fn outcome(&self) -> Result<$outcome, CoreError> {
                callback::enter(concat!(stringify!($result), ".outcome"));
                Ok(
                    match &*self.cell.0.lock().unwrap_or_else(PoisonError::into_inner) {
                        None => $outcome::Pending,
                        Some(Ok(value)) => $outcome::$variant {
                            $field: Clone::clone(value),
                        },
                        Some(Err(failure)) => $outcome::Failed {
                            failure: failure.clone(),
                        },
                    },
                )
            }
        }
        #[derive(Clone, uniffi::Record)]
        pub struct $job {
            pub job: JobHandle,
            pub result: Arc<$result>,
        }
    };
}
owned_result!(
    CreateBackupResult,
    CreateBackupOutcome,
    CreateBackupJob,
    ImportBackupReceipt,
    Created,
    backup
);
owned_result!(
    RestoreBackupResult,
    RestoreBackupOutcome,
    RestoreBackupJob,
    RestoredBackup,
    Restored,
    restored
);
owned_result!(
    DueMaintenanceResult,
    DueMaintenanceOutcome,
    DueMaintenanceJob,
    Option<i64>,
    Completed,
    deadline_ms
);

#[uniffi::export]
pub fn start_create_backup() -> Result<CreateBackupJob, CoreError> {
    callback::enter("start_create_backup");
    let engine = runtime::engine()?.clone();
    let result = Arc::new(CreateBackupResult {
        cell: Arc::new(Cell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    let worker = engine.clone();
    let job = engine
        .jobs()
        .start("create backup", engine.generation(), None, move |job| {
            job.progress
                .set_phase(meguri_engine::ProgressPhase::Named("Writing backup"));
            let outcome = worker
                .back_up_now(&job.cancel)
                .map(|v| v.receipt.into())
                .map_err(Into::into);
            job.progress
                .set_phase(meguri_engine::ProgressPhase::Named("Backup finished"));
            completion.finish(outcome)
        });
    Ok(CreateBackupJob {
        job: jobs::wrap(job),
        result,
    })
}
#[uniffi::export]
pub fn start_restore_backup(name: String) -> Result<RestoreBackupJob, CoreError> {
    callback::enter("start_restore_backup");
    let host = runtime::host()?;
    let engine = runtime::engine()?;
    let result = Arc::new(RestoreBackupResult {
        cell: Arc::new(Cell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    // Reuse activation's unlocked callback delivery, including terminal drain.
    let relay = crate::progress::CollectionEntryRelay::new()?;
    let progress = meguri_engine::ProgressSink::new();
    progress.set_listener(relay.clone());
    let worker_progress = progress.clone();
    let worker_relay = relay.clone();
    // The backend owns Restore slot admission; JobKind::Restore would take it twice.
    let job = engine
        .jobs()
        .start("restore backup", engine.generation(), None, move |job| {
            let drain = worker_relay.drain_guard();
            let outcome = host
                .restore_backup(&name, &job.cancel, &worker_progress)
                .map(Into::into)
                .map_err(Into::into);
            drop(drain);
            completion.finish(outcome)
        });
    let mut job = jobs::wrap(job);
    job.progress = crate::progress::JobProgress::wrap_collection_entry(job.id, progress, relay);
    Ok(RestoreBackupJob { job, result })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaintenanceTrigger {
    Opened,
    Background,
    Quit,
    BackgroundProcessing,
    Deadline,
}
impl From<MaintenanceTrigger> for Trigger {
    fn from(t: MaintenanceTrigger) -> Self {
        match t {
            MaintenanceTrigger::Opened => Self::Opened,
            MaintenanceTrigger::Background => Self::Background,
            MaintenanceTrigger::Quit => Self::Quit,
            MaintenanceTrigger::BackgroundProcessing => Self::BackgroundProcessing,
            MaintenanceTrigger::Deadline => Self::Deadline,
        }
    }
}
#[uniffi::export]
pub fn start_run_due_maintenance(trigger: MaintenanceTrigger) -> Result<DueMaintenanceJob, CoreError> {
    callback::enter("start_run_due_maintenance");
    let engine = runtime::engine()?.clone();
    let result = Arc::new(DueMaintenanceResult {
        cell: Arc::new(Cell(Mutex::new(None))),
    });
    let completion = Completion(result.cell.clone());
    let worker = engine.clone();
    let job = engine
        .jobs()
        .start("due maintenance", engine.generation(), None, move |job| {
            job.progress
                .set_phase(meguri_engine::ProgressPhase::Named("Evaluating maintenance"));
            let outcome = worker
                .run_due_maintenance_cancellable(trigger.into(), &job.cancel)
                .map(|d| d.map(|v| v.0))
                .map_err(MaintenanceFailure::engine);
            job.progress
                .set_phase(meguri_engine::ProgressPhase::Named("Maintenance finished"));
            completion.finish(outcome)
        });
    Ok(DueMaintenanceJob {
        job: jobs::wrap(job),
        result,
    })
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BackupLimits {
    pub daily: u32,
    pub weekly: u32,
    pub monthly: u32,
    pub minimum_interval_mins: u32,
}
impl From<meguri_engine::backup_limits::BackupLimits> for BackupLimits {
    fn from(v: meguri_engine::backup_limits::BackupLimits) -> Self {
        Self {
            daily: v.daily,
            weekly: v.weekly,
            monthly: v.monthly,
            minimum_interval_mins: v.minimum_interval_mins,
        }
    }
}
#[uniffi::export]
pub fn backup_limits() -> Result<BackupLimits, CoreError> {
    callback::enter("backup_limits");
    short_call(|| {
        let c = runtime::host().map_err(EngineError::from)?.collection()?;
        let reader = c.reader(ReaderRole::Query)?;
        Ok(meguri_engine::backup_limits::read(&*reader)?.into())
    })
}
#[uniffi::export]
pub fn set_backup_limits(limits: BackupLimits) -> Result<bool, CoreError> {
    callback::enter("set_backup_limits");
    short_call(|| {
        let c = runtime::host().map_err(EngineError::from)?.collection()?;
        Ok(meguri_engine::backup_limits::save(
            &c,
            meguri_engine::backup_limits::BackupLimits {
                daily: limits.daily,
                weekly: limits.weekly,
                monthly: limits.monthly,
                minimum_interval_mins: limits.minimum_interval_mins,
            },
        )?
        .value)
    })
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BackupKind {
    Backup,
    ServerCopy,
    ServerCopyUnverified,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BackupWallTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BackupEntry {
    pub name: String,
    pub kind: BackupKind,
    pub wall_time: BackupWallTime,
    pub size_bytes: u64,
}
#[uniffi::export]
pub fn list_backups() -> Result<Vec<BackupEntry>, CoreError> {
    callback::enter("list_backups");
    short_call(|| {
        let (_host, directory) = runtime::backup_listing_inputs().map_err(EngineError::from)?;
        let entries = meguri_package::backup::listing::list(&directory).map_err(StorageError::from)?;
        Ok(entries
            .into_iter()
            .map(|e| {
                use meguri_package::backup::naming::BackupKind as K;
                BackupEntry {
                    name: e.name,
                    size_bytes: e.size_bytes,
                    kind: match e.kind {
                        K::Backup => BackupKind::Backup,
                        K::ServerCopy => BackupKind::ServerCopy,
                        K::ServerCopyUnverified => BackupKind::ServerCopyUnverified,
                    },
                    wall_time: BackupWallTime {
                        year: e.wall_time.year,
                        month: e.wall_time.month,
                        day: e.wall_time.day,
                        hour: e.wall_time.hour,
                        minute: e.wall_time.minute,
                        second: e.wall_time.second,
                    },
                }
            })
            .collect())
    })
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LastBackup {
    pub name: String,
    pub instant_ms: i64,
    pub snapshot_modified_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LastQuickCheck {
    pub instant_ms: i64,
    pub passed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CheckDatabaseProblems {
    pub card_properties: u64,
    pub new_positions_too_high: u64,
    pub card_last_review_time: u64,
    pub cards_without_note: u64,
    pub decks_missing: u64,
    pub field_counts: u64,
    pub duplicate_card_ords: u64,
    pub cards_without_template: u64,
    pub revlog_properties: u64,
    pub notes_invalid_utf8: u64,
    pub future_ids: u64,
    pub notetypes_recovered: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CheckDatabaseResult {
    Corrupt,
    Problems { problems: CheckDatabaseProblems },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LastCheckDatabase {
    pub instant_ms: i64,
    pub result: CheckDatabaseResult,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaintenanceRemedy {
    Restore,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaintenanceStatusField {
    LastBackup,
    LastCheckDatabase,
    LastOptimize,
    LastQuickCheck,
    CollectionBytes,
    MediaBytes,
    TrashBytes,
    Remedies,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MaintenanceStatusFailure {
    pub field: MaintenanceStatusField,
    pub cause: MaintenanceError,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MaintenanceStatus {
    pub last_backup: Option<LastBackup>,
    pub last_check_database: Option<LastCheckDatabase>,
    pub last_optimize_ms: Option<i64>,
    pub last_quick_check: Option<LastQuickCheck>,
    pub collection_bytes: Option<u64>,
    pub media_bytes: Option<u64>,
    pub trash_bytes: Option<u64>,
    pub remedies: Vec<MaintenanceRemedy>,
    pub failures: Vec<MaintenanceStatusFailure>,
}
fn field<T, E: Into<MaintenanceError>>(
    result: Result<T, E>,
    name: MaintenanceStatusField,
    failures: &mut Vec<MaintenanceStatusFailure>,
) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            failures.push(MaintenanceStatusFailure {
                field: name,
                cause: e.into(),
            });
            None
        }
    }
}
impl From<meguri_engine::maintenance::status::MaintenanceStatus> for MaintenanceStatus {
    fn from(s: meguri_engine::maintenance::status::MaintenanceStatus) -> Self {
        use MaintenanceStatusField as F;
        use meguri_engine::device_state::CheckDatabaseResult as R;
        let mut failures = vec![];
        let last_backup = field(s.last_backup, F::LastBackup, &mut failures)
            .flatten()
            .map(|v| LastBackup {
                name: v.name,
                instant_ms: v.instant,
                snapshot_modified_ms: v.snapshot_mod,
            });
        let last_check_database = field(s.last_check_database, F::LastCheckDatabase, &mut failures)
            .flatten()
            .map(|v| LastCheckDatabase {
                instant_ms: v.instant,
                result: match v.result {
                    R::Corrupt => CheckDatabaseResult::Corrupt,
                    R::Problems(p) => CheckDatabaseResult::Problems {
                        problems: CheckDatabaseProblems {
                            card_properties: p.card_properties,
                            new_positions_too_high: p.new_positions_too_high,
                            card_last_review_time: p.card_last_review_time,
                            cards_without_note: p.cards_without_note,
                            decks_missing: p.decks_missing,
                            field_counts: p.field_counts,
                            duplicate_card_ords: p.duplicate_card_ords,
                            cards_without_template: p.cards_without_template,
                            revlog_properties: p.revlog_properties,
                            notes_invalid_utf8: p.notes_invalid_utf8,
                            future_ids: p.future_ids,
                            notetypes_recovered: p.notetypes_recovered,
                        },
                    },
                },
            });
        let last_optimize_ms = field(s.last_optimize, F::LastOptimize, &mut failures)
            .flatten()
            .map(|v| v.0);
        let last_quick_check = field(s.last_quick_check, F::LastQuickCheck, &mut failures)
            .flatten()
            .map(|v| LastQuickCheck {
                instant_ms: v.instant,
                passed: v.passed,
            });
        let collection_bytes = field(s.collection_bytes, F::CollectionBytes, &mut failures);
        let media_bytes = field(s.media_bytes, F::MediaBytes, &mut failures);
        let trash_bytes = field(s.trash_bytes, F::TrashBytes, &mut failures);
        let remedies = field(s.remedies, F::Remedies, &mut failures)
            .into_iter()
            .flatten()
            .map(|v| match v {
                meguri_engine::maintenance::status::Remedy::Restore => MaintenanceRemedy::Restore,
            })
            .collect();
        Self {
            last_backup,
            last_check_database,
            last_optimize_ms,
            last_quick_check,
            collection_bytes,
            media_bytes,
            trash_bytes,
            remedies,
            failures,
        }
    }
}
#[uniffi::export]
pub fn maintenance_status() -> Result<MaintenanceStatus, CoreError> {
    callback::enter("maintenance_status");
    short_call(|| {
        Ok(runtime::engine()
            .map_err(EngineError::from)?
            .maintenance_status()?
            .into())
    })
}
fn short_call<T>(work: impl FnOnce() -> Result<T, EngineError>) -> Result<T, CoreError> {
    work().map_err(|error| CoreError::Maintenance { cause: error.into() })
}
