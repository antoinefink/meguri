//! Installation authority is composed before platform account providers.
use std::sync::Arc;
use meguri_sync::{provider::Authority, cloud::{BuildScope, runtime::{IdentityProvider, IdentityStatus}}};
use crate::CoreHost;
use meguri_engine::{EngineError, device_state::{DeviceState, Publication}};
use meguri_sync::provider::{Handoff, Selection};

pub struct ProviderHandoff { intent: Handoff, generation: meguri_engine::Generation, publication: Option<Publication> }

pub(crate) struct AdmittedIdentity {
    pub original: Arc<dyn IdentityProvider>, pub authority: Arc<Authority>,
}
impl IdentityProvider for AdmittedIdentity {
    fn read_identity(&self, scope: &BuildScope) -> IdentityStatus {
        if !self.authority.permits_icloud_account_read() && !self.authority.permits_original_identity_completion() { return IdentityStatus::TemporarilyUnavailable; }
        self.original.read_identity(scope)
    }
}
impl CoreHost {
    pub fn migrate_provider_configuration(root: &std::path::Path, legacy: meguri_sync::provider::LegacyInput) -> Result<(), meguri_storage::StorageError> {
        Authority::migrate_input(root, legacy)
    }
    pub fn provider_authority(&self) -> &Arc<Authority> { &self.provider }
    /// Local failures leave the collection available. No manual operation or R6
    /// decision is finished or rewritten by this refresh.
    pub fn refresh_provider_admission(&self) {
        if !self.manual_transfer_history().active().is_ok_and(|active| active.is_none()) {
            self.provider.close_admission();
            return;
        }
        if self.provider.reload().is_err() { return; }
        if let Some(runtime) = self.cloud_runtime() {
            if runtime.qualify_provider_binding().is_err() { self.provider.close_admission(); }
        }
    }
    pub fn begin_provider_handoff(&self, destination: Selection) -> Result<ProviderHandoff, EngineError> {
        self.begin_provider_handoff_with_removal(destination, false)
    }
    pub fn begin_provider_handoff_with_removal(&self, destination: Selection, remove_credentials: bool) -> Result<ProviderHandoff, EngineError> {
        self.cloud_connection_generation()?;
        if self.review().active_session().is_some()
            || !self.manual_transfer_history().active().is_ok_and(|active| active.is_none()) {
            return Err(EngineError::Busy { running_op: "provider recovery or review".into() });
        }
        let _exclusive = self.engine().reserve_collection_export_handoff()?;
        if let Some(runtime) = self.cloud_runtime() {
            runtime.provider_transition_available().map_err(|_| EngineError::Busy { running_op: "iCloud recovery".into() })?;
        }
        let generation = self.engine().generation();
        let publication = DeviceState::for_collection(&self.cloud_connection_directory().join("collection.anki2")).settled_publication()?;
        let intent = self.provider.begin_handoff_with_removal(destination, publication.clone(), generation.0, remove_credentials)?;
        Ok(ProviderHandoff { intent, generation, publication })
    }
    /// The production app invokes this only after SyncCoordinator.cancelAndDrain
    /// returned from the original backend's actual native joins. The Core checks
    /// here add exclusions; none substitutes for that join.
    pub fn finish_provider_handoff(&self, handoff: &ProviderHandoff) -> Result<(), EngineError> {
        let _exclusive = self.engine().reserve_collection_export_handoff()?;
        if self.review().active_session().is_some() || self.cloud_connection_generation()? != handoff.generation
            || !self.manual_transfer_history().active().is_ok_and(|active| active.is_none())
            || DeviceState::for_collection(&self.cloud_connection_directory().join("collection.anki2")).settled_publication()? != handoff.publication {
            return Err(EngineError::Busy { running_op: "provider authority changed".into() });
        }
        if let Some(runtime) = self.cloud_runtime() {
            runtime.provider_after_native_join().map_err(|_| EngineError::Busy { running_op: "iCloud original owner".into() })?;
            if runtime.status().is_ok_and(|s| s.stored_usable) {
                self.stop_using_icloud().map_err(|_| EngineError::Busy { running_op: "iCloud stop".into() })?;
            }
        }
        self.provider.commit_handoff(&handoff.intent)?;
        Ok(())
    }
    pub fn configure_meguri_connection(&self, endpoint: &str) -> Result<meguri_sync::provider::MeguriConnection, EngineError> {
        let random = self.provider_random();
        let reference = format!("{:016x}{:016x}", random.next_u64(), random.next_u64());
        let connection = self.provider.configure_meguri(endpoint, reference)?;
        self.refresh_provider_admission();
        Ok(connection)
    }
    pub fn authenticate_meguri_connection(&self, connection: &meguri_sync::provider::MeguriConnection,
        username: &str, password: &str, cancel: Arc<meguri_engine::CancelToken>,
    ) -> Result<meguri_sync::provider::AuthenticatedConnection, meguri_sync::provider::AuthenticationFailure> {
        meguri_sync::provider::authenticate(&self.provider, connection, self.engine().threads(),
            self.provider_random(), username, password, cancel, None)
    }
}
