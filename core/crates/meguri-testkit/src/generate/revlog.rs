//! Review histories (`fixtures/synthetic-collections` F5): the buttons of a
//! card's answers, the phase each answer leaves the card in and its delay
//! (the answer table, F5.1), and the FSRS memory states fsrs 6.6.2 derives
//! from them (F5.3), so a card's scheduling fields, its `data` and its
//! review log agree.
//!
//! Buttons are drawn from the profile's mix for the answer's kind,
//! restricted to those from which the card can still end in its target
//! phase with the answers left (F5.2).

use fsrs::{FSRS, FSRS6_DEFAULT_DECAY, FSRSItem, FSRSReview, MemoryState};
use meguri_model::revlog::RevlogKind;

use super::profile::RevlogShape;
use super::rng::Rng;
use super::{DESIRED_RETENTION, GenError};

/// FSRS with the default parameters.
#[derive(Debug)]
pub struct Fsrs {
    model: FSRS,
}

impl Default for Fsrs {
    fn default() -> Fsrs {
        Fsrs::new()
    }
}

impl Fsrs {
    pub fn new() -> Fsrs {
        Fsrs {
            model: FSRS::new(&[]).expect("the default FSRS parameters are valid"),
        }
    }

    /// The decay the default parameters use, written to `cards.data`.
    pub fn decay(&self) -> f32 {
        FSRS6_DEFAULT_DECAY
    }

    /// The memory state after each answer, from buttons and day numbers
    /// (whole days since the previous answer, 0 for the first). A history
    /// fsrs refuses is a generation error (F5.3).
    pub fn states(&self, answers: &[(u8, i64)]) -> Result<Vec<MemoryState>, GenError> {
        let Some(&(_, mut previous)) = answers.first() else {
            return Err(GenError::Fsrs("an empty history".to_owned()));
        };
        let item = FSRSItem {
            reviews: answers
                .iter()
                .map(|&(rating, day)| {
                    let delta_t = u32::try_from((day - previous).max(0)).unwrap_or(u32::MAX);
                    previous = day;
                    FSRSReview {
                        rating: u32::from(rating),
                        delta_t,
                    }
                })
                .collect(),
        };
        self.model
            .historical_memory_states(item, None)
            .map_err(|e| GenError::Fsrs(format!("{e:?} for the history {answers:?}")))
    }

    /// F days: the interval fsrs gives a stability at the desired
    /// retention, rounded to the nearest day, from 1 to 36,500.
    pub fn interval(&self, stability: f32) -> u32 {
        let days = self.model.next_interval(Some(stability), DESIRED_RETENTION, 3);
        if days.is_finite() {
            days.round().clamp(1.0, 36_500.0) as u32
        } else {
            1
        }
    }
}

/// Where a card is before an answer (F5.1): new cards are at learning step
/// 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Step1,
    Step2,
    Review,
    Relearning,
}

/// What an answer schedules (F5.1): a delay in seconds, or F days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delay {
    Secs(i64),
    Days,
}

impl Phase {
    /// The review-log kind of an answer given in this phase.
    pub fn kind(self) -> RevlogKind {
        match self {
            Phase::Step1 | Phase::Step2 => RevlogKind::Learning,
            Phase::Review => RevlogKind::Review,
            Phase::Relearning => RevlogKind::Relearning,
        }
    }

