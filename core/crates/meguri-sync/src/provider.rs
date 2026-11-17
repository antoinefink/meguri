//! Device-local service choice. Protocol jobs never infer admission from credentials.
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex, PoisonError, atomic::{AtomicBool, Ordering}}};
use meguri_engine::device_state::{Publication, PublicationDecision};
use meguri_storage::{StorageError, sidecar};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use crate::{Endpoint, cloud::{Binding, BindingState}};

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
    fn matches(&self, binding: &Binding) -> bool {
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
impl HandoffIntent { pub fn removal_pending(&self) -> bool { self.remove_credentials } }
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
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeguriConnection { pub reference: String, pub revision: u64, pub endpoint: String }
impl ProviderDocument {
    pub fn migration(settings: LegacySettings) -> Self {
        Self { version: 1, revision: 1, selection: if settings.enabled { Selection::ICloud } else { Selection::LocalOnly },
            settings, binding: None, handoff: None, legacy_binding_pending: settings.enabled, meguri: None,
            icloud_settings: settings, meguri_settings: LegacySettings { enabled: false, paused: false, automatic: false, media: true } }
    }
    pub fn permits_icloud(&self) -> bool {
        self.selection == Selection::ICloud && self.settings.enabled && !self.settings.paused
            && self.binding.is_some() && self.handoff.is_none()
    }
    /// MP-B/MP-C own durable collection-and-media completion. No ready setter exists here.
    pub fn permits_meguri_transfer(&self) -> bool { false }
    fn validate(&self) -> Result<(), StorageError> {
        if self.version != 1 || self.revision == 0
            || (self.selection == Selection::Meguri && self.settings.automatic)
            || self.meguri_settings.automatic || !self.meguri_settings.media
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
/// sidecar operations happen before taking Runtime/tracking ownership.
pub struct Authority {
    path: PathBuf, state: Mutex<Result<ProviderDocument, StorageError>>,
    open: AtomicBool,
    writers: AtomicBool,
    discovery: Mutex<(u64, usize)>,
    original_completion: AtomicBool,
    completion: Mutex<Option<CloudBinding>>,
}
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
        Arc::new(Self { path, state: Mutex::new(state), open: AtomicBool::new(true), writers: AtomicBool::new(false), discovery: Mutex::new((0, 0)),
            original_completion: AtomicBool::new(false), completion: Mutex::new(None) })
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
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
    pub fn close_admission(&self) { self.open.store(false, Ordering::SeqCst); self.writers.store(false, Ordering::SeqCst); }
    pub fn network_blocked(&self) -> bool { !self.open.load(Ordering::SeqCst) }
    pub fn reload(&self) -> Result<(), StorageError> {
        self.close_admission();
        let value = sidecar::read_document(&self.path, decode).and_then(|doc| doc.ok_or_else(invalid));
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = value.clone();
        value?;
        self.open.store(true, Ordering::SeqCst);
        Ok(())
    }
    pub(crate) fn validate_current_binding(&self, binding: &Binding, publication: &Publication) {
        let valid = self.snapshot().is_ok_and(|doc| doc.handoff.is_none()
            && doc.binding.as_ref().is_some_and(|stored| stored.matches(binding) && &stored.publication == publication)
            && publication.decision == PublicationDecision::Clear);
        self.writers.store(valid, Ordering::SeqCst);
        if valid {
            *self.completion.lock().unwrap_or_else(PoisonError::into_inner) = self.snapshot().ok().and_then(|doc| doc.binding);
        }
    }
    pub fn permits_icloud_binding(&self, binding: &Binding) -> bool {
        self.open.load(Ordering::SeqCst) && self.writers.load(Ordering::SeqCst) && self.snapshot().is_ok_and(|doc|
            doc.permits_icloud() && doc.binding.as_ref().is_some_and(|b| b.matches(binding)))
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
        self.update(|doc| {
            if doc.handoff.is_some() || (doc.selection == Selection::Meguri && (settings.automatic || !settings.media)) { return Err(invalid()); }
            // Enable never chooses a service or manufactures a binding.
            doc.settings = settings;
            if doc.selection == Selection::Meguri { doc.meguri_settings = settings; }
            else { doc.icloud_settings = settings; }
            Ok(())
        })
    }
    pub fn configure_meguri(&self, endpoint: &str, reference: String) -> Result<MeguriConnection, StorageError> {
        let endpoint = native_endpoint(endpoint)?.as_str().to_owned();
        let doc = self.update(|doc| {
            if doc.handoff.is_some() { return Err(invalid()); }
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
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
        let doc = self.update(|doc| {
            if doc.handoff.is_some() { return Err(invalid()); }
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            doc.handoff = Some(HandoffIntent { old: doc.selection, destination, binding: doc.binding.clone(),
                publication, generation, revision: doc.revision, remove_credentials });
            Ok(())
        })?;
        self.close_admission();
        Ok(Handoff { owner: self.clone(), intent: doc.handoff.ok_or_else(invalid)? })
    }
    /// Called only after the production owner joined its original native work.
    /// No serialized drain flag can produce this original intent handle.
    pub fn commit_handoff(&self, handoff: &Handoff) -> Result<ProviderDocument, StorageError> {
        if !std::ptr::eq(self, Arc::as_ptr(&handoff.owner)) { return Err(invalid()); }
        let result = self.update(|doc| {
            if doc.handoff.as_ref() != Some(&handoff.intent) { return Err(invalid()); }
            doc.selection = handoff.intent.destination;
            doc.binding = None; doc.legacy_binding_pending = false;
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
    pub(crate) fn accept_icloud_setup(&self) -> Result<(), StorageError> {
        self.update(|doc| {
            if doc.handoff.is_some() || doc.selection == Selection::Meguri { return Err(invalid()); }
            doc.selection = Selection::ICloud; doc.settings.enabled = true;
            doc.legacy_binding_pending = true;
            Ok(())
        })?;
        Ok(())
    }
    fn update(&self, change: impl FnOnce(&mut ProviderDocument) -> Result<(), StorageError>) -> Result<ProviderDocument, StorageError> {
        self.close_admission();
        let result = sidecar::update_document(&self.path, |raw| {
            let mut doc = decode(Some(raw))?.ok_or_else(invalid)?;
            change(&mut doc)?;
            put(raw, &doc)?;
            Ok(doc)
        });
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = result.clone();
        if result.is_ok() { self.open.store(true, Ordering::SeqCst); }
        result
    }
    /// Runtime supplies the real retained binding and settled R6 state. Credentials
    /// and stamps cannot call this qualification path.
    pub(crate) fn qualify_legacy(&self, binding: &Binding, publication: &Publication) -> Result<(), StorageError> {
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
        self.update(|doc| {
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
