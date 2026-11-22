//! The Statistics page's request (`stats/history-reads` H5, H15 to H18;
//! the round 11 lines of `stats/graphs-common` G3, G5, G7, G8.3, G10), U293.
//!
//! One request per refresh names every part the page wants. The core
//! returns them by tier as each is ready (H15): the present state, then
//! the review log, then knowledge; each part as its own message carrying
//! the request's header (G1's context and its token, G7's preferences,
//! Q2's browse text, H9's oldest day), with progress for the parts that
//! wait. The request ends with its final value once every part has
//! returned or failed.
//!
//! - Before any part, one snapshot resolves the scope and the window: a
//!   scope error or an unreadable row there ends the request with no part
//!   (H15, G8.3).
//! - A part that fails alone (`unsupported_scope`, an unreadable row
//!   outside the window for the all-history parts, a crate error, a part
//!   whose unit has not landed) is reported for that part; the others
//!   still return.
//! - Cancellation (H16): no message after the token fires; the reader
//!   stops within 50 ms; a `stats_history` build the request started goes
//!   on. A record is computed whole from memory once its scope is read, so
//!   a cancel during one is seen when it ends (no reader is held then, and
//!   nothing more is sent). The token is checked just before each message,
//!   so a cancel from another thread at that instant can let one message
//!   through: the receiver drops messages of a request it cancelled.
//! - Read only (H17): nothing here writes the collection. The day clock's
//!   own check, which G1 runs first, is the day clock's write.

use std::collections::BTreeMap;
use std::sync::Arc;

use meguri_engine::{CancelToken, Collection, DerivedHost};
use meguri_sched::model::sched_config::SchedConfig;
use meguri_storage::ReaderRole;

use crate::graphs::records::{self, Graph, GraphInput, NotBuilt, Record};
use crate::graphs::{
    BrowseScope, Columns, Context, ContextToken, GraphResult, History, Preferences, Scope, ScopeData, ScopeError,
    StatsCaches, StatsError, Unreadable,
};
use crate::history::{self, CardStatesOverTime, HistoryRequest, HistoryResult, Progress, StartedPart};
use crate::parts::activity::{self, ActivityDaysPart};
use crate::parts::by_answer;
use crate::parts::by_deck::{self, ByDeckPart};
use crate::parts::decks::DeckFacts;
use crate::parts::retention_by_day::{self, RetentionByDayPart};
use crate::parts::studied_today::{self, StudiedTodayPart};

pub use crate::parts::by_answer::{Answers, ByAnswerPart};

/// The three tiers of `stats/statistics-page` S10.1, in the order the core
/// returns them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tier {
    /// The present state: card graphs and the strip's numbers.
    Present = 1,
    /// The review log, the history parts included.
    ReviewLog = 2,
    /// Knowledge Over Time.
    Knowledge = 3,
}

/// A part of the page request: H5's list, with By Answer (H6) as a part of
/// its own beside the `reviews` record (decided by the coordinator for G6's
/// size limit: the record keeps Anki's members only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    CardCounts,
    FutureDue,
    Intervals,
    Stability,
    Difficulty,
    Retrievability,
    StudiedToday,
    Reviews,
    ByAnswer,
    Retention,
    RetentionByDay,
    Hours,
    Buttons,
    Added,
    CardStatesOverTime,
    Started,
    ByDeck,
    ActivityDays,
    KnowledgeOverTime,
}

/// Where a part's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// An Anki record with its round 11 extras (G5, H7, H8).
    Graph(Graph),
    /// H6: the rows `reviews` counts, by answer.
    ByAnswer,
    /// H12: the strip's numbers, from the snapshot by id range.
    StudiedToday,
    /// H3.4 and H4: retention and Forgotten per day.
    RetentionByDay,
    /// H10: the table per deck.
    ByDeck,
    /// H11: the activity of every year.
    ActivityDays,
    /// H2's read of the `stats_history` component.
    History,
    /// H3's knowledge (U290).
    Knowledge,
    /// A part whose unit has not landed: it fails alone with
    /// [`PartFailure::NotBuilt`]. The unit named plugs it in here. None is
    /// pending now (Knowledge has a source of its own).
    #[allow(dead_code)]
    Pending(&'static str),
}

