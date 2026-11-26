//! Browse search sessions and result sets (`specs/browser/result-sets.md`,
//! U85): a session runs one request at a time on a search worker, hands back
//! a handle of ordered ids, and answers row pages, reverses, resorts,
//! invalidation by change sets and selection resolution through it. The
//! one-shot id search for the local servers is [`search_ids`]; Find
//! Duplicates (`browser/workspace` V10) reads through a session's handle or
//! the whole collection ([`find_duplicates`]).
//!
//! A search never writes: the day comes from the pure day calculation
//! (`sched/day-clock` D3, D4) on the request's snapshot, never from a day
//! query, which would unbury cards and store timing keys.

mod duplicates;
mod handle;
mod one_shot;
mod page;
mod registry;
mod selection;
mod selection_counts;
mod selection_reads;
mod worker;
pub(crate) use worker::context as search_context;

use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use meguri_model::changes::ChangeSet;
use meguri_search::SearchError;
pub use meguri_search::SearchMode;
pub use meguri_search::config::SortDirection;

use crate::cancel::CancelToken;
use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::lifecycle::{Engine, Generation, WeakEngine};

pub use duplicates::{Duplicates, DuplicatesTarget, find_duplicates};
pub use handle::{Invalidation, Refetch};
pub use one_shot::search_ids;
pub use page::{QaCells, QaWhich};
pub use selection::{MapFrom, Mapped, Selection};
pub use selection_counts::{SelectionCounts, selection_counts};
pub use selection_reads::{selection_note_cards, selection_tags};
pub(crate) use registry::{SearchRegistry, bridge};
pub(crate) use worker::{OWNER, cells, context};

use handle::Handle;
use worker::{Job, QueuedJob};

/// H1: ids are listed a page at a time; `FirstPage` holds at most this many.
pub const PAGE: usize = 200;
/// H7: the most indexes one `rows` or `qa_cells` call takes.
pub const MAX_ROWS: u64 = 1000;

/// A request's seq (H1): 1 for a session's first request, then increasing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestSeq(pub u64);

/// A handle (H6, H15): opaque, bound to the session that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HandleId(pub u64);

/// A sort request (H3): a column key, refused as `unsortable_column` by the
/// request when the mode cannot sort by it (`browser/sorting` S1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub key: String,
    pub direction: SortDirection,
}

impl SortSpec {
    pub fn new(key: impl Into<String>, direction: SortDirection) -> SortSpec {
        SortSpec {
            key: key.into(),
            direction,
        }
    }
}

/// H14.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseError {
    /// A search error (`search/errors` E1 to E3), a database refusal
    /// (`search_failed`) included.
    Search(SearchError),
    /// `browser/sorting` S1.
    UnsortableColumn,
    /// The call named another revision than the handle's (H6).
    StaleRevision { current: u64 },
    /// A released handle (H6, H12), or one of another session.
    UnknownHandle,
    /// An index outside the final prefix (H7).
    OutOfRange,
    /// A range of more than 1000 indexes or an unknown column key (H7).
    InvalidArgument { message: String },
    /// `reverse`, `resort` and `toggle` on a handle in progress (H8, H11).
    NotComplete,
    /// `StaleGeneration`, the engine's refusals, storage errors.
    Engine(EngineError),
}

impl From<EngineError> for BrowseError {
    fn from(error: EngineError) -> BrowseError {
        BrowseError::Engine(error)
    }
}

impl From<meguri_storage::StorageError> for BrowseError {
    fn from(error: meguri_storage::StorageError) -> BrowseError {
        BrowseError::Engine(error.into())
    }
}

impl BrowseError {
    pub(super) fn invalid(message: impl Into<String>) -> BrowseError {
        BrowseError::InvalidArgument {
            message: message.into(),
        }
    }
}

/// A session's events (H4); each carries its request's seq.
#[derive(Debug)]
pub enum SearchEvent {
    /// The first page is final: at most [`PAGE`] ids, in revision 0.
    FirstPage {
        seq: RequestSeq,
        handle: HandleId,
        ids: Vec<i64>,
    },
    /// Streamed requests only (`search/query-model` Q10).
    Progress {
        seq: RequestSeq,
        matched: u64,
        scanned: u64,
        revision: u64,
    },
    Complete {
        seq: RequestSeq,
        handle: HandleId,
        count: u64,
        revision: u64,
    },
    Failed {
        seq: RequestSeq,
        error: BrowseError,
        /// For a search error, the text of the request (its search, or the
        /// text of the handle it toggles or resorts): the error's range
        /// counts characters of it, and the FFI converts them to UTF-16
        /// against it (H15). `None` for every other error.
        text: Option<String>,
    },
    Cancelled {
        seq: RequestSeq,
    },
}

