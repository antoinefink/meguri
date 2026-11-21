//! Card Info (`stats/card-info`): the read-only snapshot of one card, built
//! from one snapshot of the collection. It never writes (C1): a missing
//! memory state or `lrt` is computed in memory and dropped with it.

pub mod curve;
pub mod dump;
pub mod freshness;
pub mod texts;

use fsrs::FSRS;
use meguri_model::{
    card::{Card, CardType},
    card_data::{CardData, MemoryState},
    clock::Clock,
    deck::Deck,
    deck_config::DeckConfig,
    ids::{CardId, DeckConfigId, NoteId, RevlogId},
    note::Note,
    notetype::Notetype,
    proto::deck::DeckKind,
    revlog::{RevlogEntry, RevlogKind},
};
use meguri_sched::{
    memory::{
        ignore_before,
        items::{History, HistoryForm, is_graded},
    },
    model::{deck_sched::DeckSched, preset::PresetView},
};
use meguri_storage::{Cell, ReadRows, Record, RowImage, RowKey, StorageError, Table};

pub use curve::{Curve, CurvePoint, CurveRange, RangeKind};
pub use freshness::Dependencies;
pub use texts::{EntryTexts, StatRow, StatsKey};

/// What the snapshot is built for (C1): the card and the clock's instant,
/// with the day clock's day number and next day start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub card_id: CardId,
    /// Epoch seconds.
    pub now: i64,
    pub days_elapsed: u32,
    /// Epoch seconds.
    pub next_day_at: i64,
}

/// C11's kinds, in the order they are tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    NotFound,
    UnreadableRecord,
    MissingNote,
    MissingNotetype,
    MissingTemplate,
    MissingDeck,
    MissingHomeDeck,
    FilteredHomeDeck,
    MissingPreset,
    InvalidCutoff,
    FsrsFailed,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::UnreadableRecord => "unreadable_record",
            ErrorKind::MissingNote => "missing_note",
            ErrorKind::MissingNotetype => "missing_notetype",
            ErrorKind::MissingTemplate => "missing_template",
            ErrorKind::MissingDeck => "missing_deck",
            ErrorKind::MissingHomeDeck => "missing_home_deck",
            ErrorKind::FilteredHomeDeck => "filtered_home_deck",
            ErrorKind::MissingPreset => "missing_preset",
            ErrorKind::InvalidCutoff => "invalid_cutoff",
            ErrorKind::FsrsFailed => "fsrs_failed",
        }
    }
}

#[derive(Debug)]
pub enum CardInfoError {
    /// The snapshot fails as a whole with one of C11's kinds.
    Unavailable(ErrorKind),
    /// The database itself failed (not a damaged record).
    Storage(StorageError),
}

impl std::fmt::Display for CardInfoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CardInfoError::Unavailable(kind) => write!(f, "card info unavailable: {}", kind.as_str()),
            CardInfoError::Storage(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CardInfoError {}

impl From<StorageError> for CardInfoError {
    fn from(error: StorageError) -> Self {
        CardInfoError::Storage(error)
    }
}

impl From<ErrorKind> for CardInfoError {
    fn from(kind: ErrorKind) -> Self {
        CardInfoError::Unavailable(kind)
    }
}

/// Which of C3.4's sources a memory state came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySource {
    Stored,
    Computed,
}

impl MemorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            MemorySource::Stored => "stored",
            MemorySource::Computed => "computed",
        }
    }
}

/// C3's raw values, whether or not their rows show.
#[derive(Clone, Debug, PartialEq)]
pub struct RawValues {
    pub card_id: CardId,
    pub note_id: NoteId,
    /// Epoch seconds.
    pub added: i64,
    pub card_type: String,
    pub notetype: String,
    pub deck: String,
    pub original_deck: Option<String>,
    pub preset: String,
    pub interval: u32,
    pub ease: u16,
    pub reviews: u32,
    pub lapses: u32,
    pub custom_data: String,
    pub desired_retention: Option<f32>,
    pub fsrs_params: Vec<f32>,
    pub first_review: Option<i64>,
    pub latest_review: Option<i64>,
    pub due_date: Option<i64>,
    pub due_position: Option<i32>,
    pub memory_state: Option<MemoryState>,
    pub memory_state_source: Option<MemorySource>,
    pub fsrs_retrievability: Option<f32>,
    pub average_secs: f32,
    pub total_secs: f32,
}

/// One review-log row (C6.1) with its texts (C6.2).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub revlog_id: RevlogId,
    /// Epoch seconds.
    pub instant: i64,
    pub kind: RevlogKind,
    /// The row's `ease`, 0 for unrated rows.
    pub rating: u8,
    pub interval_secs: i64,
    pub last_interval_secs: i64,
    pub factor: u32,
    pub taken_secs: f32,
    pub memory_state: Option<MemoryState>,
    pub texts: EntryTexts,
}

