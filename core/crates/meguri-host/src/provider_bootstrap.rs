//! Narrow Meguri bootstrap caller; retained transfers own all network effects.
use crate::{CoreHost, ManualTransferTask};
use meguri_engine::{EngineError, Generation, LongOp, device_state::DeviceState, media::MediaJobKind};
use meguri_sync::{HostKey, JobOptions, ServerKind, SyncTransport,
    oneway::{MediaConfirmation, MediaPreview, Preview, TransferAccess, TransferError},
    policy::{Acceptance, AccountRef, Direction, MediaOutcome, Operation, OperationId, Phase, Scope},
    provider::{EpochInputs, MeguriConnection, MeguriReadiness, MeguriReadyRefusal, ReadyInputs,
        Selection, canonical_username, native_endpoint}, provider_media::MediaAccounting};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriBootstrapDirection { UseServerCollection, UseDeviceCollection }
pub enum MeguriBootstrapError {
    NotSelected, ConnectionChanged, Busy, AlreadyBound, Username, Ready(MeguriReadyRefusal), Engine(EngineError),
    Transfer(TransferError),
    /// MP6.3: the facts a setup decision was made on no longer hold.
    SetupChanged(crate::provider_setup::MeguriSetupChange),
    /// A collection file is on this device but is not open: closed or damaged.
    /// Setup can neither read it nor treat the device as holding none.
    LocalUnusable,
}
impl fmt::Debug for MeguriBootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready(error) => f.debug_tuple("Ready").field(error).finish(),
            _ => f.write_str(match self { Self::NotSelected => "NotSelected", Self::ConnectionChanged => "ConnectionChanged",
                Self::Busy => "Busy", Self::AlreadyBound => "AlreadyBound", Self::Username => "Username",
                Self::Transfer(_) => "Transfer", Self::SetupChanged(_) => "SetupChanged",
                Self::LocalUnusable => "LocalUnusable", _ => "Engine" }),
        }
    }
}
impl From<EngineError> for MeguriBootstrapError { fn from(error: EngineError) -> Self { Self::Engine(error) } }
impl From<MeguriReadyRefusal> for MeguriBootstrapError { fn from(error: MeguriReadyRefusal) -> Self { Self::Ready(error) } }
impl From<TransferError> for MeguriBootstrapError { fn from(error: TransferError) -> Self { Self::Transfer(error) } }
type Result<T> = std::result::Result<T, MeguriBootstrapError>;

