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
    pub(super) connection: MeguriConnection, pub(super) account: AccountRef, pub(super) publication: PublicationId,
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
pub(super) type Result<T> = std::result::Result<T, MeguriReadyRefusal>;

pub(super) fn check_document(doc: &ProviderDocument, connection: &MeguriConnection) -> Result<()> {
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
pub(super) fn readiness(doc: &ProviderDocument, settled: Option<&Publication>) -> MeguriReadiness {
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
    pub(super) fn bootstrap_update(&self, generation: u64, publication: Option<Publication>,
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
        // Read under the caller's OneWaySync/media leases before taking the
        // provider sidecar lock: sidecar accessors cannot nest across files.
        // Defer policy errors until after the document and intent checks.
        let retained_operation: Result<Operation> = operation(policy, &evidence.intent.operation);
        let retained_active: Result<Option<Operation>> = active(policy);
        let settled = publication.clone();
        self.bootstrap_update(generation, publication, |doc| {
            if doc.revision != evidence.document_revision { return Err(MeguriReadyRefusal::DocumentChanged); }
            check_document(doc, &evidence.intent.connection)?;
            if doc.meguri_bootstrap.as_ref() != Some(&evidence.intent) { return Err(MeguriReadyRefusal::OperationMismatch); }
            let op = retained_operation?;
            if op.revision != evidence.operation_revision || op.phase != Phase::Complete || retained_active?.is_some() {
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
            // MP7: a completed bootstrap settles a structure upload recorded
            // for this connection, whichever collection was kept. The job
            // held the binding, which turned Sync Automatically off: the
            // stored choice comes back with the new binding (MP6.2).
            let automatic = doc.meguri_structure.take().is_none_or(|upload| upload.automatic());
            doc.settings.enabled = true;
            doc.settings.automatic = automatic;
            doc.meguri_settings.automatic = automatic;
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

/// Routine refusal contains no endpoint, account, filename or server payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriRoundRefusal {
    NotSelected, Paused, NotReady, BindingChanged, ConnectionChanged,
    AccountChanged, PublicationChanged, Busy, Interrupted, Storage,
}
/// Issued by this installation's actual Ready authority. All fields are private;
/// credentials and callers cannot manufacture a successful admission sample.
#[derive(Clone)]
pub struct MeguriRoundAdmission {
    authority: std::sync::Arc<Authority>,
    original: meguri_engine::jobs::registry::OriginalResourceAuthority,
    collection: meguri_engine::Collection,
    policy: TransferPolicy,
    media: MediaStore,
    admitted_media_epoch: std::sync::Arc<std::sync::Mutex<MediaEpochEvidence>>,
    binding: MeguriBinding,
    publication: Publication,
    revision: u64,
    closure: std::sync::Arc<()>,
    refusal: std::sync::Arc<std::sync::Mutex<Option<MeguriRoundRefusal>>>,
}
impl MeguriRoundAdmission {
    pub fn binding(&self) -> &MeguriBinding { &self.binding }
    pub fn publication(&self) -> &Publication { &self.publication }
    pub fn refusal(&self) -> Option<MeguriRoundRefusal> {
        *self.refusal.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn current(&self, check_media: bool) -> std::result::Result<(), MeguriRoundRefusal> {
        use MeguriRoundRefusal as R;
        if !self.original.is_live() || !self.collection.is_current() { return Err(R::Interrupted); }
        // Never wait on provider ownership from the collection writer. The
        // metadata lane takes lifecycle before provider, and never joins ops.
        {
            let state = self.authority.state.try_lock().map_err(|_| R::Busy)?;
            let doc = state.document.as_ref().map_err(|_| R::Storage)?;
            routine_document(doc, &self.binding, &self.publication)?;
            if doc.revision != self.revision || !std::sync::Arc::ptr_eq(&state.closures.ordinary, &self.closure)
                || self.authority.network_blocked() { return Err(R::BindingChanged); }
        }
        // Each sidecar read ends before the next starts. In particular these
        // two files share a parent lock; no sidecar callback reads another.
        let doc = sidecar::read_document(&self.authority.path, decode)
            .map_err(|_| R::Storage)?.ok_or(R::Storage)?;
        routine_document(&doc, &self.binding, &self.publication)?;
        if doc.revision != self.revision { return Err(R::BindingChanged); }
        let publication = meguri_engine::device_state::DeviceState::for_collection(self.collection.path())
            .settled_publication().map_err(|_| R::Storage)?;
        if publication.as_ref() != Some(&self.publication) { return Err(R::PublicationChanged); }
        if self.policy.active().map_err(|_| R::Storage)?.is_some() { return Err(R::Busy); }
        if check_media {
            let epoch = self.admitted_media_epoch.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            self.media.bound_sync_baseline(&self.binding.media_owner(), epoch.epoch()).map_err(|error| match error {
                meguri_storage::media::MediaBoundCheckError::Changed => R::BindingChanged,
                meguri_storage::media::MediaBoundCheckError::Storage(_) => R::Storage,
            })?;
        }
        if !self.original.is_live() || !self.collection.is_current() { return Err(R::Interrupted); }
        Ok(())
    }
    pub fn check(&self) -> std::result::Result<(), MeguriRoundRefusal> { self.check_selected(true) }
    // Storage already compares the full Stage baseline under its journal.
    // Never read that same journal from its bounded effect callback.
    pub(crate) fn check_media_effect(&self) -> std::result::Result<(), MeguriRoundRefusal> { self.check_selected(false) }
    fn check_selected(&self, check_media: bool) -> std::result::Result<(), MeguriRoundRefusal> {
        if let Some(refusal) = self.refusal() { return Err(refusal); }
        self.current(check_media).inspect_err(|&error| {
            self.refusal.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert(error);
        })
    }
    // Only a storage-minted conditional reset from this job's exact store and
    // expected epoch can advance the shared request/checkpoint witness.
    pub(crate) fn accept_media_reset(&self, reset: &meguri_storage::media::MediaBoundReset)
        -> std::result::Result<(), MeguriRoundRefusal> {
        let mut epoch = self.admitted_media_epoch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !reset.is_from(&self.media) || !epoch.matches(reset.before())
            || reset.after().owner.as_ref() != Some(&self.binding.media_owner()) {
            return Err(MeguriRoundRefusal::BindingChanged);
        }
        epoch.epoch = reset.after().epoch;
        Ok(())
    }
    pub(crate) fn job_check(&self) -> std::result::Result<(), crate::JobError> {
        self.check().map_err(|refusal| crate::JobError::new(match refusal {
            MeguriRoundRefusal::Storage => crate::ErrorKind::LocalStorage,
            MeguriRoundRefusal::Busy => crate::ErrorKind::ExclusiveBusy,
            _ => crate::ErrorKind::Interrupted,
        }, None, "Meguri original round admission closed"))
    }
}
fn routine_document(doc: &ProviderDocument, binding: &MeguriBinding, publication: &Publication)
    -> std::result::Result<(), MeguriRoundRefusal> {
    use MeguriRoundRefusal as R;
    if doc.selection != Selection::Meguri { return Err(R::NotSelected); }
    if doc.settings.paused { return Err(R::Paused); }
    if doc.handoff.is_some() { return Err(R::Busy); }
    if doc.meguri.as_ref() != Some(binding.connection()) { return Err(R::ConnectionChanged); }
    if doc.meguri_binding.as_ref() != Some(binding) { return Err(R::BindingChanged); }
    if !doc.permits_meguri_transfer() { return Err(R::NotReady); }
    if readiness(doc, Some(publication)) != MeguriReadiness::Ready { return Err(R::PublicationChanged); }
    Ok(())
}
impl Authority {
    /// Host composition supplies its real original-resource authority and
    /// retained manual policy. No baseline/readiness argument is accepted.
    pub fn admit_meguri_round(self: &std::sync::Arc<Self>, collection: meguri_engine::Collection,
        original: meguri_engine::jobs::registry::OriginalResourceAuthority, policy: TransferPolicy,
        connection: &MeguriConnection, username: &str, media: MediaStore)
        -> std::result::Result<MeguriRoundAdmission, MeguriRoundRefusal> {
        use MeguriRoundRefusal as R;
        if !original.is_live() || !collection.is_current() { return Err(R::Interrupted); }
        let publication = meguri_engine::device_state::DeviceState::for_collection(collection.path())
            .settled_publication().map_err(|_| R::Storage)?.ok_or(R::PublicationChanged)?;
        let (binding, revision, closure) = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let doc = state.document.as_ref().map_err(|_| R::Storage)?;
            if doc.selection != Selection::Meguri { return Err(R::NotSelected); }
            if doc.settings.paused { return Err(R::Paused); }
            if doc.meguri.as_ref() != Some(connection) { return Err(R::ConnectionChanged); }
            let binding = doc.meguri_binding.clone().ok_or(R::NotReady)?;
            routine_document(doc, &binding, &publication)?;
            if binding.account != account(connection, username).map_err(|_| R::AccountChanged)? { return Err(R::AccountChanged); }
            (binding, doc.revision, state.closures.ordinary.clone())
        };
        let admitted_media_epoch = std::sync::Arc::new(std::sync::Mutex::new(binding.media_epoch.clone()));
        let admission = MeguriRoundAdmission { authority: self.clone(), original, collection, policy, media, admitted_media_epoch,
            binding, publication, revision, closure, refusal: Default::default() };
        admission.check()?;
        Ok(admission)
    }
    /// Local-only finalization, inside the original Host metadata lane. The
    /// storage guard rechecks the sealed terminal baseline through publication.
    /// A no-reset round deliberately never rewrites provider bytes.
    pub fn finalize_meguri_round_epoch(&self, expected: &MeguriBinding,
        receipt: &crate::media::BoundMediaReceipt, media: &MediaStore,
        generation: u64, publication: Option<Publication>) -> Result<()> {
        if !expected.media_epoch.matches(receipt.initial())
            || expected.media_owner() != *receipt.owner()
            || publication.as_ref().map(|p| p.identity) != Some(expected.publication) {
            return Err(MeguriReadyRefusal::BindingChanged);
        }
        // Publication ownership precedes the journal. Both acquisitions are
        // nonblocking in this lane; neither can invert an existing media owner.
        let mut failure = self.publication_failure.try_lock()
            .map_err(|_| MeguriReadyRefusal::Storage(invalid()))?;
        receipt.with_current(media, || {
            if !receipt.reset_occurred() { return Ok(()); }
            let mut desired = expected.clone();
            desired.media_epoch.epoch = receipt.terminal().epoch;
            // Retry only this exact failed desired mutation, without replaying
            // requests. The original terminal result/receipt remains retained.
            let retry = {
                match failure.as_ref() {
                    None => false,
                    Some(failed) => {
                        let pending = failed.pending.as_ref().ok_or_else(|| MeguriReadyRefusal::Storage(failed.error.clone()))?;
                        let before = decode(Some(&pending.before))?.ok_or_else(invalid)?;
                        if !receipt.reset_occurred() || before.meguri_binding.as_ref() != Some(expected)
                            || pending.document.meguri_binding.as_ref() != Some(&desired)
                            || pending.facts.as_ref() != Some(&(generation, publication.clone())) {
                            return Err(MeguriReadyRefusal::BindingChanged);
                        }
                        true
                    }
                }
            };
            if retry { self.retry_round_publication_locked(&mut failure)?; }
            let doc = self.snapshot()?;
            if doc.meguri_binding.as_ref() == Some(&desired) && retry { return Ok(()); }
            if doc.meguri_binding.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::BindingChanged); }
            check_epoch_document(&doc, expected, generation, publication.as_ref())?;
            let settled = publication.clone();
            let mut refusal = None;
            let updated = self.update_publication_locked(&mut failure, Some((generation, publication)), true, |_, doc| {
                let change = (|| -> Result<()> {
                check_epoch_document(doc, expected, generation, settled.as_ref())?;
                if doc.meguri_binding.as_ref() != Some(expected)
                    || settled.as_ref().map(|p| p.identity) != Some(expected.publication) {
                    return Err(MeguriReadyRefusal::BindingChanged);
                }
                // Only this field and the document's admission revision change.
                doc.meguri_binding = Some(desired);
                // A recorded handoff already closes admission and pins this
                // revision. Settling its old worker changes only the binding's
                // media epoch; the original intent and every decision survive.
                if doc.handoff.is_none() { doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?; }
                Ok(())
                })();
                change.map_err(|error| { refusal = Some(error); invalid() })
            });
            if let Some(error) = refusal { return Err(error); }
            updated?;
            Ok(())
        }).map_err(|error| match error {
            meguri_storage::media::MediaBoundCheckError::Changed => MeguriReadyRefusal::MediaEpochChanged,
            meguri_storage::media::MediaBoundCheckError::Storage(error) => MeguriReadyRefusal::from(error),
        })?
    }
}

fn check_epoch_document(doc: &ProviderDocument, expected: &MeguriBinding,
    generation: u64, publication: Option<&Publication>) -> Result<()> {
    if doc.selection != Selection::Meguri || doc.meguri.as_ref() != Some(expected.connection())
        || doc.meguri_binding.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::BindingChanged); }
    if let Some(handoff) = &doc.handoff
        && (handoff.old != Selection::Meguri || handoff.generation != generation
            || handoff.publication.as_ref() != publication) {
        return Err(MeguriReadyRefusal::BindingChanged);
    }
    Ok(())
}

impl MeguriRoundAdmission {
    pub fn generation(&self) -> meguri_engine::Generation { self.collection.generation() }
    /// A sealed receipt permits precisely this task's own epoch publication.
    /// It never permits a foreign provider, account, publication or pause.
    pub fn check_finalized_media(&self, receipt: &crate::media::BoundMediaReceipt)
        -> std::result::Result<(), MeguriRoundRefusal> {
        use MeguriRoundRefusal as R;
        if !receipt.reset_occurred() { return self.check(); }
        if !self.binding.media_epoch.matches(receipt.initial()) || !self.original.is_live()
            || !self.collection.is_current() { return Err(R::BindingChanged); }
        let mut expected = self.binding.clone();
        expected.media_epoch.epoch = receipt.terminal().epoch;
        let doc = sidecar::read_document(&self.authority.path, decode)
            .map_err(|_| R::Storage)?.ok_or(R::Storage)?;
        routine_document(&doc, &expected, &self.publication)?;
        self.media.bound_sync_baseline(&expected.media_owner(), receipt.terminal().epoch).map_err(|error| match error {
            meguri_storage::media::MediaBoundCheckError::Changed => R::BindingChanged,
            meguri_storage::media::MediaBoundCheckError::Storage(_) => R::Storage,
        })?;
        if self.authority.network_blocked() { return Err(R::BindingChanged); }
        if self.policy.active().map_err(|_| R::Storage)?.is_some() { return Err(R::Busy); }
        if meguri_engine::device_state::DeviceState::for_collection(self.collection.path())
            .settled_publication().map_err(|_| R::Storage)?.as_ref() != Some(&self.publication) {
            return Err(R::PublicationChanged);
        }
        Ok(())
    }
}
impl Authority {
    // Same strict retained-before/desired replay as retry_failed_publication,
    // under the already-owned publication gate and original media journal.
    // The caller has additionally checked this task's exact pending facts.
    fn retry_round_publication_locked(&self, failure: &mut Option<PublicationFailure>) -> Result<()> {
        let failed = failure.as_ref().ok_or_else(invalid)?;
        let pending = failed.pending.as_ref().filter(|pending| pending.retryable).ok_or_else(|| failed.error.clone())?;
        let result = sidecar::update_document(&self.path, |raw| {
            if raw != &pending.before && raw != &pending.desired { return Err(invalid()); }
            decode(Some(raw))?.ok_or_else(invalid)?;
            *raw = pending.desired.clone();
            Ok(pending.document.clone())
        });
        match result {
            Ok(doc) => {
                let no_handoff = doc.handoff.is_none();
                *failure = None;
                if no_handoff { self.retire_original_completions(); }
                let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.document = Ok(doc);
                self.open.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
            Err(error) => {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).document = Err(failed.error.clone());
                Err(MeguriReadyRefusal::Storage(error))
            }
        }
    }
    /// Retry the actual failed hold only when the pending before/desired facts
    /// belong to this exact original task. Its joined terminal may settle its
    /// own handoff, without changing the durable intent or admitting work.
    pub fn finalize_meguri_round_hold(&self, admission: &MeguriRoundAdmission, generation: u64,
        publication: Option<Publication>) -> Result<()> {
        if !std::ptr::eq(self, std::sync::Arc::as_ptr(&admission.authority))
            || !admission.original.is_live() || !admission.collection.is_current()
            || admission.generation().0 != generation || publication.as_ref() != Some(&admission.publication) {
            return Err(MeguriReadyRefusal::BindingChanged);
        }
        let expected = admission.binding();
        let retry = {
            let failure = self.publication_failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match failure.as_ref() {
                None => false,
                Some(failed) => {
                    let pending = failed.pending.as_ref().ok_or_else(|| MeguriReadyRefusal::Storage(failed.error.clone()))?;
                    let before = decode(Some(&pending.before))?.ok_or_else(invalid)?;
                    let desired = &pending.document;
                    if before.meguri_binding.as_ref() != Some(expected) || desired.meguri_binding.is_some()
                        || desired.meguri_settings.automatic || desired.settings.automatic
                        || before.settings.paused != desired.settings.paused
                        || pending.facts.as_ref() != Some(&(generation, publication.clone())) {
                        return Err(MeguriReadyRefusal::BindingChanged);
                    }
                    check_epoch_document(&before, expected, generation, publication.as_ref())?;
                    let mut held = before.clone();
                    held.meguri_binding = None;
                    held.meguri_settings.automatic = false;
                    held.settings.automatic = false;
                    if held.handoff.is_none() { held.revision = held.revision.checked_add(1).ok_or_else(invalid)?; }
                    if desired != &held { return Err(MeguriReadyRefusal::BindingChanged); }
                    true
                }
            }
        };
        if retry { self.retry_failed_publication(generation, publication.as_ref())?; Ok(()) }
        else {
            let settled = publication.clone();
            self.bootstrap_update(generation, publication, |doc| {
                check_epoch_document(doc, expected, generation, settled.as_ref())?;
                doc.meguri_binding = None;
                doc.meguri_settings.automatic = false;
                doc.settings.automatic = false;
                // The original handoff handle pins this revision and intent.
                if doc.handoff.is_none() { doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?; }
                Ok(())
            })
        }
    }
}
