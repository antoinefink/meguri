//! The Stats graph engine (`stats/graphs-common`, U98): one request reads a
//! scope's cards and review-log rows for a history window, on one snapshot
//! and one context, and returns a record per graph it names.
//!
//! - Context (G1): the day clock first, then the clock, then the snapshot.
//! - Scope (G2) and history (G3), resolved once per request; rows read by
//!   G4's rules, an unreadable one failing the request.
//! - Results (G8.2), with the context token and memo of a refresh (G8.3).
//! - Caches (G9): the `stats_columns` component, read when current for the
//!   request's snapshot, else the request computes from the snapshot.
//! - Read only (G11): nothing here writes the collection or its config.

pub mod browse;
pub mod cache;
mod columns;
pub mod context;
pub mod encoding;
mod overlay;
pub mod records;
pub mod rows;
pub mod scope;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use meguri_engine::changes::stream::EventKind;
use meguri_engine::{CancelToken, Collection, DerivedHost, EngineError, Snapshot};
use meguri_model::ids::DeckId;
use meguri_sched::model::sched_config::SchedConfig;
use meguri_search::{
    BuildError, ErrorKind, FoldMode, Param, Plan, SearchContext, SearchError, SearchFailure, SearchMode,
    SearchNotetypes,
};
use meguri_storage::{Cell, ReadRows, ReaderRole};

pub use browse::BrowseScope;
pub use cache::{COMPONENT, HOME_IDLE, StatsColumns};
pub use context::{Context, ContextToken, Preferences};
pub use records::{Graph, GraphInput, GraphRecord, NotBuilt, Record};
pub use rows::{StatsCard, StatsRow};
pub use scope::{History, Scope, ScopeData, ScopeRow, Unreadable};
pub(crate) use scope::Span;

use columns::{CardsBuilder, RevlogBuilder};
use overlay::{Patch, View};
use scope::Membership;

/// One request (G8.1).
#[derive(Clone, Debug)]
pub struct GraphRequest {
    pub scope: Scope,
    pub history: History,
    /// The graphs wanted; a graph left out costs nothing (G5).
    pub graphs: Vec<Graph>,
    /// The refresh the request belongs to (G8.3).
    pub refresh: u64,
    /// Absent in a refresh's first request; in its second, the token of the
    /// first's Data result.
    pub context: Option<ContextToken>,
    pub cancel: Arc<CancelToken>,
    /// Where the copies come from.
    pub columns: Columns,
}

/// Where a request's column copies come from (G9.2: both give the same
/// values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Columns {
    /// The caches when they are current for the request's snapshot, else
    /// the snapshot itself.
    Cached,
    /// Always the snapshot: what the cache contract compares with (G9.6).
    Snapshot,
}

/// A scope error (G8.2): the search's error as `search/errors` gives it, or
/// a deck id that names no deck. The screen keeps the graphs it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    Search(SearchError),
    UnknownDeck,
}

/// A request's Data result (G8.2).
#[derive(Clone, Debug)]
pub struct GraphData {
    pub context: Context,
    pub token: ContextToken,
    pub preferences: Preferences,
    pub browse_scope: BrowseScope,
    /// One record per requested graph, in the request's order.
    pub records: Vec<Record>,
    /// The resolved scope the records were computed from.
    pub scope: Arc<ScopeData>,
}

/// A request's result (G8.2). An error or Context changed never adds to the
/// memo and never changes the caches.
#[derive(Clone, Debug)]
pub enum GraphResult {
    Data(Box<GraphData>),
    /// A second request whose snapshot is not its token's (G8.3).
    ContextChanged,
    ScopeError(ScopeError),
    UnreadableRow(Unreadable),
    Cancelled,
}

/// Why a request could not run.
#[derive(Debug)]
pub enum StatsError {
    Engine(EngineError),
    /// A graph whose record's unit has not landed.
    NotBuilt(Graph),
}

impl From<EngineError> for StatsError {
    fn from(error: EngineError) -> StatsError {
        StatsError::Engine(error)
    }
}

impl From<meguri_storage::StorageError> for StatsError {
    fn from(error: meguri_storage::StorageError) -> StatsError {
        StatsError::Engine(error.into())
    }
}

impl std::fmt::Display for StatsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatsError::Engine(error) => error.fmt(f),
            StatsError::NotBuilt(graph) => write!(f, "the {} graph is not built yet", graph.part()),
        }
    }
}

