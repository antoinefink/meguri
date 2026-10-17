//! The scheduler's day clock, pure (`specs/sched/day-clock.md`): the day
//! number ("today") and the next day boundary from `crt`, the creation
//! offset, the current UTC offset, the rollover hour and now, in both timing
//! modes (D3, D4); the readings of the timing keys (D2); the unbury rule
//! (D7); and the timing values of a new collection (D11). The cache, the
//! writes and the rollover effects are meguri-engine's `sched` module.
//!
//! Instants are whole epoch seconds (D1). Offsets are seconds east of UTC as
//! [`UtcOffset`] holds them, or minutes west where config keys store them.

mod day;

pub use day::{
    DEFAULT_ROLLOVER_HOUR, DayInputs, NewCollectionTiming, SchedDay, collapse_time, creation_offset, last_unburied,
    local_offset, minutes_west, needs_unbury, new_collection_timing, rollover_hour, sched_day, unburied_queue,
};