/// The Card Info snapshot (C1).
#[derive(Clone, Debug)]
pub struct CardInfo {
    pub raw: RawValues,
    /// C4: the applicable rows in order.
    pub rows: Vec<StatRow>,
    /// C6: newest first.
    pub history: Vec<HistoryEntry>,
    /// C7: `None` when the view shows no chart.
    pub curve: Option<Curve>,
    /// C10.
    pub dependencies: Dependencies,
    source: Source,
}

/// The records the snapshot read, kept for the debug dump (C8).
#[derive(Clone, Debug)]
struct Source {
    card_image: RowImage,
    revlog_images: Vec<RowImage>,
    deck: Deck,
    home_deck: Option<Deck>,
    preset: DeckConfig,
    rollover: Option<Vec<u8>>,
}

enum Read<T> {
    Found(T),
    Missing,
    Unreadable,
}

impl<T> Read<T> {
    fn of(result: Result<Option<T>, StorageError>) -> Result<Read<T>, StorageError> {
        match result {
            Ok(Some(value)) => Ok(Read::Found(value)),
            Ok(None) => Ok(Read::Missing),
            Err(StorageError::Row(_)) => Ok(Read::Unreadable),
            Err(error) => Err(error),
        }
    }

    fn found(&self) -> Option<&T> {
        match self {
            Read::Found(value) => Some(value),
            _ => None,
        }
    }
}

/// C3.5 step 1's row test, on the stored columns: `ease` 1 to 4 and not
/// both `type` 3 and `factor` 0 (a `type` 259 row counts here).
fn counts_for_last_review(image: &RowImage, row: &RevlogEntry) -> bool {
    let stored_type = match image.get(8) {
        Some(Cell::Integer(value)) => Some(*value as f64),
        Some(Cell::Real(value)) => Some(*value),
        _ => None,
    };
    (1..=4).contains(&row.button) && !(stored_type == Some(3.0) && row.ease_factor == 0)
}

/// C6.1: `ivl` above 0 in days, otherwise seconds negated; both saturate.
fn interval_secs(ivl: i32) -> i64 {
    let secs = if ivl > 0 {
        i64::from(ivl) * 86_400
    } else {
        -i64::from(ivl)
    };
    secs.min(i64::from(i32::MAX))
}

/// The FSRS model of the home preset's list (`sched/presets#V6`), or C11's
/// `fsrs_failed` when the crate refuses it.
fn model(preset: &PresetView) -> Result<FSRS, ErrorKind> {
    let params = preset.parameters();
    if !matches!(params.len(), 0 | 17 | 19 | 21) || params.iter().any(|p| !p.is_finite()) {
        return Err(ErrorKind::FsrsFailed);
    }
    FSRS::new(params).map_err(|_| ErrorKind::FsrsFailed)
}