/// Each part: its name (H5's), its tier, its source, and whether it is one
/// of G5's Meguri parts, which run with the Collection and Deck scopes only
/// (H1: a Search scope gives `unsupported_scope` for that part). By Answer
/// reads the `reviews` record's rows, so it follows that record's scopes.
const PARTS: [(Part, &str, Tier, Source, bool); 19] = [
    (Part::CardCounts, "card_counts", Tier::Present, Source::Graph(Graph::CardCounts), false),
    (Part::FutureDue, "future_due", Tier::Present, Source::Graph(Graph::FutureDue), false),
    (Part::Intervals, "intervals", Tier::Present, Source::Graph(Graph::Intervals), false),
    (Part::Stability, "stability", Tier::Present, Source::Graph(Graph::Stability), false),
    (Part::Difficulty, "difficulty", Tier::Present, Source::Graph(Graph::Difficulty), false),
    (Part::Retrievability, "retrievability", Tier::Present, Source::Graph(Graph::Retrievability), false),
    (Part::StudiedToday, "studied_today", Tier::Present, Source::StudiedToday, true),
    (Part::Reviews, "reviews", Tier::ReviewLog, Source::Graph(Graph::Reviews), false),
    (Part::ByAnswer, "by_answer", Tier::ReviewLog, Source::ByAnswer, false),
    (Part::Retention, "retention", Tier::ReviewLog, Source::Graph(Graph::Retention), false),
    (Part::RetentionByDay, "retention_by_day", Tier::ReviewLog, Source::RetentionByDay, true),
    (Part::Hours, "hours", Tier::ReviewLog, Source::Graph(Graph::Hourly), false),
    (Part::Buttons, "buttons", Tier::ReviewLog, Source::Graph(Graph::AnswerButtons), false),
    (Part::Added, "added", Tier::ReviewLog, Source::Graph(Graph::Added), false),
    (Part::CardStatesOverTime, "card_states_over_time", Tier::ReviewLog, Source::History, true),
    (Part::Started, "started", Tier::ReviewLog, Source::History, true),
    (Part::ByDeck, "by_deck", Tier::ReviewLog, Source::ByDeck, true),
    (Part::ActivityDays, "activity_days", Tier::ReviewLog, Source::ActivityDays, true),
    (Part::KnowledgeOverTime, "knowledge_over_time", Tier::Knowledge, Source::Knowledge, true),
];

impl Part {
    /// Every part, tier by tier.
    pub fn all() -> impl Iterator<Item = Part> {
        PARTS.iter().map(|entry| entry.0)
    }

    fn entry(self) -> &'static (Part, &'static str, Tier, Source, bool) {
        PARTS.iter().find(|entry| entry.0 == self).expect("every part is listed")
    }

    /// Its name in H5 (`by_answer` for By Answer).
    pub fn name(self) -> &'static str {
        self.entry().1
    }

    /// The part a name gives, if any.
    pub fn named(name: &str) -> Option<Part> {
        PARTS.iter().find(|entry| entry.1 == name).map(|entry| entry.0)
    }

    pub fn tier(self) -> Tier {
        self.entry().2
    }

    fn source(self) -> Source {
        self.entry().3
    }

    /// One of G5's Meguri parts: Collection and Deck scopes only (H1).
    fn page_scopes_only(self) -> bool {
        self.entry().4
    }

    /// Whether the part may wait on work longer than a moment (the
    /// component's build): within its tier it comes after the parts that
    /// are ready at once, so they are not held behind it (H15).
    fn may_wait(self) -> bool {
        matches!(self.source(), Source::History | Source::Knowledge)
    }
}

/// The page's period (`stats/statistics-page` S3.2). 1M to 1Y read G3's
/// `last12Months` and re-bin in Swift; All reads `allHistory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    OneMonth,
    ThreeMonths,
    SixMonths,
    OneYear,
    All,
}

impl Period {
    pub fn history(self) -> History {
        match self {
            Period::All => History::AllHistory,
            _ => History::Last12Months,
        }
    }
}

