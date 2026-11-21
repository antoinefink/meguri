//! Browse search sessions over FFI (U264, `browser/result-sets` H2 to H12,
//! H15): a session object per Browse instance, its events on a listener of
//! its own (not the core event channel), and the calls through its handles:
//! row pages, Question and Answer cells, reverse, resort, invalidation by
//! change sets, the Cards/Notes toggle, and selection by id and index.
//!
//! The engine's session (`meguri_engine::browse::search`) does the work;
//! this module converts values and errors. A search never writes. Handles
//! cross as opaque numbers bound to their session (H15), ids as 64-bit
//! integers, and the ranges of search errors in UTF-16 code units of the
//! text the request read (H15, `search/errors` E5).

use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_engine::browse::search::{
    self as engine, BrowseError, HandleId, HandleSelection, MapFrom, QaWhich, RequestSeq, SearchEvent, SearchListener,
    SearchMode, SearchSession, SortDirection, SortSpec,
};
use meguri_engine::browse::{self as browse, Answer, Cell, Percent, RowFailure, Tint};
use meguri_model::changes as model;

use crate::cancel::CancelToken;
use crate::changes::ChangeSet;
use crate::excerpts::{ExcerptQuestion, blank};
use crate::render::{RenderBlank, RenderErrorKind, RenderSide};
use crate::{CoreError, callback, runtime};

/// A search mode (`search/query-model` Q4): ids are card ids or note ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum BrowseMode {
    Cards,
    Notes,
}

impl From<BrowseMode> for SearchMode {
    fn from(mode: BrowseMode) -> SearchMode {
        match mode {
            BrowseMode::Cards => SearchMode::Cards,
            BrowseMode::Notes => SearchMode::Notes,
        }
    }
}

impl From<SearchMode> for BrowseMode {
    fn from(mode: SearchMode) -> BrowseMode {
        match mode {
            SearchMode::Cards => BrowseMode::Cards,
            SearchMode::Notes => BrowseMode::Notes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum BrowseSortDirection {
    Ascending,
    Descending,
}

/// A sort (`browser/sorting` S1, S8): one of the column keys or a field
/// column `fld:<name>` (`browser/workspace` V3), and a direction. A key
/// the mode cannot sort by fails the request with `unsortable_column`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct BrowseSort {
    pub key: String,
    pub direction: BrowseSortDirection,
}

impl From<BrowseSort> for SortSpec {
    fn from(sort: BrowseSort) -> SortSpec {
        SortSpec::new(
            sort.key,
            match sort.direction {
                BrowseSortDirection::Ascending => SortDirection::Ascending,
                BrowseSortDirection::Descending => SortDirection::Descending,
            },
        )
    }
}

/// Indexes `start` to `end - 1` of a handle (H7, H12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct BrowseIndexRange {
    pub start: u64,
    pub end: u64,
}

impl From<BrowseIndexRange> for Range<u64> {
    fn from(range: BrowseIndexRange) -> Range<u64> {
        range.start..range.end
    }
}

impl From<Range<u64>> for BrowseIndexRange {
    fn from(range: Range<u64>) -> BrowseIndexRange {
        BrowseIndexRange {
            start: range.start,
            end: range.end,
        }
    }
}

/// H14's refusals of a call that have no `CoreError` of their own. An
/// argument the core rejects (a page of more than 1000 indexes, an unknown
/// column key) is `CoreError::InvalidArgument`; `StaleGeneration`, the
/// engine's refusals and storage errors are their own `CoreError`s. Search
/// errors come only in a request's `Failed` event (`BrowseRequestError`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseFailure {
    /// `browser/sorting` S1.
    UnsortableColumn,
    /// The call named another revision than the handle's (H6).
    StaleRevision { current: u64 },
    /// A released handle (H6), a superseded one, or one of another session.
    UnknownHandle,
    /// An index outside the final prefix (H7).
    OutOfRange,
    /// `reverse`, `resort` and `toggle` on a handle in progress (H8, H11).
    NotComplete,
}

/// A search error as `Failed` gives it (H4, `search/errors` E2, E5, E7).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseSearchError {
    /// E1's kind name (`invalid_state`, `search_failed`).
    pub kind: String,
    /// E2's texts, as the search holds them.
    pub provided: Option<String>,
    pub context: Option<String>,
    /// E7's message.
    pub message: String,
    /// E5's range, in UTF-16 code units of the text the request read.
    pub start: u64,
    pub end: u64,
}

