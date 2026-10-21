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
use crate::changes::stream::{EventKind, Reservation, Seq};
use crate::changes::{self, MediaNotes};
use crate::error::EngineError;
use crate::executor::exclusive::Privilege;
use crate::executor::live_file::LiveFile;
use crate::executor::op::{CommitNotice, OpContext, OpResult, OpSpec, OpTiming, UndoMeta, WriteDelay};
use crate::executor::preconditions::{self, TouchIndex};
use crate::executor::source::QueueKind;
use crate::executor::{ExecShared, WriterWake};
use crate::lifecycle::Generation;
use crate::monotonic;
use crate::progress::ProgressPhase;
use crate::threads::ThreadActivity;
use crate::undo::reasons::DisabledReason;
use crate::undo::restorers::Direction;
use crate::undo::{Applied, CommitInfo, Restore, UndoNotes};

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
    /// An undo or redo op (`engine/undo` M4.1): the writer resolves its step,
    /// catalog op and declared write as it takes the op.
    pub restore: Option<Restore>,
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
    pub external_version: &'w mut Option<i64>,
    pub suspended_file: &'w mut Option<LiveFile>,
}

/// What reopening the file after a swap needs (`engine/lifecycle` L8).
pub(crate) struct Reopen {
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub initializers: Initializers,
}

