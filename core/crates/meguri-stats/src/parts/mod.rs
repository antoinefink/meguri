//! The Statistics page's parts computed per request beside Anki's records
//! (`stats/history-reads` H3.4, H4, H6, H9 to H13; U288, U291): one pass
//! over the request's resolved scope each, with no kept aggregate of their
//! own (H14.1's last paragraph). The page request (`crate::page`) runs
//! them; Show in Browse's day read ([`day_cards`]) runs on a click.
//!
//! - Days are H1's: a row's day index of `stats/day-buckets` B3 relative to
//!   G1's `N`, every later day folded onto day 0 ([`page_day`]).
//! - "Remembered" is `stats/statistics-page` S2's one rule: V4.1's rows
//!   with any rating but Again (`graphs::records::retention::outcome`).
//! - Read only (H17): nothing here writes the collection.

pub mod activity;
pub mod by_answer;
pub mod by_deck;
pub mod day_cards;
pub(crate) mod decks;
pub mod retention_by_day;
pub mod studied_today;

/// Seconds in one of B3's fixed day spans.
const DAY: i64 = 86_400;

/// H1's day of a review-log row: B3's day index relative to `next_day_at`
/// (`t = floor(id / 1,000)`; day `d` ≤ −1 ends at the whole second
/// `N + d × 86,400`), with every row of day 1 or later on day 0.
pub fn page_day(id_ms: i64, next_day_at: i64) -> i32 {
    let t = id_ms.div_euclid(1000);
    // The day whose end is the first boundary at or after `t`, then folded.
    let day = -(next_day_at.saturating_sub(t).div_euclid(DAY));
    i32::try_from(day.min(0)).unwrap_or(i32::MIN)
}

/// The review-log ids of H1's day `day` (≤ 0): `start <= id < end`, with no
/// end for day 0, which holds every later row, and `start` at least 0.
/// `None` for a day after 0.
pub fn day_ids(day: i32, next_day_at: i64) -> Option<(i64, Option<i64>)> {
    if day > 0 {
        return None;
    }
    let day = i64::from(day);
    // Day `d` holds the whole seconds `N + (d − 1) × 86,400 + 1` to
    // `N + d × 86,400`.
    let first = next_day_at.saturating_add((day - 1) * DAY).saturating_add(1);
    // Rows with a negative id are read by no part (G3, H9, H11).
    let start = first.saturating_mul(1000).max(0);
    let end = (day < 0).then(|| next_day_at.saturating_add(day * DAY).saturating_add(1).saturating_mul(1000));
    Some((start, end))
}

/// The first review-log id of an H1 period window (`one_month` to
/// `one_year`): the rows with `t ≥ N − days × 86,400`.
pub(crate) fn period_start(next_day_at: i64, days: i64) -> i64 {
    next_day_at.saturating_sub(days * DAY).saturating_mul(1000)
}

/// The days of H1's four period windows with a time bound, in order:
/// `one_month`, `three_months`, `six_months`, `one_year`.
pub(crate) const PERIOD_DAYS: [i64; 4] = [30, 90, 182, 365];

#[cfg(test)]
mod tests {
    use super::*;

    const N: i64 = 1_791_856_800;

    #[test]
    fn page_days_follow_b3_and_fold_later_days_onto_today() {
        let at = |t: i64| page_day(t * 1000, N);
        assert_eq!(at(N), 0);
        assert_eq!(at(N - 1), 0);
        assert_eq!(at(N - 86_399), 0);
        assert_eq!(at(N - 86_400), -1);
        assert_eq!(at(N - 86_401), -1);
        assert_eq!(at(N - 2 * 86_400), -2);
        assert_eq!(at(N + 3_600), 0);
        assert_eq!(at(N + 90_000), 0);
        // A millisecond before a whole second belongs to the second before.
        assert_eq!(page_day((N - 86_399) * 1000 - 1, N), -1);
    }

    #[test]
    fn day_ids_hold_exactly_the_rows_of_their_day() {
        for day in [-400, -2, -1, 0] {
            let (start, end) = day_ids(day, N).unwrap();
            assert_eq!(page_day(start, N), day);
            assert_eq!(page_day(start - 1, N), day - 1);
            match end {
                Some(end) => {
                    assert_eq!(page_day(end - 1, N), day);
                    assert_eq!(page_day(end, N), day + 1);
                }
                None => assert_eq!(day, 0),
            }
        }
        assert_eq!(day_ids(1, N), None);
    }
}