/// The UTF-16 length of the first `chars` characters of `text`; offsets
/// past its end count one unit each.
fn utf16_offset(text: &str, chars: usize) -> u64 {
    let mut units = 0;
    let mut seen = 0;
    for c in text.chars().take(chars) {
        units += c.len_utf16() as u64;
        seen += 1;
    }
    units + (chars - seen) as u64
}

fn search_error(error: &meguri_search::SearchError, text: &str) -> BrowseSearchError {
    let range = error.range();
    BrowseSearchError {
        kind: error.code().to_owned(),
        provided: error.kind().provided().map(str::to_owned),
        context: error.kind().context().map(str::to_owned),
        message: error.message(),
        start: utf16_offset(text, range.start),
        end: utf16_offset(text, range.end),
    }
}

impl From<BrowseError> for CoreError {
    fn from(error: BrowseError) -> CoreError {
        let failure = match error {
            // Calls never raise one; a request's `Failed` gives it whole.
            BrowseError::Search(error) => {
                return CoreError::InvalidArgument {
                    message: error.message(),
                };
            }
            BrowseError::UnsortableColumn => BrowseFailure::UnsortableColumn,
            BrowseError::StaleRevision { current } => BrowseFailure::StaleRevision { current },
            BrowseError::UnknownHandle => BrowseFailure::UnknownHandle,
            BrowseError::OutOfRange => BrowseFailure::OutOfRange,
            BrowseError::NotComplete => BrowseFailure::NotComplete,
            BrowseError::InvalidArgument { message } => return CoreError::InvalidArgument { message },
            BrowseError::Engine(error) => return error.into(),
        };
        CoreError::Browse { failure }
    }
}

/// Why a request failed (H4): a search error, or any other error of H14.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseRequestError {
    Search { error: BrowseSearchError },
    Core { error: CoreError },
}

/// H4's `Failed`; `text` is what a search error's range counts in.
fn request_error(error: BrowseError, text: Option<&str>) -> BrowseRequestError {
    match error {
        BrowseError::Search(error) => BrowseRequestError::Search {
            error: search_error(&error, text.unwrap_or_default()),
        },
        error => BrowseRequestError::Core { error: error.into() },
    }
}

/// A session's events (H4). Each carries its request's seq; exactly one
/// terminal event (`Complete`, `Failed`, `Cancelled`) ends every request,
/// and every event of a request comes before any of a later one.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum BrowseSearchEvent {
    /// The first page is final: at most 200 ids, in revision 0.
    FirstPage { seq: u64, handle: u64, ids: Vec<i64> },
    /// Streamed requests only, at most one per 100 ms.
    Progress {
        seq: u64,
        matched: u64,
        scanned: u64,
        revision: u64,
    },
    Complete {
        seq: u64,
        handle: u64,
        count: u64,
        revision: u64,
    },
    Failed { seq: u64, error: BrowseRequestError },
    Cancelled { seq: u64 },
}

impl From<SearchEvent> for BrowseSearchEvent {
    fn from(event: SearchEvent) -> BrowseSearchEvent {
        match event {
            SearchEvent::FirstPage { seq, handle, ids } => BrowseSearchEvent::FirstPage {
                seq: seq.0,
                handle: handle.0,
                ids,
            },
            SearchEvent::Progress {
                seq,
                matched,
                scanned,
                revision,
            } => BrowseSearchEvent::Progress {
                seq: seq.0,
                matched,
                scanned,
                revision,
            },
            SearchEvent::Complete {
                seq,
                handle,
                count,
                revision,
            } => BrowseSearchEvent::Complete {
                seq: seq.0,
                handle: handle.0,
                count,
                revision,
            },
            SearchEvent::Failed { seq, error, text } => BrowseSearchEvent::Failed {
                seq: seq.0,
                error: request_error(error, text.as_deref()),
            },
            SearchEvent::Cancelled { seq } => BrowseSearchEvent::Cancelled { seq: seq.0 },
        }
    }
}

