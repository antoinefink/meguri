//! Browse cell values (`specs/browser/columns.md` C4 to C9, C11, C12 time
//! spans, C13): typed values computed from decoded records. The engine reads
//! the records and resolves names (`meguri_engine::browse::rows`); nothing
//! here reads the collection or renders a template.

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::extractors::Fields;
use meguri_model::note::Note;

use super::qa::{Answer, Question};

/// Now and today of one row request (C1): read once, used for every row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowTime {
    /// Epoch seconds.
    pub now: i64,
    /// The scheduler's day number.
    pub today: u32,
}

/// One cell (C9). Due values carry C7.5's wrapped flag.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Text(String),
    Count(u64),
    /// Notes-mode Deck spread over this many decks (at least 2).
    DeckCount(u64),
    Percent(Percent),
    Position(i64),
    /// Epoch seconds, shown as a local date.
    Instant { secs: i64, wrapped: bool },
    /// Epoch seconds, shown as a local date and time to the minute.
    DateTime(i64),
    NewPosition { position: i64, wrapped: bool },
    Filtered { wrapped: bool },
    NewMarker,
    LearningMarker,
    /// Seconds as an `f32`.
    TimeSpan(f32),
    Empty,
    Question(Question),
    Answer(Answer),
}

/// A percent value (C9): an integer for Ease, an `f32` shown rounded for
/// Difficulty and Retrievability.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Percent {
    Integer(i64),
    Real(f32),
}

/// C4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    /// 0 none, 1 to 7.
    pub flag: u8,
    pub marked: bool,
    pub leech: bool,
    pub suspended: bool,
    pub buried: bool,
    pub filtered: bool,
}

/// C5's row background (round 8: the UI shows a state symbol instead; the
/// value stays the core's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    None,
    /// 1 to 7.
    Flag(u8),
    Marked,
    Suspended,
    Buried,
}

/// C6: the browser font of a template; each member absent on its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Font {
    pub name: Option<String>,
    pub size: Option<u32>,
}

impl Font {
    pub fn of(name: &str, size: u32) -> Font {
        Font {
            name: (!name.is_empty()).then(|| name.to_owned()),
            size: (size != 0).then_some(size),
        }
    }
}

/// C1: `odue` for a filtered card, `due` otherwise.
pub fn home_due(card: &Card) -> i32 {
    if is_filtered(card) { card.original_due } else { card.due }
}

pub fn is_filtered(card: &Card) -> bool {
    card.original_deck_id.0 != 0
}

fn queue_value(card: &Card) -> i64 {
    card.queue.stored()
}

/// C1: whether the card's due counts in days.
pub fn counts_in_days(card: &Card) -> bool {
    (card.card_type != CardType::New && home_due(card) <= 365_000)
        || matches!(card.queue, CardQueue::Review | CardQueue::InterdayLearning)
        || (card.card_type == CardType::Review && queue_value(card) < 0)
}

/// C7.5 rules 3 and 4: the instant of a learning card's timestamp or of a
/// day; `None` when neither applies.
fn due_instant(card: &Card, time: RowTime) -> Option<i64> {
    let due = i64::from(home_due(card));
    if card.queue == CardQueue::Learning {
        Some(due)
    } else if counts_in_days(card) {
        Some(time.now + (due - i64::from(time.today)) * 86_400)
    } else {
        None
    }
}

/// C7.5.
pub fn due(card: &Card, time: RowTime) -> Cell {
    let wrapped = queue_value(card) < 0;
    if is_filtered(card) {
        Cell::Filtered { wrapped }
    } else if card.card_type == CardType::New || card.queue == CardQueue::New {
        Cell::NewPosition {
            position: i64::from(card.due),
            wrapped,
        }
    } else {
        match due_instant(card, time) {
            Some(secs) => Cell::Instant { secs, wrapped },
            None => Cell::Empty,
        }
    }
}

/// C7.6.
pub fn ease(card: &Card) -> Cell {
    if card.card_type == CardType::New {
        Cell::NewMarker
    } else {
        Cell::Percent(Percent::Integer(i64::from(card.ease_factor / 10)))
    }
}

/// C7.7: `ivl` days as seconds, reduced modulo 2^32, as an `f32`.
pub fn interval(card: &Card) -> Cell {
    match card.card_type {
        CardType::New => Cell::NewMarker,
        CardType::Learning => Cell::LearningMarker,
        CardType::Review | CardType::Relearning => Cell::TimeSpan(day_seconds_u32(u64::from(card.interval)) as f32),
    }
}

fn day_seconds_u32(days: u64) -> u32 {
    days.wrapping_mul(86_400) as u32
}

/// C7.9: the note id divided by 1,000, truncated toward zero.
pub fn created(note_id: i64) -> Cell {
    Cell::DateTime(note_id / 1000)
}