thread_local! {
    /// Set on writer threads: work there must never wait for the writer
    /// (the day query, `sched/day-clock` D8).
    static ON_WRITER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the current thread is a collection's writer thread.
pub(crate) fn on_writer_thread() -> bool {
    ON_WRITER.with(std::cell::Cell::get)
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
    ON_WRITER.with(|on| on.set(true));
    let mut external_version = collection.writer().external_data_version().ok();
    let external_at_open = collection.external_change;
    let mut suspended_file = None;
    let mut collection = Some(collection);
    let mut touched = TouchIndex::new(floor);
    if external_at_open && let Some(opened) = &collection {
        let _ = crate::external_change::notify(&shared, opened.writer(), &mut touched);
    }
    let mut flush = WalFlush::default();
    loop {
        flush.observe(&shared, collection.as_ref());
        let mut task = match shared.next_task(flush.due) {
            WriterWake::Task(task) => task,
            WriterWake::Flush => {
                let _busy = activity.busy();
                flush.sync(&shared, collection.as_ref());
                continue;
            }
            WriterWake::Stop => break,
        };
        resolve_restore(&shared, &mut task.meta);
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
                    external_version: &mut external_version,
                    suspended_file: &mut suspended_file,
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

/// `engine/undo` M4.1: an undo or redo acts on the step on top of its scope
/// when the writer takes it, and from then on runs as the catalog op for
/// that step, declaring its write (O8), so the WAL flush before a long
/// write and its `WriteDelay` follow the step it restores.
fn resolve_restore(shared: &ExecShared, meta: &mut TaskMeta) {
    let Some(restore) = meta.restore.as_mut() else {
        return;
    };
    let resolved = shared.undo.resolve(restore.action);
    restore.target = Some(resolved.target);
    meta.name = resolved.def.name();
    meta.declared = resolved.declared;
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

    fn sync(&mut self, shared: &ExecShared, collection: Option<&OpenedCollection>) {
        let Some(collection) = collection else {
            *self = Self::default();
            return;
        };
        // No other commit can run on this writer during xSync. Only a
        // successful call advances coverage; a failure keeps every commit due.
        #[cfg(feature = "test-hooks")]
        let (started, through) = (monotonic::now(), shared.last_seq());
        let result = collection
            .writer()
            .sync_wal()
            .map_err(EngineError::from)
            .and_then(|synced| {
                crate::session::wal_covered(shared)?;
                Ok(synced)
            });
        #[cfg(feature = "test-hooks")]
        if !matches!(result, Ok(false)) {
            let completed = monotonic::now();
            shared.record_durability(|events| {
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
                if self.failures > 0 {
                    shared.stream.publish(
                        shared.generation(),
                        None,
                        EventKind::Extension(Arc::new(crate::suspend::DurabilityStatus::Recovered {
                            through: shared.last_seq(),
                        })),
                    );
                }
                self.due = None;
                self.failures = 0;
            }
            Err(error) => {
                if self.failures == 0 {
                    shared.stream.publish(
                        shared.generation(),
                        None,
                        EventKind::Extension(Arc::new(crate::suspend::DurabilityStatus::Failed {
                            through: shared.last_seq(),
                            error: match &error {
                                EngineError::Storage(e) => e.clone(),
                                _ => meguri_storage::StorageError::Io {
                                    detail: error.to_string(),
                                },
                            },
                        })),
                    );
                }
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
    mut spec: OpSpec,
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
    // The second the op began: what merges note edits into one step
    // (engine/undo A3).
    let began = shared.clock.now_ms().div_euclid(1000);
    let delay = meta
        .declared
        .map(|declared| DelayGuard::start(shared, meta.name, declared));
    let Some(collection) = env.collection.as_ref() else {
        return Err(EngineError::unavailable("the collection is not open"));
    };
    let generation = shared.generation();
    // Before any owned row mutation (including undo and revlog USN updates),
    // retire every committed answer witness. No commit/snapshot lock is held.
    crate::session::retire_witnesses(shared, collection.writer())?;
    if spec.source == OpSource::UserUndoable && spec.undo.session.is_none() {
        spec.undo.session = *shared.active_session.lock().unwrap();
    }
    let mut scope = collection.writer().begin_write(
        spec.stamp_mode(),
        spec.recorded_clock.as_deref().unwrap_or(shared.clock.as_ref()),
        Some(collection.journal_options()),
    )?;
    // BEGIN IMMEDIATE excludes a foreign commit between this check and the op.
    let version = collection.writer().external_data_version()?;
    if env.external_version.is_some_and(|old| old != version) {
        *env.external_version = Some(version);
        crate::external_change::notify(shared, collection.writer(), env.touched)?;
        scope.rollback()?;
        return Err(EngineError::Stale {
            table: meguri_storage::Table::Col,
            key: "external change".into(),
        });
    }
    *env.external_version = Some(version);
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
        pending_media: Vec::new(),
        collection_path: collection.path().to_path_buf(),
        undo: &shared.undo,
        undo_notes: UndoNotes::default(),
        restore: meta.restore,
        last_seq: shared.last_seq(),
        on_commit: Vec::new(),
        before_commit: Vec::new(),
        random: Arc::clone(&env.reopen.random),
    };
    // O3 and C12.3 share one failure boundary. The scope stays uncommitted
    // throughout validation and the file worker's durable receipt response.
    let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result = op(&mut context)?;
        for attachment in &mut context.pending_media {
            attachment.prepare(context.scope)?;
        }
        let restoring_undo = meta
            .restore
            .is_some_and(|restore| restore.action.direction() == Direction::Undo);
        crate::schema_gate::guard(context.scope(), restoring_undo)?;
        let observed_policy = if spec.source == OpSource::SyncIncoming {
            Some(meguri_sched::model::sched_config::SchedConfig::new(context.scope()).fsrs()?)
        } else {
            None
        };
        for action in context.before_commit.drain(..) {
            action()?;
        }
        Ok((result, observed_policy))
    }))
    .unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())));
    let (value, seen_fsrs) = match prepared {
        Ok(values) => values,
        Err(error) => {
            drop(context);
            scope.rollback()?;
            return Err(error);
        }
    };
    // Only successful preparation transfers these effects to the commit path.
    // Failed preparation drops the remaining actions with the operation context.
    let pending_media = std::mem::take(&mut context.pending_media);
    let media = MediaNotes {
        added: std::mem::take(&mut context.media_added),
        removed: std::mem::take(&mut context.media_removed),
    };
    let mut undo_notes = std::mem::take(&mut context.undo_notes);
    let hooks = std::mem::take(&mut context.on_commit);
    drop(context);
    if spec.source == OpSource::Rollover
        && undo_notes.clear == Some(crate::ClearCause::Rollover)
        && shared.active_session.lock().unwrap().is_some()
    {
        // C14 owns the clear while a production session is active: after
        // its shown answer, or immediately when no card can remain shown.
        undo_notes.clear = None;
    }
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
    #[cfg(feature = "test-hooks")]
    {
        let hook = shared.after_commit.lock().unwrap().take();
        if let Some(hook) = hook {
            hook(seq);
        }
    }
    if seen_fsrs == Some(true) {
        shared.fsrs_policy.observe_on();
    }
    let post_commit_error = committed.post_commit_error.clone();
    // The collection commit stands. Publish every staged operation before
    // observers, retaining each failed manifest for row-based recovery.
    let mut media_failures = Vec::new();
    for pending in pending_media {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pending.stage.publish())) {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => media_failures.push(error.to_string()),
            Err(payload) => media_failures.push(crate::error::panic_message(payload.as_ref())),
        }
    }
    let observing = !shared.observers.is_empty() || shared.answer_caches.has_live();
    let derived = changes::derive(committed.journal.as_ref(), spec.source, media, observing);
    if let Some(journal) = &committed.journal {
        env.touched
            .record(seq, journal.entries().map(|e| (e.table(), e.key())), journal.len());
    }
    let change_set = Arc::new(derived.changes);
    // The history changes before anyone sees the commit (engine/undo M2).
    let answer_owner = undo_notes
        .state
        .as_ref()
        .and_then(|s| s.downcast_ref::<crate::sched::answer::AnswerMetadata>())
        .and_then(|m| m.cache_build);
    let applied = shared.undo.after_commit(CommitInfo {
        seq,
        began,
        name: meta.name,
        source: spec.source,
        spec: &spec.undo,
        declared: spec.declared,
        journal: committed.journal.map(Arc::new),
        changes: &change_set,
        notes: undo_notes,
    });
    let restoring_answer = matches!(&applied,crate::undo::Applied::Restored {state:Some(s),..}
        if s.downcast_ref::<crate::sched::answer::AnswerMetadata>().is_some());
    if let Applied::Restored { state: Some(state), .. } = &applied
        && let Some(meta) = state.downcast_ref::<crate::sched::answer::AnswerMetadata>()
        && let Some(production) = &meta.production
    {
        production.own(generation, seq);
    }
    shared.answer_caches.observe(
        &ObservedCommit {
            seq,
            generation,
            changes: &change_set,
            cards: &derived.cards,
            revlog_added: &derived.revlog_added,
            revlog_removed: &derived.revlog_removed,
            config: &derived.config,
            collection: derived.collection.as_ref(),
        },
        answer_owner,
        restoring_answer,
    );
    shared.undo.notify_restorers(&applied, &change_set);
    // The day clock computes afresh after a change of its keys or of `col`
    // (`sched/day-clock` D6, D8). Noted before anyone hears of the commit, so
    // a check made on its event never takes the cached day.
    if !shared.replaying_review.load(Ordering::Acquire)
        && !change_set.is_empty()
        && shared.day_clock.note_commit(&change_set)
    {
        shared.day_clock.schedule_check(shared);
    }
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
                    collection: derived.collection.as_ref(),
                },
            );
        }
        reservation.fill(generation, None, EventKind::Changes(Arc::clone(&change_set)));
    }
    let notice = CommitNotice {
        seq,
        generation,
        published: !change_set.is_empty(),
    };
    for hook in hooks {
        // The commit stands whatever a hook does.
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(notice))) {
            let message = crate::error::panic_message(payload.as_ref());
            tracing::error!(op = meta.name, %message, "a commit hook panicked");
        }
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
    let undo = match applied {
        Applied::Recorded(step) => Some(UndoMeta {
            name: step.name,
            source: spec.source,
        }),
        Applied::Restored {
            direction: Direction::Redo,
            step,
            ..
        } => Some(UndoMeta {
            name: step.name,
            source: spec.source,
        }),
        Applied::Restored { .. } | Applied::Nothing => None,
    };
    if !media_failures.is_empty() {
        return Err(EngineError::MediaCommitPending {
            seq,
            message: media_failures.join("; "),
        });
    }
    if let Some(error) = post_commit_error {
        return Err(EngineError::CommitCompleted {
            seq,
            message: error.to_string(),
        });
    }
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
    /// Why the swap, the reopen or the new generation's first day check
    /// failed; `factory` says whether a collection is open again.
    pub error: Option<EngineError>,
    /// The new generation, when a file the swap changed was reopened and its
    /// first day check committed: after every successful swap, and after a
    /// failed one that had already changed the file.
    pub generation: Option<NewGeneration>,
    /// Readers of the collection open now: the new file, or the old one when
    /// the swap failed before changing it; `None` when nothing is open.
    pub factory: Option<ReaderFactory>,
}

