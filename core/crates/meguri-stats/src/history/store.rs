//! The `stats_history` component (`stats/history-reads` H14;
//! `storage/derived-store` D4): the card-state replay behind H2 and the
//! knowledge replay behind H3, kept per deck key and absolute day in its
//! own tables.
//!
//! - Tables: `meta` (the phase of the day grid and the knowledge frame),
//!   `cards` (one record per readable existing card, with its home preset
//!   and its memory over every row), `days` (per key and day: the five state
//!   changes and the started count), `keys` (suspended cards per key),
//!   `knowledge` (per key and kept sample day: the summed recall probabilities
//!   and the studied cards), `presets` (the home presets' values the
//!   knowledge was replayed with), `bad` (cards a part must fail for, G4).
//! - Held at each bind until a request calls `request_refresh` (H14.2); no
//!   catch-up, so a refresh is a rebuild.
//! - Capture (on the collection writer): every card a commit touches, as it
//!   was before the commit and as it is after, each with its rows, and its
//!   home preset after. A change of a preset's FSRS values or of a deck's
//!   preset is applied as a rebuild (H14.3's preset row). Apply (on the
//!   derived writer): both replayed on the target's grid and frame, the old
//!   with the preset values kept for it, the new with the captured ones; the
//!   old taken out of the sums and the new put in (H14.3).
//! - A build replays the cards on every core (H19), the main thread reading
//!   the log.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_engine::changes::observers::RowAfter;
use meguri_engine::derived_host::{BuildSink, Capture, ComponentHandle, ComponentSchema, ComponentTables, DerivedComponent, TableDef};
use meguri_engine::{EngineError, ObservedCommit, ObserverRows, Snapshot};
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{CardId, DeckConfigId};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::DeckKind;
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_sched::model::preset::PresetView;
use meguri_storage::{Cell, ReadRows, Record, RowImage, RowKey, StorageError, Table};

use super::knowledge::{Frame, Knowledge, Memory, Preset, PresetInputs, knowledge};
use super::replay::{BadCard, CardInput, CardRecord, Contribution, Grid, Key, State, readable, replay_rows, shape};
use super::sums::{DayValues, NowInputs, SampleValues, Sums};
use crate::graphs::rows::{self, BadColumn, RowRead};

/// The component's name (D4).
pub const HISTORY_COMPONENT: &str = "stats_history";
/// The host signal its `deferred until` names: no host sends it, so only a
/// request's `request_refresh` releases a build (H14.2).
pub const HISTORY_WANTED: &str = "statsHistoryWanted";
/// Its version: a change rebuilds it. 2: the knowledge of H3 (U290).
const VERSION: u32 = 2;
const META: &str = "meta";
const CARDS: &str = "cards";
const DAYS: &str = "days";
const KEYS: &str = "keys";
const KNOWLEDGE: &str = "knowledge";
const PRESETS: &str = "presets";
const BAD: &str = "bad";
/// Cards between two cancellation checks and progress reports of a build.
const CHECK_EVERY: u64 = 2_048;
/// Rows written per batch of a build: each batch holds the derived writer
/// (other components' commits, flushes) only briefly.
const BATCH: usize = 5_000;
/// Cards a replay worker takes at a time.
const WORK: usize = 128;
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
    /// The grid phase and knowledge frame the next build keeps its days
    /// on: the latest request's.
    wanted: Mutex<Option<(i64, Frame)>>,
    handle: Mutex<Option<ComponentHandle>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub(crate) fn want(&self, grid: Grid, frame: Frame) {
        *lock(&self.wanted) = Some((grid.phase(), frame));
    }

    /// A build no request asked for keeps phase 0 and an empty frame; the
    /// first request rebuilds it.
    fn wanted(&self) -> (Grid, Frame) {
        let (phase, frame) = lock(&self.wanted).unwrap_or((0, Frame { latest: 0, anchor: 0 }));
        (Grid::from_phase(phase), frame)
    }

    /// Asks the derived writer to rebuild the component (another phase or
    /// frame).
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
/// with its rows (absent when the card did not exist), and its home preset
/// after (`None`: it has none).
#[derive(Debug)]
struct CardChange {
    id: i64,
    before: Option<CardInput>,
    after: Option<CardInput>,
    preset: Option<PresetInputs>,
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

/// A deck's preset id (`sched/presets` V1): a filtered deck has none.
fn deck_preset(deck: &Deck) -> Option<i64> {
    match &deck.kind.kind {
        DeckKind::Normal(normal) => Some(normal.config_id),
        DeckKind::Filtered(_) => None,
    }
}

/// A preset's inputs from its row, or the built-in defaults when no preset
/// has the id (V1, as `PresetView::for_config`).
fn preset_inputs(id: i64, row: Option<DeckConfig>) -> PresetInputs {
    let source = row.unwrap_or_else(|| DeckConfig {
        id: DeckConfigId(id),
        name: String::new(),
        mtime: Default::default(),
        usn: Default::default(),
        config: ProtoBlob::new(DeckConfigMessage::builtin_defaults()),
    });
    PresetInputs::of(id, &PresetView::new(&source))
}

/// The home deck of a key (H1).
fn home(key: Key) -> i64 {
    if key.odid != 0 { key.odid } else { key.did }
}

/// The home presets of a snapshot: each deck's preset id and each preset's
/// values. A deck or preset that cannot be read has none: its cards fail
/// their knowledge (H3.1) and nothing else.
pub(crate) struct Presets {
    decks: HashMap<i64, Option<i64>>,
    presets: HashMap<i64, Preset>,
}

impl Presets {
    pub(crate) fn read(source: &impl ReadRows) -> Result<Presets, StorageError> {
        // Row by row, as a capture reads them: an unreadable deck is that
        // deck's alone.
        let mut decks: HashMap<i64, Option<i64>> = HashMap::new();
        let mut after: Option<RowKey> = None;
        loop {
            let page = source.row_page(Table::Decks, after.as_ref())?;
            let Some(last) = page.last() else { break };
            after = match last.first() {
                Some(Cell::Integer(id)) => Some(RowKey::Id(*id)),
                _ => break,
            };
            for cells in &page {
                if let Ok(deck) = Deck::from_cells(cells) {
                    decks.insert(deck.id.0, deck_preset(&deck));
                }
            }
        }
        let mut presets = HashMap::new();
        for id in decks.values().flatten().copied().collect::<BTreeSet<i64>>() {
            let row = source.row_image(Table::DeckConfig, &RowKey::Id(id))?;
            // An unreadable preset has none: its cards fail (as a capture).
            if let Ok(inputs) = image_inputs(id, row.as_ref()) {
                presets.insert(id, Preset::new(inputs));
            }
        }
        Ok(Presets { decks, presets })
    }

