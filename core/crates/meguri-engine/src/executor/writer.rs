//! The writer thread (`engine/op-model` O2 to O8): it owns the collection's
//! writer connection and runs one task at a time: ops in their write scope,
//! maintenance tasks outside any transaction, and the lifecycle's control
//! tasks (replace, close).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use meguri_storage::{Initializers, OpenOptions, OpenedCollection, ReaderFactory, Writer, open_collection};

use crate::cancel::CancelToken;
use crate::changes::observers::{self, ObservedCommit};
use crate::changes::stream::{EventKind, Seq};
use crate::changes::{self, MediaNotes};
use crate::error::EngineError;
use crate::executor::exclusive::Privilege;
use crate::executor::op::{OpContext, OpResult, OpSpec, OpTiming, UndoMeta, WriteDelay};
use crate::executor::preconditions::{self, TouchIndex};
use crate::executor::source::QueueKind;
use crate::executor::{ExecShared, WriterWake};
use crate::lifecycle::Generation;
use crate::monotonic;
use crate::progress::ProgressPhase;
use crate::threads::ThreadActivity;

/// One item of the writer's queues.
pub(crate) struct Task {
    pub meta: TaskMeta,
    pub body: Box<dyn TaskBody>,
}

pub(crate) struct TaskMeta {
    pub name: &'static str,
    pub source: OpSource,
    pub queue: QueueKind,
    pub planned: Option<Generation>,
    pub privilege: Option<Privilege>,
    pub park: bool,
    pub cancel: Option<Arc<CancelToken>>,
    pub declared: Option<Duration>,
    pub submitted: Duration,
    /// Control tasks bypass admission and the generation check.
    pub control: bool,
}

pub(crate) trait TaskBody: Send {
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, meta: &TaskMeta);
    fn fail(self: Box<Self>, error: EngineError);
}

/// What the writer thread owns and lends to each task.
pub(crate) struct WriterEnv<'w> {
    pub shared: &'w ExecShared,
    pub collection: &'w mut Option<OpenedCollection>,
    pub touched: &'w mut TouchIndex,
    pub reopen: &'w Reopen,
}

/// What reopening the file after a swap needs (`engine/lifecycle` L8).
pub(crate) struct Reopen {
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub initializers: Initializers,
}

/// The writer thread's loop: runs tasks until the executor stops and its
/// queues are empty.
pub(crate) fn run(
    shared: Arc<ExecShared>,
    collection: OpenedCollection,
    reopen: Reopen,
    floor: Seq,
    activity: &ThreadActivity,
) {
    let mut collection = Some(collection);
    let mut touched = TouchIndex::new(floor);
    let mut flush = WalFlush::default();
    loop {
        flush.observe(&shared, collection.as_ref());
        let task = match shared.next_task(flush.due) {
            WriterWake::Task(task) => task,
            WriterWake::Flush => {
                let _busy = activity.busy();
                flush.sync(&shared, collection.as_ref());
                continue;
            }
            WriterWake::Stop => break,
        };
        {
            let _busy = activity.busy();
            // A catalog long write/maintenance task may hold the thread past
            // the deadline. Cover earlier commits before it starts.
            let may_run_long = task.meta.declared.is_some()
                || super::catalog::lookup(task.meta.name)
                    .is_some_and(|def| def.may_declare_long_write() || def.is_maintenance_task());
            if may_run_long && flush.due.is_some() {
                flush.sync(&shared, collection.as_ref());
            }
            let Task { meta, body } = task;
            // A failed replace can still reopen a new writer in the same
            // generation. DATA_VERSION belongs to a connection, not a file.
            if meta.control {
                flush.version = None;
            }
            if let Some(error) = refusal(&shared, &meta) {
                body.fail(error);
            } else {
                let mut env = WriterEnv {
                    shared: &shared,
                    collection: &mut collection,
                    touched: &mut touched,
                    reopen: &reopen,
                };
                // A bug in a task must not end the writer: the task's result
                // channel drops, so its caller gets an internal error.
                let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body.run(&mut env, &meta)));
                if let Err(payload) = ran {
                    let message = crate::error::panic_message(payload.as_ref());
                    tracing::error!(task = meta.name, %message, "a writer task panicked");
                }
            }
        }
        shared.task_done();
    }
    if let Some(collection) = collection.take()
        && let Err(error) = collection.close()
    {
        tracing::warn!(%error, "closing the collection when the writer stopped");
    }
}

