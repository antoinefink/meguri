//! Event-driven local backup policy (package/backup-triggers T2/T3/T5).
//! Only value notifications run on the writer. Folder and sidecar reads, and
//! the one package writer, run on callers' workers or the existing job pool.
use crate::{
    CancelToken, Engine, EngineError, EventKind, Generation, LifecycleState, StopCause, WeakEngine,
    device_state::DeviceState,
    ports::{BackupFailureCause, BackupReceipt, BackupService, SuspendParticipant},
};
use meguri_model::{clock::Clock, time::EpochMillis};
use meguri_storage::{ReadRows, ReaderRole, StorageError};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, PoisonError, Weak},
    time::{Duration, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceTrigger {
    Opened,
    Deadline,
    Background,
    Quit,
    BackgroundProcessing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveBackup {
    pub receipt: BackupReceipt,
    pub deleted_names: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupError {
    CollectionCorrupt,
    Engine(EngineError),
    Writer {
        cause: BackupFailureCause,
        publication: crate::ports::BackupPublication,
    },
}
impl From<EngineError> for BackupError {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::SafetyBackupFailed(failure) => Self::Writer {
                cause: failure.cause,
                publication: failure.publication,
            },
            other => Self::Engine(other),
        }
    }
}
impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for BackupError {}

/// Signed epoch nanoseconds retain the OS timestamp's precision. Errors on an
/// individual entry's timestamp are ignored; directory-listing errors propagate.
pub trait BackupEntryTimes: Send + Sync {
    fn read(&self, directory: &Path) -> Result<Vec<i128>, EngineError>;
}
pub struct OsBackupEntryTimes;
impl BackupEntryTimes for OsBackupEntryTimes {
    fn read(&self, directory: &Path) -> Result<Vec<i128>, EngineError> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StorageError::from(e).into()),
        };
        let mut times = Vec::new();
        for entry in entries {
            let entry = entry.map_err(StorageError::from)?;
            // symlink_metadata counts the link itself, never the target.
            if let Ok(time) = fs::symlink_metadata(entry.path()).and_then(|m| m.created()) {
                times.push(match time.duration_since(UNIX_EPOCH) {
                    Ok(age) => age.as_nanos() as i128,
                    Err(age) => -(age.duration().as_nanos() as i128),
                });
            }
        }
        Ok(times)
    }
}

/// Exact T2 arithmetic, including u32 configuration values above the UI range.
/// Future entries do not block. The returned deadline rounds *up* to a clock
/// millisecond, so sub-millisecond creation times cannot cause an early backup.
pub fn interval_deadline(now: i64, minutes: u32, entries: &[i128]) -> Option<i64> {
    if minutes == 0 {
        return None;
    }
    let now_ns = i128::from(now) * 1_000_000;
    let interval = i128::from(minutes) * 60 * 1_000_000_000;
    entries
        .iter()
        .copied()
        .filter(|c| *c <= now_ns)
        .filter_map(|c| c.checked_add(interval))
        .filter(|end| *end > now_ns)
        .max()
        .map(|end| {
            let ms = end.div_euclid(1_000_000) + i128::from(end.rem_euclid(1_000_000) != 0);
            i64::try_from(ms).unwrap_or(i64::MAX)
        })
}

#[derive(Clone)]
struct Inputs {
    path: PathBuf,
    backups: PathBuf,
    clock: Arc<dyn Clock>,
    provider: Arc<dyn BackupService>,
    entries: Arc<dyn BackupEntryTimes>,
}
#[derive(Default)]
struct State {
    inputs: Option<Inputs>,
    epoch: u64,
    generation: Generation,
    ready: bool,
    suspended: bool,
    deadline: Option<i64>,
    reference: Option<i64>,
    current_mod: Option<i64>,
    revision: u64,
    last_attempt: Option<i64>,
    interval_until: Option<i64>,
    running: Option<Arc<CancelToken>>,
    waiting: Vec<Weak<CancelToken>>,
    active: usize,
    queued: bool,
    refresh: bool,
    dirty: bool,
}

pub struct Maintenance {
    engine: WeakEngine,
    stream: crate::ChangeStream,
    jobs: crate::JobRegistry,
    state: Mutex<State>,
    idle: Condvar,
    // Never taken by a writer or lifecycle notification.
    lane: crate::cancel::SerializationLane,
}
struct Waiting<'a>(&'a Maintenance, Arc<CancelToken>);
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0
            .lock()
            .waiting
            .retain(|token| !token.ptr_eq(&Arc::downgrade(&self.1)));
    }
}
struct Active<'a>(&'a Maintenance);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}
fn worker_only() -> Result<(), EngineError> {
    if crate::executor::writer::on_writer_thread() {
        Err(EngineError::InvalidArgument {
            message: "backup work must run off the writer".into(),
        })
    } else {
        Ok(())
    }
}

