//! The engine's clock module for elapsed time: wait deadlines, progress
//! pacing, op timing and declared delays. It is never collection time, so it
//! does not go through `meguri_model::clock` (R1875 covers the instants the
//! core stores or schedules with); `core/clippy.toml` lets only a clock
//! module read the system clock.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A source of monotonic time since an arbitrary start; tests pass their own.
pub type MonotonicClock = fn() -> Duration;

/// Monotonic time since the first call in this process.
#[allow(clippy::disallowed_methods)]
pub fn now() -> Duration {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}