/// Group nearby commits. Start early enough to leave room for a normal
/// write phase (~200 ms, O8) and the full sync before C9.2's one-second bound.
const WAL_FLUSH_DELAY: Duration = Duration::from_millis(750);
/// The first retry after a failed sync; each further failure doubles it up
/// to the maximum, so a lasting I/O error does not spin the writer.
const WAL_FLUSH_RETRY: Duration = Duration::from_millis(50);
const WAL_FLUSH_RETRY_MAX: Duration = Duration::from_secs(1);

#[derive(Default)]
struct WalFlush {
    version: Option<(Generation, u32)>,
    due: Option<Duration>,
    /// Failed syncs since the last one that succeeded.
    failures: u32,
}

impl WalFlush {
    fn observe(&mut self, shared: &ExecShared, collection: Option<&OpenedCollection>) {
        let Some(collection) = collection else {
            *self = Self::default();
            return;
        };
        match collection.writer().data_version() {
            Ok(version) => {
                let version = (shared.generation(), version);
                if self.version != Some(version) {
                    // Also covers commits made by the open gate or maintenance.
                    self.due.get_or_insert_with(|| monotonic::now() + WAL_FLUSH_DELAY);
                    self.version = Some(version);
                }
            }
            Err(error) => {
                tracing::error!(%error, "reading the writer's WAL durability token");
                self.due.get_or_insert_with(|| monotonic::now() + WAL_FLUSH_RETRY);
            }
        }
    }

    fn sync(&mut self, _shared: &ExecShared, collection: Option<&OpenedCollection>) {
        let Some(collection) = collection else {
            *self = Self::default();
            return;
        };
        // No other commit can run on this writer during xSync. Only a
        // successful call advances coverage; a failure keeps every commit due.
        #[cfg(feature = "test-hooks")]
        let (started, through) = (monotonic::now(), _shared.last_seq());
        let result = collection.writer().sync_wal();
        #[cfg(feature = "test-hooks")]
        if !matches!(result, Ok(false)) {
            let completed = monotonic::now();
            _shared.record_durability(|events| {
                events.syncs.push(super::durability_events::WalSync {
                    started,
                    completed,
                    through,
                    succeeded: result.is_ok(),
                });
            });
        }
        match result {
            Ok(_) => {
                self.due = None;
                self.failures = 0;
            }
            Err(error) => {
                self.failures = self.failures.saturating_add(1);
                let backoff = WAL_FLUSH_RETRY
                    .saturating_mul(1 << (self.failures - 1).min(5))
                    .min(WAL_FLUSH_RETRY_MAX);
                tracing::error!(%error, failures = self.failures, "full sync of the collection WAL failed; retrying");
                self.due = Some(monotonic::now() + backoff);
            }
        }
    }
}

/// Why a dequeued task must not run: cancelled while it waited, or planned on
/// an older generation.
fn refusal(shared: &ExecShared, meta: &TaskMeta) -> Option<EngineError> {
    if meta.control {
        return None;
    }
    if meta.cancel.as_ref().is_some_and(|token| token.is_cancelled()) {
        return Some(EngineError::Cancelled);
    }
    let current = shared.generation();
    match meta.planned {
        Some(planned) if planned != current => Some(EngineError::StaleGeneration { planned, current }),
        _ => None,
    }
}

/// An op: its closure, its spec and where its result goes.
pub(crate) struct OpTask<T, F> {
    pub spec: OpSpec,
    pub op: F,
    pub reply: SyncSender<Result<OpResult<T>, EngineError>>,
}

impl<T, F> TaskBody for OpTask<T, F>
where
    T: Send + 'static,
    F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
{
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, meta: &TaskMeta) {
        let OpTask { spec, op, reply } = *self;
        let result = run_op(env, meta, spec, op);
        let _ = reply.send(result);
    }

    fn fail(self: Box<Self>, error: EngineError) {
        let _ = self.reply.send(Err(error));
    }
}

