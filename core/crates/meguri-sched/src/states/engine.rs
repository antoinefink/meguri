//! Pure snapshot-to-answer computation (sched/fsrs-transitions T1–T14).
//! The caller resolves history, timing and home preset before entering here.
use super::{
    fsrs_glue::{FsrsCache, UnsupportedParams},
    fuzz,
    interval_limits::{Delay, FuzzedDayChooser, Picker, review_days, step_delay},
    labels::{AnswerLabel, format_seconds},
    learning::{self, LearningState},
    leech,
    steps::Steps,
};
use crate::model::preset::PresetView;
use meguri_model::{
    card::{Card, CardType},
    card_data::MemoryState,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReviewState {
    pub scheduled_days: u32,
    pub elapsed_days: u32,
    pub ease_factor: f32,
    pub lapses: u32,
    pub leeched: bool,
    pub memory_state: Option<MemoryState>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    New {
        position: u32,
    },
    Learning(LearningState),
    Review(ReviewState),
    Relearning {
        learning: LearningState,
        review: ReviewState,
    },
}
impl State {
    pub fn delay(self, until_next_day: u64) -> Delay {
        match self {
            Self::New { .. } => Delay::Seconds(0),
            Self::Learning(l) | Self::Relearning { learning: l, .. } => step_delay(l.scheduled_secs, until_next_day),
            Self::Review(r) => Delay::Days(r.scheduled_days),
        }
    }
    pub fn memory_state(self) -> Option<MemoryState> {
        match self {
            Self::New { .. } => None,
            Self::Learning(l) => l.memory_state,
            Self::Review(r) | Self::Relearning { review: r, .. } => r.memory_state,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevlogPreview {
    pub interval: i32,
    pub last_interval: i32,
    pub kind: u8,
    pub factor: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct AnswerState {
    pub state: State,
    pub delay: Delay,
    pub left: u32,
    pub factor: u16,
    pub revlog: RevlogPreview,
    pub label: AnswerLabel,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SchedulingStates {
    pub current: State,
    pub again: AnswerState,
    pub hard: AnswerState,
    pub good: AnswerState,
    pub easy: AnswerState,
}
pub struct StateInput<'a> {
    pub card: &'a Card,
    pub preset: &'a PresetView,
    pub memory_state: Option<MemoryState>,
    pub last_review_time: Option<i64>,
    pub desired_retention: f32,
    pub today: u32,
    pub next_day_at: i64,
    pub now: i64,
    pub short_term_with_steps: bool,
    pub learn_ahead: u32,
}
#[derive(Debug)]
pub enum StateError {
    UnsupportedParams,
    InFilteredDeck,
    Fsrs(fsrs::FSRSError),
}
impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedParams => UnsupportedParams.fmt(f),
            Self::InFilteredDeck => f.write_str("card is in a filtered deck"),
            Self::Fsrs(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for StateError {}

/// T2.3: keep the signed subtraction and the final integer conversion wrapping.
pub fn elapsed_days(next_day_at: i64, last_review_time: Option<i64>) -> u32 {
    last_review_time
        .map(|time| (next_day_at.wrapping_sub(time).max(0) / 86400) as u32)
        .unwrap_or(0)
}
fn current(input: &StateInput<'_>) -> State {
    let card = input.card;
    let memory = if card.card_type == CardType::New {
        None
    } else {
        input.memory_state
    };
    let review = ReviewState {
        scheduled_days: card.interval,
        elapsed_days: card.interval,
        ease_factor: f32::from(card.ease_factor) / 1000.0,
        lapses: card.lapses,
        leeched: false,
        memory_state: memory,
    };
    match card.card_type {
        CardType::New => State::New {
            position: card.due.max(0) as u32,
        },
        CardType::Learning => State::Learning(learning::current(
            card,
            &input.preset.fields().learn_steps,
            input.today,
            input.now,
            memory,
        )),
        CardType::Review => {
            let today = input.today as i32;
            State::Review(ReviewState {
                elapsed_days: (card.interval as i32)
                    .wrapping_sub(card.due.min(today).wrapping_sub(today))
                    .max(0) as u32,
                ..review
            })
        }
        CardType::Relearning => State::Relearning {
            learning: learning::current(
                card,
                &input.preset.fields().relearn_steps,
                input.today,
                input.now,
                memory,
            ),
            review,
        },
    }
}
fn memory(item: &fsrs::ItemState) -> Option<MemoryState> {
    Some(MemoryState {
        stability: item.memory.stability,
        difficulty: item.memory.difficulty,
    })
}
fn rounded(interval: f32) -> u32 {
    interval.round().max(1.0) as u32
}
fn short_seconds(interval: f32) -> u32 {
    (interval * 86400.0) as u32
}

struct Transitions<'a, 'b> {
    input: &'a StateInput<'a>,
    picker: Picker<'b>,
    good_interval: f32,
}
impl Transitions<'_, '_> {
    fn graduation(&mut self, item: &fsrs::ItemState, easy: bool) -> u32 {
        let maximum = self.input.preset.fields().maximum_review_interval;
        let minimum = if easy {
            self.picker.pick(self.good_interval, 1, maximum).saturating_add(1)
        } else {
            1
        };
        self.picker.pick(item.interval.round().max(1.0), minimum, maximum)
    }
    fn short_term(&self, item: &fsrs::ItemState, steps: &Steps<'_>) -> bool {
        item.interval < 0.5
            && self.input.preset.allows_short_term()
            && (steps.count() == 0 || self.input.short_term_with_steps)
    }
    fn learning(&mut self, current: LearningState, new: bool, rating: usize, item: &fsrs::ItemState) -> State {
        let minutes = &self.input.preset.fields().learn_steps;
        let steps = if new {
            Steps::unseen(minutes)
        } else {
            Steps::new(minutes, current.remaining_steps)
        };
        let selected = match rating {
            0 => steps.again(),
            1 => steps.hard(),
            2 => steps.good(),
            _ => None,
        };
        if let Some(step) = selected {
            return State::Learning(LearningState {
                remaining_steps: step.remaining,
                scheduled_secs: step.seconds,
                elapsed_secs: 0,
                memory_state: memory(item),
            });
        }
        if rating != 3 && self.short_term(item, &steps) {
            return State::Learning(LearningState {
                remaining_steps: if rating == 0 { steps.count() } else { steps.remaining() },
                scheduled_secs: short_seconds(item.interval),
                elapsed_secs: 0,
                memory_state: memory(item),
            });
        }
        State::Review(ReviewState {
            scheduled_days: self.graduation(item, rating == 3),
            elapsed_days: 0,
            ease_factor: self.input.preset.fields().initial_ease,
            lapses: 0,
            leeched: false,
            memory_state: memory(item),
        })
    }
    fn lapse(&mut self, current: ReviewState, item: &fsrs::ItemState) -> State {
        let review = ReviewState {
            scheduled_days: rounded(item.interval),
            elapsed_days: 0,
            ease_factor: (current.ease_factor - 0.2).max(1.3),
            lapses: current.lapses.wrapping_add(1),
            leeched: leech::is_leech(
                current.lapses.wrapping_add(1),
                self.input.preset.fields().leech_threshold,
            ),
            memory_state: memory(item),
        };
        let steps = Steps::unseen(&self.input.preset.fields().relearn_steps);
        let seconds = steps
            .again()
            .map(|s| s.seconds)
            .or_else(|| self.short_term(item, &steps).then(|| short_seconds(item.interval)));
        match seconds {
            Some(scheduled_secs) => State::Relearning {
                learning: LearningState {
                    remaining_steps: steps.count(),
                    scheduled_secs,
                    elapsed_secs: 0,
                    memory_state: memory(item),
                },
                review,
            },
            None => State::Review(review),
        }
    }
    fn relearning(
        &mut self,
        current: LearningState,
        review: ReviewState,
        rating: usize,
        item: &fsrs::ItemState,
    ) -> State {
        let steps = Steps::new(&self.input.preset.fields().relearn_steps, current.remaining_steps);
        let selected = match rating {
            0 => steps.again(),
            1 => steps.hard(),
            2 => steps.good(),
            _ => None,
        };
        let mut review = ReviewState {
            memory_state: memory(item),
            ..review
        };
        if let Some(step) = selected {
            if rating == 0 {
                review.scheduled_days = rounded(item.interval);
            }
            review.elapsed_days = 0;
            return State::Relearning {
                learning: LearningState {
                    remaining_steps: step.remaining,
                    scheduled_secs: step.seconds,
                    elapsed_secs: if rating == 1 { current.elapsed_secs } else { 0 },
                    memory_state: memory(item),
                },
                review,
            };
        }
        review.scheduled_days = self.graduation(item, rating == 3);
        if rating == 3 {
            review.elapsed_days = 0;
        }
        if rating != 3 && self.short_term(item, &steps) {
            return State::Relearning {
                learning: LearningState {
                    remaining_steps: match rating {
                        0 => steps.count(),
                        1 => current.remaining_steps,
                        _ => steps.good().map(|s| s.remaining).unwrap_or(0),
                    },
                    scheduled_secs: short_seconds(item.interval),
                    elapsed_secs: if rating == 0 { 0 } else { current.elapsed_secs },
                    memory_state: memory(item),
                },
                review,
            };
        }
        State::Review(review)
    }
}

/// Each call is read-only except for the model cache and the optional caller's chooser.
pub fn next_states(
    cache: &mut FsrsCache,
    input: &StateInput<'_>,
    chooser: Option<&mut dyn FuzzedDayChooser>,
) -> Result<SchedulingStates, StateError> {
    if input.card.original_deck_id.0 != 0 {
        return Err(StateError::InFilteredDeck);
    }
    let current = current(input);
    let model = cache.model(input.preset).map_err(|_| StateError::UnsupportedParams)?;
    let fsrs_memory = current.memory_state().map(|m| fsrs::MemoryState {
        stability: m.stability,
        difficulty: m.difficulty,
    });
    let next = model
        .next_states(
            fsrs_memory,
            input.desired_retention,
            elapsed_days(input.next_day_at, input.last_review_time),
        )
        .map_err(StateError::Fsrs)?;
    let mut transitions = Transitions {
        input,
        picker: Picker {
            factor: fuzz::factor(fuzz::seed(input.card.id.0 as u64, u64::from(input.card.reps))),
            chooser,
        },
        good_interval: next.good.interval,
    };
    let items = [&next.again, &next.hard, &next.good, &next.easy];
    let raw = match current {
        State::New { .. } | State::Learning(_) => {
            let (learning, new) = match current {
                State::Learning(l) => (l, false),
                _ => (
                    LearningState {
                        remaining_steps: input.preset.fields().learn_steps.len() as u32,
                        scheduled_secs: 0,
                        elapsed_secs: 0,
                        memory_state: None,
                    },
                    true,
                ),
            };
            std::array::from_fn(|rating| transitions.learning(learning, new, rating, items[rating]))
        }
        State::Review(review) => {
            let again = transitions.lapse(review, items[0]);
            let days = review_days(
                [next.hard.interval, next.good.interval, next.easy.interval],
                review.scheduled_days,
                input.preset.fields().maximum_review_interval,
                &mut transitions.picker,
            );
            let mut answers = [again; 4];
            for rating in 1..4 {
                let ease = match rating {
                    1 => (review.ease_factor - 0.15).max(1.3),
                    2 => review.ease_factor,
                    _ => review.ease_factor + 0.15,
                };
                answers[rating] = State::Review(ReviewState {
                    scheduled_days: days[rating - 1],
                    elapsed_days: 0,
                    ease_factor: ease,
                    memory_state: memory(items[rating]),
                    ..review
                });
            }
            answers
        }
        State::Relearning { learning, review } => {
            std::array::from_fn(|rating| transitions.relearning(learning, review, rating, items[rating]))
        }
    };
    let until_next_day = input.next_day_at.saturating_sub(input.now).max(0) as u64;
    let decorate = |state: State| {
        let delay = state.delay(until_next_day);
        let left = match state {
            State::Learning(l) | State::Relearning { learning: l, .. } => l.remaining_steps,
            _ => 0,
        };
        let factor = match state {
            State::Review(r) | State::Relearning { review: r, .. } => (r.ease_factor * 1000.0).round() as u16,
            _ => input.card.ease_factor,
        };
        let kind = match current {
            State::Review(_) => 1,
            State::Relearning { .. } => 2,
            _ => 0,
        };
        let difficulty = state
            .memory_state()
            .expect("FSRS answers always carry memory")
            .difficulty;
        AnswerState {
            state,
            delay,
            left,
            factor,
            revlog: RevlogPreview {
                interval: delay.revlog_interval(),
                last_interval: current.delay(until_next_day).revlog_interval(),
                kind,
                factor: (1000.0 * ((difficulty - 1.0) / 9.0 + 0.1)).round() as u32,
            },
            label: format_seconds(delay.label_seconds(), input.learn_ahead),
        }
    };
    let [again, hard, good, easy] = raw.map(decorate);
    Ok(SchedulingStates {
        current,
        again,
        hard,
        good,
        easy,
    })
}
