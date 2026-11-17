//! Provider-specific bootstrap evidence over retained manual operations.
use super::*;
use crate::{oneway::Preview, policy::{AccountRef, Direction, Operation, OperationId, Phase, Scope, TransferPolicy},
    provider_media::{MediaAccounting, MAX_ENTRY_BYTES, account_media}};
use meguri_engine::device_state::PublicationId;
use meguri_storage::media::{MediaBaselineOwner, MediaError, MediaStore, MediaSyncBaseline};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaEpochEvidence { owner: String, epoch: u64 }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeguriBootstrapIntent {
    connection: MeguriConnection, operation: OperationId, operation_revision: u64, direction: Direction,
    #[serde(default)]
    media_epoch: Option<MediaEpochEvidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeguriBinding {
    connection: MeguriConnection, account: AccountRef, publication: PublicationId,
    operation: OperationId, media_epoch: MediaEpochEvidence, completed_at_ms: i64,
}
impl MediaEpochEvidence {
    pub fn epoch(&self) -> u64 { self.epoch }
    pub fn matches(&self, baseline: &MediaSyncBaseline) -> bool {
        baseline.owner.as_ref().is_some_and(|owner| owner.as_str() == self.owner) && baseline.epoch == self.epoch
    }
}
impl MeguriBootstrapIntent {
    pub fn connection(&self) -> &MeguriConnection { &self.connection }
    pub fn operation(&self) -> &OperationId { &self.operation }
    pub fn media_epoch(&self) -> Option<&MediaEpochEvidence> { self.media_epoch.as_ref() }
}
impl MeguriBinding {
    pub fn connection(&self) -> &MeguriConnection { &self.connection }
    pub fn operation(&self) -> &OperationId { &self.operation }
    pub fn account(&self) -> &AccountRef { &self.account }
    pub fn media_owner(&self) -> MediaBaselineOwner { self.account.media_owner() }
    pub fn media_epoch(&self) -> &MediaEpochEvidence { &self.media_epoch }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeguriReadiness { NotSelected, Unbound { intent: Option<OperationId> }, Ready, Stale }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaRefusal { Cancelled, DiskFull, Unavailable }
pub enum MeguriReadyRefusal {
    NotSelected, ConnectionChanged, NoIntent, OperationMismatch, NotComplete, AccountChanged,
    PublicationChanged, DocumentChanged, BindingChanged, MediaEpochMissing, MediaEpochChanged,
    MediaIncomplete(MediaAccounting), Media(MediaRefusal), Storage(StorageError),
}
impl fmt::Debug for MeguriReadyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotSelected => "NotSelected", Self::ConnectionChanged => "ConnectionChanged",
            Self::NoIntent => "NoIntent", Self::OperationMismatch => "OperationMismatch",
            Self::NotComplete => "NotComplete", Self::AccountChanged => "AccountChanged",
            Self::PublicationChanged => "PublicationChanged", Self::DocumentChanged => "DocumentChanged",
            Self::BindingChanged => "BindingChanged", Self::MediaEpochMissing => "MediaEpochMissing",
            Self::MediaEpochChanged => "MediaEpochChanged", Self::MediaIncomplete(_) => "MediaIncomplete",
            Self::Media(_) => "Media", Self::Storage(_) => "Storage",
        })
    }
}
impl From<StorageError> for MeguriReadyRefusal {
    fn from(error: StorageError) -> Self { Self::Storage(error) }
}
impl From<MediaError> for MeguriReadyRefusal {
    fn from(error: MediaError) -> Self {
        Self::Media(match error { MediaError::Cancelled => MediaRefusal::Cancelled,
            MediaError::DiskFull => MediaRefusal::DiskFull, _ => MediaRefusal::Unavailable })
    }
}
pub struct EpochInputs<'a> {
    pub connection: &'a MeguriConnection, pub operation: &'a OperationId,
    pub policy: &'a TransferPolicy, pub media: &'a MediaStore,
}
pub struct ReadyInputs<'a> {
    pub connection: &'a MeguriConnection, pub username: &'a str, pub operation: &'a OperationId,
    pub policy: &'a TransferPolicy, pub media: &'a MediaStore, pub keep_going: &'a mut dyn FnMut() -> bool,
}
/// Process-local, consumed once. No caller can construct epoch authority.
pub struct EpochEvidence {
    intent: MeguriBootstrapIntent, document_revision: u64, evidence: MediaEpochEvidence, established: bool,
}
/// Process-local, consumed once. Finalization uses only retained operation facts.
pub struct ReadyEvidence {
    intent: MeguriBootstrapIntent, account: AccountRef, operation_revision: u64,
    document_revision: u64, media_epoch: MediaEpochEvidence, accounting: MediaAccounting,
}
impl ReadyEvidence { pub fn accounting(&self) -> &MediaAccounting { &self.accounting } }
type Result<T> = std::result::Result<T, MeguriReadyRefusal>;

