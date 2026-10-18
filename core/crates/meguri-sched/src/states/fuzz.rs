//! Interval fuzz (specs/sched/fuzz.md): the seed (Z1), the factor (Z2), the
//! delta (Z3), the range and pick (Z4), the no-regression minimum (Z5) and
//! the fuzz delta of a stored card (Z6). Everything here is integer
//! arithmetic or single 32-bit float operations in a fixed order, with no
//! math-library call, so it gives the same result on every platform.

use fuzz_rand::rngs::StdRng;
use fuzz_rand::{Rng as _, SeedableRng};

/// Z1: the seed of an answer, the card id plus the review count before the
/// answer, added with wrap-around.
pub fn seed(card_id: u64, reps: u64) -> u64 {
    card_id.wrapping_add(reps)
}

/// Z2: the first `random_range(0.0..1.0)` draw of `StdRng` (rand 0.9.4)
/// seeded with `seed`; in [0, 1).
pub fn factor(seed: u64) -> f32 {
    StdRng::seed_from_u64(seed).random_range(0.0f32..1.0f32)
}

/// Z7: a fresh integer draw; independent of the review factor's generator.
pub fn learning_delay(seconds: u32, seed: u64) -> u32 {
    let width = ((seconds as f32 * 0.25).min(300.0)) as u32;
    if width == 0 {
        return seconds;
    }
    let mut rng = StdRng::seed_from_u64(seed);
    if let Some(end) = seconds.checked_add(width) {
        rng.random_range(seconds..end)
    } else {
        // Imported steps can saturate to u32::MAX. Z7's explicit boundary
        // rule clips candidates inclusively; normal draws remain unchanged.
        let count = u64::from(width).min(u64::from(u32::MAX) - u64::from(seconds) + 1) as u32;
        if count == 1 {
            seconds
        } else {
            (u64::from(seconds) + u64::from(rng.random_range(0..count))) as u32
        }
    }
}

/// Z3: the fuzz on each side of an interval of `x` days, every operation
/// rounded to `f32`.
pub fn delta(x: f32) -> f32 {
    if x < 2.5 {
        return 0.0;
    }
    const TERMS: [(f32, f32, f32); 3] = [(2.5, 7.0, 0.15), (7.0, 20.0, 0.1), (20.0, f32::MAX, 0.05)];
    let mut delta = 1.0f32;
    for (start, end, factor) in TERMS {
        delta += factor * (x.min(end) - start).max(0.0);
    }
    delta
}

/// `round` with halves away from zero (Rust's `f32::round`), as an unsigned
/// integer; negative values become 0.
fn round_to_u32(x: f32) -> u32 {
    // Saturating: the values are small and never negative here.
    x.round() as u32
}

/// Z4 steps 1-5: the `(lower, upper)` days of the fuzz range of `x` with
/// the minimum `minimum` and the maximum `maximum`.
pub fn range(x: f32, minimum: u32, maximum: u32) -> (u32, u32) {
    let minimum = minimum.min(maximum);
    let x = x.clamp(minimum as f32, maximum as f32);
    let delta = delta(x);
    let lower = round_to_u32(x - delta).clamp(minimum, maximum);
    let mut upper = round_to_u32(x + delta).clamp(minimum, maximum);
    if lower == upper && upper > 2 && upper < maximum {
        upper = lower + 1;
    }
    (lower, upper)
}

/// Z4: the fuzzed interval of `x` days with the factor `factor` (Z2).
pub fn fuzzed_interval(x: f32, minimum: u32, maximum: u32, factor: f32) -> u32 {
    let (lower, upper) = range(x, minimum, maximum);
    let count = (1 + upper - lower) as f32;
    (lower as f32 + factor * count).floor() as u32
}

/// Z5: the smallest interval a Hard, Good or Easy answer may give a review
/// card whose current interval is `current`, for the unrounded new interval
/// `x`; 0 when there is none.
pub fn no_regression_minimum(x: f32, current: u32, maximum: u32) -> u32 {
    let rounded = round_to_u32(x);
    let (_, upper) = range(x, 1, maximum);
    if rounded > current {
        current.saturating_add(1)
    } else if current <= upper {
        current
    } else {
        0
    }
}

/// Z6: the change fuzz makes to a whole interval of `interval` days for a
/// card with this id and review count before the answer, whose home preset
/// has the maximum interval `maximum`.
pub fn stored_card_delta(card_id: u64, reps: u64, interval: u32, maximum: u32) -> i64 {
    let factor = factor(seed(card_id, reps));
    i64::from(fuzzed_interval(interval as f32, 1, maximum, factor)) - i64::from(interval)
}
