//! A session's search worker (`browser/result-sets` H3, H4, H8): one thread
//! that runs the session's requests in order, each on a search reader of its
//! own, in one snapshot. Events go to a delivery thread joined before the
//! next request starts, so the listener runs while the worker holds no lock
//! and no reader, and every event of a request precedes the next one's.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use meguri_sched::model::sched_config::SchedConfig;
use meguri_search::plan::ids;
use meguri_search::plan::sql::id_list;
use meguri_search::{
    BuildError, ContextReads, ContextValue, ErrorKind, Param, Plan, SearchContext, SearchError, SearchFailure, SearchMode,
    SearchNotetypes, Sort, SortError, SortedQuery,
};
use meguri_storage::{Cell, ReadRows};

use crate::cancel::CancelToken;
use crate::changes::stream::Seq;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::lifecycle::Generation;
use crate::threads::Qos;

use super::handle::Handle;
use super::{BrowseError, HandleId, PAGE, RequestSeq, SearchEvent, SessionShared, SortSpec, lock};

/// What a request does.
pub(super) enum Job {
    Search {
        text: String,
        mode: SearchMode,
        sort: SortSpec,
    },
    Resort {
        source: Source,
        sort: SortSpec,
    },
    Toggle {
        source: Source,
        sort: SortSpec,
    },
    /// Superseded while queued: it ends `Cancelled` without running.
    Superseded,
    /// Made during a halt: it ends `Failed` with the engine's refusal (H3).
    Refused,
}

pub(super) struct QueuedJob {
    pub(super) seq: u64,
    pub(super) token: Arc<CancelToken>,
    /// The registry's halt epoch when it was queued.
    pub(super) epoch: u64,
    pub(super) job: Job,
}

/// What a resort keeps of its handle (H8), copied when it is asked for.
pub(super) struct Source {
    mode: SearchMode,
    text: String,
    context: SearchContext,
    reads: ContextReads,
    ids: Vec<i64>,
    /// Cards mode: the note id of each card of `ids`.
    nids: Vec<i64>,
    set_aside: Vec<(i64, i64)>,
    match_seq: Seq,
    match_data: Option<Vec<i64>>,
    generation: Generation,
}

impl Source {
    pub(super) fn of(handle: &Handle) -> Source {
        Source {
            mode: handle.mode,
            text: handle.text.clone(),
            context: handle.context.clone(),
            reads: handle.reads,
            ids: handle.ids.clone(),
            nids: handle.nids.clone(),
            set_aside: handle.set_aside.clone(),
            match_seq: handle.match_seq,
            match_data: handle.match_data.clone(),
            generation: handle.generation,
        }
    }
}

/// Test settings and counters (H16).
#[cfg(feature = "test-hooks")]
#[derive(Default)]
pub(crate) struct Testing {
    /// Predicate runs: executions of a plan over the collection's rows.
    pub(crate) runs: std::sync::atomic::AtomicU64,
    /// H11's extra tests: the query on lone rows alone.
    pub(crate) extra_tests: std::sync::atomic::AtomicU64,
    /// Runs on the worker once, after the next `FirstPage` went out and
    /// before the handle completes, so tests see a handle in progress.
    pub(crate) after_first_page: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// Starts the session's worker thread.
pub(super) fn spawn(shared: &Arc<SessionShared>) -> std::io::Result<()> {
    let engine = shared
        .engine()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let session = Arc::clone(shared);
    let threads = engine.threads().clone();
    engine
        .threads()
        .spawn_owned("meguri-search", Qos::UserInitiated, OWNER, move |activity| {
            serve(&session, &threads, activity);
        })
        .map(drop)
}

/// The suspend owner of the search threads (U32): a suspend halts every
/// request first (`registry::SearchRegistry::halt`).
pub(crate) const OWNER: &str = "search";

fn serve(shared: &Arc<SessionShared>, threads: &crate::threads::ThreadRegistry, activity: &crate::threads::ThreadActivity) {
    loop {
        let job = {
            let mut state = shared.lock();
            match state.queue.pop_front() {
                Some(job) => {
                    state.current = Some(super::Current {
                        seq: job.seq,
                        token: Arc::clone(&job.token),
                        handle: None,
                    });
                    job
                }
                None => {
                    state.worker = false;
                    state.current = None;
                    return;
                }
            }
        };
        let _busy = activity.busy();
        let (outbox, inbox) = mpsc::channel::<SearchEvent>();
        let listener = Arc::clone(&shared.listener);
        let delivery = threads.spawn_owned("meguri-search-events", Qos::UserInitiated, OWNER, move |activity| {
            for event in inbox {
                let _busy = activity.busy();
                // A panicking listener loses that event, not the next ones.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener.event(event)));
            }
        });
        let seq = RequestSeq(job.seq);
        match delivery {
            Ok(delivery) => {
                let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(shared, job, &outbox)));
                if ran.is_err() {
                    // H4: the request still ends with one terminal event,
                    // and its handle goes.
                    let mut state = shared.lock();
                    if let Some(handle) = state.current.as_ref().and_then(|current| current.handle) {
                        state.handles.remove(&handle);
                        state.complete.retain(|kept| *kept != handle);
                    }
                    drop(state);
                    let _ = outbox.send(SearchEvent::Failed {
                        seq,
                        error: BrowseError::Engine(EngineError::internal("the search worker panicked")),
                        text: None,
                    });
                }
                drop(outbox);
                let _ = delivery.join();
            }
            Err(error) => {
                let event = unstarted(&job, &format!("cannot start the search event thread: {error}"));
                let listener = Arc::clone(&shared.listener);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener.event(event)));
            }
        }
        let mut state = shared.lock();
        if state.current.as_ref().is_some_and(|current| current.seq == seq.0) {
            state.current = None;
        }
    }
}