impl Maintenance {
    pub(crate) fn new(engine: WeakEngine, stream: crate::ChangeStream, jobs: crate::JobRegistry) -> Arc<Self> {
        Arc::new(Self {
            engine,
            stream,
            jobs,
            state: Mutex::new(State::default()),
            idle: Condvar::new(),
            lane: Default::default(),
        })
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    #[cfg(feature = "test-hooks")]
    pub fn wait_idle_for_testing(&self) {
        let deadline = crate::monotonic::now() + Duration::from_secs(10);
        let mut state = self.lock();
        while state.queued || state.active != 0 {
            let left = deadline.saturating_sub(crate::monotonic::now());
            assert!(!left.is_zero(), "backup maintenance did not settle");
            state = self
                .idle
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    pub fn deadline(&self) -> Option<EpochMillis> {
        self.lock().deadline.map(EpochMillis)
    }
    pub(crate) fn configure(&self, path: &Path, config: &crate::OpenConfig) {
        let mut state = self.lock();
        state.epoch += 1;
        state.inputs = config
            .backups_directory
            .as_ref()
            .zip(config.ports.backup_service.as_ref())
            .map(|(backups, provider)| Inputs {
                path: path.to_owned(),
                backups: backups.clone(),
                clock: config.clock.clone(),
                provider: provider.clone(),
                entries: config.backup_entry_times.clone(),
            });
        state.reference = None;
        state.current_mod = None;
        state.suspended = false;
    }
    fn publish(&self, state: &mut State, at: Option<i64>) {
        if state.deadline != at {
            state.deadline = at;
            self.stream
                .publish(state.generation, None, EventKind::MaintenanceDeadline { at });
        }
    }
    pub(crate) fn lifecycle(self: &Arc<Self>, lifecycle: &LifecycleState, generation: Generation) {
        let mut state = self.lock();
        state.generation = generation;
        state.ready = matches!(lifecycle, LifecycleState::Ready { .. });
        if !state.ready {
            self.publish(&mut state, None);
        }
        let ready = state.ready;
        drop(state);
        if ready {
            self.schedule(true);
        }
    }
    pub(crate) fn committed(self: &Arc<Self>, generation: Generation, col_mod: i64) {
        let mut state = self.lock();
        if state.generation != generation {
            return;
        }
        state.current_mod = Some(col_mod);
        state.revision += 1;
        if state.reference == Some(col_mod) {
            self.publish(&mut state, None);
        }
        let arm = state.ready && state.deadline.is_none() && state.reference != Some(col_mod);
        drop(state);
        if arm {
            self.schedule(false);
        }
    }
    /// Called at the end of every live backup, successful or failed, outside
    /// provider lanes and reader leases. The writer remains the sole publisher.
    pub fn backup_ended(self: &Arc<Self>) {
        // The synchronous trigger owns its terminal reevaluation.
        let running = self.lock().running.is_some();
        if !running {
            self.schedule(true);
        }
    }
    fn schedule(self: &Arc<Self>, refresh: bool) {
        let mut state = self.lock();
        if state.inputs.is_none() || !state.ready {
            return;
        }
        state.refresh |= refresh;
        state.dirty = true;
        if state.suspended {
            return;
        }
        if state.queued {
            return;
        }
        state.queued = true;
        let weak: Weak<Self> = Arc::downgrade(self);
        drop(state);
        self.jobs.maintenance_work(move || {
            if let Some(this) = weak.upgrade() {
                loop {
                    let refresh = {
                        let mut s = this.lock();
                        if s.suspended {
                            s.queued = false;
                            this.idle.notify_all();
                            break;
                        }
                        s.dirty = false;
                        std::mem::take(&mut s.refresh)
                    };
                    let _lane = this.lane.lock(None).expect("unbounded maintenance lane");
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| this.evaluate(refresh)))
                        .unwrap_or_else(|p| Err(EngineError::from_panic(p.as_ref())));
                    if let Err(error) = result {
                        tracing::warn!(%error, "backup deadline evaluation failed");
                    }
                    let mut s = this.lock();
                    if !s.dirty {
                        s.queued = false;
                        this.idle.notify_all();
                        break;
                    }
                }
            }
        });
    }
    fn begin(&self) -> Option<(Inputs, u64, u64)> {
        let mut state = self.lock();
        if !state.ready || state.suspended {
            return None;
        }
        let inputs = state.inputs.clone()?;
        state.active += 1;
        Some((inputs, state.epoch, state.revision))
    }
    fn finish(&self) {
        self.lock().active -= 1;
        self.idle.notify_all();
    }
    fn evaluate(&self, refresh: bool) -> Result<Option<i64>, EngineError> {
        self.evaluate_inner(refresh, false)
    }
    fn evaluate_inner(&self, refresh: bool, preserve_due: bool) -> Result<Option<i64>, EngineError> {
        let armed = self.lock().deadline;
        if !refresh && armed.is_some() {
            return Ok(armed);
        }
        let Some((inputs, epoch, revision)) = self.begin() else {
            let mut state = self.lock();
            if state.suspended && state.ready && state.inputs.is_some() {
                state.dirty = true;
                state.refresh |= refresh;
            }
            return Ok(state.deadline);
        };
        let _active = Active(self);
        (|| {
            let engine = self
                .engine
                .upgrade()
                .ok_or_else(|| EngineError::unavailable("engine dropped"))?;
            let collection = engine.collection()?;
            let lease = collection.reader(ReaderRole::Query)?;
            let col_mod = lease.col()?.modified.0;
            let minutes = crate::backup_limits::read(&*lease)?.minimum_interval_mins;
            drop(lease);
            let reference = DeviceState::for_collection(&inputs.path)
                .last_backup()
                .ok()
                .flatten()
                .map(|r| r.snapshot_mod);
            let mut state = self.lock();
            if state.epoch != epoch || !state.ready || state.suspended {
                if state.suspended && state.epoch == epoch && state.ready {
                    state.dirty = true;
                    state.refresh |= refresh;
                }
                return Ok(state.deadline);
            }
            if state.revision == revision {
                state.current_mod = Some(col_mod);
            }
            state.reference = reference;
            let changed = state.current_mod != reference || reference.is_none();
            if !changed {
                self.publish(&mut state, None);
                return Ok(None);
            }
            // Do not keep the notification mutex across filesystem I/O.
            drop(state);
            let entries = inputs.entries.read(&inputs.backups)?;
            let now = inputs.clock.now_ms();
            let mut state = self.lock();
            if state.epoch != epoch || !state.ready || state.suspended {
                if state.suspended && state.epoch == epoch && state.ready {
                    state.dirty = true;
                    state.refresh |= refresh;
                }
                return Ok(state.deadline);
            }
            if state.current_mod == state.reference && state.reference.is_some() {
                self.publish(&mut state, None);
            } else if refresh || state.deadline.is_none() {
                state.interval_until = interval_deadline(now, minutes, &entries);
                let interval = state.interval_until.unwrap_or(now);
                let floor = state.last_attempt.map(|at| at.saturating_add(300_000)).unwrap_or(now);
                let next = now.max(interval).max(floor);
                if !(preserve_due && next == now && state.deadline.is_some_and(|at| at <= now)) {
                    self.publish(&mut state, Some(next));
                }
            }
            Ok(state.deadline)
        })()
    }
    pub fn reevaluate(&self) -> Result<Option<EpochMillis>, EngineError> {
        worker_only()?;
        let _lane = self.lane.lock(None)?;
        self.evaluate(true).map(|at| at.map(EpochMillis))
    }
    /// Worker entry point. There is no engine timer; U133a/U154 own delivery.
    pub fn run_due(&self, trigger: MaintenanceTrigger) -> Result<Option<EpochMillis>, EngineError> {
        worker_only()?;
        let cancel = CancelToken::new();
        if trigger == MaintenanceTrigger::Quit {
            cancel.limit_until(crate::monotonic::now() + Duration::from_secs(2));
        }
        if trigger == MaintenanceTrigger::BackgroundProcessing {
            return Ok(self.deadline());
        }
        {
            let mut state = self.lock();
            if !state.ready || state.suspended {
                return Ok(state.deadline.map(EpochMillis));
            }
            state.waiting.push(Arc::downgrade(&cancel));
        }
        let waiting = Waiting(self, cancel.clone());
        let _lane = self.lane.lock(Some(&cancel))?;
        drop(waiting);
        cancel.check()?;
        let previous = self.lock().deadline;
        self.evaluate_inner(true, true)?;
        let Some((inputs, epoch, _)) = self.begin() else {
            return Ok(self.deadline());
        };
        let active = Active(self);
        let result: Result<bool, EngineError> = (|| {
            let now = inputs.clock.now_ms();
            let scheduled = matches!(trigger, MaintenanceTrigger::Deadline | MaintenanceTrigger::Opened);
            let (changed, floor) = {
                let s = self.lock();
                (
                    s.reference.is_none() || s.current_mod != s.reference,
                    s.last_attempt.is_none_or(|at| now >= at.saturating_add(300_000)),
                )
            };
            if !changed || (scheduled && (!floor || previous.is_some_and(|at| at > now))) {
                return Ok(false);
            }
            let engine = self
                .engine
                .upgrade()
                .ok_or_else(|| EngineError::unavailable("engine dropped"))?;
            let collection = engine.collection()?;
            if self.lock().interval_until.is_some_and(|at| at > now) {
                return Ok(false);
            }
            if scheduled && self.lock().deadline.is_some_and(|at| at > now) {
                return Ok(false);
            }
            cancel.check()?;
            let prior_attempt = {
                let mut s = self.lock();
                if s.epoch != epoch || !s.ready || s.suspended {
                    return Ok(false);
                }
                s.running = Some(cancel.clone());
                s.last_attempt.replace(now)
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                inputs.provider.backup_live(&collection, &cancel)
            }))
            .unwrap_or_else(|p| Err(EngineError::from_panic(p.as_ref())));
            let mut s = self.lock();
            s.running = None;
            let suspended_cancel = cancel.stop_cause() == Some(StopCause::System) && s.suspended;
            if suspended_cancel {
                s.last_attempt = prior_attempt;
            } else if let Err(error) = outcome {
                tracing::warn!(%error, "periodic backup failed");
            }
            Ok(!suspended_cancel)
        })();
        drop(active);
        if result? {
            self.evaluate(true)?;
        }
        Ok(self.deadline())
    }
}
impl SuspendParticipant for Maintenance {
    fn name(&self) -> &str {
        "backups"
    }
    fn flush(&self) -> Result<(), EngineError> {
        Ok(())
    }
    fn suspend_until(&self, deadline: Duration) -> Result<(), EngineError> {
        let mut state = self.lock();
        state.suspended = true;
        for cancel in state.waiting.iter().filter_map(Weak::upgrade) {
            cancel.interrupt();
        }
        if let Some(cancel) = &state.running {
            cancel.interrupt();
        }
        while state.active != 0 {
            let remaining = deadline.saturating_sub(crate::monotonic::now());
            if remaining.is_zero() {
                return Err(EngineError::SuspendDeadline {
                    phase: "backups".into(),
                });
            }
            state = self
                .idle
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Ok(())
    }
    fn resume(&self) -> Result<(), EngineError> {
        let mut state = self.lock();
        state.suspended = false;
        let pending = state.dirty;
        let refresh = state.refresh;
        drop(state);
        if pending && let Some(engine) = self.engine.upgrade() {
            engine.maintenance().schedule(refresh);
        }
        Ok(())
    }
}
impl Engine {
    pub fn maintenance(&self) -> &Arc<Maintenance> {
        self.inner.maintenance.get().expect("engine maintenance initialized")
    }
    pub fn run_due_maintenance(&self, trigger: MaintenanceTrigger) -> Result<Option<EpochMillis>, EngineError> {
        self.maintenance().run_due(trigger)
    }
    /// T2 alone, without scheduler cadence, for deterministic local adapters.
    /// Forced and periodic publication still use the one real live writer.
    pub fn backup_if_changed(&self, cancel: &CancelToken) -> Result<Option<LiveBackup>, BackupError> {
        worker_only()?;
        {
            let state = self.lock_state();
            if state.suspended || !matches!(state.state, LifecycleState::Ready { .. }) {
                return Ok(None);
            }
        }
        let inputs = self
            .maintenance()
            .lock()
            .inputs
            .clone()
            .ok_or_else(|| EngineError::unavailable("no backup provider"))?;
        let collection = self.collection()?;
        let lease = collection.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        let current_mod = snapshot.col().map_err(EngineError::from)?.modified.0;
        let minutes = crate::backup_limits::read(&*snapshot)?.minimum_interval_mins;
        drop(snapshot);
        drop(lease);
        let reference = DeviceState::for_collection(&inputs.path)
            .last_backup()
            .ok()
            .flatten()
            .map(|r| r.snapshot_mod);
        if reference == Some(current_mod) {
            return Ok(None);
        }
        let now = inputs.clock.now_ms();
        if interval_deadline(now, minutes, &inputs.entries.read(&inputs.backups)?).is_some() {
            return Ok(None);
        }
        self.back_up_now(cancel).map(Some)
    }
    /// Called on a background worker; it never takes the executor writer.
    pub fn back_up_now(&self, cancel: &CancelToken) -> Result<LiveBackup, BackupError> {
        worker_only()?;
        let (collection, provider) = {
            let state = self.lock_state();
            match &state.state {
                LifecycleState::Ready { .. } if !state.suspended => {}
                LifecycleState::Failed {
                    reason: crate::FailedReason::Corrupt { .. },
                } => return Err(BackupError::CollectionCorrupt),
                other => return Err(EngineError::unavailable(other.describe()).into()),
            }
            let current = state
                .current
                .as_ref()
                .ok_or_else(|| EngineError::unavailable("no collection"))?;
            (
                crate::Collection {
                    core: current.core.clone(),
                    generation: self.generation(),
                    privilege: None,
                    undo_session: None,
                },
                current
                    .core
                    .backup
                    .clone()
                    .ok_or_else(|| EngineError::unavailable("no backup provider"))?,
            )
        };
        provider.backup_live(&collection, cancel).map_err(Into::into)
    }
}
