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
    fn access(&self, credentials: ProviderCredentials) -> Result<MeguriAccess, MeguriBootstrapError> {
        let expected = information(&self.inner);
        if credentials.connection.reference != expected.reference || credentials.connection.revision != expected.revision
            || credentials.connection.endpoint != expected.endpoint { return Err(MeguriBootstrapError::ConnectionChanged); }
        MeguriAccess::new(self.inner.clone(), &credentials.username, HostKey::new(credentials.hostkey))
    }
    fn same_host(&self, owner: &ProviderOwner, connection: &MeguriConnection) -> Result<(), CoreError> {
        if !Arc::ptr_eq(&self.owner.host, &owner.host) || &self.inner != connection { return Err(invalid()); }
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
        self.same_host(&preview.owner, &preview.connection)?;
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
        self.same_host(&consent.owner, &consent.connection)?;
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
    fn connection(root: &std::path::Path) -> Arc<ProviderConnection> {
        use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
        let collection = root.join("collection");
        std::fs::create_dir_all(&collection).unwrap();
        let config = meguri_host::HostConfig::new(collection, root.join("backups"), root.join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)), Arc::new(SeededRandom::new(171)),
            Arc::new(meguri_storage::space::StatvfsSpace));
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
}