/// C7.10: the stored tags split at U+0020 and U+3000, empty parts dropped,
/// joined by one U+0020.
pub fn tags(stored: &str) -> String {
    stored_tags(stored).collect::<Vec<_>>().join(" ")
}

fn stored_tags(stored: &str) -> impl Iterator<Item = &str> {
    stored.split([' ', '\u{3000}']).filter(|tag| !tag.is_empty())
}

/// C7.11: the card data `pos`, else a new card's stored `due`.
pub fn position(card: &Card, data: &Fields) -> Cell {
    match data.position {
        Some(pos) => Cell::Position(i64::from(pos)),
        None if card.card_type == CardType::New => Cell::Position(i64::from(card.due)),
        None => Cell::Empty,
    }
}

fn memory_state(data: &Fields) -> Option<(f32, f32)> {
    Some((data.stability?, data.difficulty?))
}

/// C7.12: `s` days as seconds, the product in `f32`.
pub fn stability(data: &Fields) -> Cell {
    match memory_state(data) {
        Some((s, _)) => Cell::TimeSpan(s * 86_400.0),
        None => Cell::Empty,
    }
}

/// C7.12: ((`d` − 1) ÷ 9) × 100, each step in `f32`.
pub fn difficulty(data: &Fields) -> Cell {
    match memory_state(data) {
        Some((_, d)) => Cell::Percent(Percent::Real((d - 1.0) / 9.0 * 100.0)),
        None => Cell::Empty,
    }
}

/// C11: the cell's own elapsed seconds, then `search/card-extractors` X5
/// steps 3 to 5.
pub fn retrievability(card: &Card, data: &Fields, time: RowTime) -> Cell {
    let Some((stability, difficulty)) = memory_state(data) else {
        return Cell::Empty;
    };
    let due = i64::from(home_due(card));
    let elapsed: u32 = if let Some(last) = data.last_review_secs {
        time.now.wrapping_sub(last) as u32
    } else if counts_in_days(card) {
        let target = if card.queue == CardQueue::Learning {
            due
        } else {
            time.now + (due - i64::from(time.today)) * 86_400
        };
        let last = target - i64::from(card.interval) * 86_400;
        let since = time.now - last;
        if since < 0 { 0 } else { since as u32 }
    } else {
        time.now.wrapping_sub(due) as u32
    };
    let r = fsrs::current_retrievability(
        fsrs::MemoryState { stability, difficulty },
        elapsed as f32 / 86_400.0,
        data.decay.unwrap_or(0.5),
    );
    Cell::Percent(Percent::Real(r * 100.0))
}

/// C7.3: the template's name; for a cloze note type with the cloze number.
pub fn card_type(template_name: &str, cloze: bool, ord: u16) -> String {
    if cloze {
        format!("{template_name} {}", u32::from(ord) + 1)
    } else {
        template_name.to_owned()
    }
}

/// C7.1: the H6 line (media names kept) of the note's field at `sort_field`,
/// or the empty text when the note has none there.
pub fn sort_field(note: &Note, sort_field: u32) -> String {
    note.field_values()
        .nth(sort_field as usize)
        .map(meguri_text::html::text_line_keep_media)
        .unwrap_or_default()
}

/// C4: a tag equal to `name` ignoring ASCII letter case.
fn has_tag(stored: &str, name: &str) -> bool {
    stored_tags(stored).any(|tag| tag.eq_ignore_ascii_case(name))
}

/// C4 in cards mode.
pub fn card_state(card: &Card, note_tags: &str) -> RowState {
    RowState {
        flag: card.user_flag(),
        marked: has_tag(note_tags, "marked"),
        leech: has_tag(note_tags, "leech"),
        suspended: card.queue == CardQueue::Suspended,
        buried: matches!(card.queue, CardQueue::BuriedByScheduler | CardQueue::BuriedByUser),
        filtered: is_filtered(card),
    }
}

/// C4 in notes mode.
pub fn note_state(note_tags: &str) -> RowState {
    RowState {
        marked: has_tag(note_tags, "marked"),
        leech: has_tag(note_tags, "leech"),
        ..RowState::default()
    }
}

/// C5 in cards mode: the whole stored `flags` value counts.
pub fn card_tint(card: &Card, state: &RowState) -> Tint {
    if (1..=7).contains(&card.flags) {
        Tint::Flag(card.flags)
    } else if state.marked {
        Tint::Marked
    } else if state.suspended {
        Tint::Suspended
    } else if state.buried {
        Tint::Buried
    } else {
        Tint::None
    }
}

/// C5 in notes mode.
pub fn note_tint(state: &RowState) -> Tint {
    if state.marked { Tint::Marked } else { Tint::None }
}

/// C8: the notes-mode values computed over all of a note's cards.
pub mod notes {
    use super::*;

