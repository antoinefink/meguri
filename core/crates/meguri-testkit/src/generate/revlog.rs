//! Review histories: buttons and kinds for a card's review instants, and
//! the FSRS memory states fsrs 6.6.2 derives from them (default
//! parameters, desired retention 0.9), so a card's scheduling fields, its
//! `data` and its review log agree.
//!
//! Kinds follow the card through its phases: a new card's reviews are
//! learning reviews until Good or Easy graduates it; a review answered
//! Again lapses into relearning, which Hard, Good or Easy ends. Buttons are
//! drawn from the profile's mix for the review's kind, restricted where the
//! card must end in a given state.

use fsrs::{FSRS, FSRS6_DEFAULT_DECAY, FSRSItem, FSRSReview, MemoryState};
use meguri_model::revlog::RevlogKind;

use super::DESIRED_RETENTION;
use super::profile::RevlogShape;
use super::rng::Rng;

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

    /// The memory state after each review, from buttons and day numbers;
    /// `None` when fsrs refuses the history.
    pub fn states(&self, reviews: &[(u8, i64)]) -> Option<Vec<MemoryState>> {
        let mut previous = reviews.first()?.1;
        let item = FSRSItem {
            reviews: reviews
                .iter()
                .map(|&(rating, day)| {
                    let delta_t = (day - previous).max(0) as u32;
                    previous = day;
                    FSRSReview {
                        rating: u32::from(rating),
                        delta_t,
                    }
                })
                .collect(),
        };
        self.model.historical_memory_states(item, None).ok()
    }

    /// The review interval in days FSRS gives a stability at the desired
    /// retention, rounded, from 1 to 36,500.
    pub fn interval(&self, stability: f32) -> u32 {
        let days = self.model.next_interval(Some(stability), DESIRED_RETENTION, 3);
        if days.is_finite() {
            days.round().clamp(1.0, 36_500.0) as u32
        } else {
            1
        }
    }
}

/// The phase a card is in before a review: the kind of that review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Learning,
    Review,
    Relearning,
}

impl Phase {
    fn kind(self) -> RevlogKind {
        match self {
            Phase::Learning => RevlogKind::Learning,
            Phase::Review => RevlogKind::Review,
            Phase::Relearning => RevlogKind::Relearning,
        }
    }

    fn after(self, button: u8) -> Phase {
        match (self, button) {
            (Phase::Learning, 3 | 4) => Phase::Review,
            (Phase::Learning, _) => Phase::Learning,
            (_, 1) => Phase::Relearning,
            _ => Phase::Review,
        }
    }
}

/// How a history must end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Still learning: every button Again or Hard.
    Learning,
    /// Lapsed: the last review is a review or relearning answered Again.
    Relearning,
    /// In review: the last button graduates or passes.
    Review,
}

/// Buttons (1 to 4) of `kind` from the profile's mix, Good when it has none.
fn weights(shape: &RevlogShape, kind: RevlogKind) -> [u64; 4] {
    let name = super::profile::REVLOG_KINDS[kind.stored() as usize];
    match shape.ratings.get(name) {
        Some(counts) if counts[1..].iter().any(|&c| c > 0) => [counts[1], counts[2], counts[3], counts[4]],
        _ => [0, 0, 1, 0],
    }
}

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

/// Buttons and kinds of `count` reviews ending as `ending`, and the ending
/// reached: a single review cannot lapse, so `Relearning` with one review
/// ends `Learning`.
pub fn outcomes(shape: &RevlogShape, ending: Ending, count: usize, rng: &mut Rng) -> (Vec<(u8, RevlogKind)>, Ending) {
    let ending = if ending == Ending::Relearning && count < 2 {
        Ending::Learning
    } else {
        ending
    };
    let mut phase = Phase::Learning;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let last = i + 1 == count;
        let allowed: &[u8] = match ending {
            Ending::Learning => &[1, 2],
            Ending::Relearning if last => &[1],
            Ending::Relearning if i + 2 == count && phase == Phase::Learning => &[3, 4],
            Ending::Review if last && phase == Phase::Learning => &[3, 4],
            Ending::Review if last => &[2, 3, 4],
            _ => &[1, 2, 3, 4],
        };
        let kind = phase.kind();
        let button = draw(rng, weights(shape, kind), allowed);
        out.push((button, kind));
        phase = phase.after(button);
    }
    (out, ending)
}

/// `revlog.factor` with FSRS: the difficulty (1 to 10) mapped linearly onto
/// 100 to 1100 (`model/row-records` R5).
pub fn difficulty_factor(difficulty: f32) -> u32 {
    let mapped = 100.0 + (difficulty - 1.0) * 1000.0 / 9.0;
    if mapped.is_finite() {
        mapped.round().clamp(100.0, 1100.0) as u32
    } else {
        100
    }
}
