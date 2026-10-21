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
        rebuild_rules::{self, Action, CardImages, Context, Stamp},
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
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    task::{Context as TaskContext, Poll, Wake, Waker},
    thread::JoinHandle,
    time::Duration,
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static NEXT_BUILD: AtomicU64 = AtomicU64::new(1);
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
    pub show_timer: bool,
    pub stop_timer_on_answer: bool,
    pub maximum_answer_seconds: u32,
    pub question_seconds: f32,
    pub answer_seconds: f32,
    pub question_action: i32,
    pub answer_action: i32,
    pub wait_for_audio: bool,
    pub leech_action: i32,
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
pub struct Head {
    pub items: Vec<HeadItem>,
    pub counts: Counts,
    pub valid_until: i64,
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
    redo: Vec<FrozenAnswer>,
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
struct Inner {
    events: crate::Subscription,
    wake: Arc<StreamWake>,
    #[cfg(feature = "test-hooks")]
    fail_application: AtomicU64,
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
    },
    Observe {
        generation: Generation,
        seq: Seq,
        changes: Box<meguri_model::changes::ChangeSet>,
        cards: Vec<crate::CardDelta>,
    },
    Barrier(mpsc::SyncSender<Result<(), EngineError>>, Duration),
    #[cfg(feature = "test-hooks")]
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
pub struct ReviewService {
    inner: Arc<Inner>,
    threads: crate::ThreadRegistry,
    start_hooks: Mutex<Vec<Arc<dyn SessionStartHook>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    control: Mutex<()>,
    renders: AtomicU64,
    #[cfg(feature = "test-hooks")]
    render_hook: Mutex<Option<RenderHook>>,
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
            observing: AtomicBool::new(false),
            state: Mutex::new(State {
                binding: None,
                session: None,
                render: VecDeque::new(),
                paused: false,
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
            threads: engine.threads().clone(),
            start_hooks: Mutex::new(vec![Arc::new(engine.jobs().clone())]),
            worker: Mutex::new(Some(worker)),
            control: Mutex::new(()),
            renders: AtomicU64::new(0),
            #[cfg(feature = "test-hooks")]
            render_hook: Mutex::new(None),
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
        self.inner.state.lock().unwrap().session.as_ref().map(|s| s.id)
    }
    pub fn start(&self, deck: DeckId, resume: Option<CardId>) -> Result<StartResult, EngineError> {
        let _control = self.control.lock().unwrap();
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
                let item = project_item(
                    &*rows,
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
                )?
                .0;
                Ok(PeekResult::Card(item))
            }
        }
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
        if state.paused {
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
            state_token: captured.digest,
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
            .unwrap_or(Base::Row(captured.digest));
        let frozen = FrozenAnswer::capture(
            &request,
            base,
            captured.inputs.context,
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
            if let Some(instant) = session.presentation.lock().unwrap().queue.waiting_until() {
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
        let _control = self.control.lock().unwrap();
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
        let (c, _) = self.bound()?;
        let state = self.inner.state.lock().unwrap();
        if let UndoScope::Session(id) = scope {
            let session = state.session.as_ref().filter(|s| s.id == id).ok_or_else(no_session)?;
            if let Some(pending) = session.pending.last() {
                return Ok(UndoStatus::Available(crate::StepInfo {
                    id: crate::EntryId(pending.event),
                    name: crate::undo::names::ANSWER_CARD,
                    kind: crate::StepKind::Answer,
                    session: Some(id),
                    long_write: None,
                }));
            }
        }
        drop(state);
        c.peek_undo(scope)
    }
    pub fn undo(&self, scope: UndoScope) -> Result<Head, EngineError> {
        let (c, journal) = self.bound()?;
        if let UndoScope::Session(id) = scope {
            let mut state = self.inner.state.lock().unwrap();
            let s = state.session.as_mut().filter(|s| s.id == id).ok_or_else(no_session)?;
            if s.restoring || s.failed.load(Ordering::Acquire) {
                return Err(EngineError::unavailable("answer chain is restoring"));
            }
            if let Some(pending) = s.pending.last().cloned().filter(|pending| {
                pending
                    .phase
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |p| (p < 2).then_some(3))
                    .is_ok()
            }) {
                pending.cancel.store(true, Ordering::Release);
                let mut p = s.presentation.lock().unwrap();
                let Presentation { queue, overlay, .. } = &mut *p;
                overlay.rollback_from(pending.event, queue);
                queue.set_current(Some(CardId(pending.frozen.card)));
                drop(p);
                s.cache.undo_accepted(CardId(pending.frozen.card));
                s.pending.pop();
                s.redo.push(pending.frozen.clone());
                // Cancellation's file acknowledgement is required before a
                // conflicting operation; UI restoration itself happens above.
                drop(state);
                journal.terminal(pending.event, Terminal::Cancelled)?;
                return self.head(id, 3);
            }
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
                if self
                    .inner
                    .tx
                    .send(Work::Undo {
                        session: id,
                        expected,
                        restore: Box::new(restore.clone()),
                        rollback: Box::new(rollback.clone()),
                    })
                    .is_err()
                {
                    restore.restore(crate::Direction::Redo);
                    rollback.restore(s);
                    s.restoring = false;
                    s.failed.store(false, Ordering::Release);
                    return Err(EngineError::unavailable("review worker stopped"));
                }
                return Ok(head);
            }
        }
        self.flush_for_background()?;
        c.undo(scope)?;
        let id = self.active_session().ok_or_else(no_session)?;
        self.head(id, 3)
    }
    pub fn redo(&self, scope: UndoScope) -> Result<Head, EngineError> {
        if self
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
                if session.restoring || session.failed.load(Ordering::Acquire) {
                    return Err(EngineError::unavailable("answer chain is restoring"));
                }
                session.redo.last().cloned()
            };
            if let Some(frozen) = frozen {
                let h = self.head(id, 10)?;
                let item = h
                    .items
                    .iter()
                    .find(|h| h.card_id.0 == frozen.card)
                    .ok_or_else(|| invalid("redo card no longer presented"))?;
                let request = frozen.request([0; 32])?;
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
                return self.head(id, 3);
            }
        }
        c.redo(scope)?;
        let id = self.active_session().ok_or_else(no_session)?;
        self.head(id, 3)
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
    digest: [u8; 32],
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
    let note_id = card.note_id;
    let mut render =
        crate::rendering::Capture::read(rows, card, crate::rendering::RenderMode::Review, session_platform())?;
    if let Some(entry) = undo
        && let Some(change) = entry.journal.entry(Table::Notes, &RowKey::Id(note_id.0))
        && let Some(before) = entry.before(change)?
    {
        render.note = meguri_model::note::Note::from_cells(&before).map_err(meguri_storage::StorageError::from)?;
    }
    Ok(render)
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
    let digest = meguri_storage::journal::digest(&image);
    Ok((
        token,
        states,
        Captured {
            image,
            card,
            inputs,
            digest,
        },
    ))
}
#[allow(clippy::too_many_arguments)]
fn project_item(
    rows: &impl ReadRows,
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
) -> Result<(HeadItem, Captured), EngineError> {
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
    let render = review_capture(rows, card.clone(), undo)?;
    let p = inputs.preset.fields();
    let item = HeadItem {
        card_id: id,
        note_id: card.note_id,
        kind: if kind == Kind::InterdayLearning {
            Kind::Learning
        } else {
            kind
        },
        state_token: token,
        render_key: render.key(),
        next_states: states,
        predicted_next: predicted,
        flag: card.flags & 7,
        marked: crate::tags::canonical::split_text(&render.note.tags).any(|t| t.to_lowercase() == "marked"),
        home_preset: HomePreset {
            id: inputs.preset.source().id,
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
        },
    };
    Ok((item, captured))
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
    for e in entries {
        let (item, _) = project_item(
            &**rows,
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
    let (waiting, deck) = {
        let state = inner.state.lock().unwrap();
        let s = state.session.as_ref().filter(|s| s.id == id).ok_or_else(no_session)?;
        let p = s.presentation.lock().unwrap();
        (p.queue.waiting_until(), s.deck)
    };
    if let Some(instant) = waiting {
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
            return Err(EngineError::Cancelled);
        }
        let card = ctx
            .scope()
            .card(request.card)?
            .ok_or_else(|| invalid("answer card missing"))?;
        let inputs = frozen::inputs(ctx.scope(), &card, self.frozen.boundary, &mut FsrsCache::default())?;
        if inputs.context != self.frozen.context {
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
        ctx.before_commit(move || {
            if cancel.load(Ordering::Acquire) || failed.load(Ordering::Acquire) {
                return Err(EngineError::Cancelled);
            }
            journal.prepare(event, receipt, generation)?;
            if cancel.load(Ordering::Acquire)
                || failed.load(Ordering::Acquire)
                || phase
                    .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return Err(EngineError::Cancelled);
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
            if frozen::inputs(ctx.scope(), &card, frozen.boundary, &mut FsrsCache::default())?.context != frozen.context
            {
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
        } => {
            let (c, journal) = bound(inner)?;
            // run_op retires WAL/Done evidence before the U25 mutation. No
            // service/presentation lock is held while the writer is occupied.
            let outcome = c.submit_answer_undo(UndoScope::Session(session), expected).wait();
            let failure = match outcome {
                Ok(_) | Err(EngineError::CommitCompleted { .. }) | Err(EngineError::MediaCommitPending { .. }) => None,
                Err(error) => Some(error),
            };
            let mut state = inner.state.lock().unwrap();
            let Some(s) = state.session.as_mut().filter(|s| s.id == session) else {
                return Ok(());
            };
            let old = s.last.items.clone();
            if let Some(error) = failure {
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
                    diff: diff(&old, head),
                },
            );
            Ok(())
        }
        #[cfg(feature = "test-hooks")]
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
            cards,
        } => {
            let (c, _) = bound(inner)?;
            if c.generation() != generation {
                return Ok(());
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
            let images = cards
                .iter()
                .map(|c| CardImages {
                    id: c.id,
                    before: c.before.as_deref(),
                    after: c.after.as_deref(),
                })
                .collect::<Vec<_>>();
            let action = rebuild_rules::classify(
                &changes,
                &images,
                Context {
                    built: s.built,
                    commit: stamp,
                    owned: owned.then_some(stamp),
                    day_changed: false,
                },
            );
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

fn native_binding(c: &Collection) -> Result<((u64, u64), [u64; 2]), EngineError> {
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
        let (identity, incarnation) = native_binding(c)?;
        let journal = Journal::open(c.path(), identity, incarnation, c.generation(), &self.threads)?;
        *c.core.exec.review_journal.lock().unwrap() = Some(Arc::downgrade(&journal));
        if let Err(e) = replay(c, &journal) {
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
                cards: commit.cards.to_vec(),
            })
            .map_err(invalid)
    }
}
impl GenerationListener for ReviewService {
    fn name(&self) -> &str {
        "review-session"
    }
    fn collection_reopened(&self, c: &Collection) -> Result<(), EngineError> {
        {
            let mut state = self.inner.state.lock().unwrap();
            state.paused = true;
            state.session = None;
            state.render.clear();
            self.inner.owned.lock().unwrap().clear();
            // Drop the retired binding; queued old work must never find it
            // under the reopened generation (which may be unchanged).
            state.binding = None;
        }
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
        let (native, _) = native_binding(c)?;
        let mut state = self.inner.state.lock().unwrap();
        if let Some(b) = &state.binding {
            b.journal.check(c.generation())?;
            b.journal.check_identity(native)?;
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
