//! Opaque decisions over the original Host's retained bootstrap operations.
use super::{ProviderConnection, ProviderCredentials, ProviderOwner, information};
use crate::{CoreError, callback, jobs, manual_transfers::{TransferFailure, TransferMediaSnapshot, TransferRecord,
    TransferAcceptance, TransferBoundary, TransferCopy, TransferCounts, TransferDirection, TransferMediaPhase,
    TransferMediaStage, TransferObservation, TransferPhase}};
use meguri_host::{ManualTransferTask, MeguriAccess, MeguriBootstrapDirection, MeguriBootstrapError};
use meguri_sync::{HostKey, oneway::{MediaConfirmation, MediaPreview, Preview, TransferError},
    policy::Operation, provider::{MeguriConnection, MeguriReadiness, MeguriReadyRefusal, MediaRefusal},
    provider_media::{MediaAccounting, PromisedState}};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderBootstrapDirection { UseServerCollection, UseDeviceCollection }
impl From<ProviderBootstrapDirection> for MeguriBootstrapDirection {
    fn from(value: ProviderBootstrapDirection) -> Self {
        match value { ProviderBootstrapDirection::UseServerCollection => Self::UseServerCollection,
            ProviderBootstrapDirection::UseDeviceCollection => Self::UseDeviceCollection }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderMediaState { Transferred, Pending, Excluded }
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderMediaExclusion { InvalidName, NotRegularFile, Empty, OverLimit }
// Filenames are display payloads only. Neither these records nor their containers
// derive Debug; failures carry static categories and optional display accounting.
#[derive(Clone, uniffi::Record)]
pub struct ProviderMediaFile {
    pub name: String, pub lossy_name: bool, pub state: ProviderMediaState, pub exclusion: Option<ProviderMediaExclusion>,
}
#[derive(Clone, uniffi::Record)]
pub struct ProviderMediaAccounting {
    pub files: Vec<ProviderMediaFile>, pub remote_pending: u64, pub transferred: u64,
}
impl From<&MediaAccounting> for ProviderMediaAccounting {
    fn from(value: &MediaAccounting) -> Self {
        Self { files: value.files().iter().map(|file| ProviderMediaFile { name: file.name.clone(), lossy_name: file.lossy_name,
            state: match file.state { PromisedState::Transferred => ProviderMediaState::Transferred,
                PromisedState::Pending => ProviderMediaState::Pending, PromisedState::Excluded(_) => ProviderMediaState::Excluded },
            exclusion: match file.state {
                PromisedState::Excluded(reason) => Some(match reason {
                    meguri_storage::media::UnsupportedMedia::InvalidName => ProviderMediaExclusion::InvalidName,
                    meguri_storage::media::UnsupportedMedia::NotRegularFile => ProviderMediaExclusion::NotRegularFile,
                    meguri_storage::media::UnsupportedMedia::Empty => ProviderMediaExclusion::Empty,
                    meguri_storage::media::UnsupportedMedia::OverLimit => ProviderMediaExclusion::OverLimit,
                }), _ => None,
            } }).collect(),
            remote_pending: value.remote_pending(), transferred: value.transferred() }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderBootstrapFailure {
    NotSelected, ConnectionChanged, Busy, AlreadyBound, Username, NoIntent, OperationMismatch,
    NotComplete, AccountChanged, PublicationChanged, DocumentChanged, BindingChanged,
    MediaEpochChanged, MediaEpochMissing, MediaIncomplete, Cancelled, DiskFull, Unavailable, Storage,
    Transfer { failure: TransferFailure },
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderBootstrapOutcome {
    Pending,
    Preview { preview: Arc<ProviderBootstrapPreview> },
    MediaConsent { consent: Arc<ProviderBootstrapMediaConsent> },
    Operation { operation: Arc<ProviderBootstrapOperation> },
    Ready { accounting: ProviderMediaAccounting },
    Refused { failure: ProviderBootstrapFailure, accounting: Option<ProviderMediaAccounting> },
}
fn refused(failure: ProviderBootstrapFailure) -> ProviderBootstrapOutcome {
    ProviderBootstrapOutcome::Refused { failure, accounting: None }
}
fn failure(error: MeguriBootstrapError) -> ProviderBootstrapOutcome {
    use MeguriBootstrapError as E;
    use MeguriReadyRefusal as R;
    use ProviderBootstrapFailure as F;
    refused(match error {
        E::NotSelected | E::Ready(R::NotSelected) => F::NotSelected,
        E::ConnectionChanged | E::Ready(R::ConnectionChanged) => F::ConnectionChanged,
        E::Busy | E::Engine(meguri_engine::EngineError::Busy { .. }) => F::Busy,
        E::AlreadyBound => F::AlreadyBound, E::Username => F::Username,
        E::Transfer(error) => return transfer_failure(error),
        E::Ready(R::NoIntent) => F::NoIntent, E::Ready(R::OperationMismatch) => F::OperationMismatch,
        E::Ready(R::NotComplete) => F::NotComplete, E::Ready(R::AccountChanged) => F::AccountChanged,
        E::Ready(R::PublicationChanged) => F::PublicationChanged, E::Ready(R::DocumentChanged) => F::DocumentChanged,
        E::Ready(R::BindingChanged) => F::BindingChanged, E::Ready(R::MediaEpochChanged) => F::MediaEpochChanged,
        E::Ready(R::MediaEpochMissing) => F::MediaEpochMissing,
        E::Ready(R::MediaIncomplete(accounting)) => return ProviderBootstrapOutcome::Refused {
            failure: F::MediaIncomplete, accounting: Some((&accounting).into()) },
        E::Ready(R::Media(MediaRefusal::Cancelled)) => F::Cancelled,
        E::Ready(R::Media(MediaRefusal::DiskFull)) => F::DiskFull,
        E::Ready(R::Media(MediaRefusal::Unavailable)) => F::Unavailable,
        E::Ready(R::Storage(_)) => F::Storage, E::Engine(_) => F::Unavailable,
    })
}
fn transfer_failure(error: TransferError) -> ProviderBootstrapOutcome {
    refused(ProviderBootstrapFailure::Transfer { failure: error.into() })
}
fn invalid() -> CoreError { CoreError::InvalidArgument { message: "Meguri bootstrap decision or owner changed".into() } }

#[derive(Clone, uniffi::Record)]
pub struct ProviderBootstrapMediaEffects {
    pub stage: TransferMediaStage, pub uploaded_files: u64, pub downloaded_files: u64,
    pub uploaded_deletions: u64, pub downloaded_deletions: u64,
    pub oversized: Vec<String>, pub empty: Vec<String>, pub awaiting: u64,
    pub remote_outcome_unknown: bool,
}
#[derive(Clone, uniffi::Record)]
pub struct ProviderBootstrapRecord {
    pub direction: TransferDirection, pub phase: TransferPhase, pub acceptance: TransferAcceptance,
    pub boundary: Option<TransferBoundary>, pub local: Option<TransferCounts>, pub remote: Option<TransferObservation>,
    pub remote_has_no_cards: Option<bool>, pub served_counts: Option<TransferCounts>, pub copies: Vec<TransferCopy>,
    pub media: ProviderBootstrapMediaEffects,
}
impl From<&Operation> for ProviderBootstrapRecord {
    fn from(value: &Operation) -> Self {
        let record: TransferRecord = value.into();
        let m = &value.media;
        Self { direction: record.direction, phase: record.phase, acceptance: record.acceptance, boundary: record.boundary,
            local: record.local, remote: record.remote, remote_has_no_cards: record.remote_has_no_cards,
            served_counts: record.served_counts, copies: record.copies,
            media: ProviderBootstrapMediaEffects { stage: m.stage.into(), uploaded_files: m.counters.uploaded_files,
                downloaded_files: m.counters.downloaded_files, uploaded_deletions: m.counters.uploaded_deletions,
                downloaded_deletions: m.counters.downloaded_deletions, oversized: m.counters.oversized.clone(),
                empty: m.counters.empty.clone(), awaiting: m.counters.awaiting, remote_outcome_unknown: m.remote_outcome_unknown } }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct ProviderBootstrapMediaProgress {
    pub phase: TransferMediaPhase, pub checked: u64, pub uploaded_files: u64, pub downloaded_files: u64,
    pub bytes_uploaded: u64, pub bytes_downloaded: u64, pub awaiting: u64,
}
impl From<&meguri_sync::media::MediaSnapshot> for ProviderBootstrapMediaProgress {
    fn from(value: &meguri_sync::media::MediaSnapshot) -> Self {
        let snapshot: TransferMediaSnapshot = value.into();
        Self { phase: snapshot.phase, checked: snapshot.counters.checked, uploaded_files: snapshot.counters.uploaded_files,
            downloaded_files: snapshot.counters.downloaded_files, bytes_uploaded: snapshot.counters.bytes_uploaded,
            bytes_downloaded: snapshot.counters.bytes_downloaded, awaiting: snapshot.counters.awaiting }
    }
}

#[derive(uniffi::Object)]
pub struct ProviderBootstrapPreview {
    owner: Arc<ProviderOwner>, connection: MeguriConnection, inner: Preview, consumed: Mutex<bool>,
}
#[derive(uniffi::Object)]
pub struct ProviderBootstrapMediaConsent {
    owner: Arc<ProviderOwner>, connection: MeguriConnection, inner: MediaPreview, consumed: Mutex<bool>,
}
#[derive(uniffi::Object)]
pub struct ProviderBootstrapOperation { owner: Arc<ProviderOwner>, connection: MeguriConnection, inner: Operation }
#[uniffi::export]
impl ProviderBootstrapPreview {
    pub fn record(&self) -> Result<ProviderBootstrapRecord, CoreError> {
        callback::enter("ProviderBootstrapPreview.record");
        Ok((&self.inner.operation).into())
    }
}
#[uniffi::export]
impl ProviderBootstrapMediaConsent {
    pub fn record(&self) -> Result<ProviderBootstrapRecord, CoreError> {
        callback::enter("ProviderBootstrapMediaConsent.record");
        Ok((&self.inner.operation).into())
    }
}
#[uniffi::export]
impl ProviderBootstrapOperation {
    pub fn record(&self) -> Result<ProviderBootstrapRecord, CoreError> {
        callback::enter("ProviderBootstrapOperation.record");
        Ok((&self.inner).into())
    }
}
fn consume(consumed: &Mutex<bool>) -> Result<(), CoreError> {
    let mut consumed = consumed.lock().unwrap_or_else(PoisonError::into_inner);
    if *consumed { return Err(invalid()); }
    *consumed = true;
    Ok(())
}
enum Source {
    Cell(ProviderBootstrapOutcome), Preview(Arc<ManualTransferTask<Preview>>),
    Media(ManualTransferTask<MediaPreview>), Operation(Arc<ManualTransferTask<Operation>>),
}
#[derive(uniffi::Object)]
pub struct ProviderBootstrapResult {
    owner: Arc<ProviderOwner>, connection: MeguriConnection, source: Mutex<Source>,
    preparation: Option<Arc<ManualTransferTask<Preview>>>, media_task: Option<Arc<ManualTransferTask<Operation>>>,
}
#[derive(Clone, uniffi::Record)]
pub struct ProviderBootstrapJob {
    /// A synchronous admission refusal has no worker. Read result in either case.
    pub handle: Option<jobs::JobHandle>, pub result: Arc<ProviderBootstrapResult>,
}
#[uniffi::export]
impl ProviderBootstrapResult {
    /// Read after the matching JobFinished; pre-entry cancellation can have no
    /// typed worker result and must use that event's actual job outcome.
    pub fn outcome(&self) -> Result<ProviderBootstrapOutcome, CoreError> {
        callback::enter("ProviderBootstrapResult.outcome");
        let mut source = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        let value = match &*source {
            Source::Cell(value) => return Ok(value.clone()),
            Source::Preview(task) => task.take_result().map(|result| match result {
                Ok(inner) => ProviderBootstrapOutcome::Preview { preview: Arc::new(ProviderBootstrapPreview {
                    owner: self.owner.clone(), connection: self.connection.clone(), inner, consumed: Mutex::new(false) }) },
                Err(error) => transfer_failure(error),
            }),
            Source::Media(task) => task.take_result().map(|result| match result {
                Ok(inner) => ProviderBootstrapOutcome::MediaConsent { consent: Arc::new(ProviderBootstrapMediaConsent {
                    owner: self.owner.clone(), connection: self.connection.clone(), inner, consumed: Mutex::new(false) }) },
                Err(error) => transfer_failure(error),
            }),
            Source::Operation(task) => task.take_result().map(|result| match result {
                Ok(inner) => ProviderBootstrapOutcome::Operation { operation: Arc::new(ProviderBootstrapOperation {
                    owner: self.owner.clone(), connection: self.connection.clone(), inner }) },
                Err(error) => transfer_failure(error),
            }),
        };
        match value {
            Some(value) => { *source = Source::Cell(value.clone()); Ok(value) }
            None => Ok(ProviderBootstrapOutcome::Pending),
        }
    }
    pub fn media_snapshot(&self) -> Result<Option<ProviderBootstrapMediaProgress>, CoreError> {
        callback::enter("ProviderBootstrapResult.media_snapshot");
        Ok(self.media_task.as_ref().and_then(|task| task.media_snapshot()).as_ref().map(Into::into))
    }
    pub fn request_stop(&self, interrupted: bool) -> Result<(), CoreError> {
        callback::enter("ProviderBootstrapResult.request_stop");
        if let Some(task) = &self.media_task { task.stop_media(interrupted); }
        Ok(())
    }
    /// The task's assigned policy identity survives consumption of its result.
    /// This object provides a retry handle only; it never certifies completion.
    pub fn retained_operation(&self) -> Result<Option<Arc<ProviderBootstrapOperation>>, CoreError> {
        callback::enter("ProviderBootstrapResult.retained_operation");
        self.owner.validate()?;
        let Some(id) = self.preparation.as_ref().and_then(|task| task.operation_id()) else { return Ok(None); };
        let inner = self.owner.host.manual_transfer_history().get(&id).map_err(|_| CoreError::Unavailable {
            reason: "Meguri bootstrap history unavailable".into() })?;
        Ok(Some(Arc::new(ProviderBootstrapOperation { owner: self.owner.clone(), connection: self.connection.clone(), inner })))
    }
}
impl ProviderConnection {
    fn discard_preparation(&self, operation: &Operation) -> ProviderBootstrapOutcome {
        match self.owner.host.discard_meguri_bootstrap_preparation_for_generation(self.owner.generation, &self.inner, operation) {
            Ok(inner) => ProviderBootstrapOutcome::Operation { operation: Arc::new(ProviderBootstrapOperation {
                owner: self.owner.clone(), connection: self.inner.clone(), inner }) },
            Err(error) => failure(error),
        }
    }
    fn access(&self, credentials: ProviderCredentials) -> Result<MeguriAccess, MeguriBootstrapError> {
        let expected = information(&self.inner);
        if credentials.connection.reference != expected.reference || credentials.connection.revision != expected.revision
            || credentials.connection.endpoint != expected.endpoint { return Err(MeguriBootstrapError::ConnectionChanged); }
        MeguriAccess::new(self.inner.clone(), &credentials.username, HostKey::new(credentials.hostkey))
    }
    fn same_host(&self, owner: &ProviderOwner, connection: &MeguriConnection) -> Result<(), CoreError> {
        // Retained operation capabilities survive their own activation on the
        // same Host. The current caller and Host policy still gate every use.
        if !Arc::ptr_eq(&self.owner.host, &owner.host) || &self.inner != connection { return Err(invalid()); }
        Ok(())
    }
    fn same_current_owner(&self, owner: &ProviderOwner, connection: &MeguriConnection) -> Result<(), CoreError> {
        self.same_host(owner, connection)?;
        owner.validate()?;
        if self.owner.generation != owner.generation { return Err(invalid()); }
        Ok(())
    }
    fn job(&self, source: Source) -> ProviderBootstrapJob {
        let (handle, preparation, media_task) = match &source {
            Source::Cell(_) => (None, None, None),
            Source::Preview(task) => (Some(jobs::wrap(task.job.clone())), Some(task.clone()), None),
            Source::Media(task) => (Some(jobs::wrap(task.job.clone())), None, None),
            Source::Operation(task) => (Some(jobs::wrap(task.job.clone())), None, Some(task.clone())),
        };
        ProviderBootstrapJob { handle, result: Arc::new(ProviderBootstrapResult { owner: self.owner.clone(),
            connection: self.inner.clone(), source: Mutex::new(source), preparation, media_task }) }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderBootstrapReadiness {
    NotSelected, Unbound { operation: Option<Arc<ProviderBootstrapOperation>> }, Ready, Stale,
}
#[uniffi::export]
impl ProviderConnection {
    /// Local account selection only. The returned handle comes from this
    /// original Host's actual active policy, including pre-confirmation state.
    pub fn pending_bootstrap_preparation(&self, username: String) -> Result<Option<Arc<ProviderBootstrapOperation>>, CoreError> {
        callback::enter("ProviderConnection.pending_bootstrap_preparation");
        self.owner.validate()?;
        let inner = self.owner.host.pending_meguri_bootstrap_preparation_for_generation(self.owner.generation, &self.inner, &username)
            .map_err(|_| CoreError::Unavailable { reason: "Meguri pending preparation unavailable".into() })?;
        Ok(inner.map(|inner| Arc::new(ProviderBootstrapOperation {
            owner: self.owner.clone(), connection: self.inner.clone(), inner })))
    }
    pub fn discard_bootstrap_preview(&self, preview: Arc<ProviderBootstrapPreview>) -> Result<ProviderBootstrapOutcome, CoreError> {
        callback::enter("ProviderConnection.discard_bootstrap_preview");
        self.owner.validate()?;
        self.same_current_owner(&preview.owner, &preview.connection)?;
        consume(&preview.consumed)?;
        Ok(self.discard_preparation(&preview.inner.operation))
    }
    pub fn discard_bootstrap_operation(&self, operation: Arc<ProviderBootstrapOperation>) -> Result<ProviderBootstrapOutcome, CoreError> {
        callback::enter("ProviderConnection.discard_bootstrap_operation");
        self.owner.validate()?;
        self.same_current_owner(&operation.owner, &operation.connection)?;
        Ok(self.discard_preparation(&operation.inner))
    }
    pub fn start_bootstrap_recovery(&self, operation: Arc<ProviderBootstrapOperation>) -> Result<ProviderBootstrapJob, CoreError> {
        callback::enter("ProviderConnection.start_bootstrap_recovery");
        self.owner.validate()?;
        self.same_current_owner(&operation.owner, &operation.connection)?;
        let source = match self.owner.host.prepare_meguri_bootstrap_recovery_for_generation(self.owner.generation, &self.inner, &operation.inner) {
            Ok(task) => Source::Preview(Arc::new(task)), Err(error) => Source::Cell(failure(error)),
        };
        Ok(self.job(source))
    }
    pub fn start_bootstrap_prepare(&self, credentials: ProviderCredentials, direction: ProviderBootstrapDirection) -> Result<ProviderBootstrapJob, CoreError> {
        callback::enter("ProviderConnection.start_bootstrap_prepare");
        self.owner.validate()?;
        let source = match self.access(credentials).and_then(|access| self.owner.host.prepare_meguri_bootstrap(access, direction.into())) {
            Ok(task) => Source::Preview(Arc::new(task)), Err(error) => Source::Cell(failure(error)),
        };
        Ok(self.job(source))
    }
    pub fn start_bootstrap_confirm(&self, credentials: ProviderCredentials, preview: Arc<ProviderBootstrapPreview>) -> Result<ProviderBootstrapJob, CoreError> {
        callback::enter("ProviderConnection.start_bootstrap_confirm");
        self.owner.validate()?;
        self.same_current_owner(&preview.owner, &preview.connection)?;
        if self.owner.generation != preview.owner.generation { return Err(invalid()); }
        let access = match self.access(credentials) { Ok(access) => access, Err(error) => return Ok(self.job(Source::Cell(failure(error)))) };
        consume(&preview.consumed)?;
        let source = match self.owner.host.confirm_meguri_bootstrap(access, preview.inner.clone()) {
            Ok(task) => Source::Operation(Arc::new(task)), Err(error) => Source::Cell(failure(error)),
        };
        Ok(self.job(source))
    }
    pub fn start_bootstrap_media_prepare(&self, credentials: ProviderCredentials, operation: Arc<ProviderBootstrapOperation>) -> Result<ProviderBootstrapJob, CoreError> {
        callback::enter("ProviderConnection.start_bootstrap_media_prepare");
        self.owner.validate()?;
        self.same_host(&operation.owner, &operation.connection)?;
        let source = match self.access(credentials).and_then(|access| self.owner.host.prepare_meguri_bootstrap_media(access, operation.inner.id.clone())) {
            Ok(task) => Source::Media(task), Err(error) => Source::Cell(failure(error)),
        };
        Ok(self.job(source))
    }
    pub fn start_bootstrap_media(&self, credentials: ProviderCredentials, consent: Arc<ProviderBootstrapMediaConsent>) -> Result<ProviderBootstrapJob, CoreError> {
        callback::enter("ProviderConnection.start_bootstrap_media");
        self.owner.validate()?;
        self.same_current_owner(&consent.owner, &consent.connection)?;
        if self.owner.generation != consent.owner.generation { return Err(invalid()); }
        let access = match self.access(credentials) { Ok(access) => access, Err(error) => return Ok(self.job(Source::Cell(failure(error)))) };
        consume(&consent.consumed)?;
        let confirmation: MediaConfirmation = consent.inner.confirmation.clone();
        let source = match self.owner.host.run_meguri_bootstrap_media_for_generation(self.owner.generation, access, confirmation) {
            Ok(task) => Source::Operation(Arc::new(task)), Err(error) => Source::Cell(failure(error)),
        };
        Ok(self.job(source))
    }
    pub fn complete_bootstrap(&self, credentials: ProviderCredentials, operation: Arc<ProviderBootstrapOperation>) -> Result<ProviderBootstrapOutcome, CoreError> {
        callback::enter("ProviderConnection.complete_bootstrap");
        self.owner.validate()?;
        self.same_host(&operation.owner, &operation.connection)?;
        let username = credentials.username.clone();
        let _access = match self.access(credentials) { Ok(access) => access, Err(error) => return Ok(failure(error)) };
        // The host derives account authority from this canonical username and
        // its own policy. No completion/accounting fact comes from the facade.
        Ok(match self.owner.host.complete_meguri_bootstrap_for_generation(self.owner.generation, &self.inner, &username, &operation.inner.id) {
            Ok(accounting) => ProviderBootstrapOutcome::Ready { accounting: (&accounting).into() },
            Err(error) => failure(error),
        })
    }
    pub fn bootstrap_readiness(&self) -> Result<ProviderBootstrapReadiness, CoreError> {
        callback::enter("ProviderConnection.bootstrap_readiness");
        self.owner.validate()?;
        let doc = self.owner.host.provider_authority().snapshot()?;
        if doc.meguri.as_ref() != Some(&self.inner) { return Err(invalid()); }
        let state = self.owner.host.meguri_readiness().map_err(|_| CoreError::Unavailable { reason: "Meguri bootstrap state unavailable".into() })?;
        Ok(match state {
            MeguriReadiness::NotSelected => ProviderBootstrapReadiness::NotSelected,
            MeguriReadiness::Ready => ProviderBootstrapReadiness::Ready,
            MeguriReadiness::Stale => ProviderBootstrapReadiness::Stale,
            MeguriReadiness::Unbound { intent } => {
                let operation = intent.map(|id| self.owner.host.manual_transfer_history().get(&id)
                    .map(|inner| Arc::new(ProviderBootstrapOperation { owner: self.owner.clone(), connection: self.inner.clone(), inner }))
                    .map_err(|_| CoreError::Unavailable { reason: "Meguri bootstrap history unavailable".into() })).transpose()?;
                ProviderBootstrapReadiness::Unbound { operation }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_config(root: &std::path::Path) -> meguri_host::HostConfig {
        use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
        let collection = root.join("collection");
        std::fs::create_dir_all(&collection).unwrap();
        meguri_host::HostConfig::new(collection, root.join("backups"), root.join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)), Arc::new(SeededRandom::new(171)),
            Arc::new(meguri_storage::space::StatvfsSpace))
    }
    fn connection(root: &std::path::Path) -> Arc<ProviderConnection> {
        let config = fixture_config(root);
        meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
        let host = Arc::new(meguri_host::CoreHost::open(config).unwrap());
        let generation = host.cloud_connection_generation().unwrap();
        let inner = host.configure_meguri_connection("https://bootstrap.synthetic.example/").unwrap();
        Arc::new(ProviderConnection { owner: Arc::new(ProviderOwner { host, generation }), inner })
    }
    fn credentials(connection: &ProviderConnection) -> ProviderCredentials {
        ProviderCredentials { connection: information(&connection.inner), username: " Synthetic-User ".into(), hostkey: "synthetic-key".into() }
    }
    #[test]
    fn credentials_match_all_connection_facts_before_any_worker_starts() {
        let root = tempfile::tempdir().unwrap();
        let connection = connection(root.path());
        for field in ["reference", "revision", "endpoint"] {
            let mut credentials = credentials(&connection);
            match field {
                "reference" => credentials.connection.reference.push('x'),
                "revision" => credentials.connection.revision += 1,
                _ => credentials.connection.endpoint = "https://foreign.synthetic.example/".into(),
            }
            let job = connection.start_bootstrap_prepare(credentials, ProviderBootstrapDirection::UseDeviceCollection).unwrap();
            assert!(job.handle.is_none());
            assert!(matches!(job.result.outcome().unwrap(), ProviderBootstrapOutcome::Refused {
                failure: ProviderBootstrapFailure::ConnectionChanged, accounting: None }));
            assert!(connection.owner.host.manual_transfer_history().active().unwrap().is_none());
        }
        let job = connection.start_bootstrap_prepare(credentials(&connection), ProviderBootstrapDirection::UseServerCollection).unwrap();
        assert!(job.handle.is_none());
        assert!(matches!(job.result.outcome().unwrap(), ProviderBootstrapOutcome::Refused {
            failure: ProviderBootstrapFailure::NotSelected, .. }));
        connection.owner.host.close().unwrap();
        assert!(connection.bootstrap_readiness().is_err());
    }
    #[test]
    fn opaque_decision_is_consumed_once() {
        let consumed = Mutex::new(false);
        consume(&consumed).unwrap();
        assert!(consume(&consumed).is_err());
    }
    #[test]
    fn media_refusals_drop_all_inner_text() {
        for error in [meguri_storage::media::MediaError::Io("private-name".into()),
            meguri_storage::media::MediaError::Database("private-name".into()),
            meguri_storage::media::MediaError::InvalidManifest("private-name".into())] {
            let refusal = MeguriReadyRefusal::from(error);
            assert_eq!(format!("{refusal:?}"), "Media");
            assert!(matches!(failure(MeguriBootstrapError::Ready(refusal)),
                ProviderBootstrapOutcome::Refused { failure: ProviderBootstrapFailure::Unavailable, accounting: None }));
        }
    }
    #[test]
    fn epoch_and_binding_refusals_remain_distinct() {
        for (input, expected) in [(MeguriReadyRefusal::MediaEpochChanged, 0),
            (MeguriReadyRefusal::MediaEpochMissing, 1), (MeguriReadyRefusal::BindingChanged, 2)] {
            let outcome = failure(MeguriBootstrapError::Ready(input));
            assert!(matches!((expected, outcome),
                (0, ProviderBootstrapOutcome::Refused { failure: ProviderBootstrapFailure::MediaEpochChanged, .. }) |
                (1, ProviderBootstrapOutcome::Refused { failure: ProviderBootstrapFailure::MediaEpochMissing, .. }) |
                (2, ProviderBootstrapOutcome::Refused { failure: ProviderBootstrapFailure::BindingChanged, .. })));
        }
    }
    // The injected transport interrupts this job before a response. The real
    // Rust worker/policy supplies the admitted identity; no Preview or operation
    // is constructed from display data. This covers interrupted preparation,
    // not native delivery of a successful collection preview.
    struct InterruptedPreparation(Arc<std::sync::atomic::AtomicUsize>);
    impl meguri_sync::SyncTransport for InterruptedPreparation {
        fn post(&self, _: meguri_sync::transport::TransportRequest)
            -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
            Box::pin(async move {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                panic!("synthetic preparation worker interruption")
            })
        }
    }
    fn interrupted_preparation(connection: &ProviderConnection) -> Arc<std::sync::atomic::AtomicUsize> {
        let host = &connection.owner.host;
        let handoff = host.begin_provider_handoff(meguri_sync::provider::Selection::Meguri).unwrap();
        host.finish_provider_handoff(&handoff).unwrap();
        host.collection().unwrap().run(meguri_engine::OpSpec::named("bootstrap fixture barrier", meguri_engine::OpSource::Rollover), |_| Ok(())).unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let access = connection.access(credentials(connection)).unwrap()
            .with_transport_for_testing(Box::new(InterruptedPreparation(calls.clone())));
        let events = host.engine().stream().subscribe();
        let task = host.prepare_meguri_bootstrap(access, MeguriBootstrapDirection::UseDeviceCollection).unwrap();
        let id = task.job.id;
        let wrapped = connection.job(Source::Preview(Arc::new(task)));
        let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(20);
        loop {
            let event = events.recv_timeout(deadline.saturating_sub(meguri_engine::monotonic::now())).expect("interrupted worker deadline");
            if matches!(event.kind, meguri_engine::EventKind::JobFinished { id: actual, .. } if actual == id) { break; }
        }
        assert!(matches!(wrapped.result.outcome().unwrap(), ProviderBootstrapOutcome::Pending));
        let retained = wrapped.result.retained_operation().unwrap().unwrap();
        assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&retained.inner));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        calls
    }
    #[test]
    fn ffi_actual_interrupted_pending_operation_can_be_discarded_warm_or_cold() {
        for cold in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let original = connection(root.path());
            let calls = interrupted_preparation(&original);
            let old = original.pending_bootstrap_preparation("synthetic-user".into()).unwrap().unwrap();
            let current = if cold {
                original.owner.host.close().unwrap();
                let host = Arc::new(meguri_host::CoreHost::open(fixture_config(root.path())).unwrap());
                let generation = host.cloud_connection_generation().unwrap();
                let inner = host.provider_authority().snapshot().unwrap().meguri.unwrap();
                Arc::new(ProviderConnection { owner: Arc::new(ProviderOwner { host, generation }), inner })
            } else { original.clone() };
            assert!(current.pending_bootstrap_preparation("other-user".into()).is_err());
            let pending = current.pending_bootstrap_preparation(" Synthetic-User ".into()).unwrap().unwrap();
            assert_eq!(pending.inner, old.inner);
            if cold {
                assert!(original.pending_bootstrap_preparation("synthetic-user".into()).is_err());
                assert!(current.discard_bootstrap_operation(old.clone()).is_err());
                assert!(current.start_bootstrap_recovery(old).is_err());
            }
            let outcome = current.discard_bootstrap_operation(pending.clone()).unwrap();
            let ProviderBootstrapOutcome::Operation { operation } = outcome else { panic!("actual cancellation receipt required") };
            assert_eq!(operation.inner.phase, meguri_sync::policy::Phase::Cancelled);
            assert!(current.owner.host.manual_transfer_history().active().unwrap().is_none());
            assert_eq!(current.owner.host.manual_transfer_history().get(&pending.inner.id).unwrap(), operation.inner);
            assert!(current.pending_bootstrap_preparation("synthetic-user".into()).unwrap().is_none());
            assert!(matches!(current.discard_bootstrap_operation(pending).unwrap(), ProviderBootstrapOutcome::Refused {
                failure: ProviderBootstrapFailure::OperationMismatch, accounting: None }));
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            current.owner.host.close().unwrap();
        }
    }
    #[test]
    fn ffi_actual_pending_operation_rejects_foreign_host_and_changed_connection_without_discard() {
        let root = tempfile::tempdir().unwrap();
        let original = connection(root.path());
        let calls = interrupted_preparation(&original);
        let pending = original.pending_bootstrap_preparation("synthetic-user".into()).unwrap().unwrap();
        let other_root = tempfile::tempdir().unwrap();
        let foreign = connection(other_root.path());
        assert!(foreign.discard_bootstrap_operation(pending.clone()).is_err());
        assert!(foreign.start_bootstrap_recovery(pending.clone()).is_err());
        for field in ["reference", "revision", "endpoint"] {
            let mut inner = original.inner.clone();
            match field { "reference" => inner.reference.push('x'), "revision" => inner.revision += 1,
                _ => inner.endpoint = "https://foreign.synthetic.example/".into() }
            let wrong = ProviderConnection { owner: original.owner.clone(), inner };
            assert!(wrong.discard_bootstrap_operation(pending.clone()).is_err());
            assert!(wrong.start_bootstrap_recovery(pending.clone()).is_err());
            assert!(wrong.pending_bootstrap_preparation("synthetic-user".into()).is_err());
        }
        assert_eq!(original.owner.host.manual_transfer_history().active().unwrap().as_ref(), Some(&pending.inner));
        assert!(matches!(original.discard_bootstrap_operation(pending).unwrap(), ProviderBootstrapOutcome::Operation { .. }));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        original.owner.host.close().unwrap();
        foreign.owner.host.close().unwrap();
    }
    #[test]
    fn ffi_actual_offline_recovery_keeps_not_sent_interruption_typed_and_old_decision_stale() {
        let root = tempfile::tempdir().unwrap();
        let connection = connection(root.path());
        let calls = interrupted_preparation(&connection);
        let pending = connection.pending_bootstrap_preparation("synthetic-user".into()).unwrap().unwrap();
        let events = connection.owner.host.engine().stream().subscribe();
        let started = connection.start_bootstrap_recovery(pending.clone()).unwrap();
        let id = started.handle.as_ref().unwrap().id;
        let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(20);
        loop {
            let event = events.recv_timeout(deadline.saturating_sub(meguri_engine::monotonic::now())).expect("offline recovery deadline");
            if matches!(event.kind, meguri_engine::EventKind::JobFinished { id: actual, .. } if actual.0 == id) { break; }
        }
        assert!(matches!(started.result.outcome().unwrap(), ProviderBootstrapOutcome::Refused {
            failure: ProviderBootstrapFailure::Transfer { failure: TransferFailure {
                boundary: Some(TransferBoundary::PreparationInterrupted), .. } }, accounting: None }));
        assert!(matches!(connection.discard_bootstrap_operation(pending).unwrap(), ProviderBootstrapOutcome::Refused {
            failure: ProviderBootstrapFailure::OperationMismatch, accounting: None }));
        let fresh = connection.pending_bootstrap_preparation("synthetic-user".into()).unwrap().unwrap();
        assert_eq!(fresh.inner.phase, meguri_sync::policy::Phase::NeedsDecision);
        assert!(matches!(connection.discard_bootstrap_operation(fresh).unwrap(), ProviderBootstrapOutcome::Operation { .. }));
        connection.owner.host.close().unwrap();
        started.result.request_stop(true).unwrap();
        assert!(matches!(started.result.outcome().unwrap(), ProviderBootstrapOutcome::Refused {
            failure: ProviderBootstrapFailure::Transfer { failure: TransferFailure {
                boundary: Some(TransferBoundary::PreparationInterrupted), .. } }, accounting: None }));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn reused_transfer_refusal_keeps_unknown_acceptance_typed_and_redacted() {
        let error = MeguriBootstrapError::Transfer(TransferError::NeedsDecision(meguri_sync::policy::Boundary::RemoteCompletionUnknown));
        assert_eq!(format!("{error:?}"), "Transfer");
        assert!(matches!(failure(error), ProviderBootstrapOutcome::Refused {
            failure: ProviderBootstrapFailure::Transfer { failure: TransferFailure {
                boundary: Some(TransferBoundary::RemoteCompletionUnknown), .. } }, accounting: None }));
    }

    // Only this injected transport routes the synthetic configured endpoint to
    // the job-owned loopback stub. Provider/policy identity remains unchanged.
    struct BootstrapFixtureTransport {
        target: String, http: meguri_sync::transport::http::HttpTransport,
    }
    impl meguri_sync::SyncTransport for BootstrapFixtureTransport {
        fn post(&self, mut request: meguri_sync::transport::TransportRequest)
            -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
            let path = request.url.strip_prefix("https://bootstrap.synthetic.example/").expect("synthetic endpoint only");
            request.url = format!("{}{path}", self.target);
            request.stall_timeout = std::time::Duration::from_secs(5);
            meguri_sync::SyncTransport::post(&self.http, request)
        }
    }
    fn fixture_access(connection: &ProviderConnection, stub: &BootstrapFixtureServer) -> MeguriAccess {
        let target = stub.target.clone();
        let options = meguri_sync::JobOptions::new(meguri_sync::Endpoint::parse(&target).unwrap(), meguri_sync::ServerKind::SelfHosted)
            .stall_timeout(std::time::Duration::from_secs(5));
        connection.access(credentials(connection)).unwrap().with_transport_for_testing(Box::new(BootstrapFixtureTransport {
            target, http: meguri_sync::transport::http::HttpTransport::new(&options).unwrap(),
        }))
    }
    fn fixture_outcome(connection: &ProviderConnection, start: impl FnOnce() -> ProviderBootstrapJob) -> ProviderBootstrapOutcome {
        let events = connection.owner.host.engine().stream().subscribe();
        let job = start();
        let id = job.handle.as_ref().expect("actual worker required").id;
        let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(20);
        loop {
            let event = events.recv_timeout(deadline.saturating_sub(meguri_engine::monotonic::now())).expect("bootstrap fixture worker deadline");
            if matches!(event.kind, meguri_engine::EventKind::JobFinished { id: actual, .. } if actual.0 == id) { break; }
        }
        job.result.outcome().unwrap()
    }
    // A finite, job-owned loopback responder. Raw zstd blocks keep this fixture
    // independent of testkit (and of duplicate Host types in in-file tests).
    struct BootstrapFixtureServer {
        target: String, stop: Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<std::io::Result<Vec<String>>>>,
    }
    impl BootstrapFixtureServer {
        fn new(replies: Vec<(&'static str, Vec<u8>)>) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let target = format!("http://{}/", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopped = stop.clone();
            let worker = std::thread::spawn(move || {
                use std::io::{BufRead, Read, Write};
                let mut paths = Vec::new();
                for (expected, payload) in replies {
                    let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(20);
                    let (mut socket, _) = loop {
                        if stopped.load(std::sync::atomic::Ordering::SeqCst) || meguri_engine::monotonic::now() >= deadline {
                            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "fixture request deadline"));
                        }
                        match listener.accept() {
                            Ok(socket) => break socket,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(5)),
                            Err(error) => return Err(error),
                        }
                    };
                    socket.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                    socket.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
                    let mut reader = std::io::BufReader::new(socket.try_clone()?);
                    let mut line = String::new();
                    reader.read_line(&mut line)?;
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
                    paths.push(path.clone());
                    if path != expected {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected fixture request"));
                    }
                    let mut length = 0;
                    let mut chunked = false;
                    loop {
                        line.clear();
                        if reader.read_line(&mut line)? == 0 {
                            return Err(std::io::ErrorKind::UnexpectedEof.into());
                        }
                        if line == "\r\n" { break; }
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                length = value.trim().parse::<u64>().map_err(|_| std::io::ErrorKind::InvalidData)?;
                            }
                            if name.eq_ignore_ascii_case("transfer-encoding") { chunked = value.trim().eq_ignore_ascii_case("chunked"); }
                        }
                    }
                    // Drain the real request stream before replying, including
                    // the upload. The test never substitutes successful worker
                    // results for network/activation effects.
                    if chunked {
                        loop {
                            line.clear();
                            reader.read_line(&mut line)?;
                            let size = u64::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                                .map_err(|_| std::io::ErrorKind::InvalidData)?;
                            if size == 0 {
                                loop {
                                    line.clear();
                                    if reader.read_line(&mut line)? == 0 { return Err(std::io::ErrorKind::UnexpectedEof.into()); }
                                    if line == "\r\n" { break; }
                                }
                                break;
                            }
                            if std::io::copy(&mut reader.by_ref().take(size), &mut std::io::sink())? != size {
                                return Err(std::io::ErrorKind::UnexpectedEof.into());
                            }
                            let mut ending = [0; 2];
                            reader.read_exact(&mut ending)?;
                            if ending != *b"\r\n" { return Err(std::io::ErrorKind::InvalidData.into()); }
                        }
                    } else if std::io::copy(&mut reader.by_ref().take(length), &mut std::io::sink())? != length {
                        return Err(std::io::ErrorKind::UnexpectedEof.into());
                    }
                    // Single-segment frame with a four-byte content size and
                    // bounded raw blocks; these are response fixture bytes.
                    let size = u32::try_from(payload.len()).map_err(|_| std::io::ErrorKind::InvalidData)?;
                    let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0xa0];
                    frame.extend_from_slice(&size.to_le_bytes());
                    if payload.is_empty() { frame.extend_from_slice(&[1, 0, 0]); }
                    for (index, chunk) in payload.chunks(65_536).enumerate() {
                        let last = (index + 1) * 65_536 >= payload.len();
                        let header = ((chunk.len() as u32) << 3) | u32::from(last);
                        frame.extend_from_slice(&header.to_le_bytes()[..3]);
                        frame.extend_from_slice(chunk);
                    }
                    write!(socket, "HTTP/1.1 200 OK\r\nconnection: close\r\nanki-original-size: {}\r\ncontent-length: {}\r\n\r\n", payload.len(), frame.len())?;
                    socket.write_all(&frame)?;
                    socket.flush()?;
                }
                Ok(paths)
            });
            Self { target, stop, worker: Some(worker) }
        }
        fn finish(mut self) -> Vec<String> {
            self.worker.take().unwrap().join().expect("fixture responder joined").expect("fixture requests completed")
        }
    }
    impl Drop for BootstrapFixtureServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        }
    }
    #[test]
    fn ffi_actual_activation_retained_operation_accepts_fresh_current_owner_media_path() {
        use meguri_storage::{OpenOptions, ReadRows, StampMode, open_collection};
        const NOW: i64 = 1_894_701_600_000;
        let root = tempfile::tempdir().unwrap();
        let original = connection(&root.path().join("local"));
        let host = &original.owner.host;
        let handoff = host.begin_provider_handoff(meguri_sync::provider::Selection::Meguri).unwrap();
        host.finish_provider_handoff(&handoff).unwrap();
        host.collection().unwrap().run(meguri_engine::OpSpec::named("bootstrap fixture barrier", meguri_engine::OpSource::Rollover), |_| Ok(())).unwrap();
        let server = fixture_config(&root.path().join("server"));
        let path = server.collection_directory.join("collection.anki2");
        meguri_engine::bootstrap::create(&path, server.clock.as_ref(), server.random.as_ref()).unwrap();
        let db = open_collection(&path, &OpenOptions::new(server.clock.as_ref(), server.random.as_ref())).unwrap();
        let mut scope = db.writer().begin_write(StampMode::Raw, server.clock.as_ref(), None).unwrap();
        let mut col = scope.col().unwrap();
        col.modified = meguri_model::time::EpochMillis(NOW - 1000);
        col.schema_modified = meguri_model::time::EpochMillis(NOW - 5000);
        col.usn = meguri_model::usn::Usn(8);
        scope.put_col(&col).unwrap();
        scope.commit().unwrap();
        db.close().unwrap();
        let data = std::fs::read(path).unwrap();
        let meta = format!(r#"{{"mod":{},"scm":{},"usn":8,"ts":{},"msg":"","cont":true,"empty":true,"media_usn":0}}"#,
            NOW - 1000, NOW - 5000, NOW / 1000).into_bytes();
        let stub = BootstrapFixtureServer::new(vec![("/sync/meta", meta.clone()), ("/sync/download", data),
            ("/sync/meta", meta.clone()), ("/sync/meta", meta), ("/sync/upload", b"OK".to_vec()),
            ("/msync/begin", br#"{"data":{"usn":0,"sk":"synthetic"},"err":""}"#.to_vec())]);
        // Use the existing Host transport injection, then the real FFI result
        // conversion. Neither opaque preview nor operation authority is made
        // from a DTO or a caller-selected operation identity.
        let preview = fixture_outcome(&original, || original.job(Source::Preview(Arc::new(
            host.prepare_meguri_bootstrap(fixture_access(&original, &stub), MeguriBootstrapDirection::UseDeviceCollection).unwrap()))));
        let ProviderBootstrapOutcome::Preview { preview } = preview else { panic!("actual preview required") };
        consume(&preview.consumed).unwrap();
        let outcome = fixture_outcome(&original, || original.job(Source::Operation(Arc::new(
            host.confirm_meguri_bootstrap(fixture_access(&original, &stub), preview.inner.clone()).unwrap()))));
        let ProviderBootstrapOutcome::Operation { operation } = outcome else { panic!("actual activated operation required") };
        let generation = host.cloud_connection_generation().unwrap();
        let old_owner_stale = original.owner.validate().is_err();
        let owner = Arc::new(ProviderOwner { host: host.clone(), generation });
        let current = owner.configured_meguri().unwrap().unwrap();
        // Old operation handles still cannot authorize local decisions.
        let mut refused_owners = vec![current.discard_bootstrap_operation(operation.clone()).is_err(),
            current.start_bootstrap_recovery(operation.clone()).is_err(),
            current.discard_bootstrap_preview(preview.clone()).is_err(),
            current.start_bootstrap_confirm(credentials(&current), preview).is_err()];
        let other_root = tempfile::tempdir().unwrap();
        let foreign = connection(other_root.path());
        refused_owners.push(foreign.start_bootstrap_media_prepare(credentials(&foreign), operation.clone()).is_err());
        refused_owners.push(foreign.complete_bootstrap(credentials(&foreign), operation.clone()).is_err());
        for field in ["reference", "revision", "endpoint"] {
            let mut inner = current.inner.clone();
            match field { "reference" => inner.reference.push('x'), "revision" => inner.revision += 1,
                _ => inner.endpoint = "https://foreign.synthetic.example/".into() }
            let wrong = ProviderConnection { owner: current.owner.clone(), inner };
            refused_owners.push(wrong.start_bootstrap_media_prepare(credentials(&wrong), operation.clone()).is_err());
            refused_owners.push(wrong.complete_bootstrap(credentials(&wrong), operation.clone()).is_err());
        }
        let consent = fixture_outcome(&current, || current.start_bootstrap_media_prepare(credentials(&current), operation.clone()).unwrap());
        let ProviderBootstrapOutcome::MediaConsent { consent } = consent else { panic!("actual media consent required") };
        consume(&consent.consumed).unwrap();
        let done = fixture_outcome(&current, || current.job(Source::Operation(Arc::new(
            host.run_meguri_bootstrap_media_for_generation(generation, fixture_access(&current, &stub), consent.inner.confirmation.clone()).unwrap()))));
        let ProviderBootstrapOutcome::Operation { operation: done } = done else { panic!("actual media result required") };
        // Completion also accepts the genuine preactivation handle; Host reads
        // the current retained row and validates intent/publication/media proof.
        let completed = current.complete_bootstrap(credentials(&current), operation.clone()).unwrap();
        let paths = stub.finish(); // Join the finite responder before inspecting its results.
        host.close().unwrap();
        foreign.owner.host.close().unwrap();
        assert!(operation.inner.collection_complete);
        assert_eq!(operation.inner.acceptance, meguri_sync::policy::Acceptance::ExactOk);
        assert!(Arc::ptr_eq(&operation.owner, &original.owner));
        assert_ne!(generation, original.owner.generation);
        assert!(old_owner_stale);
        assert!(refused_owners.into_iter().all(|refused| refused));
        assert_eq!(done.inner.id, operation.inner.id);
        assert_eq!(done.inner.media.stage, meguri_sync::policy::MediaStage::Complete);
        assert!(matches!(completed, ProviderBootstrapOutcome::Ready { .. }));
        assert_eq!(paths, ["/sync/meta", "/sync/download", "/sync/meta", "/sync/meta", "/sync/upload", "/msync/begin"]);
    }
}
