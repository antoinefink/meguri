//! The randomness seam (R1875). The core never draws OS or thread-local
//! randomness: every random choice takes its bits, or the seed of a generator
//! for one task, from a [`RandomSource`]. The host seeds the source once; tests
//! and the scenario runner pass fixed seeds, so a run replays exactly.
//! `tools/mg core-lint` rejects `rand::thread_rng` and friends (core/clippy.toml).
//!
//! Interval fuzz does not use this seam: its generator is seeded from the card
//! itself, as the oracle does.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// A source of random 64-bit values.
pub trait RandomSource: Send + Sync + fmt::Debug {
    /// The next 64 random bits. To drive a generator for one task, seed it
    /// with one value: `StdRng::seed_from_u64(source.next_u64())`.
    fn next_u64(&self) -> u64;
}

/// A deterministic source: the SplitMix64 sequence of its seed. Calls from
/// several threads each get a distinct value of the sequence.
#[derive(Debug)]
pub struct SeededRandom {
    state: AtomicU64,
}

/// SplitMix64's state increment (the 64-bit golden ratio).
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

impl SeededRandom {
    pub const fn new(seed: u64) -> SeededRandom {
        SeededRandom {
            state: AtomicU64::new(seed),
        }
    }
}

impl RandomSource for SeededRandom {
    fn next_u64(&self) -> u64 {
        let mut z = self
            .state
            .fetch_add(GOLDEN_GAMMA, Ordering::Relaxed)
            .wrapping_add(GOLDEN_GAMMA);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Q3.6 scheduler stream. Test-only; leaves the legacy SplitMix sequence intact.
#[cfg(feature = "test-hooks")]
pub struct ChaCha20Random(std::sync::Mutex<rand_chacha::ChaCha20Rng>);
#[cfg(feature = "test-hooks")]
impl ChaCha20Random {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self(std::sync::Mutex::new(rand_chacha::ChaCha20Rng::seed_from_u64(seed)))
    }
}
#[cfg(feature = "test-hooks")]
impl fmt::Debug for ChaCha20Random {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChaCha20Random")
    }
}
#[cfg(feature = "test-hooks")]
impl RandomSource for ChaCha20Random {
    fn next_u64(&self) -> u64 {
        use rand::RngCore;
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next_u64()
    }
}
