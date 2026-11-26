//! F3/F4 selection over the ordinary scheduling engine.
use super::{engine::{self, AnswerState, RevlogPreview, SchedulingStates, State, StateError, StateInput},
    fsrs_glue::FsrsCache, interval_limits::{Delay, FuzzedDayChooser}, labels::format_seconds};
use meguri_model::{card::Card, proto::deck::{DeckKind, FilteredDeck}};
use meguri_storage::{ReadRows, StorageError};

pub fn options(rows: &impl ReadRows, card: &Card) -> Result<Option<FilteredDeck>, StorageError> {
    if card.original_deck_id.0 == 0 { return Ok(None); }
    Ok(rows.deck(card.deck_id)?.and_then(|deck| match deck.kind.kind {
        DeckKind::Filtered(ref filtered) => Some(filtered.clone()),
        _ => None,
    }))
}

pub fn next_states(
    rows: &impl ReadRows,
    cache: &mut FsrsCache,
    input: &StateInput<'_>,
    chooser: Option<&mut dyn FuzzedDayChooser>,
) -> Result<SchedulingStates, FilteredStateError> {
    if let Some(deck) = options(rows, input.card)? && !deck.reschedule {
        return Ok(preview_states(&deck, input.learn_ahead));
    }
    Ok(engine::next_states(cache, input, chooser)?)
}

#[derive(Debug)]
pub enum FilteredStateError { Storage(StorageError), State(StateError) }
impl std::fmt::Display for FilteredStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Storage(e) => e.fmt(f), Self::State(e) => e.fmt(f) }
    }
}
impl std::error::Error for FilteredStateError {}
impl From<StorageError> for FilteredStateError { fn from(e: StorageError) -> Self { Self::Storage(e) } }
impl From<StateError> for FilteredStateError { fn from(e: StateError) -> Self { Self::State(e) } }

pub fn preview_states(deck: &FilteredDeck, ahead: u32) -> SchedulingStates {
    let answer = |seconds| {
        let delay = Delay::Seconds(seconds);
        AnswerState {
            state: State::Preview { scheduled_secs: seconds, finished: seconds == 0 },
            delay, left: 0, factor: 0,
            revlog: RevlogPreview { interval: delay.revlog_interval(), last_interval: 0, kind: 3, factor: 0 },
            label: format_seconds(seconds, ahead),
        }
    };
    SchedulingStates {
        current: State::Preview { scheduled_secs: 0, finished: false },
        again: answer(deck.preview_again_secs), hard: answer(deck.preview_hard_secs),
        good: answer(deck.preview_good_secs), easy: answer(0),
    }
}
