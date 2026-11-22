//! The Statistics page's request over FFI (U103, U292): `stats/history-reads`
//! H15, H16 and H18, `stats/graphs-common` G6, G8, G9.5 and G10.
//!
//! - `start_stats_page` starts one request as a job (KTD9) and returns its
//!   handle at once. Its messages go to the request's own listener, in the
//!   order the core sends them: the header once, before the first part;
//!   each part by tier as it is ready (H15); progress for the parts that
//!   wait; and last, always, the request's `End`. The job's `JobFinished`
//!   follows the `End`. A job the registry never runs (cancelled before a
//!   worker took it, or admitted while the registry is paused) sends no
//!   message: its `JobFinished` is then the request's end, which
//!   CoreBridge turns into the stream's end.
//! - Cancellation is the job's token (H16). The core checks it before each
//!   message, so a cancel from another thread at that instant can let one
//!   message through: CoreBridge drops every message of a request it has
//!   cancelled.
//! - A part's value crosses as G6's bytes ([`encode`]): the Anki records as
//!   their own `encode` writes them, and the page's own parts in the member
//!   orders written on [`encode`]. The part names the decoder.
//! - Read only (H17, G11): nothing here writes the collection.

use std::sync::Arc;

use meguri_model::ids::DeckId;
use meguri_stats::graphs::encoding::{Encoder, Member};
use meguri_stats::graphs::{BrowseScope, Columns, Context, History, Preferences, Scope, ScopeError, StatsError, Unreadable};
use meguri_stats::history::{CardStatesOverTime, KnowledgeOverTime, Progress, StartedPart, StateCounts};
use meguri_stats::page::{
    self, ByAnswerPart, OldestDay, PageEnd, PageHeader, PageMessage, PageRequest, Part, PartFailure, PartOutcome,
    PartValue, Period,
};

use crate::browse_search::{BrowseSearchError, search_error};
use crate::jobs::{self, JobHandle};
use crate::{CoreError, callback, runtime};

/// The page's deck filter (`stats/history-reads` H1): All Decks or a deck
/// with its subdecks (G2.1). A search is not offered in this version: the
/// page's own parts then fail with `UnsupportedScope` (H18).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StatsScope {
    AllDecks,
    Deck { id: i64 },
    Search { text: String },
}

impl From<StatsScope> for Scope {
    fn from(scope: StatsScope) -> Scope {
        match scope {
            StatsScope::AllDecks => Scope::Collection,
            StatsScope::Deck { id } => Scope::Deck(DeckId(id)),
            StatsScope::Search { text } => Scope::Search(text),
        }
    }
}

/// The page's period (`stats/statistics-page` S3.2). 1M to 1Y read the same
/// history (`Last12Months`): a change among them re-bins without a request
/// (S10.5); All reads `AllHistory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StatsPeriod {
    OneMonth,
    ThreeMonths,
    SixMonths,
    OneYear,
    All,
}

impl From<StatsPeriod> for Period {
    fn from(period: StatsPeriod) -> Period {
        match period {
            StatsPeriod::OneMonth => Period::OneMonth,
            StatsPeriod::ThreeMonths => Period::ThreeMonths,
            StatsPeriod::SixMonths => Period::SixMonths,
            StatsPeriod::OneYear => Period::OneYear,
            StatsPeriod::All => Period::All,
        }
    }
}

/// G3's history values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StatsHistory {
    Last12Months,
    AllHistory,
}

impl From<History> for StatsHistory {
    fn from(history: History) -> StatsHistory {
        match history {
            History::Last12Months => StatsHistory::Last12Months,
            History::AllHistory => StatsHistory::AllHistory,
        }
    }
}

/// The page's parts (H5), By Answer included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum StatsPart {
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

/// The two part enums, side by side: one line per part.
const PARTS: [(StatsPart, Part); 19] = [
    (StatsPart::CardCounts, Part::CardCounts),
    (StatsPart::FutureDue, Part::FutureDue),
    (StatsPart::Intervals, Part::Intervals),
    (StatsPart::Stability, Part::Stability),
    (StatsPart::Difficulty, Part::Difficulty),
    (StatsPart::Retrievability, Part::Retrievability),
    (StatsPart::StudiedToday, Part::StudiedToday),
    (StatsPart::Reviews, Part::Reviews),
    (StatsPart::ByAnswer, Part::ByAnswer),
    (StatsPart::Retention, Part::Retention),
    (StatsPart::RetentionByDay, Part::RetentionByDay),
    (StatsPart::Hours, Part::Hours),
    (StatsPart::Buttons, Part::Buttons),
    (StatsPart::Added, Part::Added),
    (StatsPart::CardStatesOverTime, Part::CardStatesOverTime),
    (StatsPart::Started, Part::Started),
    (StatsPart::ByDeck, Part::ByDeck),
    (StatsPart::ActivityDays, Part::ActivityDays),
    (StatsPart::KnowledgeOverTime, Part::KnowledgeOverTime),
];