impl SearchEvent {
    pub fn seq(&self) -> RequestSeq {
        match self {
            SearchEvent::FirstPage { seq, .. }
            | SearchEvent::Progress { seq, .. }
            | SearchEvent::Complete { seq, .. }
            | SearchEvent::Failed { seq, .. }
            | SearchEvent::Cancelled { seq } => *seq,
        }
    }

    /// `Complete`, `Failed` and `Cancelled` end a request (H4).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SearchEvent::Complete { .. } | SearchEvent::Failed { .. } | SearchEvent::Cancelled { .. }
        )
    }
}

/// Receives a session's events (H2, H4), on a core thread that holds no
/// lock and no reader, never the writer thread. It must return promptly.
pub trait SearchListener: Send + Sync {
    fn event(&self, event: SearchEvent);
}

/// One Browse session (H1, H2). Dropping it closes it.
pub struct SearchSession {
    shared: Arc<SessionShared>,
}

pub(crate) struct SessionShared {
    engine: WeakEngine,
    registry: Weak<SearchRegistry>,
    listener: Arc<dyn SearchListener>,
    state: Mutex<SessionState>,
    #[cfg(feature = "test-hooks")]
    testing: worker::Testing,
}

#[derive(Default)]
struct SessionState {
    closed: bool,
    next_seq: u64,
    queue: VecDeque<QueuedJob>,
    /// A worker thread serves the queue.
    worker: bool,
    /// The request the worker runs.
    current: Option<Current>,
    handles: BTreeMap<HandleId, Arc<Mutex<Handle>>>,
    /// The two newest complete handles, oldest first (H6).
    complete: VecDeque<HandleId>,
}

struct Current {
    seq: u64,
    token: Arc<CancelToken>,
    /// Its handle once `FirstPage` was published.
    handle: Option<HandleId>,
}

impl Engine {
    /// H16: Q10's chunk of candidates for every search of this engine, one
    /// candidate per chunk in the conformance adapter; 0 restores the
    /// default.
    #[cfg(feature = "test-hooks")]
    pub fn set_search_stream_chunk_for_testing(&self, chunk: usize) {
        self.inner.searches.set_stream_chunk(chunk);
    }

    /// H2: a session whose events go to `listener`.
    pub fn open_search_session(&self, listener: Arc<dyn SearchListener>) -> SearchSession {
        let registry = &self.inner.searches;
        let shared = Arc::new(SessionShared {
            engine: self.downgrade(),
            registry: Arc::downgrade(registry),
            listener,
            state: Mutex::new(SessionState::default()),
            #[cfg(feature = "test-hooks")]
            testing: worker::Testing::default(),
        });
        registry.add_session(Arc::downgrade(&shared));
        SearchSession { shared }
    }
}

