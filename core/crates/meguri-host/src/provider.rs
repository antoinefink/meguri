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
    pub fn save_provider_settings(&self, settings: meguri_sync::provider::LegacySettings) -> Result<(), EngineError> {
        self.save_provider_settings_for_generation(self.cloud_connection_generation()?, settings)
    }
    /// The retained facade supplies its original generation for comparison
    /// inside the actual metadata publication, not as publication authority.
    pub fn save_provider_settings_for_generation(&self, generation: meguri_engine::Generation, settings: meguri_sync::provider::LegacySettings) -> Result<(), EngineError> {
        self.provider.close_admission();
        self.publish_provider_metadata(generation, |publication| {
            self.provider.save_settings_for_owner(settings, generation.0, publication)?;
            Ok(())
        })?;
        self.refresh_provider_admission();
        Ok(())
    }
    /// Handoff recovery facts never certify that an earlier process joined.
    fn provider_publication_facts(&self) -> Result<(meguri_engine::Generation, Publication), EngineError> {
        let generation = self.cloud_connection_generation()?;
        let collection = self.cloud_connection_collection()?;
        if collection.generation() != generation || !self.original_resource_authority(&collection).is_live()
            || !self.manual_transfer_history().active().is_ok_and(|active| active.is_none())
            || !self.engine().jobs().running().is_empty() {
            return Err(EngineError::Busy { running_op: "provider current owner recovery".into() });
        }
        let publication = DeviceState::for_collection(collection.path()).settled_publication()?
            .ok_or_else(|| EngineError::CollectionUnavailable { reason: "provider recovery publication missing".into() })?;
        Ok((generation, publication))
    }
    fn provider_recovery_facts(&self) -> Result<(meguri_engine::Generation, Publication), EngineError> {
        if self.review().active_session().is_some() {
            return Err(EngineError::Busy { running_op: "provider current owner recovery".into() });
        }
        self.provider_publication_facts()
    }
    pub fn recover_provider_handoff_to_local_only(&self) -> Result<(), EngineError> {
        self.cloud_connection_generation()?;
        self.provider.close_admission();
        let _exclusive = self.engine().reserve_collection_export_handoff()?;
        let facts = self.provider_recovery_facts()?;
        self.provider.validate_handoff_recovery(&facts.1)?;
        if let Some(runtime) = self.cloud_runtime() {
            // Refuse an SDK/identity/incoming owner, rather than pretending
            // that its disappearance or a serialized intent proves a join.
            runtime.provider_after_native_join().map_err(|_| EngineError::Busy { running_op: "provider current native owner".into() })?;
            let status = runtime.status().map_err(|_| EngineError::Busy { running_op: "provider current iCloud facts".into() })?;
            if status.stored_usable {
                self.stop_using_icloud().map_err(|_| EngineError::Busy { running_op: "provider recovery iCloud stop".into() })?;
            }
        }
        if self.provider_recovery_facts()? != facts { return Err(EngineError::Busy { running_op: "provider recovery facts changed".into() }); }
        self.provider.recover_handoff_to_local_only(facts.0.0, &facts.1)?;
        Ok(())
    }
    pub fn retry_provider_publication(&self) -> Result<(), EngineError> {
        self.retry_provider_publication_for_generation(self.cloud_connection_generation()?)
    }
    pub fn retry_provider_publication_for_generation(&self, generation: meguri_engine::Generation) -> Result<(), EngineError> {
        if self.cloud_connection_generation()? != generation {
            return Err(EngineError::Busy { running_op: "provider original host changed".into() });
        }
        self.provider.close_admission();
        if self.provider.failed_publication_requires_handoff_recovery()? {
            // Retain the collection-only recovery protocol, including review
            // and native exclusions. Metadata setup does not weaken it.
            let _exclusive = self.engine().reserve_collection_export_handoff()?;
            let facts = self.provider_recovery_facts()?;
            if facts.0 != generation { return Err(EngineError::Busy { running_op: "provider original host changed".into() }); }
            if let Some(runtime) = self.cloud_runtime() {
                runtime.provider_after_native_join().map_err(|_| EngineError::Busy { running_op: "provider publication native owner".into() })?;
            }
            if self.provider_recovery_facts()? != facts { return Err(EngineError::Busy { running_op: "provider publication facts changed".into() }); }
            self.provider.retry_failed_publication(facts.0.0, Some(&facts.1))?;
        } else {
            let facts = self.publish_provider_metadata(generation, |publication| {
                self.provider_metadata_retry_available()?;
                Ok(publication)
            })?;
            // This observation adds the existing native exclusion; it is no
            // join proof. Never call Runtime under Host/lifecycle write gates.
            if let Some(runtime) = self.cloud_runtime() {
                runtime.provider_after_native_join().map_err(|_| EngineError::Busy { running_op: "provider publication native owner".into() })?;
            }
            self.publish_provider_metadata(generation, |publication| {
                self.provider_metadata_retry_available()?;
                if publication != facts { return Err(EngineError::Busy { running_op: "provider publication facts changed".into() }); }
                self.provider.retry_failed_publication(generation.0, publication.as_ref())?;
                Ok(())
            })?;
        }
        self.refresh_provider_admission();
        Ok(())
    }
    fn provider_metadata_retry_available(&self) -> Result<(), EngineError> {
        if !self.manual_transfer_history().active().is_ok_and(|active| active.is_none())
            || !self.engine().jobs().running().is_empty() {
            return Err(EngineError::Busy { running_op: "provider current owner recovery".into() });
        }
        Ok(())
    }
    pub fn begin_provider_handoff(&self, destination: Selection) -> Result<ProviderHandoff, EngineError> {
        self.begin_provider_handoff_with_removal(destination, false)
    }
    pub fn begin_provider_handoff_with_removal(&self, destination: Selection, remove_credentials: bool) -> Result<ProviderHandoff, EngineError> {
        if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            return self.begin_no_collection_provider_handoff(destination, remove_credentials);
        }
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
        if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            return self.finish_no_collection_provider_handoff(handoff);
        }
        if self.cloud_connection_generation()? != handoff.generation {
            return Err(EngineError::Busy { running_op: "provider original host changed".into() });
        }
        self.provider.validate_handoff(&handoff.intent)?;
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
    /// Retain only an original healthy absent owner. The lane checks Host
    /// liveness, generation, recovery ownership and file absence without any
    /// blocking close-state query. Its optional settled publication is authority.
    fn reserve_no_collection_provider_handoff(&self, generation: meguri_engine::Generation)
        -> Result<(meguri_engine::SlotGuard, Option<Publication>), EngineError> {
        let slot = self.engine().acquire_slot(meguri_engine::LongOp::OneWaySync)?;
        if !matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection)
            || self.review().active_session().is_some()
            || !self.manual_transfer_history().active().is_ok_and(|active| active.is_none()) {
            return Err(EngineError::Busy { running_op: "provider recovery or review".into() });
        }
        let publication = self.publish_provider_metadata_for_slot(&slot, generation, Ok)?;
        Ok((slot, publication))
    }
    fn begin_no_collection_provider_handoff(&self, destination: Selection, remove_credentials: bool) -> Result<ProviderHandoff, EngineError> {
        let generation = self.engine().generation();
        let (slot, original_publication) = self.reserve_no_collection_provider_handoff(generation)?;
        if let Some(runtime) = self.cloud_runtime() {
            runtime.provider_transition_available().map_err(|_| EngineError::Busy { running_op: "iCloud recovery".into() })?;
            let status = runtime.status().map_err(|_| EngineError::Busy { running_op: "iCloud recovery".into() })?;
            if status.stored_usable {
                return Err(EngineError::Busy { running_op: "iCloud original owner".into() });
            }
        }
        let provider = &self.provider;
        self.publish_provider_metadata_for_slot(&slot, generation, |publication| {
            if publication != original_publication {
                return Err(EngineError::Busy { running_op: "provider authority changed".into() });
            }
            let intent = provider.begin_handoff_with_removal(destination, publication.clone(), generation.0, remove_credentials)?;
            Ok(ProviderHandoff { intent, generation, publication })
        })
    }
    fn finish_no_collection_provider_handoff(&self, handoff: &ProviderHandoff) -> Result<(), EngineError> {
        let generation = handoff.generation;
        let (slot, publication) = self.reserve_no_collection_provider_handoff(generation)?;
        self.provider.validate_handoff(&handoff.intent)?;
        if publication != handoff.publication {
            return Err(EngineError::Busy { running_op: "provider authority changed".into() });
        }
        if let Some(runtime) = self.cloud_runtime() {
            // An absent owner cannot stop a retained collection through a
            // fabricated handle. Fail closed on unavailable Runtime facts too.
            let status = runtime.status().map_err(|_| EngineError::Busy { running_op: "iCloud original owner".into() })?;
            if status.stored_usable {
                return Err(EngineError::Busy { running_op: "iCloud original owner".into() });
            }
            runtime.provider_after_native_join().map_err(|_| EngineError::Busy { running_op: "iCloud original owner".into() })?;
        }
        let provider = &self.provider;
        self.publish_provider_metadata_for_slot(&slot, generation, |publication| {
            if publication != handoff.publication {
                return Err(EngineError::Busy { running_op: "provider authority changed".into() });
            }
            provider.commit_handoff(&handoff.intent)?;
            Ok(())
        })
    }
    pub fn configure_meguri_connection(&self, endpoint: &str) -> Result<meguri_sync::provider::MeguriConnection, EngineError> {
        self.configure_meguri_connection_for_generation(self.cloud_connection_generation()?, endpoint)
    }
    pub fn configure_meguri_connection_for_generation(&self, generation: meguri_engine::Generation, endpoint: &str) -> Result<meguri_sync::provider::MeguriConnection, EngineError> {
        // RandomSource is a Host input, so sample before any ownership locks.
        let random = self.provider_random();
        let reference = format!("{:016x}{:016x}", random.next_u64(), random.next_u64());
        let connection = self.publish_provider_metadata(generation, |publication| {
            Ok(self.provider.configure_meguri_for_owner(endpoint, reference, generation.0, publication)?)
        })?;
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