/// One refresh of the page: one request naming its parts (G8.3's round 11
/// line).
#[derive(Clone, Debug)]
pub struct PageRequest {
    /// All Decks is `Collection`, a deck `Deck(id)` with its subdecks
    /// (G2.1). A `Search` scope is not offered in this version: G5's Meguri
    /// parts then fail with `unsupported_scope`.
    pub scope: Scope,
    pub history: History,
    /// The parts wanted; one named twice is returned once.
    pub parts: Vec<Part>,
    /// The refresh's id, carried in the context token (G8.3).
    pub refresh: u64,
    pub cancel: Arc<CancelToken>,
    /// [`Columns::Snapshot`] computes every part from the snapshot, the
    /// history parts included: what H14.4's equivalence compares with.
    pub columns: Columns,
}

/// What every Data message of a request carries (G8.2, H5's last line).
#[derive(Clone, Debug)]
pub struct PageHeader {
    pub context: Context,
    pub token: ContextToken,
    pub preferences: Preferences,
    /// The scope's browse text (`stats/browse-queries` Q2).
    pub browse_scope: BrowseScope,
    pub oldest_day: OldestDay,
}

/// H9's `oldest_day`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OldestDay {
    /// The smallest day (H1, folded) of a row of the scope with `id` ≥ 0.
    Day(i32),
    /// The scope has no such row.
    NoRows,
    /// Not known: an unreadable row of the scope outside the window (H18:
    /// `UnreadableRow`), or a failure reading the rows of all history.
    Unavailable(PartFailure),
}

/// A part's value.
#[derive(Clone, Debug)]
pub enum PartValue {
    /// The Anki record of the part's graph (G6), with its H6 to H8 extras.
    Record(Record),
    ByAnswer(ByAnswerPart),
    /// H12 (`studied_today`).
    StudiedToday(StudiedTodayPart),
    /// H3.4 and H4 (`retention_by_day`).
    RetentionByDay(RetentionByDayPart),
    /// H10 (`by_deck`).
    ByDeck(ByDeckPart),
    /// H11 (`activity_days`).
    ActivityDays(ActivityDaysPart),
    CardStates(CardStatesOverTime),
    Started(StartedPart),
}

/// Why one part has no value; the request's other parts still return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartFailure {
    /// A Search scope for one of G5's Meguri parts (H1, H18).
    UnsupportedScope,
    /// An unreadable row of the scope outside the window, for the parts
    /// that read all history (H15, H18).
    UnreadableRow(Unreadable),
    /// The part's unit has not landed.
    NotBuilt,
    /// A crate error that stopped the whole part (H15).
    Failed(String),
}

/// A part's result.
#[derive(Clone, Debug)]
pub enum PartOutcome {
    Data(PartValue),
    Failed(PartFailure),
}

/// One part, returned.
#[derive(Clone, Debug)]
pub struct PartMessage {
    pub part: Part,
    pub header: Arc<PageHeader>,
    pub outcome: PartOutcome,
}

/// A message of a running request, in the order it is sent.
#[derive(Clone, Debug)]
pub enum PageMessage {
    /// A part is ready, or has failed.
    Part(PartMessage),
    /// A part waits (H15): a fraction between 0 and 1 that never goes back,
    /// or indeterminate; at most every 100 ms.
    Progress { part: Part, progress: Progress },
}

/// How a request ends: its final message (H15).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageEnd {
    /// Every part named has returned or failed.
    Complete,
    /// G8.2's scope error (a search error, `unknown_deck`), before any
    /// part: every part fails alike.
    ScopeError(ScopeError),
    /// An unreadable row of the scope in the window, before any part.
    UnreadableRow(Unreadable),
    /// A commit landed while a part waited, after the request's snapshot:
    /// the parts not yet returned are dropped, and the page, stale anyway
    /// (G10), starts a new refresh. The parts returned hold for their
    /// context.
    ContextChanged,
    /// The token fired: nothing more was sent (H16).
    Cancelled,
}

/// Runs one page request: sends each part to `sink` as it is ready, by
/// tier, and returns how the request ended. Call it off the main thread
/// (R1291). `Err` is an error before any part (a reader that cannot open);
/// an error inside one part fails that part only.
pub fn request(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    request: &PageRequest,
    sink: &mut dyn FnMut(PageMessage),
) -> Result<PageEnd, StatsError> {
    match run(caches, collection, derived, request, sink) {
        // A fired token is Cancelled whatever stopped the work (G8.2).
        _ if request.cancel.is_cancelled() => Ok(PageEnd::Cancelled),
        other => other,
    }
}

