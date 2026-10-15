//! The op executor (C1, `specs/engine/op-model.md`): one writer thread per
//! open collection, two FIFO queues, admission by collection state, and the
//! commit lock that ties seqs to snapshots.

pub mod catalog;
pub(crate) mod exclusive;
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

use meguri_model::clock::Clock;
use meguri_storage::Writer;

use crate::changes::stream::{ChangeStream, Seq};
use crate::error::EngineError;
use crate::lifecycle::Generation;
use crate::ports::ChangeObserver;
use exclusive::{Admission, Decision, Privilege};
use op::{OpContext, OpResult, OpSpec, Pending, WriteDelay};
use priority::Queues;
use writer::{CloseTask, MaintenanceTask, OpTask, ReplaceTask, Replaced, Task, TaskMeta};

/// What the writer thread and the submitters share.
pub(crate) struct ExecShared {
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
}

struct QueueState {
    queues: Queues,
    parked: Vec<Task>,
    admission: Admission,
    running: bool,
    stop: bool,
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
            queue: Mutex::new(QueueState {
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
            stream,
            observers,
            clock,
            path: path.to_path_buf(),
        }
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

    /// Queues, parks or refuses a task (`engine/lifecycle` L3, L4).
    pub(crate) fn submit(&self, task: Task) {
        let refused = {
            let mut queue = self.lock_queue();
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
                        queue.queues.push(task);
                        self.wake.notify_one();
                        None
                    }
                    Decision::Park => {
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

    /// The next task for the writer; `None` once stopped with empty queues.
    pub(crate) fn next_task(&self) -> Option<Task> {
        let mut queue = self.lock_queue();
        loop {
            if let Some(task) = queue.queues.pop_next() {
                queue.running = true;
                return Some(task);
            }
            if queue.stop {
                return None;
            }
            self.idle.notify_all();
            queue = self.wake.wait(queue).unwrap_or_else(PoisonError::into_inner);
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
        let mut meta = self.meta(spec.def.name(), spec.source);
        meta.planned = (!spec.any_generation).then_some(planned);
        meta.privilege = privilege;
        meta.park = spec.park;
        meta.cancel = spec.cancel.clone();
        meta.declared = spec.declared;
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
            result: Err(EngineError::internal("the writer stopped during a replace")),
            factory: None,
        })
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
