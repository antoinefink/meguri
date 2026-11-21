//! Today's numbers for the Today page (`stats/studied-today` T12): N6's
//! answers and time for the studied-today window and for the seven
//! scheduler days that end with today, read on the query reader.

use meguri_storage::{ReadRows, ReaderRole};

use crate::{Collection, EngineError};

const DAY_MS: i64 = 86_400_000;

/// T12.1: what one read returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StudiedTodayNumbers {
    /// N6's `cards`: the rows of the studied-today window.
    pub answers: u64,
    /// The different cards those rows answer.
    pub cards: u64,
    /// N6's `time_ms`: the sum of their `time`.
    pub time_ms: i64,
    /// The same rule over the seven scheduler days that end with today.
    pub recent_answers: u64,
    pub recent_time_ms: i64,
    pub generation: u64,
}

/// `S` of `stats/day-buckets` B2: the start of day 0 in epoch milliseconds;
/// the window holds the rows whose `id` is greater.
pub fn window_start(next_day_at: i64) -> i64 {
    next_day_at.saturating_sub(86_400).saturating_mul(1000)
}

/// The start of the seven-day window, `S − 6` days (T12.1), exclusive as `S` is.
pub fn recent_start(next_day_at: i64) -> i64 {
    window_start(next_day_at).saturating_sub(6 * DAY_MS)
}

/// Reads only: the day from the day clock as the due counts take it, the
/// rows from two statements on one query snapshot. Never the resident
/// values (`stats/counting-rules` N7), so there is no `building` result.
pub fn studied_today_numbers(collection: &Collection) -> Result<StudiedTodayNumbers, EngineError> {
    let day = collection.check_rollover()?;
    let lease = collection.reader(ReaderRole::Query)?;
    let rows = lease.snapshot()?;
    let today = rows.studied_since(window_start(day.next_day_at))?;
    let recent = rows.studied_since(recent_start(day.next_day_at))?;
    Ok(StudiedTodayNumbers {
        answers: today.answers,
        cards: today.cards,
        time_ms: today.time_ms,
        recent_answers: recent.answers,
        recent_time_ms: recent.time_ms,
        generation: collection.generation().0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_start_at_day_zero_and_six_days_before() {
        let n = 1_791_856_800;
        assert_eq!(window_start(n), (n - 86_400) * 1000);
        assert_eq!(recent_start(n), (n - 7 * 86_400) * 1000);
        assert_eq!(window_start(i64::MIN), i64::MIN);
    }
}