fn run(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    request: &PageRequest,
    sink: &mut dyn FnMut(PageMessage),
) -> Result<PageEnd, StatsError> {
    let cancel = &request.cancel;
    let mut out = Sink { cancel, sink };
    // The one request of a refresh drops the memo, as a first request does
    // (G8.3); this request makes none.
    caches.drop_memo();
    if cancel.is_cancelled() {
        return Ok(PageEnd::Cancelled);
    }
    let scope = request.scope.effective().clone();
    let parts = order(&request.parts);

    // G1's order: the day clock (its check and commit, if any), then the
    // clock; the derived writer applies every commit sent to it before the
    // snapshot's first read of the store (D8); then the snapshot.
    let day = collection.check_rollover()?;
    let clock = collection.clock();
    let now_ms = clock.now_ms();
    let utc_offset_secs = clock.utc_offset_at(now_ms).seconds();
    let zone = clock.zone_id();
    derived.flush()?;
    let (header, scope_data, reads) = {
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
        // The scope and the window, before any part (H15).
        let resolved =
            caches.resolve_scope(derived, &snapshot, &context, &scope, request.history, request.columns, cancel)?;
        let (data, browse_scope) = match resolved {
            Ok(resolved) => resolved,
            Err(GraphResult::ScopeError(error)) => return Ok(PageEnd::ScopeError(error)),
            Err(GraphResult::UnreadableRow(bad)) => return Ok(PageEnd::UnreadableRow(bad)),
            Err(GraphResult::Cancelled) => return Ok(PageEnd::Cancelled),
            // A first resolution gives no other result.
            Err(GraphResult::ContextChanged | GraphResult::Data(_)) => return Ok(PageEnd::ContextChanged),
        };
        let preferences = match Preferences::read(&*snapshot) {
            Ok(preferences) => preferences,
            Err(_) if cancel.is_cancelled() => return Ok(PageEnd::Cancelled),
            Err(error) => return Err(error.into()),
        };
        // H9 and H11 read the scope's rows of all history on this snapshot,
        // whatever the window: an unreadable one outside it fails only them
        // (H15, H18).
        let all_rows = all_history_rows(&data);
        let oldest_day = match &all_rows {
            Ok(all) => activity::oldest_day(all, context.next_day_at).map_or(OldestDay::NoRows, OldestDay::Day),
            Err(failure) => OldestDay::Unavailable(failure.clone()),
        };
        let reads = SnapshotReads::read(&snapshot, &scope, &context, &parts, all_rows);
        let header = PageHeader {
            token: ContextToken {
                refresh: request.refresh,
                scope: scope.clone(),
                history: request.history,
                context: context.clone(),
            },
            context,
            preferences,
            browse_scope,
            oldest_day,
        };
        (Arc::new(header), data, reads)
    };
    if cancel.is_cancelled() {
        return Ok(PageEnd::Cancelled);
    }
    // The history build may take seconds on a first open: release it now
    // (H14.2, S10.6), so it runs while the first tiers compute.
    if request.columns == Columns::Cached
        && !matches!(scope, Scope::Search(_))
        && parts.iter().any(|part| part.source() == Source::History)
    {
        history::release_build(caches, derived, &header.context);
    }

    let input = GraphInput {
        scope: scope_data.as_ref(),
        context: &header.context,
        preferences: &header.preferences,
        history: request.history,
    };
    let searching = matches!(scope, Scope::Search(_));
    let mut built: BTreeMap<Graph, Result<Record, NotBuilt>> = BTreeMap::new();
    let mut record = |graph: Graph| built.entry(graph).or_insert_with(|| records::compute(graph, &input)).clone();
    let mut history_read: Option<HistoryRead> = None;
    let history_parts: Vec<Part> = parts.iter().copied().filter(|p| p.source() == Source::History).collect();

    for part in parts {
        if cancel.is_cancelled() {
            return Ok(PageEnd::Cancelled);
        }
        let outcome = if searching && part.page_scopes_only() {
            PartOutcome::Failed(PartFailure::UnsupportedScope)
        } else {
            match part.source() {
                Source::Graph(graph) => match record(graph) {
                    Ok(record) => PartOutcome::Data(PartValue::Record(record)),
                    Err(NotBuilt(_)) => PartOutcome::Failed(PartFailure::NotBuilt),
                },
                Source::ByAnswer => PartOutcome::Data(PartValue::ByAnswer(by_answer::build(
                    scope_data.as_ref(),
                    header.context.next_day_at,
                ))),
                Source::StudiedToday => match &reads.studied {
                    Some(Ok(studied)) => PartOutcome::Data(PartValue::StudiedToday(*studied)),
                    Some(Err(failure)) => PartOutcome::Failed(failure.clone()),
                    None => PartOutcome::Failed(PartFailure::Failed("the strip's numbers were not read".into())),
                },
                Source::RetentionByDay => match &reads.decks {
                    Some(Ok(decks)) => PartOutcome::Data(PartValue::RetentionByDay(retention_by_day::build(
                        scope_data.as_ref(),
                        header.context.next_day_at,
                        decks,
                    ))),
                    Some(Err(failure)) => PartOutcome::Failed(failure.clone()),
                    None => PartOutcome::Failed(PartFailure::Failed("the decks were not read".into())),
                },
                Source::ByDeck => match &reads.decks {
                    Some(Ok(decks)) => PartOutcome::Data(PartValue::ByDeck(by_deck::build(
                        scope_data.as_ref(),
                        &scope,
                        request.history,
                        header.context.next_day_at,
                        decks,
                    ))),
                    Some(Err(failure)) => PartOutcome::Failed(failure.clone()),
                    None => PartOutcome::Failed(PartFailure::Failed("the decks were not read".into())),
                },
                Source::ActivityDays => match &reads.all_rows {
                    Ok(all) => PartOutcome::Data(PartValue::ActivityDays(activity::build(all, header.context.next_day_at))),
                    Err(failure) => PartOutcome::Failed(failure.clone()),
                },
                Source::History => {
                    let read = match &history_read {
                        Some(read) => read,
                        None => {
                            let read =
                                read_history(caches, collection, derived, &header.context, request, &history_parts, &mut out);
                            match read {
                                Err(end) => return Ok(end),
                                Ok(read) => history_read.insert(read),
                            }
                        }
                    };
                    history_outcome(part, read)
                }
                // U290 (H3) plugs in here: its read on `header.context`, as
                // `read_history` calls H2's, with progress for this part
                // (the build's, then the replay's), mapped onto one
                // outcome; its `failed` count stays in its value.
                Source::Knowledge => PartOutcome::Failed(PartFailure::NotBuilt),
                Source::Pending(_unit) => PartOutcome::Failed(PartFailure::NotBuilt),
            }
        };
        out.send(PageMessage::Part(PartMessage {
            part,
            header: Arc::clone(&header),
            outcome,
        }));
    }
    if cancel.is_cancelled() {
        return Ok(PageEnd::Cancelled);
    }
    Ok(PageEnd::Complete)
}

