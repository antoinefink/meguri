//! Grade Now selects the existing U45 prediction; U44 owns all row application.
use crate::{
    answer::Rating,
    states::engine::{AnswerState, SchedulingStates},
};
pub fn select(states: &SchedulingStates, rating: Rating) -> AnswerState {
    match rating {
        Rating::Again => states.again.clone(),
        Rating::Hard => states.hard.clone(),
        Rating::Good => states.good.clone(),
        Rating::Easy => states.easy.clone(),
    }
}
