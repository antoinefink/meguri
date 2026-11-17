//! Connection and own-operation ports. Handles retain core-issued authority;
//! projections contain presentation counts or transport inputs only.
use crate::{CoreError, callback, sync::CloudWorkflow};
use meguri_sync::cloud::{self as c, adoption as a, publication as p, runtime as r};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudConnectionFailure {
    Closed,
    WrongOwner,
    Consumed,
    AuthorityChanged,
    AccountUnavailable,
    AccountMismatch,
    Busy,
    KeyUnavailable,
    EpochIncomplete,
    UnreadableControl,
    NotOffered,
    IncomingStagingUnavailable,
    MediaApplyUnavailable,
    ResetRecoveryUnavailable,
    Tracking,
    Incomplete,
    FileIdentityUnavailable,
}
fn failure(failure: CloudConnectionFailure) -> CoreError {
    CoreError::CloudConnection { failure }
}
pub(crate) fn error(error: r::RuntimeError) -> CoreError {
    use CloudConnectionFailure as F;
    failure(match error {
        r::RuntimeError::Publication(e) => match e {
            p::PublicationError::AccountUnavailable => F::AccountUnavailable,
            p::PublicationError::AccountMismatch => F::AccountMismatch,
            p::PublicationError::AuthorityChanged => F::AuthorityChanged,
            p::PublicationError::Busy => F::Busy,
            p::PublicationError::KeyUnavailable => F::KeyUnavailable,
            p::PublicationError::EpochIncomplete => F::EpochIncomplete,
            p::PublicationError::UnreadableControl => F::UnreadableControl,
            p::PublicationError::NotOffered => F::NotOffered,
            p::PublicationError::IncomingStagingUnavailable => F::IncomingStagingUnavailable,
            p::PublicationError::MediaApplyUnavailable => F::MediaApplyUnavailable,
            p::PublicationError::ResetRecoveryUnavailable => F::ResetRecoveryUnavailable,
        },
        r::RuntimeError::StaleIncarnation => F::AuthorityChanged,
        r::RuntimeError::IdentityUnverified => F::AccountUnavailable,
        r::RuntimeError::Incomplete(_) => F::Incomplete,
        r::RuntimeError::FileIdentityUnavailable => F::FileIdentityUnavailable,
        r::RuntimeError::Tracking(_) => F::Tracking,
        r::RuntimeError::Engine(e) => return e.into(),
    })
}