/// O3 to O8: one write scope, preconditions, the closure, commit with a seq,
/// the change set, the observers, publication.
fn run_op<T>(
    env: &mut WriterEnv<'_>,
    meta: &TaskMeta,
    spec: OpSpec,
    op: impl FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError>,
) -> Result<OpResult<T>, EngineError> {
    let started = monotonic::now();
    let span = tracing::info_span!(
        "op",
        name = meta.name,
        source = meta.source.as_str(),
        rows = tracing::field::Empty,
        plan_ms = tracing::field::Empty,
        write_ms = tracing::field::Empty,
    );
    let _entered = span.enter();
    if let Some(progress) = &spec.progress {
        progress.set_phase(ProgressPhase::Committing);
    }
    let shared = env.shared;
    let delay = meta
        .declared
        .map(|declared| DelayGuard::start(shared, meta.name, declared));
    let Some(collection) = env.collection.as_ref() else {
        return Err(EngineError::unavailable("the collection is not open"));
    };
    let generation = shared.generation();
    let mut scope = collection.writer().begin_write(
        spec.stamp_mode(),
        shared.clock.as_ref(),
        Some(collection.journal_options()),
    )?;
    preconditions::check(&spec.preconditions, &scope, env.touched)?;

    let mut context = OpContext {
        scope: &mut scope,
        source: spec.source,
        generation,
        cancel: spec.cancel.clone(),
        cancellable_write: spec.def.cancellable_write(),
        progress: spec.progress.clone(),
        media_added: Vec::new(),
        media_removed: Vec::new(),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(&mut context)));
    let media = MediaNotes {
        added: std::mem::take(&mut context.media_added),
        removed: std::mem::take(&mut context.media_removed),
    };
    let value = match outcome {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            scope.rollback()?;
            return Err(error);
        }
        Err(payload) => {
            let error = EngineError::from_panic(payload.as_ref());
            scope.rollback()?;
            return Err(error);
        }
    };

    // Commit and take the seq under the commit lock, so a snapshot begun
    // under it knows exactly which commits it sees (O6).
    let (committed, reservation) = {
        let mut last = shared.lock_commit();
        let committed = scope.commit()?;
        let reservation = shared.stream.reserve();
        *last = reservation.seq();
        (committed, reservation)
    };
    let seq = reservation.seq();
    let observing = !shared.observers.is_empty();
    let derived = changes::derive(committed.journal.as_ref(), spec.source, media, observing);
    if let Some(journal) = &committed.journal {
        env.touched
            .record(seq, journal.entries().map(|e| (e.table(), e.key())), journal.len());
    }
    let change_set = Arc::new(derived.changes);
    if change_set.is_empty() {
        reservation.give_up();
    } else {
        if observing {
            observers::notify(
                &shared.observers,
                &ObservedCommit {
                    seq,
                    generation,
                    changes: &change_set,
                    cards: &derived.cards,
                    revlog_added: &derived.revlog_added,
                    revlog_removed: &derived.revlog_removed,
                    config: &derived.config,
                },
            );
        }
        reservation.fill(generation, None, EventKind::Changes(Arc::clone(&change_set)));
    }
    drop(delay);

    let write = monotonic::now().saturating_sub(started);
    #[cfg(feature = "test-hooks")]
    shared.record_durability(|events| {
        events.writes.push(super::durability_events::WritePhase {
            started,
            completed: monotonic::now(),
        });
    });
    span.record("rows", derived.rows);
    span.record("write_ms", write.as_secs_f64() * 1000.0);
    if let Some(plan) = spec.plan_time {
        span.record("plan_ms", plan.as_secs_f64() * 1000.0);
    }
    let undo = (spec.source == OpSource::UserUndoable && !change_set.is_empty()).then_some(UndoMeta {
        name: meta.name,
        source: spec.source,
    });
    Ok(OpResult {
        value,
        seq,
        changes: change_set,
        undo,
        generation,
        past_cancel_point: spec.cancel.as_ref().is_some_and(|token| token.is_cancelled()),
        timing: OpTiming {
            queued: started.saturating_sub(meta.submitted),
            plan: spec.plan_time,
            write,
            rows: derived.rows,
        },
    })
}

/// Publishes a declared delay while it lives (O8).
struct DelayGuard<'s> {
    shared: &'s ExecShared,
    op: &'static str,
}

impl<'s> DelayGuard<'s> {
    fn start(shared: &'s ExecShared, op: &'static str, declared: Duration) -> DelayGuard<'s> {
        *shared.lock_delay() = Some(WriteDelay { op, declared });
        shared
            .stream
            .publish(shared.generation(), None, EventKind::WriteDelay { op, declared });
        DelayGuard { shared, op }
    }
}