impl std::error::Error for StatsError {}

/// The memo's key (G8.3).
#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoKey {
    scope: Scope,
    history: History,
    generation: u64,
    seq: u64,
    next_day_at: i64,
    refresh: u64,
}

/// The resolved scope of the latest refresh's first request (G8.3).
struct Memo {
    key: MemoKey,
    data: Arc<ScopeData>,
    browse: BrowseScope,
}

/// What the scope resolution gives.
enum Resolved {
    Scope(Arc<ScopeData>, BrowseScope),
    Stop(GraphResult),
}

/// The graph engine of one host: the column caches and the memo.
pub struct StatsCaches {
    shared: Arc<cache::Shared>,
    memo: Mutex<Option<Memo>>,
    /// The `stats_history` component's state (`stats/history-reads` H14).
    history: Arc<crate::history::Shared>,
}

impl Default for StatsCaches {
    fn default() -> StatsCaches {
        StatsCaches::new()
    }
}

impl std::fmt::Debug for StatsCaches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatsCaches").finish_non_exhaustive()
    }
}

/// G9.2: the host's signal once Home has been idle for 2 s after an open;
/// it releases the column caches' build. Only calls the derived host's
/// `signal("homeIdle")`.
pub fn stats_home_idle(derived: &DerivedHost) {
    derived.signal(HOME_IDLE);
}

/// G9.5: the memory warning. Drops the memo, then the derived host's
/// `release("stats_columns")` merges the overlay into the files and unmaps
/// them. Never on the main thread.
pub fn release_stats_caches(caches: &StatsCaches, derived: &DerivedHost) {
    caches.drop_memo();
    derived.release(COMPONENT);
}

/// G10's events after which the screen's graphs are stale: any published
/// change set, a day or zone change, a replaced collection. Every open of
/// the screen refreshes too.
pub fn graphs_stale(event: &EventKind) -> bool {
    matches!(
        event,
        EventKind::Changes(_)
            | EventKind::RolledOver { .. }
            | EventKind::NextDayAtChanged { .. }
            | EventKind::TimeZoneChanged { .. }
            | EventKind::CollectionReplaced { .. }
    )
}

fn cells(params: &[Param]) -> Vec<Cell> {
    params
        .iter()
        .map(|param| match param {
            Param::Integer(value) => Cell::Integer(*value),
            Param::Real(value) => Cell::Real(*value),
            Param::Text(value) => Cell::text(value.clone()),
        })
        .collect()
}

impl StatsCaches {
    pub fn new() -> StatsCaches {
        StatsCaches {
            shared: Arc::default(),
            memo: Mutex::new(None),
            history: Arc::default(),
        }
    }

    /// The `stats_columns` component, for the derived host's builder.
    pub fn component(&self) -> StatsColumns {
        StatsColumns::new(Arc::clone(&self.shared))
    }

    /// The `stats_history` component (`stats/history-reads` H14), for the
    /// derived host's builder.
    pub fn history_component(&self) -> crate::history::StatsHistory {
        crate::history::StatsHistory::new(Arc::clone(&self.history))
    }

    pub(crate) fn history_shared(&self) -> &crate::history::Shared {
        &self.history
    }