    /// Due: the earliest C7.5 instant of the cards that are not filtered,
    /// have `type` and `queue` both not 0 and a non-negative `queue`. Never
    /// wrapped.
    pub fn due(cards: &[Card], time: RowTime) -> Cell {
        cards
            .iter()
            .filter(|card| {
                !is_filtered(card)
                    && card.card_type != CardType::New
                    && card.queue != CardQueue::New
                    && queue_value(card) >= 0
            })
            .filter_map(|card| due_instant(card, time))
            .min()
            .map_or(Cell::Empty, |secs| Cell::Instant { secs, wrapped: false })
    }

    /// Avg. Ease: ⌊⌊S ÷ n⌋ ÷ 10⌋, S the `factor` sum modulo 65,536 (C13).
    pub fn ease(cards: &[Card]) -> Cell {
        let counted: Vec<_> = cards.iter().filter(|card| card.card_type != CardType::New).collect();
        if counted.is_empty() {
            return Cell::NewMarker;
        }
        let sum = counted.iter().fold(0u16, |sum, card| sum.wrapping_add(card.ease_factor));
        Cell::Percent(Percent::Integer(i64::from(sum) / counted.len() as i64 / 10))
    }

    /// Avg. Interval over review and relearning cards: the summed days times
    /// 86,400 modulo 2^32, divided by their number (C13).
    pub fn interval(cards: &[Card]) -> Cell {
        let counted: Vec<_> = cards
            .iter()
            .filter(|card| matches!(card.card_type, CardType::Review | CardType::Relearning))
            .collect();
        if counted.is_empty() {
            return Cell::Empty;
        }
        let days = counted.iter().map(|card| u64::from(card.interval)).sum::<u64>();
        let seconds = u64::from(day_seconds_u32(days)) / counted.len() as u64;
        Cell::TimeSpan(seconds as f32)
    }

    /// Reviews: the sum modulo 2^32 (C13).
    pub fn reviews(cards: &[Card]) -> Cell {
        Cell::Count(u64::from(cards.iter().fold(0u32, |sum, card| sum.wrapping_add(card.reps))))
    }

    /// Lapses: the sum modulo 2^32 (C13).
    pub fn lapses(cards: &[Card]) -> Cell {
        Cell::Count(u64::from(cards.iter().fold(0u32, |sum, card| sum.wrapping_add(card.lapses))))
    }

    /// Card Modified: the largest `mod`.
    pub fn card_modified(cards: &[Card]) -> Cell {
        cards
            .iter()
            .map(|card| card.mtime.0)
            .max()
            .map_or(Cell::Empty, Cell::DateTime)
    }

    /// The number of distinct `did` values, whether or not the deck exists.
    pub fn distinct_decks(cards: &[Card]) -> u64 {
        let mut decks: Vec<_> = cards.iter().map(|card| card.deck_id.0).collect();
        decks.sort_unstable();
        decks.dedup();
        decks.len() as u64
    }
}

/// A time span's unit (C12 step 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Months,
    Years,
}

impl SpanUnit {
    pub fn name(self) -> &'static str {
        match self {
            SpanUnit::Seconds => "seconds",
            SpanUnit::Minutes => "minutes",
            SpanUnit::Hours => "hours",
            SpanUnit::Days => "days",
            SpanUnit::Months => "months",
            SpanUnit::Years => "years",
        }
    }
}

/// C12 steps 1 and 2: the unit and the amount of a span of `seconds`.
pub fn span_parts(seconds: f32) -> (SpanUnit, f32) {
    let a = seconds.abs();
    let (unit, length) = if a < 60.0 {
        (SpanUnit::Seconds, 1.0)
    } else if a < 3_600.0 {
        (SpanUnit::Minutes, 60.0)
    } else if a < 86_400.0 {
        (SpanUnit::Hours, 3_600.0)
    } else if a < 2_628_000.0 {
        (SpanUnit::Days, 86_400.0)
    } else if a < 31_536_000.0 {
        (SpanUnit::Months, 2_628_000.0)
    } else {
        (SpanUnit::Years, 31_536_000.0)
    };
    let q: f32 = seconds / length;
    let amount = match unit {
        SpanUnit::Seconds | SpanUnit::Days => q.round(),
        _ => (q * 10.0).round() / 10.0,
    };
    (unit, amount)
}

/// C12 step 3: two decimals (the exact value, an exact half to the even
/// digit), trailing zeros and a trailing `.` removed; the sign kept.
pub fn span_amount_text(amount: f32) -> String {
    if amount.is_nan() {
        return "NaN".to_owned();
    }
    if amount.is_infinite() {
        return if amount > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    // Rust's fixed-precision formatting rounds the exact binary value, a
    // tie to even.
    let text = format!("{:.2}", f64::from(amount));
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}