/// A generation a replace made (`engine/lifecycle` L8): its number, and the
/// seq its `CollectionReplaced` holds. That seq precedes every event of the
/// new generation; the replace's caller fills it once the readers are open,
/// so nobody hears of the generation before its first day check committed.
pub(crate) struct NewGeneration {
    pub generation: Generation,
    pub announce: Reservation,
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
        // Keep the live collection installed until both journal obligations
        // succeed. An error returns its current readers without closing it.
        let Some(current) = env.collection.as_ref() else {
            let _ = reply.send(Replaced {
                error: Some(EngineError::unavailable("the collection is closed")),
                generation: None,
                factory: None,
            });
            return;
        };
        let journal_result = crate::session::retire_witnesses(shared, current.writer())
            .and_then(|()| crate::session::retire_incarnation(shared));
        if let Err(error) = journal_result {
            let _ = reply.send(Replaced {
                error: Some(error),
                generation: None,
                factory: Some(current.reader_factory()),
            });
            return;
        }
        // Only a durably closed incarnation may release review admission or
        // surrender the open file to the existing replacement protocol.
        *shared.active_session.lock().unwrap() = None;
        *shared.fsrs_policy.review_active.lock().unwrap() = false;
        let collection = env
            .collection
            .take()
            .expect("validated collection remains on the writer");
        let closed = collection.close();
        let live = LiveFile::before_swap(&shared.path);
        // Never replace a file whose final commit/checkpoint could not be
        // made durable. Reopen the original through the same recovery path
        // as a swap that failed before touching it.
        let swapped = match closed {
            Ok(outcome) if outcome.succeeded() => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| swap(&shared.path)))
                    .unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())))
            }
            Ok(_) => Err(EngineError::unavailable("checkpoint blocked by an external reader")),
            Err(error) => Err(error.into()),
        };
        // A swap that failed after changing the file still replaced the
        // collection. Checked before the reopen (see `LiveFile::changed`).
        let changed = live.changed();
        let replacing = swapped.is_ok() || changed;
        if replacing {
            // The steps describe the file the swap replaced; dropped before
            // any op of the new generation, also when the reopen fails
            // (engine/undo M6).
            shared.undo.reset(DisabledReason::ClearedByOneWay);
        }
        let reopen = env.reopen;
        let options = OpenOptions::new(reopen.clock.as_ref(), reopen.random.as_ref())
            .initializers(reopen.initializers.clone())
            .managed();
        let replaced = match open_collection(&shared.path, &options) {
            Err(error) => Replaced {
                error: Some(match swapped {
                    Err(swap_error) => swap_error,
                    Ok(()) => EngineError::Open(error),
                }),
                generation: None,
                factory: None,
            },
            Ok(collection) => {
                let factory = collection.reader_factory();
                *env.external_version = collection.writer().external_data_version().ok();
                *env.collection = Some(collection);
                env.touched.reset(*shared.lock_commit());
                if replacing {
                    let generation = Generation(shared.generation_cell.fetch_add(1, Ordering::SeqCst) + 1);
                    let announce = shared.stream.reserve();
                    match first_day_check(env) {
                        Ok(()) => Replaced {
                            error: swapped.err(),
                            generation: Some(NewGeneration { generation, announce }),
                            factory: Some(factory),
                        },
                        Err(check_error) => {
                            // As an open whose day check fails (L2): nothing
                            // stays open, no reader ever opens on the new
                            // file, and nobody hears of its generation.
                            drop(announce);
                            if let Some(collection) = env.collection.take()
                                && let Err(error) = collection.close()
                            {
                                tracing::warn!(%error, "closing the collection after its first day check failed");
                            }
                            let error = match swapped {
                                Ok(()) => check_error,
                                Err(swap_error) => {
                                    tracing::warn!(error = %check_error, "the first day check after a failed swap");
                                    swap_error
                                }
                            };
                            Replaced {
                                error: Some(error),
                                generation: None,
                                factory: None,
                            }
                        }
                    }
                } else {
                    Replaced {
                        error: swapped.err(),
                        generation: None,
                        factory: Some(factory),
                    }
                }
            }
        };
        let _ = reply.send(replaced);
    }

    fn fail(self: Box<Self>, error: EngineError) {
        let _ = self.reply.send(Replaced {
            error: Some(error),
            generation: None,
            factory: None,
        });
    }
}