impl SessionShared {
    fn lock(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn engine(&self) -> Result<Engine, EngineError> {
        self.engine
            .upgrade()
            .ok_or_else(|| EngineError::unavailable("the engine is gone"))
    }

    /// Cancels the running and queued requests and releases the running
    /// one's handle (H3, H6).
    fn supersede(&self, state: &mut SessionState) {
        if let Some(current) = &state.current {
            current.token.cancel();
            if let Some(handle) = current.handle {
                state.handles.remove(&handle);
            }
        }
        for job in &mut state.queue {
            job.token.cancel();
            // Only its seq is needed now, for its `Cancelled`; a resort's
            // copy of its ids goes at once. A request refused when it was
            // made keeps its refusal (H3).
            if !matches!(job.job, Job::Refused) {
                job.job = Job::Superseded;
            }
        }
        if let Some(registry) = self.registry.upgrade() {
            registry.wake();
        }
    }

    /// Queues a request after superseding the session's earlier ones.
    fn enqueue(self: &Arc<Self>, job: Job, cancel: Option<&CancelToken>) -> RequestSeq {
        let token = CancelToken::new();
        if let (Some(caller), Some(registry)) = (cancel, self.registry.upgrade()) {
            registry::bridge(caller, &token, &registry);
        }
        let (epoch, stopping) = self.registry.upgrade().map_or((0, false), |registry| registry.epoch());
        let mut state = self.lock();
        self.supersede(&mut state);
        state.next_seq += 1;
        let seq = state.next_seq;
        if state.closed {
            token.cancel();
        }
        // H3: a request made while the generation changes (or the
        // collection suspends or closes) fails with the engine's refusal.
        let job = if stopping { Job::Refused } else { job };
        state.queue.push_back(QueuedJob { seq, token, epoch, job });
        if !state.worker {
            state.worker = true;
            drop(state);
            if let Err(error) = worker::spawn(self) {
                self.fail_queue(&error.to_string());
            }
        }
        RequestSeq(seq)
    }

    /// No worker thread could start: every queued request ends with
    /// `Failed`, or `Cancelled` when it was cancelled (H4: one terminal event
    /// each, in order). The session keeps its worker flag until the queue is
    /// empty, so a request queued meanwhile cannot overtake these.
    fn fail_queue(&self, error: &str) {
        loop {
            let jobs: Vec<QueuedJob> = {
                let mut state = self.lock();
                if state.queue.is_empty() {
                    state.worker = false;
                    return;
                }
                state.queue.drain(..).collect()
            };
            for job in jobs {
                // A panicking listener loses that event, not the others,
                // and the worker flag still clears.
                let event = worker::unstarted(&job, &format!("cannot start a search worker: {error}"));
                let listener = &self.listener;
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener.event(event)));
            }
        }
    }

    /// The handle `id` names, if this session holds it in the current
    /// generation; one of an older generation is released (H6), and so is
    /// the running request's handle once that request is cancelled.
    fn handle(&self, id: HandleId) -> Result<Arc<Mutex<Handle>>, BrowseError> {
        let current = self.engine()?.generation();
        let mut state = self.lock();
        if let Some(running) = &state.current
            && running.handle == Some(id)
            && running.token.is_cancelled()
        {
            state.handles.remove(&id);
            return Err(BrowseError::UnknownHandle);
        }
        let handle = state.handles.get(&id).cloned().ok_or(BrowseError::UnknownHandle)?;
        if lock(&handle).generation != current {
            state.handles.remove(&id);
            state.complete.retain(|kept| *kept != id);
            return Err(BrowseError::UnknownHandle);
        }
        Ok(handle)
    }

    /// H6: drops the handles of generations before `current`.
    fn release_before(&self, current: Generation) {
        let mut state = self.lock();
        let old: Vec<HandleId> = state
            .handles
            .iter()
            .filter(|(_, handle)| lock(handle).generation < current)
            .map(|(id, _)| *id)
            .collect();
        for id in old {
            state.handles.remove(&id);
            state.complete.retain(|kept| *kept != id);
        }
    }
}

fn lock(handle: &Mutex<Handle>) -> MutexGuard<'_, Handle> {
    handle.lock().unwrap_or_else(PoisonError::into_inner)
}

/// H6: a call that takes indexes names the revision they were computed on.
fn check_revision(handle: &Handle, revision: u64) -> Result<(), BrowseError> {
    if handle.revision == revision {
        Ok(())
    } else {
        Err(BrowseError::StaleRevision {
            current: handle.revision,
        })
    }
}

impl SearchSession {
    /// H3: starts a search of `text` as given, in `mode`, sorted by `sort`;
    /// returns its seq at once. It supersedes the session's earlier
    /// requests; `cancel` cancels it too.
    pub fn start_search(
        &self,
        text: &str,
        mode: SearchMode,
        sort: SortSpec,
        cancel: Option<&CancelToken>,
    ) -> RequestSeq {
        self.shared.enqueue(
            Job::Search {
                text: text.to_owned(),
                mode,
                sort,
            },
            cancel,
        )
    }

    /// H8: sorts a complete handle's ids by `sort` in a new snapshot,
    /// without running the predicate. `NotComplete` on a handle in
    /// progress, before anything starts.
    pub fn resort(
        &self,
        handle: HandleId,
        sort: SortSpec,
        cancel: Option<&CancelToken>,
    ) -> Result<RequestSeq, BrowseError> {
        let handle = self.shared.handle(handle)?;
        let source = {
            let handle = lock(&handle);
            if !handle.complete {
                return Err(BrowseError::NotComplete);
            }
            worker::Source::of(&handle)
        };
        Ok(self.shared.enqueue(Job::Resort { source, sort }, cancel))
    }

