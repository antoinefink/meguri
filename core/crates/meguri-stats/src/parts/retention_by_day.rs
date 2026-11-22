//! Retention and Forgotten per day (`stats/history-reads` H4), with the
//! desired-retention weights of the Retention chart's dashed line (H3.4).

use std::collections::{BTreeMap, HashMap};

use super::decks::DeckFacts;
use super::page_day;
use crate::graphs::records::retention::outcome;
use crate::graphs::ScopeData;

/// One day's counts (H4.2). Member order: `young_passed`, `young_failed`,
/// `mature_passed`, `mature_failed`, `desired_sum`, `desired_rows`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RetentionDay {
    pub young_passed: u64,
    pub young_failed: u64,
    pub mature_passed: u64,
    pub mature_failed: u64,
    /// The retention in force (`sched/presets` V7) of each counted row's
    /// card, summed in binary64 from its binary32 value.
    pub desired_sum: f64,
    /// The counted rows in `desired_sum`: those whose card exists and has
    /// a home deck with a retention in force (a deleted card's row, or a
    /// card whose home deck is missing or filtered, is in neither).
    pub desired_rows: u64,
}

impl RetentionDay {
    /// Forgotten on the day (H4.2).
    pub fn forgotten(&self) -> u64 {
        self.young_failed + self.mature_failed
    }
}

/// Part `retention_by_day`. Member order: `days`, `retentions`, `several`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RetentionByDayPart {
    /// Sparse: H1 day (folded at 0) → its counts, for the window's days
    /// with a row of H4.1.
    pub days: BTreeMap<i32, RetentionDay>,
    /// H3.4: the distinct retentions in force among the home decks of the
    /// scope's existing cards, ascending (by `f32::total_cmp`).
    pub retentions: Vec<f32>,
    /// More than one of them: the dashed line is an average.
    pub several: bool,
}

/// H4 over the scope's window rows; H3.4's retentions over its cards.
pub(crate) fn build(scope: &ScopeData, next_day_at: i64, decks: &DeckFacts) -> RetentionByDayPart {
    let mut in_force: HashMap<i64, f32> = HashMap::new();
    let mut distinct: Vec<u32> = Vec::new();
    for card in scope.cards() {
        if let Some(retention) = decks.retention_of(card.did, card.odid) {
            in_force.insert(card.id, retention);
            if !distinct.contains(&retention.to_bits()) {
                distinct.push(retention.to_bits());
            }
        }
    }
    let mut part = RetentionByDayPart::default();
    for scoped in scope.rows() {
        let Some(counted) = outcome(&scoped.row) else {
            continue;
        };
        let day = part.days.entry(page_day(scoped.row.id, next_day_at)).or_default();
        let count = match (counted.mature, counted.passed) {
            (false, true) => &mut day.young_passed,
            (false, false) => &mut day.young_failed,
            (true, true) => &mut day.mature_passed,
            (true, false) => &mut day.mature_failed,
        };
        *count += 1;
        if let Some(retention) = scoped.card.and_then(|card| in_force.get(&card)) {
            day.desired_sum += f64::from(*retention);
            day.desired_rows += 1;
        }
    }
    let mut retentions: Vec<f32> = distinct.into_iter().map(f32::from_bits).collect();
    retentions.sort_by(f32::total_cmp);
    part.several = retentions.len() > 1;
    part.retentions = retentions;
    part
}
