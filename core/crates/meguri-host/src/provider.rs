//! Installation authority is composed before platform account providers.
use std::sync::Arc;
use meguri_sync::{provider::Authority, cloud::{BuildScope, runtime::{IdentityProvider, IdentityStatus}}};
use crate::CoreHost;
use meguri_engine::{EngineError, device_state::{DeviceState, Publication}};
use meguri_sync::provider::{Handoff, Selection};

#[cfg(feature = "test-hooks")]
pub(crate) struct FixtureTransportCell {
    owner: FixtureTransportOwner,
    installation_directory: std::path::PathBuf,
    policy: std::sync::Mutex<Option<Arc<FixtureTransportPolicy>>>,
}
#[cfg(feature = "test-hooks")]
#[derive(Clone)]
struct FixtureTransportOwner {
    collection: std::path::PathBuf,
    engine: meguri_engine::WeakEngine,
    liveness: meguri_engine::jobs::registry::OriginalHostLiveness,
}
#[cfg(feature = "test-hooks")]
struct FixtureTransportPolicy {
    origin: meguri_sync::Endpoint,
    ca_der: Vec<u8>,
}
#[cfg(feature = "test-hooks")]
impl FixtureTransportCell {
    pub(crate) fn new(collection: std::path::PathBuf, installation_directory: std::path::PathBuf,
        engine: meguri_engine::WeakEngine, liveness: meguri_engine::jobs::registry::OriginalHostLiveness,
    ) -> Self {
        Self { owner: FixtureTransportOwner { collection, engine, liveness }, installation_directory,
            policy: std::sync::Mutex::new(None) }
    }
}
#[cfg(feature = "test-hooks")]
impl FixtureTransportOwner {
    fn live(&self) -> bool {
        // Resolve the CURRENT incarnation only within the original engine and
        // original Host liveness. Bootstrap activation legitimately advances it.
        // Bootstrap requests may hold exclusive executor admission; this gate
        // checks ownership, not ordinary collection-operation admission.
        self.engine.upgrade().is_some_and(|engine| engine.collection().is_ok_and(|collection|
            collection.path() == self.collection
                && engine.original_resource_authority(&collection, self.liveness.clone()).is_live()))
    }
}
#[cfg(feature = "test-hooks")]
impl FixtureTransportPolicy {
    fn options(&self) -> meguri_sync::JobOptions {
        meguri_sync::JobOptions::new(self.origin.clone(), meguri_sync::ServerKind::SelfHosted)
            .trust_root(self.ca_der.clone())
    }
}
#[cfg(feature = "test-hooks")]
enum FixtureTransportFailure {
    Owner(EngineError),
    Transport(meguri_sync::SyncError),
}
#[cfg(feature = "test-hooks")]
impl From<EngineError> for FixtureTransportFailure {
    fn from(error: EngineError) -> Self { Self::Owner(error) }
}
// Admission wrapper only: HTTP/TLS, cancellation, timeouts, proxies and response
// handling remain the real transport's. No outcome or request state is shared.
#[cfg(feature = "test-hooks")]
struct FixtureHttpTransport {
    owner: FixtureTransportOwner,
    policy: Arc<FixtureTransportPolicy>,
    http: meguri_sync::transport::http::HttpTransport,
}
#[cfg(feature = "test-hooks")]
impl meguri_sync::SyncTransport for FixtureHttpTransport {
    fn post(&self, request: meguri_sync::transport::TransportRequest)
        -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
        Box::pin(async move {
            // The normalized origin ends in '/', so another host/port cannot
            // pass by sharing a textual prefix. Redirects pass this gate too.
            if !request.url.starts_with(self.policy.origin.as_str()) || !self.owner.live() {
                return Err(meguri_sync::SyncError::EndpointRefused("Meguri fixture original owner or origin changed".into()));
            }
            meguri_sync::SyncTransport::post(&self.http, request).await
        })
    }
}

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
    /// One-shot, original-Host admission of public synthetic CA bytes (T12).
    /// `expected_collection` is the exact opened collection FILE; the installation
    /// directory is HostConfig's manual_transfer_directory, not a cleanup root.
    #[cfg(feature = "test-hooks")]
    pub fn install_meguri_fixture_transport(&self, expected_generation: meguri_engine::Generation,
        expected_collection: &std::path::Path, expected_installation_directory: &std::path::Path,
        origin: &str, ca_der: &[u8],
    ) -> Result<(), EngineError> {
        let invalid = || EngineError::InvalidArgument { message: "invalid Meguri fixture transport policy".into() };
        if ca_der.is_empty() || ca_der.len() > 64 * 1024 { return Err(invalid()); }
        // Check the caller's text too: URL normalization must not bypass the
        // existing local endpoint guard. Admit an origin, with no base path.
        meguri_sync::endpoint_guard::check_endpoint(origin).map_err(|_| invalid())?;
        let endpoint = meguri_sync::provider::native_endpoint(origin).map_err(|_| invalid())?;
        if endpoint.as_str().strip_prefix("https://").and_then(|rest| rest.split_once('/'))
            .is_none_or(|(_, path)| !path.is_empty()) { return Err(invalid()); }
        let candidate = Arc::new(FixtureTransportPolicy { origin: endpoint, ca_der: ca_der.to_vec() });
        // Use the actual T12 parser/client constructor, never a parallel TLS
        // implementation. This performs no request, and precedes publication.
        meguri_sync::transport::http::HttpTransport::new(&candidate.options()).map_err(|_| invalid())?;
        let collection = self.cloud_connection_collection()?;
        let original = self.original_resource_authority(&collection);
        let cell = &self.fixture_transport;
        self.publish_provider_metadata(expected_generation, |publication| {
            if expected_collection != cell.owner.collection || collection.path() != expected_collection
                || expected_installation_directory != cell.installation_directory
                || collection.generation() != expected_generation || !original.is_live() {
                return Err(invalid());
            }
            // Immutable evidence only: no lifecycle reentry or writer operation
            // under the engine state lane. Also refuses a retired executor.
            collection.open_evidence()?;
            if !publication.as_ref().is_some_and(|p|
                p.binding == meguri_engine::device_state::AccountBinding::LocalOnly
                    && p.decision == meguri_engine::device_state::PublicationDecision::Clear) {
                return Err(EngineError::CollectionUnavailable { reason: "Meguri fixture requires a ready local collection".into() });
            }
            // snapshot releases its document lock before any other provider read.
            let doc = self.provider.snapshot()?;
            if doc.selection != Selection::LocalOnly || doc.settings.enabled || doc.meguri.is_some()
                || doc.binding.is_some() || doc.legacy_binding_pending || doc.handoff.is_some()
                || doc.meguri_binding.is_some() || doc.meguri_bootstrap.is_some() {
                return Err(EngineError::Busy { running_op: "Meguri fixture requires an unconfigured local provider".into() });
            }
            self.require_meguri_round_join()?;
            if !self.manual_transfer_history().active().is_ok_and(|active| active.is_none())
                || !self.engine().jobs().running().is_empty() {
                return Err(EngineError::Busy { running_op: "Meguri fixture requires idle transfer owners".into() });
            }
            // Host setup uses this same lane; a racing round reservation cannot
            // pass admission without a configured Meguri selection/binding.
            // Cell ownership is last and spans only the one-shot store.
            let mut policy = cell.policy.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if policy.is_some() { return Err(EngineError::Busy { running_op: "Meguri fixture policy already installed".into() }); }
            *policy = Some(candidate);
            Ok(())
        })
    }

    #[cfg(feature = "test-hooks")]
    fn meguri_fixture_transport(&self, connection: &meguri_sync::provider::MeguriConnection)
        -> Result<Option<Box<dyn meguri_sync::SyncTransport>>, FixtureTransportFailure> {
        let policy = self.fixture_transport.policy.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let Some(policy) = policy else { return Ok(None); };
        let generation = self.cloud_connection_generation()?;
        let collection = self.cloud_connection_collection()?;
        let original = self.original_resource_authority(&collection);
        self.publish_provider_metadata(generation, |_| {
            if collection.path() != self.fixture_transport.owner.collection || !original.is_live()
                || meguri_sync::provider::native_endpoint(&connection.endpoint).ok()
                    .is_none_or(|endpoint| !endpoint.as_str().starts_with(policy.origin.as_str())) {
                return Err(EngineError::CollectionUnavailable { reason: "Meguri fixture original owner or origin changed".into() });
            }
            collection.open_evidence()?;
            // Retained bootstrap preparation intentionally closes ordinary
            // network admission. Its phase-specific checks remain with the
            // existing bootstrap caller; do not prevent its media continuation.
            let doc = self.provider.snapshot()?;
            if doc.handoff.is_some() || doc.meguri.as_ref() != Some(connection) {
                return Err(EngineError::CollectionUnavailable { reason: "Meguri fixture connection changed".into() });
            }
            Ok(())
        })?;
        let http = meguri_sync::transport::http::HttpTransport::new(&policy.options())
            .map_err(FixtureTransportFailure::Transport)?;
        Ok(Some(Box::new(FixtureHttpTransport { owner: self.fixture_transport.owner.clone(), policy, http })))
    }

    /// Next slice must call this from BOTH FFI access helpers: bootstrap's
    /// ProviderConnection::access and normal's round_access. No FFI is wired here.
    #[cfg(feature = "test-hooks")]
    pub fn meguri_access_for_testing(&self, connection: meguri_sync::provider::MeguriConnection,
        username: &str, key: meguri_sync::HostKey,
    ) -> Result<crate::MeguriAccess, crate::MeguriBootstrapError> {
        let access = crate::MeguriAccess::new(connection.clone(), username, key)?;
        match self.meguri_fixture_transport(&connection) {
            Ok(Some(transport)) => Ok(access.with_transport_for_testing(transport)),
            Ok(None) => Ok(access),
            Err(FixtureTransportFailure::Owner(error)) => Err(error.into()),
            Err(FixtureTransportFailure::Transport(error)) => Err(crate::MeguriBootstrapError::Transfer(
                meguri_sync::oneway::TransferError::Server(meguri_sync::JobError::from_transport(
                    error, None, meguri_sync::ServerKind::SelfHosted)))),
        }
    }

    pub fn migrate_provider_configuration(root: &std::path::Path, legacy: meguri_sync::provider::LegacyInput) -> Result<(), meguri_storage::StorageError> {
        Authority::migrate_input(root, legacy)
    }
    pub fn provider_authority(&self) -> &Arc<Authority> { &self.provider }
    /// Whether this device's own Meguri Sync setup transfer (MP4) is what keeps
    /// network admission closed: Meguri Sync is selected with its connection, no
    /// handoff is recorded, and the active manual transfer is that connection's
    /// collection-and-media setup (the same reads as `active_meguri_preparation`).
    /// Admission stays closed from preparation until the media finishes; this
    /// changes no admission. A provider phase read from it is setup in progress,
    /// not MP1's unreadable metadata. Any read failure or mismatch answers false.
    pub fn meguri_setup_transfer_holds_admission(&self) -> bool {
        self.provider.network_blocked()
            && self.provider.snapshot().ok().and_then(|doc| doc.meguri)
                .is_some_and(|connection| self.active_meguri_preparation(&connection).is_ok_and(|active| active.is_some()))
    }
    /// Local failures leave the collection available. No manual operation or R6
    /// decision is finished or rewritten by this refresh.
    pub fn refresh_provider_admission(&self) {
        if !self.manual_transfer_history().active().is_ok_and(|active| active.is_none()) {
            self.provider.close_admission();
            return;
        }
        if self.provider.reload().is_err() { return; }
        // MP1/MP6.3: replacement changes the publication, not the account.
        // Keep history for the same-account choice, but retire stale routine
        // admission through the existing original-Host metadata lane. This
        // also handles a restore that activated but could only reopen later.
        if let Ok(doc) = self.provider.snapshot()
            && doc.selection == Selection::Meguri
            && doc.handoff.is_none()
            && let Some(binding) = doc.meguri_binding {
            let stale = match self.meguri_readiness() {
                Ok(meguri_sync::provider::MeguriReadiness::Stale) => true,
                Ok(_) => false,
                Err(_) => {
                    self.provider.close_admission();
                    return;
                }
            };
            if stale {
                let Ok(collection) = self.collection() else {
                    self.provider.close_admission();
                    return;
                };
                let generation = collection.generation();
                let held = self.publish_provider_metadata(generation, |publication| {
                    if self.provider.meguri_readiness(publication.as_ref()) != meguri_sync::provider::MeguriReadiness::Stale {
                        return Ok(Ok(()));
                    }
                    Ok(self.provider.hold_meguri_binding_for_full_sync(&binding, generation.0, publication))
                });
                if !matches!(held, Ok(Ok(()))) {
                    self.provider.close_admission();
                    return;
                }
            }
        }
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
        if self.save_initial_adoption_pause(generation, settings)? {
            // No qualification/reload here: this metadata write grants no
            // binding or writer authority. Resume still needs fresh proof.
            return Ok(());
        }
        self.publish_provider_metadata(generation, |publication| {
            self.provider.save_settings_for_owner(settings, generation.0, publication)?;
            Ok(())
        })?;
        self.refresh_provider_admission();
        Ok(())
    }
    /// Handoff recovery facts never certify that an earlier process joined.
    fn provider_publication_facts(&self) -> Result<(meguri_engine::Generation, Publication), EngineError> {
        self.require_meguri_round_join()?;
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
        self.require_meguri_round_join()?;
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
        // MP7 step 1: the structure save job holds the lane for its whole
        // length; a switch or Off waits for it (MP3 step 1). One step with
        // the closure, so that no job is admitted in between.
        self.close_meguri_round_admission()?;
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
        self.require_meguri_round_join()?;
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
        self.reopen_meguri_round_admission();
        Ok(())
    }
    /// Retain only an original healthy absent owner. The lane checks Host
    /// liveness, generation, recovery ownership and file absence without any
    /// blocking close-state query. Its optional settled publication is authority.
    fn reserve_no_collection_provider_handoff(&self, generation: meguri_engine::Generation)
        -> Result<(meguri_engine::SlotGuard, Option<Publication>), EngineError> {
        self.require_meguri_round_join()?;
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
        // A new connection drops the binding and the record of a structure save: never under a running job.
        if self.meguri_structure_job_active() {
            return Err(EngineError::Busy { running_op: "Meguri note type structure save".into() });
        }
        // RandomSource is a Host input, so sample before any ownership locks.
        let random = self.provider_random();
        let reference = format!("{:016x}{:016x}", random.next_u64(), random.next_u64());
        let connection = self.publish_provider_metadata(generation, |publication| {
            // Again inside the write: a job admitted since the check above.
            if self.meguri_structure_job_active() {
                return Err(EngineError::Busy { running_op: "Meguri note type structure save".into() });
            }
            Ok(self.provider.configure_meguri_for_owner(endpoint, reference, generation.0, publication)?)
        })?;
        self.refresh_provider_admission();
        Ok(connection)
    }
    pub fn authenticate_meguri_connection(&self, connection: &meguri_sync::provider::MeguriConnection,
        username: &str, password: &str, cancel: Arc<meguri_engine::CancelToken>,
    ) -> Result<meguri_sync::provider::AuthenticatedConnection, meguri_sync::provider::AuthenticationFailure> {
        #[cfg(feature = "test-hooks")]
        let transport = self.meguri_fixture_transport(connection).map_err(|error| match error {
            FixtureTransportFailure::Owner(_) => meguri_sync::provider::AuthenticationFailure::ConnectionChanged,
            FixtureTransportFailure::Transport(error) => meguri_sync::provider::AuthenticationFailure::Transport(error),
        })?;
        #[cfg(not(feature = "test-hooks"))]
        let transport = None;
        meguri_sync::provider::authenticate(&self.provider, connection, self.engine().threads(),
            self.provider_random(), username, password, cancel, transport)
    }
}
