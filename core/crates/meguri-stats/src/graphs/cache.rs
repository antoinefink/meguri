//! The column caches as a derived-store component, `stats_columns`
//! (`stats/graphs-common` G9; `storage/derived-store` D4).
//!
//! - Content: two column files beside the store (the card copy and the
//!   review-log copy), named by the record table `files` (name, kind,
//!   length, checksum, base seq). A file is mapped only while a row names it
//!   and its length and checksum match (G9.3).
//! - Updates: each commit's change goes into an overlay in memory, kept with
//!   its seq; a request reads the base its snapshot's rows name plus the
//!   entries after the base's seq and at or before its own (G9.3).
//! - Persist (suspend, close, release) merges the overlay into new files
//!   and replaces the record rows; release then drops the maps and the
//!   overlay (G9.5).
//! - Held at each open until `homeIdle` or a `request_refresh` (G9.2).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use meguri_engine::changes::observers::RowAfter;
use meguri_engine::derived_host::{
    BuildSink, Capture, ComponentHandle, ComponentSchema, ComponentStore, ComponentTables, DerivedComponent, TableDef,
};
use meguri_engine::{EngineError, ObservedCommit, ObserverRows, Snapshot};
use meguri_model::changes::ChangeKind;
use meguri_model::ids::CardId;
use meguri_storage::derived::MappedFile;
use meguri_storage::{Cell, ReadRows, RowImage, RowKey, Table};

use super::columns::{Backing, CardTable, CardsBuilder, MappedBytes, RevlogBuilder, RevlogTable, checksum};
use super::overlay::{CardKey, Change, Entry, Patch, View};
use super::rows::{self, RowRead};

/// The component's name (D4).
pub const COMPONENT: &str = "stats_columns";
/// Its version: a change rebuilds it.
const VERSION: u32 = 1;
const FILES: &str = "files";
const KIND_CARDS: i64 = 1;
const KIND_REVLOG: i64 = 2;
/// The host signal that releases the build's hold (G9.2).
pub const HOME_IDLE: &str = "homeIdle";
/// Rows between two cancellation checks and progress reports of a build.
const CHECK_EVERY: u64 = 8_192;

fn internal(message: impl Into<String>) -> EngineError {
    EngineError::Internal {
        message: message.into(),
    }
}

/// One record row (G9.3).
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileRecord {
    name: String,
    kind: i64,
    length: i64,
    checksum: i64,
    base_seq: i64,
}

impl FileRecord {
    fn from_cells(cells: &[Cell]) -> Option<FileRecord> {
        let int = |at: usize| match cells.get(at) {
            Some(Cell::Integer(value)) => Some(*value),
            _ => None,
        };
        let name = match cells.first() {
            Some(Cell::Text(bytes)) => String::from_utf8(bytes.clone()).ok()?,
            _ => return None,
        };
        Some(FileRecord {
            name,
            kind: int(1)?,
            length: int(2)?,
            checksum: int(3)?,
            base_seq: int(4)?,
        })
    }
}

/// The two records naming one base, when the rows hold exactly one of each.
fn base_records(records: Vec<FileRecord>) -> Option<(FileRecord, FileRecord)> {
    let mut cards = records.iter().filter(|r| r.kind == KIND_CARDS);
    let mut revlog = records.iter().filter(|r| r.kind == KIND_REVLOG);
    let (card_file, revlog_file) = (cards.next()?, revlog.next()?);
    if cards.next().is_some() || revlog.next().is_some() || records.len() != 2 {
        return None;
    }
    Some((card_file.clone(), revlog_file.clone()))
}

#[derive(Default)]
struct State {
    /// The overlay, in seq order (G9.3).
    entries: Vec<Entry>,
    /// Per generation, the seq at or before which entries were dropped.
    pruned: HashMap<u64, u64>,
    /// Files this process wrote: their generation and base seq. A file it
    /// did not write holds every commit before this process bound the store
    /// (its component was fresh at bind, D5.2), so every entry of the
    /// generation applies over it.
    written: HashMap<String, (u64, u64)>,
    cards: HashMap<String, Arc<CardTable>>,
    revlog: HashMap<String, Arc<RevlogTable>>,
    /// Advanced by each release: a map begun before it is not kept.
    epoch: u64,
    next_name: u64,
}

