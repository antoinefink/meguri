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
    BuildError, ContextReads, ErrorKind, Param, Plan, SearchContext, SearchError, SearchFailure, SearchMode,
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
    /// Superseded while queued: it ends `Cancelled` without running.
    Superseded,
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
                    });
                }
                drop(outbox);
                let _ = delivery.join();
            }
            Err(error) => {
                shared.listener.event(if job.token.is_cancelled() {
                    SearchEvent::Cancelled { seq }
                } else {
                    SearchEvent::Failed {
                        seq,
                        error: BrowseError::Engine(EngineError::internal(format!(
                            "cannot start the search event thread: {error}"
                        ))),
                    }
                });
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

/// One request: its events, then nothing more of it.
fn run(shared: &Arc<SessionShared>, job: QueuedJob, outbox: &mpsc::Sender<SearchEvent>) {
    let request = Request {
        shared,
        seq: job.seq,
        token: &job.token,
        outbox,
        published: std::cell::Cell::new(None),
    };
    // Listed for halts from the start, so a halt that begins while the
    // request parses, or began after it was queued, cancels it (H3).
    let registry = shared.registry.upgrade();
    let _running = registry.as_ref().map(|registry| registry.run(&job.token, job.epoch));
    let result = if job.token.is_cancelled() {
        Err(Stop::Cancelled)
    } else {
        match job.job {
            Job::Search { text, mode, sort } => request.search(&text, mode, &sort),
            Job::Resort { source, sort } => request.resort(source, &sort),
            Job::Superseded => Err(Stop::Cancelled),
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
        Stop::Failed(error) => SearchEvent::Failed { seq, error },
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

    /// The statement of a search's own plan (H3 step 4): a database error
    /// is `search_failed` over the whole text (`search/errors` E3, E5).
    fn visit_search(
        &self,
        rows: &impl ReadRows,
        text: &str,
        sql: &str,
        params: &[Param],
        visit: &mut dyn FnMut(&[i64]) -> bool,
    ) -> Result<(), Stop> {
        let result = match self.visit(rows, sql, params, visit) {
            Err(Stop::Failed(BrowseError::Engine(_))) => Err(Stop::Failed(BrowseError::Search(refusal(text)))),
            other => other,
        };
        #[cfg(feature = "test-hooks")]
        if result.is_ok() {
            self.shared
                .testing
                .runs
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        result
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

    /// Publishes `handle` with `first` as its first page and prefix.
    fn publish_first(&self, mut handle: Handle, first: Listed) -> Result<(HandleId, Arc<Mutex<Handle>>), Stop> {
        handle.ids = first.ids;
        handle.nids = first.nids;
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

    /// One row of one integer: not 0 (`plan::ids::ANY_LONE_CARD`).
    fn any(&self, rows: &impl ReadRows, sql: &str) -> Result<bool, Stop> {
        Ok(self.list(rows, sql, &[])?.ids.first().is_some_and(|value| *value != 0))
    }

    /// H3: parse, check the sort, build the context and the plan, run the
    /// sorted statement in one snapshot.
    fn search(&self, text: &str, mode: SearchMode, sort: &SortSpec) -> Result<(), Stop> {
        // B1's order: a parse error, then `unsortable_column`, then the
        // run errors.
        let query = meguri_search::parse(text).map_err(|error| Stop::Failed(BrowseError::Search(error)))?;
        let sort = Sort::new(mode, &sort.key, sort.direction).map_err(|_| Stop::Failed(BrowseError::UnsortableColumn))?;
        let (_engine, collection) = self.collection()?;
        let registry = self.shared.registry.upgrade().ok_or(Stop::Cancelled)?;
        let slot = registry.reader(self.token).ok_or(Stop::Cancelled)?;
        let lease = collection.search_reader(slot.index)?;
        let snapshot = lease.snapshot()?;
        let context = context(&collection, &*snapshot)?;
        let notetypes = SearchNotetypes::read(&*snapshot)?;
        let plan = Plan::build(&query, mode, &context, &notetypes).map_err(|BuildError::Search(error)| {
            Stop::Failed(BrowseError::Search(error))
        })?;
        let sorted = SortedQuery::for_plan(&plan, &sort, &context).map_err(sort_error)?;
        let rows = &*snapshot;
        // The first page comes from the same statement with a LIMIT, which
        // SQLite orders with a bounded heap rather than a full sort: equal
        // rows, final, earlier (H3 budgets). The whole list then comes
        // from the full statement in the same snapshot.
        let mut first = Listed::default();
        self.visit_search(
            rows,
            text,
            &format!("{} LIMIT {PAGE}", sorted.pairs_sql()),
            sorted.params(),
            &mut |row| {
                first.ids.push(row[0]);
                if let Some(nid) = row.get(1) {
                    first.nids.push(*nid);
                }
                true
            },
        )?;
        let template = Handle {
            mode,
            sort,
            text: text.to_owned(),
            context: context.clone(),
            reads: plan.reads(),
            sort_clock: context,
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
        let paged = first.ids.len();
        let (id, handle) = self.publish_first(template, first)?;
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
            })
            .map_err(|stop| refused(stop, text))?;
            Self::append(&handle, rest);
        }
        // H5: under S9's note-column sorts the matched cards without a note
        // row are kept aside, not listed; a sound collection has none.
        let set_aside = match (mode == SearchMode::Cards && !sorted.lists_lone_cards(), plan.lone_rows_sql()) {
            (true, Some(sql)) if self.any(rows, ids::ANY_LONE_CARD)? => {
                let lone = self.list(rows, &sql, plan.params()).map_err(|stop| refused(stop, text))?;
                lone.ids.into_iter().zip(lone.nids).collect()
            }
            _ => Vec::new(),
        };
        // H11's match data: the cards of the matching card-and-note pairs.
        let match_data = match plan.pairs_sql() {
            Some(sql) => Some(self.list(rows, &sql, plan.params()).map_err(|stop| refused(stop, text))?.ids),
            None => None,
        };
        {
            let mut handle = lock(&handle);
            handle.set_aside = set_aside;
            handle.match_data = match_data;
        }
        self.complete(id, &handle)
    }

    /// H8: the source's ids and set-aside cards sorted by `sort` in a new
    /// snapshot, without the predicate; its own clock for Due and
    /// Retrievability.
    fn resort(&self, source: Source, sort: &SortSpec) -> Result<(), Stop> {
        let sort =
            Sort::new(source.mode, &sort.key, sort.direction).map_err(|_| Stop::Failed(BrowseError::UnsortableColumn))?;
        let (_engine, collection) = self.collection()?;
        let registry = self.shared.registry.upgrade().ok_or(Stop::Cancelled)?;
        let slot = registry.reader(self.token).ok_or(Stop::Cancelled)?;
        let lease = collection.search_reader(slot.index)?;
        let snapshot = lease.snapshot()?;
        if snapshot.generation() != source.generation {
            return Err(Stop::Failed(BrowseError::Engine(EngineError::StaleGeneration {
                planned: source.generation,
                current: snapshot.generation(),
            })));
        }
        let clock = context(&collection, &*snapshot)?;
        let mut all = source.ids;
        all.extend(source.set_aside.iter().map(|(card, _)| *card));
        let sorted = SortedQuery::for_ids(&all, &sort, &clock).map_err(sort_error)?;
        let mut listed = self.list(&*snapshot, sorted.pairs_sql(), sorted.params())?;
        let set_aside = if source.mode == SearchMode::Cards && !sorted.lists_lone_cards() {
            let lone = self.list(&*snapshot, ids::LONE_CARDS_AMONG, &[id_list(&all)])?;
            lone.ids.into_iter().zip(lone.nids).collect()
        } else {
            Vec::new()
        };
        let template = Handle {
            mode: source.mode,
            sort,
            text: source.text,
            context: source.context,
            reads: source.reads,
            sort_clock: clock,
            ids: Vec::new(),
            nids: Vec::new(),
            set_aside,
            match_seq: source.match_seq,
            applied_seq: snapshot.seq(),
            generation: snapshot.generation(),
            revision: 0,
            match_data: source.match_data,
            complete: false,
            removed_while_running: Default::default(),
            prefix: 0,
        };
        let first = Listed {
            ids: listed.ids.drain(..listed.ids.len().min(PAGE)).collect(),
            nids: listed.nids.drain(..listed.nids.len().min(PAGE)).collect(),
        };
        let (id, handle) = self.publish_first(template, first)?;
        Self::append(&handle, listed);
        self.complete(id, &handle)
    }
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
        SortError::FieldColumnNotYetSupported { name } => Stop::Failed(BrowseError::Engine(EngineError::Unsupported {
            feature: format!("the sort key of field column {name:?} (U261)"),
        })),
        SortError::ModeMismatch => Stop::Failed(BrowseError::Engine(EngineError::internal(
            "a sorted statement of the other mode",
        ))),
    }
}

pub(super) fn cells(params: &[Param]) -> Vec<Cell> {
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
pub(super) fn context(collection: &Collection, rows: &impl ReadRows) -> Result<SearchContext, EngineError> {
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
