//! Activity, every year (`stats/history-reads` H11), and the oldest day of
//! the scope's review log (H9): both read the scope's rows of all history
//! (`id` ≥ 0) whatever the request's window.

use std::collections::BTreeMap;

use super::page_day;
use crate::graphs::ScopeData;
use crate::graphs::rows::StatsRow;

/// Part `activity_days`. Member order: `days`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivityDaysPart {
    /// Sparse: H1 day (folded at 0) → the number of the scope's rows of
    /// `stats/activity-heatmap` AH2 (`ease` ≥ 1, any `type`) on it, from the
    /// oldest such row with `id` ≥ 0 to today; days with a count only.
    pub days: BTreeMap<i32, u64>,
}

/// H11's counts, fed the scope's rows with `id` ≥ 0 in id order (a later
/// id is never on an earlier day).
#[derive(Default)]
pub(crate) struct ActivityCounter {
    next_day_at: i64,
    part: ActivityDaysPart,
    open: Option<(i32, u64)>,
}

impl ActivityCounter {
    pub(crate) fn new(next_day_at: i64) -> ActivityCounter {
        ActivityCounter {
            next_day_at,
            ..ActivityCounter::default()
        }
    }

    pub(crate) fn add(&mut self, row: &StatsRow) {
        if row.ease == 0 {
            return;
        }
        let day = page_day(row.id, self.next_day_at);
        match &mut self.open {
            Some((open, count)) if *open == day => *count += 1,
            _ => {
                self.store();
                self.open = Some((day, 1));
            }
        }
    }

    fn store(&mut self) {
        if let Some((day, count)) = self.open.take() {
            *self.part.days.entry(day).or_default() += count;
        }
    }

    pub(crate) fn finish(mut self) -> ActivityDaysPart {
        self.store();
        self.part
    }
}

/// H9: the day of the scope's first row with `id` ≥ 0 (the rows come in
/// `id` order, and a later id is never on an earlier day), or none.
pub(crate) fn oldest_day(all: &ScopeData, next_day_at: i64) -> Option<i32> {
    all.rows().next().map(|scoped| page_day(scoped.row.id, next_day_at))
}
