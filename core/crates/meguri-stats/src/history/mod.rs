//! The Statistics page's history reads (`stats/history-reads`), U289: Cards
//! Over Time and Started (H2), kept in the `stats_history` component (H14).
//!
//! - A page scope's series is the sum of the series of the deck keys it
//!   selects (H1, H14.1), read from the component when it is current for
//!   the request's snapshot and kept on the request's day grid.
//! - Otherwise the read releases the component's build (`request_refresh`,
//!   H14.2) and waits for it with progress (H15), cancellable within 50 ms
//!   while the build goes on (H16); a component that cannot be built is
//!   read around: the parts come from the snapshot (H18).
//! - Read only (H17): nothing here writes the collection; the day is G1's.

mod replay;
mod store;
mod sums;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use meguri_engine::{CancelToken, Collection, DerivedHost, Readiness, Snapshot};
use meguri_sched::model::sched_config::SchedConfig;
use meguri_storage::{ReadRows, ReaderRole, StorageError};

pub use store::{HISTORY_COMPONENT, HISTORY_WANTED, StatsHistory};

use crate::graphs::context::Context;
use crate::graphs::scope::{self, History, Scope, Unreadable};
use crate::graphs::{StatsCaches, StatsError};
use replay::Grid;
use sums::{FirstBad, Selection, Totals};

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
/// request's grid, this many times in a row is read around (it should
/// never happen: the read flushes first).
const MAX_TRIES: u32 = 20;

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

/// Waits until the component is `Ready`, or will not be (H18), or the read
/// is cancelled; reports the build's progress meanwhile.
fn wait(derived: &DerivedHost, cancel: &CancelToken, reporter: &mut Reporter<'_>) -> Waited {
    loop {
        if cancel.is_cancelled() {
            return Waited::Cancelled;
        }
        let Some(readiness) = derived.readiness(HISTORY_COMPONENT) else {
            return Waited::Failed;
        };
        match readiness.readiness {
            Readiness::Ready => return Waited::Ready,
            Readiness::Building { completed, total } => match total {
                Some(total) => reporter.fraction(completed, total),
                None => reporter.unknown(),
            },
            Readiness::Stale if derived.has_failed(HISTORY_COMPONENT) => return Waited::Failed,
            Readiness::Stale => reporter.unknown(),
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
    let shared = caches.history_shared();
    let grid = Grid::of(context.next_day_at);
    shared.want(grid);
    let mut reporter = Reporter {
        report: progress,
        last: None,
        best: None,
        unknown_sent: false,
    };
    let cancelled = || request.cancel.is_cancelled();
    let mut tries = 0;
    loop {
        if cancelled() {
            return Ok(HistoryResult::Cancelled);
        }
        {
            let lease = collection.reader(ReaderRole::Query)?;
            let snapshot = lease.snapshot()?;
            if snapshot.generation().0 != context.generation || snapshot.seq().0 != context.seq {
                return Ok(HistoryResult::ContextChanged);
            }
            let Some(selection) = selection(&snapshot, scope)? else {
                return Ok(HistoryResult::UnknownDeck);
            };
            // Before the snapshot's first read of `derived` (D8).
            let current = derived.is_current(HISTORY_COMPONENT, &snapshot);
            if current && store::kept_grid(&snapshot)? == Some(grid) {
                let read = read_kept(&snapshot, context, grid, request.history, selection, &cancelled);
                return finish(read, context, true, &cancelled);
            }
            if derived.has_failed(HISTORY_COMPONENT) || derived.readiness(HISTORY_COMPONENT).is_none() || tries >= MAX_TRIES
            {
                tracing::warn!("the stats history cannot be built: Cards Over Time computed from the snapshot");
                let read = read_cold(&snapshot, context, grid, request.history, selection, &cancelled);
                return finish(read, context, false, &cancelled);
            }
            if current {
                // Kept for another phase of `N` (H14.3): rebuilt on this
                // request's grid. The flush lets the writer mark it stale.
                shared.rebuild();
                derived.flush()?;
            }
        }
        derived.request_refresh(HISTORY_COMPONENT);
        match wait(derived, &request.cancel, &mut reporter) {
            Waited::Cancelled => return Ok(HistoryResult::Cancelled),
            Waited::Ready | Waited::Failed => {}
        }
        derived.flush()?;
        tries += 1;
    }
}

/// The parts, or why they cannot be given.
enum Read {
    Parts(CardStatesOverTime, StartedPart),
    Unreadable(Unreadable),
}

fn finish(
    read: Result<Read, StorageError>,
    context: &Context,
    from_component: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<HistoryResult, StatsError> {
    match read {
        Ok(Read::Parts(card_states, started)) => Ok(HistoryResult::Data(Box::new(HistoryData {
            context: context.clone(),
            card_states,
            started,
            from_component,
        }))),
        Ok(Read::Unreadable(bad)) => Ok(HistoryResult::UnreadableRow(bad)),
        Err(_) if cancelled() => Ok(HistoryResult::Cancelled),
        Err(error) => Err(error.into()),
    }
}

/// The kept series of the keys `selection` selects.
fn read_kept(
    snapshot: &Snapshot<'_>,
    context: &Context,
    grid: Grid,
    history: History,
    selection: Selection,
    cancelled: &dyn Fn() -> bool,
) -> Result<Read, StorageError> {
    let mut first = FirstBad::default();
    store::visit_kept_bad(snapshot, cancelled, &mut |bad| first.offer(&selection, bad))?;
    if let Some(bad) = first.first() {
        return Ok(Read::Unreadable(bad));
    }
    let mut totals = Totals::new(grid, context.next_day_at, history, selection);
    store::visit_kept_keys(snapshot, cancelled, &mut |key, count| totals.add_left_out(key, count))?;
    store::visit_kept_days(snapshot, cancelled, &mut |key, day, values| totals.add_day(key, day, values))?;
    let (card_states, started) = totals.finish();
    Ok(Read::Parts(card_states, started))
}

/// H18: the same series computed from the snapshot, without keeping it.
fn read_cold(
    snapshot: &Snapshot<'_>,
    context: &Context,
    grid: Grid,
    history: History,
    selection: Selection,
    cancelled: &dyn Fn() -> bool,
) -> Result<Read, StorageError> {
    let replayed = store::replay_all(&**snapshot, grid, cancelled)?;
    let mut first = FirstBad::default();
    for bad in replayed.bad {
        first.offer(&selection, bad);
    }
    if let Some(bad) = first.first() {
        return Ok(Read::Unreadable(bad));
    }
    let mut totals = Totals::new(grid, context.next_day_at, history, selection);
    totals.add_sums(&replayed.sums);
    let (card_states, started) = totals.finish();
    Ok(Read::Parts(card_states, started))
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
    finish(read, context, false, &cancelled)
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
