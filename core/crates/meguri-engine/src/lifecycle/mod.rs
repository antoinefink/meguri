//! The engine and the collection lifecycle (C7, `specs/engine/lifecycle.md`):
//! open and close, generations, admission per state, the long-operation
//! slot, availability waits and the exclusive phase. One [`Engine`] lives for
//! the whole process (meguri-ffi holds it); it owns the change stream, the
//! thread registry and the job registry, and at most one open collection.

pub mod generation;
pub mod state;
pub mod wait;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use meguri_storage::{Initializers, OpenEvent, OpenOptions, open_collection};

use crate::changes::stream::{ChangeStream, EventKind, Seq};
use crate::collection::{Collection, CollectionCore};
use crate::error::EngineError;
use crate::executor::ExecShared;
use crate::executor::exclusive::{Admission, Privilege};
use crate::executor::writer::{self, Reopen};
use crate::jobs::registry::JobRegistry;
use crate::jobs::{JobContext, JobHandle, JobKind};
use crate::ports::Ports;
use crate::readers::Readers;
use crate::threads::{Qos, ThreadRegistry};

pub use generation::Generation;
pub use state::{ExclusivePhase, FailedReason, LifecycleState, LongOp};
pub use wait::SlotGuard;

/// What an engine is made with.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Worker threads of the job pool (`engine/jobs` J2).
    pub job_workers: usize,
}

impl Default for EngineConfig {
    fn default() -> EngineConfig {
        EngineConfig { job_workers: 4 }
    }
}

/// What opening a collection needs besides its path.
#[derive(Clone)]
pub struct OpenConfig {
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub publication_binding: crate::device_state::AccountBinding,
    /// The SQL functions every connection of the collection registers
    /// (`storage/open-and-connections` C5).
    pub initializers: Initializers,
    pub ports: Ports,
    pub backups_directory: Option<PathBuf>,
    pub disk_space_provider: Arc<dyn crate::DiskSpaceProvider>,
}

impl fmt::Debug for OpenConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenConfig")
            .field("clock", &self.clock)
            .field("ports", &self.ports)
            .finish_non_exhaustive()
    }
}

impl OpenConfig {
    pub fn new(clock: Arc<dyn Clock>, random: Arc<dyn RandomSource>) -> OpenConfig {
        OpenConfig {
            clock,
            random,
            publication_binding: crate::device_state::AccountBinding::LocalOnly,
            initializers: Initializers::default(),
            ports: Ports::default(),
            backups_directory: None,
            disk_space_provider: Arc::new(meguri_storage::space::StatvfsSpace),
        }
    }

    pub fn disk_space_provider(mut self, provider: Arc<dyn crate::DiskSpaceProvider>) -> Self {
        self.disk_space_provider = provider;
        self
    }

    pub fn backups_directory(mut self, directory: PathBuf) -> Self {
        self.backups_directory = Some(directory);
        self
    }

    pub fn ports(mut self, ports: Ports) -> OpenConfig {
        self.ports = ports;
        self
    }

    pub fn initializers(mut self, initializers: Initializers) -> OpenConfig {
        self.initializers = initializers;
        self
    }
}

/// The name of the day clock's open step (L2), as a failed open reports it.
pub const DAY_CLOCK_STEP: &str = "day_clock";

/// The engine. Clones share it.
#[derive(Clone)]
pub struct Engine {
    pub(crate) inner: Arc<EngineInner>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("state", &self.state())
            .field("generation", &self.generation())
            .finish()
    }
}

pub(crate) struct EngineInner {
    pub(crate) stream: ChangeStream,
    pub(crate) threads: ThreadRegistry,
    pub(crate) jobs: JobRegistry,
    generation: Arc<AtomicU64>,
    state: Mutex<LcState>,
    pub(crate) changed: Condvar,
    next_token: AtomicU64,
    pub(crate) control_lane: Mutex<()>,
}