/// The sink, closed once the token fires: no message after a cancel (H16).
struct Sink<'a> {
    cancel: &'a CancelToken,
    sink: &'a mut dyn FnMut(PageMessage),
}

impl Sink<'_> {
    fn send(&mut self, message: PageMessage) {
        if !self.cancel.is_cancelled() {
            (self.sink)(message);
        }
    }
}

/// The scope's rows of all history (`id` ≥ 0) over the request's view
/// (`stats/history-reads` H9, H11), or why they cannot be read: the first
/// unreadable one outside the window (H18).
fn all_history_rows(data: &Arc<ScopeData>) -> Result<Arc<ScopeData>, PartFailure> {
    if data.window_start() <= 0 {
        // The window is all history, and resolved without an unreadable row.
        return Ok(Arc::clone(data));
    }
    let Some(all) = data.from_id(0) else {
        return Err(PartFailure::Failed("the view lacks the rows before the window".into()));
    };
    match all.first_unreadable() {
        Some(bad) => Err(PartFailure::UnreadableRow(bad)),
        None => Ok(Arc::new(all)),
    }
}

/// What the parts read on the request's snapshot besides its scope, before
/// the first part (H12's numbers; the decks for H3.4, H4 and H10), and the
/// scope's rows of all history. `None` for what no requested part needs.
struct SnapshotReads {
    studied: Option<Result<StudiedTodayPart, PartFailure>>,
    decks: Option<Result<DeckFacts, PartFailure>>,
    all_rows: Result<Arc<ScopeData>, PartFailure>,
}

