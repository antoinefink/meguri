//! The op executor (C1, `specs/engine/op-model.md`): one writer thread per
//! open collection, two FIFO queues, admission by collection state, and the
//! commit lock that ties seqs to snapshots.

pub mod catalog;
#[cfg(feature = "test-hooks")]
pub mod durability_events;
pub(crate) mod exclusive;
pub(crate) mod live_file;
pub mod op;
pub mod plan_commit;
pub mod preconditions;
pub(crate) mod priority;
pub mod source;
pub(crate) mod writer;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use meguri_model::clock::Clock;
use meguri_storage::Writer;

use crate::changes::stream::{ChangeStream, Seq};
use crate::error::EngineError;
use crate::lifecycle::Generation;
use crate::ports::ChangeObserver;
use crate::sched::day_clock::DayClock;
use crate::undo::UndoManager;
use exclusive::{Admission, Decision, Privilege};
use op::{OpContext, OpResult, OpSpec, Pending, WriteDelay};
use priority::Queues;
use writer::{CloseTask, MaintenanceTask, OpTask, ReplaceTask, Replaced, Task, TaskMeta};

#[cfg(feature = "test-hooks")]
type AfterCommitHook = Box<dyn FnOnce(Seq) + Send>;

/// What the writer thread and the submitters share.
pub(crate) struct ExecShared {
    pub(crate) review_journal: Mutex<Option<std::sync::Weak<crate::session::journal::Journal>>>,
    pub(crate) replaying_review: std::sync::atomic::AtomicBool,
    pub(crate) review_accepting: std::sync::atomic::AtomicBool,
    pub(crate) active_session: Mutex<Option<crate::SessionId>>,
    pub(crate) answer_caches: crate::sched::answer::AnswerCaches,
    #[cfg(feature = "test-hooks")]
    pub(crate) after_commit: Mutex<Option<AfterCommitHook>>,
    /// WAL syncs and write phases, recorded only after a test asks
    /// (`Collection::record_durability_events`), so long runs with test hooks
    /// on (benches, fault loops) keep no growing log.
    #[cfg(feature = "test-hooks")]
    durability_events: Mutex<Option<durability_events::DurabilityEvents>>,
    queue: Mutex<QueueState>,
    /// The writer waits here for tasks.
    wake: Condvar,
    /// Drain waiters wait here for empty queues and an idle writer.
    idle: Condvar,
    /// The commit lock and the last committed seq (O6).
    commit: Mutex<Seq>,
    delay: Mutex<Option<WriteDelay>>,
    /// The engine's current generation, shared with the lifecycle.
    pub(crate) generation_cell: Arc<AtomicU64>,
    pub(crate) stream: ChangeStream,
    pub(crate) observers: Vec<Arc<dyn ChangeObserver>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) path: PathBuf,
    /// The undo history (`engine/undo`), changed on the writer after each
    /// commit.
    pub(crate) undo: UndoManager,
    /// The collection's day clock (`sched/day-clock` D8).
    pub(crate) day_clock: Arc<DayClock>,
    pub(crate) fsrs_policy: Arc<crate::sched::fsrs_policy::PolicyState>,
}

struct QueueState {
    admitted: u64,
    queues: Queues,
    parked: Vec<Task>,
    admission: Admission,
    running: bool,
    stop: bool,
}

pub(crate) enum WriterWake {
    Task(Task),
    Flush,
    Stop,
}

