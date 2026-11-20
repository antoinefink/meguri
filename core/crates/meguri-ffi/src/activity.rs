//! The activity heatmap's data over FFI (`stats/activity-heatmap` AH6): one
//! short read per scope, called on the host's query lane.

use meguri_model::ids::DeckId;

use crate::{callback, error::CoreError, runtime};

/// A calendar date (proleptic Gregorian) and its weekday, 0 Sunday to 6
/// Saturday.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActivityDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub weekday: u8,
}

/// AH6: the counts of days 0 (today) to 370 for one scope.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActivityHeatmap {
    pub generation: u64,
    /// The scheduler day number the counts were taken for.
    pub today: u32,
    /// `N`, epoch seconds.
    pub next_day_at: i64,
    /// `T0`, the date of day 0.
    pub today_date: ActivityDate,
    /// Reviews per day; index = days ago.
    pub counts: Vec<u32>,
}

impl From<meguri_engine::activity::ActivityHeatmap> for ActivityHeatmap {
    fn from(value: meguri_engine::activity::ActivityHeatmap) -> Self {
        let date = value.today_date;
        Self {
            generation: value.generation,
            today: value.today,
            next_day_at: value.next_day_at,
            today_date: ActivityDate {
                year: date.year,
                month: date.month,
                day: date.day,
                weekday: date.weekday,
            },
            counts: value.counts,
        }
    }
}

/// The heatmap of the whole collection (`deck_id` nil) or of one deck and
/// its subdecks by home deck. Reads only, on the query reader. A deck id
/// that names no deck gives zeros (AH4).
#[uniffi::export]
pub fn activity_heatmap(deck_id: Option<i64>) -> Result<ActivityHeatmap, CoreError> {
    callback::enter("activity_heatmap");
    let host = runtime::host()?;
    let collection = host.collection()?;
    Ok(meguri_engine::activity::activity_heatmap(&collection, deck_id.map(DeckId))?.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_with_no_collection_open_is_unavailable() {
        assert!(matches!(activity_heatmap(None), Err(CoreError::Unavailable { .. })));
    }
}
