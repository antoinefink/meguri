//! The current learning part (L2/L8); answer transitions supply their own
//! new memory and elapsed time (L10).
use super::{fuzz, steps::Steps};
use meguri_model::{
    card::{Card, CardQueue},
    card_data::MemoryState,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LearningState {
    pub remaining_steps: u32,
    pub scheduled_secs: u32,
    pub elapsed_secs: u32,
    pub memory_state: Option<MemoryState>,
}

pub fn current(card: &Card, minutes: &[f32], today: u32, now: i64, memory_state: Option<MemoryState>) -> LearningState {
    let steps = Steps::new(minutes, card.remaining_steps);
    let scheduled_secs = steps.current();
    let elapsed_secs = match card.queue {
        CardQueue::Learning => {
            let previous_seed = fuzz::seed(card.id.0 as u64, u64::from(card.reps.wrapping_sub(1)));
            let previous_delay = fuzz::learning_delay(scheduled_secs, previous_seed);
            now.wrapping_sub(i64::from(card.due) - i64::from(previous_delay)) as u32
        }
        CardQueue::InterdayLearning => (today as i32)
            .wrapping_sub(card.due)
            .wrapping_add((scheduled_secs / 86400).max(1) as i32)
            .wrapping_mul(86400) as u32,
        _ => 0,
    };
    LearningState {
        remaining_steps: steps.remaining(),
        scheduled_secs,
        elapsed_secs,
        memory_state,
    }
}
