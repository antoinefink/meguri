//! The `stats_history` component (`stats/history-reads` H14;
//! `storage/derived-store` D4): the card-state replay behind H2, kept per
//! deck key and absolute day in its own tables.
//!
//! - Tables: `meta` (the phase of the day grid), `cards` (one record per
//!   readable existing card), `days` (per key and day: the five state
//!   changes and the started count), `keys` (suspended cards per key),
//!   `bad` (cards a part must fail for, G4).
//! - Held at each bind until a request calls `request_refresh` (H14.2); no
//!   catch-up, so a refresh is a rebuild.
//! - Capture (on the collection writer): every card a commit touches, as it
//!   was before the commit and as it is after, each with its rows. Apply (on
//!   the derived writer): both replayed on the target's grid, the old taken
//!   out of the sums and the new put in (H14.3).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_engine::changes::observers::RowAfter;
use meguri_engine::derived_host::{BuildSink, Capture, ComponentHandle, ComponentSchema, ComponentTables, DerivedComponent, TableDef};
use meguri_engine::{EngineError, ObservedCommit, ObserverRows, Snapshot};
use meguri_model::ids::CardId;
use meguri_storage::{Cell, ReadRows, RowImage, RowKey, StorageError, Table};

use super::replay::{BadCard, CardInput, CardRecord, Contribution, Grid, Key, State, replay, shape};
use super::sums::{DayValues, Sums};
use crate::graphs::rows::{self, BadColumn, RowRead};

/// The component's name (D4).
pub const HISTORY_COMPONENT: &str = "stats_history";
/// The host signal its `deferred until` names: no host sends it, so only a
/// request's `request_refresh` releases a build (H14.2).
pub const HISTORY_WANTED: &str = "statsHistoryWanted";
/// Its version: a change rebuilds it.
const VERSION: u32 = 1;
const META: &str = "meta";
const CARDS: &str = "cards";
const DAYS: &str = "days";
const KEYS: &str = "keys";
const BAD: &str = "bad";
/// Cards between two cancellation checks and progress reports of a build.
const CHECK_EVERY: u64 = 2_048;
/// Rows written per batch of a build: each batch holds the derived writer
/// (other components' commits, flushes) only briefly.
const BATCH: usize = 5_000;
/// A commit touching more cards than this is checked against H14.3's
/// tenth of the cards.
const LARGE_CHANGE: usize = 1_000;
/// The review-log rows a capture reads at most on the collection writer:
/// past them the commit is applied as a rebuild (D4's "rebuild" answer),
/// which keeps the writer's work short.
const CAPTURE_ROWS: usize = 100_000;

pub(crate) fn internal(message: impl Into<String>) -> EngineError {
    EngineError::Internal {
        message: message.into(),
    }
}

fn sql(error: meguri_storage::rusqlite::Error) -> EngineError {
    internal(error.to_string())
}

/// What the component and the requests share.
#[derive(Default)]
pub(crate) struct Shared {
    /// The phase the next build keeps its days on: the one of the latest
    /// request's `N`.
    wanted: Mutex<Option<i64>>,
    handle: Mutex<Option<ComponentHandle>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub(crate) fn want(&self, grid: Grid) {
        *lock(&self.wanted) = Some(grid.phase());
    }

    /// A build no request asked for keeps phase 0; the first request of
    /// another phase rebuilds it.
    fn wanted(&self) -> Grid {
        Grid::from_phase(lock(&self.wanted).unwrap_or(0))
    }

    /// Asks the derived writer to rebuild the component (another phase).
    pub(crate) fn rebuild(&self) {
        if let Some(handle) = lock(&self.handle).clone() {
            handle.rebuild();
        }
    }
}

/// The component (D4), registered by the host.
pub struct StatsHistory {
    shared: Arc<Shared>,
}

impl StatsHistory {
    pub(crate) fn new(shared: Arc<Shared>) -> StatsHistory {
        StatsHistory { shared }
    }
}

/// One card a commit touched: as it was before and as it is after, each
/// with its rows (absent when the card did not exist).
#[derive(Debug)]
struct CardChange {
    id: i64,
    before: Option<CardInput>,
    after: Option<CardInput>,
}

/// One commit's change, as the derived host carries it to the writer.
pub struct HistoryDelta(Vec<CardChange>);

/// Why a commit cannot be applied: the component rebuilds.
struct Unusable;

