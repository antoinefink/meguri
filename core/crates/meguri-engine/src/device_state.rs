//! R6 publication identities and replacement journal. No network or decision resolver.
mod retained_wal;
use meguri_model::rng::RandomSource;
use meguri_storage::{StorageError, sidecar};
pub use retained_wal::RawRestoreAuthority;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AccountBinding {
    LocalOnly,
    Connected {
        account: String,
        configuration: String,
        environment: String,
    },
    /// A caller with uncertain account state cannot grant publication or recovery.
    Unknown,
}
impl std::fmt::Debug for AccountBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::LocalOnly => "LocalOnly",
            Self::Connected { .. } => "Connected { .. }",
            Self::Unknown => "Unknown",
        })
    }
}
impl AccountBinding {
    pub(crate) fn certain(&self) -> bool {
        match self {
            Self::LocalOnly => true,
            Self::Connected {
                account,
                configuration,
                environment,
            } => !account.is_empty() && !configuration.is_empty() && !environment.is_empty(),
            Self::Unknown => false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicationId([u64; 2]);
impl PublicationId {
    /// Opaque caller operation/candidate identity; this grants no publication authority.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let mut first = [0; 8];
        let mut second = [0; 8];
        first.copy_from_slice(&bytes[..8]);
        second.copy_from_slice(&bytes[8..]);
        Self([u64::from_be_bytes(first), u64::from_be_bytes(second)])
    }

    fn fresh(random: &dyn RandomSource) -> Self {
        Self([random.next_u64(), random.next_u64()])
    }
    fn successor(random: &dyn RandomSource, prior: Self) -> Self {
        // Include the persistent predecessor so restarting a deterministic
        // test random source does not repeat a stored publication identifier.
        let mut hash = Sha256::new();
        hash.update(b"meguri-publication-successor-v1");
        for bits in [prior.0[0], prior.0[1], random.next_u64(), random.next_u64()] {
            hash.update(bits.to_le_bytes());
        }
        let bytes: [u8; 32] = hash.finalize().into();
        Self([
            u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        ])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicationDecision {
    Clear,
    Pending,
}
/// Optional R6.7 reason; a present JSON null is invalid, just like an unknown string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingDecisionReason {
    Restore,
    ColpkgImport,
    ManualImport,
}
fn pending_reason(extra: &BTreeMap<String, Value>) -> Result<Option<PendingDecisionReason>, StorageError> {
    extra.get("pendingDecisionReason").map(typed).transpose()
}
fn strict_optional_reason<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PendingDecisionReason>, D::Error> {
    PendingDecisionReason::deserialize(deserializer).map(Some)
}
fn set_pending_reason(state: &mut State, reason: Option<PendingDecisionReason>) -> Result<(), StorageError> {
    if let Some(reason) = reason {
        state.extra.insert(
            "pendingDecisionReason".into(),
            serde_json::to_value(reason).map_err(failure)?,
        );
    } else {
        state.extra.remove("pendingDecisionReason");
    }
    Ok(())
}
fn replacement_reason(reason: crate::replace::ReplaceReason) -> Option<PendingDecisionReason> {
    use crate::replace::ReplaceReason;
    match reason {
        ReplaceReason::Restore => Some(PendingDecisionReason::Restore),
        ReplaceReason::ColpkgImport => Some(PendingDecisionReason::ColpkgImport),
        ReplaceReason::FullDownload => Some(PendingDecisionReason::ManualImport),
        ReplaceReason::FullUpload | ReplaceReason::CloudAdopt => None,
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub identity: PublicationId,
    pub binding: AccountBinding,
    pub decision: PublicationDecision,
}
/// Complete R6 decision, including the optional replacement reason.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationDecisionState {
    pub publication: Publication,
    pub pending_reason: Option<PendingDecisionReason>,
}
impl std::fmt::Debug for PublicationDecisionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublicationDecisionState")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationToken {
    identity: PublicationId,
    binding: AccountBinding,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub length: u64,
    pub sha256: [u8; 32],
}
pub(crate) fn fingerprint(path: &Path) -> Result<Option<FileIdentity>, StorageError> {
    fingerprint_native(path).map(|identity| identity.map(|(bytes, _)| bytes))
}
fn fingerprint_native(path: &Path) -> Result<Option<(FileIdentity, NativeIdentity)>, StorageError> {
    fingerprint_native_checked(path, &|| false, false)
}
fn fingerprint_native_checked(
    path: &Path,
    cancel: &dyn Fn() -> bool,
    final_pass: bool,
) -> Result<Option<(FileIdentity, NativeIdentity)>, StorageError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let before = file.metadata()?;
    if !before.is_file() || before.nlink() != 1 {
        return Err(failure("database identity is not an independent regular file"));
    }
    let mut digest = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if final_pass {
            meguri_model::faultpoint!("replace.media.final_database_hash")?;
        }
        if cancel() {
            return Err(failure("database identity scan cancelled"));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(read as u64)
            .ok_or_else(|| failure("database identity length overflow"))?;
        digest.update(&buffer[..read]);
    }
    let after = file.metadata()?;
    let bound = std::fs::symlink_metadata(path)?;
    if before.dev() != bound.dev()
        || before.ino() != bound.ino()
        || before.len() != length
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(failure("database identity changed while reading"));
    }
    Ok(Some((
        FileIdentity {
            length,
            sha256: digest.finalize().into(),
        },
        NativeIdentity {
            device: before.dev(),
            inode: before.ino(),
        },
    )))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Phase {
    Intent,
    Activated,
    // Required enum values make main-only readers reject this intent.
    WalIntent,
    WalActivated,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Intent {
    operation: PublicationId,
    reason: crate::replace::ReplaceReason,
    prior: Publication,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "strict_optional_reason"
    )]
    prior_pending_decision_reason: Option<PendingDecisionReason>,
    candidate: Publication,
    old_file: Option<FileIdentity>,
    candidate_file: FileIdentity,
    phase: Phase,
    #[serde(default)]
    verified_at: Option<i64>,
    #[serde(default)]
    wal: Option<retained_wal::WalRecovery>,
    #[serde(default)]
    old_native: Option<NativeIdentity>,
    #[serde(default)]
    candidate_native: Option<NativeIdentity>,
    #[serde(default)]
    media: Option<meguri_storage::media::CollectionMediaIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    safety_backup: Option<ReplacementBackup>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NativeIdentity {
    device: u64,
    inode: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    active: Publication,
    intent: Option<Intent>,
    /// R6 activation chain survives policy write failure and further replacements.
    #[serde(default)]
    replacements: Vec<ReplacementEvidence>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
fn failure(message: impl ToString) -> StorageError {
    StorageError::Io {
        detail: format!("publication recovery blocked: {}", message.to_string()),
    }
}
fn known_publications(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        for key in ["active", "prior", "candidate"] {
            if let Some(publication) = object.get_mut(key).and_then(Value::as_object_mut) {
                publication.retain(|key, _| matches!(key.as_str(), "identity" | "binding" | "decision"));
            }
        }
        for key in ["intent", "lastReplacement", "replacements"] {
            if let Some(value) = object.get_mut(key) {
                known_publications(value);
            }
        }
    } else if let Some(array) = value.as_array_mut() {
        for value in array {
            known_publications(value);
        }
    }
}
fn decode(doc: Option<&Map<String, Value>>) -> Result<Option<State>, StorageError> {
    doc.and_then(|doc| doc.get("deviceState"))
        .map(|value| {
            let mut checked = value.clone();
            known_publications(&mut checked);
            if !checked.as_object().is_some_and(|state| state.contains_key("intent")) {
                return Err(failure("missing replacement intent field"));
            }
            let state: State = serde_json::from_value(checked).map_err(|_| failure("unreadable device state"))?;
            pending_reason(&state.extra)?;
            bound(&state, &state.active.binding)?;
            Ok(state)
        })
        .transpose()
}
fn preserve_neighbors(value: &mut Value, previous: &Value) {
    if let (Some(current), Some(old)) = (value.as_object_mut(), previous.as_object()) {
        // Unknown publication/intent members belong to that exact identity.
        for key in ["identity", "operation"] {
            if current.get(key) != old.get(key) {
                return;
            }
        }
        // Replacement witnesses have no top-level identity or operation;
        // their extensions belong to the exact candidate generation.
        if current.get("candidate").and_then(|p| p.get("identity"))
            != old.get("candidate").and_then(|p| p.get("identity"))
        {
            return;
        }
        for (key, old_value) in old {
            if let Some(value) = current.get_mut(key) {
                preserve_neighbors(value, old_value);
            } else if key != "pendingDecisionReason" && key != "prior_pending_decision_reason" {
                current.insert(key.clone(), old_value.clone());
            }
        }
    } else if let (Some(current), Some(old)) = (value.as_array_mut(), previous.as_array()) {
        for value in current {
            if let Some(previous) = old.iter().find(|prior| {
                value.get("candidate").and_then(|p| p.get("identity")).is_some()
                    && value.get("candidate").and_then(|p| p.get("identity"))
                        == prior.get("candidate").and_then(|p| p.get("identity"))
            }) {
                preserve_neighbors(value, previous);
            }
        }
    }
}
fn put(doc: &mut Map<String, Value>, state: &State) -> Result<(), StorageError> {
    let mut value = serde_json::to_value(state).map_err(failure)?;
    if let Some(previous) = doc.get("deviceState") {
        for key in ["active", "intent", "lastReplacement", "replacements"] {
            if let (Some(current), Some(old)) = (value.get_mut(key), previous.get(key)) {
                preserve_neighbors(current, old);
            }
        }
        // A legacy single witness becomes a retained link on activation.
        // Restore its extensions only to the record with that candidate identity.
        if let (Some(records), Some(old)) = (
            value.get_mut("replacements").and_then(Value::as_array_mut),
            previous.get("lastReplacement"),
        ) {
            for record in records {
                preserve_neighbors(record, old);
            }
        }
        // Activation moves the exact candidate publication into active.
        if let (Some(active), Some(candidate)) = (
            value.get_mut("active"),
            previous.get("intent").and_then(|i| i.get("candidate")),
        ) {
            preserve_neighbors(active, candidate);
        }
    }
    doc.insert("deviceState".into(), value);
    Ok(())
}
fn bound(state: &State, binding: &AccountBinding) -> Result<(), StorageError> {
    let cloud = state
        .intent
        .as_ref()
        .is_some_and(|intent| intent.reason == crate::replace::ReplaceReason::CloudAdopt);
    if !cloud && (!binding.certain() || &state.active.binding != binding) {
        return Err(failure("publication binding is unavailable or does not match"));
    }
    if let Some(intent) = &state.intent {
        if !cloud && (&intent.prior.binding != binding || &intent.candidate.binding != binding) {
            return Err(failure("intent account binding differs"));
        }
        let expected = if intent.reason.requires_publication_decision() {
            PublicationDecision::Pending
        } else if cloud {
            PublicationDecision::Clear
        } else {
            intent.prior.decision
        };
        let activated_matches = state.active == intent.candidate
            || (cloud
                && state.active.identity == intent.candidate.identity
                && state.active.binding == intent.candidate.binding
                && state.active.decision == PublicationDecision::Pending);
        if (cloud
            && (!intent.prior.binding.certain()
                || !intent.candidate.binding.certain()
                || intent.operation == intent.candidate.identity
                || intent.media.is_some()
                || intent.candidate_native.is_none()
                || intent.old_file.is_some() != intent.old_native.is_some()))
            || intent.candidate.decision != expected
            || intent.prior.identity == intent.candidate.identity
            || (matches!(intent.phase, Phase::Intent | Phase::WalIntent) && state.active != intent.prior)
            || (matches!(intent.phase, Phase::Activated | Phase::WalActivated) && !activated_matches)
            || (intent.wal.is_some() != matches!(intent.phase, Phase::WalIntent | Phase::WalActivated))
            || (intent.wal.is_some()
                && (intent.reason != crate::replace::ReplaceReason::Restore || intent.media.is_some()))
        {
            return Err(failure("inconsistent publication journal"));
        }
    }
    Ok(())
}
fn sidecar_path(live: &Path) -> PathBuf {
    live.parent().unwrap_or(Path::new(".")).join("meguri-sidecar.json")
}

/// Device-only maintenance records. Times and snapshot_mod are signed epoch
/// milliseconds, matching the collection's signed col.mod representation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastBackup {
    pub snapshot_mod: i64,
    pub instant: i64,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastQuickCheck {
    pub instant: i64,
    pub passed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckDatabaseResult {
    Problems(CheckDatabaseProblems),
    Corrupt,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastCheckDatabase {
    pub instant: i64,
    pub result: CheckDatabaseResult,
}
const PROBLEM_KEYS: [&str; 12] = [
    "card_properties",
    "new_positions_too_high",
    "card_last_review_time",
    "cards_without_note",
    "decks_missing",
    "field_counts",
    "duplicate_card_ords",
    "cards_without_template",
    "revlog_properties",
    "notes_invalid_utf8",
    "future_ids",
    "notetypes_recovered",
];
fn typed<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, StorageError> {
    serde_json::from_value(value.clone()).map_err(|_| failure("invalid maintenance record"))
}
fn check_record(value: &Value) -> Result<LastCheckDatabase, StorageError> {
    let record = value
        .as_object()
        .ok_or_else(|| failure("invalid database check record"))?;
    let instant = record
        .get("instant")
        .and_then(Value::as_i64)
        .ok_or_else(|| failure("invalid database check instant"))?;
    let result = record
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| failure("invalid database check result"))?;
    let result = if let Some(error) = result.get("error") {
        if error.as_str() != Some("corrupt") || PROBLEM_KEYS.iter().any(|key| result.contains_key(*key)) {
            return Err(failure("invalid database check error variant"));
        }
        CheckDatabaseResult::Corrupt
    } else {
        CheckDatabaseResult::Problems(typed(&Value::Object(result.clone()))?)
    };
    Ok(LastCheckDatabase { instant, result })
}
/// Validate without rewriting. Metadata-only documents are allowed, but the
/// presence of R6 fields requires the existing complete, consistent R6 state.
fn maintenance_object(doc: Option<&Map<String, Value>>) -> Result<Option<&Map<String, Value>>, StorageError> {
    let Some(value) = doc.and_then(|doc| doc.get("deviceState")) else {
        return Ok(None);
    };
    let state = value.as_object().ok_or_else(|| failure("invalid device state"))?;
    if let Some(reason) = state.get("pendingDecisionReason") {
        typed::<PendingDecisionReason>(reason)?;
    }
    if ["active", "intent", "replacements", "lastReplacement"]
        .iter()
        .any(|key| state.contains_key(*key))
    {
        // Serde defaults a missing Option to None; R6 requires an explicit key.
        if !state.contains_key("intent") {
            return Err(failure("missing replacement intent field"));
        }
        // Publication's decoder is strict. Validate its known fields while
        // retaining the original document, including future publication members.
        let mut checked = state.clone();
        fn publication(value: &mut Value) {
            if let Some(object) = value.as_object_mut() {
                object.retain(|key, _| matches!(key.as_str(), "identity" | "binding" | "decision"));
            }
        }
        if let Some(active) = checked.get_mut("active") {
            publication(active);
        }
        if let Some(intent) = checked.get_mut("intent").and_then(Value::as_object_mut) {
            for key in ["prior", "candidate"] {
                if let Some(value) = intent.get_mut(key) {
                    publication(value);
                }
            }
        }
        if let Some(records) = checked.get_mut("replacements").and_then(Value::as_array_mut) {
            for record in records {
                for key in ["prior", "candidate"] {
                    if let Some(value) = record.get_mut(key) {
                        publication(value);
                    }
                }
            }
        }
        if let Some(record) = checked.get_mut("lastReplacement") {
            for key in ["prior", "candidate"] {
                if let Some(value) = record.get_mut(key) {
                    publication(value);
                }
            }
            typed::<ReplacementEvidence>(record)?;
        }
        let decoded: State = typed(&Value::Object(checked))?;
        bound(&decoded, &decoded.active.binding)?;
    }
    Ok(Some(state))
}
fn maintenance_state(doc: Option<&Map<String, Value>>) -> Result<Option<&Map<String, Value>>, StorageError> {
    let Some(state) = maintenance_object(doc)? else {
        return Ok(None);
    };
    for (key, value) in state {
        match key.as_str() {
            "lastBackup" => {
                typed::<LastBackup>(value)?;
            }
            "lastCheckDatabase" => {
                check_record(value)?;
            }
            "lastOptimize" | "optimizeCadenceStart" => {
                value.as_i64().ok_or_else(|| failure("invalid optimize instant"))?;
            }
            "lastQuickCheck" => {
                typed::<LastQuickCheck>(value)?;
            }
            _ => {}
        }
    }
    Ok(Some(state))
}
fn merge_fields(target: &mut Map<String, Value>, fields: Map<String, Value>) {
    for (key, value) in fields {
        if let Value::Object(fields) = value {
            let record = target.entry(key).or_insert_with(|| Value::Object(Map::new()));
            // Existing records were validated by maintenance_state.
            merge_fields(record.as_object_mut().expect("validated record object"), fields);
        } else {
            target.insert(key, value);
        }
    }
}

/// Independently readable maintenance fields; an error is never absence.
#[derive(Clone, Debug)]
pub struct MaintenanceRecords {
    pub backup: Result<Option<LastBackup>, StorageError>,
    pub check_database: Result<Option<LastCheckDatabase>, StorageError>,
    pub optimize: Result<Option<meguri_model::time::EpochMillis>, StorageError>,
    pub cadence_start: Result<Option<meguri_model::time::EpochMillis>, StorageError>,
    pub quick_check: Result<Option<LastQuickCheck>, StorageError>,
}

/// Provenance supplied by the actual forced provider before R6's intent write.
/// Absence on legacy/foreign witnesses stays unknown; lastBackup is unrelated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementBackup {
    pub name: String,
    pub snapshot_modified: i64,
    pub instant: i64,
}
impl From<crate::ports::BackupReceipt> for ReplacementBackup {
    fn from(receipt: crate::ports::BackupReceipt) -> Self {
        Self {
            name: receipt.name,
            snapshot_modified: receipt.snapshot_modified.0,
            instant: receipt.instant.0,
        }
    }
}
/// Durable local-activation evidence, retained after R6 intent cleanup. This
/// proves a local replacement only, never remote upload acceptance/equality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementEvidence {
    pub reason: crate::replace::ReplaceReason,
    pub prior: Publication,
    pub candidate: Publication,
    pub candidate_file: FileIdentity,
    pub old_file: Option<FileIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_backup: Option<ReplacementBackup>,
}