    /// H11: the handle's text in the other mode, sorted by `sort` (that
    /// mode's sort), derived from the handle without a predicate run when it
    /// can be. `NotComplete` on a handle in progress, before anything starts.
    pub fn toggle(
        &self,
        handle: HandleId,
        sort: SortSpec,
        cancel: Option<&CancelToken>,
    ) -> Result<RequestSeq, BrowseError> {
        let handle = self.shared.handle(handle)?;
        let source = {
            let handle = lock(&handle);
            if !handle.complete {
                return Err(BrowseError::NotComplete);
            }
            worker::Source::of(&handle)
        };
        Ok(self.shared.enqueue(Job::Toggle { source, sort }, cancel))
    }

    /// H8: reverses a complete handle in memory and returns its new
    /// revision; index i becomes count − 1 − i.
    pub fn reverse(&self, handle: HandleId, revision: u64) -> Result<u64, BrowseError> {
        let handle = self.shared.handle(handle)?;
        let mut handle = lock(&handle);
        check_revision(&handle, revision)?;
        if !handle.complete {
            return Err(BrowseError::NotComplete);
        }
        Ok(handle.reverse())
    }

    /// H10: applies the change sets received since the request started, in
    /// seq order, and says which rows to refetch.
    pub fn invalidated_rows(
        &self,
        handle: HandleId,
        revision: u64,
        change_sets: &[(Seq, &ChangeSet)],
    ) -> Result<Invalidation, BrowseError> {
        let handle = self.shared.handle(handle)?;
        let mut handle = lock(&handle);
        check_revision(&handle, revision)?;
        Ok(handle.invalidate(change_sets))
    }

    /// H7: the row records of the indexes in `range`, Question and Answer
    /// deferred.
    pub fn rows(
        &self,
        handle: HandleId,
        revision: u64,
        range: Range<u64>,
        columns: &[&str],
    ) -> Result<Vec<crate::browse::RowResult>, BrowseError> {
        page::rows(&self.shared, handle, revision, range, columns)
    }

    /// H7: the Question cells, the Answer cells or both for the indexes in
    /// `range`, cancellable between rows.
    pub fn qa_cells(
        &self,
        handle: HandleId,
        revision: u64,
        range: Range<u64>,
        which: QaWhich,
        cancel: Option<&CancelToken>,
    ) -> Result<Vec<Result<QaCells, crate::browse::RowFailure>>, BrowseError> {
        page::qa_cells(&self.shared, handle, revision, range, which, cancel)
    }

    /// H12: the ids an action works on, from index ranges of the handle.
    pub fn resolve_selection(
        &self,
        handle: HandleId,
        revision: u64,
        ranges: &[Range<u64>],
        target: SearchMode,
    ) -> Result<Vec<i64>, BrowseError> {
        selection::resolve(&self.shared, handle, revision, ranges, target)
    }

    /// H12: a selection of index ranges of `handle` at `revision`, kept as
    /// ranges until a bulk action resolves it inside its job (U86), so a
    /// large selection never crosses the FFI as ids.
    pub fn selection(&self, handle: HandleId, revision: u64, ranges: Vec<Range<u64>>) -> HandleSelection {
        HandleSelection {
            shared: Arc::clone(&self.shared),
            handle,
            revision,
            ranges,
        }
    }

    /// H12: the index of `id` in the handle (`kind` says whether it is a
    /// card id or a note id), or none.
    pub fn index_of(&self, handle: HandleId, revision: u64, kind: SearchMode, id: i64) -> Result<Option<u64>, BrowseError> {
        selection::index_of(&self.shared, handle, revision, kind, id)
    }

    /// H12: maps a selection and the current row from `from` to `to`, over
    /// the final prefix of `to` at `to_revision`.
    pub fn map_selection(
        &self,
        from: MapFrom,
        selection: Selection,
        current: Option<i64>,
        to: HandleId,
        to_revision: u64,
    ) -> Result<Mapped, BrowseError> {
        selection::map(&self.shared, from, selection, current, to, to_revision)
    }

    /// H6: the handle's state and count in its current revision.
    pub fn handle_state(&self, handle: HandleId) -> Result<HandleState, BrowseError> {
        let handle = self.shared.handle(handle)?;
        let handle = lock(&handle);
        Ok(HandleState {
            mode: handle.mode,
            revision: handle.revision,
            count: handle.count(),
            complete: handle.complete,
            sort_key: handle.sort.column().key(),
            direction: handle.sort.direction(),
        })
    }

    /// H2: cancels the running request and releases every handle. Dropping
    /// the session does the same.
    pub fn close(self) {}

