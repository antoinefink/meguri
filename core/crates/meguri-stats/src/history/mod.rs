//! The Statistics page's history reads (`stats/history-reads`): Cards Over
//! Time and Started (H2, U289) and Knowledge Over Time (H3, U290), kept in
//! the `stats_history` component (H14).
//!
//! - A page scope's series is the sum of the series of the deck keys it
//!   selects (H1, H14.1), read from the component when it is current for
//!   the request's snapshot and kept on the request's day grid.
//! - Otherwise the read releases the component's build (`request_refresh`,
//!   H14.2) and waits for it with progress (H15), cancellable within 50 ms
//!   while the build goes on (H16); a component that cannot be built is
//!   read around: the parts come from the snapshot (H18).
//! - Read only (H17): nothing here writes the collection; the day is G1's.

mod knowledge;
mod replay;
mod store;
mod sums;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use meguri_engine::{CancelToken, Collection, DerivedHost, Readiness, Snapshot};
use meguri_sched::model::sched_config::SchedConfig;
use meguri_storage::{ReadRows, ReaderRole, StorageError};

pub use knowledge::Memory as CardMemory;
pub use store::{HISTORY_COMPONENT, HISTORY_WANTED, StatsHistory};

use crate::graphs::context::Context;
use crate::graphs::scope::{self, History, Scope, Unreadable};
use crate::graphs::{StatsCaches, StatsError};
use knowledge::Frame;
use replay::Grid;
use sums::{FirstBad, KnowledgeTotals, Selection, Totals};

pub(crate) use store::Shared;

/// The five counts of H2, or their changes on a day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StateCounts {
    pub new: i64,
    pub learning: i64,
    pub relearning: i64,
    pub young: i64,
    pub mature: i64,
}

impl From<[i64; 5]> for StateCounts {
    fn from([new, learning, relearning, young, mature]: [i64; 5]) -> StateCounts {
        StateCounts {
            new,
            learning,
            relearning,
            young,
            mature,
        }
    }
}

impl StateCounts {
    pub fn plus(self, other: StateCounts) -> StateCounts {
        StateCounts {
            new: self.new + other.new,
            learning: self.learning + other.learning,
            relearning: self.relearning + other.relearning,
            young: self.young + other.young,
            mature: self.mature + other.mature,
        }
    }

    pub fn total(self) -> i64 {
        self.new + self.learning + self.relearning + self.young + self.mature
    }
}

/// Part `card_states_over_time` (H2.4). Days are H1's: 0 today, `−k` `k`
/// days ago.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardStatesOverTime {
    /// The counts at the end of the day before the window's first day;
    /// zeros with `allHistory`.
    pub start: StateCounts,
    /// The change of the counts on each day of the window that has one.
    pub deltas: BTreeMap<i32, StateCounts>,
    /// The scope's suspended cards, left out of every day (H2.1).
    pub left_out: u64,
    /// The oldest day any card of the series enters.
    pub first_day: Option<i32>,
}

impl CardStatesOverTime {
    /// The counts at the end of `day` (today's for day 0): `start` plus the
    /// deltas up to it.
    pub fn on(&self, day: i32) -> StateCounts {
        self.deltas
            .range(..=day)
            .fold(self.start, |counts, (_, delta)| counts.plus(*delta))
    }
}

/// Part `started` (H2.4): cards first reviewed per day of the window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartedPart {
    pub days: BTreeMap<i32, u64>,
    /// The oldest such day in all history.
    pub first_day: Option<i32>,
}

/// One sample of Knowledge Over Time (H3.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnowledgeSample {
    /// H1's day: −365 to −1 for a day end, earlier for a month end, 0 for
    /// today's point.
    pub day: i32,
    /// The sample instant, epoch milliseconds: the day end's whole second
    /// (H1), or the request's now.
    pub instant_ms: i64,
    /// The estimated knowledge: the summed recall probabilities of the cards,
    /// each kept in [0, 1] (H3.1).
    pub knowledge: f64,
    /// The cards that have an estimate there.
    pub studied: u64,
}