    /// Drops the memo (G8.3): a newer refresh's first request, the page's
    /// one request (`crate::page`) included, drops it whatever it returns.
    pub(crate) fn drop_memo(&self) {
        *self.memo.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Runs one request (G8) on the query reader. Call it off the main
    /// thread (R1291).
    pub fn request(
        &self,
        collection: &Collection,
        derived: &DerivedHost,
        request: &GraphRequest,
    ) -> Result<GraphResult, StatsError> {
        let scope = request.scope.effective().clone();
        let result = match &request.context {
            None => self.first(collection, derived, request, scope),
            Some(token) => self.second(collection, derived, request, scope, token),
        };
        if matches!(result, Ok(GraphResult::Cancelled)) || request.cancel.is_cancelled() {
            // Nothing is kept from a cancelled request (G8.2, G8.3).
            self.drop_memo();
            return Ok(GraphResult::Cancelled);
        }
        result
    }

    fn first(
        &self,
        collection: &Collection,
        derived: &DerivedHost,
        request: &GraphRequest,
        scope: Scope,
    ) -> Result<GraphResult, StatsError> {
        // A newer refresh's first request drops the memo, whatever it
        // returns (G8.3).
        self.drop_memo();
        if request.cancel.is_cancelled() {
            return Ok(GraphResult::Cancelled);
        }
        // G1's order: the day clock (its check and commit, if any), then the
        // clock, then the snapshot, which so holds what the check wrote.
        let day = collection.check_rollover()?;
        let clock = collection.clock();
        let now_ms = clock.now_ms();
        let utc_offset_secs = clock.utc_offset_at(now_ms).seconds();
        let zone = clock.zone_id();
        let lease = collection.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        let context = Context {
            next_day_at: day.next_day_at,
            today: day.today,
            rollover_hour: SchedConfig::new(&*snapshot).rollover_hour()?,
            now_ms,
            utc_offset_secs,
            zone,
            generation: snapshot.generation().0,
            seq: snapshot.seq().0,
        };
        let key = MemoKey {
            scope: scope.clone(),
            history: request.history,
            generation: context.generation,
            seq: context.seq,
            next_day_at: context.next_day_at,
            refresh: request.refresh,
        };
        let (data, browse) = match self.resolve(derived, &snapshot, &context, &scope, request)? {
            Resolved::Scope(data, browse) => (data, browse),
            Resolved::Stop(result) => return Ok(result),
        };
        let result = self.data(&snapshot, &context, &scope, request, &data, browse.clone())?;
        if matches!(result, GraphResult::Data(_)) && !request.cancel.is_cancelled() {
            *self.memo.lock().unwrap_or_else(PoisonError::into_inner) = Some(Memo { key, data, browse });
        }
        Ok(result)
    }

    fn second(
        &self,
        collection: &Collection,
        derived: &DerivedHost,
        request: &GraphRequest,
        scope: Scope,
        token: &ContextToken,
    ) -> Result<GraphResult, StatsError> {
        if request.cancel.is_cancelled() {
            return Ok(GraphResult::Cancelled);
        }
        if token.refresh != request.refresh || token.scope != scope || token.history != request.history {
            return Ok(GraphResult::ContextChanged);
        }
        // No day clock: the token's values, on a snapshot of its state.
        let lease = collection.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        let context = token.context.clone();
        if snapshot.generation().0 != context.generation || snapshot.seq().0 != context.seq {
            return Ok(GraphResult::ContextChanged);
        }
        let key = MemoKey {
            scope: scope.clone(),
            history: request.history,
            generation: context.generation,
            seq: context.seq,
            next_day_at: context.next_day_at,
            refresh: request.refresh,
        };
        // Only this refresh's second request takes the memo, and it goes
        // when that request returns, whatever it gives (G8.3).
        let memo = {
            let mut held = self.memo.lock().unwrap_or_else(PoisonError::into_inner);
            let taken = if held.as_ref().is_some_and(|memo| memo.key == key) { held.take() } else { None };
            // A Snapshot request computes afresh, and the memo goes anyway.
            taken.filter(|_| request.columns == Columns::Cached)
        };
        let (data, browse) = match memo {
            Some(memo) => (memo.data, memo.browse),
            None => match self.resolve(derived, &snapshot, &context, &scope, request)? {
                Resolved::Scope(data, browse) => (data, browse),
                Resolved::Stop(result) => return Ok(result),
            },
        };
        self.data(&snapshot, &context, &scope, request, &data, browse)
    }

    /// The Data result over a resolved scope.
    fn data(
        &self,
        snapshot: &Snapshot<'_>,
        context: &Context,
        scope: &Scope,
        request: &GraphRequest,
        data: &Arc<ScopeData>,
        browse_scope: BrowseScope,
    ) -> Result<GraphResult, StatsError> {
        let preferences = Preferences::read(&**snapshot)?;
        let input = GraphInput {
            scope: data.as_ref(),
            context,
            preferences: &preferences,
            history: request.history,
        };
        let mut records = Vec::with_capacity(request.graphs.len());
        for graph in &request.graphs {
            if request.cancel.is_cancelled() {
                return Ok(GraphResult::Cancelled);
            }
            records.push(records::compute(*graph, &input).map_err(|NotBuilt(graph)| StatsError::NotBuilt(graph))?);
        }
        Ok(GraphResult::Data(Box::new(GraphData {
            scope: Arc::clone(data),
            context: context.clone(),
            token: ContextToken {
                refresh: request.refresh,
                scope: scope.clone(),
                history: request.history,
                context: context.clone(),
            },
            preferences,
            browse_scope,
            records,
        })))
    }

    /// The Statistics page's request (`crate::page`, U293): [`Self::resolve`]
    /// on `snapshot` with `context`, with no memo, over a view that holds
    /// every row (from the caches, or read from the snapshot from id 0 on),
    /// so the parts that read all history whatever the window
    /// (`stats/history-reads` H9, H11) read the same view
    /// ([`ScopeData::rows_from`]). `Err` holds the result that stops the
    /// request (a scope error, `unreadable_row`, Cancelled).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_scope(
        &self,
        derived: &DerivedHost,
        snapshot: &Snapshot<'_>,
        context: &Context,
        scope: &Scope,
        history: History,
        columns: Columns,
        cancel: &Arc<CancelToken>,
    ) -> Result<Result<(Arc<ScopeData>, BrowseScope), GraphResult>, StatsError> {
        let span = Span {
            start: history.window_start(context.next_day_at),
            end: None,
            view_from: 0,
        };
        Ok(match self.resolve_span(derived, snapshot, context, scope, span, columns, cancel)? {
            Resolved::Scope(data, browse) => Ok((data, browse)),
            Resolved::Stop(result) => Err(result),
        })
    }