/// What the component and the requests share.
#[derive(Default)]
pub(crate) struct Shared {
    state: Mutex<State>,
    /// The component's handle to its derived writer (D1, G9.3).
    handle: Mutex<Option<ComponentHandle>>,
    /// Every file mapped, while any map of it lives (a request's or the
    /// cache's): the derived writer keeps these (D7).
    /// One name can have several maps (a release, then a new map, while a
    /// request keeps the old one): each counts.
    open: Mutex<HashMap<String, Vec<Weak<MappedBytes>>>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Maps the file a record names, checking its length and checksum
    /// (G9.3), or takes the map already held.
    fn handle(&self) -> Option<ComponentHandle> {
        self.handle.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Maps the file a record names, checking its length and checksum
    /// (G9.3). Once no map of it is left, the derived writer is asked to
    /// sweep: it deletes the file if no row names it (D7).
    fn map(self: &Arc<Shared>, dir: &Path, record: &FileRecord) -> Result<Arc<MappedBytes>, EngineError> {
        let owner = Arc::downgrade(self);
        let mapped = Arc::new(MappedBytes {
            file: map(dir, record)?,
            on_drop: Some(Box::new(move || {
                if let Some(handle) = owner.upgrade().and_then(|shared| shared.handle()) {
                    handle.sweep();
                }
            })),
        });
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(record.name.clone())
            .or_default()
            .push(Arc::downgrade(&mapped));
        Ok(mapped)
    }

    /// The files `live` does not name that a map still holds (D7). The
    /// cache's own maps of them go first, outside the state lock.
    fn files_in_use(&self, live: &HashSet<String>) -> HashSet<String> {
        let superseded = {
            let mut state = self.lock();
            let cards: Vec<Arc<CardTable>> = take_unnamed(&mut state.cards, live);
            let revlog: Vec<Arc<RevlogTable>> = take_unnamed(&mut state.revlog, live);
            (cards, revlog)
        };
        drop(superseded);
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.retain(|_, maps| {
            maps.retain(|mapped| mapped.strong_count() > 0);
            !maps.is_empty()
        });
        open.keys().filter(|name| !live.contains(*name)).cloned().collect()
    }

    fn card_table(self: &Arc<Shared>, dir: &Path, record: &FileRecord) -> Result<Arc<CardTable>, EngineError> {
        let epoch = {
            let state = self.lock();
            if let Some(table) = state.cards.get(&record.name) {
                return Ok(Arc::clone(table));
            }
            state.epoch
        };
        let table = Arc::new(
            CardTable::open(Backing::Mapped(self.map(dir, record)?))
                .map_err(|e| internal(format!("the card copy {}: {}", record.name, e.0)))?,
        );
        let mut state = self.lock();
        if state.epoch == epoch {
            state.cards.insert(record.name.clone(), Arc::clone(&table));
        }
        Ok(table)
    }

    fn revlog_table(self: &Arc<Shared>, dir: &Path, record: &FileRecord) -> Result<Arc<RevlogTable>, EngineError> {
        let epoch = {
            let state = self.lock();
            if let Some(table) = state.revlog.get(&record.name) {
                return Ok(Arc::clone(table));
            }
            state.epoch
        };
        let table = Arc::new(
            RevlogTable::open(Backing::Mapped(self.map(dir, record)?))
                .map_err(|e| internal(format!("the review-log copy {}: {}", record.name, e.0)))?,
        );
        let mut state = self.lock();
        if state.epoch == epoch {
            state.revlog.insert(record.name.clone(), Arc::clone(&table));
        }
        Ok(table)
    }

    /// The seq the base of `records` holds, for `generation`: the one this
    /// process wrote it with, else 0 (every entry of the generation).
    fn base_seq(&self, generation: u64, cards: &FileRecord, revlog: &FileRecord) -> u64 {
        let state = self.lock();
        let of = |record: &FileRecord| {
            state
                .written
                .get(&record.name)
                .filter(|(g, _)| *g == generation)
                .map(|(_, seq)| *seq)
        };
        match (of(cards), of(revlog)) {
            (Some(a), Some(b)) => a.min(b),
            _ => 0,
        }
    }

    /// The patch of the entries of `generation` after `base` and at or
    /// before `through`; `None` when some of them were already dropped.
    fn patch(&self, generation: u64, base: u64, through: u64) -> Option<Patch> {
        let state = self.lock();
        if state.pruned.get(&generation).is_some_and(|pruned| *pruned > base) {
            return None;
        }
        let mut patch = Patch::default();
        for entry in &state.entries {
            if entry.generation == generation && entry.seq > base && entry.seq <= through {
                patch.apply(&entry.change);
            }
        }
        Some(patch)
    }

    /// The view of the copies for `snapshot`, which `is_current` found
    /// current; `None` when they cannot serve it (no base, a file that does
    /// not match its row, entries already dropped).
    pub(crate) fn view(self: &Arc<Shared>, snapshot: &Snapshot<'_>, dir: &Path) -> Option<View> {
        let rows = match snapshot.derived_rows(COMPONENT, FILES, "name, kind, length, checksum, base_seq") {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "reading the stats column records");
                return None;
            }
        };
        let records: Option<Vec<FileRecord>> = rows.iter().map(|row| FileRecord::from_cells(row)).collect();
        let (card_file, revlog_file) = base_records(records?)?;
        let tables = self
            .card_table(dir, &card_file)
            .and_then(|cards| Ok((cards, self.revlog_table(dir, &revlog_file)?)));
        let (cards, revlog) = match tables {
            Ok(tables) => tables,
            Err(error) => {
                // A file of the base these rows name is damaged or gone: a
                // rebuild (G9.3), which the writer queues only while these
                // are still the live rows (not a base a swap replaced).
                tracing::warn!(%error, "mapping the stats column files");
                if let Some(handle) = self.handle() {
                    handle.rebuild_if_named(vec![card_file.name.clone(), revlog_file.name.clone()]);
                }
                return None;
            }
        };
        self.forget_unused(&[&card_file.name, &revlog_file.name]);
        let generation = snapshot.generation().0;
        let base = self.base_seq(generation, &card_file, &revlog_file);
        let patch = self.patch(generation, base, snapshot.seq().0)?;
        Some(View {
            cards,
            revlog,
            patch: Arc::new(patch),
        })
    }

    /// Unmaps the files a swap or a merge superseded that no running
    /// request holds (D7); the derived writer deletes them at its next
    /// persist or bind.
    fn forget_unused(&self, live: &[&str]) {
        let mut state = self.lock();
        state
            .cards
            .retain(|name, table| live.contains(&name.as_str()) || Arc::strong_count(table) > 1);
        state
            .revlog
            .retain(|name, table| live.contains(&name.as_str()) || Arc::strong_count(table) > 1);
    }

    /// A new file name: kind, generation, seq and a counter.
    fn new_name(&self, kind: &str, generation: u64, seq: u64) -> String {
        let mut state = self.lock();
        state.next_name += 1;
        format!("{kind}-{generation}-{seq}-{}.col", state.next_name)
    }

    /// Writes an image under a new name; returns its record.
    fn write_file(
        &self,
        dir: &Path,
        kind: i64,
        image: &[u8],
        generation: u64,
        base_seq: u64,
    ) -> Result<FileRecord, EngineError> {
        let label = if kind == KIND_CARDS { "cards" } else { "revlog" };
        let mut attempts = 0;
        let name = loop {
            let name = self.new_name(label, generation, base_seq);
            match meguri_storage::derived::files::write_new(dir, &name, image) {
                Ok(_) => break name,
                Err(_) if attempts < 3 && dir.join(&name).exists() => attempts += 1,
                Err(error) => return Err(error.into()),
            }
        };
        self.lock().written.insert(name.clone(), (generation, base_seq));
        Ok(FileRecord {
            name,
            kind,
            length: i64::try_from(image.len()).map_err(|_| internal("a column file over i64"))?,
            checksum: checksum(image),
            base_seq: i64::try_from(base_seq).unwrap_or(i64::MAX),
        })
    }
}

