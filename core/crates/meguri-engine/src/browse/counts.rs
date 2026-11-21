//! Browse's filter counts (`specs/search/filter-tokens.md` T12.4): for
//! each listed value, the number of rows of a mode its built item finds,
//! read off the main thread from one snapshot and cached by change seq.
//!
//! - **Grouped reads.** meguri-search's [`CountPlan`] sorts the run texts
//!   into jobs: one read of the cards' decks for every deck item, one read
//!   of every note's note type and tags for every tag and note type item,
//!   one statement per row set with one count column per other value, and
//!   a count-only search for each saved search or whole query. Never one
//!   search per value.
//! - **A read.** The call takes one of the search workers' readers through
//!   the search registry, so a replacement, a suspend or a close cancels it
//!   as it cancels a search (`browser/result-sets` H3), and its day is the
//!   pure day calculation (`worker::context`): it never writes.
//! - **Cancellation.** A newer call cancels the one running, and so does
//!   the caller's token; a cancelled call returns `Cancelled`, and the jobs
//!   it finished stay cached.
//! - **Cache.** Each count is kept with the seq and generation of the
//!   snapshot it was read on, the tables it read and the context values its
//!   statement read. The provider observes every commit (a change
//!   observer): a change set that touches cards, notes, decks, note types,
//!   presets or the review log marks those tables changed at its seq, and a
//!   count read before that seq on a table it read is read again. A
//!   whole-collection change marks every table. Before a lookup the call
//!   waits for the observers of every commit its snapshot holds.
//!
//! The provider caches only for the collection it was opened with (its open
//! step); without it every call reads afresh.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_model::changes::ChangeSet;
use meguri_search::filter_tokens::counts::{CountItem, CountJob, CountPlan, CountReads, CountText, NoteIndex, Tables};
use meguri_search::name_index::NameIndex;
use meguri_search::{ContextValue, SearchContext, SearchMode, SearchNotetypes};
use meguri_storage::ReadRows;

use crate::cancel::CancelToken;
use crate::changes::observers::ObservedCommit;
use crate::changes::stream::Seq;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::lifecycle::{Engine, Generation};
use crate::ports::{ChangeObserver, GenerationListener, OpenStep};

use super::search::{BrowseError, bridge, cells, context};

const NAME: &str = "filter-counts";

/// The tables a change set can touch, as [`Tables`] bits.
const TABLES: [Tables; 6] = [
    Tables::CARDS,
    Tables::NOTES,
    Tables::DECKS,
    Tables::NOTETYPES,
    Tables::DECK_CONFIG,
    Tables::REVLOG,
];

/// Holds the filter-count cache of the open collection.
#[derive(Default)]
pub struct FilterCounts {
    state: Mutex<State>,
    /// The running call's token: a newer call cancels it.
    running: Mutex<Option<Arc<CancelToken>>>,
    #[cfg(feature = "test-hooks")]
    statements: std::sync::atomic::AtomicU64,
}

#[derive(Default)]
struct State {
    /// The generation the provider observes (set by its open step and a
    /// reopen); `None` keeps nothing.
    attached: Option<Generation>,
    /// Per table of [`TABLES`]: the seq of the last commit that touched it.
    changed: [Seq; 6],
    /// One more at every reset: a call that read before a reset keeps
    /// nothing it reads after it (the reset forgot the commits between).
    epoch: u64,
    entries: HashMap<(SearchMode, String), Entry>,
}

#[derive(Clone)]
struct Entry {
    count: u64,
    seq: Seq,
    generation: Generation,
    reads: CountReads,
    context: SearchContext,
}

impl State {
    fn reset(&mut self, generation: Option<Generation>) {
        self.attached = generation;
        self.changed = [Seq::default(); 6];
        self.entries.clear();
        self.epoch += 1;
    }

    /// A count still holds on a snapshot of `generation` at `seq` with
    /// `ctx`: no commit since it was read touched a table it read, and the
    /// context values its statement read are the same.
    fn current(&self, entry: &Entry, generation: Generation, seq: Seq, ctx: &SearchContext) -> bool {
        entry.generation == generation
            && entry.seq <= seq
            && TABLES
                .iter()
                .zip(self.changed.iter())
                .all(|(table, changed)| !entry.reads.tables.intersects(*table) || *changed <= entry.seq)
            && same_reads(entry.reads, &entry.context, ctx)
    }
}

