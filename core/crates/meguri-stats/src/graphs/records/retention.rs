//! The `true_retention` record (`stats/graphs-revlog` V1, V4): pass and
//! fail counts of rated rows, young and mature, over six fixed periods
//! that end at `N`.

use meguri_model::revlog::RevlogKind;

use super::reviews::MATURE_FROM;
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;
use crate::graphs::rows::StatsRow;
use crate::graphs::scope::History;

const DAY_SECS: i64 = 86_400;

/// A long last interval (V1): a day or more, in days or in seconds.
pub(crate) fn long_last_interval(last_ivl: i32) -> bool {
    last_ivl >= 1 || last_ivl <= -86_400
}

/// The four counts of one period (V4.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetentionCounts {
    pub young_passed: u64,
    pub young_failed: u64,
    pub mature_passed: u64,
    pub mature_failed: u64,
}

impl RetentionCounts {
    /// The four counts in the order of the encoding.
    pub fn values(&self) -> [u64; 4] {
        [self.young_passed, self.young_failed, self.mature_passed, self.mature_failed]
    }
}

/// `stats/graphs-revlog` V4.3: one entry per period of V4.2. `all_time` is
/// present only with `allHistory` (G3, AE59). The By Day part of round 11
/// is U288's.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct RetentionRecord {
    pub today: RetentionCounts,
    pub yesterday: RetentionCounts,
    pub week: RetentionCounts,
    pub month: RetentionCounts,
    pub year: RetentionCounts,
    pub all_time: Option<RetentionCounts>,
}

/// What a counted row adds: its group and its outcome (V4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Outcome {
    mature: bool,
    passed: bool,
}

/// V4.1: a rating, not a cram row, and Review or a long last interval.
fn outcome(row: &StatsRow) -> Option<Outcome> {
    let cram = row.kind == RevlogKind::Filtered && row.factor == 0;
    let eligible = row.kind == RevlogKind::Review || long_last_interval(row.last_ivl);
    (row.ease >= 1 && !cram && eligible).then_some(Outcome {
        mature: row.last_ivl >= MATURE_FROM,
        passed: row.ease != 1,
    })
}

impl RetentionCounts {
    fn add(&mut self, outcome: Outcome) {
        let count = match (outcome.mature, outcome.passed) {
            (false, true) => &mut self.young_passed,
            (false, false) => &mut self.young_failed,
            (true, true) => &mut self.mature_passed,
            (true, false) => &mut self.mature_failed,
        };
        *count += 1;
    }
}

/// V4.2's spans, as (days back of the start, days back of the end) from
/// `N`; `all_time` starts at the epoch instead.
const SPANS: [(i64, i64); 5] = [(1, 0), (2, 1), (7, 0), (30, 0), (365, 0)];

pub(crate) fn build(input: &GraphInput<'_>) -> Result<RetentionRecord, NotBuilt> {
    let n = input.context.next_day_at;
    let bounds = SPANS.map(|(from, to)| (n - from * DAY_SECS, n - to * DAY_SECS));
    let mut spans = [RetentionCounts::default(); 5];
    let mut all_time = RetentionCounts::default();
    for scoped in input.scope.rows() {
        let row = scoped.row;
        let Some(outcome) = outcome(&row) else {
            continue;
        };
        let t = row.id.div_euclid(1000);
        if (0..n).contains(&t) {
            all_time.add(outcome);
        }
        for (counts, (start, end)) in spans.iter_mut().zip(bounds) {
            if (start..end).contains(&t) {
                counts.add(outcome);
            }
        }
    }
    let [today, yesterday, week, month, year] = spans;
    Ok(RetentionRecord {
        today,
        yesterday,
        week,
        month,
        year,
        all_time: (input.history == History::AllHistory).then_some(all_time),
    })
}

impl GraphRecord for RetentionRecord {
    fn graph(&self) -> Graph {
        Graph::Retention
    }

    /// A list of the periods present, in V4.2's order (five, or six with
    /// `all_time`), each its four counts in [`RetentionCounts::values`]
    /// order.
    fn encode(&self, out: &mut Encoder) {
        let periods: Vec<RetentionCounts> = [self.today, self.yesterday, self.week, self.month, self.year]
            .into_iter()
            .chain(self.all_time)
            .collect();
        out.list(&periods, |out, counts| {
            for value in counts.values() {
                out.integer(value as i64);
            }
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