impl From<StorageError> for Unusable {
    fn from(error: StorageError) -> Unusable {
        tracing::warn!(%error, "reading a commit for the stats history");
        Unusable
    }
}

/// The cards `commit` changes for the series, each before and after it.
/// `None` when it touches none.
fn changes_of(commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Result<Option<HistoryDelta>, Unusable> {
    let payload = &commit.payload;
    if payload.row_keys(Table::Cards).next().is_none() && payload.row_keys(Table::Revlog).next().is_none() {
        return Ok(None);
    }
    let mut cards: HashMap<i64, (Option<RowImage>, Option<RowImage>)> = HashMap::new();
    for delta in payload.cards() {
        let delta = delta?;
        cards.insert(delta.id.0, (delta.before, delta.after));
    }
    // The rows the commit wrote or removed: a row's image before belongs to
    // the card it named then, its image after to the card it names now.
    let mut journalled: HashSet<i64> = HashSet::new();
    let mut removed: HashMap<i64, Vec<RowRead>> = HashMap::new();
    let mut touched: BTreeSet<i64> = BTreeSet::new();
    for delta in payload.rows(Table::Revlog) {
        let delta = delta?;
        let RowKey::Id(id) = delta.key else { continue };
        journalled.insert(id);
        if let Some(image) = &delta.before {
            let row = rows::read_row(image);
            if let Some(cid) = row.cid() {
                touched.insert(cid);
                if id >= 0 {
                    removed.entry(cid).or_default().push(row);
                }
            }
        }
        if let RowAfter::Row(image) = &delta.after
            && let Some(cid) = rows::read_row(image).cid()
        {
            touched.insert(cid);
        }
    }
    // A card change matters when the series reads what it changed (H14.3:
    // its key, suspended or not, its type bucket) or its rows changed.
    for (id, (before, after)) in &cards {
        if touched.contains(id) {
            continue;
        }
        let read = |image: &Option<RowImage>| image.as_deref().map(|cells| shape(&rows::read_card(cells, &rows::TABLE_CARD)));
        if read(before) != read(after) {
            touched.insert(*id);
        }
    }
    if touched.is_empty() {
        return Ok(None);
    }
    if touched.len() > LARGE_CHANGE {
        // H14.3: more than a tenth of the cards is applied as a rebuild.
        let count = rows.row_count(Table::Cards)?;
        if u64::try_from(touched.len()).unwrap_or(u64::MAX).saturating_mul(10) > count {
            return Err(Unusable);
        }
    }
    let mut changes = Vec::with_capacity(touched.len());
    let mut read = 0usize;
    for id in touched {
        let (before, after) = match cards.remove(&id) {
            Some(sides) => sides,
            None => {
                let image = rows.row_image(Table::Cards, &RowKey::Id(id))?;
                (image.clone(), image)
            }
        };
        if before.is_none() && after.is_none() {
            // A deleted card's rows: not in the series.
            continue;
        }
        let images = rows.card_revlog_images(CardId(id))?;
        read += images.len();
        if read > CAPTURE_ROWS {
            return Err(Unusable);
        }
        let now: Vec<RowRead> = images
            .iter()
            .map(|image| rows::read_row(image))
            .filter(|row| row.id() >= 0 && row.cid() == Some(id))
            .collect();
        let then: Vec<RowRead> = now
            .iter()
            .filter(|row| !journalled.contains(&row.id()))
            .copied()
            .chain(removed.remove(&id).unwrap_or_default())
            .collect();
        let input = |image: Option<RowImage>, rows: Vec<RowRead>| {
            image.map(|cells| CardInput {
                card: rows::read_card(&cells, &rows::TABLE_CARD),
                rows,
            })
        };
        changes.push(CardChange {
            id,
            before: input(before, then),
            after: input(after, now),
        });
    }
    Ok((!changes.is_empty()).then_some(HistoryDelta(changes)))
}

/// The grid the target's content is kept on.
fn read_grid(tables: &ComponentTables<'_>) -> Result<Option<Grid>, EngineError> {
    let table = tables.table(META)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!("SELECT phase FROM \"{table}\" WHERE id = 1"))
        .map_err(sql)?;
    let mut found = statement.query([]).map_err(sql)?;
    match found.next().map_err(sql)? {
        Some(row) => Ok(Some(Grid::from_phase(row.get::<_, i64>(0).map_err(sql)?))),
        None => Ok(None),
    }
}