/// Takes out of `maps` the entries whose name `live` does not hold.
fn take_unnamed<T>(maps: &mut HashMap<String, Arc<T>>, live: &HashSet<String>) -> Vec<Arc<T>> {
    let names: Vec<String> = maps.keys().filter(|name| !live.contains(*name)).cloned().collect();
    names.into_iter().filter_map(|name| maps.remove(&name)).collect()
}

fn map(dir: &Path, record: &FileRecord) -> Result<MappedFile, EngineError> {
    // SAFETY: column files are written whole once (`write_new`, a new name
    // each time) and only ever read or deleted after that (G9.3).
    let file = unsafe { MappedFile::open(&dir.join(&record.name)) }?;
    let bytes = file.bytes();
    if i64::try_from(bytes.len()).ok() != Some(record.length) || checksum(bytes) != record.checksum {
        return Err(internal(format!(
            "the column file {} does not match its record",
            record.name
        )));
    }
    Ok(file)
}

fn replace_records(tables: &ComponentTables<'_>, records: &[FileRecord]) -> Result<(), EngineError> {
    let table = tables.table(FILES)?;
    let conn = tables.conn();
    let sql = |error: meguri_storage::rusqlite::Error| internal(error.to_string());
    conn.execute(&format!("DELETE FROM \"{table}\""), []).map_err(sql)?;
    for record in records {
        conn.execute(
            &format!("INSERT INTO \"{table}\" (name, kind, length, checksum, base_seq) VALUES (?1, ?2, ?3, ?4, ?5)"),
            (&record.name, record.kind, record.length, record.checksum, record.base_seq),
        )
        .map_err(sql)?;
    }
    Ok(())
}

