//! The scheduler's engine side. So far the day clock (U40, contract C6,
//! `specs/sched/day-clock.md`): `rollover_op` runs one day query with its
//! writes and unbury on a write scope; `day_clock` keeps the collection's
//! cached day and reported values, runs the query in `Rollover` ops, and
//! publishes `RolledOver`, `TimeZoneChanged` and `NextDayAtChanged`. The day
//! math itself is meguri-sched's `timing`.

pub mod day_clock;
pub mod memory;
pub mod rollover_op;

pub use day_clock::{DayClock, DayReport};
pub use rollover_op::{DayCache, DayQuery, day_query};
