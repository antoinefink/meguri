//! SDK delivery authority and durable adapter observations. Network scheduling
//! stays with the SDK; pending and exchange decisions stay with Runtime.
use super::{workflow::*, *};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    pub generation: u64,
    pub binding: Vec<u8>,
    pub token: u64,
}
#[derive(Clone)]
pub(crate) struct SDKAuthority {
    pub id: String,
    pub guard: Guard,
    pub stopping: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchDrain {
    pub zone: String,
    pub fetched: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectionOutcome {
    Succeeded,
    WaitingNoAccount,
    Incomplete,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedExchange {
    pub id: u64,
    pub collection: CollectionOutcome,
    pub result: ExchangeResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming: Option<ExchangeIncoming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<ExchangeWork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accounting: Option<ExchangeAccounting>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeIncoming {
    pub ready: u64,
    pub waiting: u64,
    pub blocked: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeWork {
    pub held_pending_names: BTreeSet<String>,
    pub pending_unheld_names: BTreeSet<String>,
    pub media: super::policy::MediaFacts,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeAccounting {
    pub clean_names: BTreeSet<String>,
    pub incoming: Vec<ExchangeIncomingEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeIncomingEntry {
    pub record_name: String,
    pub reason: ExchangeIncomingReason,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExchangeIncomingReason {
    Ready,
    Generation,
    Reference,
    Structure,
    Format,
    Asset,
    ApplyFailed,
}
impl From<&InboxReason> for ExchangeIncomingReason {
    fn from(reason: &InboxReason) -> Self {
        match reason {
            InboxReason::Ready => Self::Ready,
            InboxReason::Generation => Self::Generation,
            InboxReason::Reference => Self::Reference,
            InboxReason::Structure => Self::Structure,
            InboxReason::Format => Self::Format,
            InboxReason::Asset => Self::Asset,
            InboxReason::ApplyFailed { .. } => Self::ApplyFailed,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Durable {
    pub active: Option<Saved>,
    pub active_deleted: bool,
    pub collection_loss: Option<CollectionLoss>,
    pub epoch_verification: Option<EpochVerification>,
    pub absence: Option<AbsenceLedger>,
    pub absence_sequence: u64,
    pub drains: Vec<FetchDrain>,
    pub exchanges: BTreeMap<u64, CompletedExchange>,
    pub exchange_ended_ms: BTreeMap<u64, i64>,
}
// Durable service evidence, never a reconstructed SDK/read authority. The
// enclosing row and these facts restrict recovery to the original publication.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CollectionLoss {
    pub binding: Binding,
    pub file: super::runtime::NativeFileIdentity,
}
// Pending verification is not a Service loss outcome or a live read authority.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EpochVerification {
    pub binding: Binding,
    pub file: super::runtime::NativeFileIdentity,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AbsenceLedger {
    pub pass: [u8; 16],
    pub completed: bool,
    #[serde(default)]
    pub zone: String,
    #[serde(default)]
    pub started_at_ms: i64,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub binding: Vec<u8>,
    pub tracking_content: Vec<u8>,
    pub serialization: Option<Vec<u8>>,
    pub entities: Vec<EntityFacts>,
    pub inbox: Vec<InboxEntry>,
    pub root: Option<RootAcknowledgement>,
    pub tracking_stopped: bool,
    pub absence_pass_completed: bool,
}
#[derive(Default)]
pub(crate) struct FetchBoundary {
    pub group: String,
    pub manual: bool,
    pub owns_call: bool,
    pub zones: BTreeMap<String, (u64, bool)>,
}
#[derive(Default)]
pub(crate) struct FetchCall {
    pub id: u64,
    pub scope: Vec<String>,
    pub boundary: Option<FetchBoundary>,
    // A manual did event is evidence only; its API call must still return.
    pub ended: Option<FetchBoundary>,
    pub completed: bool,
    pub own_eligible: bool,
    pub failed_zones: BTreeSet<String>,
    pub read_zones: BTreeSet<String>,
    pub drains: BTreeSet<String>,
}
#[derive(Default)]
pub(crate) struct AdapterState {
    pub sdk: Option<SDKAuthority>,
    pub zone_deletions: Vec<super::zone_deletion::Diagnostic>,
    pub sdk_retrying: bool,
    pub next_sdk: u64,
    pub token: u64,
    pub paused: bool,
    pub durable: Durable,
    pub fetch: Option<FetchCall>,
    pub scheduled: Option<FetchBoundary>,
    pub next_fetch: u64,
    pub exchange_owner: Option<(u64, String, u64)>,
    pub closed_exchanges: BTreeMap<u64, (String, CompletedExchange)>,
    pub next_identity: u64,
    pub identity_read: Option<(u64, Guard, u64, Option<String>, bool)>,
    pub accepted_identity_read: Option<AcceptedIdentityRead>,
    // Creation fact, consumed only by the first bound collection-zone will event.
    pub cursorless_first_zone: bool,
    pub no_state: bool,
    pub no_state_point: u64,
    pub no_state_creation: bool,
    pub zone_wills: BTreeSet<String>,
    pub decided_zones: BTreeSet<String>,
    pub fetch_groups: BTreeSet<String>,
    pub root_reads: Vec<RootReadDiagnostic>,
    // Accepted epoch loss belongs to this SDK/binding, not a transport handle.
    pub pending_collection_loss: Option<FetchRead>,
    pub known_loss: Option<KnownLoss>,
    pub missing_did: u64,
    pub retain_inspection: bool,
    pub closed_snapshot: Option<Snapshot>,
    #[cfg(feature = "test-hooks")]
    pub fail_inbox: bool,
    #[cfg(feature = "test-hooks")]
    pub fail_state: bool,
}

/// Finite native facts; no NSError, connection or account payload is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneLossCause {
    Deleted,
    Purged,
    EncryptedDataReset,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneLoss {
    pub zone: String,
    pub cause: ZoneLossCause,
}
#[derive(Clone)]
pub(crate) struct KnownLoss {
    pub sdk: String,
    pub(super) scan: super::runtime::ScanGuard,
    pub guard: Guard,
    pub after_read: u64,
    pub losses: Vec<ZoneLoss>,
}

/// Core-owned completion evidence. The guard is the result of accepting the
/// original read, which may itself isolate a binding or invalidate its token.
pub(crate) struct AcceptedIdentityRead {
    pub request: u64,
    pub guard: Guard,
    pub incarnation: u64,
    pub no_collection: bool,
    pub identity: super::runtime::IdentityStatus,
}

/// K2/K4.1 collection zone, using the epoch bytes without changing identity.
pub fn collection_zone(epoch: EpochId) -> String {
    format!(
        "c-{}",
        data_encoding::BASE32_NOPAD.encode(&epoch.0).to_ascii_lowercase()
    )
}

/// P12.1 admission retained until capture; it supplies no boundary or write authority.
#[derive(Clone)]
pub struct SyncNowRequest {
    pub(crate) sdk: String,
    pub(super) scan: super::runtime::ScanGuard,
    pub(crate) guard: Guard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochPreparationOutcome { Unknown, Ready, Conflict, Unsupported }

#[derive(Clone)]
pub(crate) enum FetchReadPurpose {
    Fetch,
    KnownLoss,
    SyncNowEpoch(std::sync::Arc<std::sync::Mutex<EpochPreparationOutcome>>),
}

/// Issued for one original SDK and purpose. Fields cannot be
/// reconstructed by a transport caller to select a different zone or point.
#[derive(Clone)]
pub struct FetchRead {
    pub(crate) sdk: String,
    pub(super) scan: super::runtime::ScanGuard,
    pub(crate) guard: Guard,
    pub(crate) fetch: Option<u64>,
    pub(crate) point: u64,
    pub(crate) zone: String,
    pub(crate) lost_collection: bool,
    pub(crate) a7_failure: bool,
    pub(crate) purpose: FetchReadPurpose,
    pub(crate) after_identity: u64,
    pub(crate) loss_identity_after: u64,
}
impl FetchRead {
    pub fn is_epoch_preparation(&self) -> bool {
        matches!(self.purpose, FetchReadPurpose::SyncNowEpoch(_))
    }
    pub fn epoch_preparation_outcome(&self) -> Option<EpochPreparationOutcome> {
        match &self.purpose {
            FetchReadPurpose::SyncNowEpoch(result) => Some(*result.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
            _ => None,
        }
    }
    pub(crate) fn set_preparation_outcome(&self, outcome: EpochPreparationOutcome) {
        if let FetchReadPurpose::SyncNowEpoch(result) = &self.purpose {
            *result.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = outcome;
        }
    }
    pub(crate) fn is_known_loss(&self) -> bool { matches!(self.purpose, FetchReadPurpose::KnownLoss) }

    pub fn zone(&self) -> &str {
        &self.zone
    }
    pub fn is_active(&self) -> bool {
        self.zone == "meguri-root"
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootReadDiagnostic {
    pub active: bool,
    pub lost_collection: bool,
    pub outcome: String,
    pub a7_failure: bool,
    pub accepted: bool,
    pub failure: Option<super::zone_deletion::FailureFacts>,
    pub terminal: Option<super::zone_deletion::FailureFacts>,
}