/// Part `knowledge_over_time` (H3.3): an estimate, replayed with today's
/// FSRS parameters from only the rows each sample had seen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KnowledgeOverTime {
    /// Ascending by instant, today's point last; empty when no card of the
    /// scope has an estimate at any sample (H3.1's no estimate).
    pub samples: Vec<KnowledgeSample>,
    /// The cards left out for a crate error, from the first sample it
    /// failed at on (H3.1).
    pub failed: u64,
    /// The scope's suspended cards, left out of every sample (H2.1).
    pub left_out: u64,
}

/// How far a part's wait has got (H15): rows read over rows to read, or
/// unknown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    Fraction(f64),
    Indeterminate,
}

/// A read of H2's parts.
#[derive(Clone, Debug)]
pub struct HistoryRequest {
    pub scope: Scope,
    pub history: History,
    pub cancel: Arc<CancelToken>,
}

/// H2's two parts for one scope and window.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryData {
    pub context: Context,
    pub card_states: CardStatesOverTime,
    pub started: StartedPart,
    /// The component served them (else they were computed from the
    /// snapshot, H18).
    pub from_component: bool,
}

/// A read of H3's part.
#[derive(Clone, Debug)]
pub struct KnowledgeRequest {
    pub scope: Scope,
    pub history: History,
    /// H9's `oldest_day` of the scope, which the month samples of
    /// `allHistory` reach back to (H3.2). The samples never stop short of
    /// the oldest one with an estimate, so `None` only drops leading empty
    /// months.
    pub oldest_day: Option<i32>,
    pub cancel: Arc<CancelToken>,
}

/// H3's part for one scope and window.
#[derive(Clone, Debug, PartialEq)]
pub struct KnowledgeData {
    pub context: Context,
    pub knowledge: KnowledgeOverTime,
    /// The component served it (else it was computed from the snapshot,
    /// H18).
    pub from_component: bool,
}

/// What a read of H3's part gives (H18), as [`HistoryResult`] for H2.
#[derive(Clone, Debug, PartialEq)]
pub enum KnowledgeResult {
    Data(Box<KnowledgeData>),
    UnreadableRow(Unreadable),
    UnknownDeck,
    UnsupportedScope,
    ContextChanged,
    Cancelled,
}

/// What a read of H2's parts gives (H18).
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryResult {
    Data(Box<HistoryData>),
    /// An unreadable card of the scope, or row of one, in any window
    /// (H15, G4).
    UnreadableRow(Unreadable),
    /// G8.2's `unknown_deck`.
    UnknownDeck,
    /// A Search scope (H1).
    UnsupportedScope,
    /// The collection changed after the request's snapshot while the part
    /// waited for the component: the page is stale anyway (G10).
    ContextChanged,
    Cancelled,
}

/// How often a waiting read looks at the component's readiness: well
/// within H16's 50 ms.
const POLL: Duration = Duration::from_millis(10);
/// H15: progress at most every 100 ms.
const REPORT_EVERY: Duration = Duration::from_millis(100);
/// A component found ready but not current, or not rebuilt for this
/// request's grid, this many times in a row (about a second) is read
/// around.
const MAX_TRIES: u32 = 100;

/// H15's progress for one part: between 0 and 1, never decreasing, at most
/// every 100 ms.
struct Reporter<'a> {
    report: &'a mut dyn FnMut(Progress),
    last: Option<Duration>,
    best: Option<f64>,
    unknown_sent: bool,
}

impl Reporter<'_> {
    fn due(&mut self) -> bool {
        let now = meguri_engine::monotonic::now();
        if self.last.is_some_and(|last| now.saturating_sub(last) < REPORT_EVERY) {
            return false;
        }
        self.last = Some(now);
        true
    }

    fn fraction(&mut self, completed: u64, total: u64) {
        let fraction = if total == 0 { 0.0 } else { (completed as f64 / total as f64).clamp(0.0, 1.0) };
        if self.best.is_some_and(|best| fraction <= best) || !self.due() {
            return;
        }
        self.best = Some(fraction);
        (self.report)(Progress::Fraction(fraction));
    }

    fn unknown(&mut self) {
        if self.best.is_none() && !self.unknown_sent && self.due() {
            self.unknown_sent = true;
            (self.report)(Progress::Indeterminate);
        }
    }
}

enum Waited {
    Ready,
    Failed,
    Cancelled,
}

