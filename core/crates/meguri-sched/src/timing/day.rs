//! D2 to D4, D7 and D11 of `specs/sched/day-clock.md`.

use meguri_model::clock::{Clock, UtcOffset};

const DAY: i64 = 86_400;

/// The hour a missing or unreadable `rollover` reads as, and the value the
/// day query writes back (D2, D5).
pub const DEFAULT_ROLLOVER_HOUR: u8 = 4;

/// The learn-ahead limit a missing or unreadable `collapseTime` reads as, in
/// seconds (D2).
const DEFAULT_COLLAPSE_TIME: u32 = 1200;

/// The largest offset a config key may hold, in minutes either way (D2).
const MAX_OFFSET_MINUTES: i128 = 1439;

/// What one day computation needs (D3, D4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayInputs {
    /// `col.crt`, epoch seconds.
    pub crt: i64,
    /// The creation offset in minutes west, or `None` for legacy timing
    /// (D2: [`creation_offset`]).
    pub creation_offset: Option<i32>,
    /// The device's offset at `now`.
    pub offset: UtcOffset,
    /// The effective rollover hour, 0 to 23 (D2: [`rollover_hour`]).
    pub rollover_hour: u8,
    /// Epoch seconds.
    pub now: i64,
}

/// The day number and the instant the next day starts (epoch seconds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchedDay {
    pub today: u32,
    pub next_day_at: i64,
}

/// Today and `next_day_at` for `inputs`: D3 with a creation offset, D4
/// without one.
pub fn sched_day(inputs: &DayInputs) -> SchedDay {
    let offset = i64::from(inputs.offset.seconds());
    let hour = i64::from(inputs.rollover_hour.min(23)) * 3600;
    let now = inputs.now;
    let today_date = local_date(now, offset);
    let boundary_today = boundary(today_date, hour, offset);
    match inputs.creation_offset {
        Some(minutes) => {
            // D3.
            let created = local_date(inputs.crt, -i64::from(minutes) * 60);
            let rolled = boundary_today <= now;
            let days = today_date - created - i64::from(!rolled);
            SchedDay {
                today: u32::try_from(days.max(0)).unwrap_or(u32::MAX),
                next_day_at: if rolled { boundary_today + DAY } else { boundary_today },
            }
        }
        None => {
            // D4: the signed day count, truncated toward zero, kept as the
            // unsigned 32-bit number it wraps to.
            let start = boundary(local_date(inputs.crt, offset), hour, offset);
            let days = (now - start) / DAY;
            SchedDay {
                today: days as u32,
                next_day_at: if boundary_today < now {
                    boundary_today + DAY
                } else {
                    boundary_today
                },
            }
        }
    }
}

/// The local date of an instant under an offset, as days since 1970-01-01
/// (D1).
fn local_date(instant: i64, offset: i64) -> i64 {
    (instant + offset).div_euclid(DAY)
}

/// The instant whose local time under `offset` is `date` at `hour`
/// (seconds after midnight).
fn boundary(date: i64, hour: i64, offset: i64) -> i64 {
    date * DAY + hour - offset
}

/// An offset in minutes west of UTC, as config keys store it (D1). Offsets
/// are whole minutes; a part of a minute is dropped.
pub fn minutes_west(offset: UtcOffset) -> i32 {
    -offset.seconds() / 60
}

/// The effective rollover hour of a stored `rollover` (D2): `stored` is the
/// integer the key holds, `None` when it is missing or holds anything but
/// an integer JSON number. `None` back means unreadable: the hour is
/// [`DEFAULT_ROLLOVER_HOUR`] and the day query writes it (D5).
pub fn rollover_hour(stored: Option<i128>) -> Option<u8> {
    let value = u8::try_from(stored?).ok()?;
    Some(value.min(23))
}

/// The creation offset in minutes west, or `None` for legacy timing (D2).
pub fn creation_offset(stored: Option<i128>) -> Option<i32> {
    offset_minutes(stored)
}

/// The last offset the device saw, in minutes west; 0 when missing or
/// unreadable (D2).
pub fn local_offset(stored: Option<i128>) -> i32 {
    offset_minutes(stored).unwrap_or(0)
}

fn offset_minutes(stored: Option<i128>) -> Option<i32> {
    let value = stored?;
    if (-MAX_OFFSET_MINUTES..=MAX_OFFSET_MINUTES).contains(&value) {
        i32::try_from(value).ok()
    } else {
        None
    }
}

/// The last day buried cards were restored on; 0 when missing or unreadable
/// (D2).
pub fn last_unburied(stored: Option<i128>) -> u32 {
    stored.and_then(|value| u32::try_from(value).ok()).unwrap_or(0)
}

/// The learn-ahead limit in seconds (D2, R602, R760); 1200 when missing or
/// unreadable.
pub fn collapse_time(stored: Option<i128>) -> u32 {
    stored
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(DEFAULT_COLLAPSE_TIME)
}

/// D7: whether the day query restores buried cards: `lastUnburied` below
/// today or more than 7 days above it.
pub fn needs_unbury(last_unburied: u32, today: u32) -> bool {
    last_unburied < today || u64::from(last_unburied) > u64::from(today) + 7
}

/// D7's table: the queue a buried card returns to, from its stored `type`
/// and `due`; `None` for a type the table does not list (the card stays as
/// it is).
pub fn unburied_queue(card_type: i64, due: i64) -> Option<i64> {
    match card_type {
        0 => Some(0),
        2 => Some(2),
        // A learning due is a timestamp; an interday one is a day number.
        1 | 3 if due > 1_000_000_000 => Some(1),
        1 | 3 => Some(3),
        _ => None,
    }
}

/// `crt` and the creation offset a new collection starts with (D11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewCollectionTiming {
    /// Epoch seconds.
    pub crt: i64,
    /// Minutes west.
    pub creation_offset: i32,
}

/// D11: the timing values of a collection created at `created_at` (epoch
/// seconds), with the offsets `clock` reports. The day starts at 04:00 here
/// whatever `rollover` says.
pub fn new_collection_timing(created_at: i64, clock: &dyn Clock) -> NewCollectionTiming {
    let offset_at = |instant: i64| clock.utc_offset_at(instant.saturating_mul(1000));
    let offset = i64::from(offset_at(created_at).seconds());
    let start = boundary(
        local_date(created_at, offset),
        i64::from(DEFAULT_ROLLOVER_HOUR) * 3600,
        offset,
    );
    let crt = if start <= created_at { start } else { start - DAY };
    NewCollectionTiming {
        crt,
        creation_offset: minutes_west(offset_at(crt)),
    }
}
