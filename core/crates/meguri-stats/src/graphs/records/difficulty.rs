//! The `difficulty` record (`stats/graphs-cards` C6, C8, C11): the cards
//! with a memory state by difficulty percent, the median difficulty (Anki's
//! `average` member) and the started cards left out for having none.
//!
//! Encoding (G6): the percent map (one integer member: cards), then the
//! median as binary32, then the left-out count.

use std::collections::BTreeMap;

use super::card_rules::{lacks_memory, percent_bin, tally, write_counts};
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-cards` C8.3.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct DifficultyRecord {
    /// The `eases` member: difficulty percent (C8.1) → cards.
    pub by_percent: BTreeMap<u32, u32>,
    /// The `average` member: the median of C8.2, × 100.
    pub median: f32,
    /// C6.2.
    pub left_out: u32,
}

/// C8.2's value of a card: the difficulty scaled to 0–1, in f32.
fn unit(difficulty: f32) -> f32 {
    (difficulty - 1.0) / 9.0
}

/// An `f32` as a key whose integer order is IEEE-754's total order
/// (−∞ first, +∞ last).
fn ordered(value: f32) -> i32 {
    let bits = value.to_bits() as i32;
    bits ^ (((bits >> 31) as u32) >> 1) as i32
}

/// How many cards hold each distinct value of C8.2, in ascending order.
#[derive(Default)]
struct ValueCounts {
    counts: BTreeMap<i32, (f32, u32)>,
    total: u64,
}

impl ValueCounts {
    fn add(&mut self, value: f32) {
        self.counts.entry(ordered(value)).or_insert((value, 0)).1 += 1;
        self.total += 1;
    }

    /// The value at 0-based position `rank` of the sorted values.
    fn at(&self, rank: u64) -> f32 {
        let mut below = 0u64;
        for (value, count) in self.counts.values() {
            below += u64::from(*count);
            if rank < below {
                return *value;
            }
        }
        f32::NAN
    }

    /// C8.2: the middle value, or the f32 mean of the two middle values;
    /// 0 with no card.
    fn middle(&self) -> f32 {
        match self.total {
            0 => 0.0,
            n if n % 2 == 1 => self.at(n / 2),
            n => (self.at(n / 2 - 1) + self.at(n / 2)) / 2.0,
        }
    }
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<DifficultyRecord, NotBuilt> {
    let mut record = DifficultyRecord::default();
    let mut values = ValueCounts::default();
    for card in input.scope.cards() {
        let Some(memory) = card.memory else {
            record.left_out += u32::from(lacks_memory(&card));
            continue;
        };
        let scaled = unit(memory.difficulty);
        values.add(scaled);
        // C8.1: the × 100 is its own f32 step.
        tally(&mut record.by_percent, percent_bin(scaled * 100.0));
    }
    record.median = values.middle() * 100.0;
    Ok(record)
}

impl GraphRecord for DifficultyRecord {
    fn graph(&self) -> Graph {
        Graph::Difficulty
    }

    fn encode(&self, out: &mut Encoder) {
        write_counts(out, &self.by_percent);
        out.f32(self.median);
        out.integer(i64::from(self.left_out));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EIGHT: [f32; 8] = [1.0, 1.09, 2.08, 4.6, 5.5, 10.0, 11.0, 0.5];

    fn median_of(difficulties: &[f32]) -> f32 {
        let mut values = ValueCounts::default();
        for d in difficulties {
            values.add(unit(*d));
        }
        values.middle() * 100.0
    }

    /// The same median from a sort of every value (C8.2's check).
    fn sorted_median(difficulties: &[f32]) -> f32 {
        let mut values: Vec<f32> = difficulties.iter().map(|d| unit(*d)).collect();
        values.sort_by(f32::total_cmp);
        let n = values.len();
        if n == 0 {
            return 0.0;
        }
        let middle = if n % 2 == 1 { values[n / 2] } else { (values[n / 2 - 1] + values[n / 2]) / 2.0 };
        middle * 100.0
    }

    #[test]
    fn c8_1_bins_take_each_f32_step() {
        let keys: Vec<u32> = [1.0f32, 1.09, 2.08, 4.6, 5.5, 10.0, 11.0, 0.5, f32::INFINITY, f32::NEG_INFINITY]
            .iter()
            .map(|d| percent_bin(unit(*d) * 100.0))
            .collect();
        assert_eq!(keys, [0, 1, 11, 39, 50, 99, 111, 0, u32::MAX, 0]);
    }

    #[test]
    fn c8_2_medians() {
        assert_eq!(median_of(&EIGHT), 26.0);
        assert_eq!(median_of(&EIGHT[..7]), 39.999996);
        let mut nine = EIGHT.to_vec();
        nine.push(f32::INFINITY);
        assert_eq!(median_of(&nine), 39.999996);
        assert_eq!(median_of(&[f32::NEG_INFINITY, 5.5, 10.0]), 50.0);
        assert_eq!(median_of(&[]), 0.0);
        assert!(median_of(&[f32::NEG_INFINITY, f32::INFINITY]).is_nan());
    }

    #[test]
    fn c8_2_distinct_value_counts_match_a_sort_of_every_value() {
        // A deterministic spread with many repeats and both infinities.
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for len in 0..60usize {
            let values: Vec<f32> = (0..len)
                .map(|_| match next() % 50 {
                    0 => f32::INFINITY,
                    1 => f32::NEG_INFINITY,
                    k => 1.0 + (k as f32) * 0.37 + (next() % 3) as f32 * 0.1,
                })
                .collect();
            let (counted, sorted) = (median_of(&values), sorted_median(&values));
            assert!(counted.to_bits() == sorted.to_bits() || (counted.is_nan() && sorted.is_nan()), "{values:?}");
        }
    }
}