    /// [`SearchSession::close`] through a shared reference, for an owner
    /// that cannot wait for every borrower to let go: requests made after
    /// it end `Cancelled`, and every handle is released at once.
    /// Idempotent.
    pub fn shutdown(&self) {
        let mut state = self.shared.lock();
        state.closed = true;
        self.shared.supersede(&mut state);
        state.handles.clear();
        state.complete.clear();
    }
}

#[cfg(feature = "test-hooks")]
impl SearchSession {
    /// `specs/conformance/browser-ops.md` B2 for the conformance adapter:
    /// the handle's ids, each with a number that is equal exactly for
    /// consecutive rows equal on every sort key but the final one, from the
    /// handle's own keys and sort clock. The order is the statement's; the
    /// adapter checks it against the handle's.
    pub fn equal_key_runs_for_testing(&self, handle: HandleId) -> Result<Vec<(i64, i64)>, BrowseError> {
        use meguri_search::SortedQuery;
        use meguri_storage::{ReadRows, ReaderRole};
        let (ids, sorted) = {
            let handle = self.shared.handle(handle)?;
            let handle = lock(&handle);
            let sorted = SortedQuery::for_ids(&handle.ids, &handle.sort, &handle.sort_clock)
                .map_err(|error| BrowseError::Engine(EngineError::internal(format!("{error:?}"))))?;
            (handle.ids.len(), sorted)
        };
        let collection = self.shared.engine()?.collection()?;
        let reader = collection.reader(ReaderRole::Query)?;
        let snapshot = reader.snapshot()?;
        let mut runs = Vec::with_capacity(ids);
        snapshot.visit_id_rows(
            sorted.runs_sql(),
            &worker::cells(sorted.params()),
            &|| false,
            &mut |row| {
                runs.push((row[0], row[1]));
                true
            },
        )?;
        Ok(runs)
    }

    /// Runs `hook` on the worker once, after the next `FirstPage` went out
    /// and before its handle completes.
    pub fn after_first_page_for_testing(&self, hook: impl FnOnce() + Send + 'static) {
        *self
            .shared
            .testing
            .after_first_page
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }

    /// H16: how many of H11's extra tests the session's toggles made.
    pub fn extra_tests_for_testing(&self) -> u64 {
        self.shared.testing.extra_tests.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Runs `during` while every search of the engine is stopping, as a
    /// replacement, a suspend or a close halts them.
    pub fn while_searches_stopped_for_testing<T>(engine: &Engine, during: impl FnOnce() -> T) -> T {
        let _halt = engine.inner.searches.halt();
        during()
    }

    /// H16: how many predicate runs the session's requests made.
    pub fn predicate_runs_for_testing(&self) -> u64 {
        self.shared.testing.runs.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// A selection of a handle's indexes (H12), resolved to ids only when a
/// command runs. It refers to the session without keeping it open: once the
/// session is closed or the handle released, resolving it fails with
/// `UnknownHandle`.
#[derive(Clone)]
pub struct HandleSelection {
    shared: Arc<SessionShared>,
    handle: HandleId,
    revision: u64,
    ranges: Vec<Range<u64>>,
}

impl std::fmt::Debug for HandleSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleSelection")
            .field("handle", &self.handle)
            .field("revision", &self.revision)
            .field("ranges", &self.ranges)
            .finish()
    }
}

impl HandleSelection {
    /// H12's `resolve_selection` for `target`, in one read transaction on
    /// the query reader.
    pub fn resolve(&self, target: SearchMode) -> Result<Vec<i64>, BrowseError> {
        selection::resolve(&self.shared, self.handle, self.revision, &self.ranges, target)
    }

    /// [`HandleSelection::resolve`] for an action on `collection`: the ids
    /// only when the handle belongs to that collection and its generation
    /// (H6), so a selection made on another collection never names rows of
    /// this one. `UnknownHandle` otherwise.
    pub fn resolve_for(&self, collection: &crate::Collection, target: SearchMode) -> Result<Vec<i64>, BrowseError> {
        let ids = self.resolve(target)?;
        // The handle was checked against its session's collection while it
        // was read; that collection must be the action's.
        let own = self.shared.engine()?.collection()?;
        if own.generation() != collection.generation() || !Arc::ptr_eq(&own.core, &collection.core) {
            return Err(BrowseError::UnknownHandle);
        }
        Ok(ids)
    }
}

/// What [`SearchSession::handle_state`] reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleState {
    pub mode: SearchMode,
    pub revision: u64,
    pub count: u64,
    pub complete: bool,
    pub sort_key: String,
    pub direction: SortDirection,
}

impl Drop for SearchSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}