impl Drop for DelayGuard<'_> {
    fn drop(&mut self) {
        *self.shared.lock_delay() = None;
        self.shared.stream.publish(
            self.shared.generation(),
            None,
            EventKind::WriteDelayEnded { op: self.op },
        );
    }
}

/// A maintenance task: writer work outside any transaction (O8).
pub(crate) struct MaintenanceTask<T, F> {
    pub work: F,
    pub reply: SyncSender<Result<T, EngineError>>,
}

impl<T, F> TaskBody for MaintenanceTask<T, F>
where
    T: Send + 'static,
    F: FnOnce(&Writer) -> Result<T, EngineError> + Send + 'static,
{
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, meta: &TaskMeta) {
        let MaintenanceTask { work, reply } = *self;
        let result = match env.collection.as_ref() {
            None => Err(EngineError::unavailable("the collection is not open")),
            Some(collection) => {
                let _delay = meta
                    .declared
                    .map(|declared| DelayGuard::start(env.shared, meta.name, declared));
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(collection.writer())))
                    .unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())))
            }
        };
        let _ = reply.send(result);
    }

    fn fail(self: Box<Self>, error: EngineError) {
        let _ = self.reply.send(Err(error));
    }
}

/// What a replace leaves (`engine/lifecycle` L8).
pub(crate) struct Replaced {
    /// The swap or the reopen failed; `factory` says whether a collection is
    /// open again.
    pub result: Result<Generation, EngineError>,
    /// Readers of the collection open now: the new file, or the old one when
    /// the swap failed and it was reopened; `None` when nothing is open.
    pub factory: Option<ReaderFactory>,
}

/// The swap of `engine/lifecycle` L8: runs with the collection path and
/// nothing open (U124's verified replace).
pub(crate) type Swap = Box<dyn FnOnce(&Path) -> Result<(), EngineError> + Send>;

/// Closes the collection, swaps the file, reopens it through the open gate
/// and moves to the next generation.
pub(crate) struct ReplaceTask {
    pub swap: Swap,
    pub reply: SyncSender<Replaced>,
}

impl TaskBody for ReplaceTask {
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, _meta: &TaskMeta) {
        let ReplaceTask { swap, reply } = *self;
        let shared = env.shared;
        // Closed before the replace got here: there is nothing to replace,
        // and the swap must not touch a file nobody holds any more.
        let Some(collection) = env.collection.take() else {
            let _ = reply.send(Replaced {
                result: Err(EngineError::unavailable("the collection is closed")),
                factory: None,
            });
            return;
        };
        if let Err(error) = collection.close() {
            tracing::warn!(%error, "closing the collection before a swap");
        }
        let swapped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| swap(&shared.path)))
            .unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())));
        let reopen = env.reopen;
        let options =
            OpenOptions::new(reopen.clock.as_ref(), reopen.random.as_ref()).initializers(reopen.initializers.clone());
        let replaced = match open_collection(&shared.path, &options) {
            Err(error) => Replaced {
                result: Err(match swapped {
                    Err(swap_error) => swap_error,
                    Ok(()) => EngineError::Open(error),
                }),
                factory: None,
            },
            Ok(collection) => {
                let factory = Some(collection.reader_factory());
                *env.collection = Some(collection);
                env.touched.reset(*shared.lock_commit());
                match swapped {
                    Err(error) => Replaced {
                        result: Err(error),
                        factory,
                    },
                    Ok(()) => {
                        let generation = Generation(shared.generation_cell.fetch_add(1, Ordering::SeqCst) + 1);
                        shared
                            .stream
                            .publish(generation, None, EventKind::CollectionReplaced { generation });
                        Replaced {
                            result: Ok(generation),
                            factory,
                        }
                    }
                }
            }
        };
        let _ = reply.send(replaced);
    }

    fn fail(self: Box<Self>, error: EngineError) {
        let _ = self.reply.send(Replaced {
            result: Err(error),
            factory: None,
        });
    }
}

/// Closes the collection (`engine/lifecycle` L10).
pub(crate) struct CloseTask {
    pub reply: SyncSender<Result<(), EngineError>>,
}

impl TaskBody for CloseTask {
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, _meta: &TaskMeta) {
        let result = match env.collection.take() {
            Some(collection) => collection.close().map(|_| ()).map_err(EngineError::from),
            None => Ok(()),
        };
        let _ = self.reply.send(result);
    }

    fn fail(self: Box<Self>, error: EngineError) {
        let _ = self.reply.send(Err(error));
    }
}
