//! FSRS answer states of a review card with the load balancer off
//! (specs/sched/fsrs-transitions.md T1, T4, T5): the days and memory state
//! each rating gives. U45 adds the rest of each state (ease factor, lapses,
//! leech, the relearning step) around these values.

use fsrs::{FSRS, FSRSError, MemoryState};

use super::fuzz;

/// A review card as the FSRS answer states read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewCard {
    /// The card data's memory state (T2.4).
    pub stability: f32,
    pub difficulty: f32,
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
    pub stability: f32,
    pub difficulty: f32,
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
    let memory = MemoryState {
        stability: card.stability,
        difficulty: card.difficulty,
    };
    let next = model.next_states(Some(memory), card.desired_retention, card.days_elapsed)?;
    let rated = |state: &fsrs::ItemState, days: u32| Answer {
        days,
        stability: state.memory.stability,
        difficulty: state.memory.difficulty,
        unrounded: state.interval,
    };

    // T4: no fuzz, minimum or maximum interval here.
    let again = rated(&next.again, next.again.interval.round().max(1.0) as u32);

    // T5: each rating's lower bound, clamped to [1, M], then the fuzz pick.
    let maximum = card.maximum_interval.max(1);
    let factor = fuzz::factor(card.fuzz_seed);
    let pick = |state: &fsrs::ItemState, at_least: u32| {
        let minimum = fuzz::no_regression_minimum(state.interval, card.interval, card.maximum_interval);
        let bound = minimum.max(at_least).clamp(1, maximum);
        fuzz::fuzzed_interval(state.interval, bound, maximum, factor)
    };
    let hard_days = pick(&next.hard, 1);
    let good_days = pick(&next.good, hard_days.saturating_add(1));
    let easy_days = pick(&next.easy, good_days.saturating_add(1));
    Ok(ReviewAnswers {
        again,
        hard: rated(&next.hard, hard_days),
        good: rated(&next.good, good_days),
        easy: rated(&next.easy, easy_days),
    })
}