/// Implemented in Swift by CoreBridge's session relay, which only yields
/// each event into an `AsyncStream` (KTD9). Called on a core thread that
/// holds no lock and no reader, never the writer thread.
#[uniffi::export(foreign)]
pub trait BrowseSearchListener: Send + Sync {
    fn on_event(&self, event: BrowseSearchEvent);
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The engine listener: converts each event and hands it to Swift. The
/// engine calls it holding no lock (H4), and it takes none.
struct Relay(Arc<dyn BrowseSearchListener>);

impl SearchListener for Relay {
    fn event(&self, event: SearchEvent) {
        let event = BrowseSearchEvent::from(event);
        callback::invoke("BrowseSearchListener::on_event", || self.0.on_event(event));
    }
}

/// H6's state of a handle in its current revision.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseHandleState {
    pub mode: BrowseMode,
    pub revision: u64,
    /// The final prefix's length while in progress, the whole count after.
    pub count: u64,
    pub complete: bool,
    pub sort: BrowseSort,
}

/// What `invalidated_rows` says (H10).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseInvalidation {
    /// Indexes in the revision the call named, ascending.
    pub removed: Vec<u64>,
    /// Every row: a note type or a deck changed, or the whole collection.
    pub refetch_all: bool,
    /// Indexes of the new revision, ascending; empty when `refetch_all`.
    pub refetch: Vec<u64>,
    pub revision: u64,
    pub complete: bool,
    /// The final prefix's length while in progress, the whole count after.
    pub count: u64,
}

impl From<engine::Invalidation> for BrowseInvalidation {
    fn from(value: engine::Invalidation) -> BrowseInvalidation {
        let (refetch_all, refetch) = match value.refetch {
            engine::Refetch::All => (true, Vec::new()),
            engine::Refetch::Rows(rows) => (false, rows),
        };
        BrowseInvalidation {
            removed: value.removed,
            refetch_all,
            refetch,
            revision: value.revision,
            complete: value.complete,
            count: value.count,
        }
    }
}

/// A `Changes` event's change set with the event's seq (H10).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SequencedChangeSet {
    pub seq: u64,
    pub changes: ChangeSet,
}

/// C4's row state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseRowState {
    /// 0 none, 1 to 7.
    pub flag: u8,
    pub marked: bool,
    pub leech: bool,
    pub suspended: bool,
    pub buried: bool,
    pub filtered: bool,
}

/// C5's row background.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseTint {
    None,
    /// 1 to 7.
    Flag { flag: u8 },
    Marked,
    Suspended,
    Buried,
}

/// C6: the browser font of the row's template; each member absent alone.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseFont {
    pub name: Option<String>,
    pub size: Option<u32>,
}

/// The Answer cell (C10): a blank card's has no text.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseAnswer {
    Text { text: String },
    MissingCloze { kind: RenderBlank },
    TemplateError { kind: RenderErrorKind, side: RenderSide },
}

impl From<Answer> for BrowseAnswer {
    fn from(answer: Answer) -> BrowseAnswer {
        match answer {
            Answer::Text(text) => BrowseAnswer::Text { text },
            Answer::MissingCloze { kind } => BrowseAnswer::MissingCloze { kind: blank(kind) },
            Answer::TemplateError { kind, side } => BrowseAnswer::TemplateError {
                kind: kind.into(),
                side: side.into(),
            },
        }
    }
}

/// One cell (C9). Due values carry C7.5's wrapped flag.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum BrowseCell {
    Text {
        text: String,
    },
    Count {
        count: u64,
    },
    /// Notes-mode Deck spread over this many decks (at least 2).
    DeckCount {
        decks: u64,
    },
    /// Ease.
    PercentInteger {
        percent: i64,
    },
    /// Difficulty and Retrievability, shown rounded.
    PercentReal {
        percent: f32,
    },
    Position {
        position: i64,
    },
    /// Epoch seconds, shown as a local date.
    Instant {
        secs: i64,
        wrapped: bool,
    },
    /// Epoch seconds, shown as a local date and time to the minute.
    DateTime {
        secs: i64,
    },
    NewPosition {
        position: i64,
        wrapped: bool,
    },
    Filtered {
        wrapped: bool,
    },
    NewMarker,
    LearningMarker,
    /// Seconds.
    TimeSpan {
        seconds: f32,
    },
    Empty,
    Question {
        question: ExcerptQuestion,
    },
    Answer {
        answer: BrowseAnswer,
    },
    /// Question or Answer in a row page: `qa_cells` gives it (H7).
    Deferred,
    /// A field column on a row whose note type has no such field (V3).
    NoField,
    /// A field column on a field holding media (V3).
    Media {
        sound: Option<String>,
        image: Option<String>,
    },
}

