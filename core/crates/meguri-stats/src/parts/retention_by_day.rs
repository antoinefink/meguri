//! Retention and Forgotten per day (`stats/history-reads` H4), with the
//! desired-retention weights of the Retention chart's dashed line (H3.4).

use std::collections::BTreeMap;

use super::decks::Retentions;
use super::page_day;
use crate::graphs::records::retention::outcome;
use crate::graphs::ScopeData;
use crate::graphs::rows::StatsRow;

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

/// H4's counts, fed the window's rows in id order; H3.4's retentions from
/// the scope's cards.
pub(crate) struct RetentionCounter<'d> {
    next_day_at: i64,
    decks: &'d Retentions,
    part: RetentionByDayPart,
    open: Option<(i32, RetentionDay)>,
}

impl<'d> RetentionCounter<'d> {
    pub(crate) fn new(scope: &ScopeData, next_day_at: i64, decks: &'d Retentions) -> RetentionCounter<'d> {
        let mut distinct: Vec<u32> = Vec::new();
        for card in scope.cards() {
            if let Some(retention) = decks.of(card.did, card.odid)
                && !distinct.contains(&retention.to_bits())
            {
                distinct.push(retention.to_bits());
            }
        }
        let mut retentions: Vec<f32> = distinct.into_iter().map(f32::from_bits).collect();
        retentions.sort_by(f32::total_cmp);
        RetentionCounter {
            next_day_at,
            decks,
            part: RetentionByDayPart {
                several: retentions.len() > 1,
                retentions,
                days: BTreeMap::new(),
            },
            open: None,
        }
    }

    /// A row of the window, with its card's `did` and `odid` (none for a
    /// deleted card's row).
    pub(crate) fn add(&mut self, row: &StatsRow, card_decks: Option<(i64, i64)>) {
        let Some(counted) = outcome(row) else {
            return;
        };
        let at = page_day(row.id, self.next_day_at);
        if self.open.is_some_and(|(open, _)| open != at) {
            self.store();
        }
        let (_, day) = self.open.get_or_insert((at, RetentionDay::default()));
        let count = match (counted.mature, counted.passed) {
            (false, true) => &mut day.young_passed,
            (false, false) => &mut day.young_failed,
            (true, true) => &mut day.mature_passed,
            (true, false) => &mut day.mature_failed,
        };
        *count += 1;
        if let Some(retention) = card_decks.and_then(|(did, odid)| self.decks.of(did, odid)) {
            day.desired_sum += f64::from(retention);
            day.desired_rows += 1;
        }
    }

    fn store(&mut self) {
        if let Some((at, day)) = self.open.take() {
            // Days are monotonic in id, so a day is stored once.
            self.part.days.insert(at, day);
        }
    }

    pub(crate) fn finish(mut self) -> RetentionByDayPart {
        self.store();
        self.part
    }
}