fn write_grid(tables: &ComponentTables<'_>, grid: Grid) -> Result<(), EngineError> {
    let table = tables.table(META)?;
    tables
        .conn()
        .execute(&format!("INSERT OR REPLACE INTO \"{table}\" (id, phase) VALUES (1, ?1)"), [grid.phase()])
        .map_err(sql)?;
    Ok(())
}

fn optional(value: Option<i64>) -> Cell {
    value.map_or(Cell::Null, Cell::Integer)
}

fn put_records(tables: &ComponentTables<'_>, records: &[CardRecord]) -> Result<(), EngineError> {
    let table = tables.table(CARDS)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!(
            "INSERT OR REPLACE INTO \"{table}\" (id, did, odid, suspended, state, newest_row, first_review) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        ))
        .map_err(sql)?;
    for record in records {
        statement
            .execute(meguri_storage::rusqlite::params_from_iter([
                Cell::Integer(record.id),
                Cell::Integer(record.key.did),
                Cell::Integer(record.key.odid),
                Cell::Integer(i64::from(record.suspended)),
                optional(record.table_state.map(|state| state.index() as i64)),
                optional(record.newest_row),
                optional(record.first_review),
            ]))
            .map_err(sql)?;
    }
    Ok(())
}

fn put_bad(tables: &ComponentTables<'_>, bad: &[BadCard]) -> Result<(), EngineError> {
    let table = tables.table(BAD)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!(
            "INSERT OR REPLACE INTO \"{table}\" (card, did, odid, row, code) VALUES (?1, ?2, ?3, ?4, ?5)"
        ))
        .map_err(sql)?;
    for entry in bad {
        statement
            .execute(meguri_storage::rusqlite::params_from_iter([
                Cell::Integer(entry.card),
                optional(entry.did),
                optional(entry.odid),
                optional(entry.row),
                Cell::Integer(i64::from(entry.column.code())),
            ]))
            .map_err(sql)?;
    }
    Ok(())
}

fn delete_card(tables: &ComponentTables<'_>, id: i64) -> Result<(), EngineError> {
    for name in [CARDS, BAD] {
        let table = tables.table(name)?;
        let column = if name == CARDS { "id" } else { "card" };
        tables
            .conn()
            .prepare_cached(&format!("DELETE FROM \"{table}\" WHERE {column} = ?1"))
            .map_err(sql)?
            .execute([id])
            .map_err(sql)?;
    }
    Ok(())
}

/// Writes days into empty tables (a build).
fn insert_days(tables: &ComponentTables<'_>, days: &[((Key, i64), DayValues)]) -> Result<(), EngineError> {
    let table = tables.table(DAYS)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!(
            "INSERT INTO \"{table}\" (did, odid, day, new_cards, learning, relearning, young, mature, started) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ))
        .map_err(sql)?;
    for ((key, day), values) in days {
        statement.execute(day_params(*key, *day, values)).map_err(sql)?;
    }
    Ok(())
}

/// Writes the suspended counts into empty tables (a build).
fn insert_keys(tables: &ComponentTables<'_>, sums: &Sums) -> Result<(), EngineError> {
    let keys = tables.table(KEYS)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!("INSERT INTO \"{keys}\" (did, odid, left_out) VALUES (?1, ?2, ?3)"))
        .map_err(sql)?;
    for (key, count) in &sums.left_out {
        statement.execute((key.did, key.odid, count)).map_err(sql)?;
    }
    Ok(())
}

fn day_params(key: Key, day: i64, values: &DayValues) -> [i64; 9] {
    [
        key.did, key.odid, day, values[0], values[1], values[2], values[3], values[4], values[5],
    ]
}