/// R6's measured closed-file activation, retained through SQLite reopen. Its
/// byte witness is checked before any writer opens, not by hashing a live DB.
/// Only the engine can construct or return this authority.
#[derive(Clone, Debug)]
pub struct CloudAdoptActivation {
    record: CloudAdoptActivationRecord,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CloudAdoptActivationRecord {
    operation: PublicationId,
    replacement: ReplacementEvidence,
    native: NativeIdentity,
}
impl CloudAdoptActivation {
    pub fn matches(&self, operation: PublicationId, prior: &Publication, candidate: &Publication) -> bool {
        self.record.operation == operation
            && self.record.replacement.reason == crate::replace::ReplaceReason::CloudAdopt
            && &self.record.replacement.prior == prior
            && &self.record.replacement.candidate == candidate
            && self.record.replacement.safety_backup.is_some()
    }
    pub fn native_file(&self) -> (u64, u64) {
        (self.record.native.device, self.record.native.inode)
    }
}

/// Constructible only by checking the durable R6 chain against its active record.
/// Consumers must hold the collection's lifecycle slot until their CAS completes.
#[derive(Clone, Debug)]
pub struct LocalReplacementSuccessor {
    before: PublicationId,
    after: PublicationId,
}
impl LocalReplacementSuccessor {
    pub fn before(&self) -> PublicationId {
        self.before
    }
    pub fn after(&self) -> PublicationId {
        self.after
    }
}

/// Read access to the device record. Collection ownership must cover these
/// calls, including queued-work token validation immediately before sending.
#[derive(Clone, Debug)]
pub struct DeviceState {
    path: PathBuf,
    live: PathBuf,
}
impl DeviceState {
    pub fn for_collection(live: &Path) -> Self {
        Self {
            path: sidecar_path(live),
            live: live.to_owned(),
        }
    }
    fn maintenance_read<T>(
        &self,
        key: &str,
        parse: impl FnOnce(&Value) -> Result<T, StorageError>,
    ) -> Result<Option<T>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            maintenance_state(doc)?
                .and_then(|state| state.get(key))
                .map(parse)
                .transpose()
        })
    }
    // Preserve the existing backup precondition: lifecycle initialization owns
    // the publication identity. Maintenance never creates or resets it.
    fn maintenance_write(&self, key: &str, value: Value) -> Result<(), StorageError> {
        sidecar::update_document(&self.path, |doc| {
            let existing = maintenance_state(Some(doc))?;
            if !existing.is_some_and(|state| state.contains_key("active")) {
                return Err(failure("maintenance write requires existing publication identity"));
            }
            let state = doc
                .get_mut("deviceState")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| failure("invalid device state"))?;
            if key == "lastCheckDatabase"
                && let Some(result) = state
                    .get_mut(key)
                    .and_then(|v| v.get_mut("result"))
                    .and_then(Value::as_object_mut)
            {
                // Only the old variant's known fields may be removed.
                if value["result"].get("error").is_some() {
                    for field in PROBLEM_KEYS {
                        result.remove(field);
                    }
                } else {
                    result.remove("error");
                }
            }
            merge_fields(state, Map::from_iter([(key.to_owned(), value)]));
            Ok(())
        })
    }
    /// M10: decode each maintenance record independently under the shared lock.
    pub fn maintenance_records(&self) -> MaintenanceRecords {
        fn unreadable(error: StorageError) -> MaintenanceRecords {
            MaintenanceRecords {
                backup: Err(error.clone()),
                check_database: Err(error.clone()),
                optimize: Err(error.clone()),
                cadence_start: Err(error.clone()),
                quick_check: Err(error),
            }
        }
        sidecar::read_document(&self.path, |doc| {
            let object = maintenance_object(doc)?;
            let read = |key: &str| object.and_then(|o| o.get(key));
            let instant = |key: &str| {
                read(key)
                    .map(|v| {
                        v.as_i64()
                            .map(meguri_model::time::EpochMillis)
                            .ok_or_else(|| failure("invalid maintenance instant"))
                    })
                    .transpose()
            };
            Ok(MaintenanceRecords {
                backup: read("lastBackup").map(typed).transpose(),
                check_database: read("lastCheckDatabase").map(check_record).transpose(),
                optimize: instant("lastOptimize"),
                cadence_start: instant("optimizeCadenceStart"),
                quick_check: read("lastQuickCheck").map(typed).transpose(),
            })
        })
        .unwrap_or_else(unreadable)
    }
    pub fn set_optimize_cadence_start(&self, instant: meguri_model::time::EpochMillis) -> Result<(), StorageError> {
        self.maintenance_write("optimizeCadenceStart", instant.0.into())
    }
    pub fn last_backup(&self) -> Result<Option<LastBackup>, StorageError> {
        self.maintenance_read("lastBackup", typed)
    }
    pub fn set_last_backup(&self, record: &LastBackup) -> Result<(), StorageError> {
        self.maintenance_write("lastBackup", serde_json::to_value(record).map_err(failure)?)
    }
    pub fn last_check_database(&self) -> Result<Option<LastCheckDatabase>, StorageError> {
        self.maintenance_read("lastCheckDatabase", check_record)
    }
    pub fn set_last_check_database(&self, record: &LastCheckDatabase) -> Result<(), StorageError> {
        let result = match &record.result {
            CheckDatabaseResult::Problems(problems) => serde_json::to_value(problems).map_err(failure)?,
            CheckDatabaseResult::Corrupt => serde_json::json!({"error": "corrupt"}),
        };
        self.maintenance_write(
            "lastCheckDatabase",
            serde_json::json!({"instant": record.instant, "result": result}),
        )
    }
    pub fn last_optimize(&self) -> Result<Option<meguri_model::time::EpochMillis>, StorageError> {
        self.maintenance_read("lastOptimize", |value| {
            value
                .as_i64()
                .map(meguri_model::time::EpochMillis)
                .ok_or_else(|| failure("invalid optimize instant"))
        })
    }
    pub fn set_last_optimize(&self, instant: meguri_model::time::EpochMillis) -> Result<(), StorageError> {
        self.maintenance_write("lastOptimize", instant.0.into())
    }
    pub fn last_quick_check(&self) -> Result<Option<LastQuickCheck>, StorageError> {
        self.maintenance_read("lastQuickCheck", typed)
    }
    pub fn set_last_quick_check(&self, record: &LastQuickCheck) -> Result<(), StorageError> {
        self.maintenance_write("lastQuickCheck", serde_json::to_value(record).map_err(failure)?)
    }
    pub fn replacement_evidence(&self) -> Result<Option<ReplacementEvidence>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            decode(doc)?
                .and_then(|s| s.extra.get("lastReplacement").cloned())
                .map(|v| serde_json::from_value(v).map_err(|_| failure("unreadable activation evidence")))
                .transpose()
        })
    }
    pub fn replacement_evidence_for(&self, prior: PublicationId) -> Result<Option<ReplacementEvidence>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            let Some(state) = decode(doc)? else { return Ok(None) };
            Ok(retained_replacements(&state)?
                .into_iter()
                .find(|e| e.prior.identity == prior))
        })
    }
    pub(crate) fn cloud_adopt_activation(
        &self,
        native: (u64, u64),
    ) -> Result<Option<CloudAdoptActivation>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            let Some(state) = decode(doc)? else {
                return Ok(None);
            };
            let Some(value) = state.extra.get("cloudAdoptActivation") else {
                return Ok(None);
            };
            let proof: CloudAdoptActivationRecord = typed(value)?;
            // R6 account reconciliation may retain Pending on this exact
            // activated candidate. That decision never changes its swap proof.
            let mut candidate = proof.replacement.candidate.clone();
            if candidate.decision == PublicationDecision::Clear
                && matches!(&candidate.binding, AccountBinding::Connected { .. })
                && candidate.binding.certain()
                && state.active.decision == PublicationDecision::Pending
                && pending_reason(&state.extra)?.is_none()
            {
                candidate.decision = PublicationDecision::Pending;
            }
            // The retained candidate_file must still be exactly the byte proof
            // measured at activation. Later local writes need not hash equally.
            if state.intent.is_some()
                || state.active != candidate
                || proof.replacement.reason != crate::replace::ReplaceReason::CloudAdopt
                || (proof.native.device, proof.native.inode) != native
                || retained_replacements(&state)?.last() != Some(&proof.replacement)
            {
                return Ok(None);
            }
            Ok(Some(CloudAdoptActivation { record: proof }))
        })
    }
    /// Offline proof, never inferred from process generation or collection absence.
    /// Legacy lastReplacement is the first retained link on the next activation.
    pub fn local_replacement_successor(
        &self,
        before: PublicationId,
    ) -> Result<Option<LocalReplacementSuccessor>, StorageError> {
        if !std::fs::symlink_metadata(&self.live)?.file_type().is_file() {
            return Ok(None);
        }
        sidecar::read_document(&self.path, |doc| {
            let Some(state) = decode(doc)? else { return Ok(None) };
            bound(&state, &state.active.binding)?;
            if state.intent.is_some() || state.active.identity == before {
                return Ok(None);
            }
            let chain = retained_replacements(&state)?;
            let Some(start) = chain.iter().position(|e| e.prior.identity == before) else {
                return Ok(None);
            };
            if !matches!(
                chain[start].reason,
                crate::replace::ReplaceReason::ColpkgImport | crate::replace::ReplaceReason::Restore
            ) {
                return Ok(None);
            }
            let mut successor = &chain[start].prior;
            for evidence in &chain[start..] {
                if evidence.prior.identity != successor.identity
                    || evidence.prior.binding != successor.binding
                    || evidence.prior.identity == evidence.candidate.identity
                    || evidence.prior.binding != evidence.candidate.binding
                {
                    return Ok(None);
                }
                successor = &evidence.candidate;
            }
            // Decisions may be resolved independently; persistent identity and
            // binding still have to name the active verified successor.
            if successor.identity != state.active.identity || successor.binding != state.active.binding {
                return Ok(None);
            }
            Ok(Some(LocalReplacementSuccessor {
                before,
                after: successor.identity,
            }))
        })
    }
    pub fn pending_decision_reason(&self) -> Result<Option<PendingDecisionReason>, StorageError> {
        self.maintenance_read("pendingDecisionReason", typed)
    }
    /// Observation for recovery consumers after R6 startup reconciliation.
    /// An unfinished intent cannot be interpreted as an old/candidate decision.
    pub fn settled_publication(&self) -> Result<Option<Publication>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            let state = decode(doc)?;
            if state.as_ref().is_some_and(|state| state.intent.is_some()) {
                return Err(failure("replacement intent has not settled"));
            }
            Ok(state.map(|state| state.active))
        })
    }
    pub fn publication(&self) -> Result<Option<Publication>, StorageError> {
        sidecar::read_document(&self.path, |doc| Ok(decode(doc)?.map(|state| state.active)))
    }
    /// Read the decision and its reason under one sidecar lock.
    pub fn publication_decision(&self) -> Result<Option<PublicationDecisionState>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            decode(doc)?
                .map(|state| {
                    Ok(PublicationDecisionState {
                        pending_reason: pending_reason(&state.extra)?,
                        publication: state.active,
                    })
                })
                .transpose()
        })
    }
    /// V15.2/V15.3 resolver: the caller owns a durable confirmed recovery row.
    /// This is a comparison of the complete prior decision, never an unconditional
    /// fence clear. An intervening replacement/decision or unfinished R6 intent
    /// refuses the write. Publication identity and replacement evidence stay intact.
    pub(crate) fn compare_exchange_publication_decision(
        &self,
        expected: &PublicationDecisionState,
        replacement: &PublicationDecisionState,
    ) -> Result<bool, StorageError> {
        if replacement.publication.identity != expected.publication.identity
            || (replacement.publication.decision == PublicationDecision::Clear && replacement.pending_reason.is_some())
        {
            return Err(failure("invalid publication decision resolution"));
        }
        // Only the registered durable authority calls this CAS. A new decision
        // supplies its verified binding; cancellation restores the exact prior
        // decision, including an uncertain binding, without granting a token.
        sidecar::update_document(&self.path, |doc| {
            let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing publication decision"))?;
            if state.intent.is_some()
                || state.active != expected.publication
                || pending_reason(&state.extra)? != expected.pending_reason
            {
                return Ok(false);
            }
            state.active = replacement.publication.clone();
            set_pending_reason(&mut state, replacement.pending_reason)?;
            put(doc, &state)?;
            Ok(true)
        })
    }
    /// Local-only never grants a sending token. Pending and unfinished intents
    /// block both identities. There is deliberately no public clear setter.
    pub fn publication_token(&self, binding: &AccountBinding) -> Result<PublicationToken, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            let state = decode(doc)?.ok_or_else(|| failure("missing identity"))?;
            bound(&state, binding)?;
            if !std::fs::symlink_metadata(&self.live)?.file_type().is_file() {
                return Err(failure("no regular live collection"));
            }
            if state.intent.is_some()
                || state.active.decision != PublicationDecision::Clear
                || !matches!(binding, AccountBinding::Connected { .. })
            {
                return Err(failure("explicit publication decision required"));
            }
            Ok(PublicationToken {
                identity: state.active.identity,
                binding: binding.clone(),
            })
        })
    }
    pub fn validate_token(&self, token: &PublicationToken, binding: &AccountBinding) -> Result<(), StorageError> {
        if &token.binding != binding || self.publication_token(binding)? != *token {
            return Err(failure("stale publication token"));
        }
        Ok(())
    }
    /// O8 bookkeeping cannot clear or rewrite the cloud publication decision.
    pub fn record_manual_transfer_success(
        &self,
        optimized_at: Option<meguri_model::time::EpochMillis>,
    ) -> Result<(), StorageError> {
        sidecar::update_document(&self.path, |doc| {
            let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing identity"))?;
            state.extra.insert("oneWayChoicePending".into(), false.into());
            state.extra.remove("lastFailedManualSanityCounts");
            if let Some(at) = optimized_at {
                state.extra.insert("lastOptimize".into(), at.0.into());
            }
            put(doc, &state)
        })
    }
    /// Legacy manual state is independent; clearing it cannot edit R6's decision.
    pub fn set_manual_transfer_pending(&self, pending: bool) -> Result<(), StorageError> {
        sidecar::update_document(&self.path, |doc| {
            let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing identity"))?;
            state.extra.insert("oneWayChoicePending".into(), pending.into());
            put(doc, &state)
        })
    }
}

