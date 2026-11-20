//! Installation-local manual-transfer history, overlap guard and success receipts.
//! No scheduler, transport, credential store or cloud-publication authority.
use crate::Endpoint;
use meguri_storage::{StorageError, sidecar};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod settle;
pub use settle::{UploadSettlement, judge_upload};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef(String);
impl AccountRef {
    pub(crate) fn media_owner(&self) -> meguri_storage::media::MediaBaselineOwner {
        meguri_storage::media::MediaBaselineOwner::new(self.0.clone())
    }
    /// Logical identity is the normalized CONFIGURED endpoint and trimmed,
    /// case-preserved username. Neither a key nor a redirect changes it.
    pub fn derive(configured: &Endpoint, username: &str) -> Option<Self> {
        let username = username.trim();
        if username.is_empty() {
            return None;
        }
        let mut hash = Sha256::new();
        hash.update(b"meguri-manual-account-v1");
        for text in [configured.as_str(), username] {
            hash.update((text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        }
        Some(Self(hex(&hash.finalize())))
    }
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationId(pub(crate) String);
impl OperationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Import,
    Export,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    CollectionOnly,
    CollectionAndMedia,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Preparing,
    Preview,
    ConfirmedIntent,
    Transferring,
    RemoteAccepted,
    Activating,
    CollectionComplete,
    MediaIncomplete,
    /// Terminal partial outcome: a proven local replacement superseded media.
    Superseded,
    Complete,
    Cancelled,
    Failed,
    NeedsDecision,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Acceptance {
    NotSent,
    PossiblyAccepted,
    ExactOk,
    Refused,
    /// `sync/meguri-provider#MP6.9` "neither": a later read found the server
    /// holding neither what this upload sent nor what it was to replace.
    /// Terminal and never active; every artifact of the record is kept.
    Overtaken,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Boundary {
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub length: u64,
    pub sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub name: String,
    pub identity: FileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGuard {
    /// Process incarnation; process generation/seq equality after restart is insufficient.
    pub process: [u64; 2],
    pub generation: u64,
    pub admitted: u64,
    pub committed: u64,
    pub publication: Option<meguri_engine::device_state::Publication>,
    pub closed_main: Option<FileIdentity>,
    pub closed_wal: Option<FileIdentity>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteFacts {
    pub modified: i64,
    pub schema_modified: i64,
    pub usn: i32,
    pub empty: bool,
    pub media_usn: i32,
}
impl From<&crate::wire::meta::ServerMeta> for RemoteFacts {
    fn from(m: &crate::wire::meta::ServerMeta) -> Self {
        Self {
            modified: m.modified.0,
            schema_modified: m.schema_modified.0,
            usn: m.usn.0,
            empty: m.empty,
            media_usn: m.media_usn,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalFacts {
    pub modified: i64,
    pub schema_modified: i64,
    pub last_sync: i64,
    pub usn: i32,
    pub cards: u64,
    pub notes: u64,
    pub reviews: u64,
    pub pending_reviews: u64,
    /// notes, cards, decks, deck_config, notetypes, tags, config, graves.
    pub pending_rows: [u64; 8],
    pub last_review: Option<i64>,
}
impl From<meguri_storage::one_way::LocalTransferFacts> for LocalFacts {
    fn from(f: meguri_storage::one_way::LocalTransferFacts) -> Self {
        let p = f.pending_rows;
        Self {
            modified: f.stamps.modified.0,
            schema_modified: f.stamps.schema_modified.0,
            last_sync: f.stamps.last_sync.0,
            usn: f.stamps.usn.0,
            cards: f.totals.cards,
            notes: f.totals.notes,
            reviews: f.totals.reviews,
            pending_reviews: f.pending_reviews,
            last_review: f.last_review.map(|t| t.0),
            pending_rows: [
                p.content.notes,
                p.content.cards,
                p.definitions.decks,
                p.definitions.deck_config,
                p.definitions.notetypes,
                p.definitions.tags,
                p.config,
                p.graves,
            ],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyKind {
    VerifiedPackage,
    UnverifiedRaw,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerCopy {
    pub artifact: Artifact,
    pub kind: CopyKind,
    pub published_at: i64,
    /// Unknown for unverified and legacy-schema files; never inferred from empty.
    pub served_facts: Option<LocalFacts>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuccessReceipt {
    pub completed_at: i64,
    pub next_eligible_at: i64,
}
/// Revision of the disclosed M5 effect categories, not a file manifest.
pub const MEDIA_CATEGORIES: u32 = 1;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaStage {
    #[default]
    AwaitingConsent,
    Declined,
    Running,
    Cancelled,
    Interrupted,
    Failed,
    Complete,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaConsent {
    pub revision: u64,
    pub generation: u64,
    pub categories: u32,
    pub account: AccountRef,
    pub configured_endpoint: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaCounters {
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaOutcome {
    pub stage: MediaStage,
    pub consent: Option<MediaConsent>,
    pub counters: MediaCounters,
    /// A lost response or process death may leave additional remote effects.
    pub remote_outcome_unknown: bool,
    /// Exclusion names describe the latest completed check, not old attempts.
    #[serde(default)]
    pub exclusions_verified: bool,
    /// Safe stable error category, never server text or transport details.
    pub failure_kind: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: OperationId,
    pub account: AccountRef,
    pub configured_endpoint: String,
    pub target_endpoint: String,
    pub direction: Direction,
    pub scope: Scope,
    pub phase: Phase,
    pub acceptance: Acceptance,
    pub boundary: Option<Boundary>,
    pub guard: Option<LocalGuard>,
    pub recovery_guard: Option<LocalGuard>,
    pub recovery_local: Option<LocalFacts>,
    pub local: Option<LocalFacts>,
    pub remote: Option<RemoteFacts>,
    pub candidate: Option<Artifact>,
    pub activation_identity: Option<FileIdentity>,
    pub original: Option<Artifact>,
    pub safety_backups: Vec<Artifact>,
    pub server_copy: Option<ServerCopy>,
    pub prepared_at: i64,
    pub actual_generation: Option<u64>,
    pub collection_complete: bool,
    /// Persistent R6 identity distinguishes unrelated replacements across process
    /// generation reuse. No publication decision is written by media.
    #[serde(default)]
    pub resulting_publication: Option<meguri_engine::device_state::PublicationId>,
    #[serde(default)]
    pub media: MediaOutcome,
    pub receipt: Option<SuccessReceipt>,
    /// Binds final authority to a persisted revision, not caller-supplied facts.
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    serial: u64,
    active: Option<OperationId>,
    operations: BTreeMap<String, Operation>,
    cooldowns: BTreeMap<String, SuccessReceipt>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            serial: 0,
            active: None,
            operations: BTreeMap::new(),
            cooldowns: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    Storage,
    Overlap(OperationId),
    Cooldown { next_eligible_at: i64 },
    Stale,
    Invalid,
}
impl From<StorageError> for PolicyError {
    fn from(_: StorageError) -> Self {
        Self::Storage
    }
}
#[derive(Clone)]
pub struct TransferPolicy {
    path: PathBuf,
}
impl TransferPolicy {
    /// `directory` is installation-private and stable across account/collection
    /// changes. The host creates it durably; it is never a swept temp directory.
    pub fn new(directory: &Path) -> Self {
        Self {
            path: directory.join("manual-transfers.json"),
        }
    }
    fn read(&self) -> Result<State, PolicyError> {
        sidecar::read_document(&self.path, decode).map_err(Into::into)
    }
    fn update<T>(&self, f: impl FnOnce(&mut State) -> Result<T, PolicyError>) -> Result<T, PolicyError> {
        // Return the original policy variant when the document update refuses it.
        let _ownership =
            meguri_storage::lockfile::CollectionLock::acquire(self.path.parent().ok_or(PolicyError::Invalid)?)
                .map_err(|_| PolicyError::Storage)?;
        let mut result = None;
        sidecar::update_document(&self.path, |doc| {
            let mut state = decode(Some(doc))?;
            match f(&mut state) {
                Ok(value) => {
                    doc.insert(
                        "manualTransfers".into(),
                        serde_json::to_value(&state).map_err(|_| invalid())?,
                    );
                    result = Some(Ok(value));
                    Ok(())
                }
                Err(error) => {
                    result = Some(Err(error));
                    Err(invalid())
                }
            }
        })
        .map_err(|_| {
            result
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .cloned()
                .unwrap_or(PolicyError::Storage)
        })?;
        result.unwrap_or(Err(PolicyError::Storage))
    }
    pub fn history(&self) -> Result<Vec<Operation>, PolicyError> {
        Ok(self.read()?.operations.into_values().collect())
    }
    pub fn active(&self) -> Result<Option<Operation>, PolicyError> {
        let state = self.read()?;
        Ok(state.active.and_then(|id| state.operations.get(id.as_str()).cloned()))
    }
    pub fn get(&self, id: &OperationId) -> Result<Operation, PolicyError> {
        self.read()?
            .operations
            .get(id.as_str())
            .cloned()
            .ok_or(PolicyError::Stale)
    }
    pub fn next_eligible_at(&self, _account: &AccountRef) -> Result<Option<i64>, PolicyError> {
        // This manual pipeline is custom-server only. Retained deadlines are
        // history, but the complete document must still pass validation.
        self.read()?;
        Ok(None)
    }
    pub(crate) fn begin(&self, mut operation: Operation, nonce: u64, _now: i64) -> Result<Operation, PolicyError> {
        self.update(|state| {
            if let Some(id) = &state.active {
                return Err(PolicyError::Overlap(id.clone()));
            }
            state.serial = state.serial.checked_add(1).ok_or(PolicyError::Invalid)?;
            operation.id = OperationId(format!("{:016x}-{:016x}", state.serial, nonce));
            state.active = Some(operation.id.clone());
            state.operations.insert(operation.id.0.clone(), operation.clone());
            Ok(operation)
        })
    }
    pub(crate) fn save(&self, operation: &mut Operation) -> Result<(), PolicyError> {
        let next = operation.revision.checked_add(1).ok_or(PolicyError::Invalid)?;
        self.update(|state| {
            if state.active.as_ref() != Some(&operation.id)
                || state.operations.get(operation.id.as_str()).map(|o| o.revision) != Some(operation.revision)
            {
                return Err(PolicyError::Stale);
            }
            let prior = state.operations.get(operation.id.as_str()).ok_or(PolicyError::Stale)?;
            if prior.phase == Phase::Superseded
                || operation.phase == Phase::Superseded
                || prior.scope != operation.scope
                || prior.account != operation.account
                || prior.configured_endpoint != operation.configured_endpoint
                || prior.direction != operation.direction
                || (prior.collection_complete
                    && (!operation.collection_complete
                        || (prior.resulting_publication != operation.resulting_publication
                            && !(prior.resulting_publication.is_none()
                                && operation.resulting_publication.is_some()
                                && operation.media.stage == MediaStage::AwaitingConsent
                                && operation.media.consent.is_none()))
                        || prior.target_endpoint != operation.target_endpoint
                        || (prior.actual_generation != operation.actual_generation
                            && (operation.resulting_publication.is_none()
                                || operation.media.stage != MediaStage::AwaitingConsent
                                || operation.media.consent.is_some()))))
            {
                return Err(PolicyError::Invalid);
            }
            let mut saved = operation.clone();
            saved.revision = next;
            state.operations.insert(saved.id.0.clone(), saved);
            Ok(())
        })?;
        operation.revision = next;
        Ok(())
    }
    /// Retire a partial transfer while the caller owns the replacement slot.
    /// R6 must start at the publication saved by this operation.
    pub(crate) fn retire_replaced_media(
        &self,
        expected: &Operation,
        proof: &meguri_engine::device_state::LocalReplacementSuccessor,
    ) -> Result<Operation, PolicyError> {
        let origin = proof.before();
        let destination = proof.after();
        if origin == destination || !can_supersede_partial_media(expected) {
            return Err(PolicyError::Invalid);
        }
        match expected.resulting_publication {
            Some(publication) if publication == origin => self.compare_and_supersede_partial_media(expected),
            _ => Err(PolicyError::Invalid),
        }
    }
    fn compare_and_supersede_partial_media(&self, expected: &Operation) -> Result<Operation, PolicyError> {
        self.update(|state| {
            let current = state.operations.get(expected.id.as_str()).ok_or(PolicyError::Stale)?;
            if current != expected || state.active.as_ref() != Some(&current.id) {
                return Err(PolicyError::Stale);
            }
            // Clone the complete stored record: counters, effects and unknown
            // outcomes survive; this transaction has no success-policy writes.
            let mut retired = current.clone();
            retired.phase = Phase::Superseded;
            retired.revision = retired.revision.checked_add(1).ok_or(PolicyError::Invalid)?;
            state.operations.insert(retired.id.0.clone(), retired.clone());
            state.active = None;
            Ok(retired)
        })
    }
    /// The caller retains the last successfully saved snapshot under its flight
    /// guard. Per-file UI progress with identical durable facts needs no sidecar
    /// publication. Error and terminal transitions still use save directly.
    pub(crate) fn save_snapshot(&self, operation: &mut Operation, saved: &mut Operation) -> Result<(), PolicyError> {
        if operation != saved {
            self.save(operation)?;
            saved.clone_from(operation);
        }
        Ok(())
    }
    /// Only a complete selected scope gets a receipt. Receipt and exact owner
    /// release are a SINGLE durable transition; legacy deadlines stay unchanged.
    pub(crate) fn finish(&self, operation: &mut Operation, now: i64) -> Result<(), PolicyError> {
        if !selected_complete(operation) {
            return Err(PolicyError::Invalid);
        }
        let mut saved = operation.clone();
        self.update(|state| {
            if state.active.as_ref() != Some(&saved.id) || state.operations.get(saved.id.as_str()) != Some(&saved) {
                return Err(PolicyError::Stale);
            }
            let receipt = SuccessReceipt {
                completed_at: now,
                next_eligible_at: now,
            };
            saved.phase = Phase::Complete;
            saved.boundary = None;
            saved.receipt = Some(receipt);
            saved.revision = saved.revision.checked_add(1).ok_or(PolicyError::Invalid)?;
            state.operations.insert(saved.id.0.clone(), saved.clone());
            state.active = None;
            Ok(())
        })?;
        *operation = saved;
        Ok(())
    }
    /// No dismissal/credential API can clear PossiblyAccepted or known OK that
    /// has not completed locally. A completed known refusal is safe to release.
    pub(crate) fn end_before_activation(&self, operation: &mut Operation) -> Result<(), PolicyError> {
        if !matches!(operation.acceptance, Acceptance::NotSent | Acceptance::Refused)
            || operation.phase == Phase::Activating
            || operation.activation_identity.is_some()
            || operation.collection_complete
        {
            return Err(PolicyError::Invalid);
        }
        self.save(operation)?;
        self.update(|state| {
            if state.active.as_ref() != Some(&operation.id) {
                return Err(PolicyError::Stale);
            }
            state.active = None;
            Ok(())
        })
    }
}
pub(crate) fn can_supersede_partial_media(op: &Operation) -> bool {
    match op.phase {
        Phase::Complete | Phase::Superseded => false,
        _ => partial_media_facts_permit_supersession(op),
    }
}
fn partial_media_facts_permit_supersession(op: &Operation) -> bool {
    if op.scope != Scope::CollectionAndMedia || !op.collection_complete {
        return false;
    }
    if op.resulting_publication.is_none() || op.receipt.is_some() {
        return false;
    }
    let collection_outcome_permits_supersession = match op.direction {
        Direction::Import => op.acceptance != Acceptance::PossiblyAccepted,
        Direction::Export => op.acceptance == Acceptance::ExactOk,
    };
    let unfinished_media = match op.media.stage {
        MediaStage::Running | MediaStage::Complete => false,
        MediaStage::AwaitingConsent
        | MediaStage::Declined
        | MediaStage::Cancelled
        | MediaStage::Interrupted
        | MediaStage::Failed => true,
    };
    collection_outcome_permits_supersession && unfinished_media
}
fn selected_complete(op: &Operation) -> bool {
    op.collection_complete
        && (op.scope == Scope::CollectionOnly
            || (op.media.stage == MediaStage::Complete
                && op.media.consent.as_ref().is_some_and(|c| {
                    Some(c.generation) == op.actual_generation
                        && c.account == op.account
                        && c.configured_endpoint == op.configured_endpoint
                        && c.categories == MEDIA_CATEGORIES
                        && c.revision < op.revision
                })))
}
fn invalid() -> StorageError {
    StorageError::Io {
        detail: "manual transfer metadata is unavailable or inconsistent".into(),
    }
}
fn decode(doc: Option<&serde_json::Map<String, serde_json::Value>>) -> Result<State, StorageError> {
    let state: State = match doc.and_then(|d| d.get("manualTransfers")) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| invalid())?,
        None if doc.is_none_or(|d| d.is_empty()) => State::default(),
        None => return Err(invalid()),
    };
    if state.version != 1
        || state.cooldowns.keys().any(|account| {
            account.len() != 64
                || !account
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        || state
            .active
            .as_ref()
            .is_some_and(|id| !state.operations.contains_key(id.as_str()))
    {
        return Err(invalid());
    }
    for (id, op) in &state.operations {
        if op.phase == Phase::Superseded {
            // Superseded records retain partial-media facts and cannot hold admission.
            // Validate those facts with the conditions required before supersession.
            if state.active.as_ref() == Some(&op.id) || !partial_media_facts_permit_supersession(op) {
                return Err(invalid());
            }
        }
        let unresolved = op.phase != Phase::Superseded
            && (!matches!(op.phase, Phase::Complete | Phase::Failed | Phase::Cancelled)
                || (op.acceptance == Acceptance::PossiblyAccepted)
                || (op.acceptance == Acceptance::ExactOk && op.phase != Phase::Complete)
                || (op.activation_identity.is_some() && op.phase != Phase::Complete));
        if id != op.id.as_str()
            || id.len() != 33
            || id.as_bytes()[16] != b'-'
            || !id
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 16 || b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || op.account.0.len() != 64
            || !op
                .account
                .0
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || (unresolved && state.active.as_ref() != Some(&op.id))
            || (op.acceptance == Acceptance::Overtaken
                && (op.direction != Direction::Export
                    || op.phase != Phase::Failed
                    || op.activation_identity.is_some()
                    || op.collection_complete
                    || state.active.as_ref() == Some(&op.id)))
            || (op.phase == Phase::Complete && (op.receipt.is_none() || !selected_complete(op)))
            || (op.receipt.is_some() && op.phase != Phase::Complete)
        {
            return Err(invalid());
        }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn operation(account: AccountRef, direction: Direction) -> Operation {
        Operation {
            id: OperationId(String::new()),
            account,
            configured_endpoint: "http://127.0.0.1/".into(),
            target_endpoint: "http://127.0.0.1/".into(),
            direction,
            scope: Scope::CollectionOnly,
            phase: Phase::Preparing,
            acceptance: Acceptance::NotSent,
            boundary: None,
            guard: None,
            recovery_guard: None,
            recovery_local: None,
            local: None,
            remote: None,
            candidate: None,
            activation_identity: None,
            original: None,
            safety_backups: vec![],
            server_copy: None,
            prepared_at: 0,
            actual_generation: None,
            collection_complete: false,
            resulting_publication: None,
            media: MediaOutcome::default(),
            receipt: None,
            revision: 0,
        }
    }
    // A version-1 input with the old one-hour receipt, never produced by finish.
    fn legacy_history(root: &Path, account: &AccountRef) -> Operation {
        let mut old = operation(account.clone(), Direction::Import);
        old.id = OperationId("0000000000000001-000000000000000c".into());
        old.phase = Phase::Complete;
        old.collection_complete = true;
        old.receipt = Some(SuccessReceipt {
            completed_at: 100,
            next_eligible_at: 3_600_100,
        });
        let other = AccountRef::derive(&Endpoint::parse("http://127.0.0.2/").unwrap(), "other history").unwrap();
        let state = State {
            version: 1,
            serial: 1,
            active: None,
            operations: BTreeMap::from([(old.id.0.clone(), old.clone())]),
            cooldowns: BTreeMap::from([
                (account.0.clone(), old.receipt.clone().unwrap()),
                (
                    other.0,
                    SuccessReceipt {
                        completed_at: 200,
                        next_eligible_at: 3_600_200,
                    },
                ),
            ]),
        };
        let document = serde_json::json!({"manualTransfers": state});
        std::fs::write(
            root.join("manual-transfers.json"),
            serde_json::to_vec_pretty(&document).unwrap(),
        )
        .unwrap();
        old
    }
    #[test]
    fn immediate_success_survives_restart_rotation_and_clock_rollback_with_legacy_history() {
        let root = tempfile::tempdir().unwrap();
        let base = Endpoint::parse(" HTTP://127.0.0.1:80/anki ").unwrap();
        let account = AccountRef::derive(&base, "  CaseSensitiveLogin  ").unwrap();
        assert_eq!(
            account,
            AccountRef::derive(
                &Endpoint::parse("http://127.0.0.1/anki/").unwrap(),
                "CaseSensitiveLogin"
            )
            .unwrap()
        );
        assert_ne!(account, AccountRef::derive(&base, "casesensitivelogin").unwrap());
        let old = legacy_history(root.path(), &account);
        let path = root.path().join("manual-transfers.json");
        let bytes = std::fs::read(&path).unwrap();
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut retained = vec![old];
        let fresh = AccountRef::derive(&base, "no legacy map entry").unwrap();
        for (direction, now, identity) in [
            (Direction::Export, 100, account.clone()),
            (Direction::Export, 100, account.clone()),
            (Direction::Import, 99, account.clone()),
            (Direction::Import, -1_000_000, account.clone()),
            (Direction::Export, -1_000_000, fresh),
        ] {
            let restarted = TransferPolicy::new(root.path());
            let before_read = std::fs::read(&path).unwrap();
            assert_eq!(restarted.next_eligible_at(&identity).unwrap(), None);
            assert_eq!(
                std::fs::read(&path).unwrap(),
                before_read,
                "reads must preserve historical bytes"
            );
            let mut op = restarted.begin(operation(identity, direction), 13, now).unwrap();
            let mut stale = retained.last().unwrap().clone();
            let before_stale_finish = std::fs::read(&path).unwrap();
            assert_eq!(restarted.finish(&mut stale, now), Err(PolicyError::Stale));
            assert_eq!(std::fs::read(&path).unwrap(), before_stale_finish);
            assert_eq!(restarted.active().unwrap(), Some(op.clone()));
            op.collection_complete = true;
            op.phase = Phase::CollectionComplete;
            restarted.save(&mut op).unwrap();
            restarted.finish(&mut op, now).unwrap();
            assert_eq!(
                op.receipt,
                Some(SuccessReceipt {
                    completed_at: now,
                    next_eligible_at: now,
                })
            );
            assert!(restarted.active().unwrap().is_none());
            let document: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(document["manualTransfers"]["version"], 1);
            assert_eq!(
                document["manualTransfers"]["cooldowns"],
                original["manualTransfers"]["cooldowns"]
            );
            for prior in &retained {
                assert_eq!(restarted.get(&prior.id).unwrap(), *prior);
                assert_eq!(
                    document["manualTransfers"]["operations"][prior.id.as_str()],
                    serde_json::to_value(prior).unwrap()
                );
            }
            retained.push(op);
        }
        let text = std::fs::read_to_string(path).unwrap();
        assert!(!text.contains("CaseSensitiveLogin"));
    }
    #[test]
    fn failed_partial_and_unknown_never_gain_receipts_or_release_unknown_guard() {
        let root = tempfile::tempdir().unwrap();
        let p = TransferPolicy::new(root.path());
        let base = Endpoint::parse("http://127.0.0.1/").unwrap();
        let account = AccountRef::derive(&base, "synthetic").unwrap();
        let old = legacy_history(root.path(), &account);
        let old_deadlines = p.read().unwrap().cooldowns;
        let mut failed = p.begin(operation(account.clone(), Direction::Export), 1, 0).unwrap();
        failed.phase = Phase::Failed;
        p.end_before_activation(&mut failed).unwrap();
        assert_eq!(p.next_eligible_at(&account).unwrap(), None);
        let mut selected = operation(account.clone(), Direction::Export);
        selected.scope = Scope::CollectionAndMedia;
        let mut op = p.begin(selected, 2, 0).unwrap();
        op.acceptance = Acceptance::PossiblyAccepted;
        op.phase = Phase::ConfirmedIntent;
        p.save(&mut op).unwrap();
        assert_eq!(p.end_before_activation(&mut op), Err(PolicyError::Invalid));
        let rotated = AccountRef::derive(&Endpoint::parse("http://127.0.0.2/").unwrap(), "different").unwrap();
        let restart = TransferPolicy::new(root.path());
        for phase in [Phase::ConfirmedIntent, Phase::Cancelled, Phase::NeedsDecision] {
            op.phase = phase;
            p.save(&mut op).unwrap();
            let bytes = std::fs::read(&p.path).unwrap();
            for identity in [account.clone(), rotated.clone()] {
                for direction in [Direction::Import, Direction::Export] {
                    assert_eq!(
                        restart.begin(operation(identity.clone(), direction), 3, -1_000_000),
                        Err(PolicyError::Overlap(op.id.clone()))
                    );
                }
            }
            assert_eq!(p.end_before_activation(&mut op), Err(PolicyError::Invalid));
            assert_eq!(p.finish(&mut op, 100), Err(PolicyError::Invalid));
            assert_eq!(std::fs::read(&p.path).unwrap(), bytes);
            assert_eq!(restart.active().unwrap(), Some(op.clone()));
        }
        assert_eq!(restart.next_eligible_at(&account).unwrap(), None);
        op.acceptance = Acceptance::ExactOk;
        op.collection_complete = true;
        p.save(&mut op).unwrap();
        assert_eq!(p.finish(&mut op, 1_000), Err(PolicyError::Invalid));
        assert_eq!(p.next_eligible_at(&account).unwrap(), None);
        assert_eq!(p.get(&old.id).unwrap(), old);
        assert_eq!(p.read().unwrap().cooldowns, old_deadlines);
    }
    #[test]
    fn media_receipt_requires_durable_completion_and_scope_is_immutable() {
        let root = tempfile::tempdir().unwrap();
        let policy = TransferPolicy::new(root.path());
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        let mut selected = operation(account.clone(), Direction::Import);
        selected.scope = Scope::CollectionAndMedia;
        let mut op = policy.begin(selected, 20, 0).unwrap();
        op.collection_complete = true;
        op.actual_generation = Some(7);
        op.phase = Phase::MediaIncomplete;
        policy.save(&mut op).unwrap();
        // A caller cannot discard selected media by submitting a narrower scope.
        let mut collection_only = Operation {
            scope: Scope::CollectionOnly,
            ..op.clone()
        };
        let save_result = policy.save(&mut collection_only);
        assert_eq!(save_result, Err(PolicyError::Invalid));
        let finish_result = policy.finish(&mut collection_only, 1);
        assert_eq!(finish_result, Err(PolicyError::Stale));
        for stage in [
            MediaStage::AwaitingConsent,
            MediaStage::Declined,
            MediaStage::Running,
            MediaStage::Cancelled,
            MediaStage::Interrupted,
            MediaStage::Failed,
        ] {
            op.media.stage = stage;
            policy.save(&mut op).unwrap();
            assert_eq!(policy.finish(&mut op, 1), Err(PolicyError::Invalid));
            assert_eq!(policy.next_eligible_at(&account).unwrap(), None);
            let other = AccountRef::derive(&Endpoint::parse("http://127.0.0.2/").unwrap(), "different").unwrap();
            for identity in [account.clone(), other] {
                for direction in [Direction::Import, Direction::Export] {
                    assert_eq!(
                        policy.begin(operation(identity.clone(), direction), 21, 1),
                        Err(PolicyError::Overlap(op.id.clone()))
                    );
                }
            }
            assert_eq!(policy.active().unwrap(), Some(op.clone()));
            assert!(op.receipt.is_none());
        }
        op.media.consent = Some(MediaConsent {
            revision: op.revision,
            generation: 7,
            categories: MEDIA_CATEGORIES,
            account: account.clone(),
            configured_endpoint: op.configured_endpoint.clone(),
        });
        op.media.stage = MediaStage::Running;
        policy.save(&mut op).unwrap();
        op.media.stage = MediaStage::Complete;
        assert_eq!(
            policy.finish(&mut op, 1),
            Err(PolicyError::Stale),
            "cannot finish using completion that was not saved"
        );
        policy.save(&mut op).unwrap();
        policy.finish(&mut op, 1).unwrap();
        assert_eq!(TransferPolicy::new(root.path()).get(&op.id).unwrap(), op);
        assert_eq!(
            op.receipt,
            Some(SuccessReceipt {
                completed_at: 1,
                next_eligible_at: 1,
            })
        );
        assert!(policy.active().unwrap().is_none());
        assert!(policy.read().unwrap().cooldowns.is_empty());
        assert_eq!(policy.next_eligible_at(&account).unwrap(), None);
    }
    #[test]
    fn repeated_media_snapshot_does_not_publish_but_batch_error_and_terminal_do() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let policy = TransferPolicy::new(root.path());
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        let mut selected = operation(account, Direction::Import);
        selected.scope = Scope::CollectionAndMedia;
        let mut op = policy.begin(selected, 22, 0).unwrap();
        op.collection_complete = true;
        op.actual_generation = Some(7);
        op.phase = Phase::MediaIncomplete;
        op.media.consent = Some(MediaConsent {
            revision: op.revision,
            generation: 7,
            categories: MEDIA_CATEGORIES,
            account: op.account.clone(),
            configured_endpoint: op.configured_endpoint.clone(),
        });
        op.media.stage = MediaStage::Running;
        policy.save(&mut op).unwrap();
        let mut saved = op.clone();
        let path = root.path().join("manual-transfers.json");
        let original = std::fs::metadata(&path).unwrap().ino();
        let revision = op.revision;
        for _ in 0..25 {
            policy.save_snapshot(&mut op, &mut saved).unwrap();
        }
        assert_eq!(op.revision, revision);
        assert_eq!(
            std::fs::metadata(&path).unwrap().ino(),
            original,
            "unchanged progress must not republish metadata"
        );
        op.media.counters.downloaded_files = 25;
        policy.save_snapshot(&mut op, &mut saved).unwrap();
        assert_eq!(op.revision, revision + 1);
        assert_ne!(std::fs::metadata(&path).unwrap().ino(), original);
        assert_eq!(policy.get(&op.id).unwrap().media.counters.downloaded_files, 25);
        for stage in [MediaStage::Failed, MediaStage::Complete] {
            let previous = std::fs::metadata(&path).unwrap().ino();
            op.media.stage = stage;
            policy.save(&mut op).unwrap();
            assert_ne!(std::fs::metadata(&path).unwrap().ino(), previous);
            assert_eq!(policy.get(&op.id).unwrap().media.stage, stage);
        }
        assert_eq!(op.revision, revision + 3);
        policy.finish(&mut op, 1).unwrap();
        assert_eq!(policy.get(&op.id).unwrap().receipt.unwrap().next_eligible_at, 1);
    }
    #[test]
    fn legacy_history_without_media_fields_stays_readable() {
        let root = tempfile::tempdir().unwrap();
        let policy = TransferPolicy::new(root.path());
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        let op = policy.begin(operation(account, Direction::Import), 21, 0).unwrap();
        let path = root.path().join("manual-transfers.json");
        let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let saved = document["manualTransfers"]["operations"][op.id.as_str()]
            .as_object_mut()
            .unwrap();
        saved
            .get_mut("media")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("exclusions_verified");
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(
            !policy.get(&op.id).unwrap().media.exclusions_verified,
            "old snapshots cannot prove a fresh exclusion check"
        );
        let saved = document["manualTransfers"]["operations"][op.id.as_str()]
            .as_object_mut()
            .unwrap();
        saved.remove("media");
        saved.remove("resulting_publication");
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert_eq!(policy.get(&op.id).unwrap(), op);
    }
    #[test]
    fn partial_media_snapshot_mismatch_preserves_storage_and_admission() {
        // Policy snapshot conflicts are independent of activation proof.
        // Host tests cover producing that proof through verified replacements.
        let root = tempfile::tempdir().unwrap();
        let policy = TransferPolicy::new(root.path());
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        let op = policy.begin(operation(account, Direction::Import), 1, 0).unwrap();
        let path = root.path().join("manual-transfers.json");
        let bytes = std::fs::read(&path).unwrap();
        let conflicts = [
            Operation {
                id: OperationId("0000000000000002-0000000000000001".into()),
                ..op.clone()
            },
            Operation {
                revision: op.revision + 1,
                ..op.clone()
            },
            Operation {
                resulting_publication: Some(serde_json::from_str("[3,5]").unwrap()),
                ..op.clone()
            },
            Operation {
                collection_complete: true,
                ..op.clone()
            },
        ];
        for snapshot in conflicts {
            let error = policy.compare_and_supersede_partial_media(&snapshot).unwrap_err();
            assert_eq!(error, PolicyError::Stale);
            assert_eq!(bytes, std::fs::read(&path).unwrap());
            assert_eq!(policy.active().unwrap(), Some(op.clone()));
        }
        // A new operation must not be released by any snapshot of its predecessor.
        let mut closed = op.clone();
        closed.phase = Phase::Cancelled;
        policy.end_before_activation(&mut closed).unwrap();
        let newer = policy
            .begin(operation(op.account.clone(), Direction::Import), 2, 0)
            .unwrap();
        let predecessor_error = policy.compare_and_supersede_partial_media(&op).unwrap_err();
        assert_eq!(predecessor_error, PolicyError::Stale);
        assert_eq!(policy.active().unwrap(), Some(newer));
    }
    #[test]
    fn unreadable_history_is_retained_and_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("manual-transfers.json");
        std::fs::write(&path, b"{corrupt-history").unwrap();
        let p = TransferPolicy::new(root.path());
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        assert_eq!(p.history().unwrap_err(), PolicyError::Storage);
        assert_eq!(p.next_eligible_at(&account), Err(PolicyError::Storage));
        assert_eq!(
            p.begin(operation(account, Direction::Import), 1, 0),
            Err(PolicyError::Storage)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"{corrupt-history");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        assert_eq!(p.next_eligible_at(&account), Err(PolicyError::Storage));
        assert_eq!(
            p.begin(operation(account, Direction::Export), 1, 0),
            Err(PolicyError::Storage)
        );
        assert!(path.is_dir());
    }
    #[test]
    fn unexpected_policy_fields_refuse_all_queries_and_admission_without_rewriting() {
        let root = tempfile::tempdir().unwrap();
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        let mut old = legacy_history(root.path(), &account);
        let policy = TransferPolicy::new(root.path());
        let identity = FileIdentity {
            length: 42,
            sha256: [7; 32],
        };
        let artifact = Artifact {
            name: "synthetic.collection".into(),
            identity: identity.clone(),
        };
        let guard = LocalGuard {
            process: [1, 2],
            generation: 3,
            admitted: 4,
            committed: 4,
            publication: Some(
                serde_json::from_value(serde_json::json!({
                    "identity": [5, 6],
                    "binding": {"Connected": {
                        "account": "synthetic-account",
                        "configuration": "synthetic-configuration",
                        "environment": "synthetic-environment"
                    }},
                    "decision": "Clear"
                }))
                .unwrap(),
            ),
            closed_main: Some(identity.clone()),
            closed_wal: Some(identity.clone()),
        };
        let local = LocalFacts {
            modified: 10,
            schema_modified: 11,
            last_sync: 12,
            usn: 13,
            cards: 14,
            notes: 15,
            reviews: 16,
            pending_reviews: 0,
            pending_rows: [0; 8],
            last_review: Some(17),
        };
        old.guard = Some(guard.clone());
        old.recovery_guard = Some(guard);
        old.local = Some(local.clone());
        old.recovery_local = Some(local.clone());
        old.remote = Some(RemoteFacts {
            modified: 10,
            schema_modified: 11,
            usn: 13,
            empty: false,
            media_usn: 18,
        });
        old.candidate = Some(artifact.clone());
        old.activation_identity = Some(identity);
        old.original = Some(artifact.clone());
        old.safety_backups = vec![artifact.clone()];
        old.server_copy = Some(ServerCopy {
            artifact,
            kind: CopyKind::VerifiedPackage,
            published_at: 99,
            served_facts: Some(local),
        });
        old.media.consent = Some(MediaConsent {
            revision: 0,
            generation: 3,
            categories: MEDIA_CATEGORIES,
            account: account.clone(),
            configured_endpoint: old.configured_endpoint.clone(),
        });
        let mut original: serde_json::Value = serde_json::from_slice(&std::fs::read(&policy.path).unwrap()).unwrap();
        original["manualTransfers"]["operations"][old.id.as_str()] = serde_json::to_value(&old).unwrap();
        // Other sidecar namespaces remain opaque and survive a valid admission.
        original["otherNamespace"] = serde_json::json!({"opaque": {"future": 1}});
        let bytes = serde_json::to_vec(&original).unwrap();
        std::fs::write(&policy.path, &bytes).unwrap();
        assert_eq!(policy.get(&old.id).unwrap(), old);
        assert_eq!(policy.next_eligible_at(&account).unwrap(), None);
        assert_eq!(std::fs::read(&policy.path).unwrap(), bytes);
        policy
            .begin(operation(account.clone(), Direction::Import), 13, -1_000_000)
            .unwrap();
        let admitted: serde_json::Value = serde_json::from_slice(&std::fs::read(&policy.path).unwrap()).unwrap();
        assert_eq!(admitted["otherNamespace"], original["otherNamespace"]);
        assert_eq!(
            admitted["manualTransfers"]["cooldowns"],
            original["manualTransfers"]["cooldowns"]
        );
        assert_eq!(
            admitted["manualTransfers"]["operations"][old.id.as_str()],
            original["manualTransfers"]["operations"][old.id.as_str()]
        );

        let mut paths = vec![
            "/manualTransfers".to_owned(),
            format!("/manualTransfers/cooldowns/{}", account.0),
        ];
        // Cover every persisted object shape and each nested route, including
        // the publication's structured account binding. Scalars/arrays have no
        // object members to discard; no wire decoder or user payload is involved.
        for suffix in [
            "",
            "/receipt",
            "/guard",
            "/guard/publication",
            "/guard/publication/binding/Connected",
            "/guard/closed_main",
            "/guard/closed_wal",
            "/recovery_guard",
            "/recovery_guard/publication",
            "/recovery_guard/publication/binding/Connected",
            "/recovery_guard/closed_main",
            "/recovery_guard/closed_wal",
            "/local",
            "/recovery_local",
            "/remote",
            "/candidate",
            "/candidate/identity",
            "/activation_identity",
            "/original",
            "/original/identity",
            "/safety_backups/0",
            "/safety_backups/0/identity",
            "/server_copy",
            "/server_copy/artifact",
            "/server_copy/artifact/identity",
            "/server_copy/served_facts",
            "/media",
            "/media/consent",
            "/media/counters",
        ] {
            paths.push(format!("/manualTransfers/operations/{}{suffix}", old.id.as_str()));
        }
        for path in paths {
            let mut document = original.clone();
            document
                .pointer_mut(&path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected_policy_field".into(), serde_json::json!({"synthetic": 1}));
            let bytes = serde_json::to_vec(&document).unwrap();
            std::fs::write(&policy.path, &bytes).unwrap();
            let unchanged = || assert_eq!(std::fs::read(&policy.path).unwrap(), bytes, "{path}");
            assert_eq!(policy.next_eligible_at(&account), Err(PolicyError::Storage), "{path}");
            unchanged();
            assert_eq!(policy.history(), Err(PolicyError::Storage), "{path}");
            unchanged();
            assert_eq!(policy.active(), Err(PolicyError::Storage), "{path}");
            unchanged();
            assert_eq!(policy.get(&old.id), Err(PolicyError::Storage), "{path}");
            unchanged();
            for direction in [Direction::Import, Direction::Export] {
                assert_eq!(
                    policy.begin(operation(account.clone(), direction), 13, -1_000_000),
                    Err(PolicyError::Storage),
                    "{path}"
                );
                unchanged();
            }
        }
    }
    #[test]
    fn no_wait_still_validates_complete_legacy_schema_and_identities_without_rewriting() {
        let root = tempfile::tempdir().unwrap();
        let account = AccountRef::derive(&Endpoint::parse("http://127.0.0.1/").unwrap(), "synthetic").unwrap();
        let old = legacy_history(root.path(), &account);
        let policy = TransferPolicy::new(root.path());
        let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&policy.path).unwrap()).unwrap();
        for defect in 0..8 {
            let mut document = original.clone();
            let state = &mut document["manualTransfers"];
            match defect {
                0 => state["version"] = serde_json::json!(2),
                1 => state["cooldowns"][&account.0]["next_eligible_at"] = serde_json::json!("invalid"),
                2 => {
                    state["cooldowns"][&account.0]
                        .as_object_mut()
                        .unwrap()
                        .remove("completed_at");
                }
                3 => {
                    let receipt = state["cooldowns"].as_object_mut().unwrap().remove(&account.0).unwrap();
                    state["cooldowns"]["invalid-account-reference"] = receipt;
                }
                4 => state["operations"][old.id.as_str()]["account"] = serde_json::json!("invalid"),
                5 => {
                    state["operations"][old.id.as_str()]["id"] = serde_json::json!("0000000000000002-000000000000000c");
                }
                6 => state["active"] = serde_json::json!("0000000000000002-000000000000000c"),
                7 => state["operations"][old.id.as_str()]["receipt"]["next_eligible_at"] = serde_json::Value::Null,
                _ => unreachable!(),
            }
            let bytes = serde_json::to_vec(&document).unwrap();
            std::fs::write(&policy.path, &bytes).unwrap();
            assert_eq!(policy.next_eligible_at(&account), Err(PolicyError::Storage));
            assert_eq!(policy.history(), Err(PolicyError::Storage));
            for direction in [Direction::Import, Direction::Export] {
                assert_eq!(
                    policy.begin(operation(account.clone(), direction), 13, -1_000_000),
                    Err(PolicyError::Storage)
                );
            }
            assert_eq!(std::fs::read(&policy.path).unwrap(), bytes);
        }
    }
}