impl ExecShared {
    pub(crate) fn new(
        admission: Admission,
        generation_cell: Arc<AtomicU64>,
        stream: ChangeStream,
        observers: Vec<Arc<dyn ChangeObserver>>,
        clock: Arc<dyn Clock>,
        path: &Path,
        last_seq: Seq,
    ) -> ExecShared {
        ExecShared {
            answer_caches: Default::default(),
            review_journal: Mutex::default(),
            active_session: Mutex::default(),
            review_accepting: std::sync::atomic::AtomicBool::new(true),
            replaying_review: std::sync::atomic::AtomicBool::new(false),
            #[cfg(feature = "test-hooks")]
            after_commit: Mutex::default(),
            #[cfg(feature = "test-hooks")]
            durability_events: Mutex::default(),
            queue: Mutex::new(QueueState {
                admitted: 0,
                queues: Queues::default(),
                parked: Vec::new(),
                admission,
                running: false,
                stop: false,
            }),
            wake: Condvar::new(),
            idle: Condvar::new(),
            commit: Mutex::new(last_seq),
            delay: Mutex::new(None),
            generation_cell,
            day_clock: Arc::new(DayClock::new(Arc::clone(&clock), stream.clone())),
            stream,
            observers,
            clock,
            path: path.to_path_buf(),
            undo: UndoManager::default(),
            fsrs_policy: Arc::new(crate::sched::fsrs_policy::PolicyState::load(path)),
        }
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn has_queued_op(&self, name: &str) -> bool {
        self.lock_queue().queues.contains_op(name)
    }

    fn lock_queue(&self) -> MutexGuard<'_, QueueState> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn lock_commit(&self) -> MutexGuard<'_, Seq> {
        self.commit.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn lock_delay(&self) -> MutexGuard<'_, Option<WriteDelay>> {
        self.delay.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn generation(&self) -> Generation {
        Generation(self.generation_cell.load(Ordering::SeqCst))
    }

    pub(crate) fn last_seq(&self) -> Seq {
        *self.lock_commit()
    }

    pub(crate) fn write_delay(&self) -> Option<WriteDelay> {
        *self.lock_delay()
    }

    #[cfg(feature = "test-hooks")]
    fn lock_durability(&self) -> MutexGuard<'_, Option<durability_events::DurabilityEvents>> {
        self.durability_events.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn start_durability_recording(&self) {
        self.lock_durability().get_or_insert_with(Default::default);
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn durability_events(&self) -> durability_events::DurabilityEvents {
        self.lock_durability().clone().unwrap_or_default()
    }

    /// Adds to the durability log when a test turned recording on.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn record_durability(&self, record: impl FnOnce(&mut durability_events::DurabilityEvents)) {
        if let Some(events) = self.lock_durability().as_mut() {
            record(events);
        }
    }

    /// Queues, parks or refuses a task (`engine/lifecycle` L3, L4). Once the
    /// writer is stopping every task is refused, control tasks too: nothing
    /// would ever run it (L10).
    pub(crate) fn submit(&self, task: Task) {
        let refused = {
            let mut queue = self.lock_queue();
            if queue.stop {
                let error = EngineError::unavailable("the collection is closed");
                drop(queue);
                task.body.fail(error);
                return;
            }
            let current = self.generation();
            match task.meta.planned {
                Some(planned) if !task.meta.control && planned != current => {
                    Some((task, EngineError::StaleGeneration { planned, current }))
                }
                _ if task.meta.control => {
                    queue.queues.push(task);
                    self.wake.notify_one();
                    None
                }
                _ => match queue.admission.decide(task.meta.privilege, task.meta.park) {
                    Decision::Queue => {
                        queue.admitted = queue.admitted.saturating_add(1);
                        queue.queues.push(task);
                        self.wake.notify_one();
                        None
                    }
                    Decision::Park => {
                        queue.admitted = queue.admitted.saturating_add(1);
                        queue.parked.push(task);
                        None
                    }
                    Decision::Refuse(error) => Some((task, error)),
                },
            }
        };
        if let Some((task, error)) = refused {
            task.body.fail(error);
        }
    }

    /// Sets what is admitted. Opening again releases the parked tasks: each
    /// planned on an older generation fails with `StaleGeneration`, the rest
    /// are queued. Closing fails them.
    pub(crate) fn set_admission(&self, admission: Admission) {
        let failed = {
            let mut guard = self.lock_queue();
            let queue = &mut *guard;
            queue.admission = admission.clone();
            let current = self.generation();
            let mut failed = Vec::new();
            for task in std::mem::take(&mut queue.parked) {
                match (&admission, task.meta.planned) {
                    (Admission::Closed { reason }, _) => {
                        let error = EngineError::unavailable(reason.clone());
                        failed.push((task, error));
                    }
                    (_, Some(planned)) if planned != current => {
                        failed.push((task, EngineError::StaleGeneration { planned, current }));
                    }
                    (Admission::Open, _) => {
                        queue.queues.push(task);
                        self.wake.notify_one();
                    }
                    (Admission::Privileged { .. }, _) => queue.parked.push(task),
                }
            }
            failed
        };
        for (task, error) in failed {
            task.body.fail(error);
        }
    }

    /// Sidecar mutations hold a reader lease while touching files; they obey
    /// the same admission tokens as writer operations.
    pub(crate) fn require_admission(&self, privilege: Option<Privilege>) -> Result<(), EngineError> {
        let queue = self.lock_queue();
        match queue.admission.decide(privilege, false) {
            Decision::Queue if !queue.stop => Ok(()),
            Decision::Refuse(error) => Err(error),
            _ => Err(EngineError::unavailable("the collection cannot admit sidecar work")),
        }
    }

    pub(crate) fn transfer_admission(
        &self,
        privilege: Option<Privilege>,
    ) -> Result<crate::collection::TransferAdmission, EngineError> {
        let queue = self.lock_queue();
        match queue.admission.decide(privilege, false) {
            Decision::Queue if !queue.stop && queue.admitted != u64::MAX => {}
            _ => return Err(EngineError::unavailable("transfer admission is closed")),
        }
        Ok(crate::collection::TransferAdmission {
            admitted: queue.admitted,
            committed: self.last_seq().0,
            pending: queue.running || !queue.queues.is_empty() || !queue.parked.is_empty(),
        })
    }

    /// Sleeps indefinitely when clean, or until the pending WAL flush is due.
    /// Check the deadline even with queued tasks so traffic cannot starve it.
    pub(crate) fn next_task(&self, flush_at: Option<Duration>) -> WriterWake {
        let mut queue = self.lock_queue();
        loop {
            let wait = flush_at.map(|at| at.saturating_sub(crate::monotonic::now()));
            if wait == Some(Duration::ZERO) {
                return WriterWake::Flush;
            }
            if let Some(task) = queue.queues.pop_next() {
                queue.running = true;
                return WriterWake::Task(task);
            }
            if queue.stop {
                return WriterWake::Stop;
            }
            self.idle.notify_all();
            queue = match wait {
                Some(wait) => {
                    self.wake
                        .wait_timeout(queue, wait)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self.wake.wait(queue).unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    pub(crate) fn task_done(&self) {
        let mut queue = self.lock_queue();
        queue.running = false;
        if queue.queues.is_empty() {
            self.idle.notify_all();
        }
    }

    /// Waits until the queues are empty and the writer is idle: the drain of
    /// `engine/lifecycle` L7.
    pub(crate) fn wait_idle(&self) {
        let mut queue = self.lock_queue();
        while queue.running || !queue.queues.is_empty() {
            queue = self.idle.wait(queue).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Lets the writer thread end once its queues are empty.
    pub(crate) fn stop(&self) {
        self.lock_queue().stop = true;
        self.wake.notify_all();
    }

    fn meta(&self, name: &'static str, spec_source: meguri_model::changes::OpSource) -> TaskMeta {
        TaskMeta {
            name,
            source: spec_source,
            queue: source::queue_of(spec_source),
            planned: None,
            privilege: None,
            park: false,
            cancel: None,
            declared: None,
            restore: None,
            submitted: crate::monotonic::now(),
            control: false,
        }
    }

    /// Submits an op (O1 to O8).
    pub(crate) fn submit_op<T, F>(
        &self,
        spec: OpSpec,
        planned: Generation,
        privilege: Option<Privilege>,
        op: F,
    ) -> Pending<OpResult<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
    {
        let (reply, rx) = mpsc::sync_channel(1);
        if spec.declared.is_some() && !spec.def.may_declare_long_write() {
            let _ = reply.send(Err(EngineError::InvalidArgument {
                message: format!(
                    "op {} may not declare a long write: it is not in the op catalog (engine/op-model O9)",
                    spec.def.name()
                ),
            }));
            return Pending { rx };
        }
        if spec.undo.skip && spec.source != meguri_model::changes::OpSource::UserNonUndoable {
            let _ = reply.send(Err(EngineError::InvalidArgument {
                message: format!(
                    "op {} of source {} may not skip undo: only UserNonUndoable ops may (engine/undo M2)",
                    spec.def.name(),
                    spec.source
                ),
            }));
            return Pending { rx };
        }
        let mut meta = self.meta(spec.def.name(), spec.source);
        meta.planned = (!spec.any_generation).then_some(planned);
        meta.privilege = privilege;
        meta.park = spec.park;
        meta.cancel = spec.cancel.clone();
        meta.declared = spec.declared;
        meta.restore = spec
            .undo
            .action
            .map(|action| crate::undo::Restore { action, target: None });
        self.submit(Task {
            meta,
            body: Box::new(OpTask { spec, op, reply }),
        });
        Pending { rx }
    }

    /// Submits a maintenance task (O8): background, with its declared
    /// duration published while it runs.
    pub(crate) fn submit_maintenance<T, F>(
        &self,
        def: catalog::OpDef,
        declared: std::time::Duration,
        planned: Generation,
        privilege: Option<Privilege>,
        work: F,
    ) -> Pending<T>
    where
        T: Send + 'static,
        F: FnOnce(&Writer) -> Result<T, EngineError> + Send + 'static,
    {
        let (reply, rx) = mpsc::sync_channel(1);
        if !def.is_maintenance_task() {
            let _ = reply.send(Err(EngineError::InvalidArgument {
                message: format!("{} is not a maintenance task of the op catalog", def.name()),
            }));
            return Pending { rx };
        }
        let mut meta = self.meta(def.name(), meguri_model::changes::OpSource::Maintenance);
        meta.planned = Some(planned);
        meta.privilege = privilege;
        meta.declared = Some(declared);
        self.submit(Task {
            meta,
            body: Box::new(MaintenanceTask { work, reply }),
        });
        Pending { rx }
    }

    /// Runs the replace of `engine/lifecycle` L8 on the writer and waits.
    pub(crate) fn replace(&self, swap: writer::Swap) -> Replaced {
        let (reply, rx) = mpsc::sync_channel(1);
        let mut meta = self.meta("replace_collection", meguri_model::changes::OpSource::Maintenance);
        meta.control = true;
        meta.queue = source::QueueKind::Interactive;
        self.submit(Task {
            meta,
            body: Box::new(ReplaceTask { swap, reply }),
        });
        rx.recv().unwrap_or(Replaced {
            reopened: false,
            error: Some(EngineError::internal("the writer stopped during a replace")),
            generation: None,
            factory: None,
        })
    }

    pub(crate) fn control(&self, action: crate::suspend::Control) -> Pending<crate::suspend::ControlReply> {
        let (reply, rx) = mpsc::sync_channel(1);
        let mut meta = self.meta("durability_control", meguri_model::changes::OpSource::Maintenance);
        meta.control = true;
        self.submit(Task {
            meta,
            body: Box::new(writer::ControlTask { action, reply }),
        });
        Pending { rx }
    }

    pub(crate) fn wait_idle_until(&self, deadline: Duration) -> Result<(), EngineError> {
        let mut queue = self.lock_queue();
        while queue.running || !queue.queues.is_empty() {
            let remaining = deadline.saturating_sub(crate::monotonic::now());
            if remaining.is_zero() {
                return Err(EngineError::SuspendDeadline {
                    phase: "writer drain".into(),
                });
            }
            queue = self
                .idle
                .wait_timeout(queue, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Ok(())
    }

    /// Closes the collection on the writer and waits.
    pub(crate) fn close(&self) -> Result<(), EngineError> {
        let (reply, rx) = mpsc::sync_channel(1);
        let mut meta = self.meta("close_collection", meguri_model::changes::OpSource::Maintenance);
        meta.control = true;
        meta.queue = source::QueueKind::Interactive;
        self.submit(Task {
            meta,
            body: Box::new(CloseTask { reply }),
        });
        rx.recv()
            .unwrap_or_else(|_| Err(EngineError::internal("the writer stopped during close")))
    }
}