impl From<StatsPart> for Part {
    fn from(part: StatsPart) -> Part {
        PARTS.iter().find(|(ffi, _)| *ffi == part).expect("every part is listed").1
    }
}

impl From<Part> for StatsPart {
    fn from(part: Part) -> StatsPart {
        PARTS.iter().find(|(_, core)| *core == part).expect("every part is listed").0
    }
}

/// One refresh of the page: one request naming its parts (G8.3).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StatsPageRequest {
    pub scope: StatsScope,
    pub period: StatsPeriod,
    /// The parts wanted; one named twice is returned once.
    pub parts: Vec<StatsPart>,
    /// The refresh's id (G8.3), returned in the header.
    pub refresh: u64,
}

/// G1's context, as the request computed every value for it. A later read
/// for the result on screen (`stats_day_cards`, H13) passes it back.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StatsContext {
    /// `N`, epoch seconds.
    pub next_day_at: i64,
    /// The scheduler's day number.
    pub today: u32,
    pub rollover_hour: u8,
    pub now_ms: i64,
    /// Seconds east of UTC at now.
    pub utc_offset_secs: i32,
    /// The device zone's IANA id.
    pub zone: String,
    pub generation: u64,
    pub seq: u64,
}

impl From<&Context> for StatsContext {
    fn from(context: &Context) -> StatsContext {
        StatsContext {
            next_day_at: context.next_day_at,
            today: context.today,
            rollover_hour: context.rollover_hour,
            now_ms: context.now_ms,
            utc_offset_secs: context.utc_offset_secs,
            zone: context.zone.clone(),
            generation: context.generation,
            seq: context.seq,
        }
    }
}

impl From<StatsContext> for Context {
    fn from(context: StatsContext) -> Context {
        Context {
            next_day_at: context.next_day_at,
            today: context.today,
            rollover_hour: context.rollover_hour,
            now_ms: context.now_ms,
            utc_offset_secs: context.utc_offset_secs,
            zone: context.zone,
            generation: context.generation,
            seq: context.seq,
        }
    }
}

/// G7's keys and `fsrs`, as read in the request's snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StatsPreferences {
    /// 0 Sunday, 1 Monday, 5 Friday, 6 Saturday.
    pub first_day_of_week: u8,
    pub card_counts_separate_inactive: bool,
    pub future_due_show_backlog: bool,
    pub fsrs: bool,
}

impl From<Preferences> for StatsPreferences {
    fn from(preferences: Preferences) -> StatsPreferences {
        StatsPreferences {
            first_day_of_week: preferences.first_day_of_week,
            card_counts_separate_inactive: preferences.card_counts_separate_inactive,
            future_due_show_backlog: preferences.future_due_show_backlog,
            fsrs: preferences.fsrs,
        }
    }
}

/// The scope's browse text (`stats/browse-queries` Q2).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatsBrowseScope {
    /// All Decks: an item's query is used alone.
    None,
    Text { text: String },
    /// No item of the scope is clickable.
    Unavailable,
}

impl From<&BrowseScope> for StatsBrowseScope {
    fn from(scope: &BrowseScope) -> StatsBrowseScope {
        match scope {
            BrowseScope::None => StatsBrowseScope::None,
            BrowseScope::Text(text) => StatsBrowseScope::Text { text: text.clone() },
            BrowseScope::Unavailable => StatsBrowseScope::Unavailable,
        }
    }
}

/// An unreadable row (G4): its table, column and row key
/// (`model/row-records` R10).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StatsUnreadable {
    pub table: String,
    pub column: String,
    pub key: i64,
}

impl From<&Unreadable> for StatsUnreadable {
    fn from(bad: &Unreadable) -> StatsUnreadable {
        StatsUnreadable {
            table: bad.table.to_owned(),
            column: bad.column.to_owned(),
            key: bad.key,
        }
    }
}

/// Why one part has no value; the request's other parts still return.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatsPartFailure {
    /// A Search scope for one of the page's own parts (H18).
    UnsupportedScope,
    /// An unreadable row of the scope outside the window, for the parts
    /// that read all history (H15, H18).
    UnreadableRow { row: StatsUnreadable },
    /// The part's unit has not landed.
    NotBuilt,
    /// A crate error that stopped the whole part (H15).
    Failed { message: String },
}