fn read_records(tables: &ComponentTables<'_>) -> Result<Vec<FileRecord>, EngineError> {
    let table = tables.table(FILES)?;
    let sql = |error: meguri_storage::rusqlite::Error| internal(error.to_string());
    let mut statement = tables
        .conn()
        .prepare(&format!("SELECT name, kind, length, checksum, base_seq FROM \"{table}\""))
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok(FileRecord {
                name: row.get(0)?,
                kind: row.get(1)?,
                length: row.get(2)?,
                checksum: row.get(3)?,
                base_seq: row.get(4)?,
            })
        })
        .map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

/// The copies' content from a view: every card and every row, as the
/// files of a new base hold them.
fn rebase(view: &View) -> (CardsBuilder, RevlogBuilder) {
    let mut cards = CardsBuilder::default();
    for (_, card) in view.cards() {
        cards.push(card);
    }
    let slots = cards.slots();
    let slot_of = |id: i64| slots.get(&id).copied();
    let patch = &view.patch;
    // The base's unreadable rows with their card now, and the patch's rows:
    // merged by id with the base's readable rows.
    let mut others: Vec<RowRead> = view
        .revlog
        .bad()
        .iter()
        .filter(|bad| !patch.hidden.contains(&bad.id))
        .map(|bad| {
            RowRead::Bad(rows::BadRow {
                cid: patch.attach.get(&bad.id).copied().or(bad.cid),
                ..*bad
            })
        })
        .chain(patch.rows.values().copied())
        .collect();
    others.sort_by_key(RowRead::id);
    let mut others = others.into_iter().peekable();
    let mut revlog = RevlogBuilder::default();
    for index in 0..view.revlog.len() {
        let slotted = view.revlog.get(index);
        if patch.hidden.contains(&slotted.row.id) {
            continue;
        }
        while let Some(row) = others.next_if(|row| row.id() < slotted.row.id) {
            revlog.push(row, slot_of);
        }
        let card = match view.base_row_card(slotted.row.id, slotted.slot) {
            Some(CardKey::Slot(slot)) => Some(view.cards.id(slot)),
            Some(CardKey::Patched(id)) => Some(id),
            None => None,
        };
        revlog.push_slotted(slotted.row, card.and_then(slot_of));
    }
    for row in others {
        revlog.push(row, slot_of);
    }
    (cards, revlog)
}

/// One commit's overlay entry, as the derived host carries it to the
/// writer (D6).
pub struct ColumnsDelta(Entry);

/// The `stats_columns` component (D4), registered by the host.
pub struct StatsColumns {
    shared: Arc<Shared>,
}

impl StatsColumns {
    pub(crate) fn new(shared: Arc<Shared>) -> StatsColumns {
        StatsColumns { shared }
    }
}