fn check_document(doc: &ProviderDocument, connection: &MeguriConnection) -> Result<()> {
    if doc.selection != Selection::Meguri { return Err(MeguriReadyRefusal::NotSelected); }
    if doc.handoff.is_some() || doc.meguri.as_ref() != Some(connection) {
        return Err(MeguriReadyRefusal::ConnectionChanged);
    }
    Ok(())
}
fn intent_for(doc: &ProviderDocument, connection: &MeguriConnection, operation: &OperationId) -> Result<MeguriBootstrapIntent> {
    check_document(doc, connection)?;
    let intent = doc.meguri_bootstrap.as_ref().ok_or(MeguriReadyRefusal::NoIntent)?;
    if &intent.connection != connection || &intent.operation != operation { return Err(MeguriReadyRefusal::OperationMismatch); }
    Ok(intent.clone())
}
fn account(connection: &MeguriConnection, username: &str) -> Result<AccountRef> {
    AccountRef::derive(&native_endpoint(&connection.endpoint)?, &canonical_username(username)?)
        .ok_or(MeguriReadyRefusal::AccountChanged)
}
fn operation(policy: &TransferPolicy, id: &OperationId) -> Result<Operation> {
    policy.get(id).map_err(|_| MeguriReadyRefusal::NotComplete)
}
fn active(policy: &TransferPolicy) -> Result<Option<Operation>> {
    policy.active().map_err(|_| MeguriReadyRefusal::NotComplete)
}
fn readiness(doc: &ProviderDocument, settled: Option<&Publication>) -> MeguriReadiness {
    if doc.selection != Selection::Meguri { return MeguriReadiness::NotSelected; }
    let Some(binding) = &doc.meguri_binding else {
        return MeguriReadiness::Unbound { intent: doc.meguri_bootstrap.as_ref().map(|i| i.operation.clone()) };
    };
    if Some(&binding.connection) == doc.meguri.as_ref() && settled.map(|p| p.identity) == Some(binding.publication) {
        MeguriReadiness::Ready
    } else { MeguriReadiness::Stale }
}

