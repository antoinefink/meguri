//! Device-local service choice. Protocol jobs never infer admission from credentials.
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex, PoisonError, atomic::{AtomicBool, Ordering}}};
use meguri_engine::device_state::{Publication, PublicationDecision};
use meguri_storage::{StorageError, sidecar};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use crate::{Endpoint, cloud::{Binding, BindingState}};

mod bootstrap;
pub use bootstrap::{MeguriRoundAdmission, MeguriRoundRefusal, EpochEvidence, EpochInputs, MediaEpochEvidence, MediaRefusal, MeguriBinding, MeguriBootstrapIntent, MeguriReadiness, MeguriReadyRefusal, ReadyEvidence, ReadyInputs};

fn invalid() -> StorageError { StorageError::Io { detail: "provider configuration requires recovery".into() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Selection { LocalOnly, ICloud, Meguri }
pub enum LegacyInput { Absent, Settings(LegacySettings), Invalid }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySettings {
    pub enabled: bool, pub paused: bool, pub automatic: bool, pub media: bool,
}
impl LegacySettings {
    /// Absence alone permits defaults. A present legacy value is one complete record.
    pub fn decode(value: Option<&Value>) -> Result<Self, StorageError> {
        let Some(value) = value else { return Ok(Self { enabled: false, paused: false, automatic: true, media: true }); };
        let object = value.as_object().ok_or_else(invalid)?;
        let boolean = |key| object.get(key).and_then(Value::as_bool).ok_or_else(invalid);
        Ok(Self { enabled: boolean("enabled")?, paused: boolean("paused")?, automatic: boolean("automatic")?, media: boolean("media")? })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudBinding {
    binding: [u8; 16], account: Vec<u8>, publication: Publication,
    container: String, environment: crate::cloud::Environment,
}
impl CloudBinding {
    pub(crate) fn matches(&self, binding: &Binding) -> bool {
        self.binding == binding.id.0 && self.account == binding.facts.account.0
            && self.publication.identity == binding.facts.publication
            && self.container == binding.scope.container.as_str() && self.environment == binding.scope.environment
            && binding.facts.state == BindingState::Active
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffIntent {
    old: Selection, destination: Selection, binding: Option<CloudBinding>,
    publication: Option<Publication>, generation: u64, revision: u64,
    remove_credentials: bool,
}
impl HandoffIntent {
    pub fn removal_pending(&self) -> bool { self.remove_credentials }
    pub(crate) fn revision(&self) -> u64 { self.revision }
    pub(crate) fn generation(&self) -> u64 { self.generation }
    pub(crate) fn binding(&self) -> Option<&CloudBinding> { self.binding.as_ref() }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDocument {
    pub version: u64, pub revision: u64, pub selection: Selection,
    pub settings: LegacySettings, pub binding: Option<CloudBinding>,
    pub handoff: Option<HandoffIntent>,
    /// A migration may qualify only the actual retained iCloud binding.
    pub legacy_binding_pending: bool,
    pub meguri: Option<MeguriConnection>,
    pub icloud_settings: LegacySettings,
    pub meguri_settings: LegacySettings,
    #[serde(default)] pub meguri_binding: Option<MeguriBinding>,
    #[serde(default)] pub meguri_bootstrap: Option<MeguriBootstrapIntent>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeguriConnection { pub reference: String, pub revision: u64, pub endpoint: String }
impl ProviderDocument {
    pub fn migration(settings: LegacySettings) -> Self {
        Self { version: 1, revision: 1, selection: if settings.enabled { Selection::ICloud } else { Selection::LocalOnly },
            settings, binding: None, handoff: None, legacy_binding_pending: settings.enabled, meguri: None,
            icloud_settings: settings, meguri_settings: LegacySettings { enabled: false, paused: false, automatic: false, media: true },
            meguri_binding: None, meguri_bootstrap: None }
    }
    pub fn permits_icloud(&self) -> bool {
        self.selection == Selection::ICloud && self.settings.enabled && !self.settings.paused
            && self.binding.is_some() && self.handoff.is_none()
    }
    /// MP-B/MP-C own durable collection-and-media completion. No ready setter exists here.
    pub fn permits_meguri_transfer(&self) -> bool { self.selection == Selection::Meguri && self.settings.enabled && !self.settings.paused && self.handoff.is_none() && self.meguri_binding.as_ref().is_some_and(|b| Some(b.connection()) == self.meguri.as_ref()) }
    fn validate(&self) -> Result<(), StorageError> {
        if self.version != 1 || self.revision == 0
            || (self.selection == Selection::Meguri && self.settings.automatic && self.meguri_binding.is_none())
            || (self.meguri_settings.automatic && self.meguri_binding.is_none()) || !self.meguri_settings.media
            || self.meguri_binding.as_ref().is_some_and(|b| Some(b.connection()) != self.meguri.as_ref())
            || self.meguri_bootstrap.as_ref().is_some_and(|i| Some(i.connection()) != self.meguri.as_ref())
            || (self.meguri_binding.is_some() && self.meguri_bootstrap.is_some())
            || self.meguri.as_ref().is_some_and(|c| c.reference.is_empty() || c.reference.len() > 128 || c.endpoint.len() > 4096 || c.revision == 0)
            || self.binding.as_ref().is_some_and(|b| b.account.len() > 4096 || b.container.len() > 1024)
            || self.handoff.as_ref().is_some_and(|h| h.revision != self.revision || h.old != self.selection)
        { return Err(invalid()); }
        Ok(())
    }
}

fn decode(doc: Option<&Map<String, Value>>) -> Result<Option<ProviderDocument>, StorageError> {
    doc.map(|doc| {
        // R6's types are strict for their own document. This document stores a
        // comparison snapshot, whose future members must survive our RMW too.
        // Project known snapshot members for decoding; put() merges into raw.
        let mut projection = Value::Object(doc.clone());
        if let Some(binding) = projection.get_mut("binding") { project_binding(binding); }
        if let Some(intent) = projection.get_mut("handoff") {
            if let Some(binding) = intent.get_mut("binding") { project_binding(binding); }
            if let Some(publication) = intent.get_mut("publication") { project_publication(publication); }
        }
        let value: ProviderDocument = serde_json::from_value(projection).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }).transpose()
}
fn project_binding(value: &mut Value) {
    if let Some(publication) = value.get_mut("publication") { project_publication(publication); }
}
fn project_publication(value: &mut Value) {
    if let Some(publication) = value.as_object_mut() {
        publication.retain(|key, _| matches!(key.as_str(), "identity" | "binding" | "decision"));
        if let Some(connected) = publication.get_mut("binding").and_then(Value::as_object_mut)
            .and_then(|binding| binding.get_mut("Connected")).and_then(Value::as_object_mut) {
            connected.retain(|key, _| matches!(key.as_str(), "account" | "configuration" | "environment"));
        }
    }
}
fn merge(target: &mut Value, source: Value) {
    if let (Value::Object(target), Value::Object(source)) = (&mut *target, &source) {
        for (key, value) in source { merge(target.entry(key.clone()).or_insert(Value::Null), value.clone()); }
    } else { *target = source; }
}
fn put(doc: &mut Map<String, Value>, value: &ProviderDocument) -> Result<(), StorageError> {
    value.validate()?;
    let mut merged = Value::Object(doc.clone());
    merge(&mut merged, serde_json::to_value(value).map_err(|_| invalid())?);
    *doc = merged.as_object().ok_or_else(invalid)?.clone();
    Ok(())
}

/// One retained installation owner. Runtime checks only its bounded cached facts;
/// sidecar operations never hold Runtime/tracking ownership.
pub struct Authority {
    path: PathBuf, state: Mutex<AuthorityState>,
    open: AtomicBool,
    writers: AtomicBool,
    discovery: Mutex<(u64, usize)>,
    original_completion: AtomicBool,
    completion: Mutex<Option<CloudBinding>>,
    // Serializes reload/publication, including the failed desired mutation.
    publication_failure: Mutex<Option<PublicationFailure>>,
    #[cfg(feature = "test-hooks")]
    setup_contention: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    #[cfg(feature = "test-hooks")]
    cancellation_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    restart_intent: Option<HandoffIntent>,
}
struct AuthorityState {
    document: Result<ProviderDocument, StorageError>,
    closures: AdmissionClosures,
}
/// Local closure identity; never serialized or used as a policy/SDK token.
#[derive(Clone)]
pub(crate) struct AdmissionClosures {
    ordinary: Arc<()>,
    handoff_revision: Option<u64>,
}
impl AdmissionClosures {
    pub(crate) fn owns_handoff_since(&self, issued: &Self, handoff: &HandoffIntent) -> bool {
        Arc::ptr_eq(&self.ordinary, &issued.ordinary)
            && self.handoff_revision == Some(handoff.revision())
    }
    fn ordinary_close(&mut self) {
        self.ordinary = Arc::new(());
        self.handoff_revision = None;
    }
}
struct PendingPublication {
    before: Map<String, Value>, desired: Map<String, Value>, document: ProviderDocument,
    facts: Option<(u64, Option<Publication>)>, retryable: bool,
    cancel: Option<CancelRestoration>,
}
#[derive(Clone, PartialEq)]
struct CancelRestoration {
    operation: crate::cloud::publication::PublishOp, before: Option<Binding>, after: Binding,
    replacement: Publication,
}
struct PublicationFailure { error: StorageError, pending: Option<PendingPublication> }
/// Process-only discovery ownership. It can never authorize collection writers.
pub struct Discovery { owner: Arc<Authority>, revision: u64 }
impl Drop for Discovery {
    fn drop(&mut self) {
        let mut discovery = self.owner.discovery.lock().unwrap_or_else(PoisonError::into_inner);
        if discovery.0 == self.revision { discovery.1 = discovery.1.saturating_sub(1); }
    }
}
impl Discovery {
    pub fn is_current(&self) -> bool { self.owner.snapshot().is_ok_and(|doc| doc.revision == self.revision && doc.handoff.is_none()) }
}
/// The original installation and intent, never reconstructed from a revision DTO.
pub struct Handoff { owner: Arc<Authority>, intent: HandoffIntent }
impl Authority {
    pub fn migrate_input(root: &Path, input: LegacyInput) -> Result<(), StorageError> {
        match input {
            LegacyInput::Absent => Self::migrate(root, None),
            LegacyInput::Invalid => Self::migrate(root, Some(&Value::Null)),
            LegacyInput::Settings(settings) => Self::migrate(root, Some(&serde_json::to_value(settings).map_err(|_| invalid())?)),
        }
    }
    pub fn load(root: &Path) -> Arc<Self> {
        let path = root.join("provider.json");
        let state = sidecar::read_document(&path, decode).and_then(|document| document.ok_or_else(invalid));
        let restart_intent = state.as_ref().ok().and_then(|doc| doc.handoff.clone());
        Arc::new(Self { path, state: Mutex::new(AuthorityState { document: state,
                closures: AdmissionClosures { ordinary: Arc::new(()), handoff_revision: None } }),
            open: AtomicBool::new(true), writers: AtomicBool::new(false), discovery: Mutex::new((0, 0)),
            original_completion: AtomicBool::new(false), completion: Mutex::new(None),
            publication_failure: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            setup_contention: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            cancellation_hook: Mutex::new(None),
            restart_intent })
    }
    /// Before Host composition. Valid existing metadata is never reimported.
    pub fn migrate(root: &Path, legacy: Option<&Value>) -> Result<(), StorageError> {
        let path = root.join("provider.json");
        if sidecar::read_document(&path, decode)?.is_some() { return Ok(()); }
        std::fs::create_dir_all(root)?;
        let settings = match LegacySettings::decode(legacy) {
            Ok(settings) => settings,
            Err(error) => {
                // Retain a present-invalid migration, including across a later
                // launch with missing preferences. Never fall back to iCloud.
                sidecar::update_document(&path, |doc| {
                    if path.symlink_metadata().is_ok() { decode(Some(doc))?; return Ok(()); }
                    doc.insert("migrationFailure".into(), Value::String("invalidLegacySettings".into()));
                    Ok(())
                })?;
                return Err(error);
            }
        };
        sidecar::update_document(&path, |doc| {
            if path.symlink_metadata().is_ok() { decode(Some(doc))?.ok_or_else(invalid)?; return Ok(()); }
            put(doc, &ProviderDocument::migration(settings))
        })
    }
    pub fn snapshot(&self) -> Result<ProviderDocument, StorageError> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).document.clone()
    }
    pub(crate) fn restart_intent(&self) -> Option<&HandoffIntent> { self.restart_intent.as_ref() }
    pub fn close_admission(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closures.ordinary_close();
        self.open.store(false, Ordering::SeqCst);
        self.writers.store(false, Ordering::SeqCst);
    }
    pub fn network_blocked(&self) -> bool { !self.open.load(Ordering::SeqCst) }
    pub fn reload(&self) -> Result<(), StorageError> {
        let failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        self.close_admission();
        if let Some(failure) = failure.as_ref() { return Err(failure.error.clone()); }
        let value = sidecar::read_document(&self.path, decode).and_then(|doc| doc.ok_or_else(invalid));
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.document = value.clone();
        value?;
        self.open.store(true, Ordering::SeqCst);
        Ok(())
    }
    pub(crate) fn validate_current_binding(&self, binding: &Binding, publication: &Publication) {
        let (valid, completion) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let valid = state.document.as_ref().is_ok_and(|doc| doc.handoff.is_none()
                && doc.binding.as_ref().is_some_and(|stored| stored.matches(binding) && &stored.publication == publication)
                && publication.decision == PublicationDecision::Clear);
            if !valid { state.closures.ordinary_close(); }
            self.writers.store(valid, Ordering::SeqCst);
            (valid, state.document.as_ref().ok().and_then(|doc| doc.binding.clone()))
        };
        if valid {
            *self.completion.lock().unwrap_or_else(PoisonError::into_inner) = completion;
        }
    }
    pub fn permits_icloud_binding(&self, binding: &Binding) -> bool {
        self.open.load(Ordering::SeqCst) && self.writers.load(Ordering::SeqCst)
            && self.snapshot().is_ok_and(|doc| doc.permits_icloud()
                && doc.binding.as_ref().is_some_and(|b| b.matches(binding)))
    }
    /// L3 protects all predicate writes, the document and closure provenance.
    /// The bounded sample must not acquire another lock or perform provider I/O.
    pub(crate) fn with_admission_sample<T>(&self, binding: &Binding,
        sample: impl FnOnce(&Result<ProviderDocument, StorageError>, bool, &AdmissionClosures) -> T,
    ) -> T {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let permits = self.open.load(Ordering::SeqCst) && self.writers.load(Ordering::SeqCst)
            && state.document.as_ref().is_ok_and(|doc| doc.permits_icloud()
                && doc.binding.as_ref().is_some_and(|b| b.matches(binding)));
        sample(&state.document, permits, &state.closures)
    }
    /// Original completions can settle during a durable handoff. Native enqueue
    /// continues to use permits_icloud_binding, which closes immediately.
    pub(crate) fn permits_icloud_completion(&self, binding: &Binding) -> bool {
        self.completion.lock().unwrap_or_else(PoisonError::into_inner).as_ref().is_some_and(|b| b.matches(binding))
    }
    pub fn permits_icloud_account_read(&self) -> bool {
        self.open.load(Ordering::SeqCst) && self.snapshot().is_ok_and(|doc|
            !doc.settings.paused && doc.handoff.is_none() &&
                ((doc.selection == Selection::ICloud && doc.settings.enabled) || {
                    let discovery = self.discovery.lock().unwrap_or_else(PoisonError::into_inner);
                    discovery.0 == doc.revision && discovery.1 != 0
                }))
    }
    pub fn permits_original_identity_completion(&self) -> bool {
        self.original_completion.load(Ordering::SeqCst)
    }
    pub fn begin_discovery(self: &Arc<Self>) -> Result<Discovery, StorageError> {
        let doc = self.snapshot()?;
        if !self.open.load(Ordering::SeqCst) || doc.handoff.is_some() || doc.settings.paused || doc.selection == Selection::Meguri { return Err(invalid()); }
        let mut discovery = self.discovery.lock().unwrap_or_else(PoisonError::into_inner);
        if discovery.0 != doc.revision { *discovery = (doc.revision, 0); }
        discovery.1 = discovery.1.checked_add(1).ok_or_else(invalid)?;
        Ok(Discovery { owner: self.clone(), revision: doc.revision })
    }
    pub fn save_settings(&self, settings: LegacySettings) -> Result<ProviderDocument, StorageError> {
        self.save_settings_with_facts(settings, None)
    }
    /// Host captures these current collection facts before attempting publication.
    pub fn save_settings_for_owner(&self, settings: LegacySettings, generation: u64, publication: Option<Publication>) -> Result<ProviderDocument, StorageError> {
        self.save_settings_with_facts(settings, Some((generation, publication)))
    }
    fn save_settings_with_facts(&self, settings: LegacySettings, facts: Option<(u64, Option<Publication>)>) -> Result<ProviderDocument, StorageError> {
        self.update_publication(facts, true, |_, doc| {
            if doc.handoff.is_some() || (doc.selection == Selection::Meguri && ((settings.automatic && doc.meguri_binding.is_none()) || !settings.media)) { return Err(invalid()); }
            // Enable never chooses a service or manufactures a binding.
            doc.settings = settings;
            if doc.selection == Selection::Meguri { doc.meguri_settings = settings; }
            else { doc.icloud_settings = settings; }
            Ok(())
        })
    }
    pub fn configure_meguri(&self, endpoint: &str, reference: String) -> Result<MeguriConnection, StorageError> {
        self.configure_meguri_with_facts(endpoint, reference, None)
    }
    pub fn configure_meguri_for_owner(&self, endpoint: &str, reference: String, generation: u64, publication: Option<Publication>) -> Result<MeguriConnection, StorageError> {
        self.configure_meguri_with_facts(endpoint, reference, Some((generation, publication)))
    }
    fn configure_meguri_with_facts(&self, endpoint: &str, reference: String, facts: Option<(u64, Option<Publication>)>) -> Result<MeguriConnection, StorageError> {
        let endpoint = native_endpoint(endpoint)?.as_str().to_owned();
        let doc = self.update_publication(facts, true, |_, doc| {
            if doc.handoff.is_some() { return Err(invalid()); }
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            doc.meguri_binding = None; doc.meguri_bootstrap = None;
            doc.meguri_settings.automatic = false;
            if doc.selection == Selection::Meguri { doc.settings.automatic = false; }
            doc.meguri = Some(MeguriConnection { reference, revision: doc.revision, endpoint });
            Ok(())
        })?;
        doc.meguri.ok_or_else(invalid)
    }
    pub fn validate_connection(&self, expected: &MeguriConnection) -> Result<(), StorageError> {
        let doc = self.snapshot()?;
        if !self.open.load(Ordering::SeqCst) || doc.handoff.is_some() || doc.meguri.as_ref() != Some(expected) { return Err(invalid()); }
        Ok(())
    }
    pub fn begin_handoff(self: &Arc<Self>, destination: Selection, publication: Option<Publication>, generation: u64) -> Result<Handoff, StorageError> {
        self.begin_handoff_with_removal(destination, publication, generation, false)
    }
    pub fn begin_handoff_with_removal(self: &Arc<Self>, destination: Selection, publication: Option<Publication>, generation: u64, remove_credentials: bool) -> Result<Handoff, StorageError> {
        self.original_completion.store(self.completion.lock().unwrap_or_else(PoisonError::into_inner).is_some(), Ordering::SeqCst);
        let mut failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            // Only this private durable path preserves ordinary closure identity.
            // No evidence exists during I/O or after a failed publication.
            state.closures.handoff_revision = None;
            self.open.store(false, Ordering::SeqCst);
            self.writers.store(false, Ordering::SeqCst);
        }
        if let Some(failure) = failure.as_ref() { return Err(failure.error.clone()); }
        let result = self.write_publication_locked(&mut failure, None, false, |_, doc| {
            if doc.handoff.is_some() { return Err(invalid()); }
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            doc.handoff = Some(HandoffIntent { old: doc.selection, destination, binding: doc.binding.clone(),
                publication, generation, revision: doc.revision, remove_credentials });
            Ok(())
        });
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.document = result.clone();
            state.closures.handoff_revision = result.as_ref().ok()
                .and_then(|doc| doc.handoff.as_ref().map(HandoffIntent::revision));
        }
        let doc = result?;
        Ok(Handoff { owner: self.clone(), intent: doc.handoff.ok_or_else(invalid)? })
    }
    /// Called only after the production owner joined its original native work.
    /// No serialized drain flag can produce this original intent handle.
    pub fn commit_handoff(&self, handoff: &Handoff) -> Result<ProviderDocument, StorageError> {
        self.validate_handoff(handoff)?;
        let result = self.update(|doc| {
            if doc.handoff.as_ref() != Some(&handoff.intent) { return Err(invalid()); }
            doc.selection = handoff.intent.destination;
            doc.binding = None; doc.legacy_binding_pending = false;
            doc.meguri_binding = None; doc.meguri_bootstrap = None;
            doc.meguri_settings.automatic = false;
            doc.settings = if doc.selection == Selection::Meguri { doc.meguri_settings } else { doc.icloud_settings };
            doc.settings.enabled = doc.selection != Selection::LocalOnly;
            if doc.selection == Selection::Meguri { doc.settings.automatic = false; }
            doc.handoff = None;
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        });
        if result.is_ok() {
            self.original_completion.store(false, Ordering::SeqCst);
            *self.completion.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        result
    }
    pub fn validate_handoff(&self, handoff: &Handoff) -> Result<(), StorageError> {
        if !std::ptr::eq(self, Arc::as_ptr(&handoff.owner))
            || self.snapshot()?.handoff.as_ref() != Some(&handoff.intent) { return Err(invalid()); }
        Ok(())
    }
    /// Recovery never recreates Handoff or its original owner's drain claim.
    /// Host must hold current collection ownership and exclude current work.
    pub fn recover_handoff_to_local_only(&self, generation: u64, publication: &Publication) -> Result<ProviderDocument, StorageError> {
        self.close_admission();
        self.validate_handoff_recovery(publication)?;
        let intent = self.restart_intent.as_ref().ok_or_else(invalid)?;
        let result = self.update_publication(Some((generation, Some(publication.clone()))), true, |raw, doc| {
            if doc.handoff.as_ref() != Some(intent) { return Err(invalid()); }
            // Retain the complete original intent, including future members.
            // This additive local evidence does not change the provider record
            // or the R6/tracking/manual-operation documents.
            let evidence = serde_json::json!({
                "intent": raw.get("handoff").ok_or_else(invalid)?,
                "outcome": "localOnlySetupRequired",
                "currentGeneration": generation, "currentPublication": publication,
            });
            let records = raw.entry("handoffRecoveries").or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut().ok_or_else(invalid)?;
            records.push(evidence);
            let was_meguri = doc.selection == Selection::Meguri;
            doc.meguri_binding = None; doc.meguri_bootstrap = None;
            doc.meguri_settings.automatic = false;
            if was_meguri { doc.settings.automatic = false; }
            doc.selection = Selection::LocalOnly;
            doc.settings.enabled = false;
            doc.binding = None;
            doc.legacy_binding_pending = false;
            doc.handoff = None;
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        });
        if result.is_ok() { self.retire_original_completions(); }
        result
    }
    pub fn validate_handoff_recovery(&self, publication: &Publication) -> Result<(), StorageError> {
        let intent = self.restart_intent.as_ref().ok_or_else(invalid)?;
        // Credential-removal completion belongs to its retained removal owner;
        // do not report that operation completed from publication facts alone.
        if intent.remove_credentials || intent.publication.as_ref() != Some(publication)
            || self.snapshot()?.handoff.as_ref() != Some(intent) { return Err(invalid()); }
        Ok(())
    }
    fn retire_original_completions(&self) {
        self.original_completion.store(false, Ordering::SeqCst);
        *self.completion.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
    /// Host selects exclusions from the retained mutation, never a caller's
    /// description of recovery or native drain completion.
    pub fn failed_publication_requires_handoff_recovery(&self) -> Result<bool, StorageError> {
        let failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        let failed = failure.as_ref().ok_or_else(invalid)?;
        let pending = failed.pending.as_ref().filter(|pending| pending.retryable).ok_or_else(|| failed.error.clone())?;
        Ok(decode(Some(&pending.before))?.ok_or_else(invalid)?.handoff.is_some())
    }
    /// Explicit retry of the actual failed mutation, never a force reload. A
    /// handoff destination commit cannot be retried without its original owner.
    pub fn retry_failed_publication(&self, generation: u64, publication: Option<&Publication>) -> Result<ProviderDocument, StorageError> {
        let mut failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        self.close_admission();
        let failed = failure.as_ref().ok_or_else(invalid)?;
        let pending = failed.pending.as_ref().filter(|pending| pending.retryable).ok_or_else(|| failed.error.clone())?;
        if let Some((expected_generation, expected_publication)) = &pending.facts {
            if generation != *expected_generation || publication != expected_publication.as_ref() { return Err(invalid()); }
        } else {
            let prior = decode(Some(&pending.before))?.ok_or_else(invalid)?;
            if let Some(expected) = prior.binding.as_ref().map(|binding| &binding.publication)
                && publication != Some(expected) { return Err(invalid()); }
        }
        let result = sidecar::update_document(&self.path, |raw| {
            // A rename may have succeeded before directory durability failed.
            // Re-publish that same desired state, but reject any successor edit.
            if raw != &pending.before && raw != &pending.desired { return Err(invalid()); }
            decode(Some(raw))?.ok_or_else(invalid)?;
            *raw = pending.desired.clone();
            Ok(pending.document.clone())
        });
        match result {
            Ok(doc) => {
                self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Ok(doc.clone());
                *failure = None;
                if doc.handoff.is_none() { self.retire_original_completions(); }
                {
                    let _state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    self.open.store(true, Ordering::SeqCst);
                }
                Ok(doc)
            }
            Err(error) => {
                // Preserve the original publication failure category and the
                // retained desired mutation even when retry also fails.
                self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Err(failed.error.clone());
                Err(error)
            }
        }
    }
    pub(crate) fn accept_icloud_setup<E: From<StorageError>>(&self,
        current: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        #[cfg(feature = "test-hooks")]
        if let Err(std::sync::TryLockError::WouldBlock) = self.publication_failure.try_lock()
            && let Some(sender) = self.setup_contention.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let _ = sender.send(());
        }
        let mut failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        // Check the exact Runtime target while setup owns publication, then
        // release State/tracking before touching the provider document.
        current()?;
        self.update_publication_locked(&mut failure, None, false, |_, doc| {
            if doc.handoff.is_some() || doc.selection == Selection::Meguri { return Err(invalid()); }
            doc.selection = Selection::ICloud; doc.settings.enabled = true;
            doc.legacy_binding_pending = true;
            Ok(())
        })?;
        Ok(())
    }
    /// Reports only that setup encountered an occupied publication gate.
    #[cfg(feature = "test-hooks")]
    pub fn observe_setup_contention_for_testing(&self, sender: std::sync::mpsc::Sender<()>) {
        *self.setup_contention.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    }
    fn update(&self, change: impl FnOnce(&mut ProviderDocument) -> Result<(), StorageError>) -> Result<ProviderDocument, StorageError> {
        self.update_publication(None, false, |_, doc| change(doc))
    }
    fn update_publication(&self, facts: Option<(u64, Option<Publication>)>, retryable: bool,
        change: impl FnOnce(&mut Map<String, Value>, &mut ProviderDocument) -> Result<(), StorageError>,
    ) -> Result<ProviderDocument, StorageError> {
        let mut failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        self.update_publication_locked(&mut failure, facts, retryable, change)
    }
    fn update_publication_locked(&self, failure: &mut Option<PublicationFailure>,
        facts: Option<(u64, Option<Publication>)>, retryable: bool,
        change: impl FnOnce(&mut Map<String, Value>, &mut ProviderDocument) -> Result<(), StorageError>,
    ) -> Result<ProviderDocument, StorageError> {
        self.close_admission();
        if let Some(failure) = failure.as_ref() { return Err(failure.error.clone()); }
        let result = self.write_publication_locked(failure, facts, retryable, change);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.document = result.clone();
        if result.is_ok() { self.open.store(true, Ordering::SeqCst); }
        result
    }
    // L1 is held. Sidecar I/O holds neither Runtime State nor the L3 cache.
    // Each caller publishes the result under L3 with its admission transition.
    fn write_publication_locked(&self, failure: &mut Option<PublicationFailure>,
        facts: Option<(u64, Option<Publication>)>, retryable: bool,
        change: impl FnOnce(&mut Map<String, Value>, &mut ProviderDocument) -> Result<(), StorageError>,
    ) -> Result<ProviderDocument, StorageError> {
        if let Some(failure) = failure.as_ref() { return Err(failure.error.clone()); }
        let mut pending = None;
        let mut entered = false;
        let result = sidecar::update_document(&self.path, |raw| {
            entered = true;
            let mut doc = decode(Some(raw))?.ok_or_else(invalid)?;
            let before = raw.clone();
            change(raw, &mut doc)?;
            put(raw, &doc)?;
            pending = Some(PendingPublication { before, desired: raw.clone(), document: doc.clone(), facts, retryable, cancel: None });
            Ok(doc)
        });
        if (pending.is_some() || !entered) && let Err(error) = &result {
            *failure = Some(PublicationFailure { error: error.clone(), pending });
        }
        result
    }
    /// Runtime's local tracking transitions share qualification/setup ownership.
    /// The closure performs no provider I/O and releases State/tracking on return.
    pub(crate) fn with_publication_ownership<T>(&self, change: impl FnOnce() -> T) -> T {
        let _failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        change()
    }
    /// Cancelling remains durable until both this exact mapping restoration and
    /// the final tracking transaction finish. Neither step admits writers or
    /// retires an original completion. Only Runtime can retry with the same op
    /// and actual retained rows; the general settings retry cannot do so.
    pub(crate) fn finish_publication_cancellation<E: From<StorageError>>(&self,
        operation: &crate::cloud::publication::PublishOp, replacement: &Publication, retained: Option<(Option<&Binding>, &Binding)>,
        current: impl FnOnce() -> Result<(), E>, finish: impl FnOnce(bool) -> Result<(), E>,
    ) -> Result<bool, E> {
        let mut failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        self.close_admission();
        current()?;
        #[cfg(feature = "test-hooks")]
        {
            let hook = self.cancellation_hook.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(hook) = hook { hook(); }
        }
        let restored = if let Some((before, after)) = retained {
            self.restore_cancelled_binding_locked(&mut failure, &CancelRestoration {
                operation: operation.clone(), before: before.cloned(), after: after.clone(),
                replacement: replacement.clone(),
            })?
        } else {
            if let Some(failed) = failure.as_ref() { return Err(failed.error.clone().into()); }
            let doc = sidecar::read_document(&self.path, decode)?.ok_or_else(invalid)?;
            // Even when no predecessor can be restored, a public handoff can
            // retain this candidate. Its evidence must survive cancellation.
            if doc.handoff.is_some() { return Err(invalid().into()); }
            false
        };
        // Keep setup and qualifiers excluded through removal of the op. There
        // is no provider I/O while this closure holds State/tracking.
        finish(restored)?;
        if restored {
            let _state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            self.open.store(true, Ordering::SeqCst);
        }
        Ok(restored)
    }
    /// Pauses with publication ownership only, after the actual current check.
    #[cfg(feature = "test-hooks")]
    pub fn set_cancellation_hook_for_testing(&self, hook: impl FnOnce() + Send + 'static) {
        *self.cancellation_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }
    fn restore_cancelled_binding_locked(&self, failure: &mut Option<PublicationFailure>,
        cancel: &CancelRestoration,
    ) -> Result<bool, StorageError> {
        if let Some(failed) = failure.as_ref() {
            let error = failed.error.clone();
            let pending = failed.pending.as_ref().filter(|pending| pending.cancel.as_ref() == Some(cancel))
                .ok_or_else(|| error.clone())?;
            let result = sidecar::update_document(&self.path, |raw| {
                if raw != &pending.before && raw != &pending.desired { return Err(invalid()); }
                decode(Some(raw))?.ok_or_else(invalid)?;
                *raw = pending.desired.clone();
                Ok(pending.document.clone())
            });
            return match result {
                Ok(doc) => {
                    self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Ok(doc);
                    *failure = None;
                    Ok(true)
                }
                Err(_) => {
                    self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Err(error.clone());
                    Err(error)
                }
            };
        }
        let mut pending = None;
        let result = (|| {
            let before = sidecar::read_document(&self.path, |raw| Ok(raw.cloned()))?.ok_or_else(invalid)?;
            let mut doc = decode(Some(&before))?.ok_or_else(invalid)?;
            if cancel.before.is_none() && cancel.operation.binding_before.is_none()
                && doc.selection == Selection::ICloud && doc.handoff.is_none()
                && doc.binding.is_none() && doc.legacy_binding_pending {
                // No predecessor or provider association exists to restore.
                // Preserve pending consent and every raw byte: this validated
                // read performs no mutation requiring durable re-publication.
                self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Ok(doc);
                return Ok(true);
            }
            let mapping = |binding: &Binding, publication: &Publication| -> Result<CloudBinding, StorageError> {
                use meguri_engine::device_state::AccountBinding;
                if publication.identity != binding.facts.publication || publication.decision != PublicationDecision::Clear
                    || !matches!(&publication.binding, AccountBinding::Connected { account, configuration, environment }
                        if account.as_bytes() == binding.facts.account.0.as_slice()
                            && configuration == binding.scope.container.as_str()
                            && environment == match binding.scope.environment {
                                crate::cloud::Environment::Development => "development",
                                crate::cloud::Environment::Production => "production",
                            }) { return Err(invalid()); }
                Ok(CloudBinding { binding: binding.id.0, account: binding.facts.account.0.clone(),
                    publication: publication.clone(), container: binding.scope.container.as_str().into(),
                    environment: binding.scope.environment })
            };
            let prior = &cancel.operation.fence_before.publication;
            let previous = match &cancel.before {
                // Keep Local Only left a stopped row, not Connected consent.
                // Its exact R6 authority is restored by Runtime; remove only
                // this operation's candidate association, never invent a prior
                // provider document or turn account stamps into permission.
                Some(binding) if prior.binding == meguri_engine::device_state::AccountBinding::LocalOnly
                    && prior.decision == PublicationDecision::Clear
                    && prior.identity == binding.facts.publication
                    && binding.facts.state == BindingState::Isolated(crate::cloud::IsolationReason::Stopped) => None,
                Some(binding) => Some(mapping(binding, prior)?),
                // No predecessor has no mapping to reconstruct. Runtime has
                // checked the exact before fence, file and candidate row.
                None => None,
            };
            let replacement = mapping(&cancel.after, &cancel.replacement)?;
            // Successor setup is not the cancelled operation's mapping. Keep
            // Cancelling rather than erase its target evidence and let an
            // ordinary qualifier consume that consent for the predecessor.
            if doc.selection == Selection::ICloud && doc.handoff.is_none() && doc.legacy_binding_pending {
                return Err(invalid());
            }
            if doc.selection != Selection::ICloud || doc.handoff.is_some() || doc.legacy_binding_pending
                || (doc.binding.as_ref() != Some(&replacement) && doc.binding != previous) {
                // A handoff or successor may still need the candidate and op.
                // Refuse without rewriting or poisoning that original authority.
                return Err(invalid());
            }
            doc.binding = previous;
            let mut desired = before.clone();
            put(&mut desired, &doc)?;
            pending = Some(PendingPublication { before: before.clone(), desired: desired.clone(), document: doc.clone(),
                facts: None, retryable: false, cancel: Some(cancel.clone()) });
            // Even an already-restored mapping is republished: after a crash,
            // visible rename bytes alone do not prove directory durability (F4).
            sidecar::update_document(&self.path, |raw| {
                if raw != &before { return Err(invalid()); }
                *raw = desired;
                Ok(())
            })?;
            self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Ok(doc);
            Ok(true)
        })();
        if pending.is_some() && let Err(error) = &result {
            self.state.lock().unwrap_or_else(PoisonError::into_inner).document = Err(error.clone());
            *failure = Some(PublicationFailure { error: error.clone(), pending });
        }
        result
    }
    /// Runtime supplies the real retained binding and settled R6 state. Credentials
    /// and stamps cannot call this qualification path.
    pub(crate) fn qualify_legacy<E: From<StorageError>>(&self, binding: &Binding, publication: &Publication,
        current: impl FnOnce() -> Result<bool, E>,
    ) -> Result<(), E> {
        let mut failure = self.publication_failure.lock().unwrap_or_else(PoisonError::into_inner);
        // Runtime checks its actual operation target under this serialization.
        // It releases State/tracking before returning, ahead of sidecar I/O.
        // Setup cannot publish successor consent between this check and write.
        if !current()? { return Ok(()); }
        self.qualify_legacy_locked(&mut failure, binding, publication)?;
        self.validate_current_binding(binding, publication);
        Ok(())
    }
    fn qualify_legacy_locked(&self, failure: &mut Option<PublicationFailure>, binding: &Binding,
        publication: &Publication,
    ) -> Result<(), StorageError> {
        if !self.snapshot()?.legacy_binding_pending { return Ok(()); }
        if publication.decision != PublicationDecision::Clear || publication.identity != binding.facts.publication
            || binding.facts.state != BindingState::Active { return Err(invalid()); }
        let account = match &publication.binding {
            meguri_engine::device_state::AccountBinding::Connected { account, configuration, environment }
                if account.as_bytes() == binding.facts.account.0.as_slice() && configuration == binding.scope.container.as_str()
                    && environment == match binding.scope.environment { crate::cloud::Environment::Development => "development", crate::cloud::Environment::Production => "production" }
                => account,
            _ => return Ok(()),
        };
        let retained = CloudBinding { binding: binding.id.0, account: account.as_bytes().to_vec(), publication: publication.clone(),
            container: binding.scope.container.as_str().into(), environment: binding.scope.environment };
        self.update_publication_locked(failure, None, false, |_, doc| {
            if doc.selection != Selection::ICloud || doc.handoff.is_some() || !doc.legacy_binding_pending { return Err(invalid()); }
            doc.binding = Some(retained); doc.legacy_binding_pending = false;
            Ok(())
        })?;
        Ok(())
    }
}

pub fn native_endpoint(text: &str) -> Result<Endpoint, StorageError> {
    let endpoint = Endpoint::parse(text).map_err(|_| invalid())?;
    if !endpoint.is_https() { return Err(invalid()); }
    Ok(endpoint)
}
pub fn canonical_username(text: &str) -> Result<String, StorageError> {
    let name = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if !(3..=64).contains(&name.len()) || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    { return Err(invalid()); }
    Ok(name.to_ascii_lowercase())
}

pub struct AuthenticatedConnection { pub connection: MeguriConnection, pub username: String, pub hostkey: String }
#[derive(Debug)]
pub enum AuthenticationFailure { ConnectionChanged, Username, Cancelled, Transport(crate::SyncError) }
impl From<crate::SyncError> for AuthenticationFailure { fn from(error: crate::SyncError) -> Self { Self::Transport(error) } }
/// Authentication only. The sole protocol method is HostKey, whose retained
/// transport refuses redirects; neither branch constructs a collection job.
#[allow(clippy::too_many_arguments)] // Keep authority, connection and execution/authentication ports explicit.
pub fn authenticate(
    authority: &Authority, connection: &MeguriConnection, threads: &meguri_engine::ThreadRegistry,
    random: &dyn meguri_model::rng::RandomSource, username: &str, password: &str,
    cancel: Arc<meguri_engine::CancelToken>, transport: Option<Box<dyn crate::SyncTransport>>,
) -> Result<AuthenticatedConnection, AuthenticationFailure> {
    authority.validate_connection(connection).map_err(|_| AuthenticationFailure::ConnectionChanged)?;
    let original_revision = authority.snapshot().map_err(|_| AuthenticationFailure::ConnectionChanged)?.revision;
    let endpoint = native_endpoint(&connection.endpoint).map_err(|_| AuthenticationFailure::ConnectionChanged)?;
    let username = canonical_username(username).map_err(|_| AuthenticationFailure::Username)?;
    let options = crate::JobOptions::new(endpoint, crate::ServerKind::SelfHosted).cancel(cancel);
    let key = if let Some(transport) = transport {
        let mut job = crate::Job::with_transport(&options, None, random, transport)?;
        crate::transport::runtime::block_on(threads, options.cancel_token(), crate::login(&mut job, &username, password))?
    } else { crate::login_blocking(threads, &options, random, &username, password)? };
    authority.validate_connection(connection).map_err(|_| AuthenticationFailure::ConnectionChanged)?;
    if authority.snapshot().map_err(|_| AuthenticationFailure::ConnectionChanged)?.revision != original_revision {
        return Err(AuthenticationFailure::ConnectionChanged);
    }
    options.cancel_token().check().map_err(|_| AuthenticationFailure::Cancelled)?;
    Ok(AuthenticatedConnection { connection: connection.clone(), username, hostkey: key.as_str().into() })
}