/// A commit's change for the copies (G9.4), read from its journal and, for
/// an added card, the committed review log. `None` when an image cannot be
/// read: the component rebuilds.
fn change_of(commit: &ObservedCommit<'_>, rows: Option<&ObserverRows<'_>>) -> Option<Change> {
    let changes = commit.changes;
    let mut change = Change {
        revlog_removed: changes.revlog_removed.iter().map(|id| id.0).collect(),
        ..Change::default()
    };
    for card in &changes.cards {
        match card.kind {
            ChangeKind::Removed => change.cards_removed.push(card.id.0),
            ChangeKind::Added | ChangeKind::Updated => {
                let delta = commit.payload.card(card.id).ok()??;
                let after: RowImage = delta.after?;
                change.cards_put.push(rows::read_card(&after, &rows::TABLE_CARD));
                if card.kind == ChangeKind::Added {
                    let ids = rows?.revlog_ids_of_card(CardId(card.id.0)).ok()?;
                    change.attach.push((card.id.0, ids));
                }
            }
        }
    }
    for id in &changes.revlog_added {
        if id.0 < 0 {
            // No graph reads a negative id (G3); the copies hold none.
            continue;
        }
        let delta = commit.payload.row(Table::Revlog, &RowKey::Id(id.0)).ok()??;
        match delta.after {
            RowAfter::Row(image) => change.revlog_added.push(rows::read_row(&image)),
            _ => return None,
        }
    }
    Some(change)
}

