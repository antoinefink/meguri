use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

// Formatting is limited to the Rust type name.
macro_rules! type_debug {
    ($($ty:ty),+ $(,)?) => {$(
        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($ty))
            }
        }
    )+};
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Scope {
    pub container: String,
    pub environment: String,
    pub account: String,
}

/// Attribution only; this service does not model device/engine lifetimes.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caller {
    pub device: Option<String>,
    pub instance: Option<String>,
    pub own_operation: Option<String>,
}

/// Byte-backed today. A later stand-in may load a staged file into this boundary;
/// no file copy, lifetime or IO behavior is asserted by this type.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    bytes: Vec<u8>,
    sha256: [u8; 32],
}
impl Asset {
    pub fn synthetic(bytes: Vec<u8>) -> Self {
        let sha256 = Sha256::digest(&bytes).into();
        Self { bytes, sha256 }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    pub(super) fn valid(&self) -> bool {
        <[u8; 32]>::from(Sha256::digest(&self.bytes)) == self.sha256
    }
}

/// Other variants intentionally allow malformed-schema synthetic requests.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Field {
    EncryptedBytes(Vec<u8>),
    Asset(Asset),
    Bytes(Vec<u8>),
    Text(String),
    Integer(i64),
}
impl Field {
    pub(super) fn value_size(&self) -> u64 {
        match self {
            Self::EncryptedBytes(v) | Self::Bytes(v) => v.len() as u64,
            Self::Text(v) => v.len() as u64,
            Self::Integer(_) => 8,
            Self::Asset(_) => 0,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordInput {
    pub name: String,
    pub record_type: String,
    pub fields: BTreeMap<String, Field>,
}
impl RecordInput {
    pub fn encrypted(name: impl Into<String>, record_type: impl Into<String>, e: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            record_type: record_type.into(),
            fields: BTreeMap::from([("e".into(), Field::EncryptedBytes(e))]),
        }
    }
}

/// Opaque service encoding of zone, name and change tag. Only byte round-tripping
/// is public; this is not CKRecord or CKSyncEngine serialization.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemFields(pub(super) Vec<u8>);
impl SystemFields {
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub record_type: String,
    pub fields: BTreeMap<String, Field>,
    pub system_fields: SystemFields,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mutation {
    Save {
        record: RecordInput,
        if_unchanged: Option<SystemFields>,
    },
    Delete {
        name: String,
    },
}
impl Mutation {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Save { record, .. } => &record.name,
            Self::Delete { name } => name,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemovalCause {
    Deleted,
    Purged,
    EncryptedDataReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaggedMissing {
    UnknownItem,
    Recreate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub records_per_request: usize,
    pub non_asset_per_request: u64,
    pub non_asset_per_record: u64,
    pub asset_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            records_per_request: 250,
            non_asset_per_request: 2_000_000,
            non_asset_per_record: 1_000_000,
            asset_bytes: 104_857_600,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub limits: Limits,
    pub page_size: usize,
    pub tagged_missing: TaggedMissing,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            page_size: 200,
            tagged_missing: TaggedMissing::UnknownItem,
        }
    }
}
impl Config {
    pub(super) fn valid(&self) -> bool {
        let max = Limits::default();
        (1..=200).contains(&self.page_size)
            && (1..=max.records_per_request).contains(&self.limits.records_per_request)
            && self.limits.non_asset_per_request <= max.non_asset_per_request
            && self.limits.non_asset_per_record <= max.non_asset_per_record
            && self.limits.asset_bytes <= max.asset_bytes
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestedFields {
    All,
    Named(Vec<String>),
}

/// Tokens are scoped and epoch-bound, with durable snapshot pagination. They
/// carry service cursors only; they are not engine state serializations.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub(super) scope: Scope,
    pub(super) zone: Option<String>,
    pub(super) generation: u64,
    pub(super) watermark: u64,
    pub(super) snapshot: Option<u64>,
    pub(super) offset: usize,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    CreateZone {
        zone: String,
    },
    DeleteZone {
        zone: String,
        cause: RemovalCause,
    },
    Modify {
        zone: String,
        atomic: bool,
        changes: Vec<Mutation>,
    },
    Read {
        zone: String,
        name: String,
        fields: RequestedFields,
    },
    FetchZone {
        zone: String,
        token: Option<Token>,
    },
    FetchDatabase {
        token: Option<Token>,
    },
    /// Settings are applied at a request boundary and logged like every request.
    Configure(Config),
    /// Per private database/account within its container and environment.
    SetQuota {
        bytes: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    UnknownItem,
    ServerRecordChanged,
    InvalidArguments,
    LimitExceeded,
    QuotaExceeded,
    BatchRequestFailed,
    ZoneNotFound,
    UserDeletedZone,
    ChangeTokenExpired,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceError {
    pub code: ErrorCode,
    pub encrypted_data_reset: bool,
    pub server_record: Option<Box<Record>>,
}
impl ServiceError {
    pub(super) fn code(code: ErrorCode) -> Self {
        Self {
            code,
            encrypted_data_reset: false,
            server_record: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationSuccess {
    Saved(Record),
    Deleted,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationResult {
    pub name: String,
    pub result: Result<MutationSuccess, ServiceError>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordChange {
    Saved(Record),
    Deleted { name: String, record_type: String },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneChange {
    pub zone: String,
    /// None means created, Some means removed for that cause.
    pub removed: Option<RemovalCause>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub changes: Vec<T>,
    pub token: Token,
    pub more: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Done,
    Modified(Vec<MutationResult>),
    Record(Record),
    ZoneChanges(Page<RecordChange>),
    DatabaseChanges(Page<ZoneChange>),
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub request_id: u64,
    pub result: Result<Reply, ServiceError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogOutcome {
    Success,
    Failure(ErrorCode),
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordSummary {
    pub name: String,
    pub non_asset_bytes: u64,
    pub asset_bytes: u64,
    pub outcome: LogOutcome,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestKind {
    CreateZone,
    DeleteZone,
    Modify,
    Read,
    FetchZone,
    FetchDatabase,
    Configure,
    SetQuota,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestLog {
    pub request_id: u64,
    pub scope: Scope,
    pub caller: Caller,
    pub zone: Option<String>,
    pub kind: RequestKind,
    pub records: Vec<RecordSummary>,
    pub outcome: LogOutcome,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedRecordChange {
    pub sequence: u64,
    pub generation: u64,
    pub change: RecordChange,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedZoneChange {
    pub sequence: u64,
    pub change: ZoneChange,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneState {
    pub generation: u64,
    pub removed: Option<RemovalCause>,
    pub records: BTreeMap<String, Record>,
    pub changes: Vec<LoggedRecordChange>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseState {
    pub scope: Scope,
    pub config: Config,
    pub quota: Option<u64>,
    pub used_bytes: u64,
    pub zones: BTreeMap<String, ZoneState>,
    pub changes: Vec<LoggedZoneChange>,
}

/// Infrastructure errors contain no SQL diagnostics, paths, identifiers or values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    Io,
    Database,
    Encoding,
    InvalidConfig,
    Exhausted,
    InspectionLimit,
}
impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fake cloud storage: {self:?}")
    }
}
impl std::error::Error for StorageError {}
impl From<rusqlite::Error> for StorageError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}
impl From<serde_json::Error> for StorageError {
    fn from(_: serde_json::Error) -> Self {
        Self::Encoding
    }
}

type_debug!(
    Scope,
    Caller,
    Asset,
    Field,
    RecordInput,
    SystemFields,
    Record,
    Mutation,
    RequestedFields,
    Token,
    Operation,
    ServiceError,
    MutationSuccess,
    MutationResult,
    RecordChange,
    ZoneChange,
    Reply,
    Response,
    RecordSummary,
    RequestLog,
    LoggedRecordChange,
    LoggedZoneChange,
    ZoneState,
    DatabaseState
);
impl<T> fmt::Debug for Page<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Page")
            .field("count", &self.changes.len())
            .field("more", &self.more)
            .finish_non_exhaustive()
    }
}

/// Durable record identity plus the field selection returned by that request.
/// Resolve a single version explicitly; outcome inspection never reads bodies.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordReference {
    pub version: u64,
    pub fields: Vec<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReference {
    pub code: ErrorCode,
    pub encrypted_data_reset: bool,
    pub server_record: Option<RecordReference>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationReference {
    pub name: String,
    /// None is a successful deletion.
    pub result: Result<Option<RecordReference>, ErrorReference>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeReference {
    Saved(RecordReference),
    Deleted { name: String, record_type: String },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplyReference {
    Done,
    Modified(Vec<MutationReference>),
    Record(RecordReference),
    ZoneChanges(Page<ChangeReference>),
    DatabaseChanges(Page<ZoneChange>),
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeReference {
    pub request_id: u64,
    pub result: Result<ReplyReference, ErrorReference>,
}
/// Physical retention counters; payload bytes count each content hash once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageStats {
    pub payloads: u64,
    pub payload_bytes: u64,
    pub versions: u64,
    pub current_records: u64,
    pub record_changes: u64,
    pub requests: u64,
    pub snapshots: u64,
    pub memberships: u64,
}
type_debug!(
    RecordReference,
    ErrorReference,
    MutationReference,
    ChangeReference,
    ReplyReference,
    OutcomeReference
);
