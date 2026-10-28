use meguri_engine::device_state::PublicationId;
use serde::{Deserialize, Serialize};

use super::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerId(String);

impl ContainerId {
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        if value.is_empty() || value.contains('\0') {
            return Err(Error::InvalidIdentifier);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Environment {
    Development,
    Production,
}

impl Environment {
    pub(super) fn code(self) -> i64 {
        match self {
            Self::Development => 0,
            Self::Production => 1,
        }
    }
}

/// A build is scoped by its container and environment, not an executable id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildScope {
    pub container: ContainerId,
    pub environment: Environment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochId(pub [u8; 16]);

/// SDK data and account identities must never enter diagnostics.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpaqueBytes(pub Vec<u8>);

impl std::fmt::Debug for OpaqueBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpaqueBytes")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordKey(pub [u8; 32]);

impl std::fmt::Debug for RecordKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecordKey")
    }
}

/// Stored reasons only. `environment_changed` is derived, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationReason {
    SignedOut,
    AccountChanged,
    EpochSwitched,
    ZoneMissing,
    ZoneDeletedByUser,
    EncryptedDataReset,
    ActiveMissing,
    ReplacedLocally,
    FileMismatch,
    Stopped,
    Republished,
    TrackingDamaged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingState {
    Active,
    Isolated(IsolationReason),
}

/// Facts supplied by publication/adoption; no live account read is performed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingFacts {
    pub account: OpaqueBytes,
    pub epoch: EpochId,
    pub record_key: RecordKey,
    pub publication: PublicationId,
    pub state: BindingState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub id: BindingId,
    pub created_at_ms: i64,
    pub scope: BuildScope,
    pub facts: BindingFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineState {
    pub serialization: OpaqueBytes,
    pub saved_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordType {
    Entity,
    Generation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxSource {
    Fetch,
    SaveConflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyFailure {
    Apply,
    Rekey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxReason {
    Ready,
    Generation,
    Reference,
    Structure,
    Format,
    Asset,
    ApplyFailed { failure: ApplyFailure, error: OpaqueBytes },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxValue {
    Record {
        erev: u64,
        system_fields: OpaqueBytes,
        envelope: OpaqueBytes,
        /// Already staged by the caller; storage does not move asset files.
        staged_asset: Option<String>,
    },
    Deletion,
}

impl std::fmt::Debug for InboxValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InboxValue")
    }
}

/// Storage input, not a decoded/validated CloudKit record. Control-record
/// handling and stale-version/replacement rules are not implemented here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxEntry {
    pub record_type: RecordType,
    pub record_name: String,
    pub value: InboxValue,
    pub reason: InboxReason,
    pub source: InboxSource,
    pub absence_pass: Option<[u8; 16]>,
    pub retry_count: u32,
    pub fetched_at_ms: i64,
}

impl std::fmt::Debug for InboxEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InboxEntry")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredInboxEntry {
    pub id: i64,
    pub entry: InboxEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadDigest(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictKind {
    Note,
    Card,
    Deck,
    Preset,
    Notetype,
    MediaName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictReason {
    FieldConflict,
    EditVsDelete,
    BlobConflict,
    NotetypeConflict,
    MissingNotetype,
    StructureRollback,
    UnmappedStructure,
    MediaContent,
}

/// Opaque storage input. The caller supplies both payload digests; this type
/// does not decode an envelope or assert semantic CloudKit record validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictCopy {
    pub id: CopyId,
    pub kind: ConflictKind,
    pub key: OpaqueBytes,
    pub reason: ConflictReason,
    pub losing_envelope: OpaqueBytes,
    pub losing_payload_digest: PayloadDigest,
    pub winning_payload_digest: PayloadDigest,
    pub field_names: Option<Vec<String>>,
    pub template_names: Option<Vec<String>>,
    pub note_deck: Option<i64>,
    pub staged_path: Option<String>,
    pub created_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredConflict {
    pub binding: BindingId,
    pub copy: ConflictCopy,
}

/// A destination-only, permanently non-resumable owner. Kept separately from
/// ordinary bindings so absent provenance cannot become a usable identity.
/// State is always isolated/tracking_damaged; there is no transition API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SalvageBinding {
    pub id: BindingId,
    pub source: String,
    pub original_binding: Option<BindingId>,
    pub created_at_ms: i64,
    pub original_created_at_ms: Option<i64>,
    pub container: Option<ContainerId>,
    pub environment: Option<Environment>,
    pub account: Option<OpaqueBytes>,
    pub epoch: Option<EpochId>,
    pub record_key: Option<RecordKey>,
    pub publication: Option<PublicationId>,
}