impl From<Cell> for BrowseCell {
    fn from(cell: Cell) -> BrowseCell {
        match cell {
            Cell::Text(text) => BrowseCell::Text { text },
            Cell::Count(count) => BrowseCell::Count { count },
            Cell::DeckCount(decks) => BrowseCell::DeckCount { decks },
            Cell::Percent(Percent::Integer(percent)) => BrowseCell::PercentInteger { percent },
            Cell::Percent(Percent::Real(percent)) => BrowseCell::PercentReal { percent },
            Cell::Position(position) => BrowseCell::Position { position },
            Cell::Instant { secs, wrapped } => BrowseCell::Instant { secs, wrapped },
            Cell::DateTime(secs) => BrowseCell::DateTime { secs },
            Cell::NewPosition { position, wrapped } => BrowseCell::NewPosition { position, wrapped },
            Cell::Filtered { wrapped } => BrowseCell::Filtered { wrapped },
            Cell::NewMarker => BrowseCell::NewMarker,
            Cell::LearningMarker => BrowseCell::LearningMarker,
            Cell::TimeSpan(seconds) => BrowseCell::TimeSpan { seconds },
            Cell::Empty => BrowseCell::Empty,
            Cell::Question(question) => BrowseCell::Question {
                question: question.into(),
            },
            Cell::Answer(answer) => BrowseCell::Answer { answer: answer.into() },
            Cell::Deferred => BrowseCell::Deferred,
            Cell::NoField => BrowseCell::NoField,
            Cell::Media { sound, image } => BrowseCell::Media { sound, image },
        }
    }
}

/// A row record (C3).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrowseRow {
    /// The card id in cards mode, the note id in notes mode.
    pub id: i64,
    pub note_id: i64,
    pub state: BrowseRowState,
    pub tint: BrowseTint,
    pub font: BrowseFont,
    /// C6: the Sort Field cell is laid out right to left.
    pub sort_field_rtl: bool,
    /// One per requested column, in request order.
    pub cells: Vec<BrowseCell>,
}

/// One index of a page: its row, or its row error (C3.1).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum BrowseRowResult {
    Row {
        row: BrowseRow,
    },
    /// The row's own card or note no longer exists.
    Deleted,
    /// A record the row needs is missing, or the note has no card.
    Damaged,
    /// A record exists but does not decode: the value that failed.
    Unreadable {
        table: String,
        column: String,
        row: String,
    },
}

impl From<browse::RowResult> for BrowseRowResult {
    fn from(result: browse::RowResult) -> BrowseRowResult {
        let row = match result {
            Ok(row) => row,
            Err(RowFailure::Deleted) => return BrowseRowResult::Deleted,
            Err(RowFailure::Damaged) => return BrowseRowResult::Damaged,
            Err(RowFailure::Unreadable { table, column, row }) => {
                return BrowseRowResult::Unreadable {
                    table: table.to_owned(),
                    column: column.to_owned(),
                    row,
                };
            }
        };
        BrowseRowResult::Row {
            row: BrowseRow {
                id: row.id,
                note_id: row.note_id.0,
                state: BrowseRowState {
                    flag: row.state.flag,
                    marked: row.state.marked,
                    leech: row.state.leech,
                    suspended: row.state.suspended,
                    buried: row.state.buried,
                    filtered: row.state.filtered,
                },
                tint: match row.tint {
                    Tint::None => BrowseTint::None,
                    Tint::Flag(flag) => BrowseTint::Flag { flag },
                    Tint::Marked => BrowseTint::Marked,
                    Tint::Suspended => BrowseTint::Suspended,
                    Tint::Buried => BrowseTint::Buried,
                },
                font: BrowseFont {
                    name: row.font.name,
                    size: row.font.size,
                },
                sort_field_rtl: row.sort_field_rtl,
                cells: row.cells.into_iter().map(Into::into).collect(),
            },
        }
    }
}

/// Which cells `qa_cells` computes (H7, C10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseQaWhich {
    Question,
    Answer,
    Both,
}

/// One index of `qa_cells`: the cells asked for, or the row error in their
/// place (H7).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum BrowseQaResult {
    Cells {
        question: Option<ExcerptQuestion>,
        answer: Option<BrowseAnswer>,
    },
    Deleted,
    Damaged,
    Unreadable {
        table: String,
        column: String,
        row: String,
    },
}

