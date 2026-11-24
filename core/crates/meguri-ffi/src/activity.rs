//! The activity heatmap's data over FFI (`stats/activity-heatmap` AH6): one
//! short read per scope, called on the host's query lane.

use meguri_model::ids::DeckId;

use crate::{callback, error::CoreError, runtime};

/// One scope's reviews per day (AH6), flattened for the host: the counts
/// first, then where day 0 falls in the calendar and in the scheduler.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActivityCounts {
    /// Reviews on each day, index = days before today (0 to 370).
    pub per_day: Vec<u32>,
    /// `T0`: day 0's proleptic Gregorian date.
    pub day_zero_year: i32,
    pub day_zero_month: u32,
    pub day_zero_day: u32,
    /// 0 Sunday to 6 Saturday.
    pub day_zero_weekday: u32,
    /// The scheduler day number, and `N` (epoch seconds) it ends at.
    pub scheduler_day: u32,
    pub boundary_secs: i64,
    /// The collection generation the read saw.
    pub generation: u64,
}

/// One studied day, sparse so ancient timestamps cannot cause dense allocation.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActivityDayCount {
    pub days_ago: i64,
    pub count: u32,
}

/// Mac Home's sparse result. The bounded result type and export stay unchanged.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActivityHistoryCounts {
    pub window: ActivityCounts,
    /// Every studied day. Missing days mean zero.
    pub history: Vec<ActivityDayCount>,
}

impl From<meguri_engine::activity::ActivityHeatmap> for ActivityCounts {
    fn from(read: meguri_engine::activity::ActivityHeatmap) -> Self {
        let t0 = read.today_date;
        ActivityCounts {
            per_day: read.counts,
            day_zero_year: t0.year,
            day_zero_month: t0.month.into(),
            day_zero_day: t0.day.into(),
            day_zero_weekday: t0.weekday.into(),
            scheduler_day: read.today,
            boundary_secs: read.next_day_at,
            generation: read.generation,
        }
    }
}

/// The heatmap of the whole collection (`deck_id` nil) or of one deck and
/// its subdecks by home deck. Reads only, on the query reader. A deck id
/// that names no deck gives zeros (AH4).
#[uniffi::export]
pub fn activity_heatmap(deck_id: Option<i64>) -> Result<ActivityCounts, CoreError> {
    callback::enter("activity_heatmap");
    let host = runtime::host()?;
    let collection = host.collection()?;
    Ok(meguri_engine::activity::activity_heatmap(&collection, deck_id.map(DeckId))?.into())
}

/// The same scoped read for Mac Home, including sparse older history.
#[uniffi::export]
pub fn activity_heatmap_history(deck_id: Option<i64>) -> Result<ActivityHistoryCounts, CoreError> {
    callback::enter("activity_heatmap_history");
    let host = runtime::host()?;
    let collection = host.collection()?;
    let read = meguri_engine::activity::activity_heatmap_history(&collection, deck_id.map(DeckId))?;
    Ok(ActivityHistoryCounts {
        window: read.window.into(),
        history: read.history.into_iter().map(|(days_ago, count)| ActivityDayCount { days_ago, count }).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_with_no_collection_open_is_unavailable() {
        assert!(matches!(activity_heatmap(None), Err(CoreError::Unavailable { .. })));
        assert!(matches!(activity_heatmap_history(None), Err(CoreError::Unavailable { .. })));
    }
}