    fn home(&self, key: Key) -> Option<&Preset> {
        let id = (*self.decks.get(&home(key))?)?;
        self.presets.get(&id)
    }
}

/// The FSRS values of a preset row as a commit's image gives it.
fn image_inputs(id: i64, image: Option<&RowImage>) -> Result<PresetInputs, Unusable> {
    let row = image.map(|cells| DeckConfig::from_cells(cells)).transpose().map_err(|_| Unusable)?;
    Ok(preset_inputs(id, row))
}

/// Whether `commit` changes what any card's knowledge reads of its home
/// preset: a preset's FSRS values, or a normal deck's preset (H14.3).
fn changes_presets(commit: &ObservedCommit<'_>) -> Result<bool, Unusable> {
    let payload = &commit.payload;
    for delta in payload.rows(Table::DeckConfig) {
        let delta = delta?;
        let RowKey::Id(id) = delta.key else { return Err(Unusable) };
        let after = match &delta.after {
            RowAfter::Row(image) => Some(image),
            _ => None,
        };
        if !image_inputs(id, delta.before.as_ref())?.same(&image_inputs(id, after)?) {
            return Ok(true);
        }
    }
    for delta in payload.rows(Table::Decks) {
        let delta = delta?;
        let read = |image: Option<&RowImage>| -> Result<Option<Option<i64>>, Unusable> {
            image
                .map(|cells| Deck::from_cells(cells).map(|deck| deck_preset(&deck)))
                .transpose()
                .map_err(|_| Unusable)
        };
        let after = match &delta.after {
            RowAfter::Row(image) => Some(image),
            _ => None,
        };
        // An added deck has no cards yet; a removed one takes its cards
        // with it in the same commit.
        if let (Some(before), Some(after)) = (read(delta.before.as_ref())?, read(after)?)
            && before != after
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The home preset of a card as the commit leaves it.
fn committed_preset(rows: &ObserverRows<'_>, key: Key, cache: &mut HashMap<i64, Option<PresetInputs>>) -> Result<Option<PresetInputs>, Unusable> {
    let deck = home(key);
    if let Some(found) = cache.get(&deck) {
        return Ok(found.clone());
    }
    let found = match rows.row_image(Table::Decks, &RowKey::Id(deck))? {
        // An unreadable deck has no preset, as a build reads it.
        Some(cells) => match Deck::from_cells(&cells).ok().as_ref().and_then(deck_preset) {
            Some(id) => {
                let preset = rows.row_image(Table::DeckConfig, &RowKey::Id(id))?;
                image_inputs(id, preset.as_ref()).ok()
            }
            None => None,
        },
        None => None,
    };
    cache.insert(deck, found.clone());
    Ok(found)
}

/// The cards `commit` changes for the series, each before and after it.
/// `None` when it touches none.
fn changes_of(commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Result<Option<HistoryDelta>, Unusable> {
    let payload = &commit.payload;
    if changes_presets(commit)? {
        // H14.3: the affected cards replayed with the new values, here by
        // a rebuild in the background (the next request waits on it).
        return Err(Unusable);
    }
    if payload.row_keys(Table::Cards).next().is_none() && payload.row_keys(Table::Revlog).next().is_none() {
        return Ok(None);
    }
    if crate::bulk_commit(commit) {
        // A delta holds every touched row's images: a rebuild in the
        // background instead, so its memory never grows with the commit.
        return Err(Unusable);
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
    let mut presets: HashMap<i64, Option<PresetInputs>> = HashMap::new();
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
        let after = input(after, now);
        let preset = match after.as_ref().map(|after| after.card) {
            Some(rows::CardRead::Card(card)) => committed_preset(
                rows,
                Key {
                    did: card.did,
                    odid: card.odid,
                },
                &mut presets,
            )?,
            _ => None,
        };
        changes.push(CardChange {
            id,
            before: input(before, then),
            after,
            preset,
        });
    }
    Ok((!changes.is_empty()).then_some(HistoryDelta(changes)))
}

/// What one card adds to the component: its H2 part, and its knowledge
/// with the home preset it was replayed with (an unreadable card has
/// neither).
#[derive(Clone, Debug)]
pub(crate) struct Replayed {
    pub contribution: Contribution,
    pub kept: Option<Kept>,
}

/// One card's record (H14.1).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Kept {
    pub card: CardRecord,
    pub preset: Option<i64>,
    pub cut: Memory,
    pub free: Memory,
    pub failed_at: Option<i64>,
}

/// H2 and H3 for one card on `grid`, with the kept sample `days` (no
/// knowledge without them) and its home `preset`, adding `sign` times its
/// part to `sums`.
fn replay_card(
    grid: Grid,
    days: Option<&[i64]>,
    input: &CardInput,
    preset: Option<&Preset>,
    sums: &mut Sums,
    sign: i64,
) -> Replayed {
    let (card, rows) = match readable(input) {
        Ok(read) => read,
        Err(bad) => {
            return Replayed {
                contribution: Contribution::Bad(bad),
                kept: None,
            };
        }
    };
    let replayed = replay_rows(grid, &card, &rows);
    let record = replayed.record;
    // H3 reads H2.1's cards: suspended ones are left out.
    let known = match days {
        Some(days) if !record.suspended => knowledge(grid, days, card.id, &rows, preset, grid.reference()),
        _ => Knowledge::NONE,
    };
    let contribution = Contribution::Card(replayed);
    sums.add(&contribution, sign);
    sums.add_knowledge(record.key, &known, sign);
    Replayed {
        contribution,
        kept: Some(Kept {
            card: record,
            preset: preset.map(|preset| preset.inputs.id),
            cut: known.cut,
            free: known.free,
            failed_at: known.failed_at,
        }),
    }
}

/// The replay of every card: on whose grid and frame, with which presets.
pub(crate) struct Work<'a> {
    pub grid: Grid,
    /// The kept sample days, or `None` for H2 alone.
    pub days: Option<&'a [i64]>,
    pub presets: &'a Presets,
}

/// A batch of replayed cards, with the cards and rows they weigh for
/// progress.
pub(crate) struct Done {
    pub cards: Vec<Replayed>,
    pub weight: u64,
}

/// The number of replay workers: every core (H19), within reason.
fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8)
}