impl Authority {
    // Preserve typed refusals without placing filenames or journal errors into
    // the storage error channel. The publication closure writes no refused state.
    fn bootstrap_update(&self, generation: u64, publication: Option<Publication>,
        change: impl FnOnce(&mut ProviderDocument) -> Result<()>) -> Result<()> {
        let mut refusal = None;
        let result = self.update_publication(Some((generation, publication)), true, |_, doc| {
            change(doc).map_err(|error| { refusal = Some(error); invalid() })
        });
        if let Some(error) = refusal { return Err(error); }
        result?;
        Ok(())
    }
    pub fn record_meguri_bootstrap_intent(&self, connection: &MeguriConnection, username: &str,
        preview: &Preview, generation: u64, publication: Option<Publication>) -> Result<()> {
        let settled = publication.clone();
        self.bootstrap_update(generation, publication, |doc| {
            check_document(doc, connection)?;
            if readiness(doc, settled.as_ref()) == MeguriReadiness::Ready { return Err(MeguriReadyRefusal::NotComplete); }
            let op = &preview.operation;
            if op.scope != Scope::CollectionAndMedia || op.configured_endpoint != connection.endpoint {
                return Err(MeguriReadyRefusal::OperationMismatch);
            }
            if op.account != account(connection, username)? { return Err(MeguriReadyRefusal::AccountChanged); }
            if doc.meguri_bootstrap.as_ref().is_some_and(|intent|
                intent.operation == op.id && intent.media_epoch.is_some()) {
                return Err(MeguriReadyRefusal::OperationMismatch);
            }
            doc.meguri_bootstrap = Some(MeguriBootstrapIntent { connection: connection.clone(),
                operation: op.id.clone(), operation_revision: op.revision, direction: op.direction, media_epoch: None });
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })
    }
    /// Caller holds OneWaySync then media Sync for this entire preparation and record.
    pub fn prepare_meguri_media_epoch(&self, inputs: EpochInputs<'_>) -> Result<EpochEvidence> {
        let doc = self.snapshot()?;
        let intent = intent_for(&doc, inputs.connection, inputs.operation)?;
        let op = operation(inputs.policy, inputs.operation)?;
        if !op.collection_complete || op.scope != Scope::CollectionAndMedia || op.receipt.is_some()
            || active(inputs.policy)?.as_ref().map(|op| &op.id) != Some(inputs.operation) {
            return Err(MeguriReadyRefusal::NotComplete);
        }
        if op.direction != intent.direction || op.configured_endpoint != inputs.connection.endpoint {
            return Err(MeguriReadyRefusal::OperationMismatch);
        }
        let owner = op.account.media_owner();
        let baseline = inputs.media.sync_baseline()?;
        let (evidence, established) = if let Some(recorded) = &intent.media_epoch {
            if recorded.owner != owner.as_str() || !recorded.matches(&baseline) {
                return Err(MeguriReadyRefusal::MediaEpochChanged);
            }
            (recorded.clone(), false)
        } else {
            if !inputs.media.bind_sync_owner(&owner)? { inputs.media.reset_sync()?; }
            let baseline = inputs.media.sync_baseline()?;
            if baseline.owner.as_ref() != Some(&owner) { return Err(MeguriReadyRefusal::MediaEpochChanged); }
            (MediaEpochEvidence { owner: owner.as_str().into(), epoch: baseline.epoch }, true)
        };
        Ok(EpochEvidence { intent, document_revision: doc.revision, evidence, established })
    }
    pub fn record_meguri_media_epoch(&self, evidence: EpochEvidence, generation: u64, publication: Option<Publication>) -> Result<()> {
        if !evidence.established { return Ok(()); }
        self.bootstrap_update(generation, publication, |doc| {
            if doc.revision != evidence.document_revision { return Err(MeguriReadyRefusal::DocumentChanged); }
            check_document(doc, &evidence.intent.connection)?;
            if doc.meguri_bootstrap.as_ref() != Some(&evidence.intent) || evidence.intent.media_epoch.is_some() {
                return Err(MeguriReadyRefusal::OperationMismatch);
            }
            let intent = doc.meguri_bootstrap.as_mut().ok_or(MeguriReadyRefusal::NoIntent)?;
            intent.media_epoch = Some(evidence.evidence);
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })
    }
    /// The checked inventory is a snapshot; the leases exclude long jobs, not
    /// immediate editor attachments or later external folder changes.
    pub fn prepare_meguri_ready(&self, inputs: ReadyInputs<'_>) -> Result<ReadyEvidence> {
        let doc = self.snapshot()?;
        let intent = intent_for(&doc, inputs.connection, inputs.operation)?;
        // A ready binding cannot coexist with an intent in a valid document.
        if doc.meguri_binding.is_some() { return Err(MeguriReadyRefusal::BindingChanged); }
        let media_epoch = intent.media_epoch.clone().ok_or(MeguriReadyRefusal::MediaEpochMissing)?;
        let op = operation(inputs.policy, inputs.operation)?;
        if active(inputs.policy)?.is_some() || op.phase != Phase::Complete || op.receipt.is_none()
            || !op.collection_complete || op.scope != Scope::CollectionAndMedia {
            return Err(MeguriReadyRefusal::NotComplete);
        }
        if op.direction != intent.direction || op.revision <= intent.operation_revision
            || op.configured_endpoint != inputs.connection.endpoint { return Err(MeguriReadyRefusal::OperationMismatch); }
        if op.account != account(inputs.connection, inputs.username)? { return Err(MeguriReadyRefusal::AccountChanged); }
        if media_epoch.owner != op.account.media_owner().as_str() || !media_epoch.matches(&inputs.media.sync_baseline()?) {
            return Err(MeguriReadyRefusal::MediaEpochChanged);
        }
        let inventory = inputs.media.promised_inventory(MAX_ENTRY_BYTES, inputs.keep_going)?;
        let accounting = account_media(inputs.media, &inventory, &op)?;
        if accounting.operation() != &op.id || accounting.operation_revision() != op.revision
            || !accounting.verified() || !accounting.is_complete() {
            return Err(MeguriReadyRefusal::MediaIncomplete(accounting));
        }
        Ok(ReadyEvidence { intent, account: op.account, operation_revision: op.revision,
            document_revision: doc.revision, media_epoch, accounting })
    }
    pub fn commit_meguri_ready(&self, evidence: ReadyEvidence, policy: &TransferPolicy,
        generation: u64, publication: Option<Publication>) -> Result<MediaAccounting> {
        let settled = publication.clone();
        self.bootstrap_update(generation, publication, |doc| {
            if doc.revision != evidence.document_revision { return Err(MeguriReadyRefusal::DocumentChanged); }
            check_document(doc, &evidence.intent.connection)?;
            if doc.meguri_bootstrap.as_ref() != Some(&evidence.intent) { return Err(MeguriReadyRefusal::OperationMismatch); }
            let op = operation(policy, &evidence.intent.operation)?;
            if op.revision != evidence.operation_revision || op.phase != Phase::Complete || active(policy)?.is_some() {
                return Err(MeguriReadyRefusal::NotComplete);
            }
            if op.account != evidence.account { return Err(MeguriReadyRefusal::AccountChanged); }
            let publication = settled.as_ref().filter(|p| Some(p.identity) == op.resulting_publication)
                .ok_or(MeguriReadyRefusal::PublicationChanged)?;
            let receipt = op.receipt.as_ref().ok_or(MeguriReadyRefusal::NotComplete)?;
            doc.meguri_binding = Some(MeguriBinding { connection: evidence.intent.connection,
                account: op.account, publication: publication.identity, operation: op.id,
                media_epoch: evidence.media_epoch, completed_at_ms: receipt.completed_at });
            doc.meguri_bootstrap = None;
            doc.settings.enabled = true;
            doc.settings.automatic = true;
            doc.meguri_settings.automatic = true;
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })?;
        Ok(evidence.accounting)
    }
    pub fn hold_meguri_binding_for_full_sync(&self, expected: &MeguriBinding, generation: u64,
        publication: Option<Publication>) -> Result<()> {
        self.bootstrap_update(generation, publication, |doc| {
            if doc.meguri_binding.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::BindingChanged); }
            check_document(doc, &expected.connection)?;
            doc.meguri_binding = None;
            doc.meguri_settings.automatic = false;
            if doc.selection == Selection::Meguri { doc.settings.automatic = false; }
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })
    }
    pub fn meguri_readiness(&self, settled: Option<&Publication>) -> MeguriReadiness {
        self.snapshot().map_or(MeguriReadiness::Stale, |doc| readiness(&doc, settled))
    }
    pub fn permits_meguri_binding(&self, connection: &MeguriConnection, settled: Option<&Publication>) -> bool {
        !self.network_blocked() && self.snapshot().is_ok_and(|doc| doc.permits_meguri_transfer()
            && doc.meguri.as_ref() == Some(connection) && readiness(&doc, settled) == MeguriReadiness::Ready)
    }
    pub fn meguri_bootstrap_intent(&self) -> Option<MeguriBootstrapIntent> {
        self.snapshot().ok().and_then(|doc| doc.meguri_bootstrap)
    }
}
