//! Retained above the UI; native executes network actions and feeds actual outcomes back.
use super::*;
use crate::cloud::{
    adapter::CollectionOutcome,
    runtime::{IdentityStatus, Runtime, RuntimeError},
    *,
};
use meguri_engine::{Collection, Generation, LifecycleState};
use meguri_model::clock::Clock;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub action: Action,
    /// Opening identity, independent of the SDK instance used at an exchange step.
    pub owner: String,
    backend_guard: crate::cloud::adapter::Guard,
    // Part of command equality: an old handle cannot consume a later drain.
    cleanup_drain: Option<crate::cloud::publication::CleanupDrain>,
    quiet_proof: Option<QuietProof>,
}
#[derive(Clone, Debug)]
struct QuietProof(Arc<QuietIssuance>);
#[derive(Debug)]
struct QuietIssuance {
    guard: Guard,
    at_ms: i64,
    ordinal: u64,
    consumed: std::sync::atomic::AtomicBool,
}
impl PartialEq for QuietProof {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for QuietProof {}
impl QuietProof {
    fn qualifies(&self, action: &Action) -> bool {
        self.0.guard == action.guard
            && action.kind == (ActionKind::RecordQuietPoint { at_ms: self.0.at_ms })
            && !self.0.consumed.load(std::sync::atomic::Ordering::SeqCst)
    }
}
#[derive(Clone, Debug)]
pub enum DeviceEvent {
    Refresh(ChangeCause),
    TrackingWriteFailed {
        no_space: bool,
    },
    Settings(DeviceSettings),
    Account {
        identity: IdentityStatus,
        origin_sdk: Option<String>,
    },
    IdentityRead {
        request: u64,
        identity: IdentityStatus,
    },
    AcceptedIdentityRead {
        request: u64,
    },
    Network {
        reachable: bool,
    },
    Foreground,
    Background {
        deadline_ms: i64,
    },
    BackgroundExpiring,
    QuitRequested,
    FlushTimerFired,
    SyncNowRequested(Source),
    SyncNowCancelled {
        exchange: u64,
    },
    VolumeSpaceAvailable,
    VisibleMedia {
        names: Vec<String>,
    },
    Operation {
        instance: String,
        operation: Operation,
        phase: Activity,
    },
    OperationError {
        instance: String,
        operation: Operation,
        class: OperationError,
        code: i64,
        retry_after_ms: Option<i64>,
    },
}
#[derive(Debug)]
pub enum DriverError {
    Runtime(RuntimeError),
    CoreActionFailed {
        error: RuntimeError,
        commands: Vec<Command>,
    },
    ApplyDeferred(meguri_engine::EngineError),
    StaleCommand,
    Unavailable(Unavailable),
    NotCoreAction,
    CoreCapabilityUnavailable(&'static str),
    UnqualifiedCompletion,
}
impl From<RuntimeError> for DriverError {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}
impl From<meguri_engine::EngineError> for DriverError {
    fn from(value: meguri_engine::EngineError) -> Self {
        Self::Runtime(value.into())
    }
}
impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Runtime(_) | Self::CoreActionFailed { .. } => "cloud policy operation failed",
            Self::ApplyDeferred(_) => "cloud incoming apply was rolled back",
            Self::StaleCommand => "cloud policy command is stale",
            Self::Unavailable(_) => "Sync Now is unavailable",
            Self::NotCoreAction => "action requires the native coordinator",
            Self::CoreCapabilityUnavailable(_) => "core action provider is unavailable",
            Self::UnqualifiedCompletion => "exchange has no qualified Runtime completion",
        })
    }
}
impl std::error::Error for DriverError {}
#[derive(Clone, Default)]
struct Owned {
    state: State,
    sdk: Option<String>,
    drain_offset: usize,
    captures: BTreeMap<u64, (String, u64)>,
    last_owner: String,
    issued_records: BTreeMap<u64, ExchangeRecord>,
    next_quiet_ordinal: u64,
    recorded_quiet_ordinal: Option<u64>,
    issued_cleanup: Option<Command>,
}
/// Only committed policy inputs needed by final native validation. This is not
/// a projection and carries no Runtime or SDK authority.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeValidation {
    guard: Guard,
    automatic: bool,
}
impl From<&State> for NativeValidation {
    fn from(state: &State) -> Self {
        Self {
            guard: state.guard(),
            automatic: state.automatic(),
        }
    }
}
impl NativeValidation {
    fn validate_policy(&self, command: &Command) -> Result<(), DriverError> {
        let automatic = match command.action.kind {
            ActionKind::StartEngine { automatic, .. } => Some(automatic),
            ActionKind::ConfigureAutomatic { on } => Some(on),
            _ => None,
        };
        if self.guard != command.action.guard || automatic.is_some_and(|value| value != self.automatic) {
            return Err(DriverError::StaleCommand);
        }
        Ok(())
    }
}
/// Only actual Owned values use this guard. Prepared clones stay private until
/// assignment into actual Owned. Publish on every exit, including partial errors,
/// while the actual Driver lock still excludes a subsequent writer.
struct ActualOwned<'a> {
    owned: MutexGuard<'a, Owned>,
    validation: &'a Mutex<NativeValidation>,
}
impl std::ops::Deref for ActualOwned<'_> {
    type Target = Owned;
    fn deref(&self) -> &Owned {
        &self.owned
    }
}
impl std::ops::DerefMut for ActualOwned<'_> {
    fn deref_mut(&mut self) -> &mut Owned {
        &mut self.owned
    }
}
impl Drop for ActualOwned<'_> {
    fn drop(&mut self) {
        *self.validation.lock().unwrap_or_else(PoisonError::into_inner) = NativeValidation::from(&self.owned.state);
    }
}
impl Owned {
    fn issue_quiet(&mut self, guard: Guard, at_ms: i64) -> Result<QuietProof, DriverError> {
        let ordinal = self.next_quiet_ordinal;
        self.next_quiet_ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| DriverError::Runtime(Error::InvalidData.into()))?;
        Ok(QuietProof(Arc::new(QuietIssuance {
            guard,
            at_ms,
            ordinal,
            consumed: std::sync::atomic::AtomicBool::new(false),
        })))
    }
}
/// Explicit authority for one facade job, passed through its first effect.
/// Synchronous callers have no cancellation authority. After an effect, the
/// original operation and its failure follow-ups must run to their real result.
#[derive(Clone, Default)]
pub struct PolicyWork {
    cancel: Option<meguri_engine::CancelToken>,
    required_completion: bool,
    generation: Option<(meguri_engine::WeakEngine, meguri_engine::Generation)>,
    generation_rejected: Arc<std::sync::atomic::AtomicBool>,
    effected: Arc<std::sync::atomic::AtomicBool>,
    rejected: Arc<std::sync::atomic::AtomicBool>,
    owner_check: Option<Arc<dyn Fn() -> Result<(), meguri_engine::EngineError> + Send + Sync>>,
    compact_owner_check: Option<Arc<dyn Fn() -> Result<(), meguri_engine::EngineError> + Send + Sync>>,
}
impl PolicyWork {
    pub fn new(cancel: &meguri_engine::CancelToken) -> Self {
        Self {
            cancel: Some(cancel.clone()),
            ..Self::default()
        }
    }
    /// Accepted original-SDK Ended work ignores cancellation independently of
    /// the effect latch. All original authority checks still precede effects.
    pub fn required_completion(cancel: &meguri_engine::CancelToken) -> Self {
        Self {
            required_completion: true,
            ..Self::new(cancel)
        }
    }
    /// A pure atomic generation check for retained connection jobs. This
    /// never resolves a later active Host or invokes a validation callback.
    pub fn with_generation(mut self, engine: meguri_engine::WeakEngine, generation: meguri_engine::Generation) -> Self {
        self.generation = Some((engine, generation));
        self
    }
    /// Original Host validation runs only at Driver acquisition, never from
    /// the collection writer or beneath Runtime/tracking ownership.
    pub fn with_owner_check(
        mut self,
        check: Arc<dyn Fn() -> Result<(), meguri_engine::EngineError> + Send + Sync>,
    ) -> Self {
        self.owner_check = Some(check);
        self
    }
    /// Only bounded original facts: no Host lock, provider, I/O or callback to
    /// native. Unlike owner_check, this may run beneath Runtime/tracking.
    pub fn with_compact_owner_check(
        mut self,
        check: Arc<dyn Fn() -> Result<(), meguri_engine::EngineError> + Send + Sync>,
    ) -> Self {
        self.compact_owner_check = Some(check);
        self
    }
    fn after_driver_acquisition(&self) -> Result<(), RuntimeError> {
        self.check()?;
        if !self.effected.load(std::sync::atomic::Ordering::SeqCst)
            && let Some(check) = &self.owner_check
        {
            check()?;
        }
        Ok(())
    }
    pub fn check(&self) -> Result<(), RuntimeError> {
        if !self.effected.load(std::sync::atomic::Ordering::SeqCst) {
            if !self.required_completion
                && let Some(cancel) = &self.cancel
            {
                cancel.check()?;
            }
            if let Some((engine, generation)) = &self.generation
                && engine.upgrade().is_none_or(|engine| engine.generation() != *generation)
            {
                self.generation_rejected
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(RuntimeError::StaleIncarnation);
            }
            if let Some(check) = &self.compact_owner_check {
                check()?;
            }
        }
        Ok(())
    }
    /// Retain only rejection by this work's sampled engine generation.
    pub fn generation_was_rejected(&self) -> bool {
        self.generation_rejected.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn effect(&self) -> Result<(), RuntimeError> {
        if let Err(error) = self.check() {
            self.rejected.store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(error);
        }
        self.effected.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    pub(crate) fn was_rejected(&self) -> bool {
        self.rejected.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub(crate) fn engine_effect(&self) -> Result<(), meguri_engine::EngineError> {
        self.effect().map_err(|_| meguri_engine::EngineError::Cancelled)
    }
    pub(crate) fn media_effect(&self) -> Result<(), meguri_storage::media::MediaError> {
        self.effect().map_err(|_| meguri_storage::media::MediaError::Cancelled)
    }
    pub(crate) fn media_error(&self, error: meguri_storage::media::MediaError) -> RuntimeError {
        if self.rejected.load(std::sync::atomic::Ordering::SeqCst) {
            meguri_engine::EngineError::Cancelled.into()
        } else {
            let _ = error;
            crate::cloud::Error::Io(std::io::ErrorKind::Other).into()
        }
    }
}
/// No Host/Collection strong reference. Every collection operation borrows its current handle.
pub struct Driver {
    runtime: Arc<Runtime>,
    clock: Arc<dyn Clock>,
    inner: Mutex<Owned>,
    native_validation: Mutex<NativeValidation>,
    #[cfg(feature = "test-hooks")]
    contention: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    #[cfg(feature = "test-hooks")]
    projection_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(feature = "test-hooks")]
    native_validation_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    pub(super) zone_deletion: Mutex<std::sync::Weak<crate::cloud::zone_deletion::Pass>>,
}
#[cfg(feature = "test-hooks")]
pub struct ExchangeObservation {
    pub policy_exchange: u64,
    pub captures: BTreeMap<u64, u64>,
    pub completed_ids: Vec<u64>,
    pub active_id: Option<u64>,
}
impl Driver {
    /// Retained opening/generation only: completed commands may have old tokens.
    #[cfg(feature = "test-hooks")]
    pub fn exchange_observation_for_testing(&self, command: &Command) -> Result<ExchangeObservation, DriverError> {
        let owned = self.lock_owned();
        let ActionKind::RunExchange { exchange, .. } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        let (completed_ids, active_id) = self
            .runtime
            .exchange_ids_for_testing(&command.owner, command.action.guard.generation)?;
        Ok(ExchangeObservation {
            policy_exchange: exchange,
            captures: owned
                .captures
                .iter()
                .map(|(request, (_, id))| (*request, *id))
                .collect(),
            completed_ids,
            active_id,
        })
    }
    #[cfg(feature = "test-hooks")]
    pub fn original_exchange_witness_for_testing(
        &self,
        command: &Command,
        collection: &Collection,
    ) -> Result<[u8; 32], DriverError> {
        self.validate_original_exchange_issuance(command)?;
        let ActionKind::RunExchange { exchange, source } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        let witness =
            self.runtime
                .original_exchange_witness_for_testing(collection, &command.owner, exchange, source)?;
        self.validate_original_exchange_issuance(command)?;
        Ok(witness)
    }
    #[cfg(feature = "test-hooks")]
    pub fn original_exchange_capture_for_testing(
        &self,
        command: &Command,
        runtime_exchange: u64,
    ) -> Result<(u64, Vec<String>), DriverError> {
        let owned = self.lock_owned();
        self.validate_locked(&owned, command)?;
        let ActionKind::RunExchange { exchange, .. } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        let (sdk, _) = owned
            .captures
            .get(&exchange)
            .filter(|(_, id)| *id == runtime_exchange)
            .ok_or(DriverError::UnqualifiedCompletion)?;
        Ok(self.runtime.exchange_capture_for_testing(sdk, runtime_exchange)?)
    }
    #[cfg(feature = "test-hooks")]
    fn validate_original_exchange_issuance(&self, command: &Command) -> Result<(), DriverError> {
        let owned = self.lock_owned();
        // This includes the original opening and private Runtime guard. The
        // ordinary command guard alone intentionally survives result recording.
        self.validate_locked(&owned, command)?;
        let ActionKind::RunExchange { exchange, source } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        if !owned.state.exchange.as_ref().is_some_and(|active| {
            active.id == exchange && active.source == source && active.guard == command.action.guard
        }) || owned.captures.contains_key(&exchange)
        {
            return Err(DriverError::StaleCommand);
        }
        Ok(())
    }
    pub fn new(runtime: Arc<Runtime>, clock: Arc<dyn Clock>) -> Self {
        Self {
            runtime,
            clock,
            inner: Mutex::new(Owned::default()),
            native_validation: Mutex::new(NativeValidation::from(&State::default())),
            #[cfg(feature = "test-hooks")]
            contention: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            projection_hook: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            native_validation_hook: Mutex::new(None),
            zone_deletion: Mutex::new(std::sync::Weak::new()),
        }
    }
    /// One-shot Rust-only observation: a worker failed to acquire the real
    /// Driver mutex. No timing inference or foreign callback is involved.
    #[cfg(feature = "test-hooks")]
    pub fn observe_contention_for_testing(&self, entered: std::sync::mpsc::Sender<()>) {
        *self.contention.lock().unwrap_or_else(PoisonError::into_inner) = Some(entered);
    }
    /// Rust-only barrier after real review polling and projection reads, before
    /// Driver projection effects. The callback never grants work authority.
    #[cfg(feature = "test-hooks")]
    pub fn set_projection_hook_for_testing(&self, hook: impl FnOnce() + Send + 'static) {
        *self.projection_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }
    fn lock_owned(&self) -> ActualOwned<'_> {
        ActualOwned {
            owned: self.inner.lock().unwrap_or_else(PoisonError::into_inner),
            validation: &self.native_validation,
        }
    }
    fn committed_native_validation(&self) -> NativeValidation {
        self.native_validation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// One-shot Rust-only seam between Runtime checks and the final policy read.
    #[cfg(feature = "test-hooks")]
    pub fn set_native_validation_hook_for_testing(&self, hook: impl FnOnce() + Send + 'static) {
        *self
            .native_validation_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }
    #[cfg(feature = "test-hooks")]
    pub fn native_validation_matches_state_for_testing(&self) -> bool {
        let owned = self.lock_owned();
        self.committed_native_validation() == NativeValidation::from(&owned.state)
    }
    fn lock_for_work(&self, work: &PolicyWork) -> Result<ActualOwned<'_>, DriverError> {
        #[cfg(feature = "test-hooks")]
        let owned = match self.inner.try_lock() {
            Ok(owned) => owned,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                if let Some(entered) = self.contention.lock().unwrap_or_else(PoisonError::into_inner).take() {
                    let _ = entered.send(());
                }
                self.inner.lock().unwrap_or_else(PoisonError::into_inner)
            }
        };
        #[cfg(not(feature = "test-hooks"))]
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let owned = ActualOwned {
            owned,
            validation: &self.native_validation,
        };
        work.after_driver_acquisition()?;
        Ok(owned)
    }
    pub fn status_with_work(&self, work: &PolicyWork) -> Result<Status, DriverError> {
        Ok(self.lock_for_work(work)?.state.status())
    }
    pub fn status(&self) -> Status {
        self.lock_owned().state.status()
    }
    pub fn state(&self) -> State {
        self.lock_owned().state.clone()
    }
    pub fn handle(
        &self,
        lifecycle: LifecycleState,
        generation: Generation,
        event: DeviceEvent,
    ) -> Result<Vec<Command>, DriverError> {
        self.handle_with_work(lifecycle, generation, event, &mut PolicyWork::default())
    }
    pub fn handle_with_work(
        &self,
        lifecycle: LifecycleState,
        generation: Generation,
        event: DeviceEvent,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, DriverError> {
        let mut owned = self.lock_for_work(work)?;
        let event = match event {
            DeviceEvent::AcceptedIdentityRead { request } => {
                let identity = self.runtime.with_accepted_identity_read(request, Clone::clone)?;
                let mut prepared = owned.clone();
                let commands = self.project_and_step_with_work(
                    &mut prepared,
                    lifecycle,
                    generation,
                    (Event::Account(account(identity)), None),
                    work,
                    false,
                )?;
                // Providers/projection ran outside Runtime's lock. Recheck
                // freshness at the policy mutation boundary, then commit only
                // prepared values while begin/completion cannot supersede us.
                return self
                    .runtime
                    .with_accepted_identity_read(request, |_| {
                        work.effect()?;
                        *owned = prepared;
                        Ok::<_, RuntimeError>(commands)
                    })?
                    .map_err(DriverError::from);
            }
            DeviceEvent::Refresh(cause) => Event::TrackingChanged(cause),
            DeviceEvent::TrackingWriteFailed { no_space } => {
                self.runtime.policy_tracking_failed_with_work(work)?;
                Event::TrackingWriteFailed { no_space }
            }
            DeviceEvent::Settings(settings) => {
                self.runtime.policy_media_setting_with_work(settings.media, work)?;
                self.runtime.adapter_pause(!settings.enabled || settings.paused)?;
                Event::DeviceSettings(settings)
            }
            DeviceEvent::Account { identity, origin_sdk } => {
                if self.runtime.status()?.binding.is_some() {
                    self.runtime
                        .adapter_account_with_work(origin_sdk.as_deref(), identity.clone(), work)?;
                }
                Event::Account(account(identity))
            }
            DeviceEvent::IdentityRead { request, identity } => {
                if !self
                    .runtime
                    .adapter_complete_identity_read_with_work(request, identity.clone(), work)?
                {
                    return Err(DriverError::StaleCommand);
                }
                Event::Account(account(identity))
            }
            DeviceEvent::Network { reachable } => Event::Network { reachable },
            DeviceEvent::Foreground => Event::Foreground,
            DeviceEvent::Background { deadline_ms } => Event::Background { deadline_ms },
            DeviceEvent::BackgroundExpiring => Event::BackgroundExpiring,
            DeviceEvent::QuitRequested => Event::QuitRequested,
            DeviceEvent::FlushTimerFired => Event::FlushTimerFired,
            DeviceEvent::SyncNowRequested(source) => Event::SyncNowRequested(source),
            DeviceEvent::SyncNowCancelled { exchange } => Event::SyncNowCancelled { exchange },
            DeviceEvent::VolumeSpaceAvailable => Event::VolumeSpaceAvailable,
            DeviceEvent::VisibleMedia { names } => Event::VisibleMedia { names },
            DeviceEvent::Operation {
                instance,
                operation,
                phase,
            } => {
                let validation = self.runtime.validate_sdk(&instance);
                #[cfg(feature = "test-hooks")]
                let validation = validation.inspect_err(|error| {
                    let (kind, variant) = error.refusal_tags_for_testing();
                    eprintln!("cloudOperation.refused site=inputSDKValidation errorKind={kind} variant={variant}");
                });
                validation?;
                Event::EngineActivity {
                    instance,
                    operation,
                    phase,
                }
            }
            DeviceEvent::OperationError {
                instance,
                operation,
                class,
                code,
                retry_after_ms,
            } => {
                self.runtime.validate_sdk(&instance)?;
                Event::EngineError {
                    instance,
                    operation,
                    error: class,
                    code,
                    retry_after_ms,
                }
            }
        };
        self.project_and_step_with_work(&mut owned, lifecycle, generation, (event, None), work, true)
    }
    fn project_and_step_with_work(
        &self,
        owned: &mut Owned,
        lifecycle: LifecycleState,
        generation: Generation,
        observation: (Event, Option<i64>),
        work: &mut PolicyWork,
        publish: bool,
    ) -> Result<Vec<Command>, DriverError> {
        let (event, ended_ms) = observation;
        #[cfg(feature = "test-hooks")]
        let operation_input = matches!(&event, Event::EngineActivity { .. });
        let now_ms = ended_ms.unwrap_or_else(|| self.clock.now_ms());
        let cleanup_before = owned.state.last_cleanup_ms;
        let ended_session = match self.runtime.instance() {
            Ok(instance) => {
                let polled = self.runtime.poll_review_with_work(&instance, work);
                #[cfg(feature = "test-hooks")]
                let polled = polled.inspect_err(|error| {
                    if operation_input {
                        let (kind, variant) = error.refusal_tags_for_testing();
                        eprintln!("cloudOperation.refused site=inputReviewPoll errorKind={kind} variant={variant}");
                    }
                });
                polled?
            }
            Err(_) => None,
        };
        let projection = self
            .runtime
            .policy_projection_with_work(lifecycle_kind(&lifecycle), generation.0, work);
        #[cfg(feature = "test-hooks")]
        let projection = projection.inspect_err(|error| {
            if operation_input {
                let (kind, variant) = error.refusal_tags_for_testing();
                eprintln!("cloudOperation.refused site=inputProjection errorKind={kind} variant={variant}");
            }
        });
        let projection = projection?;
        #[cfg(feature = "test-hooks")]
        {
            let hook = self
                .projection_hook
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(hook) = hook {
                hook();
            }
        }
        // ReadFailed preserves Runtime's verified process owner. Project that
        // accepted effective identity without submitting another account event.
        let event = match event {
            Event::Account(Account {
                status: AccountStatus::Undetermined,
                ..
            }) if projection.facts.binding.as_ref().is_some_and(|b| b.verified) => Event::Account(Account {
                status: AccountStatus::Available,
                identity: projection.facts.binding.as_ref().map(|b| b.account.clone()),
            }),
            event => event,
        };
        let owner = self.runtime.instance().unwrap_or_default();
        work.check()?;
        if publish {
            work.effect()?;
        }
        let mut actions = Vec::new();
        let sdk = projection.engine.as_ref().map(|e| e.instance.clone());
        if owner != owned.last_owner {
            owned.captures.clear();
            owned.issued_records.clear();
            owned.next_quiet_ordinal = 0;
            owned.recorded_quiet_ordinal = None;
            owned.issued_cleanup = None;
            owned.last_owner = owner.clone();
            owned.sdk = None;
            owned.drain_offset = projection.drains.len();
        }
        if sdk != owned.sdk {
            owned.state.drained = [false; 2];
            owned.state.root_confirmed = false;
            owned.state.cycles.values_mut().for_each(|c| c.active = false);
            owned.sdk = sdk.clone();
            owned.drain_offset = projection.drains.len();
        }
        let mut engine = projection.engine.clone();
        if let Some(engine) = &mut engine
            && owned
                .state
                .engine
                .as_ref()
                .is_some_and(|old| old.instance == engine.instance && old.stopping)
        {
            engine.stopping = true;
        }
        owned.state.engine = engine;
        // Install explicit failures exactly once, before Binding can open the
        // gate. Projection-only failures still precede the original trigger.
        let (tracking_failure, event) = match event {
            failure @ Event::TrackingWriteFailed { .. } => (Some(failure), None),
            event => (
                (projection.tracking_stopped && owned.state.tracking_stop.is_none())
                    .then_some(Event::TrackingWriteFailed { no_space: false }),
                Some(event),
            ),
        };
        if let Some(event) = tracking_failure {
            let mut facts = projection.facts.clone();
            facts.binding = owned.state.facts.binding.clone();
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: Some(facts),
                    event,
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        // Runtime publication progress is a Binding input (P4/P6). Neither a
        // synthesized session event nor the original trigger may consume the
        // old binding first: Zones -> Sent must compare effective automatic
        // scheduling before projected facts replace it. Keep the nonfailure
        // trigger below, including its operation result and completion handling.
        if owned.state.facts.binding != projection.facts.binding {
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: Some(projection.facts.clone()),
                    event: Event::Binding(projection.facts.binding.clone()),
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        if let Some(session) = ended_session {
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: Some(projection.facts.clone()),
                    event: Event::SessionEnded(session),
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        // A refresh cannot turn a still-failed Runtime into a successful local commit.
        if let Some(event) = event {
            let event = if projection.tracking_stopped && matches!(event, Event::TrackingChanged(ChangeCause::Local)) {
                Event::TrackingChanged(ChangeCause::Cloud)
            } else {
                event
            };
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: Some(projection.facts),
                    event,
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        // A start may still be awaiting native construction. Reissue that
        // intent with the current scheduling preference; its old value is
        // rejected by validate_locked without invalidating other actions.
        if owned.state.engine.is_none()
            && owned.state.gate_open()
            && actions
                .iter()
                .any(|a| matches!(a.kind, ActionKind::ConfigureAutomatic { .. }))
            && !actions.iter().any(|a| matches!(a.kind, ActionKind::StartEngine { .. }))
            && let Some(binding) = &owned.state.facts.binding
        {
            actions.push(Action {
                guard: owned.state.guard(),
                kind: ActionKind::StartEngine {
                    binding: binding.id,
                    automatic: owned.state.automatic(),
                },
            });
        }
        // Durable rows are loaded after owner invalidation; old timestamps are never inferred.
        let (state, emitted) = step(
            owned.state.clone(),
            Input {
                now_ms,
                facts: None,
                event: Event::EvidenceCommitted(Box::new(projection.evidence)),
            },
        );
        owned.state = state;
        actions.extend(emitted);
        let new_drains = projection.drains.get(owned.drain_offset..).unwrap_or_default();
        for drain in new_drains {
            let zone = match drain.zone.as_str() {
                "meguri-root" => Zone::Root,
                z if owned.state.facts.binding.as_ref().is_some() && z.starts_with("c-") => Zone::Collection,
                _ => continue,
            };
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: None,
                    event: Event::FetchDrained {
                        zone,
                        fetched: drain.fetched,
                    },
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        owned.drain_offset = projection.drains.len();
        if projection.root_confirmed
            && !owned.state.root_confirmed
            && let Some(instance) = sdk
        {
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: None,
                    event: Event::RootConfirmed { instance },
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        let current_guard = owned.state.guard();
        for action in &mut actions {
            if action.guard.generation == current_guard.generation && action.guard.binding == current_guard.binding {
                action.guard.token = current_guard.token;
            }
            if let ActionKind::RecordExchange(record) = &action.kind {
                owned.issued_records.insert(record.exchange, record.clone());
            }
        }
        let commands = actions
            .into_iter()
            .map(|action| {
                // Allocate in action order: several drains can emit in this projection.
                let quiet_proof = match &action.kind {
                    ActionKind::RecordQuietPoint { at_ms } => Some(owned.issue_quiet(action.guard.clone(), *at_ms)?),
                    _ => None,
                };
                let cleanup_drain = if action.kind == ActionKind::RunCleanup {
                    projection.cleanup_drain.clone()
                } else {
                    None
                };
                Ok(Command {
                    action,
                    owner: owner.clone(),
                    backend_guard: projection.backend_guard.clone(),
                    cleanup_drain,
                    quiet_proof,
                })
            })
            .collect::<Result<Vec<_>, DriverError>>()?;
        for command in &commands {
            if command.action.kind == ActionKind::RunCleanup {
                owned.issued_cleanup = Some(command.clone());
                // Issuance is not a committed cleanup. Failures/cancellation
                // cannot advance the 24h marker or refresh a blob's 1h mark.
                owned.state.last_cleanup_ms = cleanup_before;
            }
        }
        Ok(commands)
    }
    /// Local P10.3 port for a retained, issued cleanup command. Native's full
    /// RunCleanup still awaits the later inventory observation/integration.
    /// The caller cannot substitute a timestamp; qualification is consumed once.
    pub fn compute_qualified_local_cleanup(
        &self,
        command: &Command,
        collection: &Collection,
    ) -> Result<(), DriverError> {
        let (receipt, sdk) = {
            let mut owned = self.lock_owned();
            self.validate_locked(&owned, command)?;
            if owned.issued_cleanup.as_ref() != Some(command) || command.action.kind != ActionKind::RunCleanup {
                return Err(DriverError::UnqualifiedCompletion);
            }
            let issued = owned.issued_cleanup.take().ok_or(DriverError::UnqualifiedCompletion)?;
            (
                issued.cleanup_drain.ok_or(DriverError::UnqualifiedCompletion)?,
                owned.sdk.clone().ok_or(DriverError::StaleCommand)?,
            )
        };
        // Runtime samples original providers before locking its state. Do not
        // retain the Driver mutex across those foreign callbacks either.
        self.runtime
            .compute_policy_local_cleanup(&command.backend_guard, &sdk, collection, &receipt)?;
        let mut owned = self.lock_owned();
        self.validate_locked(&owned, command)?;
        owned.state.last_cleanup_ms = Some(receipt.2);
        Ok(())
    }
    pub fn validate(&self, command: &Command) -> Result<(), DriverError> {
        self.validate_with_work(command, &mut PolicyWork::default())
    }
    pub fn validate_with_work(&self, command: &Command, work: &mut PolicyWork) -> Result<(), DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        work.check()?;
        Ok(())
    }
    pub fn native_disposition(&self, command: &Command) -> Result<NativeDisposition, DriverError> {
        self.native_disposition_with_work(command, &mut PolicyWork::default())
    }
    pub fn native_disposition_with_work(
        &self,
        command: &Command,
        work: &mut PolicyWork,
    ) -> Result<NativeDisposition, DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        if matches!(
            command.action.kind,
            ActionKind::RequestFetch { .. } | ActionKind::RequestSend { .. }
        ) {
            let engine = self.runtime.policy_engine(&command.backend_guard)?;
            if !engine.as_ref().is_some_and(|engine| {
                !engine.stopping
                    && Some(engine.binding) == command.action.guard.binding
                    && !owned
                        .state
                        .engine
                        .as_ref()
                        .is_some_and(|old| old.instance == engine.instance && old.stopping)
            }) {
                work.check()?;
                return Ok(NativeDisposition::NoOperation);
            }
        }
        if let ActionKind::ConfigureAutomatic { on } = command.action.kind {
            let engine = self.runtime.policy_engine(&command.backend_guard)?;
            let running = engine.as_ref().is_some_and(|engine| {
                !engine.stopping
                    && Some(engine.binding) == command.action.guard.binding
                    && !owned
                        .state
                        .engine
                        .as_ref()
                        .is_some_and(|old| old.instance == engine.instance && old.stopping)
            });
            // An open gate retains scheduling intent even while the old SDK stops.
            // A closed gate can only turn off an existing running instance
            // while a scan or ordinary intent holds back collection work.
            let allowed = owned.state.engine_allowed()
                && (owned.state.gate_open()
                    || (!on
                        && running
                        && owned.state.facts.lifecycle == Lifecycle::Ready
                        && !owned.state.facts.summary.crt_mismatch
                        && (owned.state.facts.summary.scan != Scan::None
                            || owned.state.facts.summary.unsettled_intents > 0)));
            if !allowed {
                work.check()?;
                return Ok(NativeDisposition::NoOperation);
            }
        }
        work.check()?;
        Ok(NativeDisposition::Enqueue)
    }
    fn validate_locked(&self, owned: &Owned, command: &Command) -> Result<(), DriverError> {
        self.validate_view(&NativeValidation::from(&owned.state), command)
    }
    fn validate_view(&self, view: &NativeValidation, command: &Command) -> Result<(), DriverError> {
        let runtime = self.runtime.adapter_guard();
        self.validate_runtime_facts(view, command, &runtime, || self.runtime.instance().unwrap_or_default())
    }
    fn validate_runtime_facts(
        &self,
        view: &NativeValidation,
        command: &Command,
        runtime: &crate::cloud::adapter::Guard,
        owner: impl FnOnce() -> String,
    ) -> Result<(), DriverError> {
        if view.guard != command.action.guard {
            #[cfg(feature = "test-hooks")]
            eprintln!(
                "cloud-policy-rejection stage=validate.policy-guard generation_matches={} binding_matches={} token_matches={}",
                view.guard.generation == command.action.guard.generation,
                view.guard.binding == command.action.guard.binding,
                view.guard.token == command.action.guard.token,
            );
            return Err(DriverError::StaleCommand);
        }
        let current_owner = owner();
        if command.owner != current_owner {
            #[cfg(feature = "test-hooks")]
            eprintln!(
                "cloud-policy-rejection stage=validate.original-owner command_owner_empty={} current_owner_empty={}",
                command.owner.is_empty(),
                current_owner.is_empty(),
            );
            return Err(DriverError::StaleCommand);
        }
        if runtime != &command.backend_guard {
            #[cfg(feature = "test-hooks")]
            eprintln!(
                "cloud-policy-rejection stage=validate.backend-guard generation_matches={} binding_matches={}",
                runtime.generation == command.backend_guard.generation,
                runtime.binding == command.backend_guard.binding,
            );
            return Err(DriverError::StaleCommand);
        }
        // Scheduling values are snapshots, unlike authority for manual work.
        // Async native construction must revalidate the original command here
        // at its registration/factory boundary as well as before enqueueing.
        let automatic = match command.action.kind {
            ActionKind::StartEngine { automatic, .. } => Some(automatic),
            ActionKind::ConfigureAutomatic { on } => Some(on),
            _ => None,
        };
        if automatic.is_some_and(|value| value != view.automatic) {
            #[cfg(feature = "test-hooks")]
            eprintln!("cloud-policy-rejection stage=validate.automatic-setting");
            return Err(DriverError::StaleCommand);
        }
        // Unbound Runtime has no collection generation. Only Host control
        // work may use that issued zero/empty guard; the facade separately
        // validates its retained Host and current collection generation.
        let unbound_host = command.action.kind.host_only()
            && command.action.guard.binding.is_none()
            && command.owner.is_empty()
            && runtime.generation == 0
            && runtime.binding.is_empty();
        if !unbound_host
            && (command.backend_guard.generation != command.action.guard.generation
                || runtime.binding != command.action.guard.binding.map_or_else(Vec::new, |b| b.0.to_vec()))
        {
            #[cfg(feature = "test-hooks")]
            eprintln!(
                "cloud-policy-rejection stage=validate.generation-binding generation_matches={} binding_matches={}",
                command.backend_guard.generation == command.action.guard.generation,
                runtime.binding == command.action.guard.binding.map_or_else(Vec::new, |b| b.0.to_vec()),
            );
            return Err(DriverError::StaleCommand);
        }
        if (command.quiet_proof.is_some() || matches!(command.action.kind, ActionKind::RecordQuietPoint { .. }))
            && command
                .quiet_proof
                .as_ref()
                .is_none_or(|proof| !proof.qualifies(&command.action))
        {
            return Err(DriverError::UnqualifiedCompletion);
        }
        Ok(())
    }
    /// Performs actual core actions. Native must execute the others, in emission order,
    /// outside SDK event handling, and return new operation inputs through `handle`.
    pub fn execute_core(
        &self,
        command: &Command,
        collection: &Collection,
        lifecycle: LifecycleState,
        timeout: Duration,
    ) -> Result<Vec<Command>, DriverError> {
        self.execute_core_with_work(command, collection, lifecycle, timeout, &mut PolicyWork::default())
    }
    pub fn execute_core_with_work(
        &self,
        command: &Command,
        collection: &Collection,
        lifecycle: LifecycleState,
        timeout: Duration,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, DriverError> {
        let mut owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        let guard = &command.backend_guard;
        let result = match &command.action.kind {
            ActionKind::UpdatePending(update) => self
                .runtime
                .policy_update_pending_with_work(guard, *update, work)
                .map(|_| Event::EvidenceCommitted(Box::new(owned.state.evidence.clone()))),
            ActionKind::ProbeTracking => self
                .runtime
                .policy_probe_with_work(guard, work)
                .map(|_| Event::TrackingChanged(ChangeCause::Probe)),
            ActionKind::ApplyReady { retry_failed } => {
                match self
                    .runtime
                    .policy_apply_ready_with_work(collection, guard, *retry_failed, timeout, work)
                {
                    Ok(crate::cloud::incoming::IncomingDisposition::Progress(0)) => {
                        // Ready media can await bytes that only an external event supplies.
                        // Reproject the observation without requesting another apply.
                        Ok(Event::EvidenceCommitted(Box::new(owned.state.evidence.clone())))
                    }
                    Ok(crate::cloud::incoming::IncomingDisposition::Progress(_)) => {
                        Ok(Event::TrackingChanged(ChangeCause::Cloud))
                    }
                    Ok(crate::cloud::incoming::IncomingDisposition::Deferred(error)) => {
                        return Err(DriverError::ApplyDeferred(error));
                    }
                    Err(error) => Err(error),
                }
            }
            ActionKind::RecordQuietPoint { at_ms } => {
                let proof = command.quiet_proof.as_ref().ok_or(DriverError::UnqualifiedCompletion)?;
                let current = self.runtime.policy_projection_with_work(
                    lifecycle_kind(&lifecycle),
                    collection.generation().0,
                    work,
                )?;
                let summary = &current.facts.summary;
                if !summary.collection_known
                    || summary.scan != Scan::None
                    || summary.pending_unheld > 0
                    || summary.in_flight > 0
                    || summary.ready > 0
                    || summary.apply_in_progress
                    || summary.unsettled_intents > 0
                    || owned.state.drained != [true, true]
                {
                    #[cfg(feature = "test-hooks")]
                    eprintln!("cloud-policy-observation stage=execute.quiet-superseded");
                    // Valid work can arrive after this drain qualified. Consume
                    // its candidate without stamping or retrying the same drain.
                    work.effect()?;
                    proof.0.consumed.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(Event::EvidenceCommitted(Box::new(owned.state.evidence.clone())))
                } else if owned
                    .recorded_quiet_ordinal
                    .is_some_and(|ordinal| ordinal > proof.0.ordinal)
                {
                    // A later issued command only supersedes this stamp after
                    // its Runtime write succeeds, regardless of clock movement.
                    Ok(Event::EvidenceCommitted(Box::new(owned.state.evidence.clone())))
                } else {
                    self.runtime
                        .policy_record_quiet_with_work(guard, *at_ms, work)
                        .map(|evidence| {
                            owned.recorded_quiet_ordinal = Some(proof.0.ordinal);
                            Event::EvidenceCommitted(Box::new(evidence))
                        })
                }
            }
            ActionKind::RecordExchange(record) => {
                if owned.issued_records.get(&record.exchange) != Some(record) {
                    return Err(DriverError::UnqualifiedCompletion);
                }
                self.runtime
                    .policy_record_exchange_with_work(collection, guard, &command.owner, record, work)
                    .map(|evidence| Event::EvidenceCommitted(Box::new(evidence)))
            }
            ActionKind::RunCleanup => {
                work.effect()?;
                owned.issued_cleanup = None;
                return Err(DriverError::CoreCapabilityUnavailable("cloud cleanup"));
            }
            ActionKind::StatusChanged(_) => return Ok(Vec::new()),
            _ => return Err(DriverError::NotCoreAction),
        };
        match result {
            Ok(event) => self.project_and_step_with_work(
                &mut owned,
                lifecycle,
                collection.generation(),
                (event, None),
                work,
                true,
            ),
            Err(error) => {
                // A rejected boundary has made no policy/durable effect. It
                // must not be turned into a new tracking-failure transition.
                if work.rejected.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(DriverError::Runtime(error));
                }
                if !self.runtime.policy_tracking_stopped() {
                    return Err(DriverError::Runtime(error));
                }
                // The original error is returned. Preserve the stop even if no later callback arrives.
                let no_space = matches!(
                    &error,
                    RuntimeError::Tracking(Error::Io(std::io::ErrorKind::StorageFull))
                );
                let (state, actions) = step(
                    owned.state.clone(),
                    Input {
                        now_ms: self.clock.now_ms(),
                        facts: None,
                        event: Event::TrackingWriteFailed { no_space },
                    },
                );
                owned.state = state;
                // These are newly issued stop actions, after Runtime reported failure.
                let backend_guard = self.runtime.adapter_guard();
                let commands = actions
                    .into_iter()
                    .map(|action| Command {
                        action,
                        owner: command.owner.clone(),
                        backend_guard: backend_guard.clone(),
                        cleanup_drain: None,
                        quiet_proof: None,
                    })
                    .collect();
                Err(DriverError::CoreActionFailed { error, commands })
            }
        }
    }
    /// Starts the existing Runtime capture, not a policy-owned selection.
    /// Call only after P12.3's gate wait/fetch prerequisites, never from an SDK handler.
    pub fn capture_exchange(&self, command: &Command, collection: &Collection) -> Result<u64, DriverError> {
        self.capture_exchange_with_work(command, collection, &mut PolicyWork::default())
    }
    pub fn capture_exchange_with_work(
        &self,
        command: &Command,
        collection: &Collection,
        work: &mut PolicyWork,
    ) -> Result<u64, DriverError> {
        let mut owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        let ActionKind::RunExchange { exchange, .. } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        if let Some((_, id)) = owned.captures.get(&exchange) {
            work.check()?;
            return Ok(*id);
        }
        let sdk = owned
            .state
            .engine
            .as_ref()
            .filter(|e| !e.stopping)
            .ok_or(DriverError::StaleCommand)?
            .instance
            .clone();
        let capture = self.runtime.sync_now_with_work(&sdk, collection, work);
        #[cfg(feature = "test-hooks")]
        let capture = capture.inspect_err(|error| {
            let (kind, variant) = error.refusal_tags_for_testing();
            eprintln!("cloudCapture.refused site=exchangeCapture errorKind={kind} variant={variant}");
        });
        let id = capture?;
        owned.captures.insert(exchange, (sdk, id));
        Ok(id)
    }
    /// Only Runtime's accepted, clocked completion can become a successful result.
    pub fn complete_exchange(
        &self,
        request: u64,
        lifecycle: LifecycleState,
        generation: Generation,
    ) -> Result<Vec<Command>, DriverError> {
        self.complete_exchange_with_work(request, lifecycle, generation, &mut PolicyWork::default())
    }
    pub fn complete_exchange_with_work(
        &self,
        request: u64,
        lifecycle: LifecycleState,
        generation: Generation,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, DriverError> {
        let mut owned = self.lock_for_work(work)?;
        let (_, id) = owned.captures.get(&request).ok_or(DriverError::UnqualifiedCompletion)?;
        let (completed, ended_ms) = self
            .runtime
            .policy_completed_exchange(*id)
            .ok_or(DriverError::UnqualifiedCompletion)?;
        let projection = self
            .runtime
            .policy_projection_with_work(lifecycle_kind(&lifecycle), generation.0, work)?;
        let summary = &projection.facts.summary;
        let frozen = completed.work.as_ref();
        let held_pending = frozen.map_or(&projection.held_pending, |snapshot| &snapshot.held_pending_names);
        let outcome = match completed.collection {
            CollectionOutcome::Succeeded => ExchangeOutcome::Succeeded {
                selected: completed.result.selected_names.len() as u64,
                held_back: held_pending.union(&completed.result.held_back_names).count() as u64,
                newer_pending: frozen.map_or(summary.pending_unheld, |snapshot| {
                    snapshot.pending_unheld_names.len() as u64
                }),
                incoming_waiting: completed
                    .incoming
                    .as_ref()
                    .map_or(summary.incoming_waiting, |i| i.waiting),
                media: frozen.map_or_else(|| summary.media.clone(), |snapshot| snapshot.media.clone()),
            },
            CollectionOutcome::WaitingNoAccount => ExchangeOutcome::Waiting {
                class: OperationError::NoAccount,
                retry_after_ms: None,
            },
            CollectionOutcome::Incomplete => ExchangeOutcome::Incomplete {
                outstanding: completed.result.outstanding_names.len() as u64,
                blocked: completed.result.blocked_names.len() as u64
                    + completed
                        .incoming
                        .as_ref()
                        .map_or(summary.incoming_waiting, |i| i.blocked),
            },
            CollectionOutcome::Failed => ExchangeOutcome::Failed {
                class: owned
                    .state
                    .last_error
                    .as_ref()
                    .map_or(OperationError::Other, |e| e.class),
            },
            CollectionOutcome::Cancelled => ExchangeOutcome::Cancelled,
        };
        self.project_and_step_with_work(
            &mut owned,
            lifecycle,
            generation,
            (
                Event::ExchangeEnded {
                    exchange: request,
                    outcome,
                },
                Some(ended_ms),
            ),
            work,
            true,
        )
    }
    /// Reports an operation that stopped before Runtime could capture it. Cannot report success.
    pub fn end_before_capture(
        &self,
        request: u64,
        outcome: ExchangeOutcome,
        lifecycle: LifecycleState,
        generation: Generation,
    ) -> Result<Vec<Command>, DriverError> {
        self.end_before_capture_with_work(request, outcome, lifecycle, generation, &mut PolicyWork::default())
    }
    pub fn end_before_capture_with_work(
        &self,
        request: u64,
        outcome: ExchangeOutcome,
        lifecycle: LifecycleState,
        generation: Generation,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, DriverError> {
        let mut owned = self.lock_for_work(work)?;
        if owned.captures.contains_key(&request)
            || matches!(
                outcome,
                ExchangeOutcome::Succeeded { .. } | ExchangeOutcome::Incomplete { .. }
            )
        {
            return Err(DriverError::UnqualifiedCompletion);
        }
        self.project_and_step_with_work(
            &mut owned,
            lifecycle,
            generation,
            (
                Event::ExchangeEnded {
                    exchange: request,
                    outcome,
                },
                None,
            ),
            work,
            true,
        )
    }
}
fn account(identity: IdentityStatus) -> Account {
    match identity {
        IdentityStatus::Available(identity) => Account {
            status: AccountStatus::Available,
            identity: Some(identity),
        },
        IdentityStatus::SignedOut => Account {
            status: AccountStatus::NoAccount,
            identity: None,
        },
        IdentityStatus::Restricted => Account {
            status: AccountStatus::Restricted,
            identity: None,
        },
        IdentityStatus::Configuration => Account {
            status: AccountStatus::Configuration,
            identity: None,
        },
        IdentityStatus::TemporarilyUnavailable => Account {
            status: AccountStatus::Temporary,
            identity: None,
        },
        IdentityStatus::ReadFailed | IdentityStatus::SwitchedAccounts => Account {
            status: AccountStatus::Undetermined,
            identity: None,
        },
    }
}
fn lifecycle_kind(lifecycle: &LifecycleState) -> Lifecycle {
    match lifecycle {
        LifecycleState::NoCollection => Lifecycle::NoCollection,
        LifecycleState::Opening => Lifecycle::Opening,
        LifecycleState::Upgrading { .. } => Lifecycle::Upgrading,
        LifecycleState::Ready { .. } => Lifecycle::Ready,
        LifecycleState::ExclusiveBusy { .. } => Lifecycle::ExclusiveBusy,
        LifecycleState::Failed {
            reason: meguri_engine::lifecycle::FailedReason::Corrupt { .. },
        } => Lifecycle::Corrupt,
        LifecycleState::Failed { .. } => Lifecycle::Failed,
    }
}

#[path = "native_execution.rs"]
mod native_execution;

#[path = "zone_deletion.rs"]
mod zone_deletion;

#[cfg(test)]
mod quiet_proof_tests {
    use super::*;

    #[test]
    fn quiet_issuance_exhaustion_is_checked_without_reusing_an_ordinal() {
        let mut owned = Owned {
            next_quiet_ordinal: u64::MAX - 1,
            ..Owned::default()
        };
        let proof = owned.issue_quiet(owned.state.guard(), 0).unwrap();
        assert_eq!(proof.0.ordinal, u64::MAX - 1);
        for _ in 0..2 {
            assert!(matches!(
                owned.issue_quiet(owned.state.guard(), 0),
                Err(DriverError::Runtime(RuntimeError::Tracking(Error::InvalidData)))
            ));
            assert_eq!(owned.next_quiet_ordinal, u64::MAX);
        }
        assert_eq!(proof.0.ordinal, u64::MAX - 1);
    }
}

#[cfg(test)]
mod native_validation_tests {
    use super::*;

    #[test]
    fn actual_owned_publishes_effective_automatic_and_partial_error_before_unlock() {
        let inner = Mutex::new(Owned::default());
        let validation = Mutex::new(NativeValidation::from(&State::default()));
        let result: Result<(), DriverError> = {
            let mut owned = ActualOwned {
                owned: inner.lock().unwrap(),
                validation: &validation,
            };
            owned.state.facts.generation = 7;
            owned.state.token = 9;
            owned.state.facts.binding = Some(BindingView {
                id: BindingId([3; 16]),
                state: BindingState::Active,
                account: OpaqueBytes(b"synthetic-account".to_vec()),
                usable: true,
                verified: true,
                publish_step: Some(PublishStep::Zones),
            });
            assert!(!owned.state.settings.automatic);
            assert!(owned.state.automatic());
            // The old committed view stays available during actual mutation.
            assert_eq!(*validation.lock().unwrap(), NativeValidation::from(&State::default()));
            Err(DriverError::StaleCommand)
        };
        assert!(matches!(result, Err(DriverError::StaleCommand)));
        let owned = inner.try_lock().expect("Driver authority released on error");
        assert_eq!(*validation.lock().unwrap(), NativeValidation::from(&owned.state));
        assert!(validation.lock().unwrap().automatic);
    }
}