// A different build cannot decide the account of the recorded build. It
// preserves that decision; token admission still requires an exact binding.
fn cloud_account_matches(recorded: &AccountBinding, current: &AccountBinding) -> bool {
    match (recorded, current) {
        (AccountBinding::LocalOnly, _) => true,
        (
            AccountBinding::Connected {
                account,
                configuration,
                environment,
            },
            AccountBinding::Connected {
                account: now,
                configuration: build,
                environment: cloud,
            },
        ) => configuration != build || environment != cloud || account == now,
        _ => false,
    }
}
fn activation_decision(intent: &Intent, binding: &AccountBinding) -> Publication {
    let mut candidate = intent.candidate.clone();
    if intent.reason == crate::replace::ReplaceReason::CloudAdopt && !cloud_account_matches(&candidate.binding, binding)
    {
        candidate.decision = PublicationDecision::Pending;
    }
    candidate
}
fn cloud_active(state: &State) -> Result<bool, StorageError> {
    let retained = retained_replacements(state)?;
    let Some(last) = retained.last() else { return Ok(false) };
    Ok(last.reason == crate::replace::ReplaceReason::CloudAdopt
        && last.candidate.decision == PublicationDecision::Clear
        && last.candidate.binding.certain()
        && state.active.identity == last.candidate.identity
        && state.active.binding == last.candidate.binding)
}
fn open_bound(state: &State, binding: &AccountBinding) -> Result<(), StorageError> {
    if state.intent.is_none() && cloud_active(state)? {
        return Ok(());
    }
    bound(state, binding)
}
fn reconcile_cloud_active(live: &Path, state: &State, binding: &AccountBinding) -> Result<(), StorageError> {
    // A previous mismatch stays pending even if that account returns. Only a
    // new explicit decision may resolve it; the captured binding never moves.
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut current = decode(Some(doc))?.ok_or_else(|| failure("missing adoption state"))?;
        if current.intent.is_some() || !cloud_active(&current)? || current.active != state.active {
            return Err(failure("adoption state changed during recovery"));
        }
        if !cloud_account_matches(&current.active.binding, binding) {
            current.active.decision = PublicationDecision::Pending;
        }
        set_pending_reason(&mut current, None)?;
        put(doc, &current)
    })
}

