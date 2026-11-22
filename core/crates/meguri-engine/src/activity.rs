//! The activity heatmap's data (`stats/activity-heatmap`): one bounded query
//! per scope on the query reader, for the scheduler day the due counts use.

use meguri_model::ids::DeckId;
use meguri_sched::model::sched_config::SchedConfig;
use meguri_storage::queries::activity::{ACTIVITY_DAYS, ActivityScope};
use meguri_storage::{ReadRows, ReaderRole};

use crate::decks::names;
use crate::{Collection, EngineError};

const DAY: i64 = 86_400;

/// A calendar date of the proleptic Gregorian calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    /// 0 Sunday to 6 Saturday.
    pub weekday: u8,
}

/// AH6: what one query returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityHeatmap {
    pub generation: u64,
    /// The scheduler day number and the boundary `N` it was counted for.
    pub today: u32,
    pub next_day_at: i64,
    /// `T0`, the calendar date of day 0 (AH3).
    pub today_date: CivilDate,
    /// Reviews per day, index `k` = days ago, 0 to 370 (AH5).
    pub counts: Vec<u32>,
}

/// The heatmap of the whole collection (`deck` `None`) or of a deck and its
/// descendants by home deck (AH4). Reads only: the day comes from the day
/// clock as the due counts take it, the rows from one statement on the
/// query reader.
pub fn activity_heatmap(collection: &Collection, deck: Option<DeckId>) -> Result<ActivityHeatmap, EngineError> {
    let day = collection.check_rollover()?;
    let clock = &collection.core.exec.clock;
    let offset = i64::from(clock.utc_offset_at(clock.now_ms()).seconds());
    let lease = collection.reader(ReaderRole::Query)?;
    let rows = lease.snapshot()?;
    let hour = SchedConfig::new(&*rows).rollover_hour()?;
    let counts = match deck {
        None => rows.activity_day_counts(ActivityScope::Collection, day.next_day_at)?,
        Some(deck) => {
            let decks = subtree(&*rows, deck)?;
            rows.activity_day_counts(ActivityScope::HomeDecks(&decks), day.next_day_at)?
        }
    };
    debug_assert_eq!(counts.len(), ACTIVITY_DAYS as usize);
    Ok(ActivityHeatmap {
        generation: collection.generation().0,
        today: day.today,
        next_day_at: day.next_day_at,
        today_date: today_date(day.next_day_at, hour, offset),
        counts,
    })
}

/// The deck and its descendants; nothing when the id names no deck (AH4).
fn subtree(rows: &impl ReadRows, deck: DeckId) -> Result<Vec<DeckId>, EngineError> {
    let Some(root) = rows.deck(deck)? else {
        return Ok(Vec::new());
    };
    Ok(rows
        .deck_rows()?
        .into_iter()
        .filter(|d| d.id == deck || names::descendant(&d.name, &root.name))
        .map(|d| d.id)
        .collect())
}

/// AH3's `T0`: the local date, under `offset` (seconds east), of the start of
/// day 0 at rollover hour `hour`, taken to the nearest date so that an offset
/// change since `next_day_at` was computed cannot move it by a day.
pub fn today_date(next_day_at: i64, hour: u8, offset: i64) -> CivilDate {
    civil(today_day_number(next_day_at, hour, offset))
}

/// [`today_date`] as a count of days since 1970-01-01.
pub fn today_day_number(next_day_at: i64, hour: u8, offset: i64) -> i64 {
    let start = next_day_at - DAY - i64::from(hour.min(23)) * 3_600;
    (start + offset + DAY / 2).div_euclid(DAY)
}

/// The date `days` days after 1970-01-01 (a Thursday). The usual
/// era-based conversion of the proleptic Gregorian calendar.
pub fn civil(days: i64) -> CivilDate {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    CivilDate {
        year: year as i32,
        month: month as u8,
        day: day as u8,
        weekday: (days + 4).rem_euclid(7) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u8, d: u8, w: u8) -> CivilDate {
        CivilDate { year: y, month: m, day: d, weekday: w }
    }

    #[test]
    fn civil_dates_and_weekdays() {
        assert_eq!(civil(0), date(1970, 1, 1, 4));
        assert_eq!(civil(-1), date(1969, 12, 31, 3));
        assert_eq!(civil(11_016), date(2000, 2, 29, 2));
        assert_eq!(civil(20_739), date(2026, 10, 13, 2));
        assert_eq!(civil(20_819), date(2027, 1, 1, 5));
    }

    #[test]
    fn day_zero_is_the_date_whose_boundary_starts_it() {
        // N = 2026-10-13 04:00 +02:00: day 0 began 2026-10-12 04:00, a Monday.
        let n = 1_791_856_800;
        assert_eq!(today_date(n, 4, 7_200), date(2026, 10, 12, 1));
        // Rollover at midnight and at 23:00, same zone.
        assert_eq!(today_date(1_791_842_400, 0, 7_200), date(2026, 10, 12, 1));
        assert_eq!(today_date(1_791_925_200, 23, 7_200), date(2026, 10, 12, 1));
        // N computed under +02:00, read after the zone moved to +01:00
        // (D3.3): still the same date, also at hour 0.
        assert_eq!(today_date(1_791_842_400, 0, 3_600), date(2026, 10, 12, 1));
        assert_eq!(today_date(n, 4, 3_600), date(2026, 10, 12, 1));
        // West of UTC: America/New_York (-04:00), rollover 4.
        assert_eq!(today_date(1_791_878_400, 4, -14_400), date(2026, 10, 12, 1));
    }
}
