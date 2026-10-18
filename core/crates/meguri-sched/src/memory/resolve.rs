//! Read-only history resolution and the separate database-repair rule.

use meguri_model::{
    card::{Card, CardType},
    card_data::MemoryState,
    ids::DeckConfigId,
    revlog::RevlogEntry,
};

use super::{
    ignore_before,
    items::{History, HistoryForm, is_cram, is_graded, is_reset},
};
use crate::{model::preset::PresetView, states::fsrs_glue::FsrsCache};

#[derive(Debug)]
pub enum MemoryError {
    InvalidDate { preset: DeckConfigId },
    UnsupportedParams,
    Fsrs(fsrs::FSRSError),
}
impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDate { preset } => write!(f, "invalid ignore-before date for preset {}", preset.0),
            Self::UnsupportedParams => f.write_str("unsupported FSRS parameters"),
            Self::Fsrs(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for MemoryError {}
impl From<fsrs::FSRSError> for MemoryError {
    fn from(error: fsrs::FSRSError) -> Self {
        Self::Fsrs(error)
    }
}

pub fn preset_cutoff(preset: &PresetView) -> Result<i64, MemoryError> {
    ignore_before::cutoff_millis(&preset.fields().ignore_revlogs_before_date).map_err(|_| MemoryError::InvalidDate {
        preset: preset.source().id,
    })
}

/// M2: no writes and no card-value fallback in the shared history builder.
pub fn on_the_fly(
    cache: &mut FsrsCache,
    preset: &PresetView,
    card: &Card,
    rows: &[RevlogEntry],
    next_day_at: i64,
) -> Result<Option<MemoryState>, MemoryError> {
    let model = cache.model(preset).map_err(|_| MemoryError::UnsupportedParams)?;
    if card.card_type == CardType::New {
        return Ok(None);
    }
    if card.data.memory_state.is_some() {
        return Ok(card.data.memory_state);
    }
    let cutoff = preset_cutoff(preset)?;
    let retention = preset.fields().historical_retention;
    if let Some(history) = History::select(rows, cutoff, HistoryForm::Memory) {
        return Ok(Some(history.memory(model, next_day_at, retention)?));
    }
    if card.interval == 0 {
        return Ok(None);
    }
    let state = model.memory_state_from_sm2(f32::from(card.ease_factor) / 1000.0, card.interval as f32, retention)?;
    Ok(Some(MemoryState {
        stability: state.stability,
        difficulty: state.difficulty,
    }))
}

/// M3's pure equivalent of the indexed lookup, including rows before Reset.
pub fn last_review_time(rows: &[RevlogEntry]) -> Option<i64> {
    rows.iter()
        .rev()
        .find(|row| (1..=4).contains(&row.button) && !is_cram(row))
        .map(|row| row.id.0 / 1000)
}

/// M6 only changes lrt. Its storage caller preserves stamps and counts the
/// repair; M3's read-only fallback deliberately uses different row classes.
pub fn repair_last_review_time(card: &mut Card, rows: &[RevlogEntry]) -> bool {
    if card.card_type == CardType::New || rows.is_empty() {
        return false;
    }
    let expected = rows
        .iter()
        .rev()
        .find_map(|row| {
            if is_graded(row) {
                Some(Some(row.id.0 / 1000))
            } else if is_reset(row) {
                Some(None)
            } else {
                None
            }
        })
        .flatten();
    if card.data.last_review_secs == expected {
        return false;
    }
    card.data.last_review_secs = expected;
    true
}