pub(crate) fn ensure(live: &Path, binding: &AccountBinding, random: &dyn RandomSource) -> Result<(), StorageError> {
    let path = sidecar_path(live);
    if sidecar::read_document(&path, |doc| {
        if let Some(state) = decode(doc)? {
            open_bound(&state, binding)?;
            Ok(true)
        } else {
            Ok(false)
        }
    })? {
        return Ok(());
    }
    if !binding.certain() {
        return Err(failure("uncertain binding"));
    }
    sidecar::update_document(&path, |doc| {
        if let Some(state) = decode(Some(doc))? {
            return open_bound(&state, binding);
        }
        put(
            doc,
            &State {
                active: Publication {
                    identity: PublicationId::fresh(random),
                    binding: binding.clone(),
                    decision: PublicationDecision::Clear,
                },
                intent: None,
                replacements: Vec::new(),
                extra: BTreeMap::new(),
            },
        )
    })
}

/// Called under the collection lock, before sweep/open/day writes. Activated
/// records deliberately do not compare hashes: local review may have committed.
/// Startup account authority is used only for CloudAdopt R6.6. Other local
/// lifecycle recovery keeps the recorded binding without treating it as a live
/// account. The actual account (including Unknown) is never replaced for adoption.
pub(crate) fn reconcile_startup(
    live: &Path,
    fallback: &AccountBinding,
    actual: Option<&AccountBinding>,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
) -> Result<(), StorageError> {
    let binding = if let Some(actual) = actual {
        sidecar::read_document(&sidecar_path(live), |doc| {
            let Some(state) = decode(doc)? else {
                return Ok(fallback.clone());
            };
            if state
                .intent
                .as_ref()
                .is_some_and(|i| i.reason == crate::replace::ReplaceReason::CloudAdopt)
                || cloud_active(&state)?
            {
                Ok(actual.clone())
            } else {
                Ok(state.active.binding)
            }
        })?
    } else {
        fallback.clone()
    };
    reconcile(live, &binding, clock, random)
}
pub(crate) fn reconcile(
    live: &Path,
    binding: &AccountBinding,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
) -> Result<(), StorageError> {
    reconcile_with_media(live, binding, clock, random, None)
}
pub(crate) fn reconcile_with_media(
    live: &Path,
    binding: &AccountBinding,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
    shared: Option<&meguri_storage::media::MediaStore>,
) -> Result<(), StorageError> {
    let path = sidecar_path(live);
    let state = sidecar::read_document(&path, decode)?;
    let Some(state) = state else {
        return if binding.certain() {
            Ok(())
        } else {
            Err(failure("uncertain binding"))
        };
    };
    if state.intent.is_none() && cloud_active(&state)? {
        reconcile_cloud_active(live, &state, binding)?;
        return cleanup_completed_media(live, &state.active.binding);
    }
    bound(&state, binding)?;
    if state.intent.is_none() {
        return cleanup_completed_media(live, binding);
    }
    if state.intent.as_ref().is_some_and(|intent| intent.wal.is_some()) {
        return retained_wal::reconcile_wal(live, state, binding);
    }
    let intent = state.intent.unwrap();
    let cloud = intent.reason == crate::replace::ReplaceReason::CloudAdopt;
    let decision = match intent.phase {
        Phase::WalIntent | Phase::WalActivated => return Err(failure("WAL intent lacks its authority")),
        Phase::Activated => {
            if cloud {
                activate(live, binding)?;
                return cleanup_completed_media(live, &intent.candidate.binding);
            }
            let expected = if intent.reason.requires_publication_decision() {
                PublicationDecision::Pending
            } else {
                intent.prior.decision
            };
            if state.active != intent.candidate || state.active.decision != expected {
                return Err(failure("invalid activated record"));
            }
            intent.candidate.clone()
        }
        Phase::Intent => {
            // No live writer is permitted in this phase. A surviving WAL or
            // journal makes main-file-only proof insufficient.
            for suffix in ["-wal", "-journal"] {
                let mut name = live.as_os_str().to_owned();
                name.push(suffix);
                match std::fs::symlink_metadata(PathBuf::from(name)) {
                    Ok(m) if !m.is_file() || m.len() != 0 => {
                        return Err(failure("unfinished intent has live journal bytes"));
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            let current = fingerprint_native(live)?;
            let file = current.as_ref().map(|(bytes, _)| bytes.clone());
            let current_native = current.map(|(_, native)| native);
            let old = file == intent.old_file && (intent.old_native.is_none() || current_native == intent.old_native);
            let candidate = file.as_ref() == Some(&intent.candidate_file)
                && (intent.candidate_native.is_none() || current_native == intent.candidate_native);
            match (old, candidate) {
                (true, false) => {
                    if cloud && (!binding.certain() || &intent.prior.binding != binding) {
                        return Err(failure("prior adoption binding no longer matches"));
                    }
                    intent.prior.clone()
                }
                (false, true) => {
                    // A crash after rename may have preceded directory sync.
                    meguri_storage::atomic_file::sync_directory(live.parent().unwrap_or(Path::new(".")))?;
                    finish_media(live, &intent, true, clock.clone(), random.clone(), shared)?;
                    activate(live, binding)?;
                    return cleanup_completed_media(live, if cloud { &intent.candidate.binding } else { binding });
                }
                _ => {
                    return Err(failure(
                        "both or neither closed file identities match; journal retained",
                    ));
                }
            }
        }
    };
    finish_media(
        live,
        &intent,
        decision.identity == intent.candidate.identity,
        clock,
        random,
        shared,
    )?;
    // Keep decision and intent together in the first durable write. Clearing
    // the journal is a second write, never the source of the decision.
    sidecar::update_document(&path, |doc| {
        let mut current = decode(Some(doc))?.ok_or_else(|| failure("missing journal"))?;
        bound(&current, binding)?;
        current.active = decision;
        put(doc, &current)
    })?;
    complete(live, binding)?;
    cleanup_completed_media(live, binding)
}

#[cfg(test)]
pub(crate) fn begin(
    paths: (&Path, &Path),
    reason: crate::replace::ReplaceReason,
    binding: &AccountBinding,
    random: &dyn RandomSource,
    recovery: (
        Option<meguri_storage::media::CollectionMediaIntent>,
        Option<RawRestoreAuthority>,
    ),
    cancel: &dyn Fn() -> bool,
    before_record: impl FnOnce(),
) -> Result<(), StorageError> {
    begin_with_cloud(
        paths,
        reason,
        binding,
        random,
        recovery,
        None,
        None,
        cancel,
        before_record,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_with_cloud(
    paths: (&Path, &Path),
    reason: crate::replace::ReplaceReason,
    binding: &AccountBinding,
    random: &dyn RandomSource,
    recovery: (
        Option<meguri_storage::media::CollectionMediaIntent>,
        Option<RawRestoreAuthority>,
    ),
    cloud: Option<crate::replace::CloudAdoptInput>,
    safety_backup: Option<ReplacementBackup>,
    cancel: &dyn Fn() -> bool,
    before_record: impl FnOnce(),
) -> Result<(), StorageError> {
    crate::replace::require_adopt_input(reason, cloud.as_ref()).map_err(|_| failure("invalid adoption input"))?;
    let (live, candidate) = paths;
    let (media, raw) = recovery;
    if let Some(input) = &cloud {
        // With no old file there is no old generation to allocate. The caller
        // operation names this absent prior view; adoption draws no identities.
        sidecar::update_document(&sidecar_path(live), |doc| {
            if let Some(state) = decode(Some(doc))? {
                return open_bound(&state, binding);
            }
            if !binding.certain() {
                return Err(failure("uncertain prior binding"));
            }
            put(
                doc,
                &State {
                    active: Publication {
                        identity: input.operation,
                        binding: binding.clone(),
                        decision: PublicationDecision::Clear,
                    },
                    intent: None,
                    replacements: Vec::new(),
                    extra: BTreeMap::new(),
                },
            )
        })?;
    } else {
        ensure(live, binding, random)?;
    }
    let old_identity = fingerprint_native_checked(live, cancel, false)?;
    let (candidate_file, candidate_native) =
        fingerprint_native_checked(candidate, cancel, false)?.ok_or_else(|| failure("candidate absent"))?;
    let (old_file, old_native) = match old_identity {
        Some((b, n)) => (Some(b), Some(n)),
        None => (None, None),
    };
    let candidate_native = Some(candidate_native);
    before_record();
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing identity"))?;
        if cloud.is_some() {
            open_bound(&state, binding)?;
        } else {
            bound(&state, binding)?;
        }
        if state.intent.is_some() {
            return Err(failure("unfinished replacement"));
        }
        let candidate = if let Some(input) = &cloud {
            input.candidate.clone()
        } else {
            Publication {
                identity: PublicationId::successor(random, state.active.identity),
                binding: binding.clone(),
                decision: if reason.requires_publication_decision() {
                    PublicationDecision::Pending
                } else {
                    state.active.decision
                },
            }
        };
        if candidate.identity == state.active.identity
            || retained_replacements(&state)?.iter().any(|record| {
                record.prior.identity == candidate.identity || record.candidate.identity == candidate.identity
            })
        {
            return Err(failure("publication identity was already used"));
        }
        let operation = cloud
            .as_ref()
            .map(|input| input.operation)
            .unwrap_or_else(|| PublicationId::successor(random, candidate.identity));
        let wal = raw
            .map(|raw| {
                if reason != crate::replace::ReplaceReason::Restore || media.is_some() {
                    return Err(failure("raw WAL authority is Restore-only"));
                }
                retained_wal::WalRecovery::prepare(live, raw, operation, old_file.as_ref(), &candidate_file)
            })
            .transpose()?;
        let phase = if wal.is_some() { Phase::WalIntent } else { Phase::Intent };
        state.intent = Some(Intent {
            operation,
            reason,
            prior: state.active.clone(),
            prior_pending_decision_reason: pending_reason(&state.extra)?,
            candidate,
            old_file,
            candidate_file,
            phase,
            verified_at: None,
            wal,
            old_native,
            candidate_native,
            media,
            safety_backup,
        });
        put(doc, &state)
    })
}
/// Bind the sound staged-file verdict before any live file action. The R6
/// completion writes it only for proven candidate activation, after WAL recovery.
pub(crate) fn record_verified_check(live: &Path, instant: i64) -> Result<(), StorageError> {
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing verified replacement intent"))?;
        state
            .intent
            .as_mut()
            .ok_or_else(|| failure("missing verified replacement intent"))?
            .verified_at = Some(instant);
        put(doc, &state)
    })
}
fn record_media(live: &Path, media: &meguri_storage::media::CollectionMediaIntent) -> Result<(), StorageError> {
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing media intent"))?;
        let intent = state.intent.as_mut().ok_or_else(|| failure("missing media intent"))?;
        if intent.media.as_ref().map(|m| m.recovery_reference()) != Some(media.recovery_reference()) {
            return Err(failure("media operation changed"));
        }
        intent.media = Some(media.clone());
        put(doc, &state)
    })
}
pub(crate) fn verify_swap_files(live: &Path, candidate: &Path, cancel: &dyn Fn() -> bool) -> Result<(), StorageError> {
    let state =
        sidecar::read_document(&sidecar_path(live), decode)?.ok_or_else(|| failure("missing final swap evidence"))?;
    let intent = state.intent.ok_or_else(|| failure("missing final swap intent"))?;
    retained_wal::verify_retired(live, &intent)?;
    let old = fingerprint_native_checked(live, cancel, true)?;
    let incoming =
        fingerprint_native_checked(candidate, cancel, true)?.ok_or_else(|| failure("final candidate missing"))?;
    if old.as_ref().map(|(b, _)| b) != intent.old_file.as_ref()
        || old.as_ref().map(|(_, n)| n) != intent.old_native.as_ref()
        || incoming.0 != intent.candidate_file
        || Some(&incoming.1) != intent.candidate_native.as_ref()
    {
        return Err(failure("closed swap file identities changed"));
    }
    Ok(())
}
pub(crate) fn owns_media_intent(live: &Path, reference: &str) -> Result<bool, StorageError> {
    sidecar::read_document(&sidecar_path(live), |doc| {
        Ok(decode(doc)?
            .and_then(|state| state.intent)
            .and_then(|intent| intent.media)
            .is_some_and(|media| media.recovery_reference() == reference))
    })
}
pub(crate) fn media_activation(live: &Path, reference: &str) -> Result<Option<bool>, StorageError> {
    sidecar::read_document(&sidecar_path(live), |doc| {
        let Some(state) = decode(doc)? else { return Ok(None) };
        let Some(list) = state.extra.get("collectionMediaReplacements").and_then(Value::as_array) else {
            return Ok(None);
        };
        list.iter()
            .rev()
            .find(|v| v.get("root").and_then(Value::as_str) == Some(reference))
            .map(|v| {
                v.get("candidateActive")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| failure("invalid completed media evidence"))
            })
            .transpose()
    })
}
pub(crate) fn publish_media(
    live: &Path,
    media: &mut meguri_storage::media::CollectionMediaIntent,
    store: &meguri_storage::media::MediaStore,
    cancel: &dyn Fn() -> bool,
) -> Result<(), meguri_storage::media::MediaError> {
    media.publish(
        store,
        |m| {
            record_media(live, m).map_err(|_| {
                meguri_storage::media::MediaError::InvalidManifest("media progress could not be recorded".into())
            })
        },
        cancel,
    )
}
fn finish_media(
    live: &Path,
    intent: &Intent,
    candidate: bool,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
    shared: Option<&meguri_storage::media::MediaStore>,
) -> Result<(), StorageError> {
    let Some(mut media) = intent.media.clone() else {
        return Ok(());
    };
    media
        .validate_initial_admission(live)
        .map_err(|e| failure(e.to_string()))?;
    let opened;
    let store = match shared {
        Some(store) => store,
        None => {
            opened = media
                .open_recovery_store(live, clock, random)
                .map_err(|e| failure(e.to_string()))?;
            &opened
        }
    };
    if candidate {
        publish_media(live, &mut media, store, &|| false).map_err(|e| failure(e.to_string()))?;
    } else {
        media.rollback(store).map_err(|e| failure(e.to_string()))?;
    }
    // Evidence survives delivery failure and future replacements. In particular
    // a backup's old-name/trash mapping is independent of backup retention.
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing media evidence"))?;
        let key = "collectionMediaReplacements";
        let mut value = serde_json::to_value(&media).map_err(failure)?;
        let record = value.as_object_mut().ok_or_else(|| failure("invalid media evidence"))?;
        record.insert(
            "operation".into(),
            serde_json::to_value(intent.operation).map_err(failure)?,
        );
        record.insert("candidateActive".into(), candidate.into());
        record.insert(
            "priorPublication".into(),
            serde_json::to_value(&intent.prior).map_err(failure)?,
        );
        record.insert(
            "candidatePublication".into(),
            serde_json::to_value(&intent.candidate).map_err(failure)?,
        );
        let list = state
            .extra
            .entry(key.into())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| failure("invalid media evidence list"))?;
        if let Some(prior) = list.iter_mut().find(|v| v.get("operation") == value.get("operation")) {
            *prior = value;
        } else {
            list.push(value);
        }
        put(doc, &state)
    })
}
fn cleanup_completed_media(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    let state = sidecar::read_document(&sidecar_path(live), decode)?
        .ok_or_else(|| failure("missing media completion state"))?;
    bound(&state, binding)?;
    if state.intent.is_some() {
        return Err(failure("media cleanup still has an active replacement"));
    }
    let Some(records) = state.extra.get("collectionMediaReplacements") else {
        return Ok(());
    };
    for record in records
        .as_array()
        .ok_or_else(|| failure("invalid media evidence list"))?
    {
        if record.get("completed").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let operation: PublicationId = serde_json::from_value(
            record
                .get("operation")
                .cloned()
                .ok_or_else(|| failure("missing media operation"))?,
        )
        .map_err(failure)?;
        let prior: Publication = serde_json::from_value(
            record
                .get("priorPublication")
                .cloned()
                .ok_or_else(|| failure("missing prior media publication"))?,
        )
        .map_err(failure)?;
        let candidate: Publication = serde_json::from_value(
            record
                .get("candidatePublication")
                .cloned()
                .ok_or_else(|| failure("missing candidate media publication"))?,
        )
        .map_err(failure)?;
        if !prior.binding.certain()
            || prior.binding != candidate.binding
            || prior.identity == candidate.identity
            || operation == prior.identity
            || operation == candidate.identity
            || record.get("candidateActive").and_then(Value::as_bool).is_none()
        {
            return Err(failure("invalid media completion witness"));
        }
        let media: meguri_storage::media::CollectionMediaIntent =
            serde_json::from_value(record.clone()).map_err(failure)?;
        media
            .cleanup_initial_admission(live)
            .map_err(|e| failure(e.to_string()))?;
    }
    Ok(())
}
fn retained_replacements(state: &State) -> Result<Vec<ReplacementEvidence>, StorageError> {
    if !state.replacements.is_empty() {
        return Ok(state.replacements.clone());
    }
    state
        .extra
        .get("lastReplacement")
        .cloned()
        .map(|value| {
            serde_json::from_value(value)
                .map(|e| vec![e])
                .map_err(|_| failure("unreadable activation evidence"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn activate(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing activation journal"))?;
        bound(&state, binding)?;
        let mut retained = retained_replacements(&state)?;
        let previous_cloud_proof = if state
            .intent
            .as_ref()
            .is_some_and(|intent| intent.reason == crate::replace::ReplaceReason::CloudAdopt)
        {
            state
                .extra
                .get("cloudAdoptActivation")
                .map(typed::<CloudAdoptActivationRecord>)
                .transpose()?
        } else {
            None
        };
        let keep_pending = state.intent.as_ref().is_some_and(|intent| {
            intent.reason == crate::replace::ReplaceReason::CloudAdopt
                && intent.phase == Phase::Activated
                && state.active.decision == PublicationDecision::Pending
        });
        let intent = state.intent.as_mut().ok_or_else(|| failure("missing intent"))?;
        state.active = activation_decision(intent, binding);
        if keep_pending {
            state.active.decision = PublicationDecision::Pending;
        }
        if intent.wal.is_some() {
            intent.phase = Phase::WalActivated;
        } else {
            intent.phase = Phase::Activated;
        }
        let evidence = ReplacementEvidence {
            reason: intent.reason,
            prior: intent.prior.clone(),
            candidate: intent.candidate.clone(),
            candidate_file: intent.candidate_file.clone(),
            old_file: intent.old_file.clone(),
            safety_backup: intent.safety_backup.clone(),
        };
        let cloud_proof = if intent.reason == crate::replace::ReplaceReason::CloudAdopt {
            let native = intent
                .candidate_native
                .clone()
                .ok_or_else(|| failure("missing candidate native identity"))?;
            let proof = CloudAdoptActivationRecord {
                operation: intent.operation,
                replacement: evidence.clone(),
                native,
            };
            if previous_cloud_proof.as_ref() != Some(&proof) {
                // R6 owns the closed candidate here. Reuse its descriptor-bound
                // verifier; never compare the DELETE-mode digest after SQLite
                // has reopened it in WAL mode or performed a day write.
                let measured = fingerprint_native(live)?.ok_or_else(|| failure("activated candidate absent"))?;
                if measured.0 != evidence.candidate_file || measured.1 != proof.native {
                    return Err(failure("activated candidate differs from R6 proof"));
                }
            }
            Some(proof)
        } else {
            None
        };
        if retained.last() != Some(&evidence) {
            retained.push(evidence.clone());
        }
        let reason = if intent.reason == crate::replace::ReplaceReason::CloudAdopt {
            None
        } else if intent.reason.requires_publication_decision() {
            replacement_reason(intent.reason)
        } else {
            intent.prior_pending_decision_reason
        };
        set_pending_reason(&mut state, reason)?;
        state.replacements = retained;
        if let Some(proof) = cloud_proof {
            state.extra.insert(
                "cloudAdoptActivation".into(),
                serde_json::to_value(proof).map_err(failure)?,
            );
        }
        state.extra.insert(
            "lastReplacement".into(),
            serde_json::to_value(evidence).map_err(failure)?,
        );
        put(doc, &state)
    })?;
    meguri_model::faultpoint!("replace.after_durable_activation")?;
    complete(live, binding)
}
fn complete(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    meguri_model::faultpoint!("replace.media.before_completion")?;
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing completed journal"))?;
        bound(&state, binding)?;
        retained_wal::complete_wal(live, &mut state)?;
        if let Some(intent) = state.intent.as_ref()
            && state.active == intent.prior
        {
            let reason = intent.prior_pending_decision_reason;
            set_pending_reason(&mut state, reason)?;
        }
        if let Some(intent) = state.intent.as_ref()
            && state.active.identity == intent.candidate.identity
            && state.active.binding == intent.candidate.binding
            && let Some(instant) = intent.verified_at
        {
            let value = serde_json::json!({"instant": instant, "passed": true});
            if let Some(existing) = state.extra.get_mut("lastQuickCheck") {
                typed::<LastQuickCheck>(existing)?;
                merge_fields(
                    existing
                        .as_object_mut()
                        .ok_or_else(|| failure("invalid quick check record"))?,
                    value.as_object().unwrap().clone(),
                );
            } else {
                state.extra.insert("lastQuickCheck".into(), value);
            }
        }
        if let Some(intent) = state.intent.take()
            && let Some(media) = intent.media
        {
            let operation = serde_json::to_value(intent.operation).map_err(failure)?;
            let record = state
                .extra
                .get_mut("collectionMediaReplacements")
                .and_then(Value::as_array_mut)
                .and_then(|records| {
                    records
                        .iter_mut()
                        .find(|record| record.get("operation") == Some(&operation))
                })
                .ok_or_else(|| failure("missing media completion evidence"))?;
            let candidate = record
                .get("candidateActive")
                .and_then(Value::as_bool)
                .ok_or_else(|| failure("missing media completion decision"))?;
            let expected = if candidate { &intent.candidate } else { &intent.prior };
            if &state.active != expected
                || record.get("root").and_then(Value::as_str) != Some(media.recovery_reference())
                || record.get("priorPublication") != Some(&serde_json::to_value(&intent.prior).map_err(failure)?)
                || record.get("candidatePublication")
                    != Some(&serde_json::to_value(&intent.candidate).map_err(failure)?)
            {
                return Err(failure("media completion publication differs"));
            }
            record
                .as_object_mut()
                .ok_or_else(|| failure("invalid media completion record"))?
                .insert("completed".into(), true.into());
        }
        put(doc, &state)
    })?;
    meguri_model::faultpoint!("replace.media.after_completion")?;
    Ok(())
}

/// Continues the just-recorded R6 Restore intent; ordinary replacement never calls this.
pub(crate) fn retire_restore_wal(live: &Path) -> Result<(), StorageError> {
    retained_wal::retire(live)
}

pub(crate) fn finish_restore_wal(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    let state = sidecar::read_document(&sidecar_path(live), decode)?.ok_or_else(|| failure("missing WAL state"))?;
    bound(&state, binding)?;
    retained_wal::reconcile_wal(live, state, binding)
}

#[cfg(test)]
mod maintenance_tests {
    use super::*;
    use meguri_model::{rng::SeededRandom, time::EpochMillis};
    use serde_json::json;

    fn owner() -> (tempfile::TempDir, DeviceState) {
        let dir = tempfile::tempdir().unwrap();
        let state = DeviceState::for_collection(&dir.path().join("collection.anki2"));
        (dir, state)
    }
    fn initialized() -> (tempfile::TempDir, DeviceState) {
        let (dir, state) = owner();
        ensure(
            &dir.path().join("collection.anki2"),
            &AccountBinding::LocalOnly,
            &SeededRandom::new(1),
        )
        .unwrap();
        (dir, state)
    }
    fn save(state: &DeviceState, doc: Value) {
        std::fs::write(&state.path, serde_json::to_vec(&doc).unwrap()).unwrap();
    }
    fn document(state: &DeviceState) -> Value {
        serde_json::from_slice(&std::fs::read(&state.path).unwrap()).unwrap()
    }
    fn backup() -> LastBackup {
        LastBackup {
            snapshot_mod: -11,
            instant: i64::MIN,
            name: "synthetic.colpkg".into(),
        }
    }
    fn problems() -> Value {
        json!({
            "card_properties": 0, "new_positions_too_high": 1,
            "card_last_review_time": 2, "cards_without_note": 3,
            "decks_missing": 4, "field_counts": 5, "duplicate_card_ords": 6,
            "cards_without_template": 7, "revlog_properties": 8,
            "notes_invalid_utf8": 9, "future_ids": 10, "notetypes_recovered": u64::MAX,
        })
    }

    #[test]
    fn absent_records_are_optional_and_metadata_does_not_create_publication_identity() {
        let (_dir, state) = owner();
        assert_eq!(state.last_backup().unwrap(), None);
        assert_eq!(state.last_check_database().unwrap(), None);
        assert_eq!(state.last_optimize().unwrap(), None);
        assert_eq!(state.last_quick_check().unwrap(), None);
        assert!(!state.path.exists(), "reads do not create a sidecar");
        save(&state, json!({"other": 7}));
        assert_eq!(state.last_backup().unwrap(), None);
        assert!(
            state.set_last_backup(&backup()).is_err(),
            "only lifecycle initialization creates identity"
        );
        assert_eq!(document(&state), json!({"other": 7}));
        save(&state, json!({"deviceState": {}}));
        assert_eq!(state.last_backup().unwrap(), None);
        assert!(state.set_last_optimize(EpochMillis(0)).is_err());
        assert_eq!(document(&state), json!({"deviceState": {}}));
        std::fs::remove_file(&state.path).unwrap();
        ensure(&state.live, &AccountBinding::LocalOnly, &SeededRandom::new(1)).unwrap();
        let publication = document(&state)["deviceState"]["active"].clone();
        state.set_last_backup(&backup()).unwrap();
        state.set_last_optimize(EpochMillis(i64::MAX)).unwrap();
        state
            .set_last_quick_check(&LastQuickCheck {
                instant: -1,
                passed: false,
            })
            .unwrap();
        assert_eq!(state.last_backup().unwrap(), Some(backup()));
        assert_eq!(state.last_optimize().unwrap(), Some(EpochMillis(i64::MAX)));
        assert_eq!(
            state.last_quick_check().unwrap(),
            Some(LastQuickCheck {
                instant: -1,
                passed: false
            })
        );
        assert_eq!(document(&state)["deviceState"]["active"], publication);
    }

    #[test]
    fn valid_active_without_intent_is_rejected_and_preserves_bytes() {
        let (_dir, state) = initialized();
        let mut doc = document(&state);
        doc["deviceState"].as_object_mut().unwrap().remove("intent");
        save(&state, doc);
        let before = std::fs::read(&state.path).unwrap();

        assert!(state.last_backup().is_err());
        assert!(state.last_check_database().is_err());
        assert!(state.last_optimize().is_err());
        assert!(state.last_quick_check().is_err());
        assert_eq!(std::fs::read(&state.path).unwrap(), before);

        assert!(state.set_last_backup(&backup()).is_err());
        assert!(
            state
                .set_last_check_database(&LastCheckDatabase {
                    instant: 1,
                    result: CheckDatabaseResult::Corrupt,
                })
                .is_err()
        );
        assert!(state.set_last_optimize(EpochMillis(1)).is_err());
        assert!(
            state
                .set_last_quick_check(&LastQuickCheck {
                    instant: 1,
                    passed: true
                })
                .is_err()
        );
        assert_eq!(std::fs::read(&state.path).unwrap(), before);
    }

    #[test]
    fn malformed_document_state_and_records_are_errors_and_setters_preserve_bytes() {
        let (_dir, state) = owner();
        let mut bad = vec![
            json!(null),
            json!([]),
            json!({"deviceState": null}),
            json!({"deviceState": 1}),
            json!({"deviceState": {"active": null}}),
        ];
        for record in [
            json!(null),
            json!(false),
            json!({}),
            json!({"snapshotMod": 1, "instant": 2}),
            json!({"snapshotMod": true, "instant": 2, "name": "x"}),
            json!({"snapshotMod": 1, "instant": 2.0, "name": "x"}),
            json!({"snapshotMod": 1, "instant": u64::MAX, "name": "x"}),
            json!({"snapshotMod": 1, "instant": 2, "name": false}),
        ] {
            bad.push(json!({"deviceState": {"lastBackup": record}}));
        }
        for record in [
            json!(null),
            json!({}),
            json!({"instant": 0}),
            json!({"instant": 0, "passed": 0}),
            json!({"instant": 0, "passed": "true"}),
            json!({"instant": true, "passed": false}),
        ] {
            bad.push(json!({"deviceState": {"lastQuickCheck": record}}));
        }
        for value in [json!(null), json!(true), json!(1.5), json!(u64::MAX)] {
            bad.push(json!({"deviceState": {"lastOptimize": value}}));
        }
        for doc in bad {
            save(&state, doc);
            let bytes = std::fs::read(&state.path).unwrap();
            assert!(state.last_backup().is_err());
            assert!(state.last_check_database().is_err());
            assert!(state.last_optimize().is_err());
            assert!(state.last_quick_check().is_err());
            assert!(state.set_last_backup(&backup()).is_err());
            assert!(state.set_last_optimize(EpochMillis(1)).is_err());
            assert!(
                state
                    .set_last_quick_check(&LastQuickCheck {
                        instant: 1,
                        passed: true
                    })
                    .is_err()
            );
            assert_eq!(std::fs::read(&state.path).unwrap(), bytes);
        }
        std::fs::write(&state.path, b"{truncated").unwrap();
        assert!(state.last_backup().is_err());
        assert!(state.set_last_backup(&backup()).is_err());
        assert_eq!(std::fs::read(&state.path).unwrap(), b"{truncated");
        std::fs::remove_file(&state.path).unwrap();
        std::fs::create_dir(&state.path).unwrap();
        assert!(state.last_backup().is_err(), "unreadable is not absent");
    }

    #[test]
    fn database_check_variants_require_all_counts_and_exact_types() {
        let (_dir, state) = owner();
        let mut invalid_results = vec![
            json!(null),
            json!({}),
            json!({"error": null}),
            json!({"error": "other"}),
            json!({"error": "corrupt", "card_properties": 0}),
        ];
        for value in [json!(-1), json!(true), json!(1.0), json!(null), json!("2")] {
            let mut result = problems();
            result["future_ids"] = value;
            invalid_results.push(result);
        }
        let mut incomplete = problems();
        incomplete.as_object_mut().unwrap().remove("field_counts");
        invalid_results.push(incomplete);
        for result in invalid_results {
            save(
                &state,
                json!({"deviceState": {"lastCheckDatabase": {"instant": 0, "result": result}}}),
            );
            let before = std::fs::read(&state.path).unwrap();
            assert!(state.last_check_database().is_err());
            assert!(
                state
                    .set_last_check_database(&LastCheckDatabase {
                        instant: 3,
                        result: CheckDatabaseResult::Corrupt,
                    })
                    .is_err()
            );
            assert_eq!(std::fs::read(&state.path).unwrap(), before);
        }
        for record in [
            json!(null),
            json!({"result": {"error": "corrupt"}}),
            json!({"instant": true, "result": {"error": "corrupt"}}),
            json!({"instant": u64::MAX, "result": {"error": "corrupt"}}),
        ] {
            save(&state, json!({"deviceState": {"lastCheckDatabase": record}}));
            assert!(state.last_check_database().is_err());
        }
    }

    #[test]
    fn variant_changes_preserve_unknown_record_result_and_document_members() {
        let (_dir, state) = initialized();
        let mut result = problems();
        result["futureProblem"] = json!({"opaque": [1, null]});
        let additions = json!({"futureDocument": [7], "deviceState": {
            "futureState": {"a": 8},
            "lastBackup": {"snapshotMod": 0, "instant": 0, "name": "old", "futureBackup": 9},
            "lastQuickCheck": {"instant": 0, "passed": true, "futureQuick": 11},
            "lastCheckDatabase": {"instant": 0, "result": result, "futureCheck": 10}
        }});
        let mut initial = document(&state);
        initial["futureDocument"] = additions["futureDocument"].clone();
        initial["deviceState"]
            .as_object_mut()
            .unwrap()
            .extend(additions["deviceState"].as_object().unwrap().clone());
        save(&state, initial);
        let counts = state.last_check_database().unwrap().unwrap().result;
        if let CheckDatabaseResult::Problems(ref counts) = counts {
            assert_eq!(counts.notetypes_recovered, u64::MAX);
        } else {
            panic!("expected problem summary");
        }
        for variant in [CheckDatabaseResult::Corrupt, counts] {
            let record = LastCheckDatabase {
                instant: -5,
                result: variant,
            };
            state.set_last_check_database(&record).unwrap();
            assert_eq!(state.last_check_database().unwrap(), Some(record));
            let doc = document(&state);
            assert_eq!(
                doc["deviceState"]["lastCheckDatabase"]["result"]["futureProblem"],
                json!({"opaque": [1, null]})
            );
            assert_eq!(doc["deviceState"]["lastCheckDatabase"]["futureCheck"], 10);
        }
        state.set_last_backup(&backup()).unwrap();
        state
            .set_last_quick_check(&LastQuickCheck {
                instant: 1,
                passed: false,
            })
            .unwrap();
        let doc = document(&state);
        assert_eq!(doc["futureDocument"], json!([7]));
        assert_eq!(doc["deviceState"]["futureState"], json!({"a": 8}));
        assert_eq!(doc["deviceState"]["lastBackup"]["futureBackup"], 9);
        assert_eq!(doc["deviceState"]["lastQuickCheck"]["futureQuick"], 11);
        assert!(doc["deviceState"]["lastCheckDatabase"]["result"].get("error").is_none());
    }

    #[test]
    fn updates_preserve_pending_replacement_intent_and_publication_unknowns() {
        let (dir, state) = owner();
        let live = dir.path().join("collection.anki2");
        let candidate = dir.path().join("candidate.anki2");
        std::fs::write(&live, b"old synthetic closed file").unwrap();
        std::fs::write(&candidate, b"candidate synthetic closed file").unwrap();
        begin(
            (&live, &candidate),
            crate::replace::ReplaceReason::Restore,
            &AccountBinding::LocalOnly,
            &SeededRandom::new(4),
            (None, None),
            &|| false,
            || {},
        )
        .unwrap();
        let mut before = document(&state);
        before["deviceState"]["active"]["futurePublication"] = json!([1, 2]);
        before["deviceState"]["intent"]["futureIntent"] = json!(7);
        save(&state, before.clone());
        state.set_last_backup(&backup()).unwrap();
        state.set_last_optimize(EpochMillis(7)).unwrap();
        let mut after = document(&state);
        for key in ["lastBackup", "lastOptimize"] {
            after["deviceState"].as_object_mut().unwrap().remove(key);
        }
        assert_eq!(after, before, "maintenance cannot change a publication fence or intent");
        let mut invalid = before;
        invalid["deviceState"]["active"]["decision"] = json!("Pending");
        save(&state, invalid);
        let bytes = std::fs::read(&state.path).unwrap();
        assert!(
            state.set_last_backup(&backup()).is_err(),
            "inconsistent R6 remains an error"
        );
        assert_eq!(std::fs::read(&state.path).unwrap(), bytes);
    }

    #[test]
    fn independent_sidecar_writers_share_the_durable_lock() {
        let (_dir, state) = initialized();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        std::thread::scope(|scope| {
            for lane in 0..4 {
                let state = state.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    for instant in 0..16 {
                        match lane {
                            0 => state.set_last_backup(&backup()).unwrap(),
                            1 => state.set_last_optimize(EpochMillis(instant)).unwrap(),
                            2 => state
                                .set_last_quick_check(&LastQuickCheck { instant, passed: true })
                                .unwrap(),
                            _ => sidecar::update_document(&state.path, |doc| {
                                doc.insert("independent".into(), instant.into());
                                Ok(())
                            })
                            .unwrap(),
                        }
                    }
                });
            }
        });
        assert_eq!(state.last_backup().unwrap(), Some(backup()));
        assert_eq!(state.last_optimize().unwrap(), Some(EpochMillis(15)));
        assert_eq!(state.last_quick_check().unwrap().unwrap().instant, 15);
        assert_eq!(document(&state)["independent"], 15);
    }

    #[test]
    fn failed_durable_publication_is_reported_and_retains_prior_record() {
        use meguri_model::faultpoint::{self, Fault};
        let (_dir, state) = initialized();
        state.set_last_backup(&backup()).unwrap();
        let before = std::fs::read(&state.path).unwrap();
        thread_local! {
            static DESTINATION: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
        }
        fn replace_destination_with_directory() {
            DESTINATION.with(|slot| {
                if let Some(path) = slot.borrow_mut().take() {
                    std::fs::rename(&path, path.with_extension("saved")).unwrap();
                    std::fs::create_dir(path).unwrap();
                }
            });
        }
        DESTINATION.with(|slot| *slot.borrow_mut() = Some(state.path.clone()));
        faultpoint::arm(
            "storage.atomic.before_rename",
            Fault::Callback(replace_destination_with_directory),
        );
        let result = state.set_last_backup(&LastBackup {
            instant: 17,
            ..backup()
        });
        faultpoint::disarm("storage.atomic.before_rename");
        let saved = state.path.with_extension("saved");
        assert_eq!(std::fs::read(&saved).unwrap(), before);
        std::fs::remove_dir(&state.path).unwrap();
        std::fs::rename(saved, &state.path).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(&state.path).unwrap(), before);
        assert_eq!(state.last_backup().unwrap(), Some(backup()));
    }
}