/// Replays every existing card of `source` with its rows, on worker
/// threads while this thread reads the log (`sched/fsrs-items` I1.1's bulk
/// read); `each` sees the replayed cards on this thread, in no set order,
/// and returns false to stop. Returns the summed parts.
pub(crate) fn replay_cards(
    source: &impl ReadRows,
    work: &Work<'_>,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(Done) -> bool,
) -> Result<Sums, StorageError> {
    let stop = AtomicBool::new(false);
    let threads = workers();
    let (inputs, queue) = mpsc::sync_channel::<Vec<CardInput>>(threads * 2);
    let queue = Mutex::new(queue);
    let (results, done) = mpsc::channel::<Done>();
    // One batch replayed, its parts added to `sums`.
    let process = |batch: &[CardInput], sums: &mut Sums| {
        let mut cards = Vec::with_capacity(batch.len());
        let mut weight = 0u64;
        for input in batch {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            weight += 1 + u64::try_from(input.rows.len()).unwrap_or(0);
            let preset = match input.card {
                rows::CardRead::Card(card) => work.presets.home(Key {
                    did: card.did,
                    odid: card.odid,
                }),
                rows::CardRead::Bad(_) => None,
            };
            cards.push(replay_card(work.grid, work.days, input, preset, sums, 1));
        }
        Done { cards, weight }
    };
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let results = results.clone();
            let (queue, stop, process) = (&queue, &stop, &process);
            let spawned = std::thread::Builder::new().spawn_scoped(scope, move || {
                let mut sums = Sums::default();
                let replayed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loop {
                    let batch = match lock(queue).recv() {
                        Ok(batch) => batch,
                        Err(_) => break,
                    };
                    if results.send(process(&batch, &mut sums)).is_err() {
                        break;
                    }
                }));
                if replayed.is_err() {
                    // Nothing this replay summed is used; keep taking the
                    // queue so the reading thread is never left blocked.
                    stop.store(true, Ordering::Relaxed);
                    while lock(queue).recv().is_ok() {}
                }
                replayed.map(|()| sums)
            });
            match spawned {
                Ok(handle) => handles.push(handle),
                // The workers that started do the work.
                Err(error) => {
                    tracing::warn!(%error, "starting a history replay worker");
                    break;
                }
            }
        }
        // No worker at all: this thread replays as it reads.
        let mut inline = handles.is_empty().then(|| (Sums::default(), results.clone()));
        drop(results);
        let mut stopped = false;
        let mut batch: Vec<CardInput> = Vec::with_capacity(WORK);
        let mut forward = |done: &mpsc::Receiver<Done>, stopped: &mut bool| {
            for finished in done.try_iter() {
                if !*stopped && !each(finished) {
                    *stopped = true;
                    stop.store(true, Ordering::Relaxed);
                }
            }
        };
        let read = each_card(source, cancel, &mut |input| {
            if let Some((sums, results)) = inline.as_mut() {
                // Card by card, so the cancel below is seen after each.
                let _ = results.send(process(std::slice::from_ref(&input), sums));
            } else {
                batch.push(input);
                if batch.len() >= WORK {
                    let full = std::mem::replace(&mut batch, Vec::with_capacity(WORK));
                    if inputs.send(full).is_err() {
                        return false;
                    }
                }
            }
            forward(&done, &mut stopped);
            !stopped && !stop.load(Ordering::Relaxed) && !cancel()
        });
        if read.is_ok() && !stopped && !cancel() && !batch.is_empty() {
            let last = std::mem::take(&mut batch);
            match inline.as_mut() {
                Some((sums, results)) => {
                    let _ = results.send(process(&last, sums));
                }
                None => {
                    let _ = inputs.send(last);
                }
            }
        }
        if read.is_err() || cancel() {
            stop.store(true, Ordering::Relaxed);
        }
        drop(inputs);
        let inline = inline.map(|(sums, _)| sums);
        for finished in done.iter() {
            if cancel() {
                // The workers drop what they hold (H16).
                stop.store(true, Ordering::Relaxed);
            }
            if !stopped && !stop.load(Ordering::Relaxed) && !each(finished) {
                stopped = true;
                stop.store(true, Ordering::Relaxed);
            }
        }
        let mut sums = inline.unwrap_or_default();
        for handle in handles {
            match handle.join() {
                Ok(Ok(part)) => sums.merge(part),
                Ok(Err(panic)) | Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        read?;
        // A replay that stopped early is never returned as a whole one.
        if cancel() || stopped || stop.load(Ordering::Relaxed) {
            return Err(interrupted());
        }
        sums.prune();
        Ok(sums)
    })
}