impl From<Result<engine::QaCells, RowFailure>> for BrowseQaResult {
    fn from(result: Result<engine::QaCells, RowFailure>) -> BrowseQaResult {
        match result {
            Ok(cells) => BrowseQaResult::Cells {
                question: cells.question.map(Into::into),
                answer: cells.answer.map(Into::into),
            },
            Err(RowFailure::Deleted) => BrowseQaResult::Deleted,
            Err(RowFailure::Damaged) => BrowseQaResult::Damaged,
            Err(RowFailure::Unreadable { table, column, row }) => BrowseQaResult::Unreadable {
                table: table.to_owned(),
                column: column.to_owned(),
                row,
            },
        }
    }
}

/// Where `map_selection` maps from (H12).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseMapFrom {
    /// A handle of the session, at a revision.
    Handle { handle: u64, revision: u64 },
    /// A released handle's mode: the selection and the current row are ids
    /// the caller holds (rows it fetched).
    Mode { mode: BrowseMode },
}

/// What is selected in `from` (H12).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseSelected {
    /// Index ranges of a `Handle` source.
    Ranges { ranges: Vec<BrowseIndexRange> },
    /// Ids of the source's mode.
    Ids { ids: Vec<i64> },
}

/// What `map_selection` gives: ascending merged ranges of `to` and the
/// current index (H12; the no-current rule is the UI's).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseMappedSelection {
    pub selected: Vec<BrowseIndexRange>,
    pub current: Option<u64>,
}

fn ranges(ranges: Vec<BrowseIndexRange>) -> Vec<Range<u64>> {
    ranges.into_iter().map(Into::into).collect()
}

/// A selection of a handle's indexes (H12), kept as ranges and resolved to
/// ids only when a command runs, so a bulk action can resolve a large
/// selection inside the core. It does not keep its session open: once the
/// session is closed or the handle released, resolving fails with
/// `UnknownHandle`.
#[derive(Debug, uniffi::Object)]
pub struct BrowseSelection {
    selection: HandleSelection,
    counts: CountsSlot,
}

/// A session's newest selection counts job: a newer one cancels it
/// (`browse_query`).
pub(crate) type CountsSlot = Arc<crate::browse_query::Newest>;

impl BrowseSelection {
    /// The engine's selection, for a bulk action's target (U86, U87).
    pub fn engine_selection(&self) -> &HandleSelection {
        &self.selection
    }

    /// Its session's slot for the newest selection counts job.
    pub(crate) fn counts_slot(&self) -> &CountsSlot {
        &self.counts
    }
}

#[uniffi::export]
impl BrowseSelection {
    /// H12's `resolve_selection`, from one read transaction on the query
    /// reader.
    pub fn resolve(&self, target: BrowseMode) -> Result<Vec<i64>, CoreError> {
        callback::enter("BrowseSelection::resolve");
        Ok(self.selection.resolve(target.into())?)
    }
}

/// One Browse session (H1, H2). Dropping it, or `close`, cancels its
/// in-flight request and releases its handles.
#[derive(uniffi::Object)]
pub struct BrowseSearchSession {
    /// `None` once closed. Calls clone it and release the lock before they
    /// reach the engine.
    session: Mutex<Option<Arc<SearchSession>>>,
    /// Held by `close` while it shuts the session down.
    closing: Mutex<()>,
    counts: CountsSlot,
}

impl std::fmt::Debug for BrowseSearchSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowseSearchSession").finish_non_exhaustive()
    }
}

/// H2: a session whose events go to `listener`. It needs no open
/// collection; its requests fail with the engine's refusal while none is.
#[uniffi::export]
pub fn open_search_session(listener: Arc<dyn BrowseSearchListener>) -> Result<Arc<BrowseSearchSession>, CoreError> {
    callback::enter("open_search_session");
    let session = runtime::engine()?.open_search_session(Arc::new(Relay(listener)));
    Ok(Arc::new(BrowseSearchSession {
        session: Mutex::new(Some(Arc::new(session))),
        closing: Mutex::new(()),
        counts: CountsSlot::default(),
    }))
}

fn engine_token(cancel: &Option<Arc<CancelToken>>) -> Option<&meguri_engine::CancelToken> {
    cancel.as_ref().map(|token| &**token.engine_token())
}

impl BrowseSearchSession {
    fn session(&self) -> Result<Arc<SearchSession>, CoreError> {
        lock(&self.session)
            .clone()
            .ok_or(CoreError::Browse {
                failure: BrowseFailure::UnknownHandle,
            })
    }
}

