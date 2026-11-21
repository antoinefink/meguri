//! The stats rows (`stats/card-info` C4) and the history texts (C6.2, C6.4).

use meguri_model::{clock::Clock, revlog::RevlogKind};

use super::{HistoryEntry, RawValues};
use crate::text::{SpanForm, custom_data::custom_data_text, date, date_time, time_span, whole_percent};

/// A row's key: the UI's `cardInfo.row.<key>` String Catalog identifier
/// (C4.1, C12). The snapshot carries no label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatsKey {
    Added,
    FirstReview,
    LatestReview,
    Due,
    Position,
    Interval,
    Stability,
    Difficulty,
    Retrievability,
    Reviews,
    Lapses,
    AverageTime,
    TotalTime,
    CardType,
    NoteType,
    Deck,
    Preset,
    CardId,
    NoteId,
    CustomData,
}

impl StatsKey {
    pub fn as_str(self) -> &'static str {
        match self {
            StatsKey::Added => "added",
            StatsKey::FirstReview => "first_review",
            StatsKey::LatestReview => "latest_review",
            StatsKey::Due => "due",
            StatsKey::Position => "position",
            StatsKey::Interval => "interval",
            StatsKey::Stability => "stability",
            StatsKey::Difficulty => "difficulty",
            StatsKey::Retrievability => "retrievability",
            StatsKey::Reviews => "reviews",
            StatsKey::Lapses => "lapses",
            StatsKey::AverageTime => "average_time",
            StatsKey::TotalTime => "total_time",
            StatsKey::CardType => "card_type",
            StatsKey::NoteType => "note_type",
            StatsKey::Deck => "deck",
            StatsKey::Preset => "preset",
            StatsKey::CardId => "card_id",
            StatsKey::NoteId => "note_id",
            StatsKey::CustomData => "custom_data",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatRow {
    pub key: StatsKey,
    pub value: String,
}

/// The history columns' texts (C6.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntryTexts {
    pub date: String,
    /// The date-and-time form, for wide layouts (R1253, R1426).
    pub date_time: String,
    pub kind: String,
    pub rating: String,
    pub interval: String,
    pub ease: String,
    pub time: String,
    pub stability: String,
}

/// C4.2: the Stability row's text for a stability of `stability` days.
pub fn stability_text(stability: f64) -> String {
    let seconds = stability * 86_400.0;
    let rounded = time_span(seconds, SpanForm::Long, true, false);
    if stability > 31.0 {
        format!("{rounded} ({})", time_span(seconds, SpanForm::Long, true, true))
    } else {
        rounded
    }
}

/// C4.1 row 8: the whole percent of `(d − 1) ÷ 9 × 100`, in binary64.
pub fn difficulty_text(difficulty: f32) -> String {
    whole_percent((f64::from(difficulty) - 1.0) / 9.0 * 100.0)
}

/// C6.4: the Ease column for a stored `factor`.
pub fn ease_text(factor: u32) -> String {
    let percent = f64::from(factor) / 10.0;
    if factor == 0 {
        String::new()
    } else if percent <= 110.0 {
        format!("D:{}", whole_percent(percent - 10.0))
    } else {
        whole_percent(percent)
    }
}

/// C6.2's Type column.
pub fn kind_text(kind: RevlogKind) -> &'static str {
    match kind {
        RevlogKind::Learning => "Learn",
        RevlogKind::Review => "Review",
        RevlogKind::Relearning => "Relearn",
        RevlogKind::Filtered => "Filtered",
        RevlogKind::Manual => "Manual",
        RevlogKind::Rescheduled => "Rescheduled",
    }
}

pub(super) fn entry_texts(entry: &HistoryEntry, clock: &dyn Clock) -> EntryTexts {
    EntryTexts {
        date: date(entry.instant, clock),
        date_time: date_time(entry.instant, clock),
        kind: kind_text(entry.kind).into(),
        rating: entry.rating.to_string(),
        interval: time_span(entry.interval_secs as f64, SpanForm::Long, false, false),
        ease: ease_text(entry.factor),
        time: time_span(f64::from(entry.taken_secs), SpanForm::Short, false, false),
        stability: match entry.memory_state {
            Some(state) if state.stability != 0.0 => {
                time_span(f64::from(state.stability) * 86_400.0, SpanForm::Long, true, false)
            }
            _ => String::new(),
        },
    }
}

/// C4.1: the applicable rows in order.
pub(super) fn stat_rows(raw: &RawValues, clock: &dyn Clock) -> Vec<StatRow> {
    let mut rows = Vec::with_capacity(20);
    let mut row = |key: StatsKey, value: String| rows.push(StatRow { key, value });
    row(StatsKey::Added, date(raw.added, clock));
    if let Some(first) = raw.first_review {
        row(StatsKey::FirstReview, date(first, clock));
    }
    if let Some(latest) = raw.latest_review {
        row(StatsKey::LatestReview, date(latest, clock));
    }
    if let Some(due) = raw.due_date {
        row(StatsKey::Due, date(due, clock));
    }
    if let Some(position) = raw.due_position {
        row(StatsKey::Position, position.to_string());
    }
    if raw.interval != 0 {
        row(
            StatsKey::Interval,
            time_span(f64::from(raw.interval) * 86_400.0, SpanForm::Long, false, false),
        );
    }
    if let Some(state) = raw.memory_state {
        row(StatsKey::Stability, stability_text(f64::from(state.stability)));
        row(StatsKey::Difficulty, difficulty_text(state.difficulty));
        if let Some(r) = raw.fsrs_retrievability.filter(|r| *r != 0.0 && !r.is_nan()) {
            row(StatsKey::Retrievability, whole_percent(f64::from(r) * 100.0));
        }
    }
    row(StatsKey::Reviews, raw.reviews.to_string());
    row(StatsKey::Lapses, raw.lapses.to_string());
    if raw.total_secs != 0.0 {
        row(
            StatsKey::AverageTime,
            time_span(f64::from(raw.average_secs), SpanForm::Long, false, false),
        );
        row(
            StatsKey::TotalTime,
            time_span(f64::from(raw.total_secs), SpanForm::Long, false, false),
        );
    }
    row(StatsKey::CardType, raw.card_type.clone());
    row(StatsKey::NoteType, raw.notetype.clone());
    row(
        StatsKey::Deck,
        match &raw.original_deck {
            Some(home) => format!("{} ({home})", raw.deck),
            None => raw.deck.clone(),
        },
    );
    row(StatsKey::Preset, raw.preset.clone());
    row(StatsKey::CardId, raw.card_id.0.to_string());
    row(StatsKey::NoteId, raw.note_id.0.to_string());
    if !raw.custom_data.is_empty() {
        row(StatsKey::CustomData, custom_data_text(&raw.custom_data));
    }
    rows
}