/// SQLITE_INTERRUPT, as a cancelled read reports it.
pub(crate) fn interrupted() -> StorageError {
    StorageError::Sqlite {
        code: 9,
        extended_code: 9,
        message: "the history replay was cancelled".into(),
    }
}

/// The grid and frame the target's content is kept on.
fn read_meta(tables: &ComponentTables<'_>) -> Result<Option<(Grid, Frame)>, EngineError> {
    let table = tables.table(META)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!("SELECT phase, latest, anchor FROM \"{table}\" WHERE id = 1"))
        .map_err(sql)?;
    let mut found = statement.query([]).map_err(sql)?;
    match found.next().map_err(sql)? {
        Some(row) => Ok(Some((
            Grid::from_phase(row.get::<_, i64>(0).map_err(sql)?),
            Frame {
                latest: row.get(1).map_err(sql)?,
                anchor: row.get(2).map_err(sql)?,
            },
        ))),
        None => Ok(None),
    }
}

fn write_meta(tables: &ComponentTables<'_>, grid: Grid, frame: Frame) -> Result<(), EngineError> {
    let table = tables.table(META)?;
    tables
        .conn()
        .execute(
            &format!("INSERT OR REPLACE INTO \"{table}\" (id, phase, latest, anchor) VALUES (1, ?1, ?2, ?3)"),
            [grid.phase(), frame.latest, frame.anchor],
        )
        .map_err(sql)?;
    Ok(())
}

fn optional(value: Option<i64>) -> Cell {
    value.map_or(Cell::Null, Cell::Integer)
}

/// A memory's three value cells: stability, difficulty, `L`.
fn memory_cells(memory: Memory) -> [Cell; 3] {
    match memory {
        Memory::State {
            stability,
            difficulty,
            last,
        } => [
            Cell::Real(f64::from(stability)),
            Cell::Real(f64::from(difficulty)),
            Cell::Integer(last),
        ],
        _ => [Cell::Null, Cell::Null, Cell::Null],
    }
}