/// The values of `reads.context` are equal in `a` and `b`.
fn same_reads(reads: CountReads, a: &SearchContext, b: &SearchContext) -> bool {
    reads.context.iter().all(|value| match value {
        ContextValue::Now => a.now == b.now,
        ContextValue::Today => a.today == b.today,
        ContextValue::NextDayAt => a.next_day_at == b.next_day_at,
        ContextValue::LearnAhead => a.learn_ahead_secs == b.learn_ahead_secs,
        ContextValue::CurrentDeck => a.current_deck == b.current_deck,
        ContextValue::NormalizeNoteText => a.normalize_note_text == b.normalize_note_text,
        ContextValue::IgnoreAccents => a.ignore_accents == b.ignore_accents,
        ContextValue::Fold => a.fold == b.fold,
    })
}

/// The tables a change set touches.
fn touched(changes: &ChangeSet) -> Tables {
    if changes.whole_collection {
        return Tables::ALL;
    }
    let mut tables = Tables::default();
    let mut mark = |touched: bool, table: Tables| {
        if touched {
            tables = tables.union(table);
        }
    };
    mark(!changes.cards.is_empty(), Tables::CARDS);
    mark(!changes.notes.is_empty(), Tables::NOTES);
    mark(!changes.decks.is_empty(), Tables::DECKS);
    mark(!changes.notetypes.is_empty(), Tables::NOTETYPES);
    mark(!changes.deck_configs.is_empty(), Tables::DECK_CONFIG);
    mark(
        !changes.revlog_added.is_empty() || !changes.revlog_removed.is_empty(),
        Tables::REVLOG,
    );
    tables
}

/// Clears the running slot when the call ends, unless a newer call took
/// it.
struct Running<'a> {
    counts: &'a FilterCounts,
    token: Arc<CancelToken>,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut running = self.counts.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.as_ref().is_some_and(|token| Arc::ptr_eq(token, &self.token)) {
            *running = None;
        }
    }
}