    /// The scope's cards and its rows with `start <= id < end` (no upper
    /// end without `end`), on `snapshot` with `context`, with no memo: the
    /// read behind one day's cards (`stats/history-reads` H13). An
    /// unreadable row of the scope in that span stops it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_ids(
        &self,
        derived: &DerivedHost,
        snapshot: &Snapshot<'_>,
        context: &Context,
        scope: &Scope,
        start: i64,
        end: Option<i64>,
        columns: Columns,
        cancel: &Arc<CancelToken>,
    ) -> Result<Result<Arc<ScopeData>, GraphResult>, StatsError> {
        let span = Span {
            start,
            end,
            view_from: start,
        };
        Ok(match self.resolve_span(derived, snapshot, context, scope, span, columns, cancel)? {
            Resolved::Scope(data, _) => Ok(data),
            Resolved::Stop(result) => Err(result),
        })
    }

    /// G2, G3, G4 and Q2 on the snapshot: the scope's cards and window
    /// rows, the first unreadable one, and the browse scope text. Every
    /// request resolves the whole scope, whatever graphs it names (G8.3).
    fn resolve(
        &self,
        derived: &DerivedHost,
        snapshot: &Snapshot<'_>,
        context: &Context,
        scope: &Scope,
        request: &GraphRequest,
    ) -> Result<Resolved, StatsError> {
        let span = Span::window(request.history.window_start(context.next_day_at));
        self.resolve_span(derived, snapshot, context, scope, span, request.columns, &request.cancel)
    }

    /// [`Self::resolve`] over the rows of `span`.
    #[allow(clippy::too_many_arguments)]
    fn resolve_span(
        &self,
        derived: &DerivedHost,
        snapshot: &Snapshot<'_>,
        context: &Context,
        scope: &Scope,
        span: Span,
        columns: Columns,
        cancel: &Arc<CancelToken>,
    ) -> Result<Resolved, StatsError> {
        let cancelled = || cancel.is_cancelled();
        // Before the snapshot's first read of `derived` (D8).
        let cached = if columns == Columns::Cached && derived.is_current(COMPONENT, snapshot) {
            derived
                .path()
                .and_then(|store| self.shared.view(snapshot, &meguri_storage::derived::component_dir(&store, COMPONENT)))
        } else {
            None
        };
        let (membership, browse) = match scope {
            Scope::Collection => (Membership::All, BrowseScope::None),
            Scope::Deck(deck) => {
                let decks = snapshot.deck_rows()?;
                let Some(set) = scope::deck_set(&decks, *deck) else {
                    return Ok(Resolved::Stop(GraphResult::ScopeError(ScopeError::UnknownDeck)));
                };
                let name = decks.iter().find(|d| d.id == *deck).map(|d| d.name.clone()).unwrap_or_default();
                let browse = browse::deck_text(&name, &set);
                (Membership::Decks(set), browse)
            }
            Scope::Search(text) => match search(snapshot, context, text, &cancelled)? {
                Ok(ids) => (Membership::Cards(ids), browse::search_text(text)),
                Err(result) => return Ok(Resolved::Stop(result)),
            },
        };
        if cancelled() {
            return Ok(Resolved::Stop(GraphResult::Cancelled));
        }
        let from_caches = cached.is_some();
        let (view, span) = match cached {
            // The caches hold every row.
            Some(view) => (view, Span { view_from: i64::MIN, ..span }),
            None => match cold_view(snapshot, span.view_from.min(span.start), span.end, &cancelled) {
                Ok(view) => (view, span),
                Err(_) if cancelled() => return Ok(Resolved::Stop(GraphResult::Cancelled)),
                Err(error) => return Err(error.into()),
            },
        };
        let data = ScopeData::new(view, &membership, span, from_caches);
        if let Some(bad) = data.first_unreadable() {
            return Ok(Resolved::Stop(GraphResult::UnreadableRow(bad)));
        }
        Ok(Resolved::Scope(Arc::new(data), browse))
    }
}