/// Why a request ends without `Complete`.
pub(super) enum Stop {
    Failed(BrowseError),
    Cancelled,
}

impl From<EngineError> for Stop {
    fn from(error: EngineError) -> Stop {
        Stop::Failed(BrowseError::Engine(error))
    }
}

impl From<meguri_storage::StorageError> for Stop {
    fn from(error: meguri_storage::StorageError) -> Stop {
        Stop::Failed(BrowseError::from(error))
    }
}

/// H3: the engine's refusal of a request made during a stop.
fn refusal_event(seq: RequestSeq) -> SearchEvent {
    SearchEvent::Failed {
        seq,
        error: BrowseError::Engine(EngineError::unavailable(
            "the collection is changing generation, suspending or closing",
        )),
        text: None,
    }
}

/// The terminal event of a request no thread could run (H4): its refusal
/// when it was made during a stop, `Cancelled` when it was cancelled,
/// otherwise `Failed` with `why`.
pub(super) fn unstarted(job: &QueuedJob, why: &str) -> SearchEvent {
    let seq = RequestSeq(job.seq);
    if matches!(job.job, Job::Refused) {
        refusal_event(seq)
    } else if job.token.is_cancelled() {
        SearchEvent::Cancelled { seq }
    } else {
        SearchEvent::Failed {
            seq,
            error: BrowseError::Engine(EngineError::internal(why)),
            text: None,
        }
    }
}

/// One request: its events, then nothing more of it.
fn run(shared: &Arc<SessionShared>, job: QueuedJob, outbox: &mpsc::Sender<SearchEvent>) {
    let request = Request {
        shared,
        seq: job.seq,
        token: &job.token,
        outbox,
        published: std::cell::Cell::new(None),
    };
    // H3: made during a stop, it fails with the engine's refusal, whatever
    // happens to its token afterwards.
    if matches!(job.job, Job::Refused) {
        let _ = outbox.send(refusal_event(RequestSeq(job.seq)));
        return;
    }
    // Listed for halts from the start, so a halt that begins while the
    // request parses, or began after it was queued, cancels it (H3).
    let registry = shared.registry.upgrade();
    let _running = registry.as_ref().map(|registry| registry.run(&job.token, job.epoch));
    // The text a search error's range counts in (H15).
    let (result, text) = if job.token.is_cancelled() {
        (Err(Stop::Cancelled), None)
    } else {
        match job.job {
            Job::Search { text, mode, sort } => (request.search(&text, mode, &sort), Some(text)),
            Job::Resort { source, sort } => (request.resort(source, &sort), None),
            Job::Toggle { source, sort } => {
                let text = source.text.clone();
                (request.toggle(source, &sort), Some(text))
            }
            Job::Superseded => (Err(Stop::Cancelled), None),
            Job::Refused => unreachable!("answered above"),
        }
    };
    let Err(stop) = result else { return };
    if let Some(handle) = request.published.get() {
        // H4: a request that fails or is cancelled after `FirstPage`
        // releases its handle.
        let mut state = shared.lock();
        state.handles.remove(&handle);
        state.complete.retain(|kept| *kept != handle);
    }
    let seq = RequestSeq(job.seq);
    let event = match stop {
        _ if job.token.is_cancelled() => SearchEvent::Cancelled { seq },
        Stop::Cancelled => SearchEvent::Cancelled { seq },
        Stop::Failed(error) => SearchEvent::Failed {
            seq,
            text: text.filter(|_| matches!(error, BrowseError::Search(_))),
            error,
        },
    };
    let _ = outbox.send(event);
}

/// One running request.
pub(super) struct Request<'a> {
    pub(super) shared: &'a Arc<SessionShared>,
    pub(super) seq: u64,
    pub(super) token: &'a Arc<CancelToken>,
    outbox: &'a mpsc::Sender<SearchEvent>,
    /// Its handle once `FirstPage` went out.
    published: std::cell::Cell<Option<HandleId>>,
}

/// The ids a request read: in cards mode each with its note id.
#[derive(Default)]
pub(super) struct Listed {
    pub(super) ids: Vec<i64>,
    pub(super) nids: Vec<i64>,
}