/// The `data` column's stored text.
fn data_text(image: &RowImage) -> String {
    let index = Table::Cards
        .columns()
        .iter()
        .position(|column| *column == "data")
        .expect("cards has a data column");
    match image.get(index) {
        Some(Cell::Text(bytes) | Cell::Blob(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    }
}

/// Builds the snapshot of `input.card_id` on `rows` (one snapshot of the
/// collection), with dates in the zone `clock` reports. Reads only.
pub fn build(rows: &impl ReadRows, clock: &dyn Clock, input: &Input) -> Result<CardInfo, CardInfoError> {
    let Some(card_image) = rows.row_image(Table::Cards, &RowKey::Id(input.card_id.0))? else {
        return Err(ErrorKind::NotFound.into());
    };
    let card = Card::from_cells(&card_image).map_err(|_| ErrorKind::UnreadableRecord)?;
    let revlog_images = rows.card_revlog_images(card.id)?;
    let decoded: Vec<Option<RevlogEntry>> = revlog_images
        .iter()
        .map(|image| RevlogEntry::from_cells(image).ok())
        .collect();
    let mut unreadable = decoded.iter().any(Option::is_none);
    let note: Read<Note> = Read::of(rows.note(card.note_id))?;
    let notetype: Read<Notetype> = match note.found() {
        Some(note) => Read::of(rows.notetype(note.notetype_id))?,
        None => Read::Missing,
    };
    let deck: Read<Deck> = Read::of(rows.deck(card.deck_id))?;
    let home: Read<Deck> = if card.original_deck_id.0 != 0 {
        Read::of(rows.deck(card.original_deck_id))?
    } else {
        match &deck {
            Read::Found(deck) => Read::Found(deck.clone()),
            Read::Missing => Read::Missing,
            Read::Unreadable => Read::Unreadable,
        }
    };
    let preset_id = home.found().and_then(|home| match &home.kind.kind {
        DeckKind::Normal(normal) => Some(DeckConfigId(normal.config_id)),
        DeckKind::Filtered(_) => None,
    });
    let preset: Read<DeckConfig> = match preset_id {
        Some(id) => Read::of(rows.deck_config(id))?,
        None => Read::Missing,
    };
    unreadable |= [
        matches!(note, Read::Unreadable),
        matches!(notetype, Read::Unreadable),
        matches!(deck, Read::Unreadable),
        matches!(home, Read::Unreadable),
        matches!(preset, Read::Unreadable),
    ]
    .contains(&true);
    if unreadable {
        return Err(ErrorKind::UnreadableRecord.into());
    }
    if !matches!(note, Read::Found(_)) {
        return Err(ErrorKind::MissingNote.into());
    }
    let Read::Found(notetype) = notetype else {
        return Err(ErrorKind::MissingNotetype.into());
    };
    let cloze = notetype.config.kind == 1;
    let template = if cloze {
        notetype.templates.first()
    } else {
        notetype.templates.iter().find(|t| t.ord == u32::from(card.ord))
    }
    .ok_or(ErrorKind::MissingTemplate)?;
    let Read::Found(deck) = deck else {
        return Err(ErrorKind::MissingDeck.into());
    };
    let Read::Found(home) = home else {
        return Err(ErrorKind::MissingHomeDeck.into());
    };
    let DeckKind::Normal(normal) = &home.kind.kind else {
        return Err(ErrorKind::FilteredHomeDeck.into());
    };
    let Read::Found(preset_row) = preset else {
        return Err(ErrorKind::MissingPreset.into());
    };
    let preset = PresetView::new(&preset_row);
    let cutoff = ignore_before::cutoff_millis(&preset.fields().ignore_revlogs_before_date)
        .map_err(|_| ErrorKind::InvalidCutoff)?;
    let model = model(&preset)?;
    let entries: Vec<RevlogEntry> = decoded.into_iter().map(|row| row.expect("checked readable")).collect();

    let (data, custom_data) = CardData::parse_for_card_info(&data_text(&card_image));
    let rated = || entries.iter().filter(|row| row.button >= 1);
    let instant = |row: &RevlogEntry| row.id.0 / 1000;
    let retention = preset.fields().historical_retention;
    let history = History::select(&entries, cutoff, HistoryForm::Memory);

    // C3.4 and C3.5.
    let (memory_state, memory_state_source) = match data.memory_state {
        Some(state) => (Some(state), Some(MemorySource::Stored)),
        None if card.card_type != CardType::New => match &history {
            Some(history) => (
                Some(
                    history
                        .memory(&model, input.next_day_at, retention)
                        .map_err(|_| ErrorKind::FsrsFailed)?,
                ),
                Some(MemorySource::Computed),
            ),
            None => (None, None),
        },
        None => (None, None),
    };
    let fsrs_retrievability = memory_state.map(|state| {
        let last = data
            .last_review_secs
            .or_else(|| {
                entries
                    .iter()
                    .zip(&revlog_images)
                    .rev()
                    .find(|(row, image)| counts_for_last_review(image, row))
                    .map(|(row, _)| instant(row))
            })
            .unwrap_or(0);
        let elapsed = input.now.wrapping_sub(last) as u32;
        let days = elapsed as f32 / 86_400.0;
        let decay = match memory_state_source {
            Some(MemorySource::Stored) => data.decay.unwrap_or(0.5),
            _ => preset.decay(),
        };
        fsrs::current_retrievability(
            fsrs::MemoryState {
                stability: state.stability,
                difficulty: state.difficulty,
            },
            days,
            decay,
        )
    });

    // C3.3.
    let due_from_day_number;
    let due_date = if card.card_type == CardType::New {
        due_from_day_number = false;
        None
    } else {
        let x = i64::from(if card.original_due != 0 {
            card.original_due
        } else {
            card.due
        });
        due_from_day_number = x <= 1_000_000_000;
        Some(if due_from_day_number {
            input.now + (x - i64::from(input.days_elapsed)) * 86_400
        } else {
            x
        })
    };
    let due_position = match data.new_position {
        Some(position) => Some(position as i32),
        None if card.card_type == CardType::New => Some(card.due),
        None => None,
    };

    // C3.6.
    let total: f32 = entries
        .iter()
        .fold(0.0f32, |sum, row| sum + row.answer_millis as f32 / 1000.0);
    let rated_count = rated().count();
    let (average_secs, total_secs) = if rated_count == 0 || total == 0.0 {
        (0.0, 0.0)
    } else {
        (total / rated_count as f32, total)
    };

    let home_name = (card.original_deck_id.0 != 0 && card.original_deck_id != card.deck_id).then(|| home.human_name());
    let raw = RawValues {
        card_id: card.id,
        note_id: card.note_id,
        added: card.id.0 / 1000,
        card_type: template.name.clone(),
        notetype: notetype.name.clone(),
        deck: deck.human_name(),
        original_deck: home_name.clone(),
        preset: preset_row.name.clone(),
        interval: card.interval,
        ease: card.ease_factor,
        reviews: card.reps,
        lapses: card.lapses,
        custom_data,
        desired_retention: data.desired_retention,
        fsrs_params: preset.parameters().to_vec(),
        first_review: rated().next().map(instant),
        latest_review: rated().next_back().map(instant),
        due_date,
        due_position,
        memory_state,
        memory_state_source,
        fsrs_retrievability,
        average_secs,
        total_secs,
    };

    // C6.3: one replay; every row then gets a kept row's state.
    let mut states: Vec<Option<MemoryState>> = vec![None; entries.len()];
    if let Some(history) = &history {
        let replayed = history
            .historical(&model, input.next_day_at, retention)
            .map_err(|_| ErrorKind::FsrsFailed)?;
        let start = entries.len() - history.len();
        let kept: Vec<usize> = (start..entries.len())
            .filter(|&index| is_graded(&entries[index]))
            .collect();
        // The crate gives one state per kept row (C6.3).
        if kept.len() == replayed.len() && !kept.is_empty() {
            let mut next = 0usize;
            for (index, state) in states.iter_mut().enumerate() {
                while next < kept.len() && kept[next] < index {
                    next += 1;
                }
                *state = if index < kept[0] {
                    None
                } else if next < kept.len() {
                    Some(replayed[next])
                } else {
                    replayed.last().copied()
                };
            }
        } else {
            return Err(ErrorKind::FsrsFailed.into());
        }
    }

    let history: Vec<HistoryEntry> = entries
        .iter()
        .zip(&states)
        .rev()
        .map(|(row, state)| {
            let mut entry = HistoryEntry {
                revlog_id: row.id,
                instant: instant(row),
                kind: row.kind,
                rating: row.button,
                interval_secs: interval_secs(row.interval),
                last_interval_secs: interval_secs(row.last_interval),
                factor: row.ease_factor,
                taken_secs: row.answer_millis as f32 / 1000.0,
                memory_state: *state,
                texts: EntryTexts::default(),
            };
            entry.texts = texts::entry_texts(&entry, clock);
            entry
        })
        .collect();

    // C7.
    let curve = memory_state.and_then(|_| {
        let mut reviews = Vec::new();
        for entry in &history {
            let reset = entry.kind == RevlogKind::Manual && entry.factor == 0;
            let Some(state) = entry.memory_state.filter(|_| !reset) else {
                break;
            };
            let skipped = matches!(entry.kind, RevlogKind::Manual | RevlogKind::Rescheduled)
                || (entry.kind == RevlogKind::Filtered && entry.factor == 0);
            if !skipped {
                reviews.push(curve::CurveReview {
                    instant: entry.instant,
                    interval_secs: entry.interval_secs,
                    stability: f64::from(state.stability),
                });
            }
        }
        reviews.reverse();
        let line = DeckSched::new(Some(normal), &home.common).retention(preset.fields().desired_retention);
        curve::build(&reviews, input.now, preset.decay(), line)
    });

    let rows_out = texts::stat_rows(&raw, clock);
    let dependencies = Dependencies {
        card: card.id,
        note: card.note_id,
        notetype: notetype.id,
        deck: card.deck_id,
        home_deck: home.id,
        preset: preset_row.id,
        revlog: entries.iter().map(|row| row.id).collect(),
        due_from_day_number,
        generation: 0,
    };
    let rollover = rows.config_entry("rollover")?.map(|stored| stored.entry.value);
    Ok(CardInfo {
        raw,
        rows: rows_out,
        history,
        curve,
        dependencies,
        source: Source {
            card_image,
            revlog_images,
            deck: deck.clone(),
            home_deck: home_name.map(|_| home.clone()),
            preset: preset_row.clone(),
            rollover,
        },
    })
}

impl CardInfo {
    /// C8: the debug dump, UTF-8 JSON with two-space indent.
    pub fn debug_json(&self) -> String {
        dump::write(self)
    }
}
