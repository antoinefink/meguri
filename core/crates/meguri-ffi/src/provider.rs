//! Thin facade retaining the ORIGINAL installation Host. No legacy session is exported.
use std::sync::{Arc, Mutex, PoisonError};
use crate::{CoreError, callback, runtime};
use meguri_sync::provider as p;
#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
#[path = "provider_bootstrap.rs"] mod bootstrap;
#[path = "provider_normal.rs"] mod normal;
#[path = "provider_structure.rs"] mod structure;

// Rust integration fixtures name the same opaque objects as the FFI exports.
#[cfg(feature = "test-hooks")]
pub use bootstrap::{ProviderBootstrapDirection, ProviderBootstrapJob, ProviderBootstrapOutcome};
#[cfg(feature = "test-hooks")]
pub use normal::{ProviderRoundRefusal, ProviderRoundStart};
#[cfg(feature = "test-hooks")]
pub use structure::{ProviderStructureJob, ProviderStructureOp, ProviderStructureOutcome,
    ProviderStructureRefusal, ProviderStructureSaved, ProviderStructureStage, ProviderStructureStart,
    ProviderStructureStep, ProviderStructureUpload, ProviderUploadPending};

#[cfg(feature = "test-hooks")]
pub(crate) fn owner_for_testing(host: Arc<meguri_host::CoreHost>) -> Result<Arc<ProviderOwner>, CoreError> {
    let generation = host.cloud_connection_generation()?;
    Ok(Arc::new(ProviderOwner { host, generation }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderSelection { LocalOnly, ICloud, Meguri }
impl From<ProviderSelection> for p::Selection {
    fn from(value: ProviderSelection) -> Self { match value { ProviderSelection::LocalOnly => Self::LocalOnly, ProviderSelection::ICloud => Self::ICloud, ProviderSelection::Meguri => Self::Meguri } }
}
impl From<p::Selection> for ProviderSelection {
    fn from(value: p::Selection) -> Self { match value { p::Selection::LocalOnly => Self::LocalOnly, p::Selection::ICloud => Self::ICloud, p::Selection::Meguri => Self::Meguri } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ProviderSettings { pub enabled: bool, pub paused: bool, pub automatic: bool, pub media: bool }
impl From<ProviderSettings> for p::LegacySettings {
    fn from(v: ProviderSettings) -> Self { Self { enabled: v.enabled, paused: v.paused, automatic: v.automatic, media: v.media } }
}
impl From<p::LegacySettings> for ProviderSettings {
    fn from(v: p::LegacySettings) -> Self { Self { enabled: v.enabled, paused: v.paused, automatic: v.automatic, media: v.media } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderPhase { LocalOnly, SetupRequired, ICloud, HandoffRecovery, Blocked }
#[derive(Clone, Debug, uniffi::Record)]
pub struct ProviderSnapshot { pub selection: Option<ProviderSelection>, pub phase: ProviderPhase, pub settings: Option<ProviderSettings>, pub revision: u64, pub removal_pending: bool }
fn snapshot(value: Result<p::ProviderDocument, meguri_storage::StorageError>) -> ProviderSnapshot {
    match value {
        Ok(doc) => ProviderSnapshot { selection: Some(doc.selection.into()), phase: if doc.handoff.is_some() { ProviderPhase::HandoffRecovery }
            else if doc.selection == p::Selection::LocalOnly { ProviderPhase::LocalOnly }
            else if doc.selection == p::Selection::ICloud && doc.binding.is_some() { ProviderPhase::ICloud }
            else { ProviderPhase::SetupRequired }, settings: Some(doc.settings.into()), revision: doc.revision,
            removal_pending: doc.handoff.as_ref().is_some_and(p::HandoffIntent::removal_pending) },
        Err(_) => ProviderSnapshot { selection: None, phase: ProviderPhase::Blocked, settings: None, revision: 0, removal_pending: false },
    }
}
/// Blocked is MP1's phase: metadata that cannot be read or published, or
/// admission closed for any other reason. A Meguri setup transfer keeps
/// admission closed until its media finishes; that is setup in progress,
/// so its phase is the document's. Admission itself stays closed either way.
fn owner_snapshot(host: &meguri_host::CoreHost) -> ProviderSnapshot {
    let authority = host.provider_authority();
    admitted_snapshot(authority.snapshot(), authority.network_blocked(), host.meguri_setup_transfer_holds_admission())
}
fn admitted_snapshot(value: Result<p::ProviderDocument, meguri_storage::StorageError>,
    network_blocked: bool, setup_transfer_holds_admission: bool) -> ProviderSnapshot {
    let mut state = snapshot(value);
    if network_blocked && state.phase != ProviderPhase::HandoffRecovery
        && !setup_transfer_holds_admission { state.phase = ProviderPhase::Blocked; }
    state
}
#[uniffi::export]
pub fn migrate_provider(root: String, legacy_json: Option<String>) -> Result<ProviderSnapshot, CoreError> {
    callback::enter("migrate_provider");
    let legacy = legacy_json.as_ref().map(|value| serde_json::from_str(value)).transpose()
        .map_err(|_| CoreError::InvalidArgument { message: "invalid legacy provider settings".into() })?;
    let input = match legacy.as_ref() {
        None => p::LegacyInput::Absent,
        Some(value) => match p::LegacySettings::decode(Some(value)) {
            Ok(settings) => p::LegacyInput::Settings(settings), Err(_) => p::LegacyInput::Invalid,
        },
    };
    meguri_host::CoreHost::migrate_provider_configuration(std::path::Path::new(&root), input)?;
    Ok(snapshot(p::Authority::load(std::path::Path::new(&root)).snapshot()))
}
#[derive(uniffi::Object)]
pub struct ProviderOwner { host: Arc<meguri_host::CoreHost>, generation: meguri_engine::Generation }
#[uniffi::export]
pub fn provider_owner() -> Result<Arc<ProviderOwner>, CoreError> {
    callback::enter("provider_owner");
    let host = runtime::host()?;
    let generation = host.cloud_connection_generation()?;
    Ok(Arc::new(ProviderOwner { host, generation }))
}
impl ProviderOwner {
    fn validate(&self) -> Result<(), CoreError> {
        if self.host.cloud_connection_generation()? != self.generation {
            return Err(CoreError::Unavailable { reason: "provider original host changed".into() });
        }
        Ok(())
    }
}
#[cfg(feature = "test-hooks")]
#[uniffi::export]
impl ProviderOwner {
    pub fn install_meguri_fixture_transport_for_testing(
        &self, collection_path: String, installation_directory: String,
        origin: String, ca_der: Vec<u8>,
    ) -> Result<(), CoreError> {
        callback::enter("ProviderOwner.install_meguri_fixture_transport_for_testing");
        self.validate()?;
        self.host.install_meguri_fixture_transport(
            self.generation,
            std::path::Path::new(&collection_path), std::path::Path::new(&installation_directory),
            &origin, &ca_der,
        )?;
        Ok(())
    }
}
#[derive(uniffi::Object)]
pub struct ProviderDiscovery { _inner: p::Discovery }
#[derive(uniffi::Object)]
pub struct ProviderHandoff { owner: Arc<ProviderOwner>, inner: Mutex<Option<meguri_host::ProviderHandoff>> }
#[derive(uniffi::Object)]
pub struct ProviderConnection { owner: Arc<ProviderOwner>, inner: p::MeguriConnection }
#[derive(Clone, Debug, uniffi::Record)]
pub struct ProviderConnectionInformation { pub reference: String, pub revision: u64, pub endpoint: String }
#[derive(Clone, uniffi::Record)]
pub struct ProviderCredentials { pub connection: ProviderConnectionInformation, pub username: String, pub hostkey: String }
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderLoginFailure { ConnectionChanged, Username, Cancelled, Credentials, RedirectRefused, Network }
#[derive(Clone, uniffi::Enum)]
pub enum ProviderLoginOutcome { Authenticated { credentials: ProviderCredentials }, Refused { failure: ProviderLoginFailure } }
#[derive(uniffi::Object)]
pub struct ProviderLoginResult { value: Mutex<Option<ProviderLoginOutcome>> }
#[derive(Clone, uniffi::Record)]
pub struct ProviderLoginJob { pub handle: crate::jobs::JobHandle, pub result: Arc<ProviderLoginResult> }
#[uniffi::export]
impl ProviderLoginResult {
    pub fn result(&self) -> Result<Option<ProviderLoginOutcome>, CoreError> {
        callback::enter("ProviderLoginResult.result");
        Ok(self.value.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
}
fn information(connection: &p::MeguriConnection) -> ProviderConnectionInformation {
    ProviderConnectionInformation { reference: connection.reference.clone(), revision: connection.revision, endpoint: connection.endpoint.clone() }
}
#[uniffi::export]
impl ProviderConnection {
    pub fn information(&self) -> Result<ProviderConnectionInformation, CoreError> {
        callback::enter("ProviderConnection.information");
        self.owner.validate()?;
        if self.owner.host.provider_authority().snapshot()?.meguri.as_ref() != Some(&self.inner) {
            return Err(CoreError::Unavailable { reason: "Meguri connection changed".into() });
        }
        Ok(information(&self.inner))
    }
    pub fn start_login(self: Arc<Self>, username: String, mut password: String) -> Result<ProviderLoginJob, CoreError> {
        callback::enter("ProviderConnection.start_login");
        self.owner.validate()?;
        self.owner.host.provider_authority().validate_connection(&self.inner)?;
        let result = Arc::new(ProviderLoginResult { value: Mutex::new(None) });
        let completion = result.clone();
        let host = self.owner.host.clone();
        let handle = host.start_cloud_connection_job(self.owner.generation, "Meguri authentication", move |context| {
            let value = match self.owner.host.authenticate_meguri_connection(&self.inner, &username, &password, context.cancel.clone()) {
                Ok(value) => ProviderLoginOutcome::Authenticated { credentials: ProviderCredentials {
                    connection: information(&value.connection), username: value.username, hostkey: value.hostkey } },
                Err(error) => {
                    use p::AuthenticationFailure as F;
                    let failure = match error {
                        F::ConnectionChanged => ProviderLoginFailure::ConnectionChanged,
                        F::Username => ProviderLoginFailure::Username,
                        F::Cancelled | F::Transport(meguri_sync::SyncError::Cancelled) => ProviderLoginFailure::Cancelled,
                        F::Transport(meguri_sync::SyncError::Redirect(_)) => ProviderLoginFailure::RedirectRefused,
                        F::Transport(error) if error.is_auth_failure() => ProviderLoginFailure::Credentials,
                        F::Transport(_) => ProviderLoginFailure::Network,
                    };
                    ProviderLoginOutcome::Refused { failure }
                },
            };
            password.clear();
            *completion.value.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
            Ok(())
        })?;
        Ok(ProviderLoginJob { handle: crate::jobs::wrap(handle), result })
    }
}
#[uniffi::export]
impl ProviderOwner {
    pub fn configured_meguri(self: Arc<Self>) -> Result<Option<Arc<ProviderConnection>>, CoreError> {
        callback::enter("ProviderOwner.configured_meguri"); self.validate()?;
        let connection = self.host.provider_authority().snapshot()?.meguri;
        Ok(connection.map(|inner| Arc::new(ProviderConnection { owner: self, inner })))
    }
    pub fn snapshot(&self) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.snapshot"); self.validate()?;
        Ok(owner_snapshot(&self.host))
    }
    /// `sync/meguri-provider` MP7 step 5: a note type structure save is
    /// committed on this device and its upload has not run. While true the
    /// status shows `sync.needsYou.meguri.structureUpload` with Upload…, in
    /// place of the collection choice, and no round starts.
    pub fn structure_upload_pending(&self) -> Result<bool, CoreError> {
        callback::enter("ProviderOwner.structure_upload_pending"); self.validate()?;
        Ok(self.host.meguri_structure_upload_pending()?)
    }
    pub fn refresh(&self) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.refresh"); self.validate()?;
        self.host.refresh_provider_admission();
        Ok(owner_snapshot(&self.host))
    }
    pub fn save_settings(&self, settings: ProviderSettings) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.save_settings"); self.validate()?;
        self.host.save_provider_settings_for_generation(self.generation, settings.into())?;
        Ok(owner_snapshot(&self.host))
    }
    pub fn begin_icloud_discovery(&self) -> Result<Arc<ProviderDiscovery>, CoreError> {
        callback::enter("ProviderOwner.begin_icloud_discovery"); self.validate()?;
        Ok(Arc::new(ProviderDiscovery { _inner: self.host.provider_authority().begin_discovery()? }))
    }
    pub fn accept_icloud_connection(&self) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.accept_icloud_connection"); self.validate()?;
        if !self.host.manual_transfer_history().active().is_ok_and(|active| active.is_none()) {
            return Err(CoreError::Busy { running_op: "manual transfer recovery".into() });
        }
        self.host.cloud_runtime().ok_or_else(|| CoreError::Unavailable { reason: "iCloud unavailable".into() })?
            .provider_accept_icloud_connection().map_err(|_| CoreError::Unavailable { reason: "iCloud connection requires recovery".into() })?;
        self.host.refresh_provider_admission();
        Ok(owner_snapshot(&self.host))
    }
    pub fn configure_meguri(self: Arc<Self>, endpoint: String) -> Result<Arc<ProviderConnection>, CoreError> {
        callback::enter("ProviderOwner.configure_meguri"); self.validate()?;
        let inner = self.host.configure_meguri_connection_for_generation(self.generation, &endpoint)?;
        Ok(Arc::new(ProviderConnection { owner: self, inner }))
    }
    pub fn begin_handoff(self: Arc<Self>, destination: ProviderSelection, remove_credentials: bool) -> Result<Arc<ProviderHandoff>, CoreError> {
        callback::enter("ProviderOwner.begin_handoff"); self.validate()?;
        let inner = self.host.begin_provider_handoff_with_removal(destination.into(), remove_credentials)?;
        Ok(Arc::new(ProviderHandoff { owner: self, inner: Mutex::new(Some(inner)) }))
    }
    pub fn recover_handoff_to_local_only(&self) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.recover_handoff_to_local_only"); self.validate()?;
        self.host.recover_provider_handoff_to_local_only()?;
        Ok(owner_snapshot(&self.host))
    }
    pub fn retry_failed_publication(&self) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.retry_failed_publication"); self.validate()?;
        self.host.retry_provider_publication_for_generation(self.generation)?;
        Ok(owner_snapshot(&self.host))
    }
    pub fn finish_handoff(self: Arc<Self>, handoff: Arc<ProviderHandoff>) -> Result<ProviderSnapshot, CoreError> {
        callback::enter("ProviderOwner.finish_handoff"); self.validate()?;
        if !Arc::ptr_eq(&self, &handoff.owner) { return Err(CoreError::Unavailable { reason: "provider handoff owner changed".into() }); }
        let mut inner = handoff.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.host.finish_provider_handoff(inner.as_ref().ok_or_else(|| CoreError::Unavailable { reason: "provider handoff completed".into() })?)?;
        *inner = None;
        Ok(owner_snapshot(&self.host))
    }
}