#[uniffi::export]
impl BrowseSearchSession {
    /// H3: starts a search of `text` as given (the search-field rule is the
    /// caller's), in `mode`, sorted by `sort`; returns its seq at once. It
    /// supersedes the session's earlier requests; `cancel` cancels it too.
    pub fn start_search(
        &self,
        text: String,
        mode: BrowseMode,
        sort: BrowseSort,
        cancel: Option<Arc<CancelToken>>,
    ) -> Result<u64, CoreError> {
        callback::enter("BrowseSearchSession::start_search");
        let session = self.session()?;
        Ok(session.start_search(&text, mode.into(), sort.into(), engine_token(&cancel)).0)
    }

    /// H8: sorts a complete handle's ids by `sort` in a new snapshot without
    /// running the predicate; returns the new request's seq.
    /// `NotComplete` on a handle in progress, before anything starts.
    pub fn resort(&self, handle: u64, sort: BrowseSort, cancel: Option<Arc<CancelToken>>) -> Result<u64, CoreError> {
        callback::enter("BrowseSearchSession::resort");
        let RequestSeq(seq) = self.session()?.resort(HandleId(handle), sort.into(), engine_token(&cancel))?;
        Ok(seq)
    }

    /// H11: the handle's text in the other mode, sorted by `sort` (that
    /// mode's sort), without a predicate run when it can be derived.
    /// `NotComplete` on a handle in progress, before anything starts.
    pub fn toggle(&self, handle: u64, sort: BrowseSort, cancel: Option<Arc<CancelToken>>) -> Result<u64, CoreError> {
        callback::enter("BrowseSearchSession::toggle");
        let RequestSeq(seq) = self.session()?.toggle(HandleId(handle), sort.into(), engine_token(&cancel))?;
        Ok(seq)
    }

    /// H8: reverses a complete handle in memory; returns its new revision,
    /// in which index i is count − 1 − i.
    pub fn reverse(&self, handle: u64, revision: u64) -> Result<u64, CoreError> {
        callback::enter("BrowseSearchSession::reverse");
        Ok(self.session()?.reverse(HandleId(handle), revision)?)
    }

    /// H10: applies every `Changes` event received since the request
    /// started, in seq order, and says which rows to refetch.
    pub fn invalidated_rows(
        &self,
        handle: u64,
        revision: u64,
        change_sets: Vec<SequencedChangeSet>,
    ) -> Result<BrowseInvalidation, CoreError> {
        callback::enter("BrowseSearchSession::invalidated_rows");
        let session = self.session()?;
        let owned: Vec<(meguri_engine::Seq, model::ChangeSet)> = change_sets
            .iter()
            .map(|set| (meguri_engine::Seq(set.seq), model::ChangeSet::from(&set.changes)))
            .collect();
        let borrowed: Vec<(meguri_engine::Seq, &model::ChangeSet)> =
            owned.iter().map(|(seq, changes)| (*seq, changes)).collect();
        Ok(session.invalidated_rows(HandleId(handle), revision, &borrowed)?.into())
    }

