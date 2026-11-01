//! Host-wired cloud capture and stable prebound workflows.
//!
//! Capture uses stored usability independently of live identity. The separate
//! Note-only reconcile API remains partial; whole completion and conditional
//! sending use the guarded workflow API. No transport is implicitly started.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use meguri_engine::{
    ChangeObserver, Collection, EngineError, Generation, GenerationListener, ObservedCommit, ObserverRows,
    OpenEvidence, OpenStep, Seq, SuspendParticipant, WalCoverage, WalReceipt,
};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, Table};
use serde::{Deserialize, Serialize};

use super::{entity_codec as codec, *};
#[path = "adapter_runtime.rs"]
mod adapter_impl;
#[path = "adoption_runtime.rs"]
mod adoption_impl;
#[path = "asset_read_runtime.rs"]
pub(super) mod asset_read_impl;
pub use asset_read_impl::AssetReadActivityGuard;
#[path = "conflict_cleanup_runtime.rs"]
mod conflict_cleanup_impl;
#[path = "incoming_runtime.rs"]
mod incoming_impl;
#[path = "media_runtime.rs"]
mod media_impl;
#[path = "policy_runtime.rs"]
mod policy_impl;
#[path = "publication_runtime.rs"]
mod publication_impl;
#[path = "workflow_runtime.rs"]
mod workflow_impl;
#[path = "zone_deletion_runtime.rs"]
mod zone_deletion_impl;
pub use super::publication::{KeyByteSource, KeySourceError};

// Serialize probes, writes and connection drops across runtime owners.
// Foundational store callers must also enter this gate.
static TRACKING_ACCESS: Mutex<()> = Mutex::new(());

/// Serialize foundational store access sharing a directory with a runtime.
/// The closure must not invoke Runtime methods, providers or callbacks.
pub fn with_tracking_access<T>(work: impl FnOnce() -> T) -> T {
    let _access = TRACKING_ACCESS.lock().unwrap_or_else(PoisonError::into_inner);
    work()
}

/// V8.2 requires a creation instant alongside the device and inode.
/// Providers inspect the file at the supplied path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeFileIdentity {
    pub device: u64,
    pub inode: u64,
    pub created_seconds: i64,
    pub created_nanos: u32,
}
impl NativeFileIdentity {
    fn pair(self) -> (u64, u64) {
        (self.device, self.inode)
    }
}

pub trait FileIdentityProvider: Send + Sync {
    fn identity(&self, path: &Path) -> Result<NativeFileIdentity, RuntimeError>;
}

/// Platform account input. Unavailable never invents an account identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityStatus {
    Available(OpaqueBytes),
    ReadFailed,
    TemporarilyUnavailable,
    Restricted,
    Configuration,
    SignedOut,
    SwitchedAccounts,
}
pub trait IdentityProvider: Send + Sync {
    fn read_identity(&self, scope: &BuildScope) -> IdentityStatus;
}