impl DerivedComponent for StatsColumns {
    type Delta = ColumnsDelta;

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new(COMPONENT, VERSION)
            .table(TableDef::columns(
                FILES,
                "(name TEXT PRIMARY KEY, kind INTEGER NOT NULL, length INTEGER NOT NULL, \
                 checksum INTEGER NOT NULL, base_seq INTEGER NOT NULL) WITHOUT ROWID",
            ))
            .files(FILES)
    }

    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<ColumnsDelta> {
        self.capture_rows(commit, None)
    }

    fn capture_with_rows(&self, commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Capture<ColumnsDelta> {
        self.capture_rows(commit, Some(rows))
    }

    /// An overlay entry for the commit (G9.3); nothing is written to the
    /// component's tables. The entry is in place before the transaction
    /// commits, so a reader whose view holds the commit finds it; a reader
    /// that does not filters it out by seq, and one of a transaction that
    /// fails is never current again before a rebuild (D6, D8).
    fn apply(&self, _tables: &ComponentTables<'_>, ColumnsDelta(delta): ColumnsDelta) -> Result<(), EngineError> {
        let mut state = self.shared.lock();
        // Entries of an older generation can no longer be read.
        // The overlay holds one generation: its last entry says which.
        let generation = delta.generation;
        if state.entries.last().is_some_and(|entry| entry.generation != generation) {
            state.entries.retain(|entry| entry.generation == generation);
        }
        state.entries.push(delta);
        Ok(())
    }

    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError> {
        let dir = sink.files_dir().ok_or(EngineError::Cancelled)?;
        let generation = source.generation().0;
        let seq = source.seq().0;
        let total = source.row_count(Table::Cards)? + source.row_count(Table::Revlog)?;
        let mut done = 0u64;
        let mut stopped: Option<EngineError> = None;
        let step = |sink: &mut BuildSink, done: &mut u64, stopped: &mut Option<EngineError>| {
            *done += 1;
            if (*done).is_multiple_of(CHECK_EVERY) {
                if let Err(error) = sink.check() {
                    *stopped = Some(error);
                    return false;
                }
                sink.progress(*done, Some(total));
            }
            true
        };
        let mut cards = CardsBuilder::with_capacity(usize::try_from(source.row_count(Table::Cards)?).unwrap_or(0));
        source.visit_stats_cards(&|| false, &mut |cells| {
            cards.push(rows::read_card(cells, &rows::VISITED_CARD));
            step(sink, &mut done, &mut stopped)
        })?;
        if let Some(error) = stopped.take() {
            return Err(error);
        }
        let slots = cards.slots();
        let mut revlog = RevlogBuilder::with_capacity(usize::try_from(source.row_count(Table::Revlog)?).unwrap_or(0));
        // One ascending pass over every row the graphs can read (id ≥ 0):
        // the files become live only at the swap, so the window's rows are
        // never read before the rest anyway (G9.2).
        source.visit_stats_revlog(0, None, &|| false, &mut |cells| {
            revlog.push(rows::read_row(cells), |cid| slots.get(&cid).copied());
            step(sink, &mut done, &mut stopped)
        })?;
        if let Some(error) = stopped.take() {
            return Err(error);
        }
        sink.check()?;
        let card_image = cards.image();
        let card_record = self.shared.write_file(&dir, KIND_CARDS, &card_image, generation, seq)?;
        drop(card_image);
        let revlog_image = revlog.image();
        let revlog_record = self.shared.write_file(&dir, KIND_REVLOG, &revlog_image, generation, seq)?;
        drop(revlog_image);
        sink.progress(total, Some(total));
        let records = vec![card_record, revlog_record];
        sink.write(move |tables| replace_records(tables, &records))
    }

    fn deferred_until(&self) -> Option<&'static str> {
        Some(HOME_IDLE)
    }

    fn attach(&self, handle: ComponentHandle) {
        *self.shared.handle.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
    }

    fn files_in_use(&self, live: &HashSet<String>) -> HashSet<String> {
        self.shared.files_in_use(live)
    }

    /// G9.3, G9.5: merges the overlay into new files whose rows replace the
    /// old ones, then deletes the files no row names and no reader maps.
    fn persist(&self, store: &mut ComponentStore<'_>) -> Result<(), EngineError> {
        let generation = store.generation().0;
        let dir = store.dir();
        let mut records = Vec::new();
        store.write(|tables| {
            records = read_records(tables)?;
            Ok(())
        })?;
        if let Some((card_file, revlog_file)) = base_records(records) {
            // A file that no longer matches its row is a rebuild (G9.3): the
            // error marks the component `ready` 0 and queues one (D4). Until
            // then requests compute from their snapshot.
            self.shared.card_table(&dir, &card_file)?;
            self.shared.revlog_table(&dir, &revlog_file)?;
            let base = self.shared.base_seq(generation, &card_file, &revlog_file);
            let through = {
                let state = self.shared.lock();
                state
                    .entries
                    .iter()
                    .filter(|entry| entry.generation == generation && entry.seq > base)
                    .map(|entry| entry.seq)
                    .max()
            };
            if let Some(through) = through {
                let view = View {
                    cards: self.shared.card_table(&dir, &card_file)?,
                    revlog: self.shared.revlog_table(&dir, &revlog_file)?,
                    patch: Arc::new(
                        self.shared
                            .patch(generation, base, through)
                            .ok_or_else(|| internal("the overlay lost entries its files lack"))?,
                    ),
                };
                let (cards, revlog) = rebase(&view);
                drop(view);
                let card_image = cards.image();
                let card_record = self.shared.write_file(&dir, KIND_CARDS, &card_image, generation, through)?;
                drop(card_image);
                let revlog_image = revlog.image();
                let revlog_record = self
                    .shared
                    .write_file(&dir, KIND_REVLOG, &revlog_image, generation, through)?;
                drop(revlog_image);
                let new = vec![card_record, revlog_record];
                store.write(|tables| replace_records(tables, &new))?;
                let mut state = self.shared.lock();
                state
                    .entries
                    .retain(|entry| entry.generation != generation || entry.seq > through);
                let pruned = state.pruned.entry(generation).or_default();
                *pruned = (*pruned).max(through);
            }
        }
        // Files a reader still maps stay until its last map goes (D7).
        let live = store.named()?;
        let keep = self.shared.files_in_use(&live);
        for error in store.sweep(&keep) {
            tracing::warn!(%error, "deleting stats column files");
        }
        Ok(())
    }

    /// G9.5: unmaps the files (a running request keeps its own maps until
    /// it ends) and drops the overlay, which persist has merged.
    fn release(&self) {
        let mut state = self.shared.lock();
        let dropped: Vec<(u64, u64)> = state.entries.iter().map(|entry| (entry.generation, entry.seq)).collect();
        for (generation, seq) in dropped {
            let pruned = state.pruned.entry(generation).or_default();
            *pruned = (*pruned).max(seq);
        }
        state.entries.clear();
        state.cards.clear();
        state.revlog.clear();
        state.epoch += 1;
    }
}

impl StatsColumns {
    fn capture_rows(&self, commit: &ObservedCommit<'_>, rows: Option<&ObserverRows<'_>>) -> Capture<ColumnsDelta> {
        let changes = commit.changes;
        if changes.cards.is_empty() && changes.revlog_added.is_empty() && changes.revlog_removed.is_empty() {
            return Capture::Nothing;
        }
        match change_of(commit, rows) {
            Some(change) if change.is_empty() => Capture::Nothing,
            Some(change) => Capture::Delta(ColumnsDelta(Entry {
                generation: commit.generation.0,
                seq: commit.seq.0,
                change: Arc::new(change),
            })),
            None => Capture::Rebuild,
        }
    }
}