pub(crate) struct LcState {
    pub(crate) suspended: bool,
    /// A failed participant flush must be retried before closing SQLite.
    pub(crate) suspend_flush_pending: bool,
    pub(crate) state: LifecycleState,
    pub(crate) slot: Option<wait::SlotHolder>,
    /// Whether `Available` was last published as true.
    pub(crate) available: bool,
    pub(crate) current: Option<Current>,
    /// Activated initial file whose open/recovery failed: keep advisory ownership.
    recovery_ownership: Option<Arc<meguri_storage::lockfile::CollectionLock>>,
}

/// The open collection.
pub(crate) struct Current {
    pub(crate) core: Arc<CollectionCore>,
    writer: Option<JoinHandle<()>>,
    ports: Ports,
    _lock: Arc<meguri_storage::lockfile::CollectionLock>,
    previous_clean_shutdown: bool,
    pub(crate) reopen: Reopen,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Engine, EngineError> {
        let stream = ChangeStream::new();
        let threads = ThreadRegistry::new();
        let jobs = JobRegistry::new(&threads, stream.clone(), config.job_workers.max(1))
            .map_err(|e| EngineError::internal(format!("cannot start the job pool: {e}")))?;
        Ok(Engine {
            inner: Arc::new(EngineInner {
                stream,
                threads,
                jobs,
                generation: Arc::new(AtomicU64::new(0)),
                state: Mutex::new(LcState {
                    suspended: false,
                    suspend_flush_pending: true,
                    state: LifecycleState::NoCollection,
                    slot: None,
                    available: false,
                    current: None,
                    recovery_ownership: None,
                }),
                changed: Condvar::new(),
                next_token: AtomicU64::new(1),
                control_lane: Mutex::new(()),
            }),
        })
    }

    pub fn stream(&self) -> &ChangeStream {
        &self.inner.stream
    }

    pub fn threads(&self) -> &ThreadRegistry {
        &self.inner.threads
    }

    pub fn jobs(&self) -> &JobRegistry {
        &self.inner.jobs
    }

    pub fn state(&self) -> LifecycleState {
        self.lock_state().state.clone()
    }

    /// The current generation; 0 before anything was opened.
    pub fn generation(&self) -> Generation {
        Generation(self.inner.generation.load(Ordering::SeqCst))
    }

