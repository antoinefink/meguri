//! Production review orchestration. Writer callbacks only capture owned values.
use super::{
    frozen::{self, Base, FrozenAnswer},
    journal::{Journal, Receipt, Terminal},
};
use crate::{
    ChangeObserver, Collection, Engine, EngineError, Generation, GenerationListener, OpContext, OpSource, OpSpec,
    OpenStep, Seq, SessionId, SuspendParticipant, UndoScope, UndoStatus,
    changes::observers::ObservedCommit,
    sched::{
        answer::{self, AnswerCache, AnswerRequest},
        queue::QueueProvider,
    },
};
use meguri_model::{
    card::{Card, CardQueue},
    ids::{CardId, DeckConfigId, DeckId, NoteId},
    proto::deck::DeckKind,
    time::EpochMillis,
};
use meguri_sched::{
    answer::{Rating, plan},
    model::sched_config::SchedConfig,
    queue::{
        self, Counts, Environment, Kind,
        rebuild_rules::{self, Action, Context, Stamp},
    },
    session::{
        QueueAnswer, Requeue, SessionQueue,
        overlay::Overlay,
        token::{self, RenderKey, StateToken},
    },
    states::{
        engine::{SchedulingStates, StateInput, next_states},
        fsrs_glue::FsrsCache,
    },
};
use meguri_storage::{Cell, ReadRows, ReaderRole, Record, RowKey, StampMode, Table};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    task::{Context as TaskContext, Poll, Wake, Waker},
    thread::JoinHandle,
    time::Duration,
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static NEXT_BUILD: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    FsrsPolicyPending,
    PresetUnsupported { preset_ids: Vec<DeckConfigId> },
}
#[derive(Clone, Debug, PartialEq)]
pub struct HomePreset {
    pub id: DeckConfigId,
    pub autoplay: bool,
    pub skip_question_when_replaying_answer: bool,
    pub wait_for_audio: bool,
    pub show_timer: bool,
    pub stop_timer_on_answer: bool,
    pub maximum_answer_seconds: u32,
    pub question_seconds: f32,
    pub question_action: i32,
    pub answer_seconds: f32,
    pub answer_action: i32,
    pub leech_action: i32,
}
impl HomePreset {
    fn from_preset(preset: &meguri_sched::model::preset::PresetView) -> Self {
        let p = preset.fields();
        Self {
            id: preset.source().id,
            autoplay: !p.disable_autoplay,
            skip_question_when_replaying_answer: p.skip_question_when_replaying_answer,
            show_timer: p.show_timer,
            stop_timer_on_answer: p.stop_timer_on_answer,
            maximum_answer_seconds: p.cap_answer_time_to_secs,
            question_seconds: p.seconds_to_show_question,
            answer_seconds: p.seconds_to_show_answer,
            question_action: p.question_action,
            answer_action: p.answer_action,
            wait_for_audio: p.wait_for_audio,
            leech_action: p.leech_action,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct HeadItem {
    pub card_id: CardId,
    pub note_id: NoteId,
    pub kind: Kind,
    pub state_token: StateToken,
    pub render_key: RenderKey,
    pub next_states: SchedulingStates,
    pub predicted_next: [Option<CardId>; 4],
    pub flag: u8,
    pub marked: bool,
    pub home_preset: HomePreset,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ReviewTerminalState {
    Active,
    WaitingUntil { instant: i64 },
    Finished,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    pub items: Vec<HeadItem>,
    pub counts: Counts,
    pub valid_until: i64,
    pub terminal_state: ReviewTerminalState,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub added: bool,
    pub restate: bool,
    pub rerender: bool,
    pub relabel: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct HeadDiff {
    pub head: Head,
    pub marks: Vec<Marks>,
    pub removed: Vec<CardId>,
}
#[derive(Clone, Debug)]
pub enum StartResult {
    Started {
        session_id: SessionId,
        generation: Generation,
        head: Head,
        time_lim: u32,
    },
    Refused(Refusal),
}
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // Owned boundary DTO; the common case is a card.
pub enum PeekResult {
    Card(HeadItem),
    Empty,
    Refused(Refusal),
}
#[derive(Clone, Debug)]
pub enum AnswerOutcome {
    Accepted {
        head_diff: HeadDiff,
        leech: bool,
        suspended: bool,
        rolled_over: bool,
    },
    Stale {
        fresh_head: Head,
        notice: bool,
    },
    NotAtHead {
        fresh_head: Head,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurrentChange {
    Eligible,
    Removed,
    MemoryOnly,
}
#[derive(Clone, Debug)]
pub enum SessionEvent {
    SessionStarted {
        session_id: SessionId,
        deck_id: DeckId,
    },
    SessionEnded {
        session_id: SessionId,
    },
    QueueHeadChanged {
        session_id: SessionId,
        diff: HeadDiff,
    },
    CurrentCardChanged {
        session_id: SessionId,
        card_id: CardId,
        change: CurrentChange,
    },
    CommitFailed {
        session_id: SessionId,
        card_id: CardId,
        error: EngineError,
    },
    WaitingUntil {
        session_id: SessionId,
        instant: i64,
    },
    Finished {
        session_id: SessionId,
        state: meguri_sched::counts::finished::DeckStudyState,
    },
    PresetUnsupported {
        session_id: SessionId,
        preset_ids: Vec<DeckConfigId>,
    },
    RecoveryDropped {
        event: u64,
        card_id: CardId,
        reason: String,
    },
}
impl crate::ExtensionEvent for SessionEvent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
fn emit(c: &Collection, cause: Option<Seq>, event: SessionEvent) {
    c.core
        .exec
        .stream
        .publish(c.generation(), cause, crate::EventKind::Extension(Arc::new(event)));
}
fn invalid(message: impl ToString) -> EngineError {
    EngineError::InvalidArgument {
        message: message.to_string(),
    }
}
fn no_session() -> EngineError {
    invalid("NoSession")
}
fn row_digest(rows: &impl ReadRows, id: CardId) -> Result<[u8; 32], EngineError> {
    rows.row_digest(Table::Cards, &RowKey::Id(id.0))?
        .ok_or_else(|| invalid("missing card"))
}
fn raw_data(image: &[Cell]) -> &[u8] {
    match image.get(17) {
        Some(Cell::Text(v)) | Some(Cell::Blob(v)) => v,
        _ => &[],
    }
}
fn choices(states: &SchedulingStates) -> [&meguri_sched::states::engine::AnswerState; 4] {
    [&states.again, &states.hard, &states.good, &states.easy]
}
fn diff(old: &[HeadItem], head: Head) -> HeadDiff {
    let marks = head
        .items
        .iter()
        .map(|item| {
            let Some(prior) = old.iter().find(|p| p.card_id == item.card_id) else {
                return Marks {
                    added: true,
                    ..Marks::default()
                };
            };
            let restate = prior.state_token != item.state_token;
            Marks {
                added: false,
                restate,
                rerender: prior.render_key != item.render_key,
                relabel: !restate
                    && (prior.next_states != item.next_states
                        || prior.predicted_next != item.predicted_next
                        || prior.home_preset != item.home_preset),
            }
        })
        .collect();
    let removed = old
        .iter()
        .filter(|p| !head.items.iter().any(|h| p.card_id == h.card_id))
        .map(|p| p.card_id)
        .collect();
    HeadDiff { head, marks, removed }
}

#[derive(Clone)]
struct Presentation {
    queue: SessionQueue,
    overlay: Overlay,
    rebuild: bool,
    force: Option<CardId>,
    label_boundary: Option<i64>,
    restore_cache: bool,
}
impl Presentation {
    fn rollover_due(&self, day: meguri_sched::timing::SchedDay, at: i64) -> bool {
        day.today != self.queue.value().today
            || self
                .label_boundary
                .is_some_and(|boundary| at.div_euclid(1000) >= boundary && day.next_day_at != boundary)
    }
}
#[derive(Clone)]
pub(crate) struct ProductionRestore {
    owned: Weak<Mutex<BTreeSet<(u64, u64)>>>,
    owner: Weak<Mutex<Presentation>>,
    record: QueueAnswer,
    undone: Arc<AtomicBool>,
    cache: Weak<AnswerCache>,
    post: Card,
}
impl std::fmt::Debug for ProductionRestore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductionRestore")
            .field("card", &self.record.removed.id())
            .finish()
    }
}
impl ProductionRestore {
    pub(crate) fn own(&self, generation: Generation, seq: Seq) {
        if let Some(owned) = self.owned.upgrade() {
            owned.lock().unwrap().insert((generation.0, seq.0));
        }
    }
    pub(crate) fn restore(&self, direction: crate::Direction) {
        let undo = direction == crate::Direction::Undo;
        if self.undone.swap(undo, Ordering::AcqRel) == undo {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            let mut p = owner.lock().unwrap();
            let result = match direction {
                crate::Direction::Undo => p.queue.undo(&self.record),
                crate::Direction::Redo => p.queue.redo(&self.record),
            };
            if result.is_err() {
                p.rebuild = true;
                p.restore_cache = true;
            }
            if direction == crate::Direction::Undo {
                let card = Some(self.record.removed.id());
                p.force = if result.is_err() { card } else { None };
                p.queue.set_current(card);
            }
        }
        if direction == crate::Direction::Undo
            && let Some(cache) = self.cache.upgrade()
        {
            cache.undo_accepted(self.post.id);
        }
    }
}
#[derive(Clone)]
struct PendingAnswer {
    event: u64,
    frozen: FrozenAnswer,
    before: Card,
    post: Card,
    elapsed: u32,
    buried: Vec<CardId>,
    clear_history: bool,
    cancel: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    restorer: ProductionRestore,
}
#[derive(Clone)]
struct PendingRedo {
    event: u64,
    frozen: FrozenAnswer,
}
#[derive(Clone)]
struct Session {
    id: SessionId,
    deck: DeckId,
    generation: Generation,
    ending: bool,
    presentation: Arc<Mutex<Presentation>>,
    cache: Arc<AnswerCache>,
    journal: Arc<Journal>,
    last: Head,
    last_answer: i64,
    pending: Vec<PendingAnswer>,
    redo: Vec<PendingRedo>,
    failed: Arc<AtomicBool>,
    built: Stamp,
    rollover_pending: bool,
    restoring: bool,
    undo_view: Option<crate::undo::entry::Entry>,
}
/// One owned rollback image for the asynchronous committed-undo presentation.
#[derive(Clone)]
struct UndoPresentation {
    head: Head,
    cache: Arc<AnswerCache>,
    chooser: meguri_sched::load_balancer::LoadBalancer,
    presentation: Presentation,
    built: Stamp,
}
impl UndoPresentation {
    fn restore(self, session: &mut Session) {
        *session.presentation.lock().unwrap() = self.presentation;
        self.cache.restore_view(self.chooser);
        session.cache = self.cache;
        session.built = self.built;
        session.undo_view = None;
        session.last = self.head;
    }
}
struct Binding {
    core: Weak<crate::collection::CollectionCore>,
    generation: Generation,
    journal: Arc<Journal>,
}
#[allow(clippy::type_complexity)] // The render LRU binds key, platform and immutable U62 payload.
struct State {
    binding: Option<Binding>,
    session: Option<Session>,
    render: VecDeque<(
        RenderKey,
        crate::rendering::Platform,
        Arc<crate::rendering::RenderedCard>,
    )>,
    paused: bool,
    mutating: bool,
}
struct StreamWake {
    tx: mpsc::Sender<Work>,
    queued: AtomicBool,
}
impl Wake for StreamWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Work::Stream);
        }
    }
}
#[cfg(feature = "test-hooks")]
type ObserveHook = Arc<dyn Fn(&crate::CommitPayload) + Send + Sync>;
struct Inner {
    events: crate::Subscription,
    wake: Arc<StreamWake>,
    #[cfg(feature = "test-hooks")]
    fail_application: AtomicU64,
    #[cfg(feature = "test-hooks")]
    before_prepare_hook: Mutex<Option<BeforePrepareHook>>,
    #[cfg(feature = "test-hooks")]
    observe_hook: Mutex<Option<ObserveHook>>,
    observing: AtomicBool,
    state: Mutex<State>,
    tx: mpsc::Sender<Work>,
    queues: Arc<QueueProvider>,
    owned: Arc<Mutex<BTreeSet<(u64, u64)>>>,
}
enum Work {
    Stream,
    Empty {
        session: SessionId,
        head: Head,
    },
    Answer {
        pending: Box<PendingAnswer>,
        session: SessionId,
        failed: Arc<AtomicBool>,
        cache: Arc<AnswerCache>,
    },
    Undo {
        session: SessionId,
        expected: crate::EntryId,
        restore: Box<ProductionRestore>,
        rollback: Box<UndoPresentation>,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
        reply: Option<mpsc::SyncSender<Result<Head, EngineError>>>,
    },
    Observe {
        generation: Generation,
        seq: Seq,
        changes: Box<meguri_model::changes::ChangeSet>,
        payload: crate::CommitPayload,
    },
    Barrier(mpsc::SyncSender<Result<(), EngineError>>, Duration),
    ApplicationBarrier(mpsc::SyncSender<()>),
    Stop,
}
/// Narrow owner hook for U47: cancellation is requested synchronously, never
/// waited for on the writer. Until U47 binds it, there is no optimizer provider.
pub trait SessionStartHook: Send + Sync {
    fn session_starting(&self);
}
impl SessionStartHook for crate::JobRegistry {
    fn session_starting(&self) {
        for job in self
            .running()
            .into_iter()
            .filter(|j| j.name == crate::JobKind::Optimizer.name())
        {
            job.cancel.cancel();
        }
    }
}
#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderStage {
    Captured,
    Publishing,
}
#[cfg(feature = "test-hooks")]
type RenderHook = Arc<dyn Fn(RenderStage) + Send + Sync>;
#[cfg(feature = "test-hooks")]
type BeforePrepareHook = Arc<dyn Fn() + Send + Sync>;
/// Job restoration stays under the existing lifecycle orchestration owner.
pub enum HistoryRestore {
    Session(Head),
    Global(crate::OpResult<crate::StepInfo>),
}
/// Excludes lifecycle owners without holding a mutex across their work or
/// synchronous progress delivery. Session `restoring` tracks worker state.
#[derive(Default)]
struct LifecycleGate {
    occupied: Mutex<bool>,
    changed: Condvar,
}
struct LifecyclePermit<'a>(&'a LifecycleGate);
impl LifecycleGate {
    fn acquire(&self) -> LifecyclePermit<'_> {
        let mut occupied = self.occupied.lock().unwrap();
        while *occupied {
            occupied = self.changed.wait(occupied).unwrap();
        }
        *occupied = true;
        drop(occupied);
        LifecyclePermit(self)
    }
    fn try_acquire(&self) -> Result<LifecyclePermit<'_>, EngineError> {
        let busy = || EngineError::unavailable("review lifecycle is busy");
        let mut occupied = self.occupied.try_lock().map_err(|_| busy())?;
        if *occupied {
            return Err(busy());
        }
        *occupied = true;
        drop(occupied);
        Ok(LifecyclePermit(self))
    }
}
impl Drop for LifecyclePermit<'_> {
    fn drop(&mut self) {
        let mut occupied = self.0.occupied.lock().unwrap();
        *occupied = false;
        self.0.changed.notify_one();
    }
}
pub struct ReviewService {
    inner: Arc<Inner>,
    stream: crate::ChangeStream,
    threads: crate::ThreadRegistry,
    engine: crate::WeakEngine,
    start_hooks: Mutex<Vec<Arc<dyn SessionStartHook>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    control: LifecycleGate,
    renders: AtomicU64,
    #[cfg(feature = "test-hooks")]
    render_hook: Mutex<Option<RenderHook>>,
    #[cfg(feature = "test-hooks")]
    fail_restore_projection: AtomicBool,
}
impl ReviewService {
    pub fn new(engine: &Engine, queues: Arc<QueueProvider>) -> Result<Arc<Self>, EngineError> {
        let (tx, rx) = mpsc::channel();
        let wake = Arc::new(StreamWake {
            tx: tx.clone(),
            queued: AtomicBool::new(false),
        });
        let inner = Arc::new(Inner {
            events: engine.stream().subscribe(),
            wake: wake.clone(),
            #[cfg(feature = "test-hooks")]
            fail_application: AtomicU64::new(0),
            #[cfg(feature = "test-hooks")]
            before_prepare_hook: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            observe_hook: Mutex::new(None),
            observing: AtomicBool::new(false),
            state: Mutex::new(State {
                binding: None,
                session: None,
                render: VecDeque::new(),
                paused: false,
                mutating: false,
            }),
            tx,
            queues,
            owned: Arc::new(Mutex::new(BTreeSet::new())),
        });
        let owned = inner.clone();
        let worker = engine
            .threads()
            .spawn_owned(
                "meguri-review",
                crate::Qos::UserInitiated,
                "review-session",
                move |activity| {
                    while let Ok(work) = rx.recv() {
                        if matches!(work, Work::Stop) {
                            break;
                        }
                        let _busy = activity.busy();
                        if let Err(e) = work_item(&owned, work) {
                            tracing::error!(%e, "review service worker");
                        }
                    }
                },
            )
            .map_err(invalid)?;
        wake.wake_by_ref();
        Ok(Arc::new(Self {
            inner,
            stream: engine.stream().clone(),
            threads: engine.threads().clone(),
            engine: engine.downgrade(),
            start_hooks: Mutex::new(vec![Arc::new(engine.jobs().clone())]),
            worker: Mutex::new(Some(worker)),
            control: LifecycleGate::default(),
            renders: AtomicU64::new(0),
            #[cfg(feature = "test-hooks")]
            render_hook: Mutex::new(None),
            #[cfg(feature = "test-hooks")]
            fail_restore_projection: AtomicBool::new(false),
        }))
    }
    #[cfg(feature = "test-hooks")]
    pub fn render_hook_for_testing(&self, hook: Option<RenderHook>) {
        *self.render_hook.lock().unwrap() = hook;
    }
    #[cfg(feature = "test-hooks")]
    fn render_checkpoint(&self, stage: RenderStage) {
        let hook = self.render_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(stage);
        }
    }
    #[cfg(feature = "test-hooks")]
    pub fn clear_render_cache_for_testing(&self) {
        self.inner.state.lock().unwrap().render.clear();
    }
    /// Gate deferred observation before it takes service state or reads images.
    #[cfg(feature = "test-hooks")]
    pub fn observe_hook_for_testing(&self, hook: Option<ObserveHook>) {
        *self.inner.observe_hook.lock().unwrap() = hook;
    }

    /// Wait for accepted U44 applications, leaving WAL/Done coverage untouched.
    #[cfg(feature = "test-hooks")]
    pub fn application_barrier_for_testing(&self) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.inner.tx.send(Work::ApplicationBarrier(tx)).map_err(invalid)?;
        rx.recv_timeout(Duration::from_secs(5)).map_err(invalid)
    }
    #[cfg(feature = "test-hooks")]
    pub fn fail_application_for_testing(&self, event: u64) {
        self.inner.fail_application.store(event, Ordering::Release);
    }
    #[cfg(feature = "test-hooks")]
    pub fn before_prepare_hook_for_testing(&self, hook: Option<BeforePrepareHook>) {
        *self.inner.before_prepare_hook.lock().unwrap() = hook;
    }
    #[cfg(feature = "test-hooks")]
    pub fn fail_next_restore_projection_for_testing(&self) {
        self.fail_restore_projection.store(true, Ordering::Release);
    }
    #[cfg(feature = "test-hooks")]
    pub fn fail_journal_for_testing(&self, stage: super::journal::JournalStage) -> Result<(), EngineError> {
        self.bound()?.1.fail_next(stage)
    }
    #[cfg(feature = "test-hooks")]
    pub fn journal_hook_for_testing(
        &self,
        hook: Arc<dyn Fn(super::journal::JournalStage) + Send + Sync>,
    ) -> Result<(), EngineError> {
        self.bound()?.1.set_hook(hook)
    }
    /// Hooks must request cancellation/mark a gate without waiting or owning a strong collection handle.
    pub fn add_session_start_hook(&self, hook: Arc<dyn SessionStartHook>) {
        self.start_hooks.lock().unwrap().push(hook);
    }
    pub fn render_count(&self) -> u64 {
        self.renders.load(Ordering::Relaxed)
    }
    #[cfg(feature = "test-hooks")]
    pub fn chooser_for_testing(&self, id: SessionId) -> Result<meguri_sched::load_balancer::LoadBalancer, EngineError> {
        let state = self.inner.state.lock().unwrap();
        Ok(state
            .session
            .as_ref()
            .filter(|s| s.id == id)
            .ok_or_else(no_session)?
            .cache
            .snapshot())
    }
    fn bound(&self) -> Result<(Collection, Arc<Journal>), EngineError> {
        bound(&self.inner)
    }
    pub fn active_session(&self) -> Option<SessionId> {
        {
            let state = self.inner.state.lock().unwrap();
            if let Some(session) = &state.session {
                return Some(session.id);
            }
        }
        self.bound()
            .ok()
            .and_then(|(c, _)| *c.core.exec.active_session.lock().unwrap())
    }
    pub fn start(&self, deck: DeckId, resume: Option<CardId>) -> Result<StartResult, EngineError> {
        let _control = self.control.acquire();
        self.start_inner(deck, resume)
    }
    /// Bounded-job entry: refuse another lifecycle owner and remain cancellable
    /// while queued for the writer. After admission, finish the lifecycle even
    /// if cancellation arrives: ending a prior session cannot be rolled back.
    pub fn start_controlled(
        &self,
        deck: DeckId,
        resume: Option<CardId>,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<StartResult, EngineError> {
        let _control = self.control.try_acquire()?;
        self.admit_lifecycle_job(cancel, progress)?;
        self.start_inner(deck, resume)
    }
    fn admit_lifecycle_job(
        &self,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<(), EngineError> {
        cancel.check()?;
        let (c, _) = self.bound()?;
        let _idle = c
            .core
            .exec
            .maintenance
            .get()
            .and_then(std::sync::Weak::upgrade)
            .map(|m| m.pause_idle());
        // No session/presentation mutex is held across this writer wait.
        // Rollover leaves rows and history untouched for this empty op.
        c.run(
            OpSpec::named("admit_review_lifecycle", OpSource::Rollover).cancel(cancel.clone()),
            |_| Ok(()),
        )?;
        cancel.check()?;
        progress.set_phase(crate::ProgressPhase::Committing);
        Ok(())
    }
    fn start_inner(&self, deck: DeckId, resume: Option<CardId>) -> Result<StartResult, EngineError> {
        let (idle_collection, _) = self.bound()?;
        let _idle = idle_collection
            .core
            .exec
            .maintenance
            .get()
            .and_then(std::sync::Weak::upgrade)
            .map(|m| m.pause_idle());
        if let Some(id) = self.active_session() {
            self.end_inner(id)?;
        }
        let (c, journal) = self.bound()?;
        journal.check(c.generation())?;
        let id = SessionId(NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
        let policy = c.core.exec.fsrs_policy.clone();
        let active = c.core.exec.clone();
        let hooks = self.start_hooks.lock().unwrap().clone();
        // Admission and select share the existing writer transaction. Refusal
        // returns before hooks or any row writes, and enable uses this writer.
        let admitted = c
            .run(
                OpSpec::named("set_current_deck", OpSource::UserNonUndoable).skip_undo(),
                move |ctx| {
                    if *policy.review_active.lock().unwrap() {
                        return Err(EngineError::SessionActive);
                    }
                    if let Some(refusal) = admission(ctx.scope(), deck, policy.seen())? {
                        return Ok(Some(refusal));
                    }
                    for hook in hooks {
                        hook.session_starting();
                    }
                    crate::decks::current::select_in(ctx.scope(), deck)?;
                    ctx.on_commit(move |_| {
                        *policy.review_active.lock().unwrap() = true;
                        *active.active_session.lock().unwrap() = Some(id);
                    });
                    Ok(None)
                },
            )?
            .value;
        if let Some(refusal) = admitted {
            return Ok(StartResult::Refused(refusal));
        }
        let result = (|| {
            c.check_rollover()?;
            let cache = AnswerCache::build_session(&c)?;
            let snapshot = self.inner.queues.queue(&c, Some(deck))?;
            let built = snapshot.stamp;
            let mut queue = SessionQueue::new(snapshot.value, NEXT_BUILD.fetch_add(1, Ordering::Relaxed));
            queue.set_current(resume);
            let time_lim = {
                let r = c.reader(ReaderRole::Interactive)?;
                SchedConfig::new(&*r).time_limit()?
            };
            let mut session = Session {
                id,
                deck,
                generation: c.generation(),
                ending: false,
                presentation: Arc::new(Mutex::new(Presentation {
                    queue,
                    overlay: Overlay::default(),
                    rebuild: false,
                    force: None,
                    label_boundary: None,
                    restore_cache: false,
                })),
                cache,
                journal: journal.clone(),
                last: Head {
                    items: Vec::new(),
                    counts: Counts::default(),
                    valid_until: 0,
                    terminal_state: ReviewTerminalState::Finished,
                },
                last_answer: i64::MIN,
                pending: Vec::new(),
                redo: Vec::new(),
                failed: Arc::new(AtomicBool::new(false)),
                built,
                rollover_pending: false,
                restoring: false,
                undo_view: None,
            };
            let head = project(&c, &mut session, 3, None)?;
            self.inner.state.lock().unwrap().session = Some(session);
            emit(
                &c,
                None,
                SessionEvent::SessionStarted {
                    session_id: id,
                    deck_id: deck,
                },
            );
            emit_empty(&self.inner, &c, id, &head)?;
            Ok(StartResult::Started {
                session_id: id,
                generation: c.generation(),
                head,
                time_lim,
            })
        })();
        if result.is_err() {
            self.inner.state.lock().unwrap().session = None;
            release_admission(&c, false)?;
        }
        result
    }
    pub fn peek_first_card(&self, deck: DeckId) -> Result<PeekResult, EngineError> {
        let (c, _) = self.bound()?;
        let r = c.reader(ReaderRole::Interactive)?;
        let rows = r.snapshot()?;
        if let Some(refusal) = admission(&*rows, deck, c.core.exec.fsrs_policy.seen())? {
            return Ok(PeekResult::Refused(refusal));
        }
        let at = c.core.exec.clock.now_ms();
        let day = timing(&*rows, &c, at)?;
        let env = Environment::read(&*rows, day.today, at.div_euclid(1000), day.next_day_at)?;
        let mut q = SessionQueue::new(queue::build(&*rows, &env, deck)?, 0);
        let first = q.read(at.div_euclid(1000), 1).first().cloned();
        // Build the exact chooser snapshot with no maintenance or selection.
        let chooser = chooser_from_rows(&*rows, &c, day.today, day.next_day_at)?;
        match first {
            None => Ok(PeekResult::Empty),
            Some(e) => {
                let mut models = FsrsCache::default();
                let mut render_keys =
                    crate::rendering::KeyBatch::new(&*rows, crate::rendering::RenderMode::Review, session_platform());
                let item = project_item(
                    &mut render_keys,
                    &q,
                    &Overlay::default(),
                    e.id,
                    e.kind,
                    at,
                    day.today,
                    day.next_day_at,
                    &chooser,
                    None,
                    &mut models,
                )?;
                Ok(PeekResult::Card(item))
            }
        }
    }
    /// U48 admission shares the lifecycle owner with start/end/undo/close.
    /// Stop new optimistic answers before draining accepted ones; no service
    /// mutex survives a writer or journal wait. The real observer projects the
    /// committed change, using C13's patch/rebuild classifier.
    pub fn sched_op(
        &self,
        request: crate::sched::ops::Request,
        session: Option<SessionId>,
        options: &crate::tags::RunOptions,
    ) -> Result<crate::OpResult<crate::sched::ops::ResultValue>, EngineError> {
        request.validate()?;
        let _control = self.control.try_acquire()?;
        if let Some(cancel) = &options.cancel {
            cancel.check()?;
        }
        let (c, journal) = self.bound()?;
        journal.check(c.generation())?;
        {
            let mut state = self.inner.state.lock().unwrap();
            if state.paused || state.mutating {
                return Err(EngineError::unavailable("review admission is closed"));
            }
            if let Some(id) = session {
                let s = state.session.as_ref().filter(|s| s.id == id).ok_or_else(no_session)?;
                if s.ending || s.restoring || s.failed.load(Ordering::Acquire) {
                    return Err(EngineError::unavailable("review is restoring"));
                }
            }
            state.mutating = true;
        }
        struct MutationAdmission<'a>(&'a Mutex<State>);
        impl Drop for MutationAdmission<'_> {
            fn drop(&mut self) {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .mutating = false;
            }
        }
        let _admission = MutationAdmission(&self.inner.state);
        self.drain_until(crate::monotonic::now() + Duration::from_secs(5))?;
        if let Some(cancel) = &options.cancel {
            cancel.check()?;
        }
        let old = session.and_then(|id| {
            self.inner
                .state
                .lock()
                .unwrap()
                .session
                .as_ref()
                .filter(|s| s.id == id)
                .map(|s| s.last.items.clone())
        });
        let mut result = crate::sched::ops::run_admitted(&c, request, session, options)?;
        // run() returns after observers have enqueued Work::Observe. A worker
        // barrier therefore witnesses that exact stream, never a preview or a
        // caller-synthesized replacement queue.
        let projected = (|| {
            let (tx, rx) = mpsc::sync_channel(1);
            self.inner.tx.send(Work::ApplicationBarrier(tx)).map_err(invalid)?;
            rx.recv().map_err(invalid)?;
            let mut state = self.inner.state.lock().unwrap();
            if let Some(s) = state.session.as_mut() {
                s.redo.clear();
            }
            if let Some(id) = session {
                let s = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
                // Re-read full values after the ordered observer; no rebuild
                // request is made by this operation API.
                let head = project(&c, s, 3, None)?;
                result.value.head_diff = Some(diff(old.as_deref().unwrap_or(&[]), head));
            }
            Ok::<_, EngineError>(())
        })();
        projected.map_err(|error| EngineError::CommitCompleted {
            seq: result.seq,
            message: error.to_string(),
        })?;
        Ok(result)
    }
    /// A Browse bulk action that writes scheduling (`browser/workspace`
    /// V8.1: Suspend, Unsuspend, Bury, Unbury, Set Due Date, Forget),
    /// admitted as [`Self::sched_op`] admits an operation with no session:
    /// new answers wait, the answers already accepted are written first, and
    /// once the action committed the session's observer has seen it and the
    /// session's redo is gone. `run` is the action (`bulk::cards::run_admitted`)
    /// on the bound collection; it is not a step of the session.
    pub fn admit_card_action<T>(
        &self,
        cancel: &Arc<crate::CancelToken>,
        run: impl FnOnce(&Collection) -> Result<crate::OpResult<T>, EngineError>,
    ) -> Result<crate::OpResult<T>, EngineError> {
        let _control = self.control.try_acquire()?;
        cancel.check()?;
        let (c, journal) = self.bound()?;
        journal.check(c.generation())?;
        {
            let mut state = self.inner.state.lock().unwrap();
            if state.paused || state.mutating {
                return Err(EngineError::unavailable("review admission is closed"));
            }
            state.mutating = true;
        }
        struct MutationAdmission<'a>(&'a Mutex<State>);
        impl Drop for MutationAdmission<'_> {
            fn drop(&mut self) {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .mutating = false;
            }
        }
        let _admission = MutationAdmission(&self.inner.state);
        self.drain_until(crate::monotonic::now() + Duration::from_secs(5))?;
        cancel.check()?;
        let result = run(&c)?;
        // As in `sched_op`: the barrier witnesses the observer's work for
        // this commit before the admission closes.
        let settled = (|| {
            let (tx, rx) = mpsc::sync_channel(1);
            self.inner.tx.send(Work::ApplicationBarrier(tx)).map_err(invalid)?;
            rx.recv().map_err(invalid)?;
            if let Some(s) = self.inner.state.lock().unwrap().session.as_mut() {
                s.redo.clear();
            }
            Ok::<_, EngineError>(())
        })();
        settled.map_err(|error| EngineError::CommitCompleted {
            seq: result.seq,
            message: error.to_string(),
        })?;
        Ok(result)
    }
    /// `review/edit-current` E2: Edit Note's save from the review, admitted
    /// as a card action is (the open session, not ending or restoring; answers
    /// in flight drained first) and recorded as a step of that session, so the
    /// review's Undo covers it. The session reacts to the committed change
    /// through its observer, as to any other change.
    pub fn patch_note(
        &self,
        input: crate::notes::NotePatch,
        session: SessionId,
        expected: Option<crate::Generation>,
    ) -> Result<crate::OpResult<crate::notes::Updated>, EngineError> {
        let _control = self.control.try_acquire()?;
        let (c, journal) = self.bound()?;
        // The editor's generation (`editor/field-html` S2), checked on the
        // collection this op runs on.
        if let Some(planned) = expected {
            if c.generation() != planned {
                return Err(EngineError::StaleGeneration { planned, current: c.generation() });
            }
        }
        journal.check(c.generation())?;
        {
            let mut state = self.inner.state.lock().unwrap();
            if state.paused || state.mutating {
                return Err(EngineError::unavailable("review admission is closed"));
            }
            let s = state.session.as_ref().filter(|s| s.id == session).ok_or_else(no_session)?;
            if s.ending || s.restoring || s.failed.load(Ordering::Acquire) {
                return Err(EngineError::unavailable("review is restoring"));
            }
            state.mutating = true;
        }
        struct MutationAdmission<'a>(&'a Mutex<State>);
        impl Drop for MutationAdmission<'_> {
            fn drop(&mut self) {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .mutating = false;
            }
        }
        let _admission = MutationAdmission(&self.inner.state);
        self.drain_until(crate::monotonic::now() + Duration::from_secs(5))?;
        let result = match crate::notes::patch_for(&c, input, meguri_model::changes::OpSource::UserUndoable, Some(session)) {
            Ok(result) => result,
            // The patch committed although the call reports an error: it is
            // still a new step, so what was undone can no longer be redone.
            Err(error @ (EngineError::CommitCompleted { .. } | EngineError::MediaCommitPending { .. })) => {
                if let Some(s) = self.inner.state.lock().unwrap().session.as_mut() {
                    s.redo.clear();
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        // A new step of the session: what was undone can no longer be redone.
        let projected = (|| {
            let (tx, rx) = mpsc::sync_channel(1);
            self.inner.tx.send(Work::ApplicationBarrier(tx)).map_err(invalid)?;
            rx.recv().map_err(invalid)?;
            if let Some(s) = self.inner.state.lock().unwrap().session.as_mut() {
                s.redo.clear();
            }
            Ok::<_, EngineError>(())
        })();
        projected.map_err(|error| EngineError::CommitCompleted {
            seq: result.seq,
            message: error.to_string(),
        })?;
        Ok(result)
    }
    pub fn head(&self, id: SessionId, count: usize) -> Result<Head, EngineError> {
        if !(1..=10).contains(&count) {
            return Err(invalid("head count must be 1..10"));
        }
        let (c, journal) = self.bound()?;
        journal.check(c.generation())?;
        let mut state = self.inner.state.lock().unwrap();
        let session = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
        let head = if session.restoring {
            let mut head = session.last.clone();
            head.items.truncate(count);
            head
        } else {
            project(&c, session, count, None)?
        };
        drop(state);
        emit_empty(&self.inner, &c, id, &head)?;
        Ok(head)
    }
    pub fn refresh_card(&self, id: SessionId, card: CardId) -> Result<Head, EngineError> {
        let (c, _) = self.bound()?;
        let mut state = self.inner.state.lock().unwrap();
        let session = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
        let mut p = session.presentation.lock().unwrap();
        if !p.queue.presented(card) {
            return Err(invalid("card is not presented"));
        }
        p.queue.set_current(Some(card));
        drop(p);
        project(&c, session, 3, None)
    }
    pub fn render_head(
        &self,
        id: SessionId,
        keys: &[RenderKey],
        platform: crate::rendering::Platform,
    ) -> Result<Vec<Arc<crate::rendering::RenderedCard>>, EngineError> {
        if platform != session_platform() {
            return Err(invalid(
                "render platform differs from the session's native review context",
            ));
        }
        let (c, _) = self.bound()?;
        let head = self.head(id, 10)?;
        let mut prepared = Vec::with_capacity(keys.len());
        for key in keys {
            let card = head
                .items
                .iter()
                .find(|h| h.render_key == *key)
                .ok_or_else(|| invalid("render key is no longer in head"))?
                .card_id;
            // Capture current consumed inputs even on hits: delivery and stamps
            // are not freshness witnesses. Release the reader before HTML work.
            let capture = {
                let state = self.inner.state.lock().unwrap();
                let session = state.session.as_ref().filter(|s| s.id == id).ok_or_else(no_session)?;
                let reader = c.reader(ReaderRole::Interactive)?;
                let rows = reader.snapshot()?;
                let presentation = session.presentation.lock().unwrap();
                let (_, card) = presentation_card(&*rows, &presentation.overlay, card, session.undo_view.as_ref())?;
                review_capture(&*rows, card, session.undo_view.as_ref())?
            };
            if capture.key() != *key {
                return Err(invalid("render changed during request"));
            }
            #[cfg(feature = "test-hooks")]
            self.render_checkpoint(RenderStage::Captured);
            let cached = {
                let state = self.inner.state.lock().unwrap();
                state
                    .render
                    .iter()
                    .find(|(k, p, _)| k == key && *p == platform)
                    .map(|(_, _, value)| value.clone())
            };
            let rendered = if let Some(cached) = cached {
                cached
            } else {
                let rendered = Arc::new(crate::rendering::render_captured(&c, &capture, None, false)?);
                self.renders.fetch_add(1, Ordering::Relaxed);
                rendered
            };
            #[cfg(feature = "test-hooks")]
            self.render_checkpoint(RenderStage::Publishing);
            prepared.push((*key, card, rendered));
        }
        // Validate and publish the batch under the presentation owner, with one
        // bounded head projection rather than one state computation per page.
        let mut state = self.inner.state.lock().unwrap();
        let session = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
        let current = project(&c, session, 10, None)?;
        if prepared
            .iter()
            .any(|(key, card, _)| !current.items.iter().any(|h| h.card_id == *card && h.render_key == *key))
        {
            return Err(invalid("render changed during request"));
        }
        let mut result = Vec::with_capacity(prepared.len());
        for (key, _, rendered) in prepared {
            if let Some(at) = state.render.iter().position(|(k, p, _)| *k == key && *p == platform) {
                let cached = state.render.remove(at).unwrap();
                result.push(cached.2.clone());
                state.render.push_back(cached);
            } else {
                state.render.push_back((key, platform, rendered.clone()));
                while state.render.len() > 16 {
                    state.render.pop_front();
                }
                result.push(rendered);
            }
        }
        Ok(result)
    }
    pub fn answer(
        &self,
        id: SessionId,
        card: CardId,
        rating: Rating,
        supplied: StateToken,
        at: EpochMillis,
        elapsed: u64,
    ) -> Result<AnswerOutcome, EngineError> {
        self.answer_inner(id, card, rating, supplied, at, elapsed, false)
    }
    #[allow(clippy::too_many_arguments)]
    fn answer_inner(
        &self,
        id: SessionId,
        card: CardId,
        rating: Rating,
        supplied: StateToken,
        at: EpochMillis,
        elapsed: u64,
        is_redo: bool,
    ) -> Result<AnswerOutcome, EngineError> {
        let (c, journal) = self.bound()?;
        journal.check(c.generation())?;
        if !c.core.exec.review_accepting.load(Ordering::Acquire) {
            return Err(EngineError::unavailable("review is draining"));
        }
        c.core.exec.require_admission(None)?;
        let now = c.core.exec.clock.now_ms();
        if !is_redo && at.0 > now {
            return Err(invalid("answer instant is in the future"));
        }
        let mut state = self.inner.state.lock().unwrap();
        if state.paused || state.mutating {
            return Err(EngineError::unavailable("review is suspended"));
        }
        let session = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
        if session.ending {
            return Err(EngineError::unavailable("session is ending"));
        }
        if session.failed.load(Ordering::Acquire) {
            return Err(EngineError::unavailable("answer chain is restoring"));
        }
        // C15 replays an already-effective instant; a later answer must not
        // clamp that retained input a second time.
        let at = if is_redo { at.0 } else { at.0.max(session.last_answer) };
        let old = session.last.items.clone();
        let permitted = session.presentation.lock().unwrap().queue.permits(card);
        if !permitted {
            let head = project(&c, session, 3, None)?;
            return Ok(AnswerOutcome::NotAtHead { fresh_head: head });
        }
        let r = c.reader(ReaderRole::Interactive)?;
        let rows = r.snapshot()?;
        let day = timing(&*rows, &c, at)?;
        let p = session.presentation.lock().unwrap();
        let kind = p.queue.kind(card).ok_or_else(|| invalid("missing queue entry"))?;
        let chooser = session.cache.snapshot();
        let mut models = FsrsCache::default();
        let (token, states, captured) = capture_state(
            &*rows,
            &p.overlay,
            card,
            at,
            day.today,
            day.next_day_at,
            &chooser,
            None,
            &mut models,
        )?;
        if supplied != token {
            drop(p);
            drop(rows);
            drop(r);
            session.presentation.lock().unwrap().rebuild = true;
            let head = project(&c, session, 3, None)?;
            let notice = head.items.iter().any(|h| h.card_id == card);
            return Ok(AnswerOutcome::Stale {
                fresh_head: head,
                notice,
            });
        }
        let digest = meguri_storage::journal::digest(&captured.image);
        let context = frozen::context(&*rows, &captured.card, &captured.inputs)?;
        let index = rating as usize - 1;
        let chosen = choices(&states)[index].clone();
        let planned = plan(
            &captured.card,
            states.current,
            &chosen,
            &captured.inputs.preset,
            captured.inputs.retention,
            day.today,
            at,
            elapsed,
        )
        .map_err(invalid)?;
        let mut post_image = captured.image.clone();
        let written = planned.card.to_cells();
        for i in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 17] {
            post_image[i] = written[i].clone();
        }
        // The next optimistic answer consumes stored codec rounding, exactly
        // as it would after reading U44's actual postimage back from SQLite.
        let post = Card::from_cells(&post_image).map_err(meguri_storage::StorageError::from)?;
        let buried = sibling_ids(&*rows, &p.overlay, &captured.card, &captured.inputs.preset, None)?;
        let request = AnswerRequest {
            card,
            state_token: digest,
            current: states.current,
            chosen,
            rating,
            today: day.today,
            next_day_at: day.next_day_at,
            answered_at: EpochMillis(at),
            elapsed_ms: elapsed,
            from_queue: Some(kind),
            learning_cutoff: Some(p.queue.value().learning_cutoff),
            session: Some(id),
            cache: Some(session.cache.clone()),
        };
        let base = p
            .overlay
            .predecessor(card)
            .map(Base::Predecessor)
            .unwrap_or(Base::Row(digest));
        let frozen = FrozenAnswer::capture(
            &request,
            base,
            context,
            c.core.exec.clock.utc_offset_at(at),
            captured.inputs.ahead,
        );
        drop(p);
        // The file queue is independent of the writer, including when a long
        // write begins after this call. No sync/writer/render wait here.
        let event = journal.next_event();
        let mut draft = session.clone();
        draft.presentation = Arc::new(Mutex::new(session.presentation.lock().unwrap().clone()));
        let mut p = draft.presentation.lock().unwrap();
        let Presentation { queue, overlay, .. } = &mut *p;
        let record = overlay
            .accept(
                queue,
                event,
                post.clone(),
                &buried,
                at.div_euclid(1000),
                day.next_day_at,
            )
            .map_err(|_| invalid("NotAtHead"))?;
        let restorer = ProductionRestore {
            owned: Arc::downgrade(&self.inner.owned),
            owner: Arc::downgrade(&session.presentation),
            record,
            undone: Arc::new(AtomicBool::new(false)),
            cache: Arc::downgrade(&session.cache),
            post: post.clone(),
        };
        drop(p);
        let pending = PendingAnswer {
            event,
            frozen: frozen.clone(),
            before: captured.card.clone(),
            post: post.clone(),
            elapsed: planned.elapsed_ms,
            buried: buried.clone(),
            clear_history: session.presentation.lock().unwrap().rollover_due(day, at),
            cancel: Arc::new(AtomicBool::new(false)),
            phase: Arc::new(AtomicU8::new(0)),
            restorer,
        };
        draft.pending.push(pending.clone());
        if pending.clear_history {
            draft.presentation.lock().unwrap().rebuild = true;
        }
        draft.last_answer = at;
        if !is_redo {
            draft.redo.clear();
        }
        let prior_cache = session.cache.snapshot();
        if !is_redo {
            session.cache.accept(&post, captured.inputs.preset.source().id);
        }
        // Reuse the exact freshness snapshot for the optimistic post-head;
        // a second snapshot adds a commit-lock wait and can mix two commits.
        let computed = project_snapshot(&c, &mut draft, &rows, 3, at, &mut models);
        let head = match computed {
            Ok(h) => h,
            Err(e) => {
                session.cache.restore_view(prior_cache);
                return Err(e);
            }
        };
        if let Err(e) = journal.enqueue(frozen) {
            session.cache.restore_view(prior_cache);
            return Err(e);
        }
        let rolled_over = pending.clear_history;
        *session.presentation.lock().unwrap() = draft.presentation.lock().unwrap().clone();
        session.last = draft.last;
        session.last_answer = session.last_answer.max(at);
        session.redo = draft.redo;
        session.cache = draft.cache;
        session.built = draft.built;
        session.pending = draft.pending;
        session.rollover_pending = draft.rollover_pending;
        self.inner
            .tx
            .send(Work::Answer {
                pending: Box::new(pending),
                session: id,
                failed: session.failed.clone(),
                cache: session.cache.clone(),
            })
            .map_err(invalid)?;
        if head.items.is_empty() {
            if let ReviewTerminalState::WaitingUntil { instant } = head.terminal_state {
                emit(
                    &c,
                    None,
                    SessionEvent::WaitingUntil {
                        session_id: id,
                        instant,
                    },
                );
            } else {
                self.inner
                    .tx
                    .send(Work::Empty {
                        session: id,
                        head: head.clone(),
                    })
                    .map_err(invalid)?;
            }
        }
        Ok(AnswerOutcome::Accepted {
            head_diff: diff(&old, head),
            leech: planned.leech,
            suspended: planned.suspended,
            rolled_over,
        })
    }
    pub fn flush_for_background(&self) -> Result<(), EngineError> {
        self.flush_until(crate::monotonic::now() + Duration::from_secs(5))
    }
    fn drain_until(&self, deadline: Duration) -> Result<(), EngineError> {
        // A fresh recovery-only host never installed a journal or accepted
        // work. A stale/failed binding must still take the normal error path.
        {
            let state = self.inner.state.lock().unwrap();
            if state.binding.is_none() && state.session.is_none() && !state.mutating {
                return Ok(());
            }
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.inner.tx.send(Work::Barrier(tx, deadline)).map_err(invalid)?;
        rx.recv_timeout(deadline.saturating_sub(crate::monotonic::now()))
            .map_err(|_| EngineError::SuspendDeadline {
                phase: "review-session".into(),
            })??;
        let (c, journal) = self.bound()?;
        c.core
            .exec
            .control(crate::suspend::Control::Flush)
            .wait_timeout(deadline.saturating_sub(crate::monotonic::now()))
            .ok_or_else(|| EngineError::SuspendDeadline {
                phase: "review WAL".into(),
            })??;
        journal.barrier_until(deadline)?;
        if c.write_delay().is_none() {
            journal.compact_until(deadline)?;
        }
        Ok(())
    }
    pub fn end(&self, id: SessionId) -> Result<(), EngineError> {
        let _control = self.control.acquire();
        self.end_inner(id)
    }
    /// Reserve the entire host close, including when no session is installed.
    /// Shutdown runs with logical exclusion only, never a held mutex guard.
    /// Publish host ownership with `closed` only after successful review cleanup.
    pub fn try_close_host(
        &self,
        shutdown: impl FnOnce() -> Result<(), EngineError>,
        closed: impl FnOnce(),
    ) -> Result<(), EngineError> {
        let _control = self.control.try_acquire().map_err(|_| EngineError::Busy {
            running_op: "review lifecycle".into(),
        })?;
        let (exec, ending) = {
            let mut state = self.inner.state.lock().unwrap();
            // Capture only shared memory before shutdown can retire the core.
            // Neither a reader nor ordinary writer admission is needed here.
            let exec = state
                .binding
                .as_ref()
                .and_then(|b| b.core.upgrade())
                .map(|c| c.exec.clone());
            let ending = state.session.as_mut().map(|s| {
                let previous = s.ending;
                s.ending = true;
                (s.id, s.generation, previous)
            });
            (exec, ending)
        };
        struct RestoreEnding<'a> {
            state: &'a Mutex<State>,
            previous: Option<(SessionId, bool)>,
        }
        impl Drop for RestoreEnding<'_> {
            fn drop(&mut self) {
                if let Some((id, previous)) = self.previous {
                    let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(s) = state.session.as_mut().filter(|s| s.id == id) {
                        s.ending = previous;
                    }
                }
            }
        }
        let mut restore = RestoreEnding {
            state: &self.inner.state,
            previous: ending.map(|(id, _, previous)| (id, previous)),
        };
        // Engine::close owns the retryable participant flush/quiescence path,
        // including fully suspended connections and failed-suspend admission.
        shutdown()?;
        restore.previous = None;
        self.inner.observing.store(false, Ordering::Release);
        let retired = {
            let mut state = self.inner.state.lock().unwrap();
            state.paused = true;
            state.render.clear();
            self.inner.owned.lock().unwrap().clear();
            (state.session.take(), state.binding.take())
        };
        if let Some(exec) = exec {
            *exec.fsrs_policy.review_active.lock().unwrap() = false;
            *exec.active_session.lock().unwrap() = None;
            *exec.review_journal.lock().unwrap() = None;
        }
        // Dropping the last journal owner joins its worker without a state lock.
        drop(retired);
        if let Some((id, generation, _)) = ending {
            self.stream.publish(
                generation,
                None,
                crate::EventKind::Extension(Arc::new(SessionEvent::SessionEnded { session_id: id })),
            );
        }
        // Publish host ownership changes only after the old binding is gone,
        // still under the same logical lifecycle reservation.
        closed();
        Ok(())
    }
    /// Like start_controlled: cancellation before writer admission changes no
    /// session state; after admission the drain/release must really complete.
    pub fn end_controlled(
        &self,
        id: SessionId,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<(), EngineError> {
        let _control = self.control.try_acquire()?;
        self.admit_lifecycle_job(cancel, progress)?;
        self.end_inner(id)
    }
    fn end_inner(&self, id: SessionId) -> Result<(), EngineError> {
        {
            let mut state = self.inner.state.lock().unwrap();
            state
                .session
                .as_mut()
                .filter(|s| s.id == id)
                .ok_or_else(no_session)?
                .ending = true;
        }
        self.drain_until(crate::monotonic::now() + Duration::from_secs(5))?;
        let (c, _) = self.bound()?;
        let clear_history = {
            let state = self.inner.state.lock().unwrap();
            let session = state.session.as_ref().filter(|s| s.id == id).ok_or_else(no_session)?;
            let reader = c.reader(ReaderRole::Interactive)?;
            let rows = reader.snapshot()?;
            let at = c.core.exec.clock.now_ms();
            let due = session
                .presentation
                .lock()
                .unwrap()
                .rollover_due(timing(&*rows, &c, at)?, at);
            session.rollover_pending || due
        };
        let previous = self.inner.state.lock().unwrap().session.take();
        if let Err(e) = release_admission(&c, clear_history) {
            self.inner.state.lock().unwrap().session = previous;
            return Err(e);
        }
        emit(&c, None, SessionEvent::SessionEnded { session_id: id });
        Ok(())
    }
    pub fn peek_undo(&self, scope: UndoScope) -> Result<UndoStatus, EngineError> {
        self.peek_restore(scope, false)
    }
    pub fn peek_redo(&self, scope: UndoScope) -> Result<UndoStatus, EngineError> {
        self.peek_restore(scope, true)
    }
    fn peek_restore(&self, scope: UndoScope, redo: bool) -> Result<UndoStatus, EngineError> {
        let (c, _) = self.bound()?;
        let state = self.inner.state.lock().unwrap();
        if let UndoScope::Session(id) = scope {
            let s = state.session.as_ref().filter(|s| s.id == id).ok_or_else(no_session)?;
            if s.ending || s.restoring || s.failed.load(Ordering::Acquire) {
                return Err(EngineError::unavailable("answer chain is restoring"));
            }
            let event = if redo {
                s.redo.last().map(|p| p.event)
            } else {
                s.pending.last().map(|p| p.event)
            };
            if let Some(event) = event {
                return Ok(UndoStatus::Available(crate::StepInfo {
                    id: crate::EntryId(event),
                    name: crate::undo::names::ANSWER_CARD,
                    kind: crate::StepKind::Answer,
                    session: Some(id),
                    long_write: None,
                }));
            }
        }
        drop(state);
        if redo { c.peek_redo(scope) } else { c.peek_undo(scope) }
    }
    /// Atomic short admission: never falls through to collection history.
    /// A busy lifecycle owner refuses promptly instead of waiting on a write.
    pub fn try_pending_restore(&self, scope: UndoScope, redo: bool) -> Result<Option<Head>, EngineError> {
        let _control = self.control.try_acquire()?;
        if redo {
            self.redo_inner(scope, true, crate::CancelToken::new(), crate::ProgressSink::new())
        } else {
            self.undo_inner(scope, true, None)
        }
    }
    pub fn restore_controlled(
        &self,
        scope: UndoScope,
        redo: bool,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<HistoryRestore, EngineError> {
        cancel.check()?;
        let _control = self.control.try_acquire()?;
        let (c, _) = self.bound()?;
        if self.active_session().is_some() || matches!(scope, UndoScope::Session(_)) {
            let head = if redo {
                self.redo_inner(scope, false, cancel, progress)?
            } else {
                self.undo_inner(scope, false, Some((cancel, progress)))?
            };
            Ok(HistoryRestore::Session(head.ok_or_else(no_session)?))
        } else {
            let result = if redo {
                c.redo_controlled(scope, cancel, progress)?
            } else {
                c.undo_controlled(scope, cancel, progress)?
            };
            Ok(HistoryRestore::Global(result))
        }
    }
    pub fn undo(&self, scope: UndoScope) -> Result<Head, EngineError> {
        let _control = self.control.acquire();
        self.undo_inner(scope, false, None)?.ok_or_else(no_session)
    }
    fn undo_inner(
        &self,
        scope: UndoScope,
        pending_only: bool,
        ports: Option<(Arc<crate::CancelToken>, crate::ProgressSink)>,
    ) -> Result<Option<Head>, EngineError> {
        let (c, journal) = self.bound()?;
        if let UndoScope::Session(id) = scope {
            let mut state = self.inner.state.lock().unwrap();
            let s = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
            if s.ending || s.restoring || s.failed.load(Ordering::Acquire) {
                return Err(EngineError::unavailable("answer chain is restoring"));
            }
            if let Some(pending) = s
                .pending
                .last()
                .cloned()
                .filter(|pending| pending.phase.load(Ordering::Acquire) < 2)
            {
                drop(state);
                if let Some((cancel, progress)) = &ports {
                    cancel.check()?;
                    progress.set_phase(crate::ProgressPhase::Committing);
                }
                let mut state = self.inner.state.lock().unwrap();
                let s = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
                if s.ending || s.restoring || s.failed.load(Ordering::Acquire) {
                    return Err(EngineError::unavailable("answer chain is restoring"));
                }
                // The application worker or a new answer can win while progress
                // is delivered without the service lock. Select again then.
                if s.pending.last().is_none_or(|last| last.event != pending.event)
                    || pending
                        .phase
                        .try_update(Ordering::AcqRel, Ordering::Acquire, |p| (p < 2).then_some(3))
                        .is_err()
                {
                    drop(state);
                    return self.undo_inner(scope, pending_only, ports);
                }
                pending.cancel.store(true, Ordering::Release);
                let mut p = s.presentation.lock().unwrap();
                let Presentation { queue, overlay, .. } = &mut *p;
                overlay.rollback_from(pending.event, queue);
                queue.set_current(Some(CardId(pending.frozen.card)));
                drop(p);
                s.cache.undo_accepted(CardId(pending.frozen.card));
                s.pending.pop();
                s.redo.push(PendingRedo {
                    event: pending.event,
                    frozen: pending.frozen.clone(),
                });
                // Cancellation's file acknowledgement is required before a
                // conflicting operation; UI restoration itself happens above.
                drop(state);
                journal.terminal(pending.event, Terminal::Cancelled)?;
                return self.head(id, 3).map(Some);
            }
            drop(state);
            if pending_only {
                return Ok(None);
            }
            let mut state = self.inner.state.lock().unwrap();
            let s = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
            if s.pending.is_empty()
                && let Some((entry, metadata)) = c.answer_undo_metadata(scope)?
                && let Some(restore) = metadata.production
                && restore
                    .owner
                    .upgrade()
                    .is_some_and(|owner| Arc::ptr_eq(&owner, &s.presentation))
            {
                let rollback = UndoPresentation {
                    head: s.last.clone(),
                    cache: s.cache.clone(),
                    chooser: s.cache.snapshot(),
                    presentation: s.presentation.lock().unwrap().clone(),
                    built: s.built,
                };
                let expected = entry.info.id;
                restore.restore(crate::Direction::Undo);
                s.undo_view = Some(entry);
                let computed = project(&c, s, 3, None);
                let head = match computed {
                    Ok(head) => head,
                    Err(error) => {
                        restore.restore(crate::Direction::Redo);
                        rollback.restore(s);
                        return Err(error);
                    }
                };
                s.restoring = true;
                s.failed.store(true, Ordering::Release);
                let (reply, wait) = if ports.is_some() {
                    let (tx, rx) = mpsc::sync_channel(1);
                    (Some(tx), Some(rx))
                } else {
                    (None, None)
                };
                let (cancel, progress) = ports
                    .clone()
                    .unwrap_or_else(|| (crate::CancelToken::new(), crate::ProgressSink::new()));
                if self
                    .inner
                    .tx
                    .send(Work::Undo {
                        session: id,
                        expected,
                        restore: Box::new(restore.clone()),
                        rollback: Box::new(rollback.clone()),
                        cancel,
                        progress,
                        reply,
                    })
                    .is_err()
                {
                    restore.restore(crate::Direction::Redo);
                    rollback.restore(s);
                    s.restoring = false;
                    s.failed.store(false, Ordering::Release);
                    return Err(EngineError::unavailable("review worker stopped"));
                }
                drop(state);
                return if let Some(wait) = wait {
                    wait.recv().map_err(invalid)?.map(Some)
                } else {
                    Ok(Some(head))
                };
            }
        }
        if pending_only {
            return Ok(None);
        }
        self.flush_for_background()?;
        let id = self.active_session().ok_or_else(no_session)?;
        let (cancel, progress) = ports.unwrap_or_else(|| (crate::CancelToken::new(), crate::ProgressSink::new()));
        let restored = c.undo_controlled(scope, cancel, progress)?;
        self.restored_head(&c, id, restored.seq).map(Some)
    }
    pub fn redo(&self, scope: UndoScope) -> Result<Head, EngineError> {
        let _control = self.control.acquire();
        self.redo_inner(scope, false, crate::CancelToken::new(), crate::ProgressSink::new())?
            .ok_or_else(no_session)
    }
    fn redo_inner(
        &self,
        scope: UndoScope,
        pending_only: bool,
        cancel: Arc<crate::CancelToken>,
        progress: crate::ProgressSink,
    ) -> Result<Option<Head>, EngineError> {
        if !pending_only
            && self
                .inner
                .state
                .lock()
                .unwrap()
                .session
                .as_ref()
                .is_some_and(|s| s.restoring)
        {
            self.flush_for_background()?;
        }
        let (c, _) = self.bound()?;
        if let UndoScope::Session(id) = scope {
            let frozen = {
                let mut state = self.inner.state.lock().unwrap();
                let session = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
                if session.ending || session.restoring || session.failed.load(Ordering::Acquire) {
                    return Err(EngineError::unavailable("answer chain is restoring"));
                }
                session.redo.last().cloned()
            };
            if let Some(redo) = frozen {
                let frozen = redo.frozen;
                let h = self.head(id, 10)?;
                let item = h
                    .items
                    .iter()
                    .find(|h| h.card_id.0 == frozen.card)
                    .ok_or_else(|| invalid("redo card no longer presented"))?;
                let request = frozen.request([0; 32])?;
                cancel.check()?;
                progress.set_phase(crate::ProgressPhase::Committing);
                let outcome = self.answer_inner(
                    id,
                    item.card_id,
                    request.rating,
                    item.state_token,
                    request.answered_at,
                    request.elapsed_ms,
                    true,
                )?;
                if !matches!(outcome, AnswerOutcome::Accepted { .. }) {
                    return Err(invalid("redo answer changed during validation"));
                }
                if let Some(s) = self.inner.state.lock().unwrap().session.as_mut().filter(|s| s.id == id) {
                    s.redo.pop();
                }
                return self.head(id, 3).map(Some);
            }
        }
        if pending_only {
            return Ok(None);
        }
        self.flush_for_background()?;
        let id = self.active_session().ok_or_else(no_session)?;
        let restored = c.redo_controlled(scope, cancel, progress)?;
        self.restored_head(&c, id, restored.seq).map(Some)
    }
    fn restored_head(&self, c: &Collection, id: SessionId, seq: Seq) -> Result<Head, EngineError> {
        #[cfg(feature = "test-hooks")]
        let fail = self.fail_restore_projection.swap(false, Ordering::AcqRel);
        #[cfg(not(feature = "test-hooks"))]
        let fail = false;
        let projected = if fail {
            Err(EngineError::unavailable("injected restore projection failure"))
        } else {
            self.head(id, 3)
        };
        projected.map_err(|error| {
            let error = EngineError::CommitCompleted {
                seq,
                message: format!("review projection unavailable: {error}"),
            };
            let mut state = self.inner.state.lock().unwrap();
            if let Some(s) = state.session.as_mut().filter(|s| s.id == id) {
                s.failed.store(true, Ordering::Release);
                if let Some(item) = s.last.items.first() {
                    emit(
                        c,
                        Some(seq),
                        SessionEvent::CommitFailed {
                            session_id: id,
                            card_id: item.card_id,
                            error: error.clone(),
                        },
                    );
                }
            }
            error
        })
    }
}

fn bound(inner: &Inner) -> Result<(Collection, Arc<Journal>), EngineError> {
    let s = inner.state.lock().unwrap();
    let b = s
        .binding
        .as_ref()
        .ok_or_else(|| EngineError::unavailable("review provider is not bound"))?;
    let core = b
        .core
        .upgrade()
        .ok_or_else(|| EngineError::unavailable("collection closed"))?;
    let c = Collection {
        core,
        generation: b.generation,
        privilege: None,
        undo_session: None,
    };
    c.check_current()?;
    Ok((c, b.journal.clone()))
}
fn release_admission(c: &Collection, clear_history: bool) -> Result<(), EngineError> {
    let exec = c.core.exec.clone();
    c.run(OpSpec::named("end_review_session", OpSource::Rollover), move |ctx| {
        ctx.on_commit(move |_| {
            if clear_history {
                exec.undo.reset(crate::DisabledReason::ClearedByRollover);
            }
            *exec.fsrs_policy.review_active.lock().unwrap() = false;
            *exec.active_session.lock().unwrap() = None;
        });
        Ok(())
    })
    .map(drop)
}
fn admission(rows: &impl ReadRows, deck: DeckId, seen: bool) -> Result<Option<Refusal>, EngineError> {
    let cfg = SchedConfig::new(rows);
    if meguri_sched::compat::fsrs_policy::status(cfg.fsrs()?, seen) != crate::sched::fsrs_policy::PolicyStatus::Ok {
        return Ok(Some(Refusal::FsrsPolicyPending));
    }
    let env = Environment::read(rows, 0, 0, 0)?;
    if env.index(deck).is_none() {
        return Err(invalid("missing selected deck"));
    }
    let unsupported = crate::sched::fsrs_policy::unsupported_home_presets(rows, &env.subtree(deck, false))?;
    if !unsupported.is_empty() {
        return Ok(Some(Refusal::PresetUnsupported {
            preset_ids: unsupported,
        }));
    }
    if cfg.load_balancer_enabled()? {
        let mut presets = rows
            .deck_config_rows()?
            .into_iter()
            .filter(|p| !matches!(p.config.easy_days_percentages.len(), 0 | 7))
            .map(|p| p.id)
            .collect::<Vec<_>>();
        presets.sort_unstable();
        presets.dedup();
        if !presets.is_empty() {
            return Ok(Some(Refusal::PresetUnsupported { preset_ids: presets }));
        }
    }
    Ok(None)
}
fn timing(rows: &impl ReadRows, c: &Collection, at: i64) -> Result<meguri_sched::timing::SchedDay, EngineError> {
    let cfg = SchedConfig::new(rows);
    Ok(meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
        crt: rows.col()?.created.0,
        creation_offset: cfg.creation_offset()?,
        rollover_hour: cfg.rollover_hour()?,
        now: at.div_euclid(1000),
        offset: c.core.exec.clock.utc_offset_at(at),
    }))
}
fn chooser_from_rows(
    rows: &impl ReadRows,
    c: &Collection,
    today: u32,
    boundary: i64,
) -> Result<meguri_sched::load_balancer::LoadBalancer, EngineError> {
    use meguri_sched::load_balancer::{Entry, LoadBalancer};
    let enabled = SchedConfig::new(rows).load_balancer_enabled()?;
    let homes = rows
        .deck_rows()?
        .into_iter()
        .filter_map(|d| match &d.kind.kind {
            DeckKind::Normal(n) => Some((d.id, DeckConfigId(n.config_id))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let presets = if enabled {
        rows.deck_config_rows()?
            .into_iter()
            .map(|p| (p.id, p.config.easy_days_percentages.clone()))
            .collect()
    } else {
        Vec::new()
    };
    let counted = if enabled {
        rows.answer_due_range(today)?
            .into_iter()
            .filter_map(|r| {
                homes.get(&r.deck).map(|id| {
                    (
                        *id,
                        r.day,
                        Entry {
                            card: r.card,
                            note: r.note,
                        },
                    )
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    LoadBalancer::new(today, boundary, c.core.exec.clock.as_ref(), presets, counted).map_err(invalid)
}
struct Captured {
    image: meguri_storage::RowImage,
    card: Card,
    inputs: frozen::Inputs,
}
fn sibling_ids(
    rows: &impl ReadRows,
    overlay: &Overlay,
    card: &Card,
    preset: &meguri_sched::model::preset::PresetView,
    undo: Option<&crate::undo::entry::Entry>,
) -> Result<Vec<CardId>, EngineError> {
    let mut siblings = rows.cards_of_note(card.note_id)?;
    if let Some(entry) = undo {
        for sibling in &mut siblings {
            if let Some(change) = entry.journal.entry(Table::Cards, &RowKey::Id(sibling.id.0))
                && let Some(before) = entry.before(change)?
            {
                *sibling = Card::from_cells(&before).map_err(meguri_storage::StorageError::from)?;
            }
        }
    }
    Ok(siblings
        .into_iter()
        .map(|s| overlay.row(s.id).cloned().unwrap_or(s))
        .filter(|s| {
            s.id != card.id
                && matches!(
                    s.queue,
                    CardQueue::New | CardQueue::Review | CardQueue::InterdayLearning
                )
                && meguri_sched::answer::siblings::allowed(card.queue, s.queue, preset.fields())
        })
        .map(|s| s.id)
        .collect())
}
fn presentation_card(
    rows: &impl ReadRows,
    overlay: &Overlay,
    id: CardId,
    undo: Option<&crate::undo::entry::Entry>,
) -> Result<(Vec<Cell>, Card), EngineError> {
    let mut image = rows
        .row_image(Table::Cards, &RowKey::Id(id.0))?
        .ok_or_else(|| invalid("queue card disappeared"))?;
    if let Some(entry) = undo
        && let Some(change) = entry.journal.entry(Table::Cards, &RowKey::Id(id.0))
        && let Some(before) = entry.before(change)?
    {
        image = before;
    }
    let stored = Card::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
    let card = overlay.row(id).cloned().unwrap_or(stored);
    if overlay.row(id).is_some() {
        let post = card.to_cells();
        for i in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 17] {
            image[i] = post[i].clone();
        }
    }
    Ok((image, card))
}

// The projected note and card feed both the key and actual U62 payload,
// including while committed undo waits for the writer.
fn review_capture(
    rows: &impl ReadRows,
    card: Card,
    undo: Option<&crate::undo::entry::Entry>,
) -> Result<crate::rendering::Capture, EngineError> {
    let mut render =
        crate::rendering::Capture::read(rows, card, crate::rendering::RenderMode::Review, session_platform())?;
    project_review_note(&mut render.note, undo)?;
    Ok(render)
}

fn project_review_note(
    note: &mut meguri_model::note::Note,
    undo: Option<&crate::undo::entry::Entry>,
) -> Result<(), EngineError> {
    if let Some(entry) = undo
        && let Some(change) = entry.journal.entry(Table::Notes, &RowKey::Id(note.id.0))
        && let Some(before) = entry.before(change)?
    {
        *note = meguri_model::note::Note::from_cells(&before).map_err(meguri_storage::StorageError::from)?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn capture_state(
    rows: &impl ReadRows,
    overlay: &Overlay,
    id: CardId,
    at: i64,
    today: u32,
    boundary: i64,
    chooser: &meguri_sched::load_balancer::LoadBalancer,
    undo: Option<&crate::undo::entry::Entry>,
    models: &mut FsrsCache,
) -> Result<(StateToken, SchedulingStates, Captured), EngineError> {
    let (image, card) = presentation_card(rows, overlay, id, undo)?;
    let removed = undo
        .map(|entry| {
            entry
                .journal
                .entries()
                .filter(|change| change.table() == Table::Revlog && !change.existed_before())
                .filter_map(|change| match change.key() {
                    RowKey::Id(id) => Some(*id),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let inputs = frozen::inputs_without_revlogs(rows, &card, boundary, models, &removed)?;
    let (configured_preset, home_dr) = match &inputs.home.kind.kind {
        DeckKind::Normal(n) => (DeckConfigId(n.config_id), n.desired_retention),
        _ => return Err(invalid("non-normal home")),
    };
    let token = token::state_token(&token::StateInputs {
        card: &card,
        raw_data: raw_data(&image),
        preset_id: configured_preset,
        preset_mtime: rows.deck_config(configured_preset)?.map(|p| p.mtime),
        home_retention: home_dr,
        effective_home: token::effective_home_fingerprint(&inputs.preset, inputs.retention),
        short_term_with_steps: inputs.short_term,
        learn_ahead: inputs.ahead,
        today,
        next_day_at: boundary,
        history_memory: if card.data.memory_state.is_none() {
            inputs.memory
        } else {
            None
        },
        history_last_review: if card.data.last_review_secs.is_none() {
            inputs.last_review
        } else {
            None
        },
    });
    let input = StateInput {
        card: &card,
        preset: &inputs.preset,
        memory_state: inputs.memory,
        last_review_time: inputs.last_review,
        desired_retention: inputs.retention,
        today,
        next_day_at: boundary,
        now: at.div_euclid(1000),
        short_term_with_steps: inputs.short_term,
        learn_ahead: inputs.ahead,
    };
    let enabled = SchedConfig::new(rows).load_balancer_enabled()?;
    let mut choose = chooser.chooser(
        inputs.preset.source().id,
        inputs.preset.fields().bury_reviews.then_some(card.note_id),
        meguri_sched::states::fuzz::seed(card.id.0 as u64, u64::from(card.reps)),
    );
    let states = next_states(models, &input, if enabled { Some(&mut choose) } else { None }).map_err(invalid)?;
    Ok((token, states, Captured { image, card, inputs }))
}
#[allow(clippy::too_many_arguments)]
fn project_item(
    render_keys: &mut crate::rendering::KeyBatch<'_, impl ReadRows>,
    q: &SessionQueue,
    overlay: &Overlay,
    id: CardId,
    kind: Kind,
    at: i64,
    today: u32,
    boundary: i64,
    chooser: &meguri_sched::load_balancer::LoadBalancer,
    undo: Option<&crate::undo::entry::Entry>,
    models: &mut FsrsCache,
) -> Result<HeadItem, EngineError> {
    let rows = render_keys.rows();
    let (token, states, captured) = capture_state(rows, overlay, id, at, today, boundary, chooser, undo, models)?;
    let Captured { card, inputs, .. } = &captured;
    let siblings = sibling_ids(rows, overlay, card, &inputs.preset, undo)?;
    let mut predicted = [None; 4];
    let mut prediction_queue = q.clone();
    prediction_queue.set_current(Some(id));
    for (index, chosen) in choices(&states).into_iter().enumerate() {
        let planned = plan(
            card,
            states.current,
            chosen,
            &inputs.preset,
            inputs.retention,
            today,
            at,
            0,
        )
        .map_err(invalid)?;
        predicted[index] = prediction_queue
            .predict(
                id,
                Requeue::from_card(&planned.card, boundary),
                &siblings,
                at.div_euclid(1000),
            )
            .map_err(|_| invalid("prediction card not permitted"))?;
    }
    let (render_key, note) = render_keys.key(card, |note| project_review_note(note, undo))?;
    let item = HeadItem {
        card_id: id,
        note_id: card.note_id,
        kind: if kind == Kind::InterdayLearning {
            Kind::Learning
        } else {
            kind
        },
        state_token: token,
        render_key,
        next_states: states,
        predicted_next: predicted,
        flag: card.flags & 7,
        marked: crate::tags::canonical::split_text(&note.tags).any(|t| t.to_lowercase() == "marked"),
        home_preset: HomePreset::from_preset(&inputs.preset),
    };
    Ok(item)
}
// C6 permits a fixed context when peek, start and renderHead share it.
fn session_platform() -> crate::rendering::Platform {
    if cfg!(target_os = "ios") {
        crate::rendering::Platform::Ios
    } else {
        crate::rendering::Platform::Mac
    }
}
fn project(c: &Collection, s: &mut Session, count: usize, at: Option<i64>) -> Result<Head, EngineError> {
    let at = at.unwrap_or_else(|| c.core.exec.clock.now_ms());
    let r = c.reader(ReaderRole::Interactive)?;
    let rows = r.snapshot()?;
    let mut models = FsrsCache::default();
    project_snapshot(c, s, &rows, count, at, &mut models)
}
fn project_background(c: &Collection, s: &mut Session, seq: Seq) -> Result<Option<Head>, EngineError> {
    let at = c.core.exec.clock.now_ms();
    let reader = c.reader(ReaderRole::Interactive)?;
    let rows = reader.snapshot()?;
    match project_snapshot(c, s, &rows, 3, at, &mut FsrsCache::default()) {
        Ok(head) => Ok(Some(head)),
        Err(EngineError::Unsupported { feature }) if feature == "FSRS parameters" => {
            let env = Environment::read(&*rows, 0, at.div_euclid(1000), 0)?;
            let preset_ids = crate::sched::fsrs_policy::unsupported_home_presets(&*rows, &env.subtree(s.deck, false))?;
            emit(
                c,
                Some(seq),
                SessionEvent::PresetUnsupported {
                    session_id: s.id,
                    preset_ids,
                },
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn project_snapshot(
    c: &Collection,
    s: &mut Session,
    rows: &crate::Snapshot<'_>,
    count: usize,
    at: i64,
    models: &mut FsrsCache,
) -> Result<Head, EngineError> {
    let day = timing(&**rows, c, at)?;
    let mut p = s.presentation.lock().unwrap();
    // Keep the boundary that supplied the shown labels. At day zero a real
    // boundary crossing need not change the clamped day number.
    let day_changed = p.rollover_due(day, at);
    if !day_changed {
        p.label_boundary = Some(day.next_day_at);
    }
    if day_changed {
        s.rollover_pending = true;
    }
    let wants_rebuild = p.rebuild || day_changed;
    if wants_rebuild {
        let mut env = Environment::read(&**rows, day.today, at.div_euclid(1000), day.next_day_at)?;
        env.fsrs = true;
        if let Some(entry) = &s.undo_view {
            for change in entry.journal.entries().filter(|change| change.table() == Table::Decks) {
                if let Some(before) = entry.before(change)? {
                    let deck =
                        meguri_model::deck::Deck::from_cells(&before).map_err(meguri_storage::StorageError::from)?;
                    if let Some(current) = env.decks.iter_mut().find(|d| d.id == deck.id) {
                        *current = deck;
                    }
                }
            }
        }
        let mut overrides = p
            .overlay
            .rows()
            .map(|card| (card.id, card.clone()))
            .collect::<BTreeMap<_, _>>();
        if let Some(entry) = &s.undo_view {
            for change in entry.journal.entries().filter(|change| change.table() == Table::Cards) {
                if let Some(before) = entry.before(change)? {
                    let card = Card::from_cells(&before).map_err(meguri_storage::StorageError::from)?;
                    overrides.insert(card.id, card);
                }
            }
        }
        let receipts = s.journal.entries().into_iter().collect::<BTreeMap<_, _>>();
        for pending in &s.pending {
            let represented = receipts
                .get(&pending.event)
                .and_then(|e| e.receipt.as_ref())
                .is_some_and(|receipt| {
                    rows.row_image(Table::Revlog, &RowKey::Id(receipt.row[0]))
                        .ok()
                        .flatten()
                        .is_some_and(|row| {
                            row.len() == 9 && row.iter().zip(receipt.row).all(|(c, v)| *c == Cell::Integer(v))
                        })
                });
            if represented {
                continue;
            }
            let home = pending.before.deck_id;
            let names = env.decks.iter().find(|d| d.id == home).map(|d| d.name.clone());
            if let Some(name) = names {
                for deck in &mut env.decks {
                    if deck.id == home
                        || name
                            .strip_prefix(&deck.name)
                            .is_some_and(|suffix| suffix.starts_with('\u{1f}'))
                    {
                        *deck = meguri_sched::answer::deck_stats::answered(
                            deck,
                            pending.frozen.today,
                            pending.before.queue,
                            pending.elapsed,
                            pending.frozen.at.div_euclid(1000),
                        );
                    }
                }
            }
            for id in &pending.buried {
                if !overrides.contains_key(id)
                    && let Some(mut card) = rows.card(*id)?
                {
                    card.queue = CardQueue::BuriedByScheduler;
                    overrides.insert(*id, card);
                }
            }
        }
        if day_changed && meguri_sched::timing::needs_unbury(SchedConfig::new(&**rows).last_unburied()?, day.today) {
            for buried in rows.cards_in_queues(&[-2, -3])? {
                if !overrides.contains_key(&buried.id)
                    && let Some(mut card) = rows.card(buried.id)?
                    && let Some(queue) =
                        meguri_sched::timing::unburied_queue(card.card_type.stored(), i64::from(card.due))
                {
                    card.queue = CardQueue::from_stored(queue)
                        .ok_or_else(|| invalid(format!("unbury returned an invalid queue: {queue}")))?;
                    overrides.insert(card.id, card);
                }
            }
        }
        let value = queue::build_overlay(&**rows, &env, s.deck, &overrides.into_values().collect::<Vec<_>>())?;
        let keep_shown = day_changed
            && p.queue
                .current()
                .is_some_and(|id| value.presentation().iter().any(|e| e.id == id));
        // W9: an answer's old cache cannot change a cache rebuilt meanwhile.
        // The fresh queue for P9's build-changed case keeps that chooser view.
        let cache = if !day_changed && (s.undo_view.is_some() || p.restore_cache) {
            s.cache.clone()
        } else {
            AnswerCache::from_session_snapshot(c, rows, day.today, day.next_day_at, at)?
        };
        for pending in &s.pending {
            if receipts
                .get(&pending.event)
                .is_some_and(|e| e.committed.is_some_and(|(_, seq)| seq <= rows.seq()))
            {
                continue;
            }
            cache.accept(
                &pending.post,
                frozen::inputs(&**rows, &pending.post, day.next_day_at, models)?
                    .preset
                    .source()
                    .id,
            );
        }
        s.cache = cache;
        if !keep_shown {
            p.queue.rebuild(value, NEXT_BUILD.fetch_add(1, Ordering::Relaxed));
            if let Some(force) = p.force.take() {
                p.queue.set_current(Some(force));
            }
            p.rebuild = false;
            p.restore_cache = false;
            p.label_boundary = Some(day.next_day_at);
            s.built = Stamp {
                seq: rows.seq().0,
                generation: rows.generation().0,
            };
            if day_changed {
                c.clear_undo(crate::ClearCause::Rollover)?;
                s.rollover_pending = false;
            }
        }
    }
    let entries = p.queue.read(at.div_euclid(1000), count);
    let chooser = s.cache.snapshot();
    let mut items = Vec::with_capacity(entries.len());
    let mut boundaries = Vec::new();
    let mut render_keys =
        crate::rendering::KeyBatch::new(&**rows, crate::rendering::RenderMode::Review, session_platform());
    for e in entries {
        let item = project_item(
            &mut render_keys,
            &p.queue,
            &p.overlay,
            e.id,
            e.kind,
            at,
            day.today,
            day.next_day_at,
            &chooser,
            s.undo_view.as_ref(),
            models,
        )?;
        for next in choices(&item.next_states) {
            match next.state {
                meguri_sched::states::engine::State::Learning(l)
                | meguri_sched::states::engine::State::Relearning { learning: l, .. } => {
                    boundaries.push(day.next_day_at.saturating_sub(i64::from(l.scheduled_secs)))
                }
                _ => {}
            }
        }
        items.push(item);
    }
    p.queue.set_current(items.first().map(|i| i.card_id));
    let head = Head {
        terminal_state: if !items.is_empty() {
            ReviewTerminalState::Active
        } else if let Some(instant) = p.queue.waiting_until() {
            ReviewTerminalState::WaitingUntil { instant }
        } else {
            ReviewTerminalState::Finished
        },
        counts: p.queue.value().counts,
        valid_until: p
            .queue
            .valid_until(at.div_euclid(1000), boundaries.into_iter().chain([day.next_day_at])),
        items,
    };
    s.last = head.clone();
    Ok(head)
}
fn emit_empty(inner: &Inner, c: &Collection, id: SessionId, head: &Head) -> Result<(), EngineError> {
    if !head.items.is_empty() {
        return Ok(());
    }
    // Projection releases the state lock; the session may have ended since then.
    let deck = {
        let state = inner.state.lock().unwrap();
        state
            .session
            .as_ref()
            .filter(|s| s.id == id)
            .ok_or_else(no_session)?
            .deck
    };
    if let ReviewTerminalState::WaitingUntil { instant } = head.terminal_state {
        emit(
            c,
            None,
            SessionEvent::WaitingUntil {
                session_id: id,
                instant,
            },
        );
    } else {
        emit(
            c,
            None,
            SessionEvent::Finished {
                session_id: id,
                state: inner.queues.deck_study_state(c, Some(deck))?,
            },
        );
    }
    Ok(())
}

/// The U44-specific handoff. It carries values, never callbacks that re-enter
/// the dispatcher or a session lock on the writer.
pub(crate) struct Application {
    pub assigned: Option<Receipt>,
    pub restorer: Option<ProductionRestore>,
    pub optimistic_cache: bool,
    pub clear_history: bool,
    journal: Arc<Journal>,
    event: u64,
    frozen: FrozenAnswer,
    cancel: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    failed: Arc<AtomicBool>,
    owned: Weak<Inner>,
}
impl Application {
    pub(crate) fn validate(&self, ctx: &mut OpContext<'_, '_>, request: &AnswerRequest) -> Result<(), EngineError> {
        self.journal.check(ctx.generation())?;
        #[cfg(feature = "test-hooks")]
        if self
            .owned
            .upgrade()
            .is_some_and(|i| i.fail_application.load(Ordering::Acquire) == self.event)
        {
            return Err(invalid("injected review application failure"));
        }
        if self.cancel.load(Ordering::Acquire) || self.failed.load(Ordering::Acquire) {
            return cancelled_application(&self.journal, self.event, &self.phase);
        }
        let card = ctx
            .scope()
            .card(request.card)?
            .ok_or_else(|| invalid("answer card missing"))?;
        let inputs = frozen::inputs(ctx.scope(), &card, self.frozen.boundary, &mut FsrsCache::default())?;
        if frozen::context(ctx.scope(), &card, &inputs)? != self.frozen.context {
            return Err(EngineError::Stale {
                table: Table::Cards,
                key: "answer semantic context".into(),
            });
        }
        if row_digest(ctx.scope(), request.card)? != self.journal.resolved_base(&self.frozen.base)? {
            return Err(EngineError::Stale {
                table: Table::Cards,
                key: request.card.0.to_string(),
            });
        }
        Ok(())
    }
    pub(crate) fn prepare(
        &self,
        ctx: &mut OpContext<'_, '_>,
        id: meguri_model::ids::RevlogId,
        base: [u8; 32],
    ) -> Result<(), EngineError> {
        let image = ctx
            .scope()
            .row_image(Table::Revlog, &RowKey::Id(id.0))?
            .ok_or_else(|| invalid("assigned revlog missing"))?;
        let mut row = [0; 9];
        for (out, cell) in row.iter_mut().zip(image) {
            let Cell::Integer(v) = cell else {
                return Err(invalid("assigned revlog storage class"));
            };
            *out = v;
        }
        let receipt = Receipt {
            row,
            base,
            post: row_digest(ctx.scope(), CardId(self.frozen.card))?,
        };
        if self.assigned.as_ref().is_some_and(|r| r != &receipt) {
            return Err(invalid("prepared receipt changed"));
        }
        let journal = self.journal.clone();
        let event = self.event;
        let generation = ctx.generation();
        let cancel = self.cancel.clone();
        let failed = self.failed.clone();
        let phase = self.phase.clone();
        #[cfg(feature = "test-hooks")]
        let before_prepare_hook = self
            .owned
            .upgrade()
            .and_then(|inner| inner.before_prepare_hook.lock().unwrap().clone());
        ctx.before_commit(move || {
            if cancel.load(Ordering::Acquire) || failed.load(Ordering::Acquire) {
                return cancelled_application(&journal, event, &phase);
            }
            #[cfg(feature = "test-hooks")]
            if let Some(hook) = before_prepare_hook {
                hook();
            }
            journal.prepare(event, receipt, generation)?;
            if cancel.load(Ordering::Acquire)
                || failed.load(Ordering::Acquire)
                || phase
                    .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return cancelled_application(&journal, event, &phase);
            }
            Ok(())
        });
        let journal = self.journal.clone();
        let owned = self.owned.clone();
        ctx.on_commit(move |notice| {
            journal.committed(event, notice.generation, notice.seq);
            if let Some(inner) = owned.upgrade() {
                inner.owned.lock().unwrap().insert((notice.generation.0, notice.seq.0));
            }
        });
        Ok(())
    }
}
// Phase 3 is owned by pending undo, unlike a failed answer chain's generic
// cancellation. Keep the writer until that event's terminal record is durable.
// These calls run before the commit lock and hold no session/presentation lock.
fn cancelled_application(journal: &Journal, event: u64, phase: &AtomicU8) -> Result<(), EngineError> {
    if phase.load(Ordering::Acquire) == 3 {
        journal.terminal(event, Terminal::Cancelled)?;
    }
    Err(EngineError::Cancelled)
}
fn recorded_prelude(
    c: &Collection,
    frozen: &FrozenAnswer,
    cancelled: Option<Arc<AtomicBool>>,
) -> Result<(), EngineError> {
    let frozen = frozen.clone();
    let offset = meguri_model::clock::UtcOffset::from_seconds(frozen.offset)
        .ok_or_else(|| invalid("invalid recorded offset"))?;
    let recorded_clock = meguri_model::clock::FixedClock::new(frozen.at, offset);
    c.run(
        OpSpec::named("review_recorded_day", OpSource::Rollover)
            .stamp(StampMode::Raw)
            .recorded_time(recorded_clock),
        move |ctx| {
            if cancelled.as_ref().is_some_and(|c| c.load(Ordering::Acquire)) {
                return Err(EngineError::Cancelled);
            }
            let card = ctx
                .scope()
                .card(CardId(frozen.card))?
                .ok_or_else(|| invalid("replay card missing"))?;
            let inputs = frozen::inputs(ctx.scope(), &card, frozen.boundary, &mut FsrsCache::default())?;
            if frozen::context(ctx.scope(), &card, &inputs)? != frozen.context {
                return Err(EngineError::Stale {
                    table: Table::Cards,
                    key: "recorded policy".into(),
                });
            }
            let report = crate::sched::rollover_op::automatic_day_query(
                ctx.scope(),
                &mut crate::DayCache::default(),
                frozen.at.div_euclid(1000),
                offset,
            )?;
            if report.day.today != frozen.today || report.day.next_day_at != frozen.boundary {
                return Err(invalid("recorded day context mismatch"));
            }
            Ok(())
        },
    )
    .map(drop)
}
fn work_item(inner: &Arc<Inner>, work: Work) -> Result<(), EngineError> {
    match work {
        Work::Stream => {
            inner.wake.queued.store(false, Ordering::Release);
            let waker = Waker::from(inner.wake.clone());
            let mut context = TaskContext::from_waker(&waker);
            loop {
                let mut next = inner.events.next();
                match Pin::new(&mut next).poll(&mut context) {
                    Poll::Ready(Some(event)) => {
                        if matches!(
                            &event.kind,
                            crate::EventKind::RolledOver { .. } | crate::EventKind::NextDayAtChanged { .. }
                        ) && let Ok((c, _)) = bound(inner)
                        {
                            let mut state = inner.state.lock().unwrap();
                            if let Some(s) = state.session.as_mut().filter(|s| s.generation == event.generation) {
                                let old = s.last.clone();
                                let Some(head) = project_background(&c, s, event.seq)? else {
                                    continue;
                                };
                                if old != head {
                                    emit(
                                        &c,
                                        Some(event.seq),
                                        SessionEvent::QueueHeadChanged {
                                            session_id: s.id,
                                            diff: diff(&old.items, head.clone()),
                                        },
                                    );
                                }
                                let empty = Work::Empty { session: s.id, head };
                                drop(state);
                                inner.tx.send(empty).map_err(invalid)?;
                            }
                        }
                    }
                    Poll::Ready(None) | Poll::Pending => break,
                }
            }
            Ok(())
        }
        Work::Stop => Ok(()),
        Work::Empty { session, head } => {
            let (c, _) = bound(inner)?;
            if !inner
                .state
                .lock()
                .unwrap()
                .session
                .as_ref()
                .is_some_and(|s| s.id == session && s.last == head)
            {
                return Ok(());
            }
            emit_empty(inner, &c, session, &head)
        }
        Work::Undo {
            session,
            expected,
            restore,
            rollback,
            cancel,
            progress,
            reply,
        } => {
            let (c, journal) = bound(inner)?;
            // run_op retires WAL/Done evidence before the U25 mutation. No
            // service/presentation lock is held while the writer is occupied.
            let outcome = c
                .submit_answer_undo(UndoScope::Session(session), expected, cancel, progress)
                .wait();
            let failure = match outcome {
                Ok(_) | Err(EngineError::CommitCompleted { .. }) | Err(EngineError::MediaCommitPending { .. }) => None,
                Err(error) => Some(error),
            };
            let mut state = inner.state.lock().unwrap();
            let Some(s) = state.session.as_mut().filter(|s| s.id == session) else {
                return Ok(());
            };
            let old = s.last.items.clone();
            if let Some(error) = failure.clone() {
                restore.restore(crate::Direction::Redo);
                (*rollback).restore(s);
                emit(
                    &c,
                    None,
                    SessionEvent::CommitFailed {
                        session_id: session,
                        card_id: restore.post.id,
                        error,
                    },
                );
            }
            s.restoring = false;
            s.undo_view = None;
            let projected = project(&c, s, 3, None);
            let projection_failed = projected.is_err();
            let head = projected.unwrap_or_else(|error| {
                emit(
                    &c,
                    None,
                    SessionEvent::CommitFailed {
                        session_id: session,
                        card_id: restore.post.id,
                        error,
                    },
                );
                s.last.clone()
            });
            // A durability failure keeps acceptance closed and its evidence
            // intact. Successful restoration may admit subsequent answers.
            s.failed.store(
                projection_failed || journal.check(c.generation()).is_err(),
                Ordering::Release,
            );
            emit(
                &c,
                None,
                SessionEvent::QueueHeadChanged {
                    session_id: session,
                    diff: diff(&old, head.clone()),
                },
            );
            if let Some(reply) = reply {
                let _ = reply.send(match failure {
                    Some(error) => Err(error),
                    None => Ok(head),
                });
            }
            Ok(())
        }
        Work::ApplicationBarrier(reply) => {
            let _ = reply.send(());
            Ok(())
        }
        Work::Barrier(reply, deadline) => {
            let result = bound(inner).and_then(|(_, journal)| journal.barrier_until(deadline));
            let _ = reply.send(result);
            Ok(())
        }
        Work::Answer {
            pending,
            session,
            failed,
            cache,
        } => {
            let (c, journal) = bound(inner)?;
            // A queued callback from a retired session cannot borrow a new
            // incarnation's binding, even when replacement kept the generation.
            if !inner
                .state
                .lock()
                .unwrap()
                .session
                .as_ref()
                .is_some_and(|s| s.id == session && Arc::ptr_eq(&s.journal, &journal))
            {
                return Ok(());
            }
            if pending.cancel.load(Ordering::Acquire)
                || pending
                    .phase
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return Ok(());
            }
            let outcome = (|| {
                journal.barrier()?;
                recorded_prelude(&c, &pending.frozen, Some(pending.cancel.clone()))?;
                let digest = journal.resolved_base(&pending.frozen.base)?;
                let mut request = pending.frozen.request(digest)?;
                request.session = Some(session);
                request.cache = Some(cache.clone());
                answer::submit_session(
                    &c,
                    request,
                    Application {
                        journal: journal.clone(),
                        event: pending.event,
                        frozen: pending.frozen.clone(),
                        assigned: None,
                        cancel: pending.cancel.clone(),
                        phase: pending.phase.clone(),
                        failed: failed.clone(),
                        optimistic_cache: true,
                        clear_history: pending.clear_history,
                        restorer: Some(pending.restorer.clone()),
                        owned: Arc::downgrade(inner),
                    },
                )
                .wait()
            })();
            let committed = match &outcome {
                Ok(_) | Err(EngineError::CommitCompleted { .. }) | Err(EngineError::MediaCommitPending { .. }) => true,
                _ => journal
                    .entries()
                    .iter()
                    .any(|(id, e)| *id == pending.event && e.committed.is_some()),
            };
            if committed {
                pending.phase.store(4, Ordering::Release);
                let mut state = inner.state.lock().unwrap();
                if let Some(s) = state.session.as_mut().filter(|s| s.id == session) {
                    s.presentation.lock().unwrap().overlay.committed(pending.event);
                    s.pending.retain(|p| p.event != pending.event);
                }
                return Ok(());
            }
            if pending.phase.load(Ordering::Acquire) == 3 {
                journal.barrier()?;
                return Ok(());
            }
            let failure = outcome.err().unwrap_or_else(|| invalid("missing answer result"));
            failed.store(true, Ordering::Release);
            let dropped = {
                let mut state = inner.state.lock().unwrap();
                let Some(s) = state.session.as_mut().filter(|s| s.id == session) else {
                    return Err(failure);
                };
                let mut p = s.presentation.lock().unwrap();
                let Presentation { queue, overlay, .. } = &mut *p;
                let changed_build = overlay
                    .pending()
                    .iter()
                    .any(|a| a.sequence >= pending.event && a.queue.build != queue.build());
                let dropped = overlay.rollback_from(pending.event, queue);
                if changed_build {
                    p.rebuild = true;
                }
                for d in &dropped {
                    s.cache.undo_accepted(d.postimage.id);
                }
                let ids = dropped.iter().map(|d| d.sequence).collect::<BTreeSet<_>>();
                for p in &s.pending {
                    if ids.contains(&p.event) {
                        p.cancel.store(true, Ordering::Release);
                    }
                }
                s.pending.retain(|p| !ids.contains(&p.event));
                p.queue.set_current(Some(CardId(pending.frozen.card)));
                dropped
            };
            let terminal = (|| {
                journal.terminal(pending.event, Terminal::Dropped)?;
                for d in dropped.iter().filter(|d| d.sequence != pending.event) {
                    journal.terminal(d.sequence, Terminal::Cancelled)?;
                }
                Ok::<(), EngineError>(())
            })();
            emit(
                &c,
                None,
                SessionEvent::CommitFailed {
                    session_id: session,
                    card_id: CardId(pending.frozen.card),
                    error: terminal.as_ref().err().unwrap_or(&failure).clone(),
                },
            );
            let mut state = inner.state.lock().unwrap();
            if let Some(s) = state.session.as_mut().filter(|s| s.id == session) {
                let old = s.last.items.clone();
                let head = project(&c, s, 3, None)?;
                emit(
                    &c,
                    None,
                    SessionEvent::QueueHeadChanged {
                        session_id: session,
                        diff: diff(&old, head),
                    },
                );
                failed.store(terminal.is_err(), Ordering::Release);
            }
            Ok(())
        }
        Work::Observe {
            generation,
            seq,
            changes,
            payload,
        } => {
            let (c, _) = bound(inner)?;
            if c.generation() != generation {
                return Ok(());
            }
            #[cfg(feature = "test-hooks")]
            {
                let hook = inner.observe_hook.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(&payload);
                }
            }
            let owned = inner.owned.lock().unwrap().remove(&(generation.0, seq.0));
            let mut state = inner.state.lock().unwrap();
            if state.paused {
                return Ok(());
            }
            let Some(s) = state.session.as_mut() else { return Ok(()) };
            if s.generation != generation || seq.0 <= s.built.seq {
                return Ok(());
            }
            let stamp = Stamp {
                seq: seq.0,
                generation: generation.0,
            };
            let action = rebuild_rules::classify_with(
                &changes,
                Context {
                    built: s.built,
                    commit: stamp,
                    owned: owned.then_some(stamp),
                    day_changed: false,
                },
                |id| payload.flag_only(id),
            );
            let action = match action {
                Ok(action) => action,
                Err(error) => {
                    let mut presentation = s.presentation.lock().unwrap();
                    presentation.rebuild = true;
                    presentation.restore_cache = false;
                    s.cache.invalidate();
                    // Do not project or emit a successful partial head. A later
                    // request uses the existing rebuild path.
                    return Err(error.into());
                }
            };
            if owned {
                return Ok(());
            }
            let old = s.last.clone();
            let current = old.items.first().map(|h| h.card_id);
            if action == Action::Rebuild {
                let mut presentation = s.presentation.lock().unwrap();
                presentation.rebuild = true;
                presentation.restore_cache = false;
            }
            let Some(head) = project_background(&c, s, seq)? else {
                return Ok(());
            };
            if let Some(card) = current {
                let prior = old.items.first().unwrap();
                let item = head.items.iter().find(|h| h.card_id == card);
                if item.is_none()
                    || item.is_some_and(|h| h.state_token != prior.state_token || h.render_key != prior.render_key)
                {
                    let change = if item.is_none() {
                        CurrentChange::Removed
                    } else if changes.cards.iter().any(|c| c.id == card && c.memory_state_only) {
                        CurrentChange::MemoryOnly
                    } else {
                        CurrentChange::Eligible
                    };
                    emit(
                        &c,
                        Some(seq),
                        SessionEvent::CurrentCardChanged {
                            session_id: s.id,
                            card_id: card,
                            change,
                        },
                    );
                }
            }
            if old != head {
                emit(
                    &c,
                    Some(seq),
                    SessionEvent::QueueHeadChanged {
                        session_id: s.id,
                        diff: diff(&old.items, head.clone()),
                    },
                );
            }
            let empty = Work::Empty { session: s.id, head };
            drop(state);
            inner.tx.send(empty).map_err(invalid)?;
            Ok(())
        }
    }
}

type NativeBinding = (
    (u64, u64),
    meguri_storage::persistent_identity::PersistentIdentity,
    [u64; 2],
);

pub(super) fn native_binding(c: &Collection) -> Result<NativeBinding, EngineError> {
    c.core
        .exec
        .control(crate::suspend::Control::ReviewIdentity)
        .wait()?
        .review_identity
        .ok_or_else(|| invalid("missing locked file identity"))
}
fn replay(c: &Collection, journal: &Arc<Journal>) -> Result<(), EngineError> {
    if c.privilege.is_none() {
        return Err(invalid("review recovery is opening-only"));
    }
    // Historical maintenance must not schedule live day-clock work. Restore
    // that behavior on every exit; the final open step computes today's day.
    struct HistoricalOpen<'a> {
        executor: &'a crate::executor::ExecShared,
    }
    impl Drop for HistoricalOpen<'_> {
        fn drop(&mut self) {
            self.executor.replaying_review.store(false, Ordering::Release);
            self.executor.day_clock.reset();
        }
    }
    c.core.exec.replaying_review.store(true, Ordering::Release);
    let _historical = HistoricalOpen { executor: &c.core.exec };
    journal.barrier()?;
    for (event, entry) in journal.entries() {
        if entry.terminal.is_some() {
            continue;
        }
        if let Some(receipt) = &entry.receipt {
            let assigned_row = {
                let reader = c.reader(ReaderRole::Interactive)?;
                reader.row_image(Table::Revlog, &RowKey::Id(receipt.row[0]))?
            };
            if let Some(stored) = assigned_row {
                let identical = stored.len() == receipt.row.len()
                    && stored
                        .iter()
                        .zip(receipt.row)
                        .all(|(cell, number)| cell == &Cell::Integer(number));
                if identical {
                    journal.committed(event, c.generation(), c.last_seq());
                    c.core.exec.control(crate::suspend::Control::Flush).wait()?;
                } else {
                    journal.terminal(event, Terminal::Dropped)?;
                    emit(
                        c,
                        None,
                        SessionEvent::RecoveryDropped {
                            event,
                            card_id: CardId(entry.answer.card),
                            reason: "prepared revlog row differs from the stored row".into(),
                        },
                    );
                }
                continue;
            }
        }
        let application = (|| {
            let base = journal.resolved_base(&entry.answer.base)?;
            // C12.6 validates context before recorded-time maintenance. U44
            // then checks the card base against the resulting database state.
            recorded_prelude(c, &entry.answer, None)?;
            let request = entry.answer.request(base)?;
            let replay = Application {
                assigned: entry.receipt.clone(),
                restorer: None,
                optimistic_cache: false,
                clear_history: false,
                journal: Arc::clone(journal),
                event,
                frozen: entry.answer.clone(),
                cancel: Arc::new(AtomicBool::new(false)),
                phase: Arc::new(AtomicU8::new(1)),
                failed: Arc::new(AtomicBool::new(false)),
                owned: Weak::new(),
            };
            answer::submit_replay(c, request, replay).wait()
        })();
        if let Err(error) = application {
            match error {
                EngineError::CommitCompleted { .. } | EngineError::MediaCommitPending { .. } => {}
                semantic @ (EngineError::Stale { .. }
                | EngineError::Unsupported { .. }
                | EngineError::InvalidArgument { .. }) => {
                    // A journal error prevents semantic resolution. Keep its
                    // recovery evidence and let the open fail with that error.
                    journal.check(c.generation())?;
                    journal.terminal(event, Terminal::Dropped)?;
                    emit(
                        c,
                        None,
                        SessionEvent::RecoveryDropped {
                            event,
                            card_id: CardId(entry.answer.card),
                            reason: semantic.to_string(),
                        },
                    );
                    continue;
                }
                other => return Err(other),
            }
        }
        // A successful transaction, including a lost post-commit result,
        // needs real WAL coverage before the journal can record Done.
        c.core.exec.control(crate::suspend::Control::Flush).wait()?;
    }
    c.core.exec.control(crate::suspend::Control::Flush).wait()?;
    journal.barrier()?;
    journal.compact()?;
    Ok(())
}
impl OpenStep for ReviewService {
    fn name(&self) -> &str {
        "review-session"
    }
    fn run(&self, c: &Collection) -> Result<(), EngineError> {
        let journal = super::journal::managed::open(c, &self.engine, &self.threads)?;
        *c.core.exec.review_journal.lock().unwrap() = Some(Arc::downgrade(&journal));
        let recovery = replay(c, &journal).and_then(|()| {
            // Re-read through the retained Writer descriptor after recovery,
            // before the file worker publishes a migrated identity.
            let (live, persistent, _) = native_binding(c)?;
            journal.finish_identity_migration(live, persistent)
        });
        if let Err(e) = recovery {
            *c.core.exec.review_journal.lock().unwrap() = None;
            return Err(e);
        }
        let mut state = self.inner.state.lock().unwrap();
        state.binding = Some(Binding {
            core: Arc::downgrade(&c.core),
            generation: c.generation(),
            journal,
        });
        state.paused = false;
        self.inner.observing.store(true, Ordering::Release);
        Ok(())
    }
}
impl ChangeObserver for ReviewService {
    fn name(&self) -> &str {
        "review-session"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        if !self.inner.observing.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner
            .tx
            .send(Work::Observe {
                generation: commit.generation,
                seq: commit.seq,
                changes: Box::new(commit.changes.clone()),
                payload: commit.payload.clone(),
            })
            .map_err(invalid)
    }
}
impl GenerationListener for ReviewService {
    fn name(&self) -> &str {
        "review-session"
    }
    fn collection_reopened(&self, c: &Collection) -> Result<(), EngineError> {
        let retired = {
            let mut state = self.inner.state.lock().unwrap();
            state.paused = true;
            state.session = None;
            state.render.clear();
            self.inner.owned.lock().unwrap().clear();
            // Drop the retired binding; queued old work must never find it
            // under the reopened generation (which may be unchanged).
            state.binding.take()
        };
        if let Some(binding) = &retired {
            if !Weak::ptr_eq(&binding.core, &Arc::downgrade(&c.core)) {
                return Err(invalid("pending_answer_identity_mismatch"));
            }
            super::journal::managed::reset_after_replace(c, &self.engine, &binding.journal)?;
        }
        drop(retired);
        self.run(c)?;
        // Replay discards its historical day state. ReplaceTask has already
        // run L8's first check; restore the quiet current D8 cache before
        // fallible rebind returns and admission can reopen.
        c.check_rollover()?;
        Ok(())
    }
    fn collection_replaced(&self, _old: Generation, _new: Generation) {}
}
impl SuspendParticipant for ReviewService {
    fn name(&self) -> &str {
        "review-session"
    }
    fn flush(&self) -> Result<(), EngineError> {
        self.drain_until(crate::monotonic::now() + Duration::from_secs(5))
    }
    fn flush_until(&self, deadline: Duration) -> Result<(), EngineError> {
        self.drain_until(deadline)
    }
    fn suspend(&self) -> Result<(), EngineError> {
        self.inner.state.lock().unwrap().paused = true;
        Ok(())
    }
    fn resume_with_collection(&self, c: &Collection) -> Result<(), EngineError> {
        let (native, persistent, _) = native_binding(c)?;
        let mut state = self.inner.state.lock().unwrap();
        if let Some(b) = &state.binding {
            b.journal.check(c.generation())?;
            b.journal.check_identity(native)?;
            b.journal.check_persistent_identity(persistent)?;
        }
        state.paused = false;
        c.core.exec.review_accepting.store(true, Ordering::Release);
        if let Some(s) = &mut state.session {
            s.presentation.lock().unwrap().rebuild = true;
        }
        Ok(())
    }
}
impl Drop for ReviewService {
    fn drop(&mut self) {
        let _ = self.inner.tx.send(Work::Stop);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

/// Collection-only review authority for protocol actors (S13). Uses the same
/// registry and policy reservation as C5; owns no queue or selected deck.
pub struct UnqueuedSession {
    collection: Collection,
    id: SessionId,
    ended: bool,
}
impl std::fmt::Debug for UnqueuedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnqueuedSession").finish_non_exhaustive()
    }
}
impl UnqueuedSession {
    pub fn id(&self) -> SessionId {
        self.id
    }
    pub fn collection(&self) -> Result<Collection, EngineError> {
        self.collection.check_current()?;
        let mut c = self.collection.clone();
        c.undo_session = Some(self.id);
        Ok(c)
    }
    pub fn end(mut self) -> Result<(), EngineError> {
        self.release()?;
        self.ended = true;
        Ok(())
    }
    /// Queue-free actor actions still use U48 application and real undo tagging.
    pub fn apply(
        &self,
        request: crate::sched::ops::Request,
    ) -> Result<crate::OpResult<crate::sched::ops::ResultValue>, EngineError> {
        if *self.collection.core.exec.active_session.lock().unwrap() != Some(self.id) {
            return Err(EngineError::SessionActive);
        }
        crate::sched::ops::run_admitted(&self.collection, request, Some(self.id), &Default::default())
    }
    fn release(&self) -> Result<(), EngineError> {
        let exec = self.collection.core.exec.clone();
        let id = self.id;
        self.collection
            .run(OpSpec::named("end_unqueued_review", OpSource::Rollover), move |ctx| {
                ctx.on_commit(move |_| {
                    let mut active = exec.active_session.lock().unwrap();
                    if *active == Some(id) {
                        *active = None;
                        *exec.fsrs_policy.review_active.lock().unwrap() = false;
                    }
                });
                Ok(())
            })?;
        Ok(())
    }
}
impl Drop for UnqueuedSession {
    fn drop(&mut self) {
        if !self.ended {
            let _ = self.release();
        }
    }
}
impl ReviewService {
    pub fn begin_unqueued_session(&self) -> Result<UnqueuedSession, EngineError> {
        let (idle_collection, _) = self.bound()?;
        let _idle = idle_collection
            .core
            .exec
            .maintenance
            .get()
            .and_then(std::sync::Weak::upgrade)
            .map(|m| m.pause_idle());
        let _permit = self.control.acquire();
        let (collection, journal) = self.bound()?;
        journal.check(collection.generation())?;
        let id = SessionId(NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
        let exec = collection.core.exec.clone();
        let hooks = self.start_hooks.lock().unwrap().clone();
        collection.run(OpSpec::named("begin_unqueued_review", OpSource::Rollover), move |ctx| {
            if exec.active_session.lock().unwrap().is_some() || *exec.fsrs_policy.review_active.lock().unwrap() {
                return Err(EngineError::SessionActive);
            }
            for hook in hooks {
                hook.session_starting();
            }
            ctx.on_commit(move |_| {
                *exec.fsrs_policy.review_active.lock().unwrap() = true;
                *exec.active_session.lock().unwrap() = Some(id);
            });
            Ok(())
        })?;
        Ok(UnqueuedSession {
            collection,
            id,
            ended: false,
        })
    }
}
