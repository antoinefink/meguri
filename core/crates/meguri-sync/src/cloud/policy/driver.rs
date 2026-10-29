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
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub action: Action,
    /// Opening identity, independent of the SDK instance used at an exchange step.
    pub owner: String,
    backend_guard: crate::cloud::adapter::Guard,
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
#[derive(Default)]
struct Owned {
    state: State,
    sdk: Option<String>,
    drain_offset: usize,
    captures: BTreeMap<u64, (String, u64)>,
    last_owner: String,
    issued_records: BTreeMap<u64, ExchangeRecord>,
    issued_quiet: Option<(Guard, i64)>,
}
/// No Host/Collection strong reference. Every collection operation borrows its current handle.
pub struct Driver {
    runtime: Arc<Runtime>,
    clock: Arc<dyn Clock>,
    inner: Mutex<Owned>,
}
impl Driver {
    pub fn new(runtime: Arc<Runtime>, clock: Arc<dyn Clock>) -> Self {
        Self {
            runtime,
            clock,
            inner: Mutex::new(Owned::default()),
        }
    }
    pub fn status(&self) -> Status {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).state.status()
    }
    pub fn state(&self) -> State {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).state.clone()
    }
    pub fn handle(
        &self,
        lifecycle: LifecycleState,
        generation: Generation,
        event: DeviceEvent,
    ) -> Result<Vec<Command>, DriverError> {
        let mut owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let event = match event {
            DeviceEvent::Refresh(cause) => Event::TrackingChanged(cause),
            DeviceEvent::TrackingWriteFailed { no_space } => {
                self.runtime.policy_tracking_failed();
                Event::TrackingWriteFailed { no_space }
            }
            DeviceEvent::Settings(settings) => {
                self.runtime.adapter_pause(!settings.enabled || settings.paused)?;
                Event::DeviceSettings(settings)
            }
            DeviceEvent::Account { identity, origin_sdk } => {
                if self.runtime.status()?.binding.is_some() {
                    self.runtime.adapter_account(origin_sdk.as_deref(), identity.clone())?;
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
                self.runtime.validate_sdk(&instance)?;
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
        self.project_and_step(&mut owned, lifecycle, generation, event, None)
    }
    fn project_and_step(
        &self,
        owned: &mut Owned,
        lifecycle: LifecycleState,
        generation: Generation,
        event: Event,
        ended_ms: Option<i64>,
    ) -> Result<Vec<Command>, DriverError> {
        let now_ms = ended_ms.unwrap_or_else(|| self.clock.now_ms());
        let projection = self
            .runtime
            .policy_projection(lifecycle_kind(&lifecycle), generation.0)?;
        let owner = self.runtime.instance().unwrap_or_default();
        let mut actions = Vec::new();
        let sdk = projection.engine.as_ref().map(|e| e.instance.clone());
        if owner != owned.last_owner {
            owned.captures.clear();
            owned.issued_records.clear();
            owned.issued_quiet = None;
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
        let tracking_failed = projection.tracking_stopped
            && owned.state.tracking_stop.is_none()
            && !matches!(event, Event::TrackingWriteFailed { .. });
        if tracking_failed {
            let (state, emitted) = step(
                owned.state.clone(),
                Input {
                    now_ms,
                    facts: Some(projection.facts.clone()),
                    event: Event::TrackingWriteFailed { no_space: false },
                },
            );
            owned.state = state;
            actions.extend(emitted);
        }
        // A refresh cannot turn a still-failed Runtime into a successful local commit.
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
        // Durable rows are loaded after owner invalidation; old timestamps are never inferred.
        let (state, emitted) = step(
            owned.state.clone(),
            Input {
                now_ms,
                facts: None,
                event: Event::EvidenceCommitted(projection.evidence),
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
            match &action.kind {
                ActionKind::RecordExchange(record) => {
                    owned.issued_records.insert(record.exchange, record.clone());
                }
                ActionKind::RecordQuietPoint { at_ms } => owned.issued_quiet = Some((action.guard.clone(), *at_ms)),
                _ => {}
            }
        }
        Ok(actions
            .into_iter()
            .map(|action| Command {
                action,
                owner: owner.clone(),
                backend_guard: projection.backend_guard.clone(),
            })
            .collect())
    }
    pub fn validate(&self, command: &Command) -> Result<(), DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)
    }
    fn validate_locked(&self, owned: &Owned, command: &Command) -> Result<(), DriverError> {
        let runtime = self.runtime.adapter_guard();
        if owned.state.guard() != command.action.guard
            || command.owner != self.runtime.instance().unwrap_or_default()
            || runtime != command.backend_guard
            || command.backend_guard.generation != command.action.guard.generation
            || runtime.binding != command.action.guard.binding.map_or_else(Vec::new, |b| b.0.to_vec())
        {
            return Err(DriverError::StaleCommand);
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
        let mut owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        let guard = &command.backend_guard;
        let result = match &command.action.kind {
            ActionKind::UpdatePending(update) => self
                .runtime
                .policy_update_pending(guard, *update)
                .map(|_| Event::EvidenceCommitted(owned.state.evidence.clone())),
            ActionKind::ProbeTracking => self
                .runtime
                .policy_probe(guard)
                .map(|_| Event::TrackingChanged(ChangeCause::Probe)),
            ActionKind::ApplyReady { retry_failed } => {
                match self
                    .runtime
                    .policy_apply_ready(collection, guard, *retry_failed, timeout)
                {
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
                if owned.issued_quiet.as_ref() != Some(&(command.action.guard.clone(), *at_ms)) {
                    return Err(DriverError::UnqualifiedCompletion);
                }
                let current = self
                    .runtime
                    .policy_projection(lifecycle_kind(&lifecycle), collection.generation().0)?;
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
                    return Err(DriverError::StaleCommand);
                }
                self.runtime
                    .policy_record_quiet(guard, *at_ms)
                    .map(Event::EvidenceCommitted)
            }
            ActionKind::RecordExchange(record) => {
                if owned.issued_records.get(&record.exchange) != Some(record) {
                    return Err(DriverError::UnqualifiedCompletion);
                }
                self.runtime
                    .policy_record_exchange(collection, guard, &command.owner, record)
                    .map(Event::EvidenceCommitted)
            }
            ActionKind::RunCleanup => return Err(DriverError::CoreCapabilityUnavailable("cloud cleanup")),
            ActionKind::StatusChanged(_) => return Ok(Vec::new()),
            _ => return Err(DriverError::NotCoreAction),
        };
        match result {
            Ok(event) => self.project_and_step(&mut owned, lifecycle, collection.generation(), event, None),
            Err(error) => {
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
                    })
                    .collect();
                Err(DriverError::CoreActionFailed { error, commands })
            }
        }
    }
    /// Starts the existing Runtime capture, not a policy-owned selection.
    /// Call only after P12.3's gate wait/fetch prerequisites, never from an SDK handler.
    pub fn capture_exchange(&self, command: &Command, collection: &Collection) -> Result<u64, DriverError> {
        let mut owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        let ActionKind::RunExchange { exchange, .. } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        if let Some((_, id)) = owned.captures.get(&exchange) {
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
        let id = self.runtime.sync_now(&sdk, collection)?;
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
        let mut owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let (_, id) = owned.captures.get(&request).ok_or(DriverError::UnqualifiedCompletion)?;
        let (completed, ended_ms) = self
            .runtime
            .policy_completed_exchange(*id)
            .ok_or(DriverError::UnqualifiedCompletion)?;
        let projection = self
            .runtime
            .policy_projection(lifecycle_kind(&lifecycle), generation.0)?;
        let summary = &projection.facts.summary;
        let outcome = match completed.collection {
            CollectionOutcome::Succeeded => ExchangeOutcome::Succeeded {
                selected: completed.result.selected_names.len() as u64,
                held_back: summary.held,
                newer_pending: summary.pending_unheld,
                incoming_waiting: summary.incoming_waiting,
                media: summary.media.clone(),
            },
            CollectionOutcome::WaitingNoAccount => ExchangeOutcome::Waiting {
                class: OperationError::NoAccount,
                retry_after_ms: None,
            },
            CollectionOutcome::Incomplete => ExchangeOutcome::Incomplete {
                outstanding: completed.result.outstanding_names.len() as u64,
                blocked: summary.incoming_waiting,
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
        self.project_and_step(
            &mut owned,
            lifecycle,
            generation,
            Event::ExchangeEnded {
                exchange: request,
                outcome,
            },
            Some(ended_ms),
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
        let mut owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if owned.captures.contains_key(&request)
            || matches!(
                outcome,
                ExchangeOutcome::Succeeded { .. } | ExchangeOutcome::Incomplete { .. }
            )
        {
            return Err(DriverError::UnqualifiedCompletion);
        }
        self.project_and_step(
            &mut owned,
            lifecycle,
            generation,
            Event::ExchangeEnded {
                exchange: request,
                outcome,
            },
            None,
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