    pub(crate) fn lock_state(&self) -> MutexGuard<'_, LcState> {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn wait_changed<'a>(
        &self,
        guard: MutexGuard<'a, LcState>,
        timeout: Duration,
    ) -> MutexGuard<'a, LcState> {
        self.inner
            .changed
            .wait_timeout(guard, timeout)
            .unwrap_or_else(PoisonError::into_inner)
            .0
    }

    pub(crate) fn new_token(&self) -> u64 {
        self.inner.next_token.fetch_add(1, Ordering::SeqCst)
    }

    pub(crate) fn ports(&self) -> Ports {
        self.lock_state()
            .current
            .as_ref()
            .map(|current| current.ports.clone())
            .unwrap_or_default()
    }

    /// Moves to `state`, publishes it, and updates availability.
    pub(crate) fn set_state(&self, state: LifecycleState) {
        let mut guard = self.lock_state();
        self.set_state_locked(&mut guard, state);
    }

    fn set_state_locked(&self, guard: &mut LcState, state: LifecycleState) {
        if guard.state == state {
            return;
        }
        guard.state = state.clone();
        self.inner
            .stream
            .publish(self.generation(), None, EventKind::Lifecycle(state));
        self.update_availability(guard);
    }

    /// Moves the exclusive phase of `core` to `phase`; nothing when `core`
    /// is no longer the open collection (it closed meanwhile).
    pub(crate) fn set_exclusive_phase(&self, core: &Arc<CollectionCore>, phase: ExclusivePhase) {
        let mut guard = self.lock_state();
        if !is_open(&guard, core) {
            return;
        }
        if let LifecycleState::ExclusiveBusy { op, cancellable, .. } = guard.state {
            self.set_state_locked(&mut guard, LifecycleState::ExclusiveBusy { op, phase, cancellable });
        }
    }

    /// A handle of the current generation, whenever a collection is open.
    pub fn collection(&self) -> Result<Collection, EngineError> {
        let state = self.lock_state();
        match &state.current {
            Some(current) => Ok(Collection {
                core: Arc::clone(&current.core),
                generation: self.generation(),
                privilege: None,
            }),
            None => Err(EngineError::unavailable(state.state.describe())),
        }
    }

    /// Opens the collection at `path` (`engine/lifecycle` L2). The engine's
    /// raw open entry point: only meguri-host calls it (`CoreHost::open`
    /// wires the ports), so layering-check refuses the name elsewhere.
    pub fn open_raw(&self, path: &Path, config: OpenConfig) -> Result<Collection, EngineError> {
        // Foreign host inputs are synchronous. A panic before the writer starts
        // must release the gate and permit another open. Started open steps are
        // caught below, where their writer can still be joined.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.open_raw_inner(path, config))).unwrap_or_else(
            |payload| {
                let error = EngineError::from_panic(payload.as_ref());
                self.set_state(LifecycleState::Failed {
                    reason: FailedReason::OpenStep {
                        step: "open".into(),
                        detail: error.to_string(),
                    },
                });
                Err(error)
            },
        )
    }

    fn open_raw_inner(&self, path: &Path, config: OpenConfig) -> Result<Collection, EngineError> {
        self.open_raw_owned(path, config, None)
    }

    // Initial verified replacement keeps the same advisory lock through open.
    pub(crate) fn open_raw_owned(
        &self,
        path: &Path,
        mut config: OpenConfig,
        retained: Option<Arc<meguri_storage::lockfile::CollectionLock>>,
    ) -> Result<Collection, EngineError> {
        // Compose here rather than in OpenConfig::new: callers can replace
        // its public initializer list. Reopen/resume keep this complete set.
        config.initializers.register(meguri_search::sqlfns::register);
        config.initializers.register(meguri_sched::sqlfns::register);
        {
            let _control = self.inner.control_lane.lock().unwrap_or_else(PoisonError::into_inner);
            let mut guard = self.lock_state();
            match guard.state {
                LifecycleState::NoCollection | LifecycleState::Failed { .. } if guard.current.is_none() => {}
                LifecycleState::Opening if retained.is_some() && guard.current.is_none() => {}
                _ => {
                    return Err(EngineError::InvalidArgument {
                        message: format!("cannot open a collection: {}", guard.state.describe()),
                    });
                }
            }
            self.set_state_locked(&mut guard, LifecycleState::Opening);
        }
        let initial_replacement = retained.is_some();
        let ownership = (|| -> Result<_, EngineError> {
            let directory = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let ownership = match retained {
                Some(ownership) => ownership,
                None => Arc::new(meguri_storage::lockfile::CollectionLock::acquire(directory)?),
            };
            crate::device_state::reconcile(path, &config.publication_binding)?;
            crate::device_state::ensure(path, &config.publication_binding, config.random.as_ref())?;
            if !initial_replacement {
                meguri_storage::leftovers::cleanup(directory)?;
            }
            meguri_storage::leftovers::cleanup_media(&directory.join("collection.media"))?;
            if let Some(backups) = &config.backups_directory {
                meguri_storage::paths::CollectionPaths::new(directory, backups)?;
                // The user may remove Backups in Files. Its forced writer
                // preflights capacity before recreating missing parents.
                match backups.symlink_metadata() {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(meguri_storage::StorageError::from(e).into()),
                    Ok(_) => {
                        if !initial_replacement {
                            meguri_storage::leftovers::cleanup(backups)?;
                        }
                    }
                }
            }
            Ok(ownership)
        })();
        let ownership = match ownership {
            Ok(lock) => lock,
            Err(error) => {
                self.set_state(LifecycleState::Failed {
                    reason: FailedReason::OpenStep {
                        step: "ownership".into(),
                        detail: error.to_string(),
                    },
                });
                return Err(error);
            }
        };
        self.jobs().pause(false);
        let progress = |event: OpenEvent| {
            let state = match event {
                OpenEvent::Opening => LifecycleState::Opening,
                OpenEvent::Upgrading { from, step } => LifecycleState::Upgrading { from, step },
            };
            self.set_state(state);
        };
        let options = OpenOptions::new(config.clock.as_ref(), config.random.as_ref())
            .initializers(config.initializers.clone())
            .progress(&progress)
            .managed();
        let opened = match open_collection(path, &options) {
            Ok(opened) => opened,
            Err(error) => {
                self.set_state(LifecycleState::Failed {
                    reason: FailedReason::Open(error.clone()),
                });
                return Err(EngineError::Open(error));
            }
        };
        self.set_state(LifecycleState::Opening);
        let previous_clean_shutdown = opened.previous_clean_shutdown;

        let generation = Generation(self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1);
        let privilege = Privilege(self.new_token());
        let last_seq = Seq(self.inner.stream.next_seq().0 - 1);
        let exec = Arc::new(ExecShared::new(
            Admission::Privileged {
                privilege,
                reason: "the collection is opening".to_owned(),
            },
            Arc::clone(&self.inner.generation),
            self.inner.stream.clone(),
            config.ports.observers.clone(),
            Arc::clone(&config.clock),
            path,
            last_seq,
        ));
        let core = Arc::new(CollectionCore {
            exec: Arc::clone(&exec),
            readers: Readers::new(opened.reader_factory()),
            derived: std::sync::OnceLock::new(),
            media: std::sync::Mutex::new(None),
            render_cache: Default::default(),
            space: config.disk_space_provider,
        });
        exec.undo.add_restorer(Arc::new(crate::sched::answer::AnswerRestorer));
        let reopen = Reopen {
            binding: config.publication_binding.clone(),
            clock: Arc::clone(&config.clock),
            random: Arc::clone(&config.random),
            initializers: config.initializers.clone(),
        };
        let current_reopen = reopen.clone();
        let writer_exec = Arc::clone(&exec);
        let writer = self
            .inner
            .threads
            .spawn_owned("meguri-writer", Qos::UserInitiated, "writer", move |activity| {
                writer::run(writer_exec, opened, reopen, last_seq, activity);
            })
            .map_err(|e| EngineError::internal(format!("cannot start the writer thread: {e}")));
        let writer = match writer {
            Ok(writer) => writer,
            Err(error) => {
                self.set_state(LifecycleState::Failed {
                    reason: FailedReason::OpenStep {
                        step: "writer".to_owned(),
                        detail: error.to_string(),
                    },
                });
                return Err(error);
            }
        };

        let privileged = Collection {
            core: Arc::clone(&core),
            generation,
            privilege: Some(privilege),
        };
        // The registered open steps, then the day clock's first check, which
        // writes and unburies before anything reads counts (L2,
        // `sched/day-clock` D8).
        type Step<'a> = (&'a str, Box<dyn Fn() -> Result<(), EngineError> + 'a>);
        let mut steps: Vec<Step<'_>> = config
            .ports
            .open_steps
            .iter()
            .map(|step| (step.name(), Box::new(|| step.run(&privileged)) as Box<dyn Fn() -> _>))
            .collect();
        steps.push((
            "fsrs_policy",
            Box::new(|| crate::sched::fsrs_policy::normalize_at_open(&privileged)),
        ));
        steps.push((DAY_CLOCK_STEP, Box::new(|| privileged.restart_day_clock().map(drop))));
        for (name, run) in steps {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                .unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())));
            if let Err(error) = result {
                if let Err(close_error) = shut_down(&core, Some(writer)) {
                    tracing::warn!(error = %close_error, "closing the collection after a failed open step");
                }
                self.set_state(LifecycleState::Failed {
                    reason: FailedReason::OpenStep {
                        step: name.to_owned(),
                        detail: error.to_string(),
                    },
                });
                return Err(error);
            }
        }
        let _control = self.inner.control_lane.lock().unwrap_or_else(PoisonError::into_inner);
        exec.set_admission(Admission::Open);
        let mut guard = self.lock_state();
        guard.recovery_ownership = None;
        guard.current = Some(Current {
            reopen: current_reopen,
            core: Arc::clone(&core),
            writer: Some(writer),
            ports: config.ports,
            _lock: ownership,
            previous_clean_shutdown,
        });
        guard.suspended = false;
        guard.suspend_flush_pending = true;
        self.set_state_locked(&mut guard, LifecycleState::Ready { generation });
        Ok(Collection {
            core,
            generation,
            privilege: None,
        })
    }

    /// Closes the collection (`engine/lifecycle` L10): queued ops commit,
    /// parked ops fail, readers close, the writer checkpoints and closes.
    /// The long-operation slot is freed: its guard, and an exclusive phase
    /// entered through it, no longer act (L5).
    /// Native destination selection cannot distinguish confirmation from an
    /// in-flight provider copy. Serialize replacement and close until completion.
    pub fn reserve_collection_export_handoff(&self) -> Result<SlotGuard, EngineError> {
        let _control = self.inner.control_lane.lock().unwrap_or_else(PoisonError::into_inner);
        self.acquire_slot(LongOp::CollectionExportHandoff)
    }

    pub fn close(&self) -> Result<(), EngineError> {
        let _control = self.inner.control_lane.lock().unwrap_or_else(PoisonError::into_inner);
        if self.slot_holder() == Some(LongOp::CollectionExportHandoff) {
            return Err(EngineError::Busy {
                running_op: LongOp::CollectionExportHandoff.name().into(),
            });
        }
        if matches!(self.state(), LifecycleState::Opening | LifecycleState::Upgrading { .. }) {
            return Err(EngineError::unavailable("the collection is opening"));
        }
        self.jobs().pause(true);
        for job in self.jobs().running() {
            job.cancel.interrupt();
        }
        let deadline = crate::monotonic::now() + Duration::from_secs(5);
        if self.lock_state().current.is_some() {
            self.quiesce_for_suspend(deadline)?;
        } else {
            self.jobs().wait_empty_until(deadline)?;
        }
        let current = {
            let mut guard = self.lock_state();
            let current = guard.current.take();
            guard.recovery_ownership = None;
            guard.suspended = false;
            guard.suspend_flush_pending = true;
            if current.is_some() {
                guard.slot = None;
                self.update_availability(&mut guard);
            }
            current
        };
        let Some(mut current) = current else {
            self.set_state(LifecycleState::NoCollection);
            self.jobs().pause(false);
            return Ok(());
        };
        let result = shut_down(&current.core, current.writer.take());
        self.set_state(LifecycleState::NoCollection);
        self.jobs().pause(false);
        result
    }

    pub fn previous_clean_shutdown(&self) -> Option<bool> {
        self.lock_state().current.as_ref().map(|c| c.previous_clean_shutdown)
    }

    /// Marks the open collection corrupt (U132): every op is refused, and
    /// only restore and the recovery download may take the slot.
    pub fn mark_corrupt(&self, detail: impl Into<String>) {
        let mut guard = self.lock_state();
        if let Some(current) = &guard.current {
            current.core.exec.set_admission(Admission::Closed {
                reason: "the collection is corrupt".to_owned(),
            });
        }
        self.set_state_locked(
            &mut guard,
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { detail: detail.into() },
            },
        );
    }

    /// Starts a job (`engine/jobs` J1, J3): one of the kinds that take the
    /// long-operation slot gets `Busy` at once while another holds it.
    pub fn start_job(
        &self,
        kind: JobKind,
        work: impl FnOnce(&JobContext) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<JobHandle, EngineError> {
        let _control = self.job_control_lane()?;
        if self.lock_state().suspended {
            return Err(EngineError::unavailable("the collection is suspended"));
        }
        if kind == JobKind::Open {
            self.jobs().pause(false);
        }
        if kind == JobKind::MediaSync && self.slot_holder() == Some(LongOp::OneWaySync) {
            return Err(EngineError::Busy {
                running_op: LongOp::OneWaySync.name().into(),
            });
        }
        let slot = match kind.long_op() {
            Some(op) => Some(self.acquire_slot(op)?),
            None => None,
        };
        Ok(self.inner.jobs.start(kind.name(), self.generation(), slot, work))
    }
}

