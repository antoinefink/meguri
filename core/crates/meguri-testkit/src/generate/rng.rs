//! The generator's random streams. Every stream is a ChaCha generator
//! (`rand`'s `StdRng`, ChaCha12 at the pinned 0.9.4) seeded with one value
//! of the randomness seam (`meguri_model::rng`, R1875): the run's seed and a
//! stream number go through `SeededRandom`, so each part of a collection
//! (structure, each note, each media file) draws from its own stream and
//! the output does not depend on the order parts are generated in.

use meguri_model::rng::{RandomSource, SeededRandom};
use rand::rngs::StdRng;
use rand::{Rng as _, RngCore, SeedableRng};

/// Stream families; an item's stream is its family and its index.
#[derive(Clone, Copy, Debug)]
pub enum Stream {
    Structure = 1,
    Note = 2,
    Media = 3,
    Recipe = 4,
}

#[derive(Debug)]
pub struct Rng(StdRng);

impl Rng {
    /// The stream of item `index` of `family` for a run seeded with `seed`.
    pub fn stream(seed: u64, family: Stream, index: u64) -> Rng {
        let mixer = SeededRandom::new(seed);
        let base = mixer.next_u64();
        let item = SeededRandom::new(base ^ ((family as u64) << 56) ^ index.wrapping_mul(0xD6E8_FEB8_6659_FD93));
        Rng(StdRng::seed_from_u64(item.next_u64()))
    }

    pub fn u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform in `0..n`; 0 when `n` is 0.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.0.random_range(0..n) }
    }

    /// Uniform in `lo..hi`; `lo` when the range is empty.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo { lo } else { self.0.random_range(lo..hi) }
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        self.0.random::<f64>()
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    pub fn fill(&mut self, bytes: &mut [u8]) {
        self.0.fill_bytes(bytes);
    }

    /// An index drawn by weight; `None` when every weight is zero.
    pub fn weighted(&mut self, weights: &[u64]) -> Option<usize> {
        let total: u64 = weights.iter().sum();
        if total == 0 {
            return None;
        }
        let mut pick = self.below(total);
        for (i, &w) in weights.iter().enumerate() {
            if pick < w {
                return Some(i);
            }
            pick -= w;
        }
        None
    }
}