    /// F5.1: the phase an answer leaves the card in, and its delay.
    pub fn answer(self, button: u8) -> (Phase, Delay) {
        match (self, button) {
            (Phase::Step1, 1) | (Phase::Step2, 1) => (Phase::Step1, Delay::Secs(60)),
            (Phase::Step1, 2) => (Phase::Step1, Delay::Secs(330)),
            (Phase::Step1, 3) => (Phase::Step2, Delay::Secs(600)),
            (Phase::Step2, 2) => (Phase::Step2, Delay::Secs(600)),
            (Phase::Review, 1) | (Phase::Relearning, 1) => (Phase::Relearning, Delay::Secs(600)),
            (Phase::Relearning, 2) => (Phase::Relearning, Delay::Secs(900)),
            _ => (Phase::Review, Delay::Days),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The phase a history ends in (F5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Never leaves learning: step 1 or 2.
    Learning,
    Relearning,
    Review,
}

impl Ending {
    fn accepts(self, phase: Phase) -> bool {
        match self {
            Ending::Learning => matches!(phase, Phase::Step1 | Phase::Step2),
            Ending::Relearning => phase == Phase::Relearning,
            Ending::Review => phase == Phase::Review,
        }
    }
}

/// The phases reachable from each phase with exactly `n` answers, indexed
/// by [`Phase::index`].
fn reachable(phase: Phase, n: usize) -> [bool; 4] {
    let mut now = [false; 4];
    now[phase.index()] = true;
    for _ in 0..n {
        let mut next = [false; 4];
        for from in [Phase::Step1, Phase::Step2, Phase::Review, Phase::Relearning] {
            if now[from.index()] {
                for button in 1..=4 {
                    next[from.answer(button).0.index()] = true;
                }
            }
        }
        now = next;
    }
    now
}

/// One answer of a history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    pub button: u8,
    pub kind: RevlogKind,
    pub after: Phase,
    pub delay: Delay,
}

/// Buttons (1 to 4) of `kind` from the profile's mix, Good when it has none.
fn weights(shape: &RevlogShape, kind: RevlogKind) -> [u64; 4] {
    let name = super::profile::REVLOG_KINDS[kind.stored() as usize];
    match shape.ratings.get(name) {
        Some(counts) if counts[1..].iter().any(|&c| c > 0) => [counts[1], counts[2], counts[3], counts[4]],
        _ => [0, 0, 1, 0],
    }
}

/// A button by weight among the allowed ones; uniform among them when they
/// all weigh 0.
fn draw(rng: &mut Rng, weights: [u64; 4], allowed: &[u8]) -> u8 {
    let masked: Vec<u64> = (1..=4u8)
        .map(|b| {
            if allowed.contains(&b) {
                weights[usize::from(b) - 1]
            } else {
                0
            }
        })
        .collect();
    match rng.weighted(&masked) {
        Some(i) => i as u8 + 1,
        None => allowed[rng.below(allowed.len() as u64) as usize],
    }
}

/// The answers of a history of `count` answers ending as `ending`, and the
/// ending reached: a relearning target with fewer than two answers ends in
/// learning instead (F5.2).
pub fn outcomes(shape: &RevlogShape, ending: Ending, count: usize, rng: &mut Rng) -> (Vec<Answer>, Ending) {
    let ending = if ending == Ending::Relearning && count < 2 {
        Ending::Learning
    } else {
        ending
    };
    let mut phase = Phase::Step1;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let left = count - i - 1;
        let allowed: Vec<u8> = (1..=4u8)
            .filter(|&b| {
                let reach = reachable(phase.answer(b).0, left);
                [Phase::Step1, Phase::Step2, Phase::Review, Phase::Relearning]
                    .iter()
                    .any(|&p| reach[p.index()] && ending.accepts(p))
            })
            .collect();
        let kind = phase.kind();
        let button = draw(rng, weights(shape, kind), &allowed);
        let (after, delay) = phase.answer(button);
        out.push(Answer {
            button,
            kind,
            after,
            delay,
        });
        phase = after;
    }
    (out, ending)
}

/// `revlog.factor` under FSRS (F5.6): the difficulty `D` (1 to 10) mapped
/// in `f32` arithmetic, each step rounded to `f32`: `(D - 1) / 9 + 0.1`,
/// times 1000, rounded half away from zero.
pub fn difficulty_factor(difficulty: f32) -> u32 {
    let n: f32 = (difficulty - 1.0) / 9.0;
    let s: f32 = n + 0.1;
    let scaled: f32 = s * 1000.0;
    let rounded = scaled.round();
    if rounded.is_finite() {
        rounded.clamp(0.0, u32::MAX as f32) as u32
    } else {
        0
    }
}
