//! Pure application of U45's selected state (sched/answering A3–A6).
pub mod deck_stats;
pub mod revlog;
pub mod siblings;

use crate::{
    model::preset::{LeechAction, PresetView},
    states::{
        engine::{AnswerState, State},
        fuzz,
        interval_limits::Delay,
    },
};
use meguri_model::{
    card::{Card, CardQueue, CardType},
    time::EpochSecs,
    usn::Usn,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

#[derive(Clone, Debug)]
pub struct AnswerPlan {
    pub card: Card,
    pub elapsed_ms: u32,
    pub leech: bool,
    pub suspended: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidAnswer;
impl std::fmt::Display for InvalidAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid selected answer state")
    }
}
impl std::error::Error for InvalidAnswer {}

#[allow(clippy::too_many_arguments)] // The pure application contract's resolved inputs.
pub fn plan(
    card: &Card,
    current: State,
    chosen: &AnswerState,
    preset: &PresetView,
    retention: f32,
    today: u32,
    answered_at: i64,
    elapsed_ms: u64,
) -> Result<AnswerPlan, InvalidAnswer> {
    let current_type = match current {
        State::New { .. } => CardType::New,
        State::Learning(_) => CardType::Learning,
        State::Review(_) => CardType::Review,
        State::Relearning { .. } => CardType::Relearning,
    };
    if current_type != card.card_type
        || matches!(chosen.state, State::New { .. })
        || chosen.state.memory_state().is_none()
        || chosen.revlog.kind > 2
    {
        return Err(InvalidAnswer);
    }
    let mut out = card.clone();
    out.reps = out.reps.wrapping_add(1);
    out.mtime = EpochSecs(answered_at.div_euclid(1000));
    out.usn = Usn::PENDING;
    if let State::New { position } = current {
        out.data.new_position = Some(position);
    }
    // Relearning stores the learning part's memory, as the application contract specifies.
    out.data.memory_state = match chosen.state {
        State::Relearning { learning, .. } => learning.memory_state,
        _ => chosen.state.memory_state(),
    };
    out.data.desired_retention = Some(retention);
    out.data.decay = Some(preset.decay());
    out.data.last_review_secs = Some(out.mtime.0);
    out.remaining_steps = chosen.left;
    let review = match chosen.state {
        State::Review(r) | State::Relearning { review: r, .. } => Some(r),
        _ => None,
    };
    if let Some(r) = review {
        out.interval = r.scheduled_days;
        out.ease_factor = chosen.factor;
        out.lapses = r.lapses;
    }
    out.card_type = match chosen.state {
        State::Learning(_) => CardType::Learning,
        State::Review(_) => CardType::Review,
        State::Relearning { .. } => CardType::Relearning,
        State::New { .. } => return Err(InvalidAnswer),
    };
    match chosen.delay {
        Delay::Days(days) => {
            out.due = today.wrapping_add(days) as i32;
            out.queue = if out.card_type == CardType::Review {
                CardQueue::Review
            } else {
                CardQueue::InterdayLearning
            };
        }
        Delay::Seconds(seconds) => {
            if out.card_type == CardType::Review {
                return Err(InvalidAnswer);
            }
            let delay = fuzz::learning_delay(seconds, fuzz::seed(card.id.0 as u64, u64::from(card.reps)));
            out.due = out.mtime.0.wrapping_add(i64::from(delay)) as i32;
            out.queue = CardQueue::Learning;
        }
    }
    let leech = review.is_some_and(|r| r.leeched);
    let suspended = leech && preset.leech_action() == LeechAction::Suspend;
    if suspended {
        out.queue = CardQueue::Suspended;
    }
    let elapsed_ms = elapsed_ms.min(u64::from(preset.fields().cap_answer_time_to_secs) * 1000) as u32;
    Ok(AnswerPlan {
        card: out,
        elapsed_ms,
        leech,
        suspended,
    })
}