impl From<&PartFailure> for StatsPartFailure {
    fn from(failure: &PartFailure) -> StatsPartFailure {
        match failure {
            PartFailure::UnsupportedScope => StatsPartFailure::UnsupportedScope,
            PartFailure::UnreadableRow(bad) => StatsPartFailure::UnreadableRow { row: bad.into() },
            PartFailure::NotBuilt => StatsPartFailure::NotBuilt,
            PartFailure::Failed(message) => StatsPartFailure::Failed {
                message: message.clone(),
            },
        }
    }
}

/// H9's `oldest_day`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatsOldestDay {
    /// The smallest day (H1, folded) of a row of the scope with `id` ≥ 0.
    Day { day: i32 },
    /// The scope has no such row.
    NoRows,
    /// Not known (H18): the page's All axis starts at the window's first
    /// month.
    Unavailable { failure: StatsPartFailure },
}

impl From<&OldestDay> for StatsOldestDay {
    fn from(day: &OldestDay) -> StatsOldestDay {
        match day {
            OldestDay::Day(day) => StatsOldestDay::Day { day: *day },
            OldestDay::NoRows => StatsOldestDay::NoRows,
            OldestDay::Unavailable(failure) => StatsOldestDay::Unavailable {
                failure: failure.into(),
            },
        }
    }
}

/// What every part of a request is computed for (G8.2, H5's last line).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StatsPageHeader {
    pub refresh: u64,
    /// The history the request read (its period's).
    pub history: StatsHistory,
    pub context: StatsContext,
    pub preferences: StatsPreferences,
    pub browse_scope: StatsBrowseScope,
    pub oldest_day: StatsOldestDay,
}

impl StatsPageHeader {
    /// `history` is the request's: the token keeps its own to itself.
    fn new(header: &PageHeader, history: History) -> StatsPageHeader {
        StatsPageHeader {
            refresh: header.token.refresh(),
            history: history.into(),
            context: (&header.context).into(),
            preferences: header.preferences.into(),
            browse_scope: (&header.browse_scope).into(),
            oldest_day: (&header.oldest_day).into(),
        }
    }
}

/// A part's result: G6's bytes of its value, or why it has none.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatsPartOutcome {
    Data { bytes: Vec<u8> },
    Failed { failure: StatsPartFailure },
}

/// How far a waiting part has got (H15).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StatsProgress {
    /// Between 0 and 1; never goes back.
    Fraction { fraction: f64 },
    Indeterminate,
}

impl From<Progress> for StatsProgress {
    fn from(progress: Progress) -> StatsProgress {
        match progress {
            Progress::Fraction(fraction) => StatsProgress::Fraction { fraction },
            Progress::Indeterminate => StatsProgress::Indeterminate,
        }
    }
}

/// G8.2's scope error.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatsScopeError {
    /// The search's error, its range in UTF-16 code units of the scope's
    /// text (`search/errors` E5).
    Search { error: BrowseSearchError },
    /// A deck id that names no deck.
    UnknownDeck,
}

/// How a request ended (H15): always its last message.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StatsPageEnd {
    /// Every part named has returned or failed.
    Complete,
    /// Before any part: every part fails alike.
    ScopeError { error: StatsScopeError },
    /// An unreadable row of the scope in the window, before any part.
    UnreadableRow { row: StatsUnreadable },
    /// A commit landed while a part waited: the parts not yet returned are
    /// dropped; the ones returned hold for their context. Refresh.
    ContextChanged,
    /// The token fired: no part after it (H16).
    Cancelled,
    /// The request could not run (a reader that cannot open).
    Failed { error: CoreError },
}

/// A message of a running request, in the order it is sent.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StatsPageMessage {
    /// Once, before the first part.
    Header { header: StatsPageHeader },
    Part { part: StatsPart, outcome: StatsPartOutcome },
    Progress { part: StatsPart, progress: StatsProgress },
    /// Last, exactly once.
    End { end: StatsPageEnd },
}

/// Implemented in Swift by CoreBridge's page relay, which only yields each
/// message into an `AsyncStream` (KTD9). Called on the request's job thread,
/// which holds no lock; a reader may be open.
#[uniffi::export(foreign)]
pub trait StatsPageListener: Send + Sync {
    fn on_message(&self, message: StatsPageMessage);
}