impl SnapshotReads {
    fn read(
        snapshot: &meguri_engine::Snapshot<'_>,
        scope: &Scope,
        context: &Context,
        parts: &[Part],
        all_rows: Result<Arc<ScopeData>, PartFailure>,
    ) -> SnapshotReads {
        let searching = matches!(scope, Scope::Search(_));
        let wants = |source: Source| !searching && parts.iter().any(|part| part.source() == source);
        let studied = wants(Source::StudiedToday).then(|| {
            studied_today::read(&**snapshot, scope, context.next_day_at).map_err(|error| {
                tracing::warn!(%error, "the page's studied-today numbers failed");
                PartFailure::Failed(error.to_string())
            })
        });
        let decks = (wants(Source::RetentionByDay) || wants(Source::ByDeck)).then(|| {
            DeckFacts::read(&**snapshot).map_err(|error| {
                tracing::warn!(%error, "the page's deck read failed");
                PartFailure::Failed(error.to_string())
            })
        });
        SnapshotReads { studied, decks, all_rows }
    }
}

/// The parts once each, by tier; within a tier the parts that are ready at
/// once first, then those that may wait, each group in the request's order.
fn order(parts: &[Part]) -> Vec<Part> {
    let mut unique: Vec<Part> = Vec::with_capacity(parts.len());
    for part in parts {
        if !unique.contains(part) {
            unique.push(*part);
        }
    }
    // A stable sort keeps the request's order within each group.
    unique.sort_by_key(|part| (part.tier(), part.may_wait()));
    unique
}

/// What H2's read gave the history parts.
type HistoryRead = Result<Box<(CardStatesOverTime, StartedPart)>, PartFailure>;

/// H2's read, once for both history parts, on the request's context: from
/// the component (waiting for its build with progress, H14.2, H15), or,
/// for a [`Columns::Snapshot`] request, from the snapshot. `Err` ends the
/// request.
fn read_history(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    context: &Context,
    request: &PageRequest,
    parts: &[Part],
    out: &mut Sink<'_>,
) -> Result<HistoryRead, PageEnd> {
    let read = HistoryRequest {
        scope: request.scope.clone(),
        history: request.history,
        cancel: Arc::clone(&request.cancel),
    };
    let result = match request.columns {
        Columns::Cached => history::card_states_in(caches, collection, derived, context, &read, &mut |progress| {
            for part in parts {
                out.send(PageMessage::Progress { part: *part, progress });
            }
        }),
        Columns::Snapshot => history::card_states_cold(collection, context, &read),
    };
    match result {
        _ if request.cancel.is_cancelled() => Err(PageEnd::Cancelled),
        Ok(HistoryResult::Data(data)) => Ok(Ok(Box::new((data.card_states, data.started)))),
        // Rows outside the window: the windowed parts have returned (H15).
        Ok(HistoryResult::UnreadableRow(bad)) => Ok(Err(PartFailure::UnreadableRow(bad))),
        Ok(HistoryResult::UnsupportedScope) => Ok(Err(PartFailure::UnsupportedScope)),
        // The scope resolved on the request's snapshot and the read's has
        // its generation and seq, so the deck is there; G8.2 makes it the
        // request's error all the same.
        Ok(HistoryResult::UnknownDeck) => Err(PageEnd::ScopeError(ScopeError::UnknownDeck)),
        Ok(HistoryResult::ContextChanged) => Err(PageEnd::ContextChanged),
        Ok(HistoryResult::Cancelled) => Err(PageEnd::Cancelled),
        Err(error) => {
            tracing::warn!(%error, "the page's history parts failed");
            Ok(Err(PartFailure::Failed(error.to_string())))
        }
    }
}

fn history_outcome(part: Part, read: &HistoryRead) -> PartOutcome {
    match read {
        Ok(parts) => {
            let (card_states, started) = parts.as_ref();
            PartOutcome::Data(match part {
                Part::Started => PartValue::Started(started.clone()),
                _ => PartValue::CardStates(card_states.clone()),
            })
        }
        Err(failure) => PartOutcome::Failed(failure.clone()),
    }
}
