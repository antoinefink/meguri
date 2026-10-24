//! Native adapter only: the host and sync crates own protocol and replacement.
//! Short calls retain typed ownership; all disk/network work runs in bounded jobs.
use crate::{
    callback,
    collection::{CollectionLocation, DiskSpaceProvider, ForeignClock, ForeignSpace, HostClock},
    error::CoreError,
    jobs::{self, JobHandle},
    runtime,
};
use meguri_host::{HostConfig, ManualTransferTask};
use meguri_model::rng::SeededRandom;
use meguri_sync::{
    Endpoint, HostKey, JobOptions, Method, ServerKind,
    oneway::{Confirmation, MediaConfirmation, MediaPreview, Preview, TransferAccess, TransferError},
    policy::{self, Operation},
};
use std::sync::{Arc, Mutex, PoisonError};

fn invalid() -> CoreError {
    CoreError::InvalidArgument {
        message: "Manual transfer input or ownership is invalid".into(),
    }
}
fn endpoint(text: &str) -> Result<Endpoint, CoreError> {
    Endpoint::parse(text).map_err(|_| invalid())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferDirection {
    Import,
    Export,
}
impl From<policy::Direction> for TransferDirection {
    fn from(v: policy::Direction) -> Self {
        match v {
            policy::Direction::Import => Self::Import,
            policy::Direction::Export => Self::Export,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferScope {
    CollectionOnly,
    CollectionAndMedia,
}
impl From<TransferScope> for policy::Scope {
    fn from(v: TransferScope) -> Self {
        match v {
            TransferScope::CollectionOnly => Self::CollectionOnly,
            TransferScope::CollectionAndMedia => Self::CollectionAndMedia,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferMediaStage {
    AwaitingConsent,
    Declined,
    Running,
    Cancelled,
    Interrupted,
    Failed,
    Complete,
}
impl From<policy::MediaStage> for TransferMediaStage {
    fn from(v: policy::MediaStage) -> Self {
        match v {
            policy::MediaStage::AwaitingConsent => Self::AwaitingConsent,
            policy::MediaStage::Declined => Self::Declined,
            policy::MediaStage::Running => Self::Running,
            policy::MediaStage::Cancelled => Self::Cancelled,
            policy::MediaStage::Interrupted => Self::Interrupted,
            policy::MediaStage::Failed => Self::Failed,
            policy::MediaStage::Complete => Self::Complete,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferMediaCounters {
    pub uploaded_files: u64,
    pub uploaded_deletions: u64,
    pub downloaded_files: u64,
    pub downloaded_deletions: u64,
    pub checked: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
    pub resynced: bool,
    pub oversized: Vec<String>,
    pub empty: Vec<String>,
    pub awaiting: u64,
}
impl From<&policy::MediaCounters> for TransferMediaCounters {
    fn from(v: &policy::MediaCounters) -> Self {
        Self {
            uploaded_files: v.uploaded_files,
            uploaded_deletions: v.uploaded_deletions,
            downloaded_files: v.downloaded_files,
            downloaded_deletions: v.downloaded_deletions,
            checked: v.checked,
            bytes_downloaded: v.bytes_downloaded,
            bytes_uploaded: v.bytes_uploaded,
            resynced: v.resynced,
            oversized: v.oversized.clone(),
            empty: v.empty.clone(),
            awaiting: v.awaiting,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferMediaOutcome {
    pub stage: TransferMediaStage,
    pub counters: TransferMediaCounters,
    pub remote_outcome_unknown: bool,
    pub exclusions_verified: bool,
    pub failure_kind: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferMediaPhase {
    Checking,
    Listing,
    Downloading,
    Uploading,
    Confirming,
    Resyncing,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferMediaSnapshot {
    pub phase: TransferMediaPhase,
    pub counters: TransferMediaCounters,
    pub listed: u64,
    pub download_files: u64,
    pub download_received: u64,
    pub published_files: u64,
    pub files_left: u64,
    pub estimate_seconds: Option<f64>,
}
impl From<&meguri_sync::media::MediaSnapshot> for TransferMediaSnapshot {
    fn from(v: &meguri_sync::media::MediaSnapshot) -> Self {
        use meguri_sync::media::MediaPhase as P;
        let t = &v.totals;
        Self {
            phase: match v.phase {
                P::Checking => TransferMediaPhase::Checking,
                P::Listing => TransferMediaPhase::Listing,
                P::Downloading => TransferMediaPhase::Downloading,
                P::Uploading => TransferMediaPhase::Uploading,
                P::Confirming => TransferMediaPhase::Confirming,
                P::Resyncing => TransferMediaPhase::Resyncing,
            },
            counters: TransferMediaCounters {
                uploaded_files: t.uploaded_files,
                uploaded_deletions: t.uploaded_deletions,
                downloaded_files: t.downloaded_files,
                downloaded_deletions: t.downloaded_deletions,
                checked: t.checked,
                bytes_downloaded: t.bytes_downloaded,
                bytes_uploaded: t.bytes_uploaded,
                resynced: t.resynced,
                oversized: t.oversized.clone(),
                empty: t.empty.clone(),
                awaiting: t.awaiting,
            },
            listed: v.listed,
            download_files: v.download_files,
            download_received: v.download_received,
            published_files: v.published_files,
            files_left: v.files_left,
            estimate_seconds: v.estimate.map(|e| e.as_secs_f64()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferPhase {
    Preparing,
    Preview,
    ConfirmedIntent,
    Transferring,
    RemoteAccepted,
    Activating,
    CollectionComplete,
    MediaIncomplete,
    Superseded,
    Complete,
    Cancelled,
    Failed,
    NeedsDecision,
}
impl From<policy::Phase> for TransferPhase {
    fn from(v: policy::Phase) -> Self {
        match v {
            policy::Phase::Preparing => Self::Preparing,
            policy::Phase::Preview => Self::Preview,
            policy::Phase::ConfirmedIntent => Self::ConfirmedIntent,
            policy::Phase::Transferring => Self::Transferring,
            policy::Phase::RemoteAccepted => Self::RemoteAccepted,
            policy::Phase::Activating => Self::Activating,
            policy::Phase::CollectionComplete => Self::CollectionComplete,
            policy::Phase::MediaIncomplete => Self::MediaIncomplete,
            policy::Phase::Superseded => Self::Superseded,
            policy::Phase::Complete => Self::Complete,
            policy::Phase::Cancelled => Self::Cancelled,
            policy::Phase::Failed => Self::Failed,
            policy::Phase::NeedsDecision => Self::NeedsDecision,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferAcceptance {
    NotSent,
    PossiblyAccepted,
    ExactOk,
    Refused,
}
impl From<policy::Acceptance> for TransferAcceptance {
    fn from(v: policy::Acceptance) -> Self {
        match v {
            policy::Acceptance::NotSent => Self::NotSent,
            policy::Acceptance::PossiblyAccepted => Self::PossiblyAccepted,
            policy::Acceptance::ExactOk => Self::ExactOk,
            policy::Acceptance::Refused => Self::Refused,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferBoundary {
    LocalChanged,
    RemoteChanged,
    IdentityChanged,
    ArtifactChanged,
    RemoteCompletionUnknown,
    ActivationInterrupted,
    MediaProviderUnavailable,
    PreparationInterrupted,
    Cancelled,
    Interrupted,
    Storage,
    Server,
    InvalidDownload,
}
impl From<policy::Boundary> for TransferBoundary {
    fn from(v: policy::Boundary) -> Self {
        match v {
            policy::Boundary::LocalChanged => Self::LocalChanged,
            policy::Boundary::RemoteChanged => Self::RemoteChanged,
            policy::Boundary::IdentityChanged => Self::IdentityChanged,
            policy::Boundary::ArtifactChanged => Self::ArtifactChanged,
            policy::Boundary::RemoteCompletionUnknown => Self::RemoteCompletionUnknown,
            policy::Boundary::ActivationInterrupted => Self::ActivationInterrupted,
            policy::Boundary::MediaProviderUnavailable => Self::MediaProviderUnavailable,
            policy::Boundary::PreparationInterrupted => Self::PreparationInterrupted,
            policy::Boundary::Cancelled => Self::Cancelled,
            policy::Boundary::Interrupted => Self::Interrupted,
            policy::Boundary::Storage => Self::Storage,
            policy::Boundary::Server => Self::Server,
            policy::Boundary::InvalidDownload => Self::InvalidDownload,
        }
    }
}
impl From<TransferDirection> for policy::Direction {
    fn from(v: TransferDirection) -> Self {
        match v {
            TransferDirection::Import => Self::Import,
            TransferDirection::Export => Self::Export,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransferIssue {
    Busy,
    Stale,
    Artifact,
    Cancelled,
    Interrupted,
    SessionActive,
    Unavailable,
    DiskFull,
    SafetyBackup,
    InvalidDownload,
    Storage,
    Server,
    NeedsDecision,
    MediaIncomplete,
    Cooldown,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferRedirectDiagnostic {
    pub destination: Option<String>,
    /// Stable RedirectRefusal id; native presentation allowlists known ids.
    pub reason: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferFailure {
    pub issue: TransferIssue,
    pub boundary: Option<TransferBoundary>,
    pub generation: Option<u64>,
    pub candidate_active: bool,
    pub next_eligible_ms: Option<i64>,
    /// Safe stable category only; never raw server bodies/details.
    pub server_kind: Option<String>,
    /// Static qualified method name only; never error prose.
    pub request_method: Option<String>,
    pub redirect: Option<TransferRedirectDiagnostic>,
}
impl From<TransferError> for TransferFailure {
    fn from(e: TransferError) -> Self {
        let mut f = Self {
            issue: TransferIssue::Unavailable,
            boundary: None,
            generation: None,
            candidate_active: false,
            next_eligible_ms: None,
            server_kind: None,
            request_method: None,
            redirect: None,
        };
        f.issue = match e {
            TransferError::Policy(policy::PolicyError::Overlap(_)) | TransferError::Busy => TransferIssue::Busy,
            TransferError::Policy(policy::PolicyError::Stale) => TransferIssue::Stale,
            TransferError::Policy(_) | TransferError::Storage => TransferIssue::Storage,
            TransferError::Stale(b) => {
                f.boundary = Some(b.into());
                TransferIssue::Stale
            }
            TransferError::NeedsDecision(b) => {
                f.boundary = Some(b.into());
                TransferIssue::NeedsDecision
            }
            TransferError::Artifact => TransferIssue::Artifact,
            TransferError::Cancelled => TransferIssue::Cancelled,
            TransferError::Interrupted => TransferIssue::Interrupted,
            TransferError::SessionActive => TransferIssue::SessionActive,
            TransferError::Unavailable => TransferIssue::Unavailable,
            TransferError::DiskFull { .. } => TransferIssue::DiskFull,
            TransferError::SafetyBackup => TransferIssue::SafetyBackup,
            TransferError::InvalidDownload(_) => TransferIssue::InvalidDownload,
            TransferError::Server(e) => {
                f.server_kind = Some(e.kind.id().into());
                f.request_method = e.method.map(Method::qualified_name).map(str::to_owned);
                if matches!(e.kind, meguri_sync::ErrorKind::RedirectRefused { .. }) {
                    f.redirect = e.redirect.map(|redirect| TransferRedirectDiagnostic {
                        destination: redirect.destination,
                        reason: redirect.reason.as_str().into(),
                    });
                }
                TransferIssue::Server
            }
            TransferError::Replacement { activated, generation } => {
                f.candidate_active = activated;
                f.generation = generation;
                TransferIssue::NeedsDecision
            }
            TransferError::MediaIncomplete { generation, .. } => {
                f.candidate_active = generation.is_some();
                f.generation = generation;
                TransferIssue::MediaIncomplete
            }
        };
        f
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferCounts {
    pub cards: u64,
    pub notes: u64,
    pub reviews: u64,
    pub pending_reviews: u64,
    pub pending_rows: Vec<u64>,
}
impl From<&policy::LocalFacts> for TransferCounts {
    fn from(v: &policy::LocalFacts) -> Self {
        Self {
            cards: v.cards,
            notes: v.notes,
            reviews: v.reviews,
            pending_reviews: v.pending_reviews,
            pending_rows: v.pending_rows.to_vec(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferCopy {
    pub name: String,
    pub length: u64,
    pub sha256: Vec<u8>,
    pub verified_package: bool,
    pub instant_ms: Option<i64>,
    pub server: bool,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferObservation {
    pub modified_ms: i64,
    pub schema_modified_ms: i64,
    pub revision: i32,
    pub media_revision: i32,
}
impl From<policy::RemoteFacts> for TransferObservation {
    fn from(v: policy::RemoteFacts) -> Self {
        Self {
            modified_ms: v.modified,
            schema_modified_ms: v.schema_modified,
            revision: v.usn,
            media_revision: v.media_usn,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TransferRecord {
    pub id: String,
    pub configured_endpoint: String,
    pub target_endpoint: String,
    pub direction: TransferDirection,
    pub phase: TransferPhase,
    pub acceptance: TransferAcceptance,
    pub boundary: Option<TransferBoundary>,
    pub local: Option<TransferCounts>,
    pub remote_has_no_cards: Option<bool>,
    pub remote: Option<TransferObservation>,
    pub served_counts: Option<TransferCounts>,
    pub prepared_ms: i64,
    pub generation: Option<u64>,
    pub collection_complete: bool,
    pub media_selected: bool,
    pub revision: u64,
    pub media: Option<TransferMediaOutcome>,
    pub copies: Vec<TransferCopy>,
    pub next_eligible_ms: Option<i64>,
}
impl From<&Operation> for TransferRecord {
    fn from(v: &Operation) -> Self {
        let mut copies: Vec<_> = v
            .safety_backups
            .iter()
            .map(|a| TransferCopy {
                name: a.name.clone(),
                length: a.identity.length,
                sha256: a.identity.sha256.to_vec(),
                verified_package: a.name.ends_with(".colpkg"),
                instant_ms: None,
                server: false,
            })
            .collect();
        if let Some(c) = &v.server_copy {
            copies.push(TransferCopy {
                name: c.artifact.name.clone(),
                length: c.artifact.identity.length,
                sha256: c.artifact.identity.sha256.to_vec(),
                verified_package: c.kind == policy::CopyKind::VerifiedPackage,
                instant_ms: Some(c.published_at),
                server: true,
            });
        }
        Self {
            id: v.id.as_str().into(),
            configured_endpoint: v.configured_endpoint.clone(),
            target_endpoint: v.target_endpoint.clone(),
            direction: v.direction.into(),
            phase: v.phase.into(),
            acceptance: v.acceptance.into(),
            boundary: v.boundary.map(Into::into),
            local: v.recovery_local.as_ref().or(v.local.as_ref()).map(Into::into),
            remote_has_no_cards: v.remote.map(|r| r.empty),
            remote: v.remote.map(Into::into),
            served_counts: v
                .server_copy
                .as_ref()
                .and_then(|c| c.served_facts.as_ref())
                .map(Into::into),
            prepared_ms: v.prepared_at,
            generation: v.actual_generation,
            collection_complete: v.collection_complete,
            media_selected: v.scope == policy::Scope::CollectionAndMedia,
            revision: v.revision,
            media: (v.scope == policy::Scope::CollectionAndMedia).then(|| TransferMediaOutcome {
                stage: v.media.stage.into(),
                counters: (&v.media.counters).into(),
                remote_outcome_unknown: v.media.remote_outcome_unknown,
                exclusions_verified: v.media.exclusions_verified,
                failure_kind: v.media.failure_kind.clone(),
            }),
            copies,
            // Custom-server receipts retain their raw compatibility field only
            // in history; this DTO projects effective eligibility.
            next_eligible_ms: None,
        }
    }
}
/// Opaque single-flow authority. It contains no access credentials.
#[derive(uniffi::Object)]
pub struct ManualTransferPreview {
    preview: Preview,
    consumed: Mutex<bool>,
}
#[uniffi::export]
impl ManualTransferPreview {
    pub fn record(&self) -> Result<TransferRecord, CoreError> {
        callback::enter("ManualTransferPreview::record");
        Ok((&self.preview.operation).into())
    }
}
impl ManualTransferPreview {
    fn take(&self) -> Result<Confirmation, CoreError> {
        let mut consumed = self.consumed.lock().unwrap_or_else(PoisonError::into_inner);
        if *consumed {
            return Err(invalid());
        }
        *consumed = true;
        Ok(self.preview.confirmation.clone())
    }
}
/// History-owned same-operation category decision, with no foreign id input.
#[derive(uniffi::Object)]
pub struct ManualMediaPreview {
    preview: MediaPreview,
    consumed: Mutex<bool>,
}
#[uniffi::export]
impl ManualMediaPreview {
    pub fn record(&self) -> Result<TransferRecord, CoreError> {
        callback::enter("ManualMediaPreview::record");
        Ok((&self.preview.operation).into())
    }
}
impl ManualMediaPreview {
    fn take(&self) -> Result<MediaConfirmation, CoreError> {
        let mut used = self.consumed.lock().unwrap_or_else(PoisonError::into_inner);
        if *used {
            return Err(invalid());
        }
        *used = true;
        Ok(self.preview.confirmation.clone())
    }
}
#[derive(Clone, uniffi::Record)]
pub struct ManualLogin {
    pub username: String,
    pub host_key: String,
}
#[derive(Clone, uniffi::Enum)]
pub enum ManualTransferValue {
    MediaPreview {
        preview: Arc<ManualMediaPreview>,
    },
    Ready,
    Login {
        credentials: ManualLogin,
    },
    History {
        entries: Vec<Arc<ManualTransferHistoryEntry>>,
    },
    Preview {
        preview: Arc<ManualTransferPreview>,
    },
    Operation {
        record: Box<TransferRecord>,
    },
}
#[derive(Clone, uniffi::Enum)]
pub enum ManualTransferOutcome {
    Pending,
    Value { value: ManualTransferValue },
    Failed { failure: TransferFailure },
}
enum ResultSource {
    Cell(Option<ManualTransferOutcome>),
    Preview(Arc<ManualTransferTask<Preview>>),
    Operation(Arc<ManualTransferTask<Operation>>),
    MediaPreview(ManualTransferTask<MediaPreview>),
}
#[derive(uniffi::Object)]
pub struct ManualTransferResult {
    source: Mutex<ResultSource>,
    preparation: Option<Arc<ManualTransferTask<Preview>>>,
    media_task: Option<Arc<ManualTransferTask<Operation>>>,
    known_operation: Option<policy::OperationId>,
}
#[uniffi::export]
impl ManualTransferResult {
    pub fn media_snapshot(&self) -> Result<Option<TransferMediaSnapshot>, CoreError> {
        callback::enter("ManualTransferResult::media_snapshot");
        Ok(self
            .media_task
            .as_ref()
            .and_then(|t| t.media_snapshot())
            .as_ref()
            .map(Into::into))
    }
    pub fn request_stop(&self, interrupted: bool) -> Result<(), CoreError> {
        callback::enter("ManualTransferResult::request_stop");
        if let Some(task) = &self.media_task {
            task.stop_media(interrupted);
        }
        Ok(())
    }
    pub fn operation_id(&self) -> Result<Option<String>, CoreError> {
        callback::enter("ManualTransferResult::operation_id");
        Ok(self
            .preparation
            .as_ref()
            .and_then(|task| task.operation_id())
            .or_else(|| self.known_operation.clone())
            .map(|id| id.as_str().into()))
    }
    /// Call only after the matching JobFinished. A pre-entry cancellation has
    /// no typed worker result; the native adapter uses its job outcome.
    pub fn outcome(&self) -> Result<ManualTransferOutcome, CoreError> {
        callback::enter("ManualTransferResult::outcome");
        let mut source = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        let outcome = match &*source {
            ResultSource::Cell(value) => return Ok(value.clone().unwrap_or(ManualTransferOutcome::Pending)),
            ResultSource::Preview(task) => task.take_result().map(|v| match v {
                Ok(preview) => ManualTransferOutcome::Value {
                    value: ManualTransferValue::Preview {
                        preview: Arc::new(ManualTransferPreview {
                            preview,
                            consumed: Mutex::new(false),
                        }),
                    },
                },
                Err(e) => ManualTransferOutcome::Failed { failure: e.into() },
            }),
            ResultSource::MediaPreview(task) => task.take_result().map(|v| match v {
                Ok(preview) => ManualTransferOutcome::Value {
                    value: ManualTransferValue::MediaPreview {
                        preview: Arc::new(ManualMediaPreview {
                            preview,
                            consumed: Mutex::new(false),
                        }),
                    },
                },
                Err(e) => ManualTransferOutcome::Failed { failure: e.into() },
            }),
            ResultSource::Operation(task) => task.take_result().map(|v| match v {
                Ok(op) => ManualTransferOutcome::Value {
                    value: ManualTransferValue::Operation {
                        record: Box::new((&op).into()),
                    },
                },
                Err(e) => ManualTransferOutcome::Failed { failure: e.into() },
            }),
        };
        if let Some(outcome) = outcome {
            *source = ResultSource::Cell(Some(outcome.clone()));
            Ok(outcome)
        } else {
            Ok(ManualTransferOutcome::Pending)
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct ManualTransferJob {
    pub job: JobHandle,
    pub result: Arc<ManualTransferResult>,
}
fn preview_job(task: ManualTransferTask<Preview>) -> ManualTransferJob {
    let task = Arc::new(task);
    ManualTransferJob {
        job: jobs::wrap(task.job.clone()),
        result: Arc::new(ManualTransferResult {
            source: Mutex::new(ResultSource::Preview(task.clone())),
            preparation: Some(task),
            media_task: None,
            known_operation: None,
        }),
    }
}
fn media_preview_job(task: ManualTransferTask<MediaPreview>) -> ManualTransferJob {
    let known_operation = task.operation_id();
    ManualTransferJob {
        job: jobs::wrap(task.job.clone()),
        result: Arc::new(ManualTransferResult {
            source: Mutex::new(ResultSource::MediaPreview(task)),
            preparation: None,
            media_task: None,
            known_operation,
        }),
    }
}
fn operation_job(task: ManualTransferTask<Operation>) -> ManualTransferJob {
    let known_operation = task.operation_id();
    let task = Arc::new(task);
    ManualTransferJob {
        job: jobs::wrap(task.job.clone()),
        result: Arc::new(ManualTransferResult {
            source: Mutex::new(ResultSource::Operation(task.clone())),
            preparation: None,
            media_task: Some(task),
            known_operation,
        }),
    }
}
fn worker(
    name: &str,
    work: impl FnOnce(&meguri_engine::JobContext) -> Result<ManualTransferValue, TransferError> + Send + 'static,
) -> Result<ManualTransferJob, CoreError> {
    let engine = runtime::engine()?;
    let result = Arc::new(ManualTransferResult {
        source: Mutex::new(ResultSource::Cell(None)),
        preparation: None,
        media_task: None,
        known_operation: None,
    });
    let output = result.clone();
    let job = engine.jobs().start(name, engine.generation(), None, move |ctx| {
        let value = work(ctx);
        let status = value
            .as_ref()
            .map(|_| ())
            .map_err(|e| meguri_sync::oneway::worker_error(e.clone()));
        *output.source.lock().unwrap_or_else(PoisonError::into_inner) = ResultSource::Cell(Some(match value {
            Ok(value) => ManualTransferOutcome::Value { value },
            Err(error) => ManualTransferOutcome::Failed { failure: error.into() },
        }));
        status
    });
    Ok(ManualTransferJob {
        job: jobs::wrap(job),
        result,
    })
}
/// Frontend-owned platform inputs; no endpoint or credentials are retained here.
#[derive(uniffi::Object)]
pub struct ManualTransferSession {
    config: HostConfig,
}
#[uniffi::export]
impl ManualTransferSession {
    #[uniffi::constructor]
    pub fn new(
        location: CollectionLocation,
        clock: Arc<dyn HostClock>,
        disk_space: Arc<dyn DiskSpaceProvider>,
        random_seed: u64,
    ) -> Result<Arc<Self>, CoreError> {
        callback::enter("ManualTransferSession::new");
        let root = std::path::PathBuf::from(location.manual_transfer_directory);
        if !root.is_absolute() {
            return Err(invalid());
        }
        let mut config = HostConfig::new(
            location.collection_directory.into(),
            location.backups_directory.into(),
            location.app_group_snapshot.into(),
            Arc::new(ForeignClock(clock)),
            Arc::new(SeededRandom::new(random_seed)),
            Arc::new(ForeignSpace(disk_space)),
        );
        config.manual_transfer_directory = root;
        Ok(Arc::new(Self { config }))
    }
    /// Offline setup is explicit and independent from login. Reserve the same
    /// process owner used by Files initial import; never make a second engine.
    pub fn start_setup(&self) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_setup");
        let config = self.config.clone();
        worker("prepare manual transfer host", move |ctx| {
            ctx.cancel.check()?;
            if runtime::host().is_err() {
                runtime::reserve_open()
                    .map_err(|_| TransferError::Busy)?
                    .open_manual(config)
                    .map_err(|_| TransferError::Unavailable)?;
            }
            Ok(ManualTransferValue::Ready)
        })
    }
    pub fn start_login(
        &self,
        configured: String,
        username: String,
        password: String,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_login");
        let base = endpoint(&configured)?;
        let random = self.config.random.clone();
        worker("manual server login", move |ctx| {
            let options = JobOptions::new(base, ServerKind::SelfHosted).cancel(ctx.cancel.clone());
            let key = meguri_sync::login_blocking(
                runtime::engine().map_err(|_| TransferError::Unavailable)?.threads(),
                &options,
                random.as_ref(),
                &username,
                &password,
            )
            .map_err(|e| {
                TransferError::from(meguri_sync::JobError::from_transport(
                    e,
                    Some(meguri_sync::Method::HostKey),
                    ServerKind::SelfHosted,
                ))
            })?;
            Ok(ManualTransferValue::Login {
                credentials: ManualLogin {
                    username: username.trim().into(),
                    host_key: key.as_str().into(),
                },
            })
        })
    }
    /// Offline and usable with no host, no login and no server connection.
    pub fn start_history(&self) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_history");
        let root = self.config.manual_transfer_directory.clone();
        // Read history without requiring an open collection. Reconcile a verified
        // successor only after replacement and media work have stopped.
        let host = match runtime::host() {
            Ok(host) => Some(host),
            Err(CoreError::Unavailable { .. }) => None,
            Err(error) => return Err(error),
        };
        worker("read manual transfer history", move |_| {
            if let Some(host) = host
                && host.engine().slot_holder().is_none()
                && !host
                    .engine()
                    .jobs()
                    .running()
                    .iter()
                    .any(|job| job.name == meguri_engine::JobKind::MediaSync.name())
            {
                host.reconcile_manual_transfer_replacement()?;
            }
            let entries = policy::TransferPolicy::new(&root)
                .history()?
                .into_iter()
                .map(|operation| Arc::new(ManualTransferHistoryEntry { operation }))
                .collect();
            Ok(ManualTransferValue::History { entries })
        })
    }
    pub fn start_prepare(
        &self,
        configured: String,
        username: String,
        host_key: String,
        direction: TransferDirection,
        scope: TransferScope,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_prepare");
        let base = endpoint(&configured)?;
        let access = TransferAccess::new(
            base.clone(),
            &username,
            HostKey::new(host_key),
            JobOptions::new(base, ServerKind::SelfHosted),
        )
        .map_err(|_| invalid())?;
        Ok(preview_job(runtime::host()?.prepare_manual_transfer(
            access,
            direction.into(),
            scope.into(),
        )?))
    }
    pub fn start_confirm(
        &self,
        configured: String,
        username: String,
        host_key: String,
        preview: Arc<ManualTransferPreview>,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_confirm");
        let base = endpoint(&configured)?;
        let access = TransferAccess::new(
            base.clone(),
            &username,
            HostKey::new(host_key),
            JobOptions::new(base, ServerKind::SelfHosted),
        )
        .map_err(|_| invalid())?;
        let host = runtime::host()?;
        Ok(operation_job(host.confirm_manual_transfer(access, preview.take()?)?))
    }
    pub fn start_media_preview(
        &self,
        configured: String,
        username: String,
        host_key: String,
        entry: Arc<ManualTransferHistoryEntry>,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_media_preview");
        let base = endpoint(&configured)?;
        let access = TransferAccess::new(
            base.clone(),
            &username,
            HostKey::new(host_key),
            JobOptions::new(base, ServerKind::SelfHosted),
        )
        .map_err(|_| invalid())?;
        Ok(media_preview_job(
            runtime::host()?.prepare_manual_media(entry.operation.id.clone(), access)?,
        ))
    }
    /// Also used by explicit Resume: this entry point cannot dispatch a
    /// collection request, upload or replacement.
    pub fn start_media(
        &self,
        configured: String,
        username: String,
        host_key: String,
        preview: Arc<ManualMediaPreview>,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_media");
        let base = endpoint(&configured)?;
        let access = TransferAccess::new(
            base.clone(),
            &username,
            HostKey::new(host_key),
            JobOptions::new(base, ServerKind::SelfHosted),
        )
        .map_err(|_| invalid())?;
        Ok(operation_job(
            runtime::host()?.run_manual_media(access, preview.take()?)?,
        ))
    }
    /// Dismiss only this prepared media decision; no transport is constructed.
    pub fn start_cancel_prepared_media(
        &self,
        preview: Arc<ManualMediaPreview>,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_cancel_prepared_media");
        Ok(operation_job(
            runtime::host()?.cancel_prepared_manual_media(preview.take()?)?,
        ))
    }
    pub fn start_invalidate_media_owner(&self) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_invalidate_media_owner");
        worker("invalidate manual media owner", move |_| {
            if let Ok(host) = runtime::host() {
                host.invalidate_manual_media_owner()?;
            }
            Ok(ManualTransferValue::Ready)
        })
    }
    pub fn start_recovery_preview(
        &self,
        record: Arc<ManualTransferHistoryEntry>,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_recovery_preview");
        Ok(preview_job(
            runtime::host()?.prepare_manual_transfer_recovery(record.operation.id.clone())?,
        ))
    }
    /// Copy a history-owned backup, never the live collection or an invented
    /// export artifact. Native holds destination coordination until terminal.
    pub fn start_save_copy(
        &self,
        entry: Arc<ManualTransferHistoryEntry>,
        index: u32,
        destination: String,
        final_destination: String,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_save_copy");
        let config = self.config.clone();
        worker("save retained manual transfer copy", move |ctx| {
            save_copy(
                &config,
                &entry.operation,
                index as usize,
                std::path::Path::new(&destination),
                std::path::Path::new(&final_destination),
                &ctx.cancel,
            )?;
            Ok(ManualTransferValue::Ready)
        })
    }
    pub fn start_cancel_history(
        &self,
        entry: Arc<ManualTransferHistoryEntry>,
        interrupted: bool,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_cancel_history");
        let host = runtime::host()?;
        let id = entry.operation.id.clone();
        worker("cancel interrupted manual preparation", move |_| {
            Ok(ManualTransferValue::Operation {
                record: Box::new((&host.cancel_prepared_manual_transfer(&id, interrupted)?).into()),
            })
        })
    }
    pub fn start_cancel_preview(
        &self,
        preview: Arc<ManualTransferPreview>,
        interrupted: bool,
    ) -> Result<ManualTransferJob, CoreError> {
        callback::enter("ManualTransferSession::start_cancel_preview");
        let id = preview.preview.operation.id.clone();
        let host = runtime::host()?;
        preview.take()?;
        worker("cancel prepared manual transfer", move |_| {
            Ok(ManualTransferValue::Operation {
                record: Box::new((&host.cancel_prepared_manual_transfer(&id, interrupted)?).into()),
            })
        })
    }
}
// Historical authority carries the actual backend id, never a caller-made id.
#[derive(uniffi::Object)]
pub struct ManualTransferHistoryEntry {
    operation: Operation,
}
#[uniffi::export]
impl ManualTransferHistoryEntry {
    pub fn record(&self) -> Result<TransferRecord, CoreError> {
        callback::enter("ManualTransferHistoryEntry::record");
        Ok((&self.operation).into())
    }
}
#[uniffi::export]
pub fn normalize_manual_endpoint(text: String) -> Result<String, CoreError> {
    callback::enter("normalize_manual_endpoint");
    Ok(endpoint(&text)?.as_str().into())
}

fn save_copy(
    config: &HostConfig,
    op: &Operation,
    index: usize,
    destination: &std::path::Path,
    final_destination: &std::path::Path,
    cancel: &meguri_engine::CancelToken,
) -> Result<(), TransferError> {
    use crate::collection_export::{alias, copy_bounded, digest, resolved};
    use meguri_storage::atomic_file::{AtomicFile, Purpose};
    use std::{
        fs::{self, File},
        os::unix::fs::MetadataExt,
        path::Component,
    };
    let (artifact, root) = if index < op.safety_backups.len() {
        let safety = &op.safety_backups[index];
        if let Some(original) = op.original.as_ref().filter(|a| a.identity == safety.identity) {
            (original, config.manual_transfer_directory.join(op.id.as_str()))
        } else {
            (safety, config.backups_directory.clone())
        }
    } else if index == op.safety_backups.len() {
        (
            &op.server_copy.as_ref().ok_or(TransferError::Artifact)?.artifact,
            config.manual_transfer_directory.join(op.id.as_str()),
        )
    } else {
        return Err(TransferError::Artifact);
    };
    let valid = |name: &str| !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']);
    if !valid(&artifact.name) || !valid(op.id.as_str()) {
        return Err(TransferError::Artifact);
    }
    // Refuse redirected parent components, not just the leaf. Private retained
    // roots cannot be swapped into paths supplied by the native file picker.
    let mut checked = std::path::PathBuf::new();
    for part in root.components() {
        match part {
            Component::RootDir | Component::Normal(_) => checked.push(part.as_os_str()),
            _ => return Err(TransferError::Artifact),
        }
        if !fs::symlink_metadata(&checked)?.file_type().is_dir() {
            return Err(TransferError::Artifact);
        }
    }
    let source = root.join(&artifact.name);
    let metadata = fs::symlink_metadata(&source)?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(TransferError::Artifact);
    }
    let expected = (artifact.identity.length, artifact.identity.sha256);
    let validate = || -> Result<(), TransferError> {
        for target in [destination, final_destination] {
            let target = resolved(target).ok_or(TransferError::Artifact)?;
            if alias(&source, &target) {
                return Err(TransferError::Artifact);
            }
            for protected in [
                &config.collection_directory,
                &config.backups_directory,
                &config.manual_transfer_directory,
            ] {
                let protected = resolved(protected).ok_or(TransferError::Artifact)?;
                if target.starts_with(&protected) || protected.starts_with(&target) {
                    return Err(TransferError::Artifact);
                }
            }
        }
        if digest(&source, cancel)? != expected {
            return Err(TransferError::Artifact);
        }
        Ok(())
    };
    cancel.check()?;
    validate()?;
    meguri_storage::space::require_space(
        config.disk_space_provider.as_ref(),
        destination.parent().ok_or(TransferError::Artifact)?,
        expected.0,
    )?;
    let mut output = AtomicFile::new(destination, Purpose::Export)?;
    let mut input = File::open(&source)?;
    let opened = input.metadata()?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(TransferError::Artifact);
    }
    copy_bounded(&mut input, output.file_mut(), expected.0, cancel)?;
    output.publish_new(|path| {
        let checked = (|| -> Result<(), TransferError> {
            validate()?;
            if digest(path, cancel)? != expected {
                return Err(TransferError::Artifact);
            }
            cancel.check()?;
            Ok(())
        })();
        checked.map_err(|_| meguri_storage::StorageError::Io {
            detail: "Retained copy validation failed".into(),
        })
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Clock;
    impl HostClock for Clock {
        fn now_ms(&self) -> i64 {
            1_894_701_600_000
        }
        fn offset_from_utc(&self, _: i64) -> i32 {
            0
        }
        fn zone_id(&self) -> String {
            "UTC".into()
        }
    }
    struct Space;
    impl DiskSpaceProvider for Space {
        fn available_bytes(&self, _: String) -> crate::collection::DiskCapacity {
            crate::collection::DiskCapacity::Available { bytes: u64::MAX }
        }
    }
    fn session(root: &std::path::Path, profile: &str) -> Arc<ManualTransferSession> {
        ManualTransferSession::new(
            CollectionLocation {
                collection_directory: root.join(profile).join("collection").to_string_lossy().into(),
                backups_directory: root.join(profile).join("backups").to_string_lossy().into(),
                app_group_snapshot: root.join(profile).join("widget").to_string_lossy().into(),
                manual_transfer_directory: root.join("installation-transfers").to_string_lossy().into(),
            },
            Arc::new(Clock),
            Arc::new(Space),
            819,
        )
        .unwrap()
    }
    #[test]
    fn manual_endpoint_adapter_uses_prefix_normalization_and_rejects_secrets() {
        assert_eq!(
            normalize_manual_endpoint(" https://Example.test/prefix ".into()).unwrap(),
            "https://example.test/prefix/"
        );
        for input in [
            "https://user:password@example.test/",
            "https://example.test/?key=secret",
            "file:///tmp/collection",
            "https://example.test/#secret",
        ] {
            assert!(normalize_manual_endpoint(input.into()).is_err());
        }
    }
    #[test]
    fn manual_session_construction_is_offline_and_root_is_installation_wide() {
        let root = tempfile::tempdir().unwrap();
        let a = session(root.path(), "profile-a");
        let b = session(root.path(), "another-parent/profile-b");
        assert_eq!(a.config.manual_transfer_directory, b.config.manual_transfer_directory);
        assert_ne!(a.config.collection_directory, b.config.collection_directory);
        assert!(
            !a.config.manual_transfer_directory.exists(),
            "short construction must not touch the filesystem"
        );
        assert!(!a.config.collection_directory.exists());
    }
    #[test]
    fn manual_result_pending_and_repeated_failure_preserve_typed_authority() {
        let result = ManualTransferResult {
            source: Mutex::new(ResultSource::Cell(None)),
            preparation: None,
            media_task: None,
            known_operation: None,
        };
        assert!(matches!(result.outcome().unwrap(), ManualTransferOutcome::Pending));
        *result.source.lock().unwrap() = ResultSource::Cell(Some(ManualTransferOutcome::Failed {
            failure: TransferError::NeedsDecision(policy::Boundary::RemoteCompletionUnknown).into(),
        }));
        for _ in 0..2 {
            let ManualTransferOutcome::Failed { failure } = result.outcome().unwrap() else {
                panic!("typed failure lost")
            };
            assert_eq!(failure.issue, TransferIssue::NeedsDecision);
            assert_eq!(failure.boundary, Some(TransferBoundary::RemoteCompletionUnknown));
            assert!(!failure.candidate_active);
        }
        let failure: TransferFailure = TransferError::Replacement {
            activated: true,
            generation: Some(37),
        }
        .into();
        assert!(failure.candidate_active);
        assert_eq!(failure.generation, Some(37));
    }
    #[test]
    fn manual_remote_observation_keeps_post_copy_times_and_change_markers() {
        let observed: TransferObservation = policy::RemoteFacts {
            modified: 1_900_000_001_234,
            schema_modified: 1_899_000_005_678,
            usn: 19,
            empty: true,
            media_usn: 4,
        }
        .into();
        assert_eq!(observed.modified_ms, 1_900_000_001_234);
        assert_eq!(observed.schema_modified_ms, 1_899_000_005_678);
        assert_eq!(observed.revision, 19);
        assert_eq!(observed.media_revision, 4);
    }
    #[test]
    fn manual_legacy_cooldown_error_is_failure_without_an_effective_wait() {
        let failure: TransferFailure = TransferError::Policy(policy::PolicyError::Cooldown {
            next_eligible_at: 12345,
        })
        .into();
        assert_eq!(failure.issue, TransferIssue::Storage);
        assert_eq!(failure.next_eligible_ms, None);
        assert!(!failure.candidate_active);
    }
    #[test]
    fn manual_failure_preserves_only_the_structured_request_method() {
        for (method, expected) in [
            (Some(Method::Meta), Some("meta")),
            (Some(Method::Download), Some("download")),
            (None, None),
        ] {
            let failure: TransferFailure = TransferError::Server(meguri_sync::JobError::new(
                meguri_sync::ErrorKind::BadResponse,
                method,
                "SECRET detail mentions upload",
            ))
            .into();
            assert_eq!(failure.request_method.as_deref(), expected);
        }
        let failure: TransferFailure = TransferError::Unavailable.into();
        assert_eq!(failure.request_method, None);
    }
    #[test]
    fn manual_failure_exports_only_structured_redirect_facts() {
        use meguri_sync::{EndpointRule, JobError, RedirectError, RedirectRefusal, ServerKind, SyncError};
        for (reason, id) in [
            (RedirectRefusal::NotMeta, "not_meta"),
            (RedirectRefusal::NotPermanent, "not_308"),
            (RedirectRefusal::MissingLocation, "missing_location"),
            (RedirectRefusal::Target(EndpointRule::Empty), "empty"),
            (RedirectRefusal::Target(EndpointRule::Unparseable), "unparseable"),
            (
                RedirectRefusal::Target(EndpointRule::UnsupportedScheme),
                "unsupported_scheme",
            ),
            (RedirectRefusal::Target(EndpointRule::MissingHost), "missing_host"),
            (RedirectRefusal::Target(EndpointRule::Credentials), "credentials"),
            (
                RedirectRefusal::Target(EndpointRule::QueryOrFragment),
                "query_or_fragment",
            ),
            (RedirectRefusal::LostHttps, "https_lost"),
            (RedirectRefusal::Host, "host"),
            (RedirectRefusal::Repeated, "repeated"),
            (RedirectRefusal::NotResendable, "not_resendable"),
        ] {
            let error = JobError::from_transport(
                SyncError::Redirect(RedirectError {
                    code: 308,
                    location: Some("https://USER:SECRET@example.test/prefix/?token=SECRET#SECRET".into()),
                    refusal: reason,
                }),
                None,
                ServerKind::SelfHosted,
            );
            let failure: TransferFailure = TransferError::Server(error).into();
            assert_eq!(failure.server_kind.as_deref(), Some("redirect_refused"));
            let diagnostic = failure.redirect.unwrap();
            assert_eq!(diagnostic.destination.as_deref(), Some("https://example.test/prefix/"));
            assert_eq!(diagnostic.reason, id);
        }
        for kind in [
            meguri_sync::ErrorKind::ServerMessage {
                text: "SECRET server body".into(),
            },
            meguri_sync::ErrorKind::RedirectRefused { code: 308 },
            meguri_sync::ErrorKind::BadResponse,
        ] {
            let failure: TransferFailure = TransferError::Server(JobError::new(kind, None, "SECRET raw detail")).into();
            assert!(failure.redirect.is_none());
            assert!(!failure.server_kind.unwrap().contains("SECRET"));
        }
        let failure: TransferFailure = TransferError::Unavailable.into();
        assert!(failure.redirect.is_none());
    }
    #[test]
    fn manual_custom_history_projects_no_wait_and_preserves_old_and_immediate_receipts() {
        for direction in ["Import", "Export"] {
            for next_eligible_at in [100, 3_600_100] {
                // Direct v1-shaped history input, including an old future
                // deadline. Projection must never rewrite that raw receipt.
                let operation: Operation = serde_json::from_value(serde_json::json!({
                    "id": "0000000000000001-000000000000000c",
                    "account": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    "configured_endpoint": "http://127.0.0.1/",
                    "target_endpoint": "http://127.0.0.1/",
                    "direction": direction,
                    "scope": "CollectionOnly",
                    "phase": "Complete",
                    "acceptance": "ExactOk",
                    "boundary": null,
                    "guard": null,
                    "recovery_guard": null,
                    "recovery_local": null,
                    "local": null,
                    "remote": null,
                    "candidate": null,
                    "activation_identity": null,
                    "original": null,
                    "safety_backups": [],
                    "server_copy": null,
                    "prepared_at": 99,
                    "actual_generation": 7,
                    "collection_complete": true,
                    "receipt": {"completed_at": 100, "next_eligible_at": next_eligible_at},
                    "revision": 4
                }))
                .unwrap();
                let raw = serde_json::to_value(&operation).unwrap();
                let record = TransferRecord::from(&operation);
                assert_eq!(record.next_eligible_ms, None);
                assert_eq!(record.phase, TransferPhase::Complete);
                assert!(record.collection_complete);
                assert_eq!(record.generation, Some(7));
                assert_eq!(serde_json::to_value(&operation).unwrap(), raw);
                assert_eq!(operation.receipt.as_ref().unwrap().completed_at, 100);
                assert_eq!(operation.receipt.as_ref().unwrap().next_eligible_at, next_eligible_at);
            }
        }
    }
}

#[cfg(test)]
mod media_adapter_tests {
    use super::*;
    #[test]
    fn media_snapshot_preserves_stage_bytes_exclusions_and_window_estimate() {
        let value = meguri_sync::media::MediaSnapshot {
            phase: meguri_sync::media::MediaPhase::Downloading,
            totals: meguri_sync::media::MediaSyncResult {
                downloaded_files: 7,
                uploaded_files: 2,
                downloaded_deletions: 1,
                bytes_downloaded: 900,
                bytes_uploaded: 200,
                oversized: vec!["large.bin".into()],
                empty: vec!["empty.bin".into()],
                awaiting: 3,
                ..Default::default()
            },
            files_left: 3,
            estimate: Some(std::time::Duration::from_millis(1500)),
            ..Default::default()
        };
        let mapped = TransferMediaSnapshot::from(&value);
        assert_eq!(mapped.phase, TransferMediaPhase::Downloading);
        assert_eq!(mapped.counters.downloaded_files, 7);
        assert_eq!(mapped.counters.uploaded_files, 2);
        assert_eq!(mapped.counters.downloaded_deletions, 1);
        assert_eq!(mapped.counters.bytes_downloaded, 900);
        assert_eq!(mapped.counters.bytes_uploaded, 200);
        assert_eq!(mapped.counters.oversized, ["large.bin"]);
        assert_eq!(mapped.counters.empty, ["empty.bin"]);
        assert_eq!(mapped.counters.awaiting, 3);
        assert_eq!(mapped.files_left, 3);
        assert_eq!(mapped.estimate_seconds, Some(1.5));
    }
}