#[derive(Clone)]
pub struct CloudInputs {
    pub scope: BuildScope,
    pub identity: Arc<dyn IdentityProvider>,
    pub files: Arc<dyn FileIdentityProvider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incomplete {
    WholeBindingReconciliation,
    NewOrRemovedNote,
    StructuralLayout,
    RootConfirmationAndSending,
}
#[derive(Debug)]
pub enum RuntimeError {
    Tracking(Error),
    Engine(EngineError),
    StaleIncarnation,
    IdentityUnverified,
    Incomplete(Incomplete),
    FileIdentityUnavailable,
    Publication(super::publication::PublicationError),
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Tracking(_) => "cloud runtime tracking failed",
            Self::Engine(_) => "cloud runtime collection read failed",
            Self::StaleIncarnation => "cloud runtime incarnation is stale",
            Self::IdentityUnverified => "cloud runtime account is unverified",
            Self::Incomplete(_) => "cloud runtime component is incomplete",
            Self::FileIdentityUnavailable => "cloud runtime file identity is unavailable",
            Self::Publication(_) => "cloud publication could not proceed",
        })
    }
}
impl std::error::Error for RuntimeError {}
impl From<Error> for RuntimeError {
    fn from(value: Error) -> Self {
        Self::Tracking(value)
    }
}
impl From<EngineError> for RuntimeError {
    fn from(value: EngineError) -> Self {
        Self::Engine(value)
    }
}
impl From<meguri_storage::StorageError> for RuntimeError {
    fn from(value: meguri_storage::StorageError) -> Self {
        Self::Engine(value.into())
    }
}
fn engine_error(_: impl std::fmt::Display) -> EngineError {
    EngineError::CollectionUnavailable {
        reason: "cloud tracking capture failed; reconciliation required".into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirtySource {
    Observer,
    Snapshot,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyMark {
    pub source: DirtySource,
    pub change_seq: u64,
    pub payload_digest: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteAcknowledgement {
    pub erev: u64,
    pub system_fields: OpaqueBytes,
    pub payload: OpaqueBytes,
    pub row_digest: [u8; 32],
    pub payload_digest: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteTracking {
    pub id: i64,
    pub name: String,
    pub origin: [u8; 16],
    pub local_rev: u64,
    pub last_observed_digest: Option<[u8; 32]>,
    pub local_payload: OpaqueBytes,
    pub base: Option<OpaqueBytes>,
    pub mark: Option<DirtyMark>,
    pub acknowledged: NoteAcknowledgement,
    pub observer_open: u64,
    pub observed_seq: u64,
    pub observed_generation: u64,
    #[serde(default)]
    pub acked_rev: u64,
    #[serde(default)]
    pub in_flight: Vec<super::workflow::Flight>,
    #[serde(default)]
    pub acked_envelope: OpaqueBytes,
    #[serde(default)]
    pub tombstone_at: Option<i64>,
}
impl NoteTracking {
    pub(super) fn layout_generation(&self) -> Result<u64, Error> {
        let codec::Payload::Note(image) = codec::Payload::decode(
            codec::Kind::Note,
            if self.acknowledged.erev == 0 {
                &self.local_payload.0
            } else {
                &self.acknowledged.payload.0
            },
        )
        .map_err(|_| Error::InvalidData)?
        else {
            return Err(Error::InvalidData);
        };
        Ok(image.nt_struct_gen)
    }
    pub(super) fn require_supported_update(
        &self,
        before: Option<&codec::Note>,
        after: Option<&codec::Note>,
    ) -> Result<(), Error> {
        let codec::Payload::Note(acked) = codec::Payload::decode(
            codec::Kind::Note,
            if self.acknowledged.erev == 0 {
                &self.local_payload.0
            } else {
                &self.acknowledged.payload.0
            },
        )
        .map_err(|_| Error::InvalidData)?
        else {
            return Err(Error::InvalidData);
        };
        let after = after.ok_or(Error::InvalidData)?;
        if after.id != self.id
            || after.mid != acked.mid
            || after.nt_struct_gen != acked.nt_struct_gen
            || before.is_some_and(|image| image.id != self.id || image.mid != acked.mid)
        {
            return Err(Error::InvalidData);
        }
        Ok(())
    }
    pub(super) fn validate(&self) -> Result<(), Error> {
        use super::runtime_store::checked;
        for value in [
            self.local_rev,
            self.observer_open,
            self.observed_seq,
            self.observed_generation,
            self.acknowledged.erev,
            self.layout_generation()?,
        ] {
            checked(value)?;
        }
        let acked = codec::Payload::decode(
            codec::Kind::Note,
            if self.acknowledged.erev == 0 {
                &self.local_payload.0
            } else {
                &self.acknowledged.payload.0
            },
        )
        .map_err(|_| Error::InvalidData)?;
        let local = codec::Payload::decode(codec::Kind::Note, &self.local_payload.0).map_err(|_| Error::InvalidData)?;
        let valid_ack = if self.acknowledged.erev == 0 {
            self.acked_rev == 0
                && self.acknowledged.payload.0.is_empty()
                && self.acknowledged.system_fields.0.is_empty()
                && self.acknowledged.row_digest == [0; 32]
                && self.acknowledged.payload_digest == [0; 32]
        } else {
            !self.acknowledged.system_fields.0.is_empty()
                && (matches!(codec::Entity::decode(&self.acked_envelope.0),
                    Ok(codec::DecodeOutcome::Supported(e)) if e.fields.tombstone)
                    || (acked.row_digest().map_err(|_| Error::InvalidData)? == self.acknowledged.row_digest
                        && acked.payload_digest().map_err(|_| Error::InvalidData)? == self.acknowledged.payload_digest))
        };
        if !valid_ack
            || self.layout_generation()? == 0
            || Some(local.row_digest().map_err(|_| Error::InvalidData)?) != self.last_observed_digest
        {
            return Err(Error::InvalidData);
        }
        if !matches!(&acked, codec::Payload::Note(image) if image.id == self.id) {
            return Err(Error::InvalidData);
        }
        if let codec::Payload::Note(local) = &local {
            self.require_supported_update(None, Some(local))?;
        }
        if self.tombstone_at.is_some()
            || matches!(codec::Entity::decode(&self.acked_envelope.0),
            Ok(codec::DecodeOutcome::Supported(e)) if e.fields.tombstone)
        {
            super::workflow_store::note_facts(self.clone())?.validate()?;
        }
        if let Some(base) = &self.base {
            codec::Payload::decode(codec::Kind::Note, &base.0).map_err(|_| Error::InvalidData)?;
        }
        if let Some(mark) = &self.mark {
            checked(mark.change_seq)?;
            if mark.change_seq == 0 || mark.payload_digest != local.payload_digest().map_err(|_| Error::InvalidData)? {
                return Err(Error::InvalidData);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteScanCursor {
    pub seq: u64,
    pub generation: u64,
    pub covered_through: u64,
    pub native_file: NativeFileIdentity,
    pub modified: i64,
    pub schema_modified: i64,
}

/// Durable factual readout. A Note cursor never implies a whole-binding scan.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRuntimeState {
    pub change_counter: u64,
    pub open_counter: u64,
    pub drained: bool,
    pub file: Option<NativeFileIdentity>,
    pub note_scan: Option<NoteScanCursor>,
    #[serde(default)]
    pub whole_scan: bool,
    #[serde(default)]
    pub coverage: Option<super::workflow::CoverageLedger>,
    #[serde(default)]
    pub root_ack: Option<super::workflow::RootAcknowledgement>,
    #[serde(default)]
    pub close_metadata: Option<(i64, i64)>,
}
impl StoredRuntimeState {
    pub(super) fn validate(&self) -> Result<(), Error> {
        for value in [self.change_counter, self.open_counter] {
            super::runtime_store::checked(value)?;
        }
        if let Some(cursor) = &self.note_scan {
            for value in [cursor.seq, cursor.generation, cursor.covered_through] {
                super::runtime_store::checked(value)?;
            }
            if cursor.covered_through < cursor.seq || Some(cursor.native_file) != self.file {
                return Err(Error::InvalidData);
            }
        }
        if self.file.is_some_and(|file| file.created_nanos >= 1_000_000_000) {
            return Err(Error::InvalidData);
        }
        if let Some(coverage) = &self.coverage {
            coverage.validate()?;
        }
        // Note enumeration cannot establish whole-binding drain.
        if self.drained && (!self.whole_scan || self.coverage.is_none()) {
            return Err(Error::InvalidData);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CaptureProof {
    pub observed_seq: Seq,
    pub generation: Generation,
    pub attempted: bool,
    pub persisted: bool,
}
#[derive(Clone, Debug)]
pub struct NoteScanProof {
    pub snapshot_seq: Seq,
    pub snapshot_generation: Generation,
    pub snapshot_native_file: NativeFileIdentity,
    pub receipt: WalReceipt,
    pub recovered_names: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct RuntimeStatus {
    pub binding: Option<Binding>,
    pub generation: Option<Generation>,
    pub stored_usable: bool,
    pub identity_verified: bool,
    pub scan_required: bool,
    pub scan_running: bool,
    pub scan_completed_this_open: bool,
    pub note_scan_completed_this_open: bool,
    pub tracking_stopped: bool,
    pub scan_drained: bool,
    pub change_counter: u64,
    pub incomplete: Incomplete,
}

struct Bound {
    binding: Binding,
    evidence: OpenEvidence,
    file: NativeFileIdentity,
    open_counter: u64,
    usable: bool,
    verified: bool,
    latest_mod: i64,
    latest_scm: i64,
}
#[derive(Default)]
struct State {
    incarnation: u64,
    invalidation: u64,
    invalidation_exhausted: bool,
    store: Option<TrackingStore>,
    bound: Option<Bound>,
    scan_required: bool,
    scan_running: bool,
    tracking_stopped: bool,
    last_capture: Option<CaptureProof>,
    note_scan: Option<NoteScanProof>,
    whole_complete: bool,
    root_confirmed: bool,
    actions: super::workflow::Actions,
    last_session: Option<u64>,
    cloud: super::workflow::CloudFrontier,
    root_fetch_qualified: bool,
    root_reads: std::collections::BTreeSet<u64>,
    trace: Vec<super::workflow::TraceEvent>,
    adapter: super::adapter::AdapterState,
    policy: super::policy::RuntimeEvidenceState,
    publication: super::publication::ProcessState,
    // A successful zone removal supersedes all earlier inventory observations.
    zone_listing_fence: Arc<()>,
    incoming: super::incoming::ProcessState,
    media: super::media::Process,
    #[cfg(feature = "test-hooks")]
    drop_capture: Option<i64>,
    #[cfg(feature = "test-hooks")]
    last_capture_body_reads: usize,
    #[cfg(feature = "test-hooks")]
    fail_capture_after: Option<usize>,
}

#[cfg(feature = "test-hooks")]
type CloudCaptureHook = Box<dyn FnOnce(&Runtime) + Send>;

#[derive(Clone)]
struct NoCollectionIdentityOwner {
    live: std::sync::Weak<Mutex<bool>>,
    engine: meguri_engine::WeakEngine,
    generation: Generation,
    incarnation: u64,
}

/// Host-owned ports accept a collection handle for each read call.
pub struct Runtime {
    directory: PathBuf,
    inputs: CloudInputs,
    adapter_clock: Mutex<Option<Arc<dyn meguri_model::clock::Clock>>>,
    no_collection_identity_owner: Mutex<Option<NoCollectionIdentityOwner>>,
    state: Arc<Mutex<State>>,
    incoming_idle: Arc<std::sync::Condvar>,
    #[cfg(feature = "test-hooks")]
    incoming_hooks: Arc<super::testing::Hooks>,
    random: Mutex<Option<Arc<dyn meguri_model::rng::RandomSource>>>,
    key_bytes: Mutex<Option<Arc<dyn KeyByteSource>>>,
    adoption: Mutex<Option<Arc<adoption_impl::AdoptionOwner>>>,
    review: Mutex<std::sync::Weak<meguri_engine::session::ReviewService>>,
    #[cfg(feature = "test-hooks")]
    scan_pause: Mutex<Option<ScanPause>>,
    #[cfg(feature = "test-hooks")]
    current_account_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(feature = "test-hooks")]
    cloud_capture_hook: Mutex<Option<CloudCaptureHook>>,
}
#[derive(Clone, PartialEq, Eq)]
struct ScanGuard {
    invalidation: u64,
    binding: BindingId,
    open: u64,
    generation: Generation,
    file: NativeFileIdentity,
    incarnation: u64,
}

/// Original authority for one actual database inventory request. Not a set of
/// names or a serializable grant; completion consumes it, including on failure.
pub struct ZoneListingRequest {
    owner: std::sync::Weak<Mutex<State>>,
    collection: Collection,
    scan: ScanGuard,
    adapter: super::adapter::Guard,
    instance: String,
    account: OpaqueBytes,
    decision: super::publication::DecisionSnapshot,
    deletion_fence: Arc<()>,
}
#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanPausePoint {
    BeforeSnapshot,
    BeforeMerge,
}
#[cfg(feature = "test-hooks")]
struct ScanPause {
    point: ScanPausePoint,
    reached: std::sync::mpsc::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}

impl Runtime {
    /// Construction performs no account/provider call. Tracking opens under
    /// its serialization gate in the pre-admission OpenStep.
    pub fn new(directory: PathBuf, inputs: CloudInputs) -> Self {
        Self {
            directory,
            inputs,
            adapter_clock: Mutex::new(None),
            no_collection_identity_owner: Mutex::new(None),
            random: Mutex::new(None),
            key_bytes: Mutex::new(None),
            adoption: Mutex::new(None),
            review: Mutex::new(std::sync::Weak::new()),
            incoming_idle: Arc::new(std::sync::Condvar::new()),
            #[cfg(feature = "test-hooks")]
            incoming_hooks: Arc::new(super::testing::Hooks::default()),
            state: Arc::new(Mutex::new(State {
                scan_required: true,
                actions: super::workflow::Actions::new(),
                ..State::default()
            })),
            #[cfg(feature = "test-hooks")]
            scan_pause: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            current_account_hook: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            cloud_capture_hook: Mutex::new(None),
        }
    }
    /// Change external test inputs immediately before the fresh account read.
    #[cfg(feature = "test-hooks")]
    pub fn set_current_account_hook_for_test(&self, hook: impl FnOnce() + Send + 'static) {
        *self.current_account_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }
    #[cfg(feature = "test-hooks")]
    fn take_current_account_hook_for_test(&self) -> Option<Box<dyn FnOnce() + Send>> {
        self.current_account_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
    /// One-shot observation after writer capture, before fallible registration.
    /// The hook mutex is released before invocation; the pinned reader lease
    /// still excludes replacement. Do not reenter the Cloud reader.
    #[cfg(feature = "test-hooks")]
    pub fn set_cloud_capture_hook_for_test(&self, hook: impl FnOnce(&Runtime) + Send + 'static) {
        *self.cloud_capture_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }
    #[cfg(feature = "test-hooks")]
    fn take_cloud_capture_hook_for_test(&self) -> Option<CloudCaptureHook> {
        self.cloud_capture_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn invalidate_locked(state: &mut State) {
        match state.invalidation.checked_add(1) {
            Some(next) => state.invalidation = next,
            None => state.invalidation_exhausted = true,
        }
        state.scan_required = true;
        state.whole_complete = false;
        state.tracking_stopped = true;
        if let Some(id) = state.bound.as_ref().map(|b| b.binding.id) {
            let _ = with_tracking_access(|| {
                state
                    .store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .set_workflow_drained(id, false, (0, 0))
            });
        }
    }
    fn bind(&self, collection: &Collection) -> Result<(), RuntimeError> {
        // Providers and sidecar RMW locks never run under tracking/state locks.
        let evidence = collection.open_evidence()?;
        let file = self.inputs.files.identity(collection.path())?;
        if file.pair() != evidence.native_file || file.created_nanos >= 1_000_000_000 {
            return Err(RuntimeError::FileIdentityUnavailable);
        }
        let device = meguri_engine::device_state::DeviceState::for_collection(collection.path());
        let publication = device.settled_publication()?;
        let activation = collection.cloud_adopt_activation()?;
        let live_adoption = self.adoption.lock().unwrap_or_else(PoisonError::into_inner).is_some();
        let mut state = self.state();
        Self::adapter_close_exchange_locked(&mut state);
        state.incarnation = state.incarnation.checked_add(1).ok_or(Error::InvalidData)?;
        if state.cloud.generation != Some(evidence.generation) || state.cloud.native_file != Some(file) {
            state.cloud = super::workflow::CloudFrontier {
                generation: Some(evidence.generation),
                native_file: Some(file),
                ..Default::default()
            };
            state.last_session = None;
        }
        let retained_sdk = state.adapter.sdk.is_some();
        let flights = std::mem::take(&mut state.policy.flights);
        state.policy = super::policy::RuntimeEvidenceState::default();
        if retained_sdk {
            state.policy.flights = flights;
        }
        state.cloud.flagged.clear();
        state.cloud.acked_images.clear();
        if !retained_sdk {
            state.cloud.handed_images.clear();
        }
        state.bound = None;
        state.scan_required = true;
        state.scan_running = false;
        state.tracking_stopped = state.invalidation_exhausted;
        state.note_scan = None;
        state.last_capture = None;
        state.whole_complete = false;
        state.root_confirmed = false;
        state.root_fetch_qualified = false;
        state.root_reads.clear();
        state.publication.running = None;
        state.publication.zones_created = false;
        state.publication.own_epoch_read = false;
        state.actions.rebind(retained_sdk);
        state.incoming.failed_this_open.clear();
        state.incoming.retry_granted = false;
        with_tracking_access(|| -> Result<(), RuntimeError> {
            if state.store.is_none() {
                let outcome = TrackingStore::open(&self.directory, self.inputs.scope.clone())?;
                state.store = Some(match outcome {
                    OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
                    OpenOutcome::Damaged => return Err(Error::RecoveryRequired.into()),
                    OpenOutcome::NewerSchema { .. } => return Err(Error::RuntimeSchemaUnsupported.into()),
                });
            }
            let store = state.store.as_mut().ok_or(Error::Database)?;
            store.install_runtime()?;
            store.adapter_install()?;
            if !live_adoption {
                let retained_adoption = store.adoption_op()?;
                store.recover_adoption(publication.as_ref(), activation.as_ref(), Some(file))?;
                if store.adoption_op()?.is_none()
                    && let Some(op) = retained_adoption
                {
                    adoption_impl::remove_adoption_stage(&self.directory, &op)?;
                }
            }
            let publication_op = store.publication_op()?;
            let restarted_confirmed = publication_op
                .as_ref()
                .is_some_and(|op| op.step == super::publication::PublishStep::Confirmed);
            let selected = if let Some(op) = publication_op.as_ref()
                && let Some(id) = op.binding_after
            {
                let binding = store.binding(id)?.ok_or(Error::BindingNotFound)?;
                if restarted_confirmed
                    || op.binding_before.as_ref().is_some_and(|before| before.id == id)
                    || binding.scope != self.inputs.scope
                    || binding.facts.account != op.account
                    || binding.facts.epoch != op.epoch
                    || binding.facts.record_key != op.record_key
                    || binding.facts.publication != op.confirmed_publication
                {
                    return Err(Error::InvalidData.into());
                }
                Some(binding)
            } else {
                store.selected_binding()?
            };
            let Some(mut binding) = selected else {
                state.publication.restarted_confirmed = restarted_confirmed;
                return Ok(());
            };
            let previous = store.runtime_binding(binding.id)?;
            // Every active open clears drain before account verification.
            let durable = if binding.facts.state == BindingState::Active {
                store.open_runtime_binding(binding.id, file)?
            } else {
                previous.clone()
            };
            let publication_matches = publication
                .as_ref()
                .is_some_and(|p| p.identity == binding.facts.publication);
            let publication_clear = publication
                .as_ref()
                .is_some_and(|p| p.decision == meguri_engine::device_state::PublicationDecision::Clear);
            if binding.facts.state == BindingState::Active {
                let reason = if !publication_matches {
                    Some(IsolationReason::ReplacedLocally)
                } else if previous.file.is_some_and(|stored| stored != file) {
                    Some(IsolationReason::FileMismatch)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    store.isolate_runtime_binding(binding.id, reason)?;
                    binding = store.binding(binding.id)?.ok_or(Error::BindingNotFound)?;
                }
            }
            let decision_matches = publication.as_ref().is_some_and(|p| match &p.binding {
                meguri_engine::device_state::AccountBinding::LocalOnly => true,
                meguri_engine::device_state::AccountBinding::Connected {
                    account,
                    configuration,
                    environment,
                } => {
                    account.as_bytes() == binding.facts.account.0.as_slice()
                        && configuration == self.inputs.scope.container.as_str()
                        && environment
                            == match self.inputs.scope.environment {
                                Environment::Development => "development",
                                Environment::Production => "production",
                            }
                }
                meguri_engine::device_state::AccountBinding::Unknown => false,
            });
            let usable = binding.facts.state == BindingState::Active
                && publication_matches
                && publication_clear
                && decision_matches;
            let latest_mod = evidence.opened_mod;
            let latest_scm = evidence.opened_scm;
            state.publication.restarted_confirmed = restarted_confirmed;
            // This is a real binding input, emitted only after installation. A
            // confirmed row has no new binding and is never its publish step.
            state.trace.push(super::workflow::TraceEvent::PublicationBinding {
                binding: binding.id,
                publish_step: publication_op
                    .as_ref()
                    .and_then(|op| (op.step != super::publication::PublishStep::Confirmed).then_some(op.step)),
            });
            state.media = super::media::Process::default();
            state.bound = Some(Bound {
                binding,
                evidence,
                file,
                open_counter: durable.open_counter,
                usable,
                verified: false,
                latest_mod,
                latest_scm,
            });
            Ok(())
        })?;
        drop(state);
        self.recover_conflict_staging_at_open()
    }

    /// An actor/host calls this off the writer at an actual account read point.
    /// An old result cannot verify a replacement or resumed incarnation.
    pub fn refresh_identity(&self) -> Result<(), RuntimeError> {
        self.wait_incoming_idle();
        let guard = {
            let state = self.state();
            state.bound.as_ref().map(|b| {
                (
                    state.incarnation,
                    b.evidence.generation,
                    b.open_counter,
                    Self::adapter_guard_locked(&state),
                )
            })
        };
        let Some(guard) = guard else { return Ok(()) };
        let result = self.inputs.identity.read_identity(&self.inputs.scope);
        let verified_account = match &result {
            IdentityStatus::Available(a) => Some(a.clone()),
            _ => None,
        };
        let instance = self.instance().ok();
        let signed_out = matches!(&result, IdentityStatus::SignedOut);
        let mut state = self.state();
        if state.bound.as_ref().map(|b| {
            (
                state.incarnation,
                b.evidence.generation,
                b.open_counter,
                Self::adapter_guard_locked(&state),
            )
        }) != Some(guard)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let result = with_tracking_access(|| -> Result<(), RuntimeError> {
            let State { bound, store, .. } = &mut *state;
            let bound = bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            match result {
                IdentityStatus::Available(identity) if identity == bound.binding.facts.account => {
                    bound.verified = bound.usable;
                }
                IdentityStatus::Available(_) | IdentityStatus::SignedOut | IdentityStatus::SwitchedAccounts => {
                    // Close the gate before attempting the isolation commit.
                    bound.verified = false;
                    bound.usable = false;
                    let reason = if signed_out {
                        IsolationReason::SignedOut
                    } else {
                        IsolationReason::AccountChanged
                    };
                    let store = store.as_mut().ok_or(Error::Database)?;
                    store.isolate_runtime_binding(bound.binding.id, reason)?;
                    bound.binding = store.binding(bound.binding.id)?.ok_or(Error::BindingNotFound)?;
                }
                IdentityStatus::ReadFailed => {} // V9.1: a later failed read keeps verification.
                _ => bound.verified = false,
            }
            Ok(())
        });
        if result.is_ok()
            && let (Some(account), Some(instance)) = (verified_account, instance)
            && let Ok(account) = String::from_utf8(account.0)
        {
            state
                .trace
                .push(super::workflow::TraceEvent::IdentityVerified { instance, account });
        }
        if !state.bound.as_ref().is_some_and(|bound| bound.verified) {
            state.publication.running = None;
            state.publication.own_epoch_read = false;
        }
        if result.is_err() {
            state.tracking_stopped = true;
            state.scan_required = true;
        }
        result
    }

    pub fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        let state = self.state();
        let bound = state.bound.as_ref();
        let durable = with_tracking_access(|| -> Result<StoredRuntimeState, Error> {
            match (state.store.as_ref(), bound) {
                (Some(store), Some(bound)) => store.runtime_binding(bound.binding.id),
                _ => Ok(StoredRuntimeState::default()),
            }
        })?;
        Ok(RuntimeStatus {
            binding: bound.map(|b| b.binding.clone()),
            generation: bound.map(|b| b.evidence.generation),
            stored_usable: bound.is_some_and(|b| b.usable),
            identity_verified: bound.is_some_and(|b| b.verified),
            scan_required: state.scan_required,
            scan_running: state.scan_running,
            scan_completed_this_open: state.whole_complete,
            note_scan_completed_this_open: state.note_scan.is_some(),
            tracking_stopped: state.tracking_stopped,
            scan_drained: durable.drained,
            change_counter: durable.change_counter,
            incomplete: Incomplete::WholeBindingReconciliation,
        })
    }
    pub fn tracked_note(&self, id: i64) -> Result<Option<NoteTracking>, RuntimeError> {
        let state = self.state();
        with_tracking_access(|| -> Result<Option<NoteTracking>, RuntimeError> {
            let Some(bound) = &state.bound else { return Ok(None) };
            Ok(state
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .tracked_note(bound.binding.id, id)?)
        })
    }
    pub fn capture_proof(&self, seq: Seq, generation: Generation) -> Option<CaptureProof> {
        self.state()
            .last_capture
            .as_ref()
            .filter(|proof| proof.observed_seq == seq && proof.generation == generation)
            .cloned()
    }
    pub fn note_scan_proof(&self) -> Option<NoteScanProof> {
        self.state().note_scan.clone()
    }

    fn with_scan<T>(
        &self,
        guard: &ScanGuard,
        collection: &Collection,
        work: impl FnOnce(&mut State) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        // Platform and sidecar reads precede writer-shared guards.
        let file = self.inputs.files.identity(collection.path())?;
        let publication = meguri_engine::device_state::DeviceState::for_collection(collection.path()).publication()?;
        if file != guard.file || file.created_nanos >= 1_000_000_000 {
            return Err(RuntimeError::FileIdentityUnavailable);
        }
        let mut state = self.state();
        let bound = state.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if state.incarnation != guard.incarnation
            || (
                bound.binding.id,
                bound.open_counter,
                bound.evidence.generation,
                bound.file,
            ) != (guard.binding, guard.open, guard.generation, guard.file)
            || !bound.usable
            || !bound.verified
            || state.invalidation != guard.invalidation
            || state.invalidation_exhausted
            || !collection.is_current()
            || collection.generation() != guard.generation
            || !publication.as_ref().is_some_and(|p| {
                p.identity == bound.binding.facts.publication
                    && p.decision == meguri_engine::device_state::PublicationDecision::Clear
            })
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        with_tracking_access(|| work(&mut state))
    }

    /// Compare bounded pages through one held CloudReader snapshot and its
    /// matching WAL receipt. Collection reads and encoding hold no writer guards.
    pub fn reconcile_notes(&self, collection: &Collection, timeout: Duration) -> Result<NoteScanProof, RuntimeError> {
        // Own the test hook for this attempt, so every early return disconnects it.
        #[cfg(feature = "test-hooks")]
        let mut scan_pause = self.scan_pause.lock().unwrap_or_else(PoisonError::into_inner).take();
        let guard = {
            let mut state = self.state();
            let bound = state.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if !bound.usable || !bound.verified {
                return Err(RuntimeError::IdentityUnverified);
            }
            if state.invalidation_exhausted || state.scan_running {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            if bound.evidence.generation != collection.generation() {
                return Err(RuntimeError::StaleIncarnation);
            }
            let guard = ScanGuard {
                invalidation: state.invalidation,
                binding: bound.binding.id,
                open: bound.open_counter,
                generation: bound.evidence.generation,
                file: bound.file,
                incarnation: state.incarnation,
            };
            state.scan_running = true;
            state.note_scan = None;
            guard
        };
        let result = (|| -> Result<NoteScanProof, RuntimeError> {
            #[cfg(feature = "test-hooks")]
            Self::pause_scan(&mut scan_pause, ScanPausePoint::BeforeSnapshot)?;
            self.with_scan(&guard, collection, |_| Ok(()))?;
            let lease = collection.reader(ReaderRole::Cloud)?;
            let snapshot = lease.snapshot()?;
            let receipt = match collection.wait_wal_covered(snapshot.wal_identity(), snapshot.seq(), timeout)? {
                WalCoverage::Covered(receipt)
                    if receipt.identity() == snapshot.wal_identity()
                        && receipt.through() >= snapshot.seq()
                        && snapshot.generation() == guard.generation
                        && snapshot.wal_identity().native_file() == guard.file.pair() =>
                {
                    receipt
                }
                _ => return Err(RuntimeError::StaleIncarnation),
            };
            let col = snapshot.col()?;
            // Detect unsupported removals in bounded tracking pages, off the guards.
            let mut after = None;
            loop {
                let ids = self.with_scan(&guard, collection, |state| {
                    Ok(state
                        .store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .runtime_note_ids_page(guard.binding, after)?)
                })?;
                if ids.is_empty() {
                    break;
                }
                for id in &ids {
                    if snapshot.row_image(Table::Notes, &RowKey::Id(*id))?.is_none() {
                        return Err(RuntimeError::Incomplete(Incomplete::NewOrRemovedNote));
                    }
                }
                after = ids.last().copied();
            }
            let mut cursor = None;
            let mut recovered_names = Vec::new();
            loop {
                let page = snapshot.row_page(Table::Notes, cursor.as_ref())?;
                if page.is_empty() {
                    break;
                }
                let ids = page
                    .iter()
                    .map(|row| {
                        match RowKey::from_stored(Table::Notes, row).map_err(meguri_storage::StorageError::from)? {
                            RowKey::Id(id) => Ok(id),
                            _ => Err(RuntimeError::Tracking(Error::InvalidData)),
                        }
                    })
                    .collect::<Result<Vec<_>, RuntimeError>>()?;
                let contents = self.with_scan(&guard, collection, |state| {
                    Ok(state
                        .store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .runtime_note_contents(guard.binding, &ids)?)
                })?;
                let images = page
                    .into_iter()
                    .zip(contents)
                    .map(|(row, content)| {
                        let tracked: NoteTracking = super::store::decode(&content)?;
                        let image = note_image(&row, tracked.layout_generation()?)?;
                        tracked.require_supported_update(None, Some(&image))?;
                        super::runtime_store::PreparedNote::new(image)
                    })
                    .collect::<Result<Vec<_>, Error>>()?;
                #[cfg(feature = "test-hooks")]
                Self::pause_scan(&mut scan_pause, ScanPausePoint::BeforeMerge)?;
                recovered_names.extend(self.with_scan(&guard, collection, |state| {
                    Ok(state.store.as_mut().ok_or(Error::Database)?.merge_runtime_note_page(
                        guard.binding,
                        guard.open,
                        snapshot.seq(),
                        snapshot.generation(),
                        images,
                    )?)
                })?);
                cursor = ids.last().copied().map(RowKey::Id);
            }
            // An armed barrier that was never reached cannot certify a scan.
            #[cfg(feature = "test-hooks")]
            if scan_pause.is_some() {
                return Err(Error::InvalidData.into());
            }
            let proof = NoteScanProof {
                snapshot_seq: snapshot.seq(),
                snapshot_generation: snapshot.generation(),
                snapshot_native_file: guard.file,
                receipt,
                recovered_names,
            };
            self.with_scan(&guard, collection, |state| {
                state.store.as_mut().ok_or(Error::Database)?.finish_runtime_note_scan(
                    guard.binding,
                    guard.open,
                    NoteScanCursor {
                        seq: snapshot.seq().0,
                        generation: snapshot.generation().0,
                        covered_through: receipt.through().0,
                        native_file: guard.file,
                        modified: col.modified.0,
                        schema_modified: col.schema_modified.0,
                    },
                )?;
                state.note_scan = Some(proof.clone());
                Ok(())
            })?;
            // Whole-binding reconciliation remains required and undrained.
            Ok(proof)
        })();
        let mut state = self.state();
        if state.incarnation == guard.incarnation {
            state.scan_running = false;
            if result.is_err() {
                state.scan_required = true;
                state.tracking_stopped = true;
            }
        }
        result
    }

    #[cfg(feature = "test-hooks")]
    pub fn arm_note_scan_pause_for_testing(
        &self,
        point: ScanPausePoint,
        reached: std::sync::mpsc::Sender<()>,
        resume: std::sync::mpsc::Receiver<()>,
    ) -> Result<(), RuntimeError> {
        let mut pause = self.scan_pause.lock().unwrap_or_else(PoisonError::into_inner);
        if pause.is_some() {
            return Err(Error::InvalidData.into());
        }
        *pause = Some(ScanPause { point, reached, resume });
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    fn pause_scan(pause: &mut Option<ScanPause>, point: ScanPausePoint) -> Result<(), RuntimeError> {
        let pause = if pause.as_ref().is_some_and(|pause| pause.point == point) {
            pause.take()
        } else {
            None
        };
        if let Some(pause) = pause {
            pause.reached.send(()).map_err(|_| Error::InvalidData)?;
            pause
                .resume
                .recv_timeout(Duration::from_secs(60))
                .map_err(|_| Error::InvalidData)?;
        }
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub fn stored_state_for_testing(&self) -> Result<StoredRuntimeState, RuntimeError> {
        let state = self.state();
        let id = state.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| Ok(state.store.as_ref().ok_or(Error::Database)?.runtime_binding(id)?))
    }

    /// Read existing exchange identities without allocating or completing work.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn exchange_ids_for_testing(
        &self,
        owner: &str,
        generation: u64,
    ) -> Result<(Vec<u64>, Option<u64>), RuntimeError> {
        let state = self.state();
        if Self::instance_locked(&state)? != owner || Self::adapter_guard_locked(&state).generation != generation {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok((
            state.adapter.durable.exchanges.keys().copied().collect(),
            state.actions.exchange.as_ref().map(|(id, _)| *id),
        ))
    }

    /// Counts actual workflow entity body materializations on the last observer callback.
    #[cfg(feature = "test-hooks")]
    pub fn last_capture_body_reads_for_testing(&self) -> usize {
        self.state().last_capture_body_reads
    }

    /// Fail after a real tracking row write, before the transaction can commit.
    #[cfg(feature = "test-hooks")]
    pub fn arm_capture_failure_for_testing(&self, after_writes: usize) -> Result<(), RuntimeError> {
        let mut state = self.state();
        if after_writes == 0 || state.fail_capture_after.is_some() {
            return Err(Error::InvalidData.into());
        }
        state.fail_capture_after = Some(after_writes);
        Ok(())
    }

    /// Requires current whole coverage, account and root confirmation.
    /// Active reviews use the per-batch eligibility frontier.
    pub fn require_collection_sending(&self) -> Result<(), RuntimeError> {
        self.workflow_gate()
    }

    /// Drop one entire observer transaction before an edit. This changes only
    /// whether capture runs; it never changes an engine op or service result.
    #[cfg(feature = "test-hooks")]
    pub fn arm_capture_loss_for_testing(&self, note: i64) -> Result<(), RuntimeError> {
        let mut state = self.state();
        let bound = state.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !bound.usable {
            return Err(RuntimeError::StaleIncarnation);
        }
        if state.drop_capture.is_some() {
            return Err(Error::InvalidData.into());
        }
        state.drop_capture = Some(note);
        Ok(())
    }
}

impl ChangeObserver for Runtime {
    fn name(&self) -> &str {
        "cloud-note-capture"
    }
    fn observe(&self, _commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        Self::invalidate_locked(&mut self.state());
        Err(engine_error("callback row capability required"))
    }
    fn observe_with_rows(&self, commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Result<(), EngineError> {
        #[cfg(feature = "test-hooks")]
        let body_reads_before = super::workflow_store::body_reads();
        let mut state = self.state();
        let Some(bound) = state.bound.as_ref() else {
            return Ok(());
        };
        if !bound.usable || bound.evidence.generation != commit.generation {
            return Ok(());
        }
        let binding = bound.binding.id;
        let open = bound.open_counter;
        state.last_capture = Some(CaptureProof {
            observed_seq: commit.seq,
            generation: commit.generation,
            attempted: true,
            persisted: false,
        });
        // Whole-collection invalidation has no before-images. Other entity
        // kinds require their own capture/intent support before scan can finish.
        let changes = commit.changes;
        let generic = with_tracking_access(|| {
            state
                .store
                .as_ref()
                .is_some_and(|s| s.workflow_has_layouts(binding).unwrap_or(false))
        });
        let awaiting_initial_scan = with_tracking_access(|| {
            state
                .store
                .as_ref()
                .and_then(|store| store.publication_op().ok().flatten())
                .is_some_and(|op| {
                    op.binding_after == Some(binding)
                        && matches!(
                            op.step,
                            super::publication::PublishStep::Bound | super::publication::PublishStep::Zones
                        )
                })
        }) && !generic;
        if awaiting_initial_scan {
            // No baseline exists yet. The required held-snapshot scan captures
            // these committed rows before this binding can hand an entity out.
            Self::invalidate_locked(&mut state);
            state.tracking_stopped = state.invalidation_exhausted;
            return Ok(());
        }
        let unsupported = changes.whole_collection
            || !changes.cards.is_empty()
            || !changes.notetypes.is_empty()
            || !changes.decks.is_empty()
            || !changes.deck_configs.is_empty()
            || !changes.config_keys.is_empty()
            || !changes.tags.is_empty()
            || !changes.revlog_added.is_empty()
            || !changes.revlog_removed.is_empty()
            || !changes.media_added.is_empty()
            || !changes.media_removed.is_empty();
        if unsupported && !generic {
            Self::invalidate_locked(&mut state);
        }
        #[cfg(feature = "test-hooks")]
        if state
            .drop_capture
            .is_some_and(|id| commit.changes.notes.iter().any(|n| n.id.0 == id))
        {
            state.drop_capture = None;
            Self::invalidate_locked(&mut state);
            return Ok(());
        }
        #[cfg(feature = "test-hooks")]
        let fail_after = state.fail_capture_after.take();
        let result = with_tracking_access(|| -> Result<(), RuntimeError> {
            if generic {
                return state.store.as_mut().ok_or(Error::Database)?.capture_workflow(
                    binding,
                    open,
                    commit,
                    rows,
                    self.random
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_deref()
                        .ok_or(Error::InvalidData)?,
                    |_written| {
                        #[cfg(feature = "test-hooks")]
                        if fail_after == Some(_written) {
                            return Err(Error::Io(std::io::ErrorKind::Other));
                        }
                        Ok(())
                    },
                );
            }
            state.store.as_mut().ok_or(Error::Database)?.capture_runtime_notes(
                binding,
                open,
                commit,
                rows,
                |_written| {
                    #[cfg(feature = "test-hooks")]
                    if fail_after == Some(_written) {
                        return Err(Error::Io(std::io::ErrorKind::Other));
                    }
                    Ok(())
                },
            )
        });
        if result.is_err() {
            Self::invalidate_locked(&mut state);
        } else {
            if let Some(proof) = state.last_capture.as_mut() {
                proof.persisted = true;
            }
            // Restore capture availability only. The independent watermark and
            // scan_required retain every uncovered failure/unsupported commit.
            state.tracking_stopped = false;
        }
        if result.is_ok()
            && let (Some(mark), Some(bound)) = (commit.collection.as_ref(), state.bound.as_mut())
        {
            bound.latest_mod = mark.col_mod;
            bound.latest_scm = mark.scm;
        }
        if result.is_ok() && generic && Self::gate_locked(&state).is_ok() {
            self.emit_pending_locked(&mut state, super::workflow::UpdateKind::Full, Default::default())
                .map_err(engine_error)?;
        }
        #[cfg(feature = "test-hooks")]
        {
            state.last_capture_body_reads = super::workflow_store::body_reads().saturating_sub(body_reads_before);
        }
        result.map_err(engine_error)
    }
    fn observe_failed(&self, seq: Seq) {
        let mut state = self.state();
        Self::invalidate_locked(&mut state);
        if let Some(proof) = state.last_capture.as_mut().filter(|proof| proof.observed_seq == seq) {
            proof.persisted = false;
        }
    }
}
impl OpenStep for Runtime {
    fn name(&self) -> &str {
        "cloud-note-capture"
    }
    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        // Cloud failures close the cloud gate, never local study. No provider
        // is called by the writer; this runs on the lifecycle caller.
        if self.bind(collection).is_err() {
            let mut state = self.state();
            state.scan_required = true;
            state.tracking_stopped = true;
            state.bound = None;
        }
        Ok(())
    }
}
impl GenerationListener for Runtime {
    fn name(&self) -> &str {
        "cloud-note-capture"
    }
    fn collection_reopened(&self, collection: &Collection) -> Result<(), EngineError> {
        OpenStep::run(self, collection)
    }
    fn collection_replaced(&self, _old: Generation, new: Generation) {
        let mut state = self.state();
        // Reopen already bound the new incarnation. A failed reopen has no
        // matching guard and remains closed; never reuse the old verification.
        if state
            .bound
            .as_ref()
            .is_some_and(|bound| bound.evidence.generation != new)
        {
            state.bound = None;
        }
        state.scan_required = true;
    }
}
impl SuspendParticipant for Runtime {
    fn name(&self) -> &str {
        "cloud-note-capture"
    }
    fn flush(&self) -> Result<(), EngineError> {
        Ok(())
    }
    fn suspend(&self) -> Result<(), EngineError> {
        // The lifecycle has stopped admission and joined admitted writer ops.
        let file = self.inputs.files.identity(&self.directory.join("collection.anki2"));
        let publication =
            meguri_engine::device_state::DeviceState::for_collection(&self.directory.join("collection.anki2"))
                .publication();
        let mut state = self.state();
        let complete = state.whole_complete
            && !state.scan_required
            && !state.scan_running
            && !state.tracking_stopped
            && state.bound.as_ref().is_some_and(|b| {
                b.usable
                    && b.verified
                    && file.as_ref().is_ok_and(|f| *f == b.file)
                    && publication.as_ref().is_ok_and(|p| {
                        p.as_ref().is_some_and(|p| {
                            p.identity == b.binding.facts.publication
                                && p.decision == meguri_engine::device_state::PublicationDecision::Clear
                        })
                    })
            });
        if let Some((id, mod_time, scm)) = state.bound.as_ref().map(|b| (b.binding.id, b.latest_mod, b.latest_scm)) {
            with_tracking_access(|| {
                state
                    .store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .set_workflow_drained(id, complete, (mod_time, scm))
            })
            .map_err(engine_error)?;
        }
        Self::adapter_close_exchange_locked(&mut state);
        state.incarnation = state
            .incarnation
            .checked_add(1)
            .ok_or_else(|| engine_error("incarnation exhausted"))?;
        if state.adapter.retain_inspection && state.bound.is_some() {
            state.adapter.closed_snapshot = Some(Self::adapter_snapshot_locked(&state).map_err(engine_error)?);
        }
        state.bound = None;
        state.scan_required = true;
        state.scan_running = false;
        state.note_scan = None;
        Ok(())
    }
    fn release_connections_until(&self, _deadline: Duration) -> Result<(), EngineError> {
        let mut state = self.state();
        with_tracking_access(|| {
            state.store.take();
        });
        Ok(())
    }
    fn resume_with_collection(&self, collection: &Collection) -> Result<(), EngineError> {
        OpenStep::run(self, collection)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        // Incoming writer closures share state; take its lock before the tracking gate.
        let mut state = self.state();
        with_tracking_access(|| {
            state.store.take();
        });
    }
}

/// K6.1 conversion of a stored Notes image. No model coercion and no USN in
/// either digest. The acknowledged layout is retained only for the same MID.
pub(super) fn note_image(row: &[Cell], nt_struct_gen: u64) -> Result<codec::Note, Error> {
    if row.len() != Table::Notes.columns().len() {
        return Err(Error::InvalidData);
    }
    let integer = |index: usize| match row.get(index) {
        Some(Cell::Integer(value)) => Ok(*value),
        _ => Err(Error::InvalidData),
    };
    let text = |index: usize| match row.get(index) {
        Some(Cell::Text(value)) => Ok(value.clone()),
        _ => Err(Error::InvalidData),
    };
    let sfld = match &row[7] {
        Cell::Integer(value) => codec::Stored::Integer(*value),
        Cell::Real(value) => codec::Stored::Real(*value),
        Cell::Text(value) => codec::Stored::Text(value.clone()),
        Cell::Blob(value) => codec::Stored::Blob(value.clone()),
        Cell::Null => return Err(Error::InvalidData),
    };
    Ok(codec::Note {
        id: integer(0)?,
        guid: text(1)?,
        mid: integer(2)?,
        modified: integer(3)?,
        tags: text(5)?,
        flds: text(6)?,
        sfld,
        csum: integer(8)?,
        flags: integer(9)?,
        data: text(10)?,
        nt_struct_gen,
    })
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    pub fn set_incoming_stage_hook_for_testing(
        &self,
        hook: impl Fn(super::testing::IncomingStage) + Send + Sync + 'static,
    ) {
        *self.incoming_hooks.stage.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(hook));
    }
    pub fn incoming_stage_for_testing(&self, stage: super::testing::IncomingStage) {
        self.incoming_hooks.hit(stage);
    }
    pub fn interrupt_next_incoming_for_testing(&self, interruption: super::testing::ApplyInterruption) {
        *self
            .incoming_hooks
            .interruption
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(interruption);
    }
    pub fn fail_next_media_io_for_testing(&self) {
        super::media::io::fail_next_open();
    }
    pub fn incoming_sdk_instance_for_testing(&self) -> Option<String> {
        self.state().adapter.sdk.as_ref().map(|sdk| sdk.id.clone())
    }
    pub fn incoming_engine_state_for_testing(&self, binding: BindingId) -> Result<Option<Vec<u8>>, RuntimeError> {
        let state = self.state();
        with_tracking_access(|| {
            Ok(state
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .engine_state(binding)?
                .map(|s| s.serialization.0))
        })
    }
}