/// Polls a `Ready` component this many times for the rebuild it was asked
/// for to begin before taking it as it is (the read then asks again).
const REBUILD_POLLS: u32 = 100;

/// Waits until the component is `Ready`, or will not be (H18), or the read
/// is cancelled; reports the build's progress meanwhile. It never blocks on
/// the derived writer, so a cancel is seen within one poll (H16). With
/// `rebuilding`, a `Ready` is taken only once the asked-for rebuild has
/// shown, or after [`REBUILD_POLLS`].
fn wait(derived: &DerivedHost, cancel: &CancelToken, reporter: &mut Reporter<'_>, rebuilding: bool) -> Waited {
    let mut changed = !rebuilding;
    let mut polls = 0;
    loop {
        if cancel.is_cancelled() {
            return Waited::Cancelled;
        }
        // Before the readiness: a store that closed during a build leaves it
        // `Building` (D11).
        if derived.has_failed(HISTORY_COMPONENT) {
            return Waited::Failed;
        }
        let Some(readiness) = derived.readiness(HISTORY_COMPONENT) else {
            return Waited::Failed;
        };
        match readiness.readiness {
            Readiness::Ready if changed || polls >= REBUILD_POLLS => return Waited::Ready,
            Readiness::Ready => polls += 1,
            Readiness::Building { completed, total } => {
                changed = true;
                match total {
                    Some(total) => reporter.fraction(completed, total),
                    None => reporter.unknown(),
                }
            }
            Readiness::Stale => {
                changed = true;
                reporter.unknown();
            }
        }
        std::thread::sleep(POLL);
    }
}

/// The keys a page scope selects, or `None` for a deck id that names no
/// deck.
fn selection(snapshot: &Snapshot<'_>, scope: &Scope) -> Result<Option<Selection>, StorageError> {
    Ok(match scope {
        Scope::Deck(deck) => scope::deck_set(&snapshot.deck_rows()?, *deck).map(Selection::Decks),
        _ => Some(Selection::All),
    })
}

/// G1's context, as a refresh's first request takes it: the day clock,
/// then the clock, then the snapshot (after the derived writer has applied
/// every commit sent to it, so the snapshot's first read of the store sees
/// them, D8).
pub fn page_context(collection: &Collection, derived: &DerivedHost) -> Result<Context, StatsError> {
    let day = collection.check_rollover()?;
    let clock = collection.clock();
    let now_ms = clock.now_ms();
    let utc_offset_secs = clock.utc_offset_at(now_ms).seconds();
    let zone = clock.zone_id();
    derived.flush()?;
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    Ok(Context {
        next_day_at: day.next_day_at,
        today: day.today,
        rollover_hour: SchedConfig::new(&*snapshot).rollover_hour()?,
        now_ms,
        utc_offset_secs,
        zone,
        generation: snapshot.generation().0,
        seq: snapshot.seq().0,
    })
}

/// H2's parts for `request`, with a context of their own (G1). Call it off
/// the main thread (R1291); `progress` hears of a wait for the component.
pub fn card_states(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    request: &HistoryRequest,
    progress: &mut dyn FnMut(Progress),
) -> Result<HistoryResult, StatsError> {
    if matches!(request.scope.effective(), Scope::Search(_)) {
        return Ok(HistoryResult::UnsupportedScope);
    }
    if request.cancel.is_cancelled() {
        return Ok(HistoryResult::Cancelled);
    }
    let context = page_context(collection, derived)?;
    if request.cancel.is_cancelled() {
        return Ok(HistoryResult::Cancelled);
    }
    card_states_in(caches, collection, derived, &context, request, progress)
}