/// Starts one page request as a job and returns its handle at once. Its
/// messages go to `listener`, ending with `End`; its token cancels it
/// (H16). Reads only, on the query reader; off the main thread.
#[uniffi::export]
pub fn start_stats_page(
    request: StatsPageRequest,
    listener: Arc<dyn StatsPageListener>,
) -> Result<JobHandle, CoreError> {
    callback::enter("start_stats_page");
    let host = runtime::host()?;
    let collection = host.collection()?;
    jobs::start("stats_page", collection.generation().0, move |job| {
        let request = PageRequest {
            history: Period::from(request.period).history(),
            scope: request.scope.into(),
            parts: request.parts.into_iter().map(Part::from).collect(),
            refresh: request.refresh,
            cancel: Arc::clone(job.cancel.engine_token()),
            columns: Columns::Cached,
        };
        let end = run(&request, listener.as_ref(), |sink| {
            page::request(host.stats(), &collection, host.derived(), &request, sink)
        });
        match end {
            StatsPageEnd::Cancelled => Err(CoreError::Cancelled),
            StatsPageEnd::Failed { error } => Err(error),
            _ => Ok(()),
        }
    })
}

/// Runs `request` through `work`, relaying every message to `listener`, and
/// ends with the `End` it sends. The seam the tests drive with made-up
/// messages.
pub(crate) fn run(
    request: &PageRequest,
    listener: &dyn StatsPageListener,
    work: impl FnOnce(&mut dyn FnMut(PageMessage)) -> Result<PageEnd, StatsError>,
) -> StatsPageEnd {
    let mut header: Option<Arc<PageHeader>> = None;
    let deliver = |message: StatsPageMessage| {
        callback::invoke("StatsPageListener::on_message", || listener.on_message(message));
    };
    let result = work(&mut |message| match message {
        PageMessage::Part(part) => {
            if header.as_ref().is_none_or(|sent| !Arc::ptr_eq(sent, &part.header)) {
                deliver(StatsPageMessage::Header {
                    header: StatsPageHeader::new(&part.header, request.history),
                });
                header = Some(Arc::clone(&part.header));
            }
            deliver(StatsPageMessage::Part {
                part: part.part.into(),
                outcome: match &part.outcome {
                    PartOutcome::Data(value) => StatsPartOutcome::Data { bytes: encode(value) },
                    PartOutcome::Failed(failure) => StatsPartOutcome::Failed {
                        failure: failure.into(),
                    },
                },
            });
        }
        PageMessage::Progress { part, progress } => deliver(StatsPageMessage::Progress {
            part: part.into(),
            progress: progress.into(),
        }),
    });
    let end = match result {
        // A fired token is Cancelled whatever the work returned (H16).
        _ if request.cancel.is_cancelled() => StatsPageEnd::Cancelled,
        Ok(end) => page_end(end, &request.scope),
        Err(error) => StatsPageEnd::Failed {
            error: stats_error(error),
        },
    };
    deliver(StatsPageMessage::End { end: end.clone() });
    end
}

fn page_end(end: PageEnd, scope: &Scope) -> StatsPageEnd {
    match end {
        PageEnd::Complete => StatsPageEnd::Complete,
        PageEnd::ScopeError(error) => StatsPageEnd::ScopeError {
            error: scope_error(&error, scope),
        },
        PageEnd::UnreadableRow(bad) => StatsPageEnd::UnreadableRow { row: (&bad).into() },
        PageEnd::ContextChanged => StatsPageEnd::ContextChanged,
        PageEnd::Cancelled => StatsPageEnd::Cancelled,
    }
}

fn scope_error(error: &ScopeError, scope: &Scope) -> StatsScopeError {
    match error {
        ScopeError::Search(error) => StatsScopeError::Search {
            error: search_error(error, match scope {
                Scope::Search(text) => text,
                Scope::Collection | Scope::Deck(_) => "",
            }),
        },
        ScopeError::UnknownDeck => StatsScopeError::UnknownDeck,
    }
}

pub(crate) fn stats_error(error: StatsError) -> CoreError {
    match error {
        StatsError::Engine(error) => error.into(),
        StatsError::NotBuilt(graph) => CoreError::Unsupported {
            feature: format!("the {} graph", graph.part()),
        },
    }
}