/// Whether `core` is the open collection.
fn is_open(state: &LcState, core: &Arc<CollectionCore>) -> bool {
    state
        .current
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.core, core))
}

/// Stops admission, drains the queue, closes the readers and the writer, and
/// ends the writer thread.
fn shut_down(core: &CollectionCore, writer: Option<JoinHandle<()>>) -> Result<(), EngineError> {
    core.exec.set_admission(Admission::Closed {
        reason: "the collection is closing".to_owned(),
    });
    core.exec.wait_idle();
    core.readers.close_all();
    let result = core.exec.close();
    core.exec.stop();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    result
}

impl Drop for EngineInner {
    fn drop(&mut self) {
        let current = self
            .state
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .current
            .take();
        if let Some(mut current) = current
            && let Err(error) = shut_down(&current.core, current.writer.take())
        {
            tracing::warn!(%error, "closing the collection when the engine dropped");
        }
    }
}

/// An exclusive phase (`engine/lifecycle` L7): while it lasts only the ops
/// submitted through [`Exclusive::collection`] commit. Dropping it ends the
/// phase.
pub struct Exclusive<'s> {
    slot: &'s SlotGuard,
    privilege: Privilege,
    core: Arc<CollectionCore>,
    collection: Collection,
    /// The state to go back to when the phase ends without a replacement.
    prior: LifecycleState,
    replaced: bool,
}