/// H2's parts for `request` within a page request whose context is
/// `context` (U293's request): computed on a snapshot of that context's
/// generation and seq, or `ContextChanged`.
pub fn card_states_in(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    context: &Context,
    request: &HistoryRequest,
    progress: &mut dyn FnMut(Progress),
) -> Result<HistoryResult, StatsError> {
    let scope = request.scope.effective();
    if matches!(scope, Scope::Search(_)) {
        return Ok(HistoryResult::UnsupportedScope);
    }
    let grid = Grid::of(context.next_day_at);
    let history = request.history;
    let served = serve(
        caches,
        collection,
        derived,
        context,
        scope,
        &request.cancel,
        progress,
        &|snapshot| Ok(store::kept_grid(snapshot)? == Some(grid)),
        &|snapshot, selection, cancelled| read_kept(snapshot, context, grid, history, selection, cancelled),
        &|snapshot, selection, cancelled| read_cold(snapshot, context, grid, history, selection, cancelled),
    )?;
    Ok(match served {
        Served::Data((card_states, started), from_component) => HistoryResult::Data(Box::new(HistoryData {
            context: context.clone(),
            card_states,
            started,
            from_component,
        })),
        Served::Unreadable(bad) => HistoryResult::UnreadableRow(bad),
        Served::UnknownDeck => HistoryResult::UnknownDeck,
        Served::ContextChanged => HistoryResult::ContextChanged,
        Served::Cancelled => HistoryResult::Cancelled,
    })
}

/// A read of one part from a snapshot, for the keys a scope selects.
type PartRead<'a, T> = dyn Fn(&Snapshot<'_>, Selection, &dyn Fn() -> bool) -> Result<Read<T>, StorageError> + 'a;

/// What [`serve`] gives a part.
enum Served<T> {
    Data(T, bool),
    Unreadable(Unreadable),
    UnknownDeck,
    ContextChanged,
    Cancelled,
}