/// Adds `sums` (a change) to the tables, dropping the entries it brings to
/// nothing.
fn add_sums(tables: &ComponentTables<'_>, sums: &Sums) -> Result<(), EngineError> {
    let days = tables.table(DAYS)?;
    let conn = tables.conn();
    let mut add = conn
        .prepare_cached(&format!(
            "INSERT INTO \"{days}\" (did, odid, day, new_cards, learning, relearning, young, mature, started) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT (did, odid, day) DO UPDATE SET \
             new_cards = new_cards + excluded.new_cards, learning = learning + excluded.learning, \
             relearning = relearning + excluded.relearning, young = young + excluded.young, \
             mature = mature + excluded.mature, started = started + excluded.started"
        ))
        .map_err(sql)?;
    let mut drop = conn
        .prepare_cached(&format!(
            "DELETE FROM \"{days}\" WHERE did = ?1 AND odid = ?2 AND day = ?3 AND new_cards = 0 AND learning = 0 \
             AND relearning = 0 AND young = 0 AND mature = 0 AND started = 0"
        ))
        .map_err(sql)?;
    for ((key, day), values) in &sums.days {
        add.execute(day_params(*key, *day, values)).map_err(sql)?;
        drop.execute((key.did, key.odid, day)).map_err(sql)?;
    }
    let keys = tables.table(KEYS)?;
    let mut add = conn
        .prepare_cached(&format!(
            "INSERT INTO \"{keys}\" (did, odid, left_out) VALUES (?1, ?2, ?3) ON CONFLICT (did, odid) DO UPDATE \
             SET left_out = left_out + excluded.left_out"
        ))
        .map_err(sql)?;
    let mut drop = conn
        .prepare_cached(&format!("DELETE FROM \"{keys}\" WHERE did = ?1 AND odid = ?2 AND left_out = 0"))
        .map_err(sql)?;
    for (key, count) in &sums.left_out {
        add.execute((key.did, key.odid, count)).map_err(sql)?;
        drop.execute((key.did, key.odid)).map_err(sql)?;
    }
    Ok(())
}

/// Where the card's columns sit in a [`ReadRows::visit_stats_histories`]
/// row.
const CARD_WIDTH: usize = 9;

/// Visits every existing card of `source` with its rows of `id` ≥ 0 (the
/// rows that read its id as their `cid`); `each` returns false to stop.
pub(crate) fn each_card(
    source: &impl ReadRows,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(CardInput) -> bool,
) -> Result<(), StorageError> {
    let mut current: Option<(i64, CardInput)> = None;
    let mut stopped = false;
    source.visit_stats_histories(cancel, &mut |cells| {
        let id = match cells.first() {
            Some(Cell::Integer(id)) => *id,
            _ => return true,
        };
        if current.as_ref().is_none_or(|(at, _)| *at != id) {
            if let Some((_, input)) = current.take()
                && !each(input)
            {
                stopped = true;
                return false;
            }
            current = Some((
                id,
                CardInput {
                    card: rows::read_card(&cells[..CARD_WIDTH], &rows::HISTORY_CARD),
                    rows: Vec::new(),
                },
            ));
        }
        if !matches!(cells.get(CARD_WIDTH), None | Some(Cell::Null))
            && let Some((_, input)) = current.as_mut()
        {
            let row = rows::read_row(&cells[CARD_WIDTH..]);
            // A `cid` that matched without reading as this id (a REAL) names
            // no card (G4).
            if row.cid() == Some(id) {
                input.rows.push(row);
            }
        }
        true
    })?;
    if !stopped && let Some((_, input)) = current {
        each(input);
    }
    Ok(())
}

/// Every existing card's contribution summed per key, from `source`, with
/// the records and unreadable cards (the build, and H18's read without the
/// component).
pub(crate) struct Replay {
    pub sums: Sums,
    pub bad: Vec<BadCard>,
}

pub(crate) fn replay_all(
    source: &impl ReadRows,
    grid: Grid,
    cancel: &dyn Fn() -> bool,
) -> Result<Replay, StorageError> {
    let mut all = Replay {
        sums: Sums::default(),
        bad: Vec::new(),
    };
    each_card(source, cancel, &mut |input| {
        match replay(grid, &input) {
            Contribution::Bad(bad) => all.bad.push(bad),
            card => all.sums.add(&card, 1),
        }
        true
    })?;
    all.sums.prune();
    Ok(all)
}