fn put_records(tables: &ComponentTables<'_>, records: &[Kept]) -> Result<(), EngineError> {
    let table = tables.table(CARDS)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!(
            "INSERT OR REPLACE INTO \"{table}\" (id, did, odid, suspended, state, newest_row, first_review, preset, \
             cut_code, cut_s, cut_d, cut_l, free_code, free_s, free_d, free_l, failed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"
        ))
        .map_err(sql)?;
    for kept in records {
        let record = &kept.card;
        let [cut_s, cut_d, cut_l] = memory_cells(kept.cut);
        let [free_s, free_d, free_l] = memory_cells(kept.free);
        statement
            .execute(meguri_storage::rusqlite::params_from_iter([
                Cell::Integer(record.id),
                Cell::Integer(record.key.did),
                Cell::Integer(record.key.odid),
                Cell::Integer(i64::from(record.suspended)),
                optional(record.table_state.map(|state| state.index() as i64)),
                optional(record.newest_row),
                optional(record.first_review),
                optional(kept.preset),
                Cell::Integer(kept.cut.code()),
                cut_s,
                cut_d,
                cut_l,
                Cell::Integer(kept.free.code()),
                free_s,
                free_d,
                free_l,
                optional(kept.failed_at),
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

fn params_blob(params: &[f32]) -> Vec<u8> {
    params.iter().flat_map(|value| value.to_le_bytes()).collect()
}

fn blob_params(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect()
}

/// Writes the values of `presets` a replay used. A preset already kept with
/// other values means a change the capture missed: the component rebuilds.
fn put_presets<'a>(tables: &ComponentTables<'_>, presets: impl Iterator<Item = &'a PresetInputs>) -> Result<(), EngineError> {
    let table = tables.table(PRESETS)?;
    for inputs in presets {
        if let Some(kept) = read_preset(tables, inputs.id)? {
            if !kept.inputs.same(inputs) {
                return Err(internal("a preset's values changed without a rebuild"));
            }
            continue;
        }
        tables
            .conn()
            .prepare_cached(&format!(
                "INSERT INTO \"{table}\" (id, params, usable, decay, cutoff, retention) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            ))
            .map_err(sql)?
            .execute(meguri_storage::rusqlite::params_from_iter([
                Cell::Integer(inputs.id),
                Cell::Blob(params_blob(&inputs.params)),
                Cell::Integer(i64::from(inputs.usable)),
                // The bits: a refused list may end in a NaN, which SQLite
                // would store as NULL.
                Cell::Integer(i64::from(inputs.decay.to_bits())),
                optional(inputs.cutoff),
                Cell::Integer(i64::from(inputs.retention.to_bits())),
            ]))
            .map_err(sql)?;
    }
    Ok(())
}

fn preset_of_cells(cells: &[Cell]) -> Option<Preset> {
    let (Some(Cell::Integer(id)), Some(Cell::Blob(params)), Some(Cell::Integer(usable)), Some(Cell::Integer(decay)), Some(cutoff), Some(Cell::Integer(retention))) =
        (cells.first(), cells.get(1), cells.get(2), cells.get(3), cells.get(4), cells.get(5))
    else {
        return None;
    };
    Some(Preset::new(PresetInputs {
        id: *id,
        params: blob_params(params),
        usable: *usable == 1,
        decay: f32::from_bits(u32::try_from(*decay).ok()?),
        cutoff: match cutoff {
            Cell::Integer(cutoff) => Some(*cutoff),
            _ => None,
        },
        retention: f32::from_bits(u32::try_from(*retention).ok()?),
    }))
}

const PRESET_COLUMNS: &str = "id, params, usable, decay, cutoff, retention";

fn read_preset(tables: &ComponentTables<'_>, id: i64) -> Result<Option<Preset>, EngineError> {
    let table = tables.table(PRESETS)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!("SELECT {PRESET_COLUMNS} FROM \"{table}\" WHERE id = ?1"))
        .map_err(sql)?;
    let mut found = statement.query([id]).map_err(sql)?;
    match found.next().map_err(sql)? {
        Some(row) => {
            let cells = [
                Cell::Integer(row.get(0).map_err(sql)?),
                Cell::Blob(row.get(1).map_err(sql)?),
                Cell::Integer(row.get(2).map_err(sql)?),
                Cell::Integer(row.get(3).map_err(sql)?),
                optional(row.get(4).map_err(sql)?),
                Cell::Integer(row.get(5).map_err(sql)?),
            ];
            preset_of_cells(&cells)
                .map(Some)
                .ok_or_else(|| internal("an unreadable kept preset"))
        }
        None => Ok(None),
    }
}

/// The home preset a card's kept knowledge was replayed with: `Ok(None)`
/// when it had none (or no record).
fn kept_preset(tables: &ComponentTables<'_>, card: i64) -> Result<Option<Preset>, EngineError> {
    let table = tables.table(CARDS)?;
    let preset: Option<Option<i64>> = {
        let mut statement = tables
            .conn()
            .prepare_cached(&format!("SELECT preset FROM \"{table}\" WHERE id = ?1"))
            .map_err(sql)?;
        let mut found = statement.query([card]).map_err(sql)?;
        match found.next().map_err(sql)? {
            Some(row) => Some(row.get::<_, Option<i64>>(0).map_err(sql)?),
            None => None,
        }
    };
    match preset.flatten() {
        Some(id) => read_preset(tables, id)?
            .map(Some)
            .ok_or_else(|| internal("a card's kept preset is missing")),
        None => Ok(None),
    }
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

/// Writes kept samples into empty tables (a build).
fn insert_samples(tables: &ComponentTables<'_>, samples: &[((Key, i64), SampleValues)]) -> Result<(), EngineError> {
    let table = tables.table(KNOWLEDGE)?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!(
            "INSERT INTO \"{table}\" (did, odid, day, recall, studied) VALUES (?1, ?2, ?3, ?4, ?5)"
        ))
        .map_err(sql)?;
    for ((key, day), values) in samples {
        statement
            .execute([key.did, key.odid, *day, values[0], values[1]])
            .map_err(sql)?;
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
    let samples = tables.table(KNOWLEDGE)?;
    let mut add = conn
        .prepare_cached(&format!(
            "INSERT INTO \"{samples}\" (did, odid, day, recall, studied) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (did, odid, day) DO UPDATE SET recall = recall + excluded.recall, \
             studied = studied + excluded.studied"
        ))
        .map_err(sql)?;
    // A sample brought to nothing stays as a zero row, which reads as no
    // row does: an update then costs one statement per sample, and the
    // next build drops them.
    for ((key, day), values) in &sums.knowledge {
        add.execute([key.did, key.odid, *day, values[0], values[1]]).map_err(sql)?;
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
/// the unreadable cards and, with `presets`, the records today's point reads
/// (the build, and H18's read without the component).
pub(crate) struct Replay {
    pub sums: Sums,
    pub bad: Vec<BadCard>,
    pub now: Vec<NowInputs>,
    /// The home presets the replay used, by id.
    pub presets: HashMap<i64, Preset>,
}

pub(crate) fn replay_all(
    source: &impl ReadRows,
    grid: Grid,
    frame: Option<Frame>,
    cancel: &dyn Fn() -> bool,
) -> Result<Replay, StorageError> {
    let presets = match frame {
        Some(_) => Presets::read(source)?,
        None => Presets {
            decks: HashMap::new(),
            presets: HashMap::new(),
        },
    };
    // Without a frame, H2 alone.
    let days = frame.map(Frame::kept_days);
    let work = Work {
        grid,
        days: days.as_deref(),
        presets: &presets,
    };
    let mut bad = Vec::new();
    let mut now = Vec::new();
    let sums = replay_cards(source, &work, cancel, &mut |done| {
        for card in done.cards {
            match (card.contribution, card.kept) {
                (Contribution::Bad(entry), _) => bad.push(entry),
                (_, Some(kept)) if frame.is_some() => now.push(now_inputs(&kept, |id| presets.presets.get(&id))),
                _ => {}
            }
        }
        true
    })?;
    Ok(Replay {
        sums,
        bad,
        now,
        presets: presets.presets,
    })
}

/// What today's point reads of a record, with its preset's decay and
/// cutoff.
fn now_inputs<'a>(kept: &Kept, preset: impl Fn(i64) -> Option<&'a Preset>) -> NowInputs {
    let found = kept.preset.and_then(preset);
    NowInputs {
        id: kept.card.id,
        key: kept.card.key,
        suspended: kept.card.suspended,
        preset: kept.preset,
        decay: found.map(Preset::decay),
        cutoff: found.and_then(|preset| preset.inputs.cutoff),
        cut: kept.cut,
        free: kept.free,
        failed_at: kept.failed_at,
        newest_row: kept.card.newest_row,
    }
}

/// Today's point of a card with a row after the request's `N`
/// (`next_day_at`): replayed from its rows in `source` with I4's days
/// counted back from that `N`, as H3.1 says, instead of from the grid's
/// far boundary that serves every past sample.
pub(crate) fn now_at(source: &impl ReadRows, grid: Grid, card: &NowInputs, preset: Option<&Preset>, next_day_at: i64) -> Result<NowInputs, StorageError> {
    let rows: Vec<rows::StatsRow> = {
        let mut read: Vec<rows::StatsRow> = Vec::new();
        for image in source.card_revlog_images(CardId(card.id))? {
            match rows::read_row(&image) {
                RowRead::Row { row, cid } if cid == card.id && row.id >= 0 => read.push(row),
                // An unreadable row fails the part before this (G4).
                _ => {}
            }
        }
        read.sort_by_key(|row| row.id);
        read
    };
    let known = knowledge(grid, &[], card.id, &rows, preset, next_day_at);
    Ok(NowInputs {
        cut: known.cut,
        free: known.free,
        failed_at: match (known.failed_at, card.failed_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        },
        ..*card
    })
}

impl DerivedComponent for StatsHistory {
    type Delta = HistoryDelta;

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new(HISTORY_COMPONENT, VERSION)
            .table(TableDef::columns(
                META,
                "(id INTEGER PRIMARY KEY, phase INTEGER NOT NULL, latest INTEGER NOT NULL, anchor INTEGER NOT NULL)",
            ))
            .table(TableDef::columns(
                CARDS,
                "(id INTEGER PRIMARY KEY, did INTEGER NOT NULL, odid INTEGER NOT NULL, suspended INTEGER NOT NULL, \
                 state INTEGER, newest_row INTEGER, first_review INTEGER, preset INTEGER, \
                 cut_code INTEGER NOT NULL, cut_s REAL, cut_d REAL, cut_l INTEGER, \
                 free_code INTEGER NOT NULL, free_s REAL, free_d REAL, free_l INTEGER, failed_at INTEGER)",
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
                KNOWLEDGE,
                "(did INTEGER NOT NULL, odid INTEGER NOT NULL, day INTEGER NOT NULL, recall INTEGER NOT NULL, \
                 studied INTEGER NOT NULL, PRIMARY KEY (did, odid, day)) WITHOUT ROWID",
            ))
            .table(TableDef::columns(
                PRESETS,
                "(id INTEGER PRIMARY KEY, params BLOB NOT NULL, usable INTEGER NOT NULL, decay INTEGER NOT NULL, \
                 cutoff INTEGER, retention INTEGER NOT NULL)",
            ))
            .table(TableDef::columns(
                BAD,
                "(card INTEGER PRIMARY KEY, did INTEGER, odid INTEGER, row INTEGER, code INTEGER NOT NULL)",
            ))
    }

    /// Without the committed rows the touched cards cannot be read again.
    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<HistoryDelta> {
        let payload = &commit.payload;
        if payload.row_keys(Table::Cards).next().is_none()
            && payload.row_keys(Table::Revlog).next().is_none()
            && payload.row_keys(Table::DeckConfig).next().is_none()
            && payload.row_keys(Table::Decks).next().is_none()
        {
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

    /// Each touched card replayed twice on the target's grid and frame, as
    /// it was (with the preset values its record was replayed with) and as
    /// it is (with the captured ones): the first taken out, the second put
    /// in (H14.3). A target with no grid cannot be followed: the component
    /// rebuilds.
    fn apply(&self, tables: &ComponentTables<'_>, HistoryDelta(changes): HistoryDelta) -> Result<(), EngineError> {
        let (grid, frame) = read_meta(tables)?.ok_or_else(|| internal("the stats history keeps no day grid"))?;
        let days = frame.kept_days();
        let mut sums = Sums::default();
        let mut records = Vec::new();
        let mut bad = Vec::new();
        let mut used: Vec<PresetInputs> = Vec::new();
        for change in &changes {
            if let Some(before) = &change.before {
                let preset = kept_preset(tables, change.id)?;
                replay_card(grid, Some(&days), before, preset.as_ref(), &mut sums, -1);
            }
            delete_card(tables, change.id)?;
            let Some(after) = &change.after else { continue };
            let preset = change.preset.clone().map(Preset::new);
            let replayed = replay_card(grid, Some(&days), after, preset.as_ref(), &mut sums, 1);
            match (replayed.contribution, replayed.kept) {
                (Contribution::Bad(entry), _) => bad.push(entry),
                (_, Some(kept)) => {
                    records.push(kept);
                    if let Some(inputs) = &change.preset {
                        used.push(inputs.clone());
                    }
                }
                _ => {}
            }
        }
        sums.prune();
        add_sums(tables, &sums)?;
        put_presets(tables, used.iter())?;
        put_records(tables, &records)?;
        put_bad(tables, &bad)
    }

    /// H14.2: one pass over every card with its rows, replayed on every
    /// core, the records written in batches as they come, then the per-key
    /// sums, the presets and the grid.
    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError> {
        let (grid, frame) = self.shared.wanted();
        let total = source.row_count(Table::Cards)? + source.row_count(Table::Revlog)?;
        let presets = Presets::read(&**source)?;
        let days = frame.kept_days();
        let work = Work {
            grid,
            days: Some(&days),
            presets: &presets,
        };
        let mut bad: Vec<BadCard> = Vec::new();
        let mut records: Vec<Kept> = Vec::with_capacity(BATCH);
        let mut cards = 0u64;
        let mut reported = 0u64;
        let mut done = 0u64;
        let mut stopped: Option<EngineError> = None;
        let sums = replay_cards(&**source, &work, &|| false, &mut |finished| {
            done += finished.weight;
            for card in finished.cards {
                cards += 1;
                match (card.contribution, card.kept) {
                    (Contribution::Bad(entry), _) => bad.push(entry),
                    (_, Some(kept)) => records.push(kept),
                    _ => {}
                }
            }
            if records.len() >= BATCH {
                let batch = std::mem::replace(&mut records, Vec::with_capacity(BATCH));
                if let Err(error) = sink.write(move |tables| put_records(tables, &batch)) {
                    stopped = Some(error);
                    return false;
                }
            }
            if cards - reported >= CHECK_EVERY {
                reported = cards;
                if let Err(error) = sink.check() {
                    stopped = Some(error);
                    return false;
                }
                sink.progress(done.min(total), Some(total));
            }
            true
        });
        if let Some(error) = stopped {
            return Err(error);
        }
        let mut sums = sums?;
        let mut days: Vec<((Key, i64), DayValues)> = std::mem::take(&mut sums.days).into_iter().collect();
        while !days.is_empty() {
            let batch: Vec<((Key, i64), DayValues)> = days.split_off(days.len().saturating_sub(BATCH));
            sink.write(move |tables| insert_days(tables, &batch))?;
        }
        let mut samples: Vec<((Key, i64), SampleValues)> = std::mem::take(&mut sums.knowledge).into_iter().collect();
        while !samples.is_empty() {
            let batch: Vec<((Key, i64), SampleValues)> = samples.split_off(samples.len().saturating_sub(BATCH));
            sink.write(move |tables| insert_samples(tables, &batch))?;
        }
        let used: Vec<PresetInputs> = presets.presets.into_values().map(|preset| preset.inputs).collect();
        // The grid last: a build that stops before it leaves none.
        sink.write(move |tables| {
            put_records(tables, &records)?;
            put_bad(tables, &bad)?;
            insert_keys(tables, &sums)?;
            put_presets(tables, used.iter())?;
            write_meta(tables, grid, frame)
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
    Ok(kept_meta(snapshot)?.map(|(grid, _)| grid))
}

/// The grid and the knowledge frame of the kept content.
pub(crate) fn kept_meta(snapshot: &Snapshot<'_>) -> Result<Option<(Grid, Frame)>, StorageError> {
    let rows = snapshot.derived_rows(HISTORY_COMPONENT, META, "phase, latest, anchor")?;
    Ok(rows.first().and_then(|row| match row.as_slice() {
        [Cell::Integer(phase), Cell::Integer(latest), Cell::Integer(anchor)] => Some((
            Grid::from_phase(*phase),
            Frame {
                latest: *latest,
                anchor: *anchor,
            },
        )),
        _ => None,
    }))
}

fn integer(cells: &[Cell], at: usize) -> Option<i64> {
    match cells.get(at) {
        Some(Cell::Integer(value)) => Some(*value),
        _ => None,
    }
}

fn real(cells: &[Cell], at: usize) -> Option<f64> {
    match cells.get(at) {
        Some(Cell::Real(value)) => Some(*value),
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

/// The kept samples of every key.
pub(crate) fn visit_kept_samples(
    snapshot: &Snapshot<'_>,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(Key, i64, &SampleValues),
) -> Result<(), StorageError> {
    snapshot.visit_derived(HISTORY_COMPONENT, KNOWLEDGE, "did, odid, day, recall, studied", cancel, &mut |cells| {
        if let (Some(did), Some(odid), Some(day), Some(recall), Some(studied)) =
            (integer(cells, 0), integer(cells, 1), integer(cells, 2), integer(cells, 3), integer(cells, 4))
        {
            each(Key { did, odid }, day, &[recall, studied]);
        }
        true
    })
}

/// The kept presets, by id.
pub(crate) fn kept_presets(snapshot: &Snapshot<'_>) -> Result<HashMap<i64, Preset>, StorageError> {
    let mut presets = HashMap::new();
    snapshot.visit_derived(HISTORY_COMPONENT, PRESETS, PRESET_COLUMNS, &|| false, &mut |cells| {
        if let Some(preset) = preset_of_cells(cells) {
            presets.insert(preset.inputs.id, preset);
        }
        true
    })?;
    Ok(presets)
}

fn memory_of(cells: &[Cell], at: usize) -> Memory {
    match integer(cells, at) {
        Some(1) => match (real(cells, at + 1), real(cells, at + 2), integer(cells, at + 3)) {
            (Some(stability), Some(difficulty), Some(last)) => Memory::State {
                stability: stability as f32,
                difficulty: difficulty as f32,
                last,
            },
            _ => Memory::Failed,
        },
        Some(2) => Memory::Failed,
        _ => Memory::None,
    }
}

/// What today's point reads of each kept record (H3.1).
pub(crate) fn visit_kept_now(
    snapshot: &Snapshot<'_>,
    presets: &HashMap<i64, Preset>,
    cancel: &dyn Fn() -> bool,
    each: &mut dyn FnMut(&NowInputs),
) -> Result<(), StorageError> {
    // Only the records today's point can count: a suspended card, or one
    // with no item and no failure, adds nothing.
    snapshot.visit_derived_where(
        HISTORY_COMPONENT,
        CARDS,
        "did, odid, suspended, preset, cut_code, cut_s, cut_d, cut_l, free_code, free_s, free_d, free_l, id, failed_at, newest_row",
        "suspended = 0 AND (cut_code != 0 OR free_code != 0 OR failed_at IS NOT NULL)",
        cancel,
        &mut |cells| {
            let (Some(did), Some(odid), Some(id)) = (integer(cells, 0), integer(cells, 1), integer(cells, 12)) else {
                return true;
            };
            let found = integer(cells, 3).and_then(|id| presets.get(&id));
            each(&NowInputs {
                id,
                key: Key { did, odid },
                suspended: integer(cells, 2) == Some(1),
                preset: integer(cells, 3),
                decay: found.map(Preset::decay),
                cutoff: found.and_then(|preset| preset.inputs.cutoff),
                cut: memory_of(cells, 4),
                free: memory_of(cells, 8),
                failed_at: integer(cells, 13),
                newest_row: integer(cells, 14),
            });
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

/// The kept memory records, for tests (H14.1): card, home preset, and its
/// memory over every row with and without the cutoff.
pub(crate) fn visit_kept_memory_records(
    snapshot: &Snapshot<'_>,
    each: &mut dyn FnMut(i64, Option<i64>, Memory, Memory),
) -> Result<(), StorageError> {
    snapshot.visit_derived(
        HISTORY_COMPONENT,
        CARDS,
        "id, preset, cut_code, cut_s, cut_d, cut_l, free_code, free_s, free_d, free_l",
        &|| false,
        &mut |cells| {
            if let Some(id) = integer(cells, 0) {
                each(id, integer(cells, 1), memory_of(cells, 2), memory_of(cells, 6));
            }
            true
        },
    )
}