/// The new generation's first day check (`engine/lifecycle` L8,
/// `sched/day-clock` D8), run by the replace task itself: the day clock
/// forgets the old collection's day and what it reported, then writes and
/// unburies on the new file in a `Rollover` op, while the readers are still
/// closed and before anyone hears of the generation. Its error fails the
/// replace, as it fails an open.
fn first_day_check(env: &mut WriterEnv<'_>) -> Result<(), EngineError> {
    let shared = env.shared;
    shared.day_clock.reset();
    let day_clock = Arc::clone(&shared.day_clock);
    let spec = crate::sched::day_clock::op_spec();
    let mut meta = shared.meta(spec.def.name(), spec.source);
    meta.planned = Some(shared.generation());
    let policy = Arc::clone(&shared.fsrs_policy);
    run_op(env, &meta, spec, move |ctx| {
        if meguri_sched::model::sched_config::SchedConfig::new(ctx.scope()).fsrs()? {
            ctx.on_commit(move |_| policy.observe_on());
        }
        day_clock.run(ctx)
    })
    .map(drop)
}

/// Closes the collection (`engine/lifecycle` L10).
pub(crate) struct CloseTask {
    pub reply: SyncSender<Result<(), EngineError>>,
}

impl TaskBody for CloseTask {
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, _meta: &TaskMeta) {
        // Steps live only while the collection is open (engine/undo M1, M6).
        env.shared.undo.reset(DisabledReason::None);
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

/// Short lifecycle work bypasses public admission, but always uses the writer.
pub(crate) struct ControlTask {
    pub action: crate::suspend::Control,
    pub reply: SyncSender<Result<crate::suspend::ControlReply, EngineError>>,
}
impl TaskBody for ControlTask {
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>, _meta: &TaskMeta) {
        let result = crate::suspend::control(env, self.action);
        let _ = self.reply.send(result);
    }
    fn fail(self: Box<Self>, error: EngineError) {
        let _ = self.reply.send(Err(error));
    }
}