/// G2's Search scope: the cards `text` matches in cards mode, with the
/// request's context; a parse or run failure is the scope error, a cancel
/// the Cancelled result.
fn search(
    snapshot: &Snapshot<'_>,
    context: &Context,
    text: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Result<HashSet<i64>, GraphResult>, StatsError> {
    let query = match meguri_search::parse(text) {
        Ok(query) => query,
        Err(error) => return Ok(Err(GraphResult::ScopeError(ScopeError::Search(error)))),
    };
    let config = SchedConfig::new(&**snapshot);
    let flag = |key: &str, missing: bool| -> Result<bool, StatsError> {
        Ok(snapshot.config_value(key)?.and_then(|value| value.as_bool()).unwrap_or(missing))
    };
    let search_context = SearchContext {
        now: context.now_ms.div_euclid(1000),
        today: context.today,
        next_day_at: context.next_day_at,
        learn_ahead_secs: config.collapse_time()?,
        // `search/names-and-ids` N3.4: `curDeck` as stored, 1 when it does
        // not read as an integer. Read, never written (R1353).
        current_deck: DeckId(
            snapshot
                .config_value("curDeck")?
                .and_then(|value| value.as_i64())
                .unwrap_or(1),
        ),
        normalize_note_text: flag("normalize_note_text", true)?,
        ignore_accents: flag("ignoreAccentsInSearch", false)?,
        fold: FoldMode::Unicode,
    };
    let notetypes = SearchNotetypes::read(&**snapshot)?;
    let plan = match Plan::build(&query, SearchMode::Cards, &search_context, &notetypes) {
        Ok(plan) => plan,
        Err(BuildError::Search(error)) => return Ok(Err(GraphResult::ScopeError(ScopeError::Search(error)))),
    };
    let mut ids = HashSet::new();
    let ran = snapshot.visit_id_rows(plan.sql(), &cells(plan.params()), cancelled, &mut |row| {
        ids.insert(row[0]);
        true
    });
    match ran {
        Ok(()) => Ok(Ok(ids)),
        Err(_) if cancelled() => Ok(Err(GraphResult::Cancelled)),
        // `search/errors` E3: a statement the database refused.
        Err(_) => Ok(Err(GraphResult::ScopeError(ScopeError::Search(SearchError::new(
            ErrorKind::SearchFailed(SearchFailure::DatabaseRefusal),
            0..text.chars().count(),
        ))))),
    }
}

/// The copies built from the snapshot itself (G9.2: equal to the caches'):
/// every card and the rows with `from <= id < below` (no upper end without
/// `below`).
fn cold_view(
    snapshot: &Snapshot<'_>,
    from: i64,
    below: Option<i64>,
    cancelled: &dyn Fn() -> bool,
) -> Result<View, meguri_storage::StorageError> {
    let mut cards = CardsBuilder::default();
    snapshot.visit_stats_cards(cancelled, &mut |cells| {
        cards.push(rows::read_card(cells, &rows::VISITED_CARD));
        true
    })?;
    let slots = cards.slots();
    let mut revlog = RevlogBuilder::default();
    snapshot.visit_stats_revlog(from, below, cancelled, &mut |cells| {
        revlog.push(rows::read_row(cells), |cid| slots.get(&cid).copied());
        true
    })?;
    Ok(View {
        cards: Arc::new(cards.table()),
        revlog: Arc::new(revlog.table()),
        patch: Arc::new(Patch::default()),
    })
}
