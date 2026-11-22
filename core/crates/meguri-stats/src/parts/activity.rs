//! Activity, every year (`stats/history-reads` H11), and the oldest day of
//! the scope's review log (H9): both read the scope's rows of all history
//! (`id` ≥ 0) whatever the request's window.

use std::collections::BTreeMap;

use super::page_day;
use crate::graphs::ScopeData;

/// Part `activity_days`. Member order: `days`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivityDaysPart {
    /// Sparse: H1 day (folded at 0) → the number of the scope's rows of
    /// `stats/activity-heatmap` AH2 (`ease` ≥ 1, any `type`) on it, from the
    /// oldest such row with `id` ≥ 0 to today; days with a count only.
    pub days: BTreeMap<i32, u64>,
}

/// H11 over `all`, the scope's rows with `id` ≥ 0.
pub(crate) fn build(all: &ScopeData, next_day_at: i64) -> ActivityDaysPart {
    let mut part = ActivityDaysPart::default();
    for scoped in all.rows() {
        if scoped.row.ease >= 1 {
            *part.days.entry(page_day(scoped.row.id, next_day_at)).or_default() += 1;
        }
    }
    part
}

/// H9: the day of the scope's first row with `id` ≥ 0 (the rows come in
/// `id` order, and a later id is never on an earlier day), or none.
pub(crate) fn oldest_day(all: &ScopeData, next_day_at: i64) -> Option<i32> {
    all.rows().next().map(|scoped| page_day(scoped.row.id, next_day_at))
}