/// The read loop both parts share: the kept content when the component is
/// current for the request's snapshot and `fits` it, else the component's
/// build released (H14.2) and waited for with progress (H15), cancellable
/// within 50 ms while the build goes on (H16); a component that cannot be
/// built is read around, from the snapshot (H18).
#[allow(clippy::too_many_arguments)]
fn serve<T>(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    context: &Context,
    scope: &Scope,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
    fits: &dyn Fn(&Snapshot<'_>) -> Result<bool, StorageError>,
    kept: &PartRead<'_, T>,
    cold: &PartRead<'_, T>,
) -> Result<Served<T>, StatsError> {
    let shared = caches.history_shared();
    shared.want(Grid::of(context.next_day_at), Frame::of(context));
    let mut reporter = Reporter {
        report: progress,
        last: None,
        best: None,
        unknown_sent: false,
    };
    let cancelled = || cancel.is_cancelled();
    let mut tries = 0;
    loop {
        if cancelled() {
            return Ok(Served::Cancelled);
        }
        let other_grid = {
            let lease = collection.reader(ReaderRole::Query)?;
            let snapshot = lease.snapshot()?;
            if snapshot.generation().0 != context.generation || snapshot.seq().0 != context.seq {
                return Ok(Served::ContextChanged);
            }
            let Some(selection) = selection(&snapshot, scope)? else {
                return Ok(Served::UnknownDeck);
            };
            // Before the snapshot's first read of `derived` (D8).
            let current = derived.is_current(HISTORY_COMPONENT, &snapshot);
            if current && fits(&snapshot)? {
                let read = kept(&snapshot, selection, &cancelled);
                return finish(read, true, &cancelled);
            }
            if derived.has_failed(HISTORY_COMPONENT) || derived.readiness(HISTORY_COMPONENT).is_none() || tries >= MAX_TRIES
            {
                tracing::warn!("the stats history cannot be built: the history part is computed from the snapshot");
                let read = cold(&snapshot, selection, &cancelled);
                return finish(read, false, &cancelled);
            }
            current
        };
        if other_grid {
            // Kept for another phase of `N` or another frame (H14.3):
            // rebuilt on this request's, the reader let go first.
            shared.rebuild();
        }
        derived.request_refresh(HISTORY_COMPONENT);
        let waited = wait(derived, cancel, &mut reporter, other_grid);
        match waited {
            Waited::Cancelled => return Ok(Served::Cancelled),
            Waited::Ready | Waited::Failed => {}
        }
        if tries > 0 {
            // Ready but not yet current: the writer applies the last
            // commits (D8). No blocking wait on it, so a cancel stays quick.
            std::thread::sleep(POLL);
        }
        tries += 1;
    }
}

/// Releases the component's build for a page request whose context is
/// `context` (U293), on that request's grid, before the request reads its
/// other parts: what [`card_states_in`] then waits for. Returns at once.
pub(crate) fn release_build(caches: &StatsCaches, derived: &DerivedHost, context: &Context) {
    caches.history_shared().want(Grid::of(context.next_day_at), Frame::of(context));
    derived.request_refresh(HISTORY_COMPONENT);
}

/// The parts, or why they cannot be given.
enum Read<T> {
    Parts(T),
    Unreadable(Unreadable),
}

fn finish<T>(read: Result<Read<T>, StorageError>, from_component: bool, cancelled: &dyn Fn() -> bool) -> Result<Served<T>, StatsError> {
    // A cancelled request returns no part (H16).
    if cancelled() {
        return Ok(Served::Cancelled);
    }
    match read {
        Ok(Read::Parts(parts)) => Ok(Served::Data(parts, from_component)),
        Ok(Read::Unreadable(bad)) => Ok(Served::Unreadable(bad)),
        Err(_) if cancelled() => Ok(Served::Cancelled),
        Err(error) => Err(error.into()),
    }
}

type StateParts = (CardStatesOverTime, StartedPart);

/// The kept series of the keys `selection` selects.
fn read_kept(
    snapshot: &Snapshot<'_>,
    context: &Context,
    grid: Grid,
    history: History,
    selection: Selection,
    cancelled: &dyn Fn() -> bool,
) -> Result<Read<StateParts>, StorageError> {
    let mut first = FirstBad::default();
    store::visit_kept_bad(snapshot, cancelled, &mut |bad| first.offer(&selection, bad))?;
    if let Some(bad) = first.first() {
        return Ok(Read::Unreadable(bad));
    }
    let mut totals = Totals::new(grid, context.next_day_at, history, selection);
    store::visit_kept_keys(snapshot, cancelled, &mut |key, count| totals.add_left_out(key, count))?;
    store::visit_kept_days(snapshot, cancelled, &mut |key, day, values| totals.add_day(key, day, values))?;
    Ok(Read::Parts(totals.finish()))
}

/// H18: the same series computed from the snapshot, without keeping it.
fn read_cold(
    snapshot: &Snapshot<'_>,
    context: &Context,
    grid: Grid,
    history: History,
    selection: Selection,
    cancelled: &dyn Fn() -> bool,
) -> Result<Read<StateParts>, StorageError> {
    let replayed = store::replay_all(&**snapshot, grid, None, cancelled)?;
    let mut first = FirstBad::default();
    for bad in replayed.bad {
        first.offer(&selection, bad);
    }
    if let Some(bad) = first.first() {
        return Ok(Read::Unreadable(bad));
    }
    let mut totals = Totals::new(grid, context.next_day_at, history, selection);
    totals.add_sums(&replayed.sums);
    Ok(Read::Parts(totals.finish()))
}

/// H2's parts computed from a snapshot of `collection` with `context`'s
/// `N`, never from the component: what H14.4's equivalence compares with.
pub fn card_states_cold(
    collection: &Collection,
    context: &Context,
    request: &HistoryRequest,
) -> Result<HistoryResult, StatsError> {
    let scope = request.scope.effective();
    if matches!(scope, Scope::Search(_)) {
        return Ok(HistoryResult::UnsupportedScope);
    }
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    if snapshot.generation().0 != context.generation || snapshot.seq().0 != context.seq {
        return Ok(HistoryResult::ContextChanged);
    }
    let Some(selection) = selection(&snapshot, scope)? else {
        return Ok(HistoryResult::UnknownDeck);
    };
    let cancelled = || request.cancel.is_cancelled();
    let grid = Grid::of(context.next_day_at);
    let read = read_cold(&snapshot, context, grid, request.history, selection, &cancelled);
    Ok(match finish(read, false, &cancelled)? {
        Served::Data((card_states, started), from_component) => HistoryResult::Data(Box::new(HistoryData {
            context: context.clone(),
            card_states,
            started,
            from_component,
        })),
        Served::Unreadable(bad) => HistoryResult::UnreadableRow(bad),
        Served::UnknownDeck => HistoryResult::UnknownDeck,
        Served::ContextChanged => HistoryResult::ContextChanged,
        Served::Cancelled => HistoryResult::Cancelled,
    })
}

/// H3's part for `request`, with a context of its own (G1). Call it off the
/// main thread (R1291); `progress` hears of a wait for the component.
pub fn knowledge_over_time(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    request: &KnowledgeRequest,
    progress: &mut dyn FnMut(Progress),
) -> Result<KnowledgeResult, StatsError> {
    if matches!(request.scope.effective(), Scope::Search(_)) {
        return Ok(KnowledgeResult::UnsupportedScope);
    }
    if request.cancel.is_cancelled() {
        return Ok(KnowledgeResult::Cancelled);
    }
    let context = page_context(collection, derived)?;
    if request.cancel.is_cancelled() {
        return Ok(KnowledgeResult::Cancelled);
    }
    knowledge_in(caches, collection, derived, &context, request, progress)
}

/// H3's part for `request` within a page request whose context is
/// `context` (U293's request, as [`card_states_in`]): computed on a
/// snapshot of that context's generation and seq, or `ContextChanged`.
pub fn knowledge_in(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    context: &Context,
    request: &KnowledgeRequest,
    progress: &mut dyn FnMut(Progress),
) -> Result<KnowledgeResult, StatsError> {
    let scope = request.scope.effective();
    if matches!(scope, Scope::Search(_)) {
        return Ok(KnowledgeResult::UnsupportedScope);
    }
    let grid = Grid::of(context.next_day_at);
    let frame = Frame::of(context);
    let served = serve(
        caches,
        collection,
        derived,
        context,
        scope,
        &request.cancel,
        progress,
        &|snapshot| Ok(store::kept_meta(snapshot)?.is_some_and(|(kept, on)| kept == grid && on.serves(frame))),
        &|snapshot, selection, cancelled| read_kept_knowledge(snapshot, context, request, selection, cancelled),
        &|snapshot, selection, cancelled| read_cold_knowledge(snapshot, context, request, selection, cancelled),
    )?;
    Ok(knowledge_result(served, context))
}

fn knowledge_result(served: Served<KnowledgeOverTime>, context: &Context) -> KnowledgeResult {
    match served {
        Served::Data(knowledge, from_component) => KnowledgeResult::Data(Box::new(KnowledgeData {
            context: context.clone(),
            knowledge,
            from_component,
        })),
        Served::Unreadable(bad) => KnowledgeResult::UnreadableRow(bad),
        Served::UnknownDeck => KnowledgeResult::UnknownDeck,
        Served::ContextChanged => KnowledgeResult::ContextChanged,
        Served::Cancelled => KnowledgeResult::Cancelled,
    }
}

/// The request's oldest day on the grid, and whether it samples months.
fn months(context: &Context, request: &KnowledgeRequest) -> (bool, Option<i64>) {
    let today = context.next_day_at.div_euclid(replay::DAY);
    (
        request.history == History::AllHistory,
        request.oldest_day.map(|day| today + i64::from(day)),
    )
}

/// H3's part from the kept samples of the keys `selection` selects and
/// today's point from their records.
fn read_kept_knowledge(
    snapshot: &Snapshot<'_>,
    context: &Context,
    request: &KnowledgeRequest,
    selection: Selection,
    cancelled: &dyn Fn() -> bool,
) -> Result<Read<KnowledgeOverTime>, StorageError> {
    let mut first = FirstBad::default();
    store::visit_kept_bad(snapshot, cancelled, &mut |bad| first.offer(&selection, bad))?;
    if let Some(bad) = first.first() {
        return Ok(Read::Unreadable(bad));
    }
    let mut totals = KnowledgeTotals::new(selection);
    store::visit_kept_keys(snapshot, cancelled, &mut |key, count| totals.add_left_out(key, count))?;
    store::visit_kept_samples(snapshot, cancelled, &mut |key, day, values| totals.add_day(key, day, values))?;
    let presets = store::kept_presets(snapshot)?;
    store::visit_kept_now(snapshot, &presets, cancelled, &mut |card| totals.add_now(card, context.now_ms))?;
    let (months, oldest) = months(context, request);
    Ok(Read::Parts(totals.finish(
        Grid::of(context.next_day_at),
        Frame::of(context),
        context.now_ms,
        months,
        oldest,
    )))
}

/// H18: the same part replayed from the snapshot, without keeping it.
fn read_cold_knowledge(
    snapshot: &Snapshot<'_>,
    context: &Context,
    request: &KnowledgeRequest,
    selection: Selection,
    cancelled: &dyn Fn() -> bool,
) -> Result<Read<KnowledgeOverTime>, StorageError> {
    let grid = Grid::of(context.next_day_at);
    let frame = Frame::of(context);
    let replayed = store::replay_all(&**snapshot, grid, Some(frame), cancelled)?;
    let mut first = FirstBad::default();
    for bad in replayed.bad {
        first.offer(&selection, bad);
    }
    if let Some(bad) = first.first() {
        return Ok(Read::Unreadable(bad));
    }
    let mut totals = KnowledgeTotals::new(selection);
    totals.add_sums(&replayed.sums);
    for card in &replayed.now {
        totals.add_now(card, context.now_ms);
    }
    let (months, oldest) = months(context, request);
    Ok(Read::Parts(totals.finish(grid, frame, context.now_ms, months, oldest)))
}

/// H3's part computed from a snapshot of `collection` with `context`,
/// never from the component: what H14.4's equivalence compares with.
pub fn knowledge_cold(
    collection: &Collection,
    context: &Context,
    request: &KnowledgeRequest,
) -> Result<KnowledgeResult, StatsError> {
    let scope = request.scope.effective();
    if matches!(scope, Scope::Search(_)) {
        return Ok(KnowledgeResult::UnsupportedScope);
    }
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    if snapshot.generation().0 != context.generation || snapshot.seq().0 != context.seq {
        return Ok(KnowledgeResult::ContextChanged);
    }
    let Some(selection) = selection(&snapshot, scope)? else {
        return Ok(KnowledgeResult::UnknownDeck);
    };
    let cancelled = || request.cancel.is_cancelled();
    let read = read_cold_knowledge(&snapshot, context, request, selection, &cancelled);
    Ok(knowledge_result(finish(read, false, &cancelled)?, context))
}

/// One kept card record (H14.1), for tests and later units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeptCard {
    pub id: i64,
    pub did: i64,
    pub odid: i64,
    pub suspended: bool,
    /// The H2.2 table's state after the card's newest row: 0 New to 4
    /// Mature.
    pub table_state: Option<u8>,
    pub newest_row: Option<i64>,
    /// The absolute day of its first review on the kept grid.
    pub first_review: Option<i64>,
}

