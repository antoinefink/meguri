use super::{publication::*, runtime::NativeFileIdentity, *};
use meguri_engine::device_state::{Publication, PublicationId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdoptStep {
    Staging,
    Staged,
    Ready,
}
/// Current cancellation availability, not authority to execute a cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdoptionCancellation {
    NotOffered,
    Live,
    AfterReview,
    RecoveredCancelOnly,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptOp {
    pub id: [u8; 16],
    pub step: AdoptStep,
    pub binding_before: Option<BindingBefore>,
    pub binding_after: BindingId,
    pub account: OpaqueBytes,
    pub before: Option<DecisionSnapshot>,
    pub file_before: Option<NativeFileIdentity>,
    pub candidate: Publication,
    pub active: ControlRecord,
    pub epoch: EpochId,
    pub epoch_record: Option<ControlRecord>,
    pub media_root: Option<ControlRecord>,
    pub staging_name: String,
    pub started_at_ms: i64,
    #[serde(default)]
    pub transport_wait: bool,
    #[serde(default)]
    pub retry_after_ms: Option<i64>,
    /// V27.2 row 5: started with no confirmation because the local collection
    /// was proved to hold no user content. The exclusive phase tests it again
    /// and cancels the op when that no longer holds. False for a confirmed op.
    #[serde(default)]
    pub requires_no_user_content: bool,
}
impl std::fmt::Debug for AdoptOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdoptOp")
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdoptionTask {
    ReadActive,
    ReadEpoch,
    ReadMediaRoot,
    Fetch {
        cursor: Option<OpaqueBytes>,
    },
    FetchPayload {
        name: String,
        encrypted: OpaqueBytes,
        system_fields: OpaqueBytes,
    },
    Validate,
    RecheckActive,
    Activate,
}
impl std::fmt::Debug for AdoptionTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ReadActive => "ReadActive",
            Self::ReadEpoch => "ReadEpoch",
            Self::ReadMediaRoot => "ReadMediaRoot",
            Self::Fetch { .. } => "Fetch",
            Self::FetchPayload { .. } => "FetchPayload",
            Self::Validate => "Validate",
            Self::RecheckActive => "RecheckActive",
            Self::Activate => "Activate",
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionWork {
    pub op: [u8; 16],
    pub task: AdoptionTask,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StagingRecord {
    pub name: String,
    pub record_type: String,
    pub record: ControlRecord,
    /// Borrowed transport source. Adoption copies it; it never moves/removes it.
    #[serde(default)]
    pub payload_file: Option<PathBuf>,
}
#[derive(Clone, Serialize, Deserialize)]
pub enum AdoptionReply {
    Read(RecordRead),
    /// A terminal transport failure invalidates the entire unpublished candidate.
    Failed,
    /// Numeric evidence from an original issued SDK page/payload failure only.
    TransportFailed {
        code: i64,
        retry_after_duration_ms: Option<u64>,
    },
    Payload {
        record: StagingRecord,
    },
    Page {
        records: Vec<StagingRecord>,
        deleted: Vec<String>,
        next: Option<OpaqueBytes>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStep {
    Publish(PublishStep),
    Adopt(AdoptStep),
}
impl PartialEq<PublishStep> for RecoveryStep {
    fn eq(&self, step: &PublishStep) -> bool {
        matches!(self, Self::Publish(p) if p == step)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryKind {
    Publish(Box<PublishOp>),
    Adopt(Box<AdoptOp>),
}
/// Common presentation fields retain publication clients' read surface. The
/// complete durable row and its distinct step are carried by `kind`/`step`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryOpFacts {
    pub id: [u8; 16],
    pub step: RecoveryStep,
    pub kind: RecoveryKind,
    pub account: OpaqueBytes,
    pub epoch: EpochId,
    pub binding_after: Option<BindingId>,
    pub confirmed_publication: Option<PublicationId>,
    pub confirmed_file: Option<NativeFileIdentity>,
    pub switch_sent: bool,
    pub active_attempt: Option<OpaqueBytes>,
    pub switch_conflict: Option<OpaqueBytes>,
}
impl From<PublishOp> for RecoveryOpFacts {
    fn from(p: PublishOp) -> Self {
        Self {
            id: p.id,
            step: RecoveryStep::Publish(p.step),
            account: p.account.clone(),
            epoch: p.epoch,
            binding_after: p.binding_after,
            confirmed_publication: Some(p.confirmed_publication),
            confirmed_file: Some(p.confirmed_file),
            switch_sent: p.switch_sent,
            active_attempt: p.active_attempt.clone(),
            switch_conflict: p.switch_conflict.clone(),
            kind: RecoveryKind::Publish(Box::new(p)),
        }
    }
}
impl From<AdoptOp> for RecoveryOpFacts {
    fn from(p: AdoptOp) -> Self {
        Self {
            id: p.id,
            step: RecoveryStep::Adopt(p.step),
            account: p.account.clone(),
            epoch: p.epoch,
            binding_after: (p.step == AdoptStep::Ready).then_some(p.binding_after),
            confirmed_publication: p.before.as_ref().map(|d| d.publication.identity),
            confirmed_file: p.file_before,
            switch_sent: false,
            active_attempt: None,
            switch_conflict: None,
            kind: RecoveryKind::Adopt(Box::new(p)),
        }
    }
}

// Ephemeral, content-free evidence for one reply invocation; never serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdoptionDiagnosticTask {
    ReadActive,
    ReadEpoch,
    ReadMediaRoot,
    Fetch,
    FetchPayload,
    Validate,
    RecheckActive,
    Activate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdoptionDiagnosticStage {
    Operation,
    Owner,
    Claim,
    Authority,
    ActiveValidation,
    EpochShape,
    EpochValidation,
    EpochWrite,
    MediaRootShape,
    MediaRootValidation,
    MediaRootWrite,
    MediaRootEpoch,
    MediaRootStageCreate,
    MediaRootSpace,
    PageValidation,
    PageRecordValidation,
    PageEpochShape,
    PageEpochData,
    PageEpochSystemFields,
    PageSpace,
    PayloadValidation,
    PayloadRead,
    PayloadSpace,
    PayloadWrite,
    ReceivedValidation,
    RecordEncode,
    RecordSpace,
    RecordWrite,
    RowStage,
    FinalSpace,
    PageAdvance,
    PageFinish,
    NextAuthority,
    Transport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdoptionFailureCategory {
    Validation,
    Tracking,
    Engine,
    Cancelled,
    AuthorityChanged,
    AccountUnavailable,
    Publication,
    Incomplete,
    FileIdentityUnavailable,
    Transport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdoptionFailureDiagnostic {
    pub task: AdoptionDiagnosticTask,
    /// The last entered stage, not necessarily the operation that failed.
    pub last_entered_stage: AdoptionDiagnosticStage,
    pub primary_category: Option<AdoptionFailureCategory>,
    pub cleanup_category: Option<AdoptionFailureCategory>,
}
/// Owned only by the current call chain. Only a frozen failure is exported.
pub struct AdoptionDiagnosticRecorder {
    value: AdoptionFailureDiagnostic,
}
impl AdoptionDiagnosticRecorder {
    pub fn new(task: &AdoptionTask) -> Self {
        Self {
            value: AdoptionFailureDiagnostic {
                task: match task {
                    AdoptionTask::ReadActive => AdoptionDiagnosticTask::ReadActive,
                    AdoptionTask::ReadEpoch => AdoptionDiagnosticTask::ReadEpoch,
                    AdoptionTask::ReadMediaRoot => AdoptionDiagnosticTask::ReadMediaRoot,
                    AdoptionTask::Fetch { .. } => AdoptionDiagnosticTask::Fetch,
                    AdoptionTask::FetchPayload { .. } => AdoptionDiagnosticTask::FetchPayload,
                    AdoptionTask::Validate => AdoptionDiagnosticTask::Validate,
                    AdoptionTask::RecheckActive => AdoptionDiagnosticTask::RecheckActive,
                    AdoptionTask::Activate => AdoptionDiagnosticTask::Activate,
                },
                last_entered_stage: AdoptionDiagnosticStage::Operation,
                primary_category: None,
                cleanup_category: None,
            },
        }
    }
    pub(super) fn enter(&mut self, stage: AdoptionDiagnosticStage) {
        self.value.last_entered_stage = stage;
    }
    pub(super) fn primary(&mut self, error: &runtime::RuntimeError) {
        if self.value.primary_category.is_none() && self.value.cleanup_category.is_none() {
            self.value.primary_category = Some(AdoptionFailureCategory::of(error));
        }
    }
    pub(super) fn transport(&mut self) {
        self.value.primary_category = Some(AdoptionFailureCategory::Transport);
    }
    pub(super) fn cleanup(&mut self, error: &runtime::RuntimeError) {
        self.value.cleanup_category = Some(AdoptionFailureCategory::of(error));
    }
    pub fn snapshot(&self) -> Option<AdoptionFailureDiagnostic> {
        (self.value.primary_category.is_some() || self.value.cleanup_category.is_some()).then_some(self.value)
    }
}
impl AdoptionFailureCategory {
    fn of(error: &runtime::RuntimeError) -> Self {
        use runtime::RuntimeError as R;
        match error {
            R::Tracking(Error::InvalidData | Error::InvalidIdentifier) => Self::Validation,
            R::Tracking(_) => Self::Tracking,
            R::Engine(meguri_engine::EngineError::Cancelled) => Self::Cancelled,
            R::Engine(_) => Self::Engine,
            R::StaleIncarnation | R::Publication(PublicationError::AuthorityChanged) => Self::AuthorityChanged,
            R::IdentityUnverified | R::Publication(PublicationError::AccountUnavailable) => Self::AccountUnavailable,
            R::Publication(_) => Self::Publication,
            R::Incomplete(_) => Self::Incomplete,
            R::FileIdentityUnavailable => Self::FileIdentityUnavailable,
        }
    }
}