impl FilterCounts {
    pub fn new() -> FilterCounts {
        FilterCounts::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Test-only: how many statements the calls ran (a cached count runs
    /// none).
    #[cfg(feature = "test-hooks")]
    pub fn statements_for_testing(&self) -> u64 {
        self.statements.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn ran_statement(&self) {
        #[cfg(feature = "test-hooks")]
        self.statements.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// T12.4: for each item, the number of rows of `mode` its run text finds
    /// (`search/query-model` Q4), or `None` when it cannot be counted (it
    /// has no run text, or its search does not plan or fails). The run
    /// texts of saved searches and queries read the names in `names`, the
    /// index Browse reads them with. Blocks; call it off the main thread at
    /// a lower priority than searches. A newer call, `cancel`, or a
    /// replacement, suspend or close cancels it: `Cancelled`.
    pub fn filter_counts(
        &self,
        engine: &Engine,
        names: &NameIndex,
        mode: SearchMode,
        items: &[CountItem],
        cancel: Option<&CancelToken>,
    ) -> Result<Vec<Option<u64>>, BrowseError> {
        let token = CancelToken::new();
        {
            let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(older) = running.replace(Arc::clone(&token)) {
                older.cancel();
                // An older call waiting for a reader sees its token now.
                engine.inner.searches.wake();
            }
        }
        let _running = Running {
            counts: self,
            token: Arc::clone(&token),
        };
        let registry = &engine.inner.searches;
        if let Some(caller) = cancel {
            bridge(caller, &token, registry);
        }
        // The distinct run texts, each with the items that count it.
        let mut texts: Vec<(String, bool)> = Vec::new();
        let mut of_item: Vec<Option<usize>> = Vec::with_capacity(items.len());
        let mut by_text: HashMap<String, usize> = HashMap::new();
        for item in items {
            let Some(text) = item.run_text(names) else {
                of_item.push(None);
                continue;
            };
            let at = *by_text.entry(text.clone()).or_insert_with(|| {
                texts.push((text, false));
                texts.len() - 1
            });
            texts[at].1 |= item.runs_alone();
            of_item.push(Some(at));
        }
        let counted = self.count_texts(engine, mode, &texts, &token)?;
        // Superseded at the very end: the counts read stay cached, but the
        // caller hears it was cancelled.
        if token.is_cancelled() {
            return Err(BrowseError::Engine(EngineError::Cancelled));
        }
        Ok(of_item.into_iter().map(|at| at.and_then(|at| counted[at])).collect())
    }

    fn count_texts(
        &self,
        engine: &Engine,
        mode: SearchMode,
        texts: &[(String, bool)],
        token: &Arc<CancelToken>,
    ) -> Result<Vec<Option<u64>>, BrowseError> {
        let cancelled = || BrowseError::Engine(EngineError::Cancelled);
        let registry = &engine.inner.searches;
        let (epoch, stopping) = registry.epoch();
        if stopping {
            return Err(BrowseError::Engine(EngineError::unavailable(
                "the collection is changing generation, suspending or closing",
            )));
        }
        let _listed = registry.run(token, epoch);
        let collection = engine.collection()?;
        let slot = registry.reader(token).ok_or_else(cancelled)?;
        let lease = collection.search_reader(slot.index)?;
        if token.is_cancelled() {
            return Err(cancelled());
        }
        let snapshot = lease.snapshot()?;
        let ctx = context(&collection, &*snapshot)?;
        // Every commit the snapshot holds has been observed: a count it
        // changed is no longer current.
        collection.core.exec.stream.wait_settled(snapshot.seq());
        let (generation, seq) = (snapshot.generation(), snapshot.seq());
        let mut out: Vec<Option<u64>> = vec![None; texts.len()];
        let mut missing: Vec<usize> = Vec::new();
        let epoch = {
            let mut state = self.lock();
            // The cache is bounded: past 65,536 counts it starts again.
            if state.attached != Some(generation) || state.entries.len() > 1 << 16 {
                state.entries.clear();
            }
            for (k, (text, _)) in texts.iter().enumerate() {
                match state.entries.get(&(mode, text.clone())) {
                    Some(entry) if state.current(entry, generation, seq, &ctx) => out[k] = Some(entry.count),
                    _ => missing.push(k),
                }
            }
            state.epoch
        };
        if missing.is_empty() {
            return Ok(out);
        }
        let notetypes = SearchNotetypes::read(&*snapshot)?;
        let wanted: Vec<CountText<'_>> = missing
            .iter()
            .map(|k| CountText {
                text: &texts[*k].0,
                alone: texts[*k].1,
            })
            .collect();
        let plan = CountPlan::new(mode, &wanted, &ctx, &notetypes);
        let stop = || token.is_cancelled();
        // Notes mode: the notes that exist, from the note read when there is
        // one (it comes first), or read for the jobs that need them.
        let mut notes: Option<NoteIndex> = None;
        let mut jobs: std::collections::VecDeque<CountJob> = plan.jobs.into();
        while let Some(job) = jobs.pop_front() {
            if stop() {
                return Err(cancelled());
            }
            let reads = job.reads();
            let shared = job.is_shared();
            let found = match self.run_job(&*snapshot, job, &mut notes, &stop) {
                Ok(found) => found,
                Err(_) if stop() => return Err(cancelled()),
                // A shared statement the database refuses: each of its texts
                // is counted alone on the same snapshot.
                Err(_) if shared => {
                    let indexes: Vec<usize> = reads.iter().map(|(text, _)| *text).collect();
                    jobs.extend(CountPlan::alone(mode, &wanted, &indexes, &ctx, &notetypes));
                    continue;
                }
                // A statement the database refuses: its counts stay unknown.
                Err(error) => {
                    tracing::warn!(%error, "a filter count statement failed");
                    continue;
                }
            };
            let reads: HashMap<usize, CountReads> = reads.into_iter().collect();
            let mut state = self.lock();
            let keep = state.attached == Some(generation) && state.epoch == epoch;
            for (local, count) in found {
                let k = missing[local];
                out[k] = Some(count);
                if keep && let Some(reads) = reads.get(&local) {
                    state.entries.insert(
                        (mode, texts[k].0.clone()),
                        Entry {
                            count,
                            seq,
                            generation,
                            reads: *reads,
                            context: ctx.clone(),
                        },
                    );
                }
            }
        }
        Ok(out)
    }

    /// One job's counts, by its texts' indexes in the plan.
    fn run_job(
        &self,
        snapshot: &impl ReadRows,
        job: CountJob,
        notes: &mut Option<NoteIndex>,
        stop: &dyn Fn() -> bool,
    ) -> Result<Vec<(usize, u64)>, meguri_storage::StorageError> {
        let count = |value: i64| u64::try_from(value).unwrap_or(0);
        if job.needs_notes() && notes.is_none() {
            let mut read = NoteIndex::default();
            self.ran_statement();
            snapshot.visit_count_rows("SELECT n.id FROM notes n", &[], stop, &mut |row| {
                read.insert(row[0]);
                true
            })?;
            *notes = Some(read);
        }
        let none = NoteIndex::default();
        match job {
            CountJob::Decks(mut decks) => {
                let names = snapshot.deck_names()?;
                decks.decks(names.iter().map(|(id, name)| (*id, name.as_deref())));
                self.ran_statement();
                let sql = decks.sql();
                let existing = notes.as_ref().unwrap_or(&none);
                snapshot.visit_count_rows(sql, &[], stop, &mut |row| {
                    decks.row(row, existing);
                    true
                })?;
                Ok(decks.finish())
            }
            CountJob::Notes(mut job) => {
                let notetypes = snapshot.notetype_names()?;
                job.notetypes(notetypes.iter().map(|(id, name)| (*id, name.as_deref())));
                if let Some(sql) = job.cards_sql() {
                    self.ran_statement();
                    snapshot.visit_count_rows(sql, &[], stop, &mut |row| {
                        job.cards_row(row);
                        true
                    })?;
                }
                self.ran_statement();
                snapshot.visit_note_tags(stop, &mut |nid, mid, text| {
                    job.note(nid, mid, text);
                    true
                })?;
                *notes = Some(job.take_notes());
                Ok(job.finish())
            }
            CountJob::NoteColumns(mut columns) => {
                self.ran_statement();
                let params = cells(&columns.params);
                let sql = columns.sql.clone();
                let existing = notes.as_ref().unwrap_or(&none);
                snapshot.visit_count_rows(&sql, &params, stop, &mut |row| {
                    columns.row(row, existing);
                    true
                })?;
                Ok(columns.finish())
            }
            CountJob::Columns { sql, params, texts, .. } => {
                self.ran_statement();
                let mut found = Vec::new();
                snapshot.visit_count_rows(&sql, &cells(&params), stop, &mut |row| {
                    found = texts.iter().copied().zip(row.iter().map(|v| count(*v))).collect();
                    false
                })?;
                Ok(found)
            }
            CountJob::Search { sql, params, text, .. } => {
                self.ran_statement();
                let mut found = Vec::new();
                snapshot.visit_count_rows(&sql, &cells(&params), stop, &mut |row| {
                    found = vec![(text, count(row[0]))];
                    false
                })?;
                Ok(found)
            }
        }
    }

    fn attach(&self, generation: Generation) {
        self.lock().reset(Some(generation));
    }
}

impl OpenStep for FilterCounts {
    fn name(&self) -> &str {
        NAME
    }
    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        self.attach(collection.generation());
        Ok(())
    }
}

impl ChangeObserver for FilterCounts {
    fn name(&self) -> &str {
        NAME
    }
    /// Marks the tables the commit touched; never reads.
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        let tables = touched(commit.changes);
        let mut state = self.lock();
        if state.attached != Some(commit.generation) {
            return Ok(());
        }
        for (k, table) in TABLES.iter().enumerate() {
            if tables.intersects(*table) && state.changed[k] < commit.seq {
                state.changed[k] = commit.seq;
            }
        }
        Ok(())
    }
}

impl GenerationListener for FilterCounts {
    fn name(&self) -> &str {
        NAME
    }
    fn collection_reopened(&self, collection: &Collection) -> Result<(), EngineError> {
        self.attach(collection.generation());
        Ok(())
    }
    fn collection_replaced(&self, _old: Generation, new: Generation) {
        self.attach(new);
    }
}

/// T12.4's count of a deck page's Suspended Cards row (plan decision 18):
/// the deck with its subdecks and Card State: Suspended, as one query.
pub fn deck_suspended(names: &NameIndex, deck: meguri_model::ids::DeckId) -> Option<CountItem> {
    use meguri_search::filter_tokens::{DeckValue, FilterQuery, StateValue, Token, Value, ValueToken};
    let deck = DeckValue::of(names, deck)?;
    Some(CountItem::Query(FilterQuery::new(
        vec![
            Token::Value(ValueToken::new(vec![Value::Deck(deck)], false)),
            Token::Value(ValueToken::new(vec![Value::State(StateValue::Suspended)], false)),
        ],
        "",
    )))
}