impl DerivedComponent for StatsHistory {
    type Delta = HistoryDelta;

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new(HISTORY_COMPONENT, VERSION)
            .table(TableDef::columns(META, "(id INTEGER PRIMARY KEY, phase INTEGER NOT NULL)"))
            .table(TableDef::columns(
                CARDS,
                "(id INTEGER PRIMARY KEY, did INTEGER NOT NULL, odid INTEGER NOT NULL, suspended INTEGER NOT NULL, \
                 state INTEGER, newest_row INTEGER, first_review INTEGER)",
            ))
            .table(TableDef::columns(
                DAYS,
                "(did INTEGER NOT NULL, odid INTEGER NOT NULL, day INTEGER NOT NULL, new_cards INTEGER NOT NULL, \
                 learning INTEGER NOT NULL, relearning INTEGER NOT NULL, young INTEGER NOT NULL, \
                 mature INTEGER NOT NULL, started INTEGER NOT NULL, PRIMARY KEY (did, odid, day)) WITHOUT ROWID",
            ))
            .table(TableDef::columns(
                KEYS,
                "(did INTEGER NOT NULL, odid INTEGER NOT NULL, left_out INTEGER NOT NULL, \
                 PRIMARY KEY (did, odid)) WITHOUT ROWID",
            ))
            .table(TableDef::columns(
                BAD,
                "(card INTEGER PRIMARY KEY, did INTEGER, odid INTEGER, row INTEGER, code INTEGER NOT NULL)",
            ))
    }

    /// Without the committed rows the touched cards cannot be read again.
    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<HistoryDelta> {
        let payload = &commit.payload;
        if payload.row_keys(Table::Cards).next().is_none() && payload.row_keys(Table::Revlog).next().is_none() {
            Capture::Nothing
        } else {
            Capture::Rebuild
        }
    }

    fn capture_with_rows(&self, commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Capture<HistoryDelta> {
        match changes_of(commit, rows) {
            Ok(Some(delta)) => Capture::Delta(delta),
            Ok(None) => Capture::Nothing,
            Err(Unusable) => Capture::Rebuild,
        }
    }

    /// Each touched card replayed twice on the target's grid, as it was and
    /// as it is: the first taken out, the second put in (H14.3). A target
    /// with no grid cannot be followed: the component rebuilds.
    fn apply(&self, tables: &ComponentTables<'_>, HistoryDelta(changes): HistoryDelta) -> Result<(), EngineError> {
        let grid = read_grid(tables)?.ok_or_else(|| internal("the stats history keeps no day grid"))?;
        let mut sums = Sums::default();
        let mut records = Vec::new();
        let mut bad = Vec::new();
        for change in &changes {
            if let Some(before) = &change.before {
                sums.add(&replay(grid, before), -1);
            }
            delete_card(tables, change.id)?;
            match change.after.as_ref().map(|after| replay(grid, after)) {
                Some(Contribution::Bad(entry)) => bad.push(entry),
                Some(card @ Contribution::Card(_)) => {
                    sums.add(&card, 1);
                    if let Contribution::Card(replayed) = card {
                        records.push(replayed.record);
                    }
                }
                None => {}
            }
        }
        sums.prune();
        add_sums(tables, &sums)?;
        put_records(tables, &records)?;
        put_bad(tables, &bad)
    }

    /// H14.2: one pass over every card with its rows, the records written
    /// in batches as they come, then the per-key sums and the grid.
    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError> {
        let grid = self.shared.wanted();
        let total = source.row_count(Table::Cards)? + source.row_count(Table::Revlog)?;
        let mut sums = Sums::default();
        let mut bad: Vec<BadCard> = Vec::new();
        let mut records: Vec<CardRecord> = Vec::with_capacity(BATCH);
        let mut cards = 0u64;
        let mut done = 0u64;
        let mut stopped: Option<EngineError> = None;
        each_card(&**source, &|| false, &mut |input| {
            cards += 1;
            done += 1 + u64::try_from(input.rows.len()).unwrap_or(0);
            match replay(grid, &input) {
                Contribution::Bad(entry) => bad.push(entry),
                card @ Contribution::Card(_) => {
                    sums.add(&card, 1);
                    if let Contribution::Card(replayed) = card {
                        records.push(replayed.record);
                    }
                }
            }
            if records.len() >= BATCH {
                let batch = std::mem::replace(&mut records, Vec::with_capacity(BATCH));
                if let Err(error) = sink.write(move |tables| put_records(tables, &batch)) {
                    stopped = Some(error);
                    return false;
                }
            }
            if cards.is_multiple_of(CHECK_EVERY) {
                if let Err(error) = sink.check() {
                    stopped = Some(error);
                    return false;
                }
                sink.progress(done.min(total), Some(total));
            }
            true
        })?;
        if let Some(error) = stopped {
            return Err(error);
        }
        sums.prune();
        let mut days: Vec<((Key, i64), DayValues)> = std::mem::take(&mut sums.days).into_iter().collect();
        while !days.is_empty() {
            let batch: Vec<((Key, i64), DayValues)> = days.split_off(days.len().saturating_sub(BATCH));
            sink.write(move |tables| insert_days(tables, &batch))?;
        }
        // The grid last: a build that stops before it leaves none.
        sink.write(move |tables| {
            put_records(tables, &records)?;
            put_bad(tables, &bad)?;
            insert_keys(tables, &sums)?;
            write_grid(tables, grid)
        })?;
        sink.progress(total, Some(total));
        Ok(())
    }

    fn deferred_until(&self) -> Option<&'static str> {
        Some(HISTORY_WANTED)
    }

    fn attach(&self, handle: ComponentHandle) {
        *lock(&self.shared.handle) = Some(handle);
    }
}