impl Request<'_> {
    fn send(&self, event: SearchEvent) {
        let _ = self.outbox.send(event);
    }

    fn cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// The engine's current collection (H3: none open is the engine's
    /// refusal).
    fn collection(&self) -> Result<(crate::Engine, Collection), Stop> {
        let engine = self.shared.engine()?;
        let collection = engine.collection()?;
        Ok((engine, collection))
    }

    /// Runs `sql` on `rows`, interrupted once the request is cancelled.
    pub(super) fn visit(
        &self,
        rows: &impl ReadRows,
        sql: &str,
        params: &[Param],
        visit: &mut dyn FnMut(&[i64]) -> bool,
    ) -> Result<(), Stop> {
        let cells = cells(params);
        rows.visit_id_rows(sql, &cells, &|| self.token.is_cancelled(), visit)
            .map_err(|error| {
                if self.cancelled() {
                    Stop::Cancelled
                } else {
                    Stop::Failed(BrowseError::Engine(error.into()))
                }
            })
    }

    /// Reads one-or-two-column rows into a list.
    pub(super) fn list(&self, rows: &impl ReadRows, sql: &str, params: &[Param]) -> Result<Listed, Stop> {
        let mut listed = Listed::default();
        self.visit(rows, sql, params, &mut |row| {
            listed.ids.push(row[0]);
            if let Some(nid) = row.get(1) {
                listed.nids.push(*nid);
            }
            true
        })?;
        Ok(listed)
    }

    /// Publishes the handle and its first page, unless the request was
    /// superseded meanwhile.
    fn first_page(&self, handle: Handle) -> Result<(HandleId, Arc<Mutex<Handle>>), Stop> {
        let ids = handle.ids[..handle.ids.len().min(PAGE)].to_vec();
        let mut handle = handle;
        handle.prefix = ids.len();
        let handle = Arc::new(Mutex::new(handle));
        let mut state = self.shared.lock();
        let live = state
            .current
            .as_ref()
            .is_some_and(|current| current.seq == self.seq && !current.token.is_cancelled());
        if !live || state.closed {
            return Err(Stop::Cancelled);
        }
        let id = match self.shared.registry.upgrade() {
            Some(registry) => registry.next_handle(),
            None => return Err(Stop::Cancelled),
        };
        state.handles.insert(id, Arc::clone(&handle));
        if let Some(current) = state.current.as_mut() {
            current.handle = Some(id);
        }
        drop(state);
        self.published.set(Some(id));
        self.send(SearchEvent::FirstPage {
            seq: RequestSeq(self.seq),
            handle: id,
            ids,
        });
        Ok((id, handle))
    }

    /// Appends confirmed rows to a handle in progress, leaving out the ids
    /// H10 recorded as removed meanwhile.
    fn append(handle: &Mutex<Handle>, rows: Listed) {
        let mut handle = lock(handle);
        for (index, id) in rows.ids.iter().enumerate() {
            if handle.removed_while_running.contains(id) {
                continue;
            }
            handle.ids.push(*id);
            if let Some(nid) = rows.nids.get(index) {
                handle.nids.push(*nid);
            }
        }
    }

    /// Completes the handle and sends `Complete`, unless the request was
    /// superseded. Set-aside cards H10 recorded as removed go too.
    fn complete(&self, id: HandleId, handle: &Mutex<Handle>) -> Result<(), Stop> {
        // One transition under the session's lock, then the handle's: no
        // call sees the handle complete while its request is still current.
        let mut state = self.shared.lock();
        let live = state
            .current
            .as_ref()
            .is_some_and(|current| current.seq == self.seq && !current.token.is_cancelled());
        if !live || state.closed {
            return Err(Stop::Cancelled);
        }
        let (count, revision) = {
            let mut handle = lock(handle);
            let removed = std::mem::take(&mut handle.removed_while_running);
            if handle.mode == SearchMode::Cards {
                handle.set_aside.retain(|(card, _)| !removed.contains(card));
            }
            handle.complete = true;
            (handle.count(), handle.revision)
        };
        state.current = None;
        state.complete.push_back(id);
        while state.complete.len() > 2 {
            if let Some(old) = state.complete.pop_front() {
                state.handles.remove(&old);
            }
        }
        drop(state);
        self.send(SearchEvent::Complete {
            seq: RequestSeq(self.seq),
            handle: id,
            count,
            revision,
        });
        Ok(())
    }

    /// Moves the first page of `pending` into `handle` and publishes it.
    fn publish_first(&self, mut handle: Handle, pending: &mut Listed) -> Result<(HandleId, Arc<Mutex<Handle>>), Stop> {
        let first = pending.ids.len().min(PAGE);
        handle.ids = pending.ids.drain(..first).collect();
        handle.nids = pending.nids.drain(..first.min(pending.nids.len())).collect();
        let published = self.first_page(handle)?;
        #[cfg(feature = "test-hooks")]
        if let Some(hook) = self
            .shared
            .testing
            .after_first_page
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            hook();
        }
        Ok(published)
    }

    /// Runs `work` on a search reader's snapshot of the current collection,
    /// with the request listed for halts.
    fn with_snapshot<T>(
        &self,
        work: impl FnOnce(&Collection, &crate::readers::Snapshot<'_>) -> Result<T, Stop>,
    ) -> Result<T, Stop> {
        let (_engine, collection) = self.collection()?;
        let registry = self.shared.registry.upgrade().ok_or(Stop::Cancelled)?;
        let slot = registry.reader(self.token).ok_or(Stop::Cancelled)?;
        let lease = collection.search_reader(slot.index)?;
        let snapshot = lease.snapshot()?;
        work(&collection, &snapshot)
    }

    /// H3: parse, check the sort, build the context and the plan, run the
    /// sorted statement in one snapshot.
    fn search(&self, text: &str, mode: SearchMode, sort: &SortSpec) -> Result<(), Stop> {
        // B1's order: a parse error, then `unsortable_column`, then the
        // run errors.
        let query = meguri_search::parse(text).map_err(|error| Stop::Failed(BrowseError::Search(error)))?;
        let sort = Sort::new(mode, &sort.key, sort.direction).map_err(|_| Stop::Failed(BrowseError::UnsortableColumn))?;
        self.with_snapshot(|collection, snapshot| {
            let context = context(collection, &**snapshot)?;
            let notetypes = SearchNotetypes::read(&**snapshot)?;
            let plan = Plan::build(&query, mode, &context, &notetypes).map_err(build_error)?;
            self.run_query(
                snapshot,
                QueryRun {
                    text,
                    query: &query,
                    sort,
                    context,
                    notetypes: &notetypes,
                    plan,
                },
            )
        })
    }

    /// H3 step 4 and Q10: one predicate run of `plan` sorted, streamed when
    /// the plan has slow terms; then the set-aside cards (H5) and the match
    /// data (H11), in the same snapshot.
    fn run_query(&self, snapshot: &crate::readers::Snapshot<'_>, run: QueryRun<'_>) -> Result<(), Stop> {
        let QueryRun {
            text,
            query,
            sort,
            context,
            notetypes,
            plan,
        } = run;
        let mode = plan.mode();
        let sorted = SortedQuery::for_plan(&plan, &sort, &context).map_err(sort_error)?;
        let template = Handle {
            mode,
            sort: sort.clone(),
            text: text.to_owned(),
            context: context.clone(),
            reads: plan.reads(),
            sort_clock: context.clone(),
            ids: Vec::new(),
            nids: Vec::new(),
            set_aside: Vec::new(),
            match_seq: snapshot.seq(),
            applied_seq: snapshot.seq(),
            generation: snapshot.generation(),
            revision: 0,
            match_data: None,
            complete: false,
            removed_while_running: Default::default(),
            prefix: 0,
        };
        let rows = &**snapshot;
        let (id, handle, match_data) = if streams(
            rows,
            &plan,
            self.shared.registry.upgrade().as_deref(),
            &|| self.token.is_cancelled(),
        )? {
            self.streamed(rows, text, query, &sort, &context, notetypes, &plan, template)?
        } else {
            let (id, handle) = self
                .paged(rows, &sorted, template)
                .map_err(|stop| refused(stop, text))?;
            self.count_run();
            // H11's match data: the cards of the matching card-and-note
            // pairs.
            let match_data = match plan.pairs_sql() {
                Some(sql) => Some(self.list(rows, &sql, plan.params()).map_err(|stop| refused(stop, text))?.ids),
                None => None,
            };
            (id, handle, match_data)
        };
        // H5: under S9's note-column sorts the matched cards without a note
        // row are kept aside, not listed; a sound collection has none.
        let set_aside = match (mode == SearchMode::Cards && !sorted.lists_lone_cards(), plan.lone_rows_sql()) {
            (true, Some(sql)) if self.any(rows, ids::ANY_LONE_CARD)? => {
                let lone = self.list(rows, &sql, plan.params()).map_err(|stop| refused(stop, text))?;
                lone.ids.into_iter().zip(lone.nids).collect()
            }
            _ => Vec::new(),
        };
        {
            let mut handle = lock(&handle);
            handle.set_aside = set_aside;
            handle.match_data = match_data;
        }
        self.complete(id, &handle)
    }

    /// Q10: the relaxed query sorted gives the candidates in final order;
    /// the original query tests them a chunk at a time, so the kept rows so
    /// far are the start of the result. The first page goes out as soon as
    /// it is full; `Progress` follows at most every 100 ms (`engine/jobs`
    /// J5). Returns the published handle and, in notes mode with card or
    /// card-and-note scope, the cards of the kept pairs as match data.
    #[allow(clippy::too_many_arguments)]
    fn streamed(
        &self,
        rows: &meguri_storage::ReadSnapshot<'_>,
        text: &str,
        query: &meguri_search::Query,
        sort: &Sort,
        context: &SearchContext,
        notetypes: &SearchNotetypes,
        plan: &Plan,
        template: Handle,
    ) -> Result<Published, Stop> {
        let mode = plan.mode();
        let relaxed = Plan::build_relaxed(query, mode, context, notetypes).map_err(build_error)?;
        let candidates = SortedQuery::for_plan(&relaxed, sort, context).map_err(sort_error)?;
        let candidates = self
            .list(rows, candidates.pairs_sql(), candidates.params())
            .map_err(|stop| refused(stop, text))?;
        let fixed = self.shared.registry.upgrade().and_then(|registry| registry.stream_chunk());
        let chunk = fixed.unwrap_or(DEFAULT_CHUNK);
        let list = format!("?{}", plan.params().len() + 1);
        let pairs_sql = plan.restricted_pairs_sql(&list);
        let sql = plan.restricted_sql(&list);
        let mut params = plan.params().to_vec();
        params.push(Param::Integer(0));
        let mut pending = Listed::default();
        let mut pairs: Option<Vec<i64>> = pairs_sql.as_ref().map(|_| Vec::new());
        let mut template = Some(template);
        let mut published: Option<(HandleId, Arc<Mutex<Handle>>)> = None;
        let mut scanned = 0u64;
        let mut last_progress = crate::monotonic::now();
        // Chunks start at `chunk` and double up to MAX_CHUNK, so a sparse
        // result does not pay one statement per few hundred candidates,
        // while a dense one fills its first page from the first chunks. A
        // chunk size a test sets stays fixed (H16).
        let grows = fixed.is_none();
        let mut windows = Vec::new();
        let mut start = 0usize;
        let mut size = chunk;
        while start < candidates.ids.len() {
            let end = (start + size).min(candidates.ids.len());
            windows.push(start..end);
            start = end;
            if grows {
                size = (size * 2).min(MAX_CHUNK);
            }
        }
        for range in windows {
            let window = &candidates.ids[range.clone()];
            *params.last_mut().expect("the list parameter") = id_list(window);
            let keep: std::collections::HashSet<i64> = match &pairs_sql {
                Some(sql) => {
                    let found = self.list(rows, sql, &params).map_err(|stop| refused(stop, text))?;
                    if let Some(pairs) = pairs.as_mut() {
                        pairs.extend(found.ids.iter().copied());
                    }
                    found.nids.into_iter().collect()
                }
                None => self
                    .list(rows, &sql, &params)
                    .map_err(|stop| refused(stop, text))?
                    .ids
                    .into_iter()
                    .collect(),
            };
            let base = range.start;
            for (offset, id) in window.iter().enumerate() {
                if keep.contains(id) {
                    pending.ids.push(*id);
                    if let Some(nid) = candidates.nids.get(base + offset)
                        && mode == SearchMode::Cards
                    {
                        pending.nids.push(*nid);
                    }
                }
            }
            scanned += window.len() as u64;
            match &published {
                None if pending.ids.len() >= PAGE => {
                    let handle = template.take().expect("published once");
                    let first = self.publish_first(handle, &mut pending)?;
                    Self::append(&first.1, std::mem::take(&mut pending));
                    published = Some(first);
                }
                None => {}
                Some((_, handle)) => Self::append(handle, std::mem::take(&mut pending)),
            }
            if let Some((_, handle)) = &published
                && crate::monotonic::now().saturating_sub(last_progress) >= PROGRESS_EVERY
            {
                last_progress = crate::monotonic::now();
                // The rows confirmed so far become the announced prefix.
                let (matched, revision) = {
                    let mut handle = lock(handle);
                    handle.prefix = handle.ids.len();
                    (handle.count(), handle.revision)
                };
                self.send(SearchEvent::Progress {
                    seq: RequestSeq(self.seq),
                    matched,
                    scanned,
                    revision,
                });
            }
        }
        let (id, handle) = match published {
            Some(published) => {
                Self::append(&published.1, pending);
                published
            }
            None => {
                let handle = template.take().expect("published once");
                let first = self.publish_first(handle, &mut pending)?;
                Self::append(&first.1, pending);
                first
            }
        };
        self.count_run();
        Ok((id, handle, pairs))
    }

    /// Publishes a sorted statement's rows: the first page from the same
    /// statement with a LIMIT, which SQLite orders with a bounded heap rather
    /// than a full sort (equal rows, final, earlier: H3, H8 budgets), then
    /// the rest from the full statement in the same snapshot.
    fn paged(
        &self,
        rows: &meguri_storage::ReadSnapshot<'_>,
        sorted: &SortedQuery,
        template: Handle,
    ) -> Result<(HandleId, Arc<Mutex<Handle>>), Stop> {
        let mut first = self.list(rows, &format!("{} LIMIT {PAGE}", sorted.pairs_sql()), sorted.params())?;
        let paged = first.ids.len();
        let (id, handle) = self.publish_first(template, &mut first)?;
        if paged == PAGE {
            let mut rest = Listed::default();
            let mut index = 0usize;
            self.visit(rows, sorted.pairs_sql(), sorted.params(), &mut |row| {
                if index >= PAGE {
                    rest.ids.push(row[0]);
                    if let Some(nid) = row.get(1) {
                        rest.nids.push(*nid);
                    }
                }
                index += 1;
                true
            })?;
            Self::append(&handle, rest);
        }
        Ok((id, handle))
    }

    /// One row of one integer: not 0 (`plan::ids::ANY_LONE_CARD`).
    fn any(&self, rows: &impl ReadRows, sql: &str) -> Result<bool, Stop> {
        Ok(self.list(rows, sql, &[])?.ids.first().is_some_and(|value| *value != 0))
    }

    /// H16: counts one predicate run.
    fn count_run(&self) {
        #[cfg(feature = "test-hooks")]
        self.shared
            .testing
            .runs
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// H16: counts one of H11's extra tests.
    fn count_extra_test(&self) {
        #[cfg(feature = "test-hooks")]
        self.shared
            .testing
            .extra_tests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// H8: the source's ids and set-aside cards sorted by `sort` in a new
    /// snapshot, without the predicate; its own clock for Due and
    /// Retrievability.
    fn resort(&self, source: Source, sort: &SortSpec) -> Result<(), Stop> {
        let sort =
            Sort::new(source.mode, &sort.key, sort.direction).map_err(|_| Stop::Failed(BrowseError::UnsortableColumn))?;
        self.with_snapshot(|collection, snapshot| {
            check_generation(snapshot, source.generation)?;
            let clock = context(collection, &**snapshot)?;
            let mut all = source.ids;
            all.extend(source.set_aside.iter().map(|(card, _)| *card));
            let template = Handle {
                mode: source.mode,
                sort: sort.clone(),
                text: source.text,
                context: source.context,
                reads: source.reads,
                sort_clock: clock.clone(),
                ids: Vec::new(),
                nids: Vec::new(),
                set_aside: Vec::new(),
                match_seq: source.match_seq,
                applied_seq: snapshot.seq(),
                generation: snapshot.generation(),
                revision: 0,
                match_data: source.match_data,
                complete: false,
                removed_while_running: Default::default(),
                prefix: 0,
            };
            self.sort_ids(snapshot, &all, &sort, &clock, template)
        })
    }

    /// Sorts `ids` of the template's mode in the snapshot (H8, H11): those
    /// that no longer exist are left out, and in cards mode a note-column
    /// sort sets the cards without a note row aside (S9).
    fn sort_ids(
        &self,
        snapshot: &crate::readers::Snapshot<'_>,
        ids: &[i64],
        sort: &Sort,
        clock: &SearchContext,
        template: Handle,
    ) -> Result<(), Stop> {
        let rows = &**snapshot;
        let sorted = SortedQuery::for_ids(ids, sort, clock).map_err(sort_error)?;
        let cards = template.mode == SearchMode::Cards;
        let (id, handle) = self.paged(rows, &sorted, template)?;
        let set_aside = if cards && !sorted.lists_lone_cards() {
            let lone = self.list(rows, ids::LONE_CARDS_AMONG, &[id_list(ids)])?;
            lone.ids.into_iter().zip(lone.nids).collect()
        } else {
            Vec::new()
        };
        lock(&handle).set_aside = set_aside;
        self.complete(id, &handle)
    }

    /// H11: the handle's text in the other mode, sorted by `sort`, with a
    /// context of its own. Derived from the handle without a predicate run
    /// when no op committed after its match seq and the context reads the
    /// same values; otherwise the query runs.
    fn toggle(&self, source: Source, sort: &SortSpec) -> Result<(), Stop> {
        let mode = match source.mode {
            SearchMode::Cards => SearchMode::Notes,
            SearchMode::Notes => SearchMode::Cards,
        };
        let query = meguri_search::parse(&source.text).map_err(|error| Stop::Failed(BrowseError::Search(error)))?;
        let sort = Sort::new(mode, &sort.key, sort.direction).map_err(|_| Stop::Failed(BrowseError::UnsortableColumn))?;
        self.with_snapshot(|collection, snapshot| {
            check_generation(snapshot, source.generation)?;
            let context = context(collection, &**snapshot)?;
            let notetypes = SearchNotetypes::read(&**snapshot)?;
            let plan = Plan::build(&query, mode, &context, &notetypes).map_err(build_error)?;
            let reads = source.reads.union(plan.reads());
            let same = snapshot.seq() == source.match_seq
                && reads.iter().all(|value| same_value(value, &source.context, &context));
            let derived = if same {
                self.derive(snapshot, &source, &query, &plan)?
            } else {
                None
            };
            let Some(derived) = derived else {
                return self.run_query(
                    snapshot,
                    QueryRun {
                        text: &source.text,
                        query: &query,
                        sort,
                        context,
                        notetypes: &notetypes,
                        plan,
                    },
                );
            };
            let template = Handle {
                mode,
                sort: sort.clone(),
                text: source.text.clone(),
                context: context.clone(),
                reads: plan.reads(),
                sort_clock: context.clone(),
                ids: Vec::new(),
                nids: Vec::new(),
                set_aside: Vec::new(),
                match_seq: snapshot.seq(),
                applied_seq: snapshot.seq(),
                generation: snapshot.generation(),
                revision: 0,
                match_data: derived.match_data,
                complete: false,
                removed_while_running: Default::default(),
                prefix: 0,
            };
            self.sort_ids(snapshot, &derived.ids, &sort, &context, template)
        })
    }

    /// H11's table: the set in the other mode, from the handle; `None` when
    /// the query must run.
    fn derive(
        &self,
        rows: &meguri_storage::ReadSnapshot<'_>,
        source: &Source,
        query: &meguri_search::Query,
        plan: &Plan,
    ) -> Result<Option<Derived>, Stop> {
        use meguri_search::Scope;
        let scope = meguri_search::plan::query_scope(query);
        let any = |sql: &str| -> Result<bool, Stop> {
            let found = self.list(rows, sql, &[])?;
            Ok(found.ids.first().is_some_and(|value| *value != 0))
        };
        Ok(match (source.mode, scope) {
            (_, Scope::Either) => None,
            (SearchMode::Cards, scope) => {
                // The handle's whole membership, set-aside cards included.
                let matched: Vec<(i64, i64)> = source
                    .ids
                    .iter()
                    .copied()
                    .zip(source.nids.iter().copied())
                    .chain(source.set_aside.iter().copied())
                    .collect();
                let mut seen = std::collections::HashSet::new();
                let mut notes: Vec<i64> = matched.iter().map(|(_, note)| *note).filter(|note| seen.insert(*note)).collect();
                let mut pairs = matched;
                // A sound collection has no card without a note row: every
                // matched card's note exists.
                if scope == Scope::Card && any(ids::ANY_LONE_CARD)? {
                    let existing: std::collections::HashSet<i64> =
                        self.list(rows, ids::EXISTING_NOTES, &[id_list(&notes)])?.ids.into_iter().collect();
                    notes.retain(|note| existing.contains(note));
                    pairs.retain(|(_, note)| existing.contains(note));
                }
                if scope == Scope::Note && any(ids::ANY_LONE_NOTE)? {
                    if let Some(sql) = plan.lone_rows_sql() {
                        self.count_extra_test();
                        notes.extend(
                            self.list(rows, &sql, plan.params())
                                .map_err(|stop| refused(stop, &source.text))?
                                .ids,
                        );
                    }
                }
                let match_data = (scope != Scope::Note).then(|| pairs.iter().map(|(card, _)| *card).collect());
                Some(Derived {
                    ids: notes,
                    match_data,
                })
            }
            (SearchMode::Notes, Scope::Note) => {
                let cards = self.list(rows, ids::CARDS_OF_NOTES, &[id_list(&source.ids)])?;
                Some(Derived {
                    ids: cards.ids,
                    match_data: None,
                })
            }
            (SearchMode::Notes, scope) => {
                let Some(pairs) = &source.match_data else { return Ok(None) };
                let mut cards: Vec<i64> = pairs.clone();
                if scope == Scope::Card && any(ids::ANY_LONE_CARD)? {
                    if let Some(sql) = plan.lone_rows_sql() {
                        self.count_extra_test();
                        cards.extend(
                            self.list(rows, &sql, plan.params())
                                .map_err(|stop| refused(stop, &source.text))?
                                .ids,
                        );
                    }
                }
                Some(Derived {
                    ids: cards,
                    match_data: None,
                })
            }
        })
    }
}

/// A plan's run, for [`Request::run_query`].
struct QueryRun<'a> {
    text: &'a str,
    query: &'a meguri_search::Query,
    sort: Sort,
    context: SearchContext,
    notetypes: &'a SearchNotetypes,
    plan: Plan,
}

/// A handle published by a run, with its match data.
type Published = (HandleId, Arc<Mutex<Handle>>, Option<Vec<i64>>);

/// H11: a toggle's set without a predicate run.
struct Derived {
    ids: Vec<i64>,
    match_data: Option<Vec<i64>>,
}

/// Q10 step 4's chunk of candidates, unless a test sets another (H16).
pub(super) const DEFAULT_CHUNK: usize = 500;
/// The largest chunk the doubling reaches.
pub(super) const MAX_CHUNK: usize = 32_768;
/// `engine/jobs` J5: at most one `Progress` per 100 ms.
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// Collections up to this many rows of the mode run a slow plan as one
/// statement, its first page from its `LIMIT` form. On the 150k fixture
/// that is faster than streaming, which sorts every candidate before its
/// first test. Larger collections stream.
pub(super) const STREAM_ROWS: u64 = 300_000;

/// Q10 "may": whether a plan with slow terms takes the two stages. Always
/// when a test set a chunk size (H16).
pub(super) fn streams(
    rows: &impl ReadRows,
    plan: &Plan,
    registry: Option<&super::SearchRegistry>,
    cancel: &dyn Fn() -> bool,
) -> Result<bool, Stop> {
    if !plan.is_slow() {
        return Ok(false);
    }
    if registry.and_then(super::SearchRegistry::stream_chunk).is_some() {
        return Ok(true);
    }
    let table = match plan.mode() {
        SearchMode::Cards => meguri_storage::Table::Cards,
        SearchMode::Notes => meguri_storage::Table::Notes,
    };
    // Counts at most one row past the threshold, under the request's
    // cancel test.
    let mut seen = 0u64;
    rows.visit_id_rows(
        &format!("SELECT 1 FROM {} LIMIT {}", table.name(), STREAM_ROWS + 1),
        &[],
        cancel,
        &mut |_| {
            seen += 1;
            true
        },
    )
    .map_err(|error| if cancel() { Stop::Cancelled } else { Stop::from(error) })?;
    Ok(seen > STREAM_ROWS)
}

/// Q8: whether two contexts hold the same `value`.
fn same_value(value: ContextValue, a: &SearchContext, b: &SearchContext) -> bool {
    match value {
        ContextValue::Now => a.now == b.now,
        ContextValue::Today => a.today == b.today,
        ContextValue::NextDayAt => a.next_day_at == b.next_day_at,
        ContextValue::LearnAhead => a.learn_ahead_secs == b.learn_ahead_secs,
        ContextValue::CurrentDeck => a.current_deck == b.current_deck,
        ContextValue::NormalizeNoteText => a.normalize_note_text == b.normalize_note_text,
        ContextValue::IgnoreAccents => a.ignore_accents == b.ignore_accents,
        ContextValue::Fold => a.fold == b.fold,
    }
}

/// A request on a handle reads the generation the handle was made in.
fn check_generation(snapshot: &crate::readers::Snapshot<'_>, planned: Generation) -> Result<(), Stop> {
    if snapshot.generation() == planned {
        Ok(())
    } else {
        Err(Stop::Failed(BrowseError::Engine(EngineError::StaleGeneration {
            planned,
            current: snapshot.generation(),
        })))
    }
}

fn build_error(BuildError::Search(error): BuildError) -> Stop {
    Stop::Failed(BrowseError::Search(error))
}

/// A database error of a search's own statement is `search_failed` over the
/// whole text (`search/errors` E3, E5).
fn refused(stop: Stop, text: &str) -> Stop {
    match stop {
        Stop::Failed(BrowseError::Engine(_)) => Stop::Failed(BrowseError::Search(refusal(text))),
        other => other,
    }
}

/// `search/errors` E3: the database refused the statement; the range is the
/// whole text (E5).
pub(super) fn refusal(text: &str) -> SearchError {
    SearchError::new(
        ErrorKind::SearchFailed(SearchFailure::DatabaseRefusal),
        0..text.chars().count(),
    )
}

fn sort_error(error: SortError) -> Stop {
    match error {
        SortError::ModeMismatch => Stop::Failed(BrowseError::Engine(EngineError::internal(
            "a sorted statement of the other mode",
        ))),
    }
}

pub(crate) fn cells(params: &[Param]) -> Vec<Cell> {
    params
        .iter()
        .map(|param| match param {
            Param::Integer(value) => Cell::Integer(*value),
            Param::Real(value) => Cell::Real(*value),
            Param::Text(value) => Cell::text(value.clone()),
        })
        .collect()
}

/// H9: the request's context, from the engine's clock and the config the
/// snapshot holds. The day is the pure day calculation (`sched/day-clock`
/// D3, D4): a search never runs the day query, which writes.
pub(crate) fn context(collection: &Collection, rows: &impl ReadRows) -> Result<SearchContext, EngineError> {
    let clock = collection.core.exec.clock.as_ref();
    let at = clock.now_ms();
    let config = SchedConfig::new(rows);
    let day = meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
        crt: rows.col()?.created.0,
        creation_offset: config.creation_offset()?,
        rollover_hour: config.rollover_hour()?,
        offset: clock.utc_offset_at(at),
        now: at.div_euclid(1000),
    });
    let flag = |key: &str, missing: bool| -> Result<bool, EngineError> {
        Ok(rows
            .config_value(key)?
            .and_then(|value| value.as_bool())
            .unwrap_or(missing))
    };
    Ok(SearchContext {
        now: at.div_euclid(1000),
        today: day.today,
        next_day_at: day.next_day_at,
        learn_ahead_secs: config.collapse_time()?,
        // `search/names-and-ids` N3.4: `curDeck` as stored, 1 when it does
        // not read as an integer.
        current_deck: meguri_model::ids::DeckId(
            rows.config_value("curDeck")?
                .and_then(|value| value.as_i64())
                .unwrap_or(1),
        ),
        normalize_note_text: flag("normalize_note_text", true)?,
        ignore_accents: flag("ignoreAccentsInSearch", false)?,
        fold: meguri_search::FoldMode::Unicode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(job: Job, cancelled: bool) -> QueuedJob {
        let token = CancelToken::new();
        if cancelled {
            token.cancel();
        }
        QueuedJob {
            seq: 7,
            token,
            epoch: 0,
            job,
        }
    }

    // H3, H4: a request no thread could run keeps its refusal over its
    // cancellation; others end Cancelled or Failed.
    #[test]
    fn an_unstarted_request_keeps_its_refusal_over_its_cancellation() {
        let refused = |event: &SearchEvent| {
            matches!(
                event,
                SearchEvent::Failed {
                    error: BrowseError::Engine(EngineError::CollectionUnavailable { .. }),
                    ..
                }
            )
        };
        assert!(refused(&unstarted(&job(Job::Refused, true), "no thread")));
        assert!(refused(&unstarted(&job(Job::Refused, false), "no thread")));
        assert!(matches!(
            unstarted(&job(Job::Superseded, true), "no thread"),
            SearchEvent::Cancelled { seq: RequestSeq(7) }
        ));
        let search = || Job::Search {
            text: String::new(),
            mode: SearchMode::Cards,
            sort: SortSpec::new("cardDue", meguri_search::config::SortDirection::Ascending),
        };
        assert!(matches!(
            unstarted(&job(search(), false), "no thread"),
            SearchEvent::Failed {
                error: BrowseError::Engine(EngineError::Internal { .. }),
                ..
            }
        ));
    }
}