/// The card records the component keeps, as `snapshot` reads the store,
/// when it is current for it; `None` otherwise.
pub fn kept_cards(derived: &DerivedHost, snapshot: &Snapshot<'_>) -> Result<Option<Vec<KeptCard>>, StorageError> {
    if !derived.is_current(HISTORY_COMPONENT, snapshot) {
        return Ok(None);
    }
    let mut cards = Vec::new();
    store::visit_kept_records(snapshot, &mut |record| {
        cards.push(KeptCard {
            id: record.id,
            did: record.key.did,
            odid: record.key.odid,
            suspended: record.suspended,
            table_state: record.table_state.map(|state| state.index() as u8),
            newest_row: record.newest_row,
            first_review: record.first_review,
        });
    })?;
    Ok(Some(cards))
}

/// One kept card's memory (H14.1), for tests: its home preset, and its
/// memory over every row with and without that preset's cutoff.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeptMemory {
    pub id: i64,
    pub preset: Option<i64>,
    pub with_cutoff: CardMemory,
    pub without_cutoff: CardMemory,
}

/// The memory records the component keeps, as `snapshot` reads the store, when
/// it is current for it; `None` otherwise.
pub fn kept_memory_records(derived: &DerivedHost, snapshot: &Snapshot<'_>) -> Result<Option<Vec<KeptMemory>>, StorageError> {
    if !derived.is_current(HISTORY_COMPONENT, snapshot) {
        return Ok(None);
    }
    let mut states_at = Vec::new();
    store::visit_kept_memory_records(snapshot, &mut |id, preset, with_cutoff, without_cutoff| {
        states_at.push(KeptMemory {
            id,
            preset,
            with_cutoff,
            without_cutoff,
        });
    })?;
    Ok(Some(states_at))
}