pub struct MeguriAccess {
    connection: MeguriConnection, username: String, key: HostKey, options: JobOptions,
    #[cfg(feature = "test-hooks")]
    transport: Option<Box<dyn SyncTransport>>,
}
impl MeguriAccess {
    pub fn new(connection: MeguriConnection, username: &str, key: HostKey) -> Result<Self> {
        let endpoint = native_endpoint(&connection.endpoint).map_err(|_| MeguriBootstrapError::ConnectionChanged)?;
        let username = canonical_username(username).map_err(|_| MeguriBootstrapError::Username)?;
        Ok(Self { connection, username, key, options: JobOptions::new(endpoint, ServerKind::SelfHosted),
            #[cfg(feature = "test-hooks")] transport: None })
    }
    #[cfg(feature = "test-hooks")]
    pub fn with_transport_for_testing(mut self, transport: Box<dyn SyncTransport>) -> Self {
        self.transport = Some(transport); self
    }
    pub(crate) fn connection(&self) -> &MeguriConnection { &self.connection }
    pub(crate) fn username(&self) -> &str { &self.username }
    pub(crate) fn into_round_inputs(self) -> (HostKey, JobOptions, Option<Box<dyn SyncTransport>>) {
        #[cfg(feature = "test-hooks")]
        let transport = self.transport;
        #[cfg(not(feature = "test-hooks"))]
        let transport = None;
        (self.key, self.options, transport)
    }
    pub(crate) fn into_transfer_access(self) -> Result<TransferAccess> {
        let configured = native_endpoint(&self.connection.endpoint).map_err(|_| MeguriBootstrapError::ConnectionChanged)?;
        let access = TransferAccess::new(configured, &self.username, self.key, self.options)
            .map_err(|_| MeguriBootstrapError::ConnectionChanged)?;
        #[cfg(feature = "test-hooks")]
        let access = match self.transport { Some(transport) => access.with_transport_for_testing(transport), None => access };
        Ok(access)
    }
}
impl CoreHost {
    /// Local inspection is allowed while retained preparation closes network
    /// admission. Snapshot and policy reads finish before cancellation/refresh;
    /// no provider sidecar callback contains a second sidecar read.
    pub(crate) fn active_meguri_preparation(&self, connection: &MeguriConnection) -> Result<Option<Operation>> {
        let doc = self.provider_authority().snapshot().map_err(MeguriReadyRefusal::Storage)?;
        if doc.selection != Selection::Meguri { return Err(MeguriBootstrapError::NotSelected); }
        if doc.handoff.is_some() || doc.meguri.as_ref() != Some(connection) {
            return Err(MeguriBootstrapError::ConnectionChanged);
        }
        if doc.meguri_bootstrap.as_ref().is_some_and(|intent| intent.connection() != connection) {
            return Err(MeguriReadyRefusal::OperationMismatch.into());
        }
        let active = self.manual_transfer_history().active().map_err(TransferError::from)?;
        if let Some(op) = &active {
            if op.configured_endpoint != connection.endpoint || op.scope != Scope::CollectionAndMedia {
                return Err(MeguriReadyRefusal::OperationMismatch.into());
            }
            if let Some(intent) = &doc.meguri_bootstrap {
                if intent.operation() != &op.id {
                    // An inactive cancelled or failed confirmation can leave
                    // its intent without effects. Retain that evidence; only
                    // the actual old policy row can prove this mismatch safe.
                    // Preparation captures actual_generation before activation.
                    let old = self.manual_transfer_history().get(intent.operation()).map_err(TransferError::from)?;
                    // An overtaken upload (MP6.9 neither) is settled and inactive too.
                    if !matches!(old.phase, Phase::Cancelled | Phase::Failed)
                        || !matches!(old.acceptance, Acceptance::NotSent | Acceptance::Refused | Acceptance::Overtaken)
                        || old.id == op.id || old.account != op.account || old.scope != op.scope
                        || old.configured_endpoint != connection.endpoint
                        || old.activation_identity.is_some()
                        || old.resulting_publication.is_some() || old.collection_complete
                        || old.receipt.is_some() || old.media != MediaOutcome::default()
                        || intent.media_epoch().is_some()
                    {
                        return Err(MeguriReadyRefusal::OperationMismatch.into());
                    }
                }
            }
        }
        Ok(active)
    }
    /// Cold lookup derives the account locally; sign-in is never dispatched.
    pub fn pending_meguri_bootstrap_preparation(&self, connection: &MeguriConnection, username: &str) -> Result<Option<Operation>> {
        self.pending_meguri_bootstrap_preparation_for_generation(self.cloud_connection_generation()?, connection, username)
    }
    pub fn pending_meguri_bootstrap_preparation_for_generation(&self, generation: Generation,
        connection: &MeguriConnection, username: &str) -> Result<Option<Operation>> {
        let endpoint = native_endpoint(&connection.endpoint).map_err(|_| MeguriBootstrapError::ConnectionChanged)?;
        let username = canonical_username(username).map_err(|_| MeguriBootstrapError::Username)?;
        let account = AccountRef::derive(&endpoint, &username).ok_or(MeguriReadyRefusal::AccountChanged)?;
        self.publish_provider_metadata(generation, |_| Ok((|| {
            let active = self.active_meguri_preparation(connection)?;
            if active.as_ref().is_some_and(|op| op.account != account) { return Err(MeguriReadyRefusal::AccountChanged.into()); }
            Ok(active)
        })()))?
    }
    fn check_pending_meguri_preparation(&self, connection: &MeguriConnection, operation: &Operation) -> Result<()> {
        if self.active_meguri_preparation(connection)?.as_ref() != Some(operation) {
            return Err(MeguriReadyRefusal::OperationMismatch.into());
        }
        Ok(())
    }
    /// Only the actual active decision may be discarded. The retained Flight
    /// and policy guards decide whether cancellation is still safe.
    pub fn discard_meguri_bootstrap_preparation(&self, connection: &MeguriConnection, operation: &Operation) -> Result<Operation> {
        self.discard_meguri_bootstrap_preparation_for_generation(self.cloud_connection_generation()?, connection, operation)
    }
    pub fn discard_meguri_bootstrap_preparation_for_generation(&self, generation: Generation,
        connection: &MeguriConnection, operation: &Operation) -> Result<Operation> {
        // MP7 step 1: the structure save job's own preparation is its own to end.
        if self.meguri_structure_job_active() { return Err(MeguriBootstrapError::Busy); }
        self.discard_admitted(generation, connection, operation)
    }
    pub(crate) fn discard_meguri_bootstrap_preparation_admitted(&self, connection: &MeguriConnection, operation: &Operation) -> Result<Operation> {
        self.discard_admitted(self.cloud_connection_generation()?, connection, operation)
    }
    fn discard_admitted(&self, generation: Generation,
        connection: &MeguriConnection, operation: &Operation) -> Result<Operation> {
        let result = self.publish_provider_metadata(generation, |_| Ok((|| {
            self.check_pending_meguri_preparation(connection, operation)?;
            self.cancel_prepared_manual_transfer(&operation.id, false).map_err(Into::into)
        })()))?;
        // Original-owner publication gates, Flight and policy locks are released.
        self.refresh_provider_admission();
        // It sent nothing and replaced nothing: what it alone owned goes with it.
        if let Ok(cancelled) = &result { self.sweep_discarded_preparation(cancelled); }
        result
    }
    /// Explicit offline exposure of the existing recovery decision. In
    /// particular, unknown acceptance stays NeedsDecision and globally fenced.
    pub fn prepare_meguri_bootstrap_recovery(&self, connection: &MeguriConnection, operation: &Operation) -> Result<ManualTransferTask<Preview>> {
        self.prepare_meguri_bootstrap_recovery_for_generation(self.cloud_connection_generation()?, connection, operation)
    }
    pub fn prepare_meguri_bootstrap_recovery_for_generation(&self, generation: Generation,
        connection: &MeguriConnection, operation: &Operation) -> Result<ManualTransferTask<Preview>> {
        self.publish_provider_metadata(generation, |_| Ok(self.check_pending_meguri_preparation(connection, operation)))??;
        Ok(self.prepare_manual_transfer_recovery(operation.id.clone())?)
    }
    pub(crate) fn check_meguri_bootstrap(&self, connection: &MeguriConnection) -> Result<()> {
        let doc = self.provider_authority().snapshot().map_err(MeguriReadyRefusal::Storage)?;
        if doc.selection != Selection::Meguri { return Err(MeguriBootstrapError::NotSelected); }
        if doc.handoff.is_some() || doc.meguri.as_ref() != Some(connection) { return Err(MeguriBootstrapError::ConnectionChanged); }
        if self.review().active_session().is_some() { return Err(MeguriBootstrapError::Busy); }
        match self.meguri_readiness()? {
            MeguriReadiness::Ready => Err(MeguriBootstrapError::AlreadyBound),
            // Retained stale authority must first be explicitly invalidated.
            MeguriReadiness::Stale => Err(MeguriBootstrapError::ConnectionChanged),
            _ => Ok(()),
        }
    }
    pub(crate) fn check_meguri_bootstrap_operation(&self, connection: &MeguriConnection, id: &OperationId) -> Result<()> {
        self.check_meguri_bootstrap(connection)?;
        let intent = self.provider_authority().meguri_bootstrap_intent().ok_or(MeguriReadyRefusal::NoIntent)?;
        if intent.operation() != id || intent.connection() != connection { return Err(MeguriReadyRefusal::OperationMismatch.into()); }
        Ok(())
    }
    pub fn prepare_meguri_bootstrap(&self, access: MeguriAccess, direction: MeguriBootstrapDirection) -> Result<ManualTransferTask<Preview>> {
        // MP7 step 1: only the structure save job prepares while it holds the lane.
        if self.meguri_structure_job_active() { return Err(MeguriBootstrapError::Busy); }
        self.prepare_meguri_bootstrap_admitted(access, direction)
    }
    pub(crate) fn prepare_meguri_bootstrap_admitted(&self, access: MeguriAccess, direction: MeguriBootstrapDirection) -> Result<ManualTransferTask<Preview>> {
        self.check_meguri_bootstrap(access.connection())?;
        if self.provider_authority().network_blocked()
            || self.manual_transfer_history().active().map_err(|_| MeguriBootstrapError::Busy)?.is_some() {
            return Err(MeguriBootstrapError::Busy);
        }
        // Only an admitted preparation disarms: a refused call must not clear
        // the retest of a confirmation that is still running.
        self.setup_retest.disarm();
        let direction = match direction { MeguriBootstrapDirection::UseServerCollection => Direction::Import,
            MeguriBootstrapDirection::UseDeviceCollection => Direction::Export };
        Ok(self.prepare_manual_transfer(access.into_transfer_access()?, direction, Scope::CollectionAndMedia)?)
    }
    pub fn confirm_meguri_bootstrap(&self, access: MeguriAccess, preview: Preview) -> Result<ManualTransferTask<Operation>> {
        if self.meguri_structure_job_active() { return Err(MeguriBootstrapError::Busy); }
        self.confirm_meguri_bootstrap_retesting(access, preview, None)
    }
    /// `retest` arms MP6.3 row 2's test inside the replacement's exclusive
    /// phase for this one confirmation; every other confirmation disarms it.
    pub(crate) fn confirm_meguri_bootstrap_retesting(&self, access: MeguriAccess, preview: Preview,
        retest: Option<crate::provider_setup::Retest>) -> Result<ManualTransferTask<Operation>> {
        let generation = self.cloud_connection_generation()?;
        self.check_meguri_bootstrap(access.connection())?;
        let active = self.manual_transfer_history().active().map_err(|_| MeguriBootstrapError::Busy)?;
        if active.as_ref() != Some(&preview.operation) || preview.operation.phase != Phase::Preview {
            return Err(MeguriBootstrapError::Busy);
        }
        // Only the confirmation of the active preview disarms; a refused call
        // leaves another confirmation's retest armed.
        self.setup_retest.disarm();
        self.publish_provider_metadata(generation, |publication| Ok(self.provider_authority().record_meguri_bootstrap_intent(
            access.connection(), access.username(), &preview, generation.0, publication)))??;
        let access = access.into_transfer_access()?;
        if let Some(retest) = retest { self.setup_retest.arm(retest); }
        let task = self.confirm_manual_transfer(access, preview.confirmation);
        if task.is_err() { self.setup_retest.disarm(); }
        Ok(task?)
    }
    pub fn prepare_meguri_bootstrap_media(&self, access: MeguriAccess, id: OperationId) -> Result<ManualTransferTask<MediaPreview>> {
        self.check_meguri_bootstrap_operation(access.connection(), &id)?;
        Ok(self.prepare_manual_media(id, access.into_transfer_access()?)?)
    }
    pub fn run_meguri_bootstrap_media(&self, access: MeguriAccess, consent: MediaConfirmation) -> Result<ManualTransferTask<Operation>> {
        self.run_meguri_bootstrap_media_for_generation(self.cloud_connection_generation()?, access, consent)
    }
    pub fn run_meguri_bootstrap_media_for_generation(&self, generation: Generation, access: MeguriAccess,
        consent: MediaConfirmation) -> Result<ManualTransferTask<Operation>> {
        self.check_meguri_bootstrap_operation(access.connection(), consent.operation_id())?;
        let media = self.media()?;
        let slot = self.engine().acquire_slot(LongOp::OneWaySync)?;
        let lease = media.try_job(MediaJobKind::Sync)?;
        let result = (|| -> Result<()> {
            if self.collection()?.generation() != generation { return Err(MeguriBootstrapError::Busy); }
            let evidence = self.provider_authority().prepare_meguri_media_epoch(EpochInputs { connection: access.connection(),
                operation: consent.operation_id(), policy: self.manual_transfer_history(), media: media.store() })?;
            self.publish_provider_metadata_for_slot(&slot, generation, |publication|
                Ok(self.provider_authority().record_meguri_media_epoch(evidence, generation.0, publication)))??;
            Ok(())
        })();
        drop(lease); drop(slot);
        self.refresh_provider_admission();
        result?;
        Ok(self.run_manual_media(access.into_transfer_access()?, consent)?)
    }
    pub fn complete_meguri_bootstrap(&self, connection: &MeguriConnection, username: &str, id: &OperationId) -> Result<MediaAccounting> {
        self.complete_meguri_bootstrap_for_generation(self.cloud_connection_generation()?, connection, username, id)
    }
    pub fn complete_meguri_bootstrap_for_generation(&self, generation: Generation, connection: &MeguriConnection,
        username: &str, id: &OperationId) -> Result<MediaAccounting> {
        self.check_meguri_bootstrap_operation(connection, id)?;
        let media = self.media()?;
        let slot = self.engine().acquire_slot(LongOp::OneWaySync)?;
        let lease = media.try_job(MediaJobKind::Sync)?;
        let result = (|| -> Result<MediaAccounting> {
            if self.collection()?.generation() != generation { return Err(MeguriBootstrapError::Busy); }
            let evidence = self.provider_authority().prepare_meguri_ready(ReadyInputs { connection, username, operation: id,
                policy: self.manual_transfer_history(), media: media.store(), keep_going: &mut || true })?;
            Ok(self.publish_provider_metadata_for_slot(&slot, generation, |publication|
                Ok(self.provider_authority().commit_meguri_ready(evidence, self.manual_transfer_history(), generation.0, publication)))??)
        })();
        drop(lease); drop(slot);
        self.refresh_provider_admission();
        result
    }
    pub fn meguri_readiness(&self) -> Result<MeguriReadiness> {
        let path = if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            // MP4 permits an initial download. NoCollection never turns a file
            // that appeared since preparation into authority to overwrite it.
            let path = self.cloud_connection_directory().join("collection.anki2");
            match path.symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(MeguriBootstrapError::ConnectionChanged),
            }
            path
        } else { self.collection()?.path().to_owned() };
        let settled = DeviceState::for_collection(&path).settled_publication()
            .map_err(MeguriReadyRefusal::Storage)?;
        Ok(self.provider_authority().meguri_readiness(settled.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::rng::SeededRandom;
    use meguri_sync::Job;
    fn connection() -> MeguriConnection {
        MeguriConnection { reference: "synthetic".into(), revision: 1, endpoint: "https://bootstrap.synthetic.example/".into() }
    }
    #[test]
    fn new_canonicalizes_username_and_retains_round_inputs_without_a_transport() {
        let connection = connection();
        let key = HostKey::new("synthetic-key");
        let access = MeguriAccess::new(connection.clone(), "  Synthetic-User\t", key.clone()).unwrap();
        assert_eq!(access.connection(), &connection);
        assert_eq!(access.username(), "synthetic-user");
        let (actual, options, transport) = access.into_round_inputs();
        assert_eq!(actual.as_str(), key.as_str());
        let job = Job::new(&options, Some(actual), &SeededRandom::new(1)).unwrap();
        assert_eq!(job.base().as_str(), connection.endpoint);
        assert!(transport.is_none());
    }
    #[test]
    fn new_refuses_non_native_endpoint_or_invalid_username() {
        let mut connection = connection();
        assert!(matches!(MeguriAccess::new(connection.clone(), "bad name", HostKey::new("key")), Err(MeguriBootstrapError::Username)));
        connection.endpoint = "http://bootstrap.synthetic.example/".into();
        assert!(matches!(MeguriAccess::new(connection, "synthetic-user", HostKey::new("key")), Err(MeguriBootstrapError::ConnectionChanged)));
    }
}
