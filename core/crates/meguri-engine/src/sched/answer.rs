//! One answer transaction, with pure U45/U46 predictions and U25 restoration.
use crate::{
    Collection, Direction, EngineError, OpResult, OpSource, OpSpec, Precondition, RestoredStep, SessionId, StepKind,
    UndoRestorer, changes::observers::ObservedCommit, collection::CollectionCore,
};
use meguri_model::{
    card::{Card, CardQueue, CardType},
    config::ConfigEntry,
    ids::{CardId, DeckConfigId, RevlogId},
    proto::deck::DeckKind,
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_sched::{
    answer::{self as values, Rating},
    load_balancer::{Entry, LoadBalancer},
    model::{deck_sched::DeckSched, preset::PresetView, sched_config::SchedConfig},
    queue::{
        Kind,
        rebuild_rules::{self, Action, CardImages, Context, Stamp},
    },
    states::{
        engine::{AnswerState, SchedulingStates, State, StateInput, next_states},
        fsrs_glue::FsrsCache,
        fuzz,
    },
};
use meguri_storage::{ReadRows, ReaderRole, RowKey, StampMode, StorageError, Table};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

static NEXT_BUILD: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheBuildId(pub u64);

/// A cache is bound to one open collection, one generation and one real build.
/// Invalidated handles cannot mutate or supply predictions for a new build.
pub struct AnswerCache {
    collection: Weak<CollectionCore>,
    generation: crate::Generation,
    build: CacheBuildId,
    built: Stamp,
    scenario: bool,
    snapshot_day: bool,
    enabled: bool,
    checked_second: Option<i64>,
    built_at: i64,
    offset: meguri_model::clock::UtcOffset,
    zone: String,
    valid: AtomicBool,
    value: Mutex<LoadBalancer>,
}
impl std::fmt::Debug for AnswerCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnswerCache").field("build", &self.build).finish()
    }
}
impl AnswerCache {
    pub fn build(collection: &Collection) -> Result<Option<Arc<Self>>, EngineError> {
        Self::build_mode(collection, false)
    }
    /// H2/H3's cache lifetime differs from a product queue's config classifier.
    pub fn build_scenario(collection: &Collection) -> Result<Option<Arc<Self>>, EngineError> {
        Self::build_mode(collection, true)
    }
    /// H3 also needs a registered change witness when balancing is off.
    pub fn build_scenario_held(collection: &Collection) -> Result<Arc<Self>, EngineError> {
        Self::build_mode_inner(collection, true, true)?.ok_or_else(|| invalid("missing held build witness"))
    }
    pub(crate) fn build_session(collection: &Collection) -> Result<Arc<Self>, EngineError> {
        Self::build_mode_inner(collection, false, true)?.ok_or_else(|| invalid("missing session cache"))
    }
    pub(crate) fn accept(&self, card: &Card, preset: DeckConfigId) {
        if self.enabled && card.queue == CardQueue::Review {
            self.value.lock().unwrap().append(
                preset,
                card.interval,
                Entry {
                    card: card.id,
                    note: card.note_id,
                },
            );
        }
    }
    pub(crate) fn from_session_snapshot(
        collection: &Collection,
        rows: &crate::Snapshot<'_>,
        today: u32,
        boundary: i64,
        at: i64,
    ) -> Result<Arc<Self>, EngineError> {
        Self::from_snapshot(
            collection,
            rows,
            super::DayReport {
                today,
                next_day_at: boundary,
                rolled_over_from: None,
            },
            at,
            false,
            true,
            true,
        )?
        .ok_or_else(|| invalid("missing session build cache"))
    }
    pub(crate) fn restore_view(&self, value: LoadBalancer) {
        *self.value.lock().unwrap() = value;
    }
    pub(crate) fn undo_accepted(&self, card: CardId) {
        self.value.lock().unwrap().remove_answer(card);
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[cfg(feature = "test-hooks")]
    pub fn notify_before_next_reader(collection: &Collection, sender: std::sync::mpsc::Sender<ReaderRole>) {
        collection.core.exec.answer_caches.1.notify_before_next_reader(sender);
    }
    fn build_mode(collection: &Collection, scenario: bool) -> Result<Option<Arc<Self>>, EngineError> {
        Self::build_mode_inner(collection, scenario, false)
    }
    fn build_mode_inner(
        collection: &Collection,
        scenario: bool,
        hold_disabled: bool,
    ) -> Result<Option<Arc<Self>>, EngineError> {
        loop {
            let cache = if scenario {
                let seq = collection.last_seq();
                collection.core.exec.stream.wait_settled(seq);
                let lease = collection.reader(ReaderRole::Interactive)?;
                let rows = lease.snapshot()?;
                if rows.seq() != seq {
                    drop(rows);
                    drop(lease);
                    continue;
                }
                // Q2.2: scenario queue reads compute timing without maintenance.
                let clock = collection.core.exec.clock.as_ref();
                let at = clock.now_ms();
                let config = SchedConfig::new(&*rows);
                let day = meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
                    crt: rows.col()?.created.0,
                    creation_offset: config.creation_offset()?,
                    rollover_hour: config.rollover_hour()?,
                    offset: clock.utc_offset_at(at),
                    now: at.div_euclid(1000),
                });
                Self::from_snapshot(
                    collection,
                    &rows,
                    super::day_clock::DayReport {
                        today: day.today,
                        next_day_at: day.next_day_at,
                        rolled_over_from: None,
                    },
                    at,
                    true,
                    hold_disabled,
                    false,
                )?
            } else {
                collection.core.exec.answer_caches.1.read_timed(
                    collection,
                    ReaderRole::Interactive,
                    |rows, env, at| {
                        Self::from_snapshot(
                            collection,
                            rows,
                            super::day_clock::DayReport {
                                today: env.today,
                                next_day_at: env.next_day_at,
                                rolled_over_from: None,
                            },
                            at,
                            false,
                            hold_disabled,
                            false,
                        )
                    },
                )?
            };
            // Registration raced a commit or the clock moved during the build.
            // Release the snapshot before starting another maintenance prelude.
            if cache.as_ref().is_some_and(|c| !c.is_current(collection)) {
                continue;
            }
            return Ok(cache);
        }
    }
    fn from_snapshot(
        collection: &Collection,
        rows: &crate::readers::Snapshot<'_>,
        day: super::day_clock::DayReport,
        at: i64,
        scenario: bool,
        hold_disabled: bool,
        snapshot_day: bool,
    ) -> Result<Option<Arc<Self>>, EngineError> {
        let (today, next_day_at) = (day.today, day.next_day_at);
        let clock = collection.core.exec.clock.as_ref();
        let offset = clock.utc_offset_at(at);
        let zone = clock.zone_id();
        let enabled = SchedConfig::new(&**rows).load_balancer_enabled()?;
        if !enabled && !hold_disabled {
            return Ok(None);
        }
        let decks = rows
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
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let counted = rows
            .answer_due_range(today)?
            .into_iter()
            .filter_map(|r| {
                decks.get(&r.deck).map(|p| {
                    (
                        *p,
                        r.day,
                        Entry {
                            card: r.card,
                            note: r.note,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        let value = LoadBalancer::new(today, next_day_at, clock, presets, counted)
            .map_err(|e| EngineError::InvalidArgument { message: e.to_string() })?;
        let built = Stamp {
            seq: rows.seq().0,
            generation: rows.generation().0,
        };
        let cache = Arc::new(Self {
            collection: Arc::downgrade(&collection.core),
            generation: collection.generation(),
            build: CacheBuildId(NEXT_BUILD.fetch_add(1, Ordering::Relaxed)),
            built,
            scenario,
            snapshot_day,
            enabled,
            checked_second: (!scenario && at.div_euclid(1000) == next_day_at).then_some(at.div_euclid(1000)),
            built_at: at.div_euclid(1000),
            offset,
            zone,
            valid: AtomicBool::new(true),
            value: Mutex::new(value),
        });
        collection
            .core
            .exec
            .answer_caches
            .0
            .lock()
            .unwrap()
            .push(Arc::downgrade(&cache));
        // A commit between the snapshot and registration must not be missed.
        if collection.last_seq() != rows.seq()
            || (!scenario
                && !snapshot_day
                && !collection.core.exec.day_clock.queue_report_is_current(
                    super::day_clock::DayReport {
                        today,
                        next_day_at,
                        rolled_over_from: None,
                    },
                    at.div_euclid(1000),
                    cache.offset,
                    &cache.zone,
                ))
        {
            cache.invalidate();
        }
        Ok(Some(cache))
    }
    pub fn identity(&self) -> CacheBuildId {
        self.build
    }
    pub fn snapshot(&self) -> LoadBalancer {
        self.value.lock().unwrap().clone()
    }
    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }
    pub fn is_current(&self, collection: &Collection) -> bool {
        self.bound_and_time_current(collection)
            && (self.scenario
                || self.snapshot_day
                || self.checked_second.is_none()
                || collection
                    .core
                    .exec
                    .day_clock
                    .queue_day_is_current(self.snapshot().today, self.next_day_at()))
    }
    fn bound_and_time_current(&self, collection: &Collection) -> bool {
        self.valid.load(Ordering::Acquire)
            && self.generation == collection.generation()
            && collection.is_current()
            && self
                .collection
                .upgrade()
                .is_some_and(|core| Arc::ptr_eq(&core, &collection.core))
            && (self.scenario || {
                let clock = collection.core.exec.clock.as_ref();
                let at = clock.now_ms();
                let now = at.div_euclid(1000);
                // The exact-second proof comes only from the validated native
                // build, with the same clock inputs and no extra boundary second.
                now >= self.built_at
                    && clock.utc_offset_at(at) == self.offset
                    && clock.zone_id() == self.zone
                    && (now < self.next_day_at() || self.checked_second == Some(now))
            })
    }
    pub fn next_day_at(&self) -> i64 {
        self.value.lock().unwrap().next_day_at
    }
    pub fn with_chooser<T>(
        &self,
        collection: &Collection,
        card: &Card,
        preset: &PresetView,
        run: impl FnOnce(Option<&mut dyn meguri_sched::states::interval_limits::FuzzedDayChooser>) -> T,
    ) -> T {
        if !self.enabled || !self.is_current(collection) {
            return run(None);
        }
        let value = self.value.lock().unwrap();
        let note = preset.fields().bury_reviews.then_some(card.note_id);
        let mut chooser = value.chooser(
            preset.source().id,
            note,
            fuzz::seed(card.id.0 as u64, u64::from(card.reps)),
        );
        run(Some(&mut chooser))
    }
}

/// Per-collection weak registrations; no cache or session is kept alive here.
#[derive(Default)]
pub(crate) struct AnswerCaches(Mutex<Vec<Weak<AnswerCache>>>, super::queue::QueueProvider);
impl AnswerCaches {
    pub(crate) fn invalidate_all(&self) {
        self.0.lock().unwrap().retain(|w| {
            if let Some(c) = w.upgrade() {
                c.invalidate();
                true
            } else {
                false
            }
        });
    }
    pub(crate) fn has_live(&self) -> bool {
        self.0.lock().unwrap().iter().any(|w| w.strong_count() > 0)
    }
    pub(crate) fn observe(&self, c: &ObservedCommit<'_>, owner: Option<CacheBuildId>, restoring: bool) {
        self.0.lock().unwrap().retain(|w| {
            let Some(cache) = w.upgrade() else {
                return false;
            };
            if c.seq.0 <= cache.built.seq && c.generation.0 == cache.built.generation {
                return true;
            }
            let mut changes = c.changes.clone();
            if cache.scenario {
                changes.config_keys.retain(|k| k.key == "curDeck");
            }
            let images = c
                .cards
                .iter()
                .map(|r| CardImages {
                    id: r.id,
                    before: r.before.as_deref(),
                    after: r.after.as_deref(),
                })
                .collect::<Vec<_>>();
            let commit = Stamp {
                seq: c.seq.0,
                generation: c.generation.0,
            };
            let action = rebuild_rules::classify(
                &changes,
                &images,
                Context {
                    built: cache.built,
                    commit,
                    owned: (owner == Some(cache.build) || restoring).then_some(commit),
                    day_changed: false,
                },
            );
            if action == Action::Rebuild {
                cache.invalidate();
            }
            true
        });
    }
}

#[derive(Clone, Debug)]
pub struct AnswerMetadata {
    pub card: CardId,
    pub from_queue: Option<Kind>,
    pub learning_cutoff: Option<i64>,
    pub cache_build: Option<CacheBuildId>,
    cache: Option<Weak<AnswerCache>>,
    pub(crate) queue: Option<crate::session::held::QueueRestore>,
    pub(crate) production: Option<crate::session::ProductionRestore>,
}
/// Also exposed to U43's restorer through RestoredStep::state. The row journal
/// remains U25's authority; queue owners restore presentation/cutoff themselves.
#[derive(Clone, Debug)]
pub struct AnswerResult {
    pub leech: bool,
    pub suspended: bool,
    pub buried: Vec<CardId>,
    pub revlog_id: RevlogId,
    pub metadata: AnswerMetadata,
}
pub struct AnswerRestorer;
impl UndoRestorer for AnswerRestorer {
    fn name(&self) -> &str {
        "answer"
    }
    fn restored(&self, step: &RestoredStep<'_>) {
        let Some(meta) = step.state.and_then(|s| s.downcast_ref::<AnswerMetadata>()) else {
            return;
        };
        if let Some(queue) = &meta.production {
            queue.restore(step.direction);
            // Optimistic undo and U25 acknowledgement share one queue/W9
            // removal. Actual redo adds no chooser contribution.
            return;
        }
        if let Some(queue) = &meta.queue {
            queue.restore(step.direction);
        }
        if step.direction == Direction::Undo
            && meta.from_queue.is_some()
            && let Some(cache) = meta.cache.as_ref().and_then(Weak::upgrade)
            && meta.cache_build == Some(cache.build)
            && cache.valid.load(Ordering::Acquire)
            && cache
                .collection
                .upgrade()
                .is_some_and(|c| c.exec.generation() == cache.generation)
        {
            cache.value.lock().unwrap().remove_answer(meta.card);
        }
    }
}

/// The caller consumes a U45 answer and its card-row token. U43 can construct
/// this directly after its own prediction/queue validation; Grade Now leaves
/// the session metadata empty. No mutable presentation queue lives here.
#[derive(Clone, Debug)]
pub struct AnswerRequest {
    pub card: CardId,
    pub state_token: [u8; 32],
    pub current: State,
    pub chosen: AnswerState,
    pub rating: Rating,
    pub today: u32,
    pub next_day_at: i64,
    pub answered_at: EpochMillis,
    pub elapsed_ms: u64,
    pub from_queue: Option<Kind>,
    pub learning_cutoff: Option<i64>,
    pub session: Option<SessionId>,
    pub cache: Option<Arc<AnswerCache>>,
}
#[derive(Clone, Debug)]
pub struct Prediction {
    pub states: SchedulingStates,
    pub card: CardId,
    pub state_token: [u8; 32],
    pub today: u32,
    pub next_day_at: i64,
    pub answered_at: EpochMillis,
    pub cache: Option<Arc<AnswerCache>>,
}
impl Prediction {
    pub fn answer(self, rating: Rating, elapsed_ms: u64) -> AnswerRequest {
        let chosen = meguri_sched::ops::grade_now::select(&self.states, rating);
        AnswerRequest {
            card: self.card,
            state_token: self.state_token,
            current: self.states.current,
            chosen,
            rating,
            today: self.today,
            next_day_at: self.next_day_at,
            answered_at: self.answered_at,
            elapsed_ms,
            from_queue: None,
            learning_cutoff: None,
            session: None,
            cache: self.cache,
        }
    }
}
fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument {
        message: message.into(),
    }
}
fn missing(table: Table, id: i64) -> EngineError {
    EngineError::NotFound {
        table,
        key: id.to_string(),
    }
}
fn state_error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Unsupported { feature: e.to_string() }
}

pub fn predict(
    collection: &Collection,
    id: CardId,
    models: &mut FsrsCache,
    cache: Option<Arc<AnswerCache>>,
) -> Result<Prediction, EngineError> {
    collection
        .core
        .exec
        .answer_caches
        .1
        .read_timed(collection, ReaderRole::Interactive, |rows, env, at| {
            let cache = cache.filter(|c| c.is_current(collection));
            let mut prediction = if let Some(c) = &cache {
                let card = rows.card(id)?.ok_or_else(|| missing(Table::Cards, id.0))?;
                let (preset, _) = home(&**rows, &card)?;
                c.with_chooser(collection, &card, &preset, |chooser| {
                    predict_in(
                        &**rows,
                        id,
                        models,
                        env.today,
                        env.next_day_at,
                        EpochMillis(at),
                        chooser,
                    )
                })?
            } else {
                predict_in(&**rows, id, models, env.today, env.next_day_at, EpochMillis(at), None)?
            };
            prediction.cache = cache;
            Ok(prediction)
        })
}

/// U45 prediction from the live transaction. Later Grade Now siblings observe
/// earlier U44 writes, using exactly the ordinary prediction inputs.
pub(crate) fn predict_in(
    rows: &impl ReadRows,
    id: CardId,
    models: &mut FsrsCache,
    today: u32,
    next_day_at: i64,
    answered_at: EpochMillis,
    chooser: Option<&mut dyn meguri_sched::states::interval_limits::FuzzedDayChooser>,
) -> Result<Prediction, EngineError> {
    let card = rows.card(id)?.ok_or_else(|| missing(Table::Cards, id.0))?;
    let (preset, home) = home(rows, &card)?;
    let normal = match &home.kind.kind {
        DeckKind::Normal(n) => n,
        _ => return Err(state_error("InFilteredDeck")),
    };
    let retention = DeckSched::new(Some(normal), &home.common).retention(preset.fields().desired_retention);
    let mut history = Vec::new();
    if card.card_type != CardType::New && card.data.memory_state.is_none() {
        rows.visit_fsrs_histories(
            &meguri_storage::queries::revlog_fsrs::HistorySelection::Cards(vec![id]),
            |_, found| -> Result<(), StorageError> {
                history.extend_from_slice(found);
                Ok(())
            },
        )?;
    }
    let memory = meguri_sched::memory::resolve::on_the_fly(models, &preset, &card, &history, next_day_at)
        .map_err(state_error)?;
    let last_review_time = match card.data.last_review_secs {
        Some(t) => Some(t),
        None => rows.fsrs_last_review_time(id)?,
    };
    let cfg = SchedConfig::new(rows);
    let input = StateInput {
        card: &card,
        preset: &preset,
        memory_state: memory,
        last_review_time,
        desired_retention: retention,
        today,
        next_day_at,
        now: answered_at.whole_secs().0,
        short_term_with_steps: cfg.fsrs_short_term_with_steps()?,
        learn_ahead: cfg.collapse_time()?,
    };
    let states = next_states(models, &input, chooser).map_err(state_error)?;
    let state_token = rows
        .row_digest(Table::Cards, &RowKey::Id(id.0))?
        .ok_or_else(|| missing(Table::Cards, id.0))?;
    Ok(Prediction {
        states,
        card: id,
        state_token,
        today,
        next_day_at,
        answered_at,
        cache: None,
    })
}

fn home(rows: &impl ReadRows, card: &Card) -> Result<(PresetView, meguri_model::deck::Deck), EngineError> {
    if card.original_deck_id.0 != 0 {
        return Err(state_error("InFilteredDeck"));
    }
    let deck = rows
        .deck(card.deck_id)?
        .ok_or_else(|| missing(Table::Decks, card.deck_id.0))?;
    let DeckKind::Normal(normal) = &deck.kind.kind else {
        return Err(state_error("InFilteredDeck"));
    };
    Ok((PresetView::for_config(rows, DeckConfigId(normal.config_id))?, deck))
}

pub fn answer(collection: &Collection, request: AnswerRequest) -> Result<OpResult<AnswerResult>, EngineError> {
    answer_with_queue(collection, request, None)
}

pub(crate) fn answer_held(
    collection: &Collection,
    request: AnswerRequest,
    queue: Arc<Mutex<crate::session::held::HeldState>>,
) -> Result<OpResult<AnswerResult>, EngineError> {
    answer_with_queue(collection, request, Some(queue))
}

fn answer_with_queue(
    collection: &Collection,
    request: AnswerRequest,
    queue: Option<Arc<Mutex<crate::session::held::HeldState>>>,
) -> Result<OpResult<AnswerResult>, EngineError> {
    // D8 must finish with no reader held, before the writer submission.
    // The request's day/boundary belong to its recorded answer instant;
    // pending-session replay can submit that immutable event on a later day.
    collection.check_rollover()?;
    let spec = OpSpec::named("answer_card", OpSource::UserUndoable)
        .stamp(StampMode::CallerSupplied)
        .undo_step(crate::undo::names::ANSWER_CARD, StepKind::Answer)
        .precondition(Precondition::RowDigest {
            table: Table::Cards,
            key: RowKey::Id(request.card.0),
            digest: Some(request.state_token),
        });
    let spec = if let Some(session) = request.session {
        spec.session(session)
    } else {
        spec
    };
    submit_selected(collection, request, spec, queue, None).wait()
}

/// U43 submits frozen accepted events through this same U44 application.
/// Its caller establishes recorded-time D8 before submission; no current-clock
/// prelude is hidden here (opening recovery must never rewind the live clock).
pub(crate) fn submit_session(
    collection: &Collection,
    request: AnswerRequest,
    application: crate::session::Application,
) -> crate::Pending<OpResult<AnswerResult>> {
    let spec = OpSpec::named("answer_card", OpSource::UserUndoable)
        .stamp(StampMode::CallerSupplied)
        .undo_step(crate::undo::names::ANSWER_CARD, StepKind::Answer)
        .session(request.session.expect("production answer session"))
        .precondition(Precondition::RowDigest {
            table: Table::Cards,
            key: RowKey::Id(request.card.0),
            digest: Some(request.state_token),
        });
    submit_selected(collection, request, spec, None, Some(application))
}

pub(crate) fn submit_replay(
    collection: &Collection,
    request: AnswerRequest,
    application: crate::session::Application,
) -> crate::Pending<OpResult<AnswerResult>> {
    let spec = OpSpec::named("answer_card", OpSource::Rollover)
        .stamp(StampMode::CallerSupplied)
        .precondition(Precondition::RowDigest {
            table: Table::Cards,
            key: RowKey::Id(request.card.0),
            digest: Some(request.state_token),
        });
    submit_selected(collection, request, spec, None, Some(application))
}

fn submit_selected(
    collection: &Collection,
    request: AnswerRequest,
    spec: OpSpec,
    queue: Option<Arc<Mutex<crate::session::held::HeldState>>>,
    application: Option<crate::session::Application>,
) -> crate::Pending<OpResult<AnswerResult>> {
    let bound = collection.clone();
    collection.submit(spec, move |ctx| {
        apply_in(ctx, &bound, request, queue, application, true)
    })
}

/// Shared U44 transaction application. Production/replay retain validation,
/// receipts and commit hooks; Grade Now supplies no queue/cache/application.
pub(crate) fn apply_in(
    ctx: &mut crate::OpContext<'_, '_>,
    bound: &Collection,
    request: AnswerRequest,
    queue: Option<Arc<Mutex<crate::session::held::HeldState>>>,
    application: Option<crate::session::Application>,
    record_undo_state: bool,
) -> Result<AnswerResult, EngineError> {
    if let Some(app) = &application {
        app.validate(ctx, &request)?;
    }
    let base = ctx
        .scope()
        .row_digest(Table::Cards, &RowKey::Id(request.card.0))?
        .ok_or_else(|| missing(Table::Cards, request.card.0))?;
    let card = ctx
        .scope()
        .card(request.card)?
        .ok_or_else(|| missing(Table::Cards, request.card.0))?;
    let (preset, deck) = home(ctx.scope(), &card)?;
    FsrsCache::default().model(&preset).map_err(state_error)?;
    let normal = match &deck.kind.kind {
        DeckKind::Normal(n) => n,
        _ => return Err(state_error("InFilteredDeck")),
    };
    let retention = DeckSched::new(Some(normal), &deck.common).retention(preset.fields().desired_retention);
    let until = request
        .next_day_at
        .saturating_sub(request.answered_at.whole_secs().0)
        .max(0) as u64;
    if request.chosen.delay != request.chosen.state.delay(until)
        || request.chosen.revlog.interval != request.chosen.delay.revlog_interval()
        || request.chosen.revlog.last_interval != request.current.delay(until).revlog_interval()
        || request.chosen.revlog.kind
            != match request.current {
                State::Review(_) => 1,
                State::Relearning { .. } => 2,
                _ => 0,
            }
    {
        return Err(invalid("inconsistent selected answer values"));
    }
    let plan = values::plan(
        &card,
        request.current,
        &request.chosen,
        &preset,
        retention,
        request.today,
        request.answered_at.0,
        request.elapsed_ms,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let cache = request.cache.filter(|c| c.is_current(&bound));
    let mut meta = AnswerMetadata {
        card: card.id,
        from_queue: request.from_queue,
        learning_cutoff: request.learning_cutoff,
        cache_build: cache.as_ref().map(|c| c.build),
        cache: cache.as_ref().map(Arc::downgrade),
        queue: None,
        production: None,
    };
    let scope = ctx.scope();
    scope.put_answer_card(&plan.card)?;
    let log = values::revlog::row(card.id, request.rating, &request.chosen, plan.elapsed_ms);
    let revlog_id = if let Some(prepared) = application.as_ref().and_then(|a| a.assigned.as_ref()) {
        let mut assigned = log.clone();
        assigned.id = RevlogId(prepared.row[0]);
        use meguri_storage::Record;
        let cells = assigned.to_cells();
        if cells
            .iter()
            .zip(&prepared.row)
            .any(|(c, v)| *c != meguri_storage::Cell::Integer(*v))
        {
            return Err(invalid("prepared selected answer identity changed"));
        }
        scope.insert_prepared_answer(&assigned)?
    } else {
        scope.add_answer_revlog(&log, request.answered_at)?
    };
    let decks = scope.deck_rows()?;
    let mut current = Some(deck);
    while let Some(deck) = current {
        let changed = values::deck_stats::answered(
            &deck,
            request.today,
            card.queue,
            plan.elapsed_ms,
            request.answered_at.whole_secs().0,
        );
        scope.put(&changed)?;
        current = deck.name.rsplit_once('\u{1f}').and_then(|(parent, _)| {
            decks
                .iter()
                .find(|d| meguri_text::fold::compare_folded(&d.name, parent).is_eq())
                .cloned()
        });
    }
    let buried = scope
        .answer_siblings(card.note_id, card.id)?
        .into_iter()
        .filter(|s| values::siblings::allowed(card.queue, s.queue, preset.fields()))
        .map(|s| s.id)
        .collect::<Vec<_>>();
    let queue_change = queue
        .as_ref()
        .map(|owner| {
            crate::session::held::QueueRestore::prepare(
                owner,
                &plan.card,
                &buried,
                request.answered_at.whole_secs().0,
                request.next_day_at,
            )
        })
        .transpose()?;
    if let Some((record, _)) = &queue_change {
        meta.queue = Some(record.clone());
    }
    if !buried.is_empty() {
        let value = meguri_storage::ConfigValue::from(u64::from(request.today));
        if scope.config_value("lastUnburied")? != Some(value.clone()) {
            scope.put_config_entry(&ConfigEntry {
                key: "lastUnburied".into(),
                usn: Usn::PENDING,
                mtime: EpochSecs(request.answered_at.whole_secs().0),
                value: value.compact().into_bytes(),
            })?;
        }
        for id in &buried {
            scope.bury_answer_sibling(*id, request.answered_at.whole_secs().0)?;
        }
    }
    if plan.leech {
        // U30 writes locally-stamped tags/notes; this answer scope uses
        // recorded timestamps, so explicitly stamp the note it changed.
        if crate::tags::add_in(scope, &[card.note_id], "leech")? > 0 {
            let mut note = scope
                .note(card.note_id)?
                .ok_or_else(|| missing(Table::Notes, card.note_id.0))?;
            note.mtime = EpochSecs(request.answered_at.whole_secs().0);
            note.usn = Usn::PENDING;
            scope.put(&note)?;
        }
    }
    scope.set_col_mod(request.answered_at)?;
    if let Some(app) = &application {
        meta.production = app.restorer.clone();
        app.prepare(ctx, revlog_id, base)?;
    }
    if application.as_ref().is_some_and(|a| a.clear_history) {
        let core = Arc::downgrade(&bound.core);
        ctx.on_commit(move |_| {
            if let Some(core) = core.upgrade() {
                core.exec.undo.reset(crate::DisabledReason::ClearedByRollover);
            }
        });
    }
    if record_undo_state {
        ctx.set_undo_state(Arc::new(meta.clone()));
    }
    if let Some((record, next)) = queue_change {
        ctx.on_commit(move |_| record.commit(next));
    }
    if let Some(cache) = cache.filter(|_| !application.as_ref().is_some_and(|a| a.optimistic_cache)) {
        let written = plan.card.clone();
        let bound = bound.clone();
        let preset = preset.source().id;
        ctx.on_commit(move |_| {
            // Own col.mod invalidates the maintained report after the commit.
            // The registered build/clock witness still proves W8 ownership.
            if cache.enabled && cache.bound_and_time_current(&bound) && written.queue == CardQueue::Review {
                cache.value.lock().unwrap().append(
                    preset,
                    written.interval,
                    Entry {
                        card: written.id,
                        note: written.note_id,
                    },
                );
            }
        });
    }
    Ok(AnswerResult {
        leech: plan.leech,
        suspended: plan.suspended,
        buried,
        revlog_id,
        metadata: meta,
    })
}