    /// H7: the row records of the indexes in `range` (at most 1000), read
    /// from the collection as it is now; Question and Answer are
    /// `Deferred`.
    pub fn rows(
        &self,
        handle: u64,
        revision: u64,
        range: BrowseIndexRange,
        columns: Vec<String>,
    ) -> Result<Vec<BrowseRowResult>, CoreError> {
        callback::enter("BrowseSearchSession::rows");
        let columns: Vec<&str> = columns.iter().map(String::as_str).collect();
        Ok(self
            .session()?
            .rows(HandleId(handle), revision, range.into(), &columns)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// H7: the Question cells, the Answer cells or both for the indexes in
    /// `range` (at most 1000), cancellable between rows.
    pub fn qa_cells(
        &self,
        handle: u64,
        revision: u64,
        range: BrowseIndexRange,
        which: BrowseQaWhich,
        cancel: Option<Arc<CancelToken>>,
    ) -> Result<Vec<BrowseQaResult>, CoreError> {
        callback::enter("BrowseSearchSession::qa_cells");
        let which = match which {
            BrowseQaWhich::Question => QaWhich::Question,
            BrowseQaWhich::Answer => QaWhich::Answer,
            BrowseQaWhich::Both => QaWhich::Both,
        };
        Ok(self
            .session()?
            .qa_cells(HandleId(handle), revision, range.into(), which, engine_token(&cancel))?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// H12: the index of `id` (a card id when `kind` is cards, a note id
    /// when it is notes) in the handle's final prefix, or none.
    pub fn index_of(&self, handle: u64, revision: u64, kind: BrowseMode, id: i64) -> Result<Option<u64>, CoreError> {
        callback::enter("BrowseSearchSession::index_of");
        Ok(self.session()?.index_of(HandleId(handle), revision, kind.into(), id)?)
    }

    /// H12: maps a selection and the current row from `from` to `to`, over
    /// the final prefix of `to` at `to_revision`.
    pub fn map_selection(
        &self,
        from: BrowseMapFrom,
        selected: BrowseSelected,
        current: Option<i64>,
        to: u64,
        to_revision: u64,
    ) -> Result<BrowseMappedSelection, CoreError> {
        callback::enter("BrowseSearchSession::map_selection");
        let from = match from {
            BrowseMapFrom::Handle { handle, revision } => MapFrom::Handle {
                handle: HandleId(handle),
                revision,
            },
            BrowseMapFrom::Mode { mode } => MapFrom::Mode(mode.into()),
        };
        let selection = match selected {
            BrowseSelected::Ranges { ranges: list } => engine::Selection::Ranges(ranges(list)),
            BrowseSelected::Ids { ids } => engine::Selection::Ids(ids),
        };
        let mapped = self
            .session()?
            .map_selection(from, selection, current, HandleId(to), to_revision)?;
        Ok(BrowseMappedSelection {
            selected: mapped.selected.into_iter().map(Into::into).collect(),
            current: mapped.current,
        })
    }

    /// H12: the ids an action works on, from index ranges of the handle, in
    /// one read transaction on the query reader (target notes on a cards
    /// handle: distinct notes in the order of their first selected card;
    /// target cards on a notes handle: every card of each note, ascending).
    pub fn resolve_selection(
        &self,
        handle: u64,
        revision: u64,
        ranges: Vec<BrowseIndexRange>,
        target: BrowseMode,
    ) -> Result<Vec<i64>, CoreError> {
        callback::enter("BrowseSearchSession::resolve_selection");
        let list = self::ranges(ranges);
        Ok(self
            .session()?
            .resolve_selection(HandleId(handle), revision, &list, target.into())?)
    }

    /// H12: the selection of `ranges` at `revision`, kept as ranges until a
    /// command resolves it. Nothing is checked until then.
    pub fn selection(
        &self,
        handle: u64,
        revision: u64,
        ranges: Vec<BrowseIndexRange>,
    ) -> Result<Arc<BrowseSelection>, CoreError> {
        callback::enter("BrowseSearchSession::selection");
        Ok(Arc::new(BrowseSelection {
            selection: self
                .session()?
                .selection(HandleId(handle), revision, self::ranges(ranges)),
            counts: Arc::clone(&self.counts),
        }))
    }

    /// H6: the handle's mode, revision, count, state and sort.
    pub fn handle_state(&self, handle: u64) -> Result<BrowseHandleState, CoreError> {
        callback::enter("BrowseSearchSession::handle_state");
        let state = self.session()?.handle_state(HandleId(handle))?;
        Ok(BrowseHandleState {
            mode: state.mode.into(),
            revision: state.revision,
            count: state.count,
            complete: state.complete,
            sort: BrowseSort {
                key: state.sort_key,
                direction: match state.direction {
                    SortDirection::Ascending => BrowseSortDirection::Ascending,
                    SortDirection::Descending => BrowseSortDirection::Descending,
                },
            },
        })
    }

    /// H2: cancels the in-flight request and releases every handle. Later
    /// calls fail with `UnknownHandle`. Idempotent.
    pub fn close(&self) -> Result<(), CoreError> {
        callback::enter("BrowseSearchSession::close");
        // Concurrent closes wait for the one shutting down; other calls
        // never take this lock.
        let _closing = lock(&self.closing);
        // The session lock is released before the engine work.
        let session = lock(&self.session).take();
        // Shut down now, not when the last call in flight lets go of its
        // clone: a request one of them makes meanwhile ends `Cancelled`.
        if let Some(session) = session {
            session.shutdown();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_ranges_count_utf16_units_of_the_request_text() {
        // `𝄞` is one character and two UTF-16 units; `é` one of each.
        assert_eq!(utf16_offset("a𝄞é b", 0), 0);
        assert_eq!(utf16_offset("a𝄞é b", 2), 3);
        assert_eq!(utf16_offset("a𝄞é b", 5), 6);
        // Past the end, each character counts one unit.
        assert_eq!(utf16_offset("𝄞", 3), 4);
        let text = "𝄞 is:bogus";
        let error = meguri_search::SearchError::new(
            meguri_search::ErrorKind::InvalidState {
                provided: "bogus".into(),
            },
            2..10,
        );
        let converted = search_error(&error, text);
        assert_eq!((converted.start, converted.end), (3, 11));
        assert_eq!(converted.kind, "invalid_state");
        assert_eq!(converted.provided.as_deref(), Some("bogus"));
        assert_eq!(converted.context, None);
        assert_eq!(converted.message, error.message());
    }

    #[test]
    fn browse_errors_keep_their_values_across_the_boundary() {
        let browse = |failure| CoreError::Browse { failure };
        assert_eq!(CoreError::from(BrowseError::UnsortableColumn), browse(BrowseFailure::UnsortableColumn));
        assert_eq!(
            CoreError::from(BrowseError::StaleRevision { current: 7 }),
            browse(BrowseFailure::StaleRevision { current: 7 })
        );
        assert_eq!(CoreError::from(BrowseError::UnknownHandle), browse(BrowseFailure::UnknownHandle));
        assert_eq!(CoreError::from(BrowseError::OutOfRange), browse(BrowseFailure::OutOfRange));
        assert_eq!(CoreError::from(BrowseError::NotComplete), browse(BrowseFailure::NotComplete));
        assert_eq!(
            CoreError::from(BrowseError::InvalidArgument {
                message: "too many".into()
            }),
            CoreError::InvalidArgument {
                message: "too many".into()
            }
        );
        assert_eq!(
            CoreError::from(BrowseError::Engine(meguri_engine::EngineError::StaleGeneration {
                planned: meguri_engine::Generation(2),
                current: meguri_engine::Generation(3),
            })),
            CoreError::StaleGeneration { planned: 2, current: 3 }
        );
    }

    #[test]
    fn cells_and_rows_keep_every_value() {
        use meguri_engine::browse::{Font, Question, Row, RowState};
        let row = Row {
            id: -4,
            note_id: meguri_model::ids::NoteId(i64::MAX),
            state: RowState {
                flag: 7,
                marked: true,
                leech: false,
                suspended: true,
                buried: false,
                filtered: true,
            },
            tint: Tint::Flag(7),
            font: Font {
                name: Some("Plain".into()),
                size: None,
            },
            sort_field_rtl: true,
            cells: vec![
                Cell::Percent(Percent::Real(41.5)),
                Cell::Instant { secs: -1, wrapped: true },
                Cell::Question(Question::Text(String::new())),
                Cell::Answer(Answer::MissingCloze {
                    kind: meguri_engine::rendering::BlankKind::EmptyFront,
                }),
                Cell::Deferred,
                Cell::Media {
                    sound: None,
                    image: Some("pic.png".into()),
                },
            ],
        };
        assert_eq!(
            BrowseRowResult::from(Ok(row)),
            BrowseRowResult::Row {
                row: BrowseRow {
                    id: -4,
                    note_id: i64::MAX,
                    state: BrowseRowState {
                        flag: 7,
                        marked: true,
                        leech: false,
                        suspended: true,
                        buried: false,
                        filtered: true,
                    },
                    tint: BrowseTint::Flag { flag: 7 },
                    font: BrowseFont {
                        name: Some("Plain".into()),
                        size: None,
                    },
                    sort_field_rtl: true,
                    cells: vec![
                        BrowseCell::PercentReal { percent: 41.5 },
                        BrowseCell::Instant { secs: -1, wrapped: true },
                        BrowseCell::Question {
                            question: ExcerptQuestion::Text { text: String::new() },
                        },
                        BrowseCell::Answer {
                            answer: BrowseAnswer::MissingCloze {
                                kind: RenderBlank::EmptyFront,
                            },
                        },
                        BrowseCell::Deferred,
                        BrowseCell::Media {
                            sound: None,
                            image: Some("pic.png".into()),
                        },
                    ],
                },
            }
        );
        assert_eq!(BrowseRowResult::from(Err(RowFailure::Deleted)), BrowseRowResult::Deleted);
        assert_eq!(
            BrowseQaResult::from(Err(RowFailure::Unreadable {
                table: "notes",
                column: "flds",
                row: "9".into(),
            })),
            BrowseQaResult::Unreadable {
                table: "notes".into(),
                column: "flds".into(),
                row: "9".into(),
            }
        );
    }
}