#[derive(Clone, uniffi::Record)]
pub struct CloudControlRecord {
    pub encrypted: Option<Vec<u8>>,
    pub system_fields: Vec<u8>,
}
impl From<CloudControlRecord> for p::ControlRecord {
    fn from(v: CloudControlRecord) -> Self {
        Self {
            encrypted: v.encrypted,
            system_fields: c::OpaqueBytes(v.system_fields),
        }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudControlRead {
    Found { record: CloudControlRecord },
    Missing,
    ZoneMissing,
    Failed,
}
impl From<CloudControlRead> for p::RecordRead {
    fn from(v: CloudControlRead) -> Self {
        match v {
            CloudControlRead::Found { record } => Self::Found(record.into()),
            CloudControlRead::Missing => Self::Missing,
            CloudControlRead::ZoneMissing => Self::ZoneMissing,
            CloudControlRead::Failed => Self::Failed,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudControlTarget {
    pub zone: String,
    pub name: String,
    pub record_type: String,
}
fn active() -> CloudControlTarget {
    CloudControlTarget {
        zone: "meguri-root".into(),
        name: "active".into(),
        record_type: "MGActive".into(),
    }
}
fn epoch(epoch: c::EpochId) -> CloudControlTarget {
    CloudControlTarget {
        zone: c::adapter::collection_zone(epoch),
        name: "epoch".into(),
        record_type: "MGEpoch".into(),
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudConnectionDecision {
    ConfirmUpload,
    ConfirmAdoption,
    ConfirmResume,
    ChooseCollection,
    CloudUnreadable,
    NoCollection,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudCountKind {
    Note,
    Card,
    Revlog,
    Deck,
    Preset,
    Notetype,
    Tag,
    Config,
    MediaName,
}
impl From<c::entity_codec::Kind> for CloudCountKind {
    fn from(v: c::entity_codec::Kind) -> Self {
        use c::entity_codec::Kind as K;
        match v {
            K::Note => Self::Note,
            K::Card => Self::Card,
            K::Revlog => Self::Revlog,
            K::Deck => Self::Deck,
            K::Preset => Self::Preset,
            K::Notetype => Self::Notetype,
            K::Tag => Self::Tag,
            K::Config => Self::Config,
            K::MediaName => Self::MediaName,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudKindCount {
    pub kind: CloudCountKind,
    pub count: u64,
}
fn counts(v: &std::collections::BTreeMap<c::entity_codec::Kind, u64>) -> Vec<CloudKindCount> {
    v.iter()
        .map(|(k, n)| CloudKindCount {
            kind: (*k).into(),
            count: *n,
        })
        .collect()
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudLocalPublicationCondition {
    Pending,
    Clear,
    LocalOnly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudLocalPendingDecisionReason {
    Restore,
    ColpkgImport,
    ManualImport,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudIsolatedLocalContent {
    ReplacedLocally,
    FileMismatch,
    Unverified,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudLocalBackupKnowledge {
    Unknown,
    NotTakenForExternalReplacement,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudIsolatedLocalInformation {
    pub pending_counts: Vec<CloudKindCount>,
    pub content: CloudIsolatedLocalContent,
    pub backup: CloudLocalBackupKnowledge,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudLocalTrackingInformation {
    /// Scoped ordinary bindings only; excludes salvage owners and other devices.
    pub isolated: Vec<CloudIsolatedLocalInformation>,
    pub unresolved_conflict_copies: u64,
}
/// Values only: no opaque authority, action offer, account or file identifiers.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudLocalPublicationInformation {
    pub generation: u64,
    pub collection_present: bool,
    pub condition: Option<CloudLocalPublicationCondition>,
    pub pending_reason: Option<CloudLocalPendingDecisionReason>,
    pub local_counts: Vec<CloudKindCount>,
    /// Unknown with no local collection, rather than claiming an empty one.
    pub local_empty: Option<bool>,
    pub last_review_ms: Option<i64>,
    pub last_change_ms: Option<i64>,
    pub tracking: Option<CloudLocalTrackingInformation>,
}
impl From<c::local_publication::LocalPublicationInformation> for CloudLocalPublicationInformation {
    fn from(v: c::local_publication::LocalPublicationInformation) -> Self {
        use c::local_publication::*;
        Self {
            generation: v.generation,
            collection_present: v.collection_present,
            condition: v.condition.map(|v| match v {
                LocalPublicationCondition::Pending => CloudLocalPublicationCondition::Pending,
                LocalPublicationCondition::Clear => CloudLocalPublicationCondition::Clear,
                LocalPublicationCondition::LocalOnly => CloudLocalPublicationCondition::LocalOnly,
            }),
            pending_reason: v.pending_reason.map(|v| match v {
                LocalPendingDecisionReason::Restore => CloudLocalPendingDecisionReason::Restore,
                LocalPendingDecisionReason::ColpkgImport => CloudLocalPendingDecisionReason::ColpkgImport,
                LocalPendingDecisionReason::ManualImport => CloudLocalPendingDecisionReason::ManualImport,
            }),
            local_counts: v.local.as_ref().map(|v| counts(&v.counts)).unwrap_or_default(),
            local_empty: v.local.map(|v| v.empty),
            last_review_ms: v.last_review_ms,
            last_change_ms: v.last_change_ms,
            tracking: v.tracking.map(|v| CloudLocalTrackingInformation {
                isolated: v
                    .isolated
                    .into_iter()
                    .map(|v| CloudIsolatedLocalInformation {
                        pending_counts: counts(&v.pending_by_kind),
                        content: match v.content {
                            IsolatedLocalContent::ReplacedLocally => CloudIsolatedLocalContent::ReplacedLocally,
                            IsolatedLocalContent::FileMismatch => CloudIsolatedLocalContent::FileMismatch,
                            IsolatedLocalContent::Unverified => CloudIsolatedLocalContent::Unverified,
                        },
                        backup: match v.backup {
                            LocalBackupKnowledge::Unknown => CloudLocalBackupKnowledge::Unknown,
                            LocalBackupKnowledge::NotTakenForExternalReplacement => {
                                CloudLocalBackupKnowledge::NotTakenForExternalReplacement
                            }
                        },
                    })
                    .collect(),
                unresolved_conflict_copies: v.unresolved_conflict_copies,
            }),
        }
    }
}
pub(crate) fn local_information_for_host(
    host: &meguri_host::CoreHost,
    generation: u64,
) -> Result<CloudLocalPublicationInformation, CoreError> {
    let current = host
        .cloud_connection_generation()
        .map_err(|_| failure(CloudConnectionFailure::Closed))?;
    if current.0 != generation {
        return Err(failure(CloudConnectionFailure::AuthorityChanged));
    }
    let info = host
        .local_publication_information(meguri_engine::Generation(generation))
        .map_err(error)?;
    if host
        .cloud_connection_generation()
        .map_err(|_| failure(CloudConnectionFailure::Closed))?
        .0
        != generation
    {
        return Err(failure(CloudConnectionFailure::AuthorityChanged));
    }
    Ok(info.into())
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudConnectionSummary {
    pub local_collection_present: bool,
    /// The native owner must transition to V14 without a replacement confirmation.
    pub initial_adoption_required: bool,
    pub local_counts: Vec<CloudKindCount>,
    pub local_empty: bool,
    /// Manifest estimates only. The native caller must show this date.
    pub manifest_counts: Option<Vec<CloudKindCount>>,
    pub published_at_ms: Option<i64>,
    pub decision: CloudConnectionDecision,
    pub unpublished_counts: Vec<CloudKindCount>,
    /// Present “Continue syncing this collection” and these unsent counts.
    pub resume_binding_id: Option<Vec<u8>>,
    pub unsent_counts: Vec<CloudKindCount>,
    /// False: own total is unknown; unsent_counts are only known lower bounds.
    pub unsent_counts_exact: bool,
    pub fetched_unverified_count: u64,
    pub held_count: u64,
    pub isolated_unsent_count: u64,
    pub conflict_copy_count: u64,
}
impl From<&p::Preview> for CloudConnectionSummary {
    fn from(v: &p::Preview) -> Self {
        Self {
            local_collection_present: v.native_file.is_some(),
            initial_adoption_required: v.native_file.is_none()
                && v.active_epoch.is_some()
                && v.decision == p::ConnectDecision::NoCollection,
            local_counts: counts(&v.local.counts),
            local_empty: v.local.empty,
            manifest_counts: v.cloud.as_ref().map(|v| counts(&v.counts)),
            published_at_ms: v.published_at_ms,
            decision: match v.decision {
                p::ConnectDecision::NoCollection => CloudConnectionDecision::NoCollection,
                p::ConnectDecision::ConfirmUpload => CloudConnectionDecision::ConfirmUpload,
                p::ConnectDecision::ConfirmAdoption => CloudConnectionDecision::ConfirmAdoption,
                p::ConnectDecision::ConfirmResume => CloudConnectionDecision::ConfirmResume,
                p::ConnectDecision::ChooseCollection => CloudConnectionDecision::ChooseCollection,
                p::ConnectDecision::CloudUnreadable => CloudConnectionDecision::CloudUnreadable,
            },
            unpublished_counts: counts(&v.unpublished_by_kind),
            resume_binding_id: v.resume_binding.map(|b| b.0.to_vec()),
            unsent_counts: counts(&v.unsent_by_kind),
            unsent_counts_exact: v.unsent_counts_exact,
            fetched_unverified_count: v.retained_work.fetched_unverified,
            held_count: v.retained_work.held,
            isolated_unsent_count: v.retained_work.isolated_unsent,
            conflict_copy_count: v.retained_work.conflict_copies,
        }
    }
}
struct Owner {
    host: Arc<meguri_host::CoreHost>,
    core: Arc<r::Runtime>,
}
impl Owner {
    fn current_generation(&self) -> Result<u64, CoreError> {
        Ok(self
            .host
            .cloud_connection_generation()
            .map_err(|_| failure(CloudConnectionFailure::Closed))?
            .0)
    }
    fn check_generation(&self, generation: u64) -> Result<(), CoreError> {
        if self.current_generation()? != generation {
            return Err(failure(CloudConnectionFailure::AuthorityChanged));
        }
        Ok(())
    }
    fn collection(&self) -> Result<meguri_engine::Collection, CoreError> {
        self.host
            .cloud_connection_generation()
            .map_err(|_| failure(CloudConnectionFailure::Closed))?;
        self.host
            .collection()
            .map_err(|_| failure(CloudConnectionFailure::NotOffered))
    }
    fn generation(&self, generation: u64) -> Result<meguri_engine::Collection, CoreError> {
        let c = self.collection()?;
        if c.generation().0 != generation {
            return Err(failure(CloudConnectionFailure::AuthorityChanged));
        }
        Ok(c)
    }
}
#[derive(uniffi::Object)]
pub struct CloudConnection {
    owner: Arc<Owner>,
}
impl CloudConnection {
    pub(crate) fn for_workflow(v: &CloudWorkflow) -> Result<Arc<Self>, CoreError> {
        v.host.cloud_connection_generation()?;
        Ok(Arc::new(Self {
            owner: Arc::new(Owner {
                host: v.host.clone(),
                core: v.core.clone(),
            }),
        }))
    }
    fn owns(&self, owner: &Arc<Owner>) -> Result<(), CoreError> {
        if !Arc::ptr_eq(&self.owner, owner) {
            return Err(failure(CloudConnectionFailure::WrongOwner));
        }
        Ok(())
    }
    fn preview_work(&self, work: p::PreviewWork, generation: u64) -> CloudConnectionPreviewWork {
        let handle = Arc::new(CloudConnectionPreview {
            owner: self.owner.clone(),
            generation,
            work: Mutex::new(Some(work.clone())),
        });
        match work {
            p::PreviewWork::ReadActive { .. } => CloudConnectionPreviewWork::ReadActive {
                preview: handle,
                target: active(),
            },
            p::PreviewWork::ReadEpoch { epoch: id, .. } => CloudConnectionPreviewWork::ReadEpoch {
                preview: handle,
                target: epoch(id),
            },
            p::PreviewWork::Ready(p) => CloudConnectionPreviewWork::Ready {
                preview: handle,
                summary: (&*p).into(),
            },
        }
    }
    fn recovery(&self, id: [u8; 16], adoption: bool, generation: u64) -> Arc<CloudRecoveryOperation> {
        Arc::new(CloudRecoveryOperation {
            owner: self.owner.clone(),
            id,
            adoption,
            generation,
        })
    }
}
#[derive(uniffi::Object)]
pub struct CloudConnectionPreview {
    owner: Arc<Owner>,
    generation: u64,
    work: Mutex<Option<p::PreviewWork>>,
}
impl CloudConnectionPreview {
    fn take(&self) -> Result<p::PreviewWork, CoreError> {
        self.work
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| failure(CloudConnectionFailure::Consumed))
    }
    fn take_adoption(&self) -> Result<p::PreviewWork, CoreError> {
        let mut work = self.work.lock().unwrap_or_else(PoisonError::into_inner);
        // Inspect only owned preview facts. Runtime validates authority after
        // this guard is dropped; an absent adoption offer stays cancellable.
        match work.as_ref() {
            Some(p::PreviewWork::Ready(preview)) if preview.active.is_some() && preview.active_epoch.is_some() => {}
            Some(_) => return Err(failure(CloudConnectionFailure::NotOffered)),
            None => return Err(failure(CloudConnectionFailure::Consumed)),
        }
        work.take().ok_or_else(|| failure(CloudConnectionFailure::Consumed))
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudConnectionPreviewWork {
    ReadActive {
        preview: Arc<CloudConnectionPreview>,
        target: CloudControlTarget,
    },
    ReadEpoch {
        preview: Arc<CloudConnectionPreview>,
        target: CloudControlTarget,
    },
    Ready {
        preview: Arc<CloudConnectionPreview>,
        summary: CloudConnectionSummary,
    },
}
#[derive(uniffi::Object)]
pub struct CloudRecoveryOperation {
    owner: Arc<Owner>,
    id: [u8; 16],
    adoption: bool,
    generation: u64,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudPublicationLocalKind {
    Bind,
    WriteDecision,
    Cancel,
}
#[derive(uniffi::Object)]
pub struct CloudPublicationLocal {
    owner: Arc<Owner>,
    generation: u64,
    work: p::PublicationWork,
}
#[derive(uniffi::Object)]
pub struct CloudPublicationRequest {
    owner: Arc<Owner>,
    request: p::OwnRequest,
}
impl CloudPublicationRequest {
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn selected_event(
        &self,
        reply: CloudPublicationResult,
    ) -> Option<c::selected_observation::PublicationEvent> {
        c::selected_observation::PublicationEvent::from_reply(&self.request, &reply.into())
    }
    #[cfg(test)]
    pub(crate) fn retained_for_test(&self) -> &p::OwnRequest {
        &self.request
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudOwnKind {
    CreateZones,
    SaveMediaRoot,
    SavePublishingEpoch,
    SaveCompleteEpoch,
    ReadOwnEpoch,
    SaveActive,
    ReadResetMediaRoot,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudOwnMetadata {
    pub operation_id: Vec<u8>,
    pub attempt: u64,
    pub kind: CloudOwnKind,
}
impl From<&p::OwnRequest> for CloudOwnMetadata {
    fn from(v: &p::OwnRequest) -> Self {
        Self {
            operation_id: v.op.to_vec(),
            attempt: v.attempt,
            kind: match v.kind {
                p::OwnKind::CreateZones => CloudOwnKind::CreateZones,
                p::OwnKind::SaveMediaRoot => CloudOwnKind::SaveMediaRoot,
                p::OwnKind::SavePublishingEpoch => CloudOwnKind::SavePublishingEpoch,
                p::OwnKind::SaveCompleteEpoch => CloudOwnKind::SaveCompleteEpoch,
                p::OwnKind::ReadOwnEpoch => CloudOwnKind::ReadOwnEpoch,
                p::OwnKind::SaveActive => CloudOwnKind::SaveActive,
                p::OwnKind::ReadResetMediaRoot => CloudOwnKind::ReadResetMediaRoot,
            },
        }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudOwnOperation {
    CreateZones {
        zones: Vec<String>,
    },
    Read {
        target: CloudControlTarget,
    },
    Save {
        target: CloudControlTarget,
        encrypted: Vec<u8>,
        system_fields: Option<Vec<u8>>,
    },
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudPublicationResult {
    ZoneLosses { losses: Vec<crate::sync::CloudZoneLoss> },
    ZonesCreated,
    Saved { record: CloudControlRecord },
    Conflict { record: CloudControlRecord },
    Read { read: CloudControlRead },
    Failed { retry_after_ms: Option<i64> },
    RetryAfter { duration_ms: u64 },
    NotStarted,
}
impl From<CloudPublicationResult> for p::OwnResult {
    fn from(v: CloudPublicationResult) -> Self {
        match v {
            CloudPublicationResult::ZoneLosses { losses } => {
                Self::ZoneLosses(losses.into_iter().map(Into::into).collect())
            }
            CloudPublicationResult::ZonesCreated => Self::ZonesCreated,
            CloudPublicationResult::Saved { record } => Self::Saved(record.into()),
            CloudPublicationResult::Conflict { record } => Self::Conflict(record.into()),
            CloudPublicationResult::Read { read } => Self::Read(read.into()),
            CloudPublicationResult::Failed { retry_after_ms } => Self::Failed { retry_after_ms },
            CloudPublicationResult::RetryAfter { duration_ms } => Self::RetryAfter { duration_ms },
            CloudPublicationResult::NotStarted => Self::NotStarted,
        }
    }
}
/// Synchronous queue admission ONLY. The prepared CKDatabaseOperation is added
/// before returning true. No async work, UI, provider calls or core reentry.
#[uniffi::export(foreign)]
pub trait CloudPublicationQueue: Send + Sync {
    fn enqueue(&self) -> bool;
}
#[derive(uniffi::Object)]
pub struct CloudAdoptionWork {
    owner: Arc<Owner>,
    generation: u64,
    work: a::AdoptionWork,
}
#[cfg(test)]
impl CloudAdoptionWork {
    pub(crate) fn retained_work_for_test(&self) -> &a::AdoptionWork {
        &self.work
    }
    pub(crate) fn operation_id_for_test(&self) -> [u8; 16] {
        self.work.op
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudAdoptionAction {
    ReadActive {
        target: CloudControlTarget,
        expected: CloudControlRecord,
    },
    Read {
        target: CloudControlTarget,
    },
    Fetch {
        zone: String,
        cursor: Option<Vec<u8>>,
        expected_epoch: CloudControlRecord,
    },
    FetchPayload {
        zone: String,
        name: String,
        expected: CloudControlRecord,
    },
    Validate,
    Activate,
}
#[derive(Clone, uniffi::Record)]
pub struct CloudStagingRecord {
    pub name: String,
    pub record_type: String,
    pub record: CloudControlRecord,
    pub payload_file: Option<String>,
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudAdoptionResult {
    Failed,
    TransportFailed {
        code: i64,
        retry_after_duration_ms: Option<u64>,
    },
    Payload {
        record: CloudStagingRecord,
    },
    Read {
        read: CloudControlRead,
    },
    Page {
        records: Vec<CloudStagingRecord>,
        deleted: Vec<String>,
        next: Option<Vec<u8>>,
    },
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudPublicationWork {
    Local {
        handle: Arc<CloudPublicationLocal>,
        kind: CloudPublicationLocalKind,
        operation: Arc<CloudRecoveryOperation>,
    },
    Own {
        request: Arc<CloudPublicationRequest>,
        command: CloudOwnOperation,
        operation: Arc<CloudRecoveryOperation>,
        metadata: CloudOwnMetadata,
    },
    Adoption {
        handle: Arc<CloudAdoptionWork>,
        action: CloudAdoptionAction,
        operation: Arc<CloudRecoveryOperation>,
    },
    Scan {
        handle: Arc<CloudPublicationLocal>,
    },
    SendEntities,
    Idle,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudRecoveryStep {
    Confirmed,
    Bound,
    Zones,
    Sent,
    Complete,
    Cancelling,
    Staging,
    Staged,
    Ready,
}
#[derive(Clone, uniffi::Record)]
pub struct CloudRecoveryStatus {
    pub operation: Arc<CloudRecoveryOperation>,
    pub step: CloudRecoveryStep,
    pub switch_sent: bool,
    pub switch_conflict: bool,
    pub retry_after_ms: Option<i64>,
    pub adoption_transport_wait: bool,
    pub adoption_cancellable: bool,
    /// Live Ready owner with pending activation held by an actual review.
    pub adoption_after_review: bool,
    /// Exact ownerless recovered adoption; Cancel is its only operation.
    pub adoption_recovered_cancel_only: bool,
    pub publication_conflict_cancellable: bool,
    pub publication_cancellable: bool,
}

#[uniffi::export]
impl CloudConnection {
    pub fn local_publication_information(
        &self,
        generation: u64,
    ) -> Result<CloudLocalPublicationInformation, CoreError> {
        callback::enter("CloudConnection.local_publication_information");
        local_information_for_host(&self.owner.host, generation)
    }
    pub fn keep_local_only(
        &self, generation: u64, preview: Option<Arc<CloudConnectionPreview>>,
    ) -> Result<(), CoreError> {
        callback::enter("CloudConnection.keep_local_only");
        self.owner.check_generation(generation)?;
        let ready = if let Some(preview) = preview {
            self.owns(&preview.owner)?;
            if preview.generation != generation { return Err(failure(CloudConnectionFailure::AuthorityChanged)); }
            let p::PreviewWork::Ready(p) = preview.take()? else {
                return Err(failure(CloudConnectionFailure::NotOffered));
            };
            Some(p)
        } else { None };
        self.owner.host.keep_cloud_local_only(meguri_engine::Generation(generation), ready.as_deref()).map_err(error)
    }
    pub fn resume_binding(&self, preview: Arc<CloudConnectionPreview>) -> Result<(), CoreError> {
        callback::enter("CloudConnection.resume_binding");
        self.owns(&preview.owner)?;
        self.owner.check_generation(preview.generation)?;
        let p::PreviewWork::Ready(p) = preview.take()? else {
            return Err(failure(CloudConnectionFailure::NotOffered));
        };
        if p.decision != p::ConnectDecision::ConfirmResume {
            return Err(failure(CloudConnectionFailure::NotOffered));
        }
        self.owner
            .host
            .resume_cloud_binding(&p, meguri_engine::Generation(preview.generation))
            .map_err(error)
    }
    pub fn begin_connection_preview(&self) -> Result<CloudConnectionPreviewWork, CoreError> {
        callback::enter("CloudConnection.begin_connection_preview");
        let generation = self.owner.current_generation()?;
        let work = self.owner.host.begin_cloud_connection_preview();
        #[cfg(feature = "test-hooks")]
        let work = work.inspect_err(|error| {
            let (kind, variant) = error.refusal_tags_for_testing();
            eprintln!("cloudConnection.refused boundary=begin_connection_preview errorKind={kind} variant={variant}");
        });
        let work = work.map_err(error)?;
        self.owner.check_generation(generation)?;
        Ok(self.preview_work(work, generation))
    }
    pub fn connection_active_read(
        &self,
        preview: Arc<CloudConnectionPreview>,
        read: CloudControlRead,
    ) -> Result<CloudConnectionPreviewWork, CoreError> {
        callback::enter("CloudConnection.connection_active_read");
        self.owns(&preview.owner)?;
        self.owner.check_generation(preview.generation)?;
        let p::PreviewWork::ReadActive { id } = preview.take()? else {
            return Err(failure(CloudConnectionFailure::NotOffered));
        };
        Ok(self.preview_work(
            self.owner.core.connection_active_read(id, read.into()).map_err(error)?,
            preview.generation,
        ))
    }
    pub fn connection_epoch_read(
        &self,
        preview: Arc<CloudConnectionPreview>,
        read: CloudControlRead,
    ) -> Result<CloudConnectionPreviewWork, CoreError> {
        callback::enter("CloudConnection.connection_epoch_read");
        self.owns(&preview.owner)?;
        self.owner.check_generation(preview.generation)?;
        let p::PreviewWork::ReadEpoch { id, .. } = preview.take()? else {
            return Err(failure(CloudConnectionFailure::NotOffered));
        };
        Ok(self.preview_work(
            self.owner.core.connection_epoch_read(id, read.into()).map_err(error)?,
            preview.generation,
        ))
    }
    pub fn cancel_preview(&self, preview: Arc<CloudConnectionPreview>) -> Result<(), CoreError> {
        callback::enter("CloudConnection.cancel_preview");
        self.owns(&preview.owner)?;
        self.owner.check_generation(preview.generation)?;
        let id = match preview.take()? {
            p::PreviewWork::ReadActive { id } | p::PreviewWork::ReadEpoch { id, .. } => id,
            p::PreviewWork::Ready(p) => p.id,
        };
        self.owner.core.cancel_connection_preview_id(id).map_err(error)
    }
    pub fn confirm_publication(
        &self,
        preview: Arc<CloudConnectionPreview>,
    ) -> Result<Arc<CloudRecoveryOperation>, CoreError> {
        callback::enter("CloudConnection.confirm_publication");
        self.owns(&preview.owner)?;
        let collection = self.owner.generation(preview.generation)?;
        let p::PreviewWork::Ready(p) = preview.take()? else {
            return Err(failure(CloudConnectionFailure::NotOffered));
        };
        Ok(self.recovery(
            self.owner
                .core
                .confirm_connection_publication(&p, &collection)
                .map_err(error)?,
            false,
            preview.generation,
        ))
    }
    pub fn confirm_adoption(
        &self,
        preview: Arc<CloudConnectionPreview>,
    ) -> Result<Arc<CloudRecoveryOperation>, CoreError> {
        callback::enter("CloudConnection.confirm_adoption");
        self.owns(&preview.owner)?;
        self.owner.check_generation(preview.generation)?;
        let p::PreviewWork::Ready(p) = preview.take_adoption()? else {
            return Err(failure(CloudConnectionFailure::NotOffered));
        };
        Ok(self.recovery(
            self.owner.host.confirm_cloud_adoption(&p).map_err(error)?,
            true,
            preview.generation,
        ))
    }
    pub fn next_publication_work(&self) -> Result<CloudPublicationWork, CoreError> {
        callback::enter("CloudConnection.next_publication_work");
        self.next_publication_work_with_work(self.owner.current_generation()?, &mut c::policy::PolicyWork::default())
    }
}

impl CloudConnection {
    fn next_publication_work_with_work(
        &self,
        generation: u64,
        authority: &mut c::policy::PolicyWork,
    ) -> Result<CloudPublicationWork, CoreError> {
        self.owner.check_generation(generation)?;
        let local = |work| {
            Arc::new(CloudPublicationLocal {
                owner: self.owner.clone(),
                generation,
                work,
            })
        };
        Ok(
            match self
                .owner
                .host
                .next_cloud_publication_work_with_work(authority)
                .map_err(error)?
            {
                work @ p::PublicationWork::Local { op, work: kind } => CloudPublicationWork::Local {
                    handle: local(work),
                    operation: self.recovery(op, false, generation),
                    kind: match kind {
                        p::LocalWork::Bind => CloudPublicationLocalKind::Bind,
                        p::LocalWork::WriteDecision => CloudPublicationLocalKind::WriteDecision,
                        p::LocalWork::Cancel => CloudPublicationLocalKind::Cancel,
                    },
                },
                p::PublicationWork::Own(request) => {
                    let command = match &request.operation {
                        p::OwnOperation::CreateZones(zones) => CloudOwnOperation::CreateZones { zones: zones.clone() },
                        p::OwnOperation::Read { zone, name } => CloudOwnOperation::Read {
                            target: CloudControlTarget {
                                zone: zone.clone(),
                                name: name.clone(),
                                record_type: if name == "root" { "MGMediaRoot" } else { "MGEpoch" }.into(),
                            },
                        },
                        p::OwnOperation::Save {
                            zone,
                            name,
                            record_type,
                            encrypted,
                            if_unchanged,
                        } => CloudOwnOperation::Save {
                            target: CloudControlTarget {
                                zone: zone.clone(),
                                name: name.clone(),
                                record_type: record_type.clone(),
                            },
                            encrypted: encrypted.clone(),
                            system_fields: if_unchanged.as_ref().map(|v| v.0.clone()),
                        },
                    };
                    CloudPublicationWork::Own {
                        operation: self.recovery(request.op, false, generation),
                        command,
                        metadata: (&*request).into(),
                        request: Arc::new(CloudPublicationRequest {
                            owner: self.owner.clone(),
                            request: *request,
                        }),
                    }
                }
                p::PublicationWork::Adoption(work) => {
                    let op = self
                        .owner
                        .core
                        .adoption_op()
                        .map_err(error)?
                        .filter(|o| o.id == work.op)
                        .ok_or_else(|| failure(CloudConnectionFailure::AuthorityChanged))?;
                    let action = match &work.task {
                        a::AdoptionTask::ReadActive | a::AdoptionTask::RecheckActive => {
                            CloudAdoptionAction::ReadActive {
                                target: active(),
                                expected: CloudControlRecord {
                                    encrypted: op.active.encrypted.clone(),
                                    system_fields: op.active.system_fields.0.clone(),
                                },
                            }
                        }
                        a::AdoptionTask::ReadEpoch => CloudAdoptionAction::Read {
                            target: epoch(op.epoch),
                        },
                        a::AdoptionTask::ReadMediaRoot => CloudAdoptionAction::Read {
                            target: CloudControlTarget {
                                zone: "meguri-media".into(),
                                name: "root".into(),
                                record_type: "MGMediaRoot".into(),
                            },
                        },
                        a::AdoptionTask::Fetch { cursor } => {
                            // Retain the exact epoch read owned by this work's validated op.
                            let expected = op
                                .epoch_record
                                .as_ref()
                                .ok_or_else(|| failure(CloudConnectionFailure::AuthorityChanged))?;
                            CloudAdoptionAction::Fetch {
                                zone: c::adapter::collection_zone(op.epoch),
                                cursor: cursor.as_ref().map(|v| v.0.clone()),
                                expected_epoch: CloudControlRecord {
                                    encrypted: expected.encrypted.clone(),
                                    system_fields: expected.system_fields.0.clone(),
                                },
                            }
                        }
                        a::AdoptionTask::FetchPayload {
                            name,
                            encrypted,
                            system_fields,
                        } => CloudAdoptionAction::FetchPayload {
                            zone: c::adapter::collection_zone(op.epoch),
                            name: name.clone(),
                            expected: CloudControlRecord {
                                encrypted: Some(encrypted.0.clone()),
                                system_fields: system_fields.0.clone(),
                            },
                        },
                        a::AdoptionTask::Validate => CloudAdoptionAction::Validate,
                        a::AdoptionTask::Activate => CloudAdoptionAction::Activate,
                    };
                    CloudPublicationWork::Adoption {
                        operation: self.recovery(work.op, true, generation),
                        action,
                        handle: Arc::new(CloudAdoptionWork {
                            owner: self.owner.clone(),
                            generation,
                            work,
                        }),
                    }
                }
                p::PublicationWork::Scan => CloudPublicationWork::Scan {
                    handle: local(p::PublicationWork::Scan),
                },
                p::PublicationWork::SendEntities => CloudPublicationWork::SendEntities,
                p::PublicationWork::Idle => CloudPublicationWork::Idle,
            },
        )
    }
}

#[uniffi::export]
impl CloudConnection {
    pub fn perform_local(&self, handle: Arc<CloudPublicationLocal>) -> Result<(), CoreError> {
        callback::enter("CloudConnection.perform_local");
        self.owns(&handle.owner)?;
        let collection = self.owner.generation(handle.generation)?;
        match handle.work {
            p::PublicationWork::Local { op, work } => self
                .owner
                .core
                .perform_publication_local(op, work, &collection)
                .map_err(error),
            p::PublicationWork::Scan => self
                .owner
                .core
                .reconcile_binding(
                    &collection,
                    &self.owner.host.media()?,
                    std::time::Duration::from_secs(30),
                )
                .map(|_| ())
                .map_err(error),
            _ => Err(failure(CloudConnectionFailure::NotOffered)),
        }
    }
    pub fn submit_publication_request(
        &self,
        request: Arc<CloudPublicationRequest>,
        queue: Arc<dyn CloudPublicationQueue>,
    ) -> Result<bool, CoreError> {
        callback::enter("CloudConnection.submit_publication_request");
        self.owns(&request.owner)?;
        let collection = self.owner.collection()?;
        self.owner
            .core
            .submit_publication_queue(&request.request, &collection, || {
                callback::invoke("CloudPublicationQueue.enqueue", || queue.enqueue())
            })
            .map_err(error)
    }
    pub fn publication_result(
        &self,
        request: Arc<CloudPublicationRequest>,
        result: CloudPublicationResult,
    ) -> Result<(), CoreError> {
        callback::enter("CloudConnection.publication_result");
        self.owns(&request.owner)?;
        self.owner
            .core
            .publication_result(&request.request, result.into(), &self.owner.collection()?)
            .map_err(error)
    }
    pub fn adoption_result(
        &self,
        handle: Arc<CloudAdoptionWork>,
        result: CloudAdoptionResult,
    ) -> Result<(), CoreError> {
        callback::enter("CloudConnection.adoption_result");
        let mut diagnostic = a::AdoptionDiagnosticRecorder::new(&handle.work.task);
        self.adoption_result_with_work(handle, result, &mut c::policy::PolicyWork::default(), &mut diagnostic)
    }
}

impl CloudConnection {
    fn adoption_result_with_work(
        &self,
        handle: Arc<CloudAdoptionWork>,
        result: CloudAdoptionResult,
        authority: &mut c::policy::PolicyWork,
        diagnostic: &mut a::AdoptionDiagnosticRecorder,
    ) -> Result<(), CoreError> {
        self.owns(&handle.owner)?;
        self.owner.check_generation(handle.generation)?;
        let reply = match result {
            CloudAdoptionResult::Failed => a::AdoptionReply::Failed,
            CloudAdoptionResult::TransportFailed {
                code,
                retry_after_duration_ms,
            } => a::AdoptionReply::TransportFailed {
                code,
                retry_after_duration_ms,
            },
            CloudAdoptionResult::Payload { record: v } => a::AdoptionReply::Payload {
                record: a::StagingRecord {
                    name: v.name,
                    record_type: v.record_type,
                    record: v.record.into(),
                    payload_file: v.payload_file.map(std::path::PathBuf::from),
                },
            },
            CloudAdoptionResult::Read { read } => a::AdoptionReply::Read(read.into()),
            CloudAdoptionResult::Page { records, deleted, next } => a::AdoptionReply::Page {
                records: records
                    .into_iter()
                    .map(|v| a::StagingRecord {
                        name: v.name,
                        record_type: v.record_type,
                        record: v.record.into(),
                        payload_file: v.payload_file.map(std::path::PathBuf::from),
                    })
                    .collect(),
                deleted,
                next: next.map(c::OpaqueBytes),
            },
        };
        self.owner
            .host
            .cloud_adoption_reply_with_diagnostic(&handle.work, reply, authority, diagnostic)
            .map_err(error)
    }
}

#[uniffi::export]
impl CloudConnection {
    pub fn perform_adoption_local(&self, handle: Arc<CloudAdoptionWork>) -> Result<(), CoreError> {
        callback::enter("CloudConnection.perform_adoption_local");
        self.owns(&handle.owner)?;
        self.owner.check_generation(handle.generation)?;
        self.owner
            .host
            .perform_cloud_adoption_local(&handle.work)
            .map_err(error)
    }
    pub fn cancel_operation(&self, operation: Arc<CloudRecoveryOperation>) -> Result<(), CoreError> {
        callback::enter("CloudConnection.cancel_operation");
        self.owns(&operation.owner)?;
        self.owner.current_generation()?;
        if operation.adoption {
            self.owner.host.cancel_cloud_adoption(operation.id).map_err(error)
        } else {
            self.owner
                .core
                .cancel_publication(operation.id, &self.owner.collection()?)
                .map_err(error)
        }
    }
    pub fn trigger(&self) -> Result<(), CoreError> {
        callback::enter("CloudConnection.trigger");
        self.owner.collection()?;
        self.owner.core.publication_trigger();
        Ok(())
    }
    pub fn recovery_status(&self) -> Result<Option<CloudRecoveryStatus>, CoreError> {
        callback::enter("CloudConnection.recovery_status");
        let generation = self.owner.current_generation()?;
        if let Some(op) = self.owner.core.adoption_op().map_err(error)? {
            let cancellation = self.owner.host.cloud_adoption_cancellation(op.id).map_err(error)?;
            return Ok(Some(CloudRecoveryStatus {
                operation: self.recovery(op.id, true, generation),
                step: match op.step {
                    a::AdoptStep::Staging => CloudRecoveryStep::Staging,
                    a::AdoptStep::Staged => CloudRecoveryStep::Staged,
                    a::AdoptStep::Ready => CloudRecoveryStep::Ready,
                },
                switch_sent: false,
                switch_conflict: false,
                retry_after_ms: op.retry_after_ms,
                adoption_transport_wait: op.transport_wait,
                adoption_cancellable: cancellation != a::AdoptionCancellation::NotOffered,
                adoption_after_review: cancellation == a::AdoptionCancellation::AfterReview,
                adoption_recovered_cancel_only: cancellation == a::AdoptionCancellation::RecoveredCancelOnly,
                publication_conflict_cancellable: false,
                publication_cancellable: false,
            }));
        }
        self
            .owner
            .core
            .publication_op()
            .map_err(error)?
            .map(|op| -> Result<CloudRecoveryStatus, CoreError> {
                let cancellable = self.owner.core.publication_conflict_cancellable(op.id).map_err(error)?;
                Ok(CloudRecoveryStatus {
                    operation: self.recovery(op.id, false, generation),
                    step: match op.step {
                        p::PublishStep::Confirmed => CloudRecoveryStep::Confirmed,
                        p::PublishStep::Bound => CloudRecoveryStep::Bound,
                        p::PublishStep::Zones => CloudRecoveryStep::Zones,
                        p::PublishStep::Sent => CloudRecoveryStep::Sent,
                        p::PublishStep::Complete => CloudRecoveryStep::Complete,
                        p::PublishStep::Cancelling => CloudRecoveryStep::Cancelling,
                    },
                    switch_sent: op.switch_sent,
                    switch_conflict: op.switch_conflict.is_some(),
                    retry_after_ms: op.retry_after_ms,
                    adoption_transport_wait: false,
                    adoption_cancellable: false,
                    adoption_after_review: false,
                    adoption_recovered_cancel_only: false,
                    publication_conflict_cancellable: cancellable,
                    publication_cancellable: self.owner.core.publication_cancellable(op.id).map_err(error)?,
                })
            }).transpose()
    }
}

#[path = "cloud_connection_jobs.rs"]
mod connection_jobs;
pub use connection_jobs::*;