impl fmt::Debug for Exclusive<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exclusive")
            .field("op", &self.slot.op)
            .field("generation", &self.collection.generation)
            .finish()
    }
}

impl<'s> Exclusive<'s> {
    /// The open collection `slot` may take over now: its core, ports and
    /// state.
    fn entry(slot: &SlotGuard, state: &LcState) -> Result<(Arc<CollectionCore>, Ports, LifecycleState), EngineError> {
        slot.check_held(state)?;
        let allowed = match &state.state {
            LifecycleState::Ready { .. } => true,
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { .. },
            } => slot.op.allowed_when_corrupt(),
            _ => false,
        };
        match (&state.current, allowed) {
            (Some(current), true) => Ok((Arc::clone(&current.core), current.ports.clone(), state.state.clone())),
            _ => Err(EngineError::unavailable(state.state.describe())),
        }
    }

    fn enter(slot: &'s SlotGuard, cancellable: bool) -> Result<Exclusive<'s>, EngineError> {
        let engine = &slot.engine;
        if !slot.op.replaces_collection() {
            return Err(EngineError::InvalidArgument {
                message: format!("{} does not replace the collection", slot.op.name()),
            });
        }
        let (core, ports, _) = Exclusive::entry(slot, &engine.lock_state())?;
        core.exec.review_accepting.store(false, Ordering::Release);
        for participant in &ports.suspend_participants {
            if let Err(error) = participant.flush() {
                core.exec.review_accepting.store(true, Ordering::Release);
                return Err(error);
            }
        }
        let privilege = Privilege(engine.new_token());
        let prior = {
            let mut guard = engine.lock_state();
            // Checked again: the collection may have closed or failed while
            // the participants flushed.
            let (again, _, prior) = match Exclusive::entry(slot, &guard) {
                Ok(entry) => entry,
                Err(error) => {
                    core.exec.review_accepting.store(true, Ordering::Release);
                    return Err(error);
                }
            };
            if !Arc::ptr_eq(&again, &core) {
                return Err(EngineError::unavailable("the collection closed"));
            }
            core.exec.set_admission(Admission::Privileged {
                privilege,
                reason: format!("{} is running", slot.op.name()),
            });
            engine.set_state_locked(
                &mut guard,
                LifecycleState::ExclusiveBusy {
                    op: slot.op,
                    phase: ExclusivePhase::Draining,
                    cancellable,
                },
            );
            prior
        };
        core.exec.wait_idle();
        engine.set_exclusive_phase(&core, ExclusivePhase::Working);
        let collection = Collection {
            core: Arc::clone(&core),
            generation: engine.generation(),
            privilege: Some(privilege),
        };
        Ok(Exclusive {
            slot,
            privilege,
            core,
            collection,
            prior,
            replaced: false,
        })
    }

    pub(crate) fn verified_environment(
        &self,
        reason: crate::replace::ReplaceReason,
    ) -> Result<Reopen, crate::replace::ReplaceError> {
        let state = self.slot.engine.lock_state();
        self.check_active(&state)?;
        if self.slot.op != reason.long_op() {
            return Err(crate::replace::ReplaceError::NotExclusive);
        }
        if reason.refuses_active_review() && self.core.exec.active_session.lock().unwrap().is_some() {
            return Err(EngineError::SessionActive.into());
        }
        Ok(state
            .current
            .as_ref()
            .expect("active exclusive has current")
            .reopen
            .clone())
    }

    /// The privileged handle: the only one whose ops commit during the phase.
    pub fn collection(&self) -> &Collection {
        &self.collection
    }

    /// Swaps the collection file (`engine/lifecycle` L8): `swap` runs with
    /// every connection closed (U124's verified replace). Returns the new
    /// generation's privileged handle. `CollectionUnavailable`, without
    /// running `swap`, once the phase has ended (the collection closed).
    ///
    /// When `swap` fails after it changed the file, the generation still
    /// moves and this returns the swap's error: [`Exclusive::collection`]
    /// then holds the new generation's handle.
    pub fn replace(
        &mut self,
        swap: impl FnOnce(&Path) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<Collection, EngineError> {
        self.replace_inner(crate::executor::writer::Swap {
            run: Box::new(swap),
            activated: None,
        })
    }

    pub(crate) fn replace_tracked(
        &mut self,
        swap: impl FnOnce(&Path) -> Result<(), EngineError> + Send + 'static,
        activated: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Collection, EngineError> {
        self.replace_inner(crate::executor::writer::Swap {
            run: Box::new(swap),
            activated: Some(activated),
        })
    }

    fn replace_inner(&mut self, swap: crate::executor::writer::Swap) -> Result<Collection, EngineError> {
        let engine = &self.slot.engine;
        self.check_active(&engine.lock_state())?;
        let replacement = generation::replace(engine, &self.core, self.privilege, swap);
        if let Some(collection) = &replacement.moved {
            self.replaced = true;
            self.collection = collection.clone();
            // The slot now belongs to the new generation.
            if let Some(holder) = engine.lock_state().slot.as_mut()
                && holder.id == self.slot.id
            {
                holder.generation = collection.generation;
            }
        }
        match (replacement.error, replacement.moved) {
            (None, Some(collection)) => Ok(collection),
            (Some(error), _) => Err(error),
            (None, None) => Err(EngineError::internal(
                "a replace reported neither a generation nor an error",
            )),
        }
    }

    /// Whether the phase still runs: its slot is held, its collection is
    /// open and in `ExclusiveBusy`.
    fn check_active(&self, state: &LcState) -> Result<(), EngineError> {
        self.slot.check_held(state)?;
        if is_open(state, &self.core) && matches!(state.state, LifecycleState::ExclusiveBusy { .. }) {
            Ok(())
        } else {
            Err(EngineError::unavailable(format!(
                "the exclusive phase of {} has ended: {}",
                self.slot.op.name(),
                state.state.describe()
            )))
        }
    }
}

impl Drop for Exclusive<'_> {
    fn drop(&mut self) {
        let engine = &self.slot.engine;
        let mut guard = engine.lock_state();
        if self.check_active(&guard).is_err() {
            // The collection closed, or the swap failed and left it closed:
            // the state is no longer this phase's.
            return;
        }
        let back = if self.replaced {
            LifecycleState::Ready {
                generation: engine.generation(),
            }
        } else {
            self.prior.clone()
        };
        let admission = match back {
            LifecycleState::Ready { .. } if !guard.suspended => Admission::Open,
            _ => Admission::Closed {
                reason: back.describe(),
            },
        };
        // Admit first, so nobody woken by the state change is refused.
        self.core
            .exec
            .review_accepting
            .store(matches!(admission, Admission::Open), Ordering::Release);
        self.core.exec.set_admission(admission);
        engine.set_state_locked(&mut guard, back);
    }
}