/// G6's bytes of a part's value. The Anki records write themselves (G6's
/// member table, with their H6 to H8 extras after Anki's members). The
/// page's own parts, whose orders G6 does not list yet, are written so
/// (every integer a zigzag varint, a map G6's runs; an optional value a
/// list of none or one item):
///
/// | Part | Members, in order |
/// |---|---|
/// | `by_answer` | one sparse map keyed by day index (V2's, unfolded) with eight integer members: the counts of Again, Hard, Good and Easy, then their summed times in milliseconds in the same order; then `unrated` |
/// | `card_states_over_time` | `start`: five integers, New, Learning, Relearning, Young, Mature; `deltas`: one sparse map keyed by day (H1) with five integer members in the same order; `left_out`; `first_day` as an optional integer |
/// | `started` | one sparse map keyed by day (H1) with one integer member (cards); `first_day` as an optional integer |
/// | `knowledge_over_time` | `samples`: a list, ascending by instant with today's point last, each item `day` (H1), `instant_ms`, `knowledge` as binary64, `studied`; then `failed`; then `left_out` |
pub(crate) fn encode(value: &PartValue) -> Vec<u8> {
    let mut out = Encoder::new();
    match value {
        PartValue::Record(record) => record.encode(&mut out),
        PartValue::ByAnswer(part) => by_answer(&mut out, part),
        PartValue::CardStates(states) => card_states(&mut out, states),
        PartValue::Started(started) => started_part(&mut out, started),
        PartValue::Knowledge(knowledge) => knowledge_part(&mut out, knowledge),
    }
    out.into_bytes()
}

fn by_answer(out: &mut Encoder, part: &ByAnswerPart) {
    let keys: Vec<i64> = part.count_by_answer.keys().map(|day| i64::from(*day)).collect();
    // Each day's times by its own key, so the columns stay as long as the
    // keys and paired with their days whatever the maps hold.
    let times: Vec<[i64; 4]> = part
        .count_by_answer
        .keys()
        .map(|day| part.time_by_answer.get(day).copied().unwrap_or_default())
        .collect();
    let columns: Vec<Vec<i64>> = (0..4)
        .map(|at| part.count_by_answer.values().map(|day| day[at]).collect())
        .chain((0..4).map(|at| times.iter().map(|day| day[at]).collect()))
        .collect();
    let members: Vec<Member<'_>> = columns.iter().map(|values| Member::Integers(values)).collect();
    out.sparse_map(&keys, &members);
    out.integer(part.unrated as i64);
}

fn state_values(counts: &StateCounts) -> [i64; 5] {
    [counts.new, counts.learning, counts.relearning, counts.young, counts.mature]
}

fn optional(out: &mut Encoder, value: Option<i32>) {
    out.list(value.as_slice(), |out, value| out.integer(i64::from(*value)));
}

fn card_states(out: &mut Encoder, states: &CardStatesOverTime) {
    for value in state_values(&states.start) {
        out.integer(value);
    }
    let keys: Vec<i64> = states.deltas.keys().map(|day| i64::from(*day)).collect();
    let columns: Vec<Vec<i64>> = (0..5)
        .map(|at| states.deltas.values().map(|delta| state_values(delta)[at]).collect())
        .collect();
    let members: Vec<Member<'_>> = columns.iter().map(|values| Member::Integers(values)).collect();
    out.sparse_map(&keys, &members);
    out.integer(states.left_out as i64);
    optional(out, states.first_day);
}

fn started_part(out: &mut Encoder, started: &StartedPart) {
    let keys: Vec<i64> = started.days.keys().map(|day| i64::from(*day)).collect();
    let counts: Vec<i64> = started.days.values().map(|count| *count as i64).collect();
    out.sparse_map(&keys, &[Member::Integers(&counts)]);
    optional(out, started.first_day);
}

fn knowledge_part(out: &mut Encoder, knowledge: &KnowledgeOverTime) {
    out.list(&knowledge.samples, |out, sample| {
        out.integer(i64::from(sample.day));
        out.integer(sample.instant_ms);
        out.f64(sample.knowledge);
        out.integer(sample.studied as i64);
    });
    out.integer(knowledge.failed as i64);
    out.integer(knowledge.left_out as i64);
}

/// G9.5 and H14.4: the memory warning, as a job. Drops the memo and releases
/// the column caches and the history component (each persisted first, then
/// unmapped); the next request reloads them. The release waits on the
/// derived writer, which can be busy with a build: no lane waits with it.
#[uniffi::export]
pub fn start_release_stats_caches() -> Result<JobHandle, CoreError> {
    callback::enter("start_release_stats_caches");
    let host = runtime::host()?;
    let generation = host.collection()?.generation().0;
    jobs::start("release_stats_caches", generation, move |_| {
        meguri_stats::graphs::release_stats_caches(host.stats(), host.derived());
        host.derived().release(meguri_stats::history::HISTORY_COMPONENT);
        Ok(())
    })
}

/// G9.2: Home has been idle for 2 s after an open. Releases the column
/// caches' background build; returns at once.
#[uniffi::export]
pub fn stats_home_idle() -> Result<(), CoreError> {
    callback::enter("stats_home_idle");
    meguri_stats::graphs::stats_home_idle(runtime::host()?.derived());
    Ok(())
}

#[cfg(test)]
#[path = "stats_page_tests.rs"]
mod tests;