/// The kept series as `snapshot` reads the store: the grid, then every
/// unreadable card, key and day through the visitors. Ask
/// [`meguri_engine::DerivedHost::is_current`] first.
pub(crate) fn kept_grid(snapshot: &Snapshot<'_>) -> Result<Option<Grid>, StorageError> {
    let rows = snapshot.derived_rows(HISTORY_COMPONENT, META, "phase")?;
    Ok(rows.first().and_then(|row| match row.first() {
        Some(Cell::Integer(phase)) => Some(Grid::from_phase(*phase)),
        _ => None,
    }))
}

fn integer(cells: &[Cell], at: usize) -> Option<i64> {
    match cells.get(at) {
        Some(Cell::Integer(value)) => Some(*value),
        _ => None,
    }
}

pub(crate) fn visit_kept_bad(
    snapshot: &Snapshot<'_>,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(BadCard),
) -> Result<(), StorageError> {
    snapshot.visit_derived(HISTORY_COMPONENT, BAD, "card, did, odid, row, code", cancel, &mut |cells| {
        let column = integer(cells, 4)
            .and_then(|code| u8::try_from(code).ok())
            .and_then(BadColumn::from_code);
        if let (Some(card), Some(column)) = (integer(cells, 0), column) {
            each(BadCard {
                card,
                did: integer(cells, 1),
                odid: integer(cells, 2),
                row: integer(cells, 3),
                column,
            });
        }
        true
    })
}

pub(crate) fn visit_kept_keys(
    snapshot: &Snapshot<'_>,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(Key, i64),
) -> Result<(), StorageError> {
    snapshot.visit_derived(HISTORY_COMPONENT, KEYS, "did, odid, left_out", cancel, &mut |cells| {
        if let (Some(did), Some(odid), Some(count)) = (integer(cells, 0), integer(cells, 1), integer(cells, 2)) {
            each(Key { did, odid }, count);
        }
        true
    })
}

pub(crate) fn visit_kept_days(
    snapshot: &Snapshot<'_>,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(Key, i64, &DayValues),
) -> Result<(), StorageError> {
    snapshot.visit_derived(
        HISTORY_COMPONENT,
        DAYS,
        "did, odid, day, new_cards, learning, relearning, young, mature, started",
        cancel,
        &mut |cells| {
            let mut values: DayValues = [0; 6];
            for (at, value) in values.iter_mut().enumerate() {
                *value = integer(cells, 3 + at).unwrap_or(0);
            }
            if let (Some(did), Some(odid), Some(day)) = (integer(cells, 0), integer(cells, 1), integer(cells, 2)) {
                each(Key { did, odid }, day, &values);
            }
            true
        },
    )
}

/// The kept card records, for tests (H14.1).
pub(crate) fn visit_kept_records(
    snapshot: &Snapshot<'_>,
    each: &mut dyn FnMut(CardRecord),
) -> Result<(), StorageError> {
    snapshot.visit_derived(
        HISTORY_COMPONENT,
        CARDS,
        "id, did, odid, suspended, state, newest_row, first_review",
        &|| false,
        &mut |cells| {
            if let (Some(id), Some(did), Some(odid)) = (integer(cells, 0), integer(cells, 1), integer(cells, 2)) {
                each(CardRecord {
                    id,
                    key: Key { did, odid },
                    suspended: integer(cells, 3) == Some(1),
                    table_state: integer(cells, 4).and_then(State::from_index),
                    newest_row: integer(cells, 5),
                    first_review: integer(cells, 6),
                });
            }
            true
        },
    )
}
