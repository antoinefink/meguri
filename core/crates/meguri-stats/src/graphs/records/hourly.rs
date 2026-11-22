//! The `hours` record (`stats/graphs-revlog` V1, V5): for each range, the
//! rows answered in each local hour and how many of them were correct.
//! The ranges of V5.2 are also Answer Buttons' (V6.2). Round 11 adds
//! `stats/history-reads` H7: the `six_months` range, and per range and hour
//! the rows the retention rule (V4.1) passes and fails.

use meguri_model::revlog::RevlogKind;

use super::retention::outcome;

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;
use crate::graphs::scope::History;

const DAY_SECS: i64 = 86_400;

/// The bounded ranges, by their length in days: V5.2's `one_month`,
/// `three_months` and `one_year`, with H7's `six_months` (Meguri's) in
/// `stats/history-reads` H1's order. Each starts that many days before `N`
/// and has no upper end.
const RANGE_DAYS: [i64; 4] = [30, 90, 182, 365];

/// The index of `all_time` in [`Ranges::holding`].
pub(crate) const ALL_TIME: usize = 4;

/// Which ranges hold a row: the four bounded ones by their start, and
/// `all_time`, which holds every row read, kept only with `allHistory`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ranges {
    starts: [i64; 4],
    all_time: bool,
}

impl Ranges {
    pub(crate) fn new(input: &GraphInput<'_>) -> Ranges {
        let n = input.context.next_day_at;
        Ranges {
            starts: RANGE_DAYS.map(|days| n - days * DAY_SECS),
            all_time: input.history == History::AllHistory,
        }
    }

    /// The ranges that hold a row at whole second `t`, as indices: 0 to 3
    /// `one_month`, `three_months`, `six_months` and `one_year`, then
    /// [`ALL_TIME`].
    pub(crate) fn holding(&self, t: i64) -> impl Iterator<Item = usize> + '_ {
        (0..4)
            .filter(move |at| t >= self.starts[*at])
            .chain(self.all_time.then_some(ALL_TIME))
    }
}

/// One hour of one range (V5.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HourCounts {
    pub total: u64,
    pub correct: u64,
}

/// Hours 0 to 23 of one range.
pub type Hours = [HourCounts; 24];

/// The rows of one hour that the retention rule (V4.1) counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassFail {
    pub passed: u64,
    pub failed: u64,
}

/// Hours 0 to 23 of one range, by the retention rule.
pub type RetentionHours = [PassFail; 24];

/// H7's `retention_by_hour`: the five ranges; `all_time` only with
/// `allHistory`.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct RetentionByHour {
    pub one_month: RetentionHours,
    pub three_months: RetentionHours,
    pub six_months: RetentionHours,
    pub one_year: RetentionHours,
    pub all_time: Option<RetentionHours>,
}

/// `stats/graphs-revlog` V5.3: 24 hours per range; `all_time` only with
/// `allHistory`. Then the Meguri extras of `stats/history-reads` H7, not
/// observed: `six_months` (whatever the history) and `retention_by_hour`.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct HourlyRecord {
    pub one_month: Hours,
    pub three_months: Hours,
    pub one_year: Hours,
    pub all_time: Option<Hours>,
    pub six_months: Hours,
    pub retention_by_hour: RetentionByHour,
}

/// V5.1: the hour at the zone's offset now, for every row alike; a row
/// whose shifted second is before the epoch is in hour 0 (V9
/// `stats-hourly-epoch-rows`).
pub(crate) fn hour_of(t: i64, offset_secs: i32) -> usize {
    let local = t + i64::from(offset_secs);
    if local < 0 {
        return 0;
    }
    ((local / 3_600) % 24) as usize
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<HourlyRecord, NotBuilt> {
    let ranges = Ranges::new(input);
    let offset = input.context.utc_offset_secs;
    let mut ranges_hours = [[HourCounts::default(); 24]; 5];
    let mut ranges_kept = [[PassFail::default(); 24]; 5];
    for scoped in input.scope.rows() {
        let row = scoped.row;
        // V1: only Learn, Review and Relearn rows, whatever their `ease`.
        let answered = matches!(row.kind, RevlogKind::Learning | RevlogKind::Review | RevlogKind::Relearning);
        // H7: the retention rule's rows, of any kind.
        let kept = outcome(&row);
        if !answered && kept.is_none() {
            continue;
        }
        let t = row.id.div_euclid(1000);
        let hour = hour_of(t, offset);
        for range in ranges.holding(t) {
            if answered {
                let cell = &mut ranges_hours[range][hour];
                cell.total += 1;
                cell.correct += u64::from(row.ease > 1);
            }
            if let Some(kept) = kept {
                let cell = &mut ranges_kept[range][hour];
                if kept.passed {
                    cell.passed += 1;
                } else {
                    cell.failed += 1;
                }
            }
        }
    }
    let [one_month, three_months, six_months, one_year, all_time] = ranges_hours;
    let [kept_month, kept_three, kept_six, kept_year, kept_all] = ranges_kept;
    Ok(HourlyRecord {
        one_month,
        three_months,
        one_year,
        all_time: ranges.all_time.then_some(all_time),
        six_months,
        retention_by_hour: RetentionByHour {
            one_month: kept_month,
            three_months: kept_three,
            six_months: kept_six,
            one_year: kept_year,
            all_time: ranges.all_time.then_some(kept_all),
        },
    })
}

impl GraphRecord for HourlyRecord {
    fn graph(&self) -> Graph {
        Graph::Hourly
    }

    /// A list of V5.2's ranges present (three, or four with `all_time`),
    /// each a list of 24 hours, each its total then its correct count.
    /// Then H7's extras: `six_months` as one such list of 24 hours; then
    /// `retention_by_hour` as a list of its ranges present in H1's order
    /// (`one_month`, `three_months`, `six_months`, `one_year`, and
    /// `all_time` when present), each a list of 24 hours, each its passed
    /// then its failed count.
    fn encode(&self, out: &mut Encoder) {
        let write_hours = |out: &mut Encoder, hours: &&Hours| {
            out.list(&hours[..], |out, hour| {
                out.integer(hour.total as i64);
                out.integer(hour.correct as i64);
            });
        };
        let present: Vec<&Hours> = [&self.one_month, &self.three_months, &self.one_year]
            .into_iter()
            .chain(self.all_time.as_ref())
            .collect();
        out.list(&present, write_hours);
        write_hours(out, &&self.six_months);
        let kept = &self.retention_by_hour;
        let present: Vec<&RetentionHours> = [&kept.one_month, &kept.three_months, &kept.six_months, &kept.one_year]
            .into_iter()
            .chain(kept.all_time.as_ref())
            .collect();
        out.list(&present, |out, hours| {
            out.list(&hours[..], |out, hour| {
                out.integer(hour.passed as i64);
                out.integer(hour.failed as i64);
            });
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
