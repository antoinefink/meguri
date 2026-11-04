//! Explicit connection decisions and publication operations (V13, V15, V26).
//! SDK/platform adapters execute these requests; they never select the next step.
use super::{entity_codec::Kind, runtime::NativeFileIdentity, *};
pub use meguri_engine::device_state::PublicationDecisionState as DecisionSnapshot;
use meguri_engine::device_state::{Publication, PublicationId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A platform secure-byte provider, independent of collection/id randomness.
/// Implementations need not implement Debug and must not log the returned bytes.
pub trait KeyByteSource: Send + Sync {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeySourceError {
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationError {
    AccountUnavailable,
    AccountMismatch,
    AuthorityChanged,
    Busy,
    KeyUnavailable,
    EpochIncomplete,
    UnreadableControl,
    NotOffered,
    IncomingStagingUnavailable,
    MediaApplyUnavailable,
    ResetRecoveryUnavailable,
}
impl From<PublicationError> for runtime::RuntimeError {
    fn from(error: PublicationError) -> Self {
        Self::Publication(error)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectDecision {
    ConfirmUpload,
    ConfirmAdoption,
    /// V13.3: confirm “Continue syncing this collection” with unsent counts.
    ConfirmResume,
    ChooseCollection,
    CloudUnreadable,
    NoCollection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub counts: BTreeMap<Kind, u64>,
    pub empty: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRecord {
    /// Absent encrypted bytes are distinct from a missing record or a failed read.
    pub encrypted: Option<Vec<u8>>,
    pub system_fields: OpaqueBytes,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordRead {
    Found(ControlRecord),
    Missing,
    ZoneMissing,
    Failed,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub id: u64,
    pub local: Counts,
    /// Counts from a complete manifest, until real staging is available.
    /// Consumers must present their publication date, never label these exact.
    pub cloud: Option<Counts>,
    pub published_at_ms: Option<i64>,
    pub decision: ConnectDecision,
    pub account: OpaqueBytes,
    pub native_file: Option<NativeFileIdentity>,
    pub decision_before: Option<DecisionSnapshot>,
    pub active: Option<ControlRecord>,
    pub active_epoch: Option<EpochId>,
    pub unpublished_by_kind: BTreeMap<Kind, u64>,
    pub resume_binding: Option<BindingId>,
    pub unsent_by_kind: BTreeMap<Kind, u64>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewWork {
    ReadActive { id: u64 },
    ReadEpoch { id: u64, epoch: EpochId },
    Ready(Box<Preview>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublishStep {
    Confirmed,
    Bound,
    Zones,
    Sent,
    Complete,
    Cancelling,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingBefore {
    pub id: BindingId,
    pub state: BindingState,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishOp {
    pub id: [u8; 16],
    pub step: PublishStep,
    pub binding_before: Option<BindingBefore>,
    pub binding_after: Option<BindingId>,
    pub account: OpaqueBytes,
    pub confirmed_publication: PublicationId,
    pub confirmed_file: NativeFileIdentity,
    pub fence_before: DecisionSnapshot,
    pub epoch: EpochId,
    pub record_key: RecordKey,
    pub media_key: RecordKey,
    pub active: Option<ControlRecord>,
    pub active_epoch: Option<EpochId>,
    pub epoch_system_fields: Option<OpaqueBytes>,
    pub complete_system_fields: Option<OpaqueBytes>,
    pub root_acked: bool,
    pub switch_sent: bool,
    pub switch_conflict: Option<OpaqueBytes>,
    /// Canonical bytes of the last attempt, persisted before reporting it due.
    pub active_attempt: Option<OpaqueBytes>,
    pub started_at_ms: i64,
    pub retry_after_ms: Option<i64>,
    pub crt: i64,
    pub reason: u8,
}
impl PublishOp {
    pub(crate) fn is_encrypted_reset(&self) -> bool {
        self.binding_before
            .as_ref()
            .is_some_and(|b| b.state == BindingState::Isolated(IsolationReason::EncryptedDataReset))
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.account.0.is_empty()
            || self.confirmed_file.created_nanos >= 1_000_000_000
            || super::control_codec::EpochReason::try_from(u64::from(self.reason)).is_err()
            || (self.step != PublishStep::Confirmed && self.binding_after.is_none())
            || (matches!(
                self.step,
                PublishStep::Zones | PublishStep::Sent | PublishStep::Complete
            ) && (!self.root_acked || self.epoch_system_fields.is_none()))
        {
            return Err(Error::InvalidData);
        }
        Ok(())
    }
    pub(crate) fn decision_written(&self, scope: &BuildScope) -> Result<DecisionSnapshot, Error> {
        Ok(DecisionSnapshot {
            publication: Publication {
                identity: self.confirmed_publication,
                binding: meguri_engine::device_state::AccountBinding::Connected {
                    account: String::from_utf8(self.account.0.clone()).map_err(|_| Error::InvalidData)?,
                    configuration: scope.container.as_str().to_owned(),
                    environment: match scope.environment {
                        Environment::Development => "development",
                        Environment::Production => "production",
                    }
                    .to_owned(),
                },
                decision: meguri_engine::device_state::PublicationDecision::Clear,
            },
            pending_reason: None,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnKind {
    CreateZones,
    SaveMediaRoot,
    SavePublishingEpoch,
    SaveCompleteEpoch,
    ReadOwnEpoch,
    SaveActive,
    ReadResetMediaRoot,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnOperation {
    CreateZones(Vec<String>),
    Read {
        zone: String,
        name: String,
    },
    Save {
        zone: String,
        name: String,
        record_type: String,
        encrypted: Vec<u8>,
        if_unchanged: Option<OpaqueBytes>,
    },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnRequest {
    pub op: [u8; 16],
    pub attempt: u64,
    pub kind: OwnKind,
    pub operation: OwnOperation,
    pub(crate) incarnation: u64,
    pub(crate) generation: u64,
    pub(crate) instance: String,
    pub(crate) decision_before: DecisionSnapshot,
    pub(crate) decision_current: DecisionSnapshot,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnResult {
    ZonesCreated,
    Saved(ControlRecord),
    Conflict(ControlRecord),
    Read(RecordRead),
    /// An absolute core-clock deadline, retained for existing Rust callers.
    Failed {
        retry_after_ms: Option<i64>,
    },
    /// A server duration; Runtime converts it only after accepting this reply.
    RetryAfter {
        duration_ms: u64,
    },
    NotStarted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalWork {
    Bind,
    WriteDecision,
    Cancel,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicationWork {
    Local { op: [u8; 16], work: LocalWork },
    Own(Box<OwnRequest>),
    Adoption(super::adoption::AdoptionWork),
    Scan,
    SendEntities,
    Idle,
}
pub(crate) type CleanupDrain = (super::adapter::Guard, String, i64);

#[derive(Default)]
pub(crate) struct ProcessState {
    pub next_preview: u64,
    pub preview: Option<(Preview, bool)>,
    pub resume: Option<super::runtime::ResumeContext>,
    pub next_attempt: u64,
    pub running: Option<OwnRequest>,
    pub submitted: bool,
    pub zones_created: bool,
    /// Only a validated Missing reply grants a conditional root save. Never persisted.
    pub reset_root_missing: Option<(OwnRequest, super::adapter::Guard)>,
    pub reset_root_read_guard: Option<super::adapter::Guard>,
    pub own_epoch_read: bool,
    pub waiting: bool,
    pub last_cleanup_ms: Option<i64>,
    pub cleanup_drain: Option<CleanupDrain>,
    pub restarted_confirmed: bool,
}
macro_rules! opaque_debug {($($t:ty),+)=>{$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(stringify!($t))}
})+};}
opaque_debug!(
    ControlRecord,
    RecordRead,
    Preview,
    PreviewWork,
    BindingBefore,
    PublishOp,
    OwnOperation,
    OwnRequest,
    OwnResult,
    PublicationWork
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedZone {
    pub first_seen_ms: i64,
    pub deletion_due: bool,
}
#[derive(Clone, PartialEq, Eq)]
pub struct ZoneDeletionGuard {
    pub(crate) instance: String,
    pub(crate) incarnation: u64,
    pub(crate) file: NativeFileIdentity,
    pub(crate) decision: DecisionSnapshot,
    pub(crate) account: OpaqueBytes,
}
impl std::fmt::Debug for ZoneDeletionGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ZoneDeletionGuard")
    }
}

/// Read-only projection of the actual scoped runtime/store and local authority.
#[derive(Clone, Serialize, Deserialize)]
pub struct PublicationFacts {
    pub current: Option<Binding>,
    pub bindings: Vec<Binding>,
    pub recovery_ops: Vec<super::adoption::RecoveryOpFacts>,
    pub entities: BTreeMap<String, super::workflow::EntityFacts>,
    pub zones: BTreeMap<String, RetainedZone>,
    pub decision: DecisionSnapshot,
    pub native_file: NativeFileIdentity,
    pub media_key: Option<RecordKey>,
    pub root_acked: bool,
}
impl std::fmt::Debug for PublicationFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublicationFacts")
    }
}
