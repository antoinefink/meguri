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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef(String);
impl AccountRef {
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
pub struct FileIdentity {
    pub length: u64,
    pub sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub identity: FileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            modified: f.modified.0,
            schema_modified: f.schema_modified.0,
            last_sync: f.last_sync.0,
            usn: f.usn.0,
            cards: f.cards,
            notes: f.notes,
            reviews: f.reviews,
            pending_reviews: f.pending_reviews,
            last_review: f.last_review.map(|t| t.0),
            pending_rows: [
                p.notes,
                p.cards,
                p.decks,
                p.deck_config,
                p.notetypes,
                p.tags,
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
pub struct ServerCopy {
    pub artifact: Artifact,
    pub kind: CopyKind,
    pub published_at: i64,
    /// Unknown for unverified and legacy-schema files; never inferred from empty.
    pub served_facts: Option<LocalFacts>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessReceipt {
    pub completed_at: i64,
    pub next_eligible_at: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub receipt: Option<SuccessReceipt>,
    /// Binds final authority to a persisted revision, not caller-supplied facts.
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
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
        sidecar::read_document(&self.path, |doc| decode(doc)).map_err(Into::into)
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
    pub fn next_eligible_at(&self, account: &AccountRef) -> Result<Option<i64>, PolicyError> {
        Ok(self.read()?.cooldowns.get(&account.0).map(|r| r.next_eligible_at))
    }
    pub(crate) fn begin(&self, mut operation: Operation, nonce: u64, now: i64) -> Result<Operation, PolicyError> {
        self.update(|state| {
            if let Some(id) = &state.active {
                return Err(PolicyError::Overlap(id.clone()));
            }
            if let Some(r) = state.cooldowns.get(&operation.account.0) {
                if now < r.next_eligible_at {
                    return Err(PolicyError::Cooldown {
                        next_eligible_at: r.next_eligible_at,
                    });
                }
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
            let mut saved = operation.clone();
            saved.revision = next;
            state.operations.insert(saved.id.0.clone(), saved);
            Ok(())
        })?;
        operation.revision = next;
        Ok(())
    }
    /// Only a complete selected scope gets a receipt. Receipt, retained deadline
    /// and guard release are a SINGLE durable transition. Rollback never lowers it.
    pub(crate) fn finish(&self, operation: &mut Operation, now: i64) -> Result<(), PolicyError> {
        if !operation.collection_complete || operation.scope != Scope::CollectionOnly {
            return Err(PolicyError::Invalid);
        }
        let mut saved = operation.clone();
        self.update(|state| {
            if state.active.as_ref() != Some(&saved.id)
                || state.operations.get(saved.id.as_str()).map(|o| o.revision) != Some(saved.revision)
            {
                return Err(PolicyError::Stale);
            }
            let previous = state
                .cooldowns
                .get(&saved.account.0)
                .map(|r| r.next_eligible_at)
                .unwrap_or(i64::MIN);
            let deadline = now
                .checked_add(60 * 60 * 1000)
                .ok_or(PolicyError::Invalid)?
                .max(previous);
            let receipt = SuccessReceipt {
                completed_at: now,
                next_eligible_at: deadline,
            };
            saved.phase = Phase::Complete;
            saved.boundary = None;
            saved.receipt = Some(receipt.clone());
            saved.revision = saved.revision.checked_add(1).ok_or(PolicyError::Invalid)?;
            state.cooldowns.insert(saved.account.0.clone(), receipt);
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
        || state
            .active
            .as_ref()
            .is_some_and(|id| !state.operations.contains_key(id.as_str()))
    {
        return Err(invalid());
    }
    for (id, op) in &state.operations {
        let unresolved = !matches!(op.phase, Phase::Complete | Phase::Failed | Phase::Cancelled)
            || (op.acceptance == Acceptance::PossiblyAccepted)
            || (op.acceptance == Acceptance::ExactOk && op.phase != Phase::Complete)
            || (op.activation_identity.is_some() && op.phase != Phase::Complete);
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
            || (op.phase == Phase::Complete
                && (op.receipt.is_none() || !op.collection_complete || op.scope != Scope::CollectionOnly))
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
            receipt: None,
            revision: 0,
        }
    }
    #[test]
    fn shared_success_cooldown_survives_restart_rotation_and_clock_rollback() {
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
        let policy = TransferPolicy::new(root.path());
        let mut op = policy
            .begin(operation(account.clone(), Direction::Import), 12, 100)
            .unwrap();
        op.collection_complete = true;
        op.phase = Phase::CollectionComplete;
        policy.save(&mut op).unwrap();
        policy.finish(&mut op, 100).unwrap();
        assert_eq!(op.receipt.as_ref().unwrap().next_eligible_at, 3_600_100);
        let restarted = TransferPolicy::new(root.path());
        for now in [99, -1_000_000, 3_600_099] {
            assert!(matches!(
                restarted.begin(operation(account.clone(), Direction::Export), 13, now),
                Err(PolicyError::Cooldown {
                    next_eligible_at: 3_600_100
                })
            ));
        }
        assert!(
            restarted
                .begin(operation(account, Direction::Export), 14, 3_600_100)
                .is_ok()
        );
        let text = std::fs::read_to_string(root.path().join("manual-transfers.json")).unwrap();
        assert!(!text.contains("CaseSensitiveLogin"));
    }
    #[test]
    fn failed_partial_and_unknown_never_consume_cooldown_or_release_unknown_guard() {
        let root = tempfile::tempdir().unwrap();
        let p = TransferPolicy::new(root.path());
        let base = Endpoint::parse("http://127.0.0.1/").unwrap();
        let account = AccountRef::derive(&base, "synthetic").unwrap();
        let mut failed = p.begin(operation(account.clone(), Direction::Export), 1, 0).unwrap();
        failed.phase = Phase::Failed;
        p.end_before_activation(&mut failed).unwrap();
        assert_eq!(p.next_eligible_at(&account).unwrap(), None);
        let mut op = p.begin(operation(account.clone(), Direction::Export), 2, 0).unwrap();
        op.acceptance = Acceptance::PossiblyAccepted;
        op.phase = Phase::ConfirmedIntent;
        p.save(&mut op).unwrap();
        assert_eq!(p.end_before_activation(&mut op), Err(PolicyError::Invalid));
        let rotated = AccountRef::derive(&Endpoint::parse("http://127.0.0.2/").unwrap(), "different").unwrap();
        let restart = TransferPolicy::new(root.path());
        assert!(matches!(
            restart.begin(operation(rotated, Direction::Import), 3, 9_999_999),
            Err(PolicyError::Overlap(_))
        ));
        assert_eq!(restart.next_eligible_at(&account).unwrap(), None);
        op.scope = Scope::CollectionAndMedia;
        op.acceptance = Acceptance::ExactOk;
        op.collection_complete = true;
        p.save(&mut op).unwrap();
        assert_eq!(p.finish(&mut op, 1_000), Err(PolicyError::Invalid));
        assert_eq!(p.next_eligible_at(&account).unwrap(), None);
    }
    #[test]
    fn unreadable_history_is_retained_and_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("manual-transfers.json");
        std::fs::write(&path, b"{corrupt-history").unwrap();
        let p = TransferPolicy::new(root.path());
        assert_eq!(p.history().unwrap_err(), PolicyError::Storage);
        assert_eq!(std::fs::read(path).unwrap(), b"{corrupt-history");
    }
}
