//! Review-only projection for the floating-point probe (F5). The full
//! engine uses the same interval rules; both carry the model memory value.

use fsrs::{FSRS, FSRSError};
use meguri_model::card_data::MemoryState;

use super::{
    fuzz,
    interval_limits::{Picker, review_days},
};

/// A review card as the FSRS answer states read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewCard {
    /// The card data's memory state (T2.4).
    pub memory_state: MemoryState,
    /// T2.3: whole days from the last review to the next day boundary.
    pub days_elapsed: u32,
    /// The card's current interval, `cards.ivl` (T3).
    pub interval: u32,
    /// T2.2.
    pub desired_retention: f32,
    /// The home preset's maximum interval as stored, `M_raw` (T2.5).
    pub maximum_interval: u32,
    /// The fuzz seed: [`fuzz::seed`] of the card id and its review count
    /// before the answer (sched/fuzz Z1).
    pub fuzz_seed: u64,
}

/// What one rating gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Answer {
    /// Again: the review part's `scheduled_days` (T4); otherwise the new
    /// interval (T5).
    pub days: u32,
    pub memory_state: MemoryState,
    /// The rating's unrounded FSRS interval, `u_r` (T1).
    pub unrounded: f32,
}

/// The four ratings' results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewAnswers {
    pub again: Answer,
    pub hard: Answer,
    pub good: Answer,
    pub easy: Answer,
}

/// T1, T4 and T5 for a review card, with `model` built by `FSRS::new` from
/// the home preset's parameter list (T2.1). Errors are the crate's.
pub fn answers(model: &FSRS, card: &ReviewCard) -> Result<ReviewAnswers, FSRSError> {
    let memory = fsrs::MemoryState {
        stability: card.memory_state.stability,
        difficulty: card.memory_state.difficulty,
    };
    let next = model.next_states(Some(memory), card.desired_retention, card.days_elapsed)?;
    let rated = |state: &fsrs::ItemState, days: u32| Answer {
        days,
        memory_state: MemoryState {
            stability: state.memory.stability,
            difficulty: state.memory.difficulty,
        },
        unrounded: state.interval,
    };

    // T4: no fuzz, minimum or maximum interval here.
    let again = rated(&next.again, next.again.interval.round().max(1.0) as u32);

    let mut picker = Picker {
        factor: fuzz::factor(card.fuzz_seed),
        chooser: None,
    };
    let [hard_days, good_days, easy_days] = review_days(
        [next.hard.interval, next.good.interval, next.easy.interval],
        card.interval,
        card.maximum_interval,
        &mut picker,
    );
    Ok(ReviewAnswers {
        again,
        hard: rated(&next.hard, hard_days),
        good: rated(&next.good, good_days),
        easy: rated(&next.easy, easy_days),
    })
}
