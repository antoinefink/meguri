//! The `hours` record (`stats/graphs-revlog` V1, V5): for each range, the
//! rows answered in each local hour and how many of them were correct.
//! The ranges of V5.2 are also Answer Buttons' (V6.2).

use meguri_model::revlog::RevlogKind;

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;
use crate::graphs::scope::History;

const DAY_SECS: i64 = 86_400;

/// V5.2's bounded ranges, by their length in days: `one_month`,
/// `three_months`, `one_year`. Each starts that many days before `N` and
/// has no upper end.
const RANGE_DAYS: [i64; 3] = [30, 90, 365];

/// Which of V5.2's ranges hold a row: the three bounded ones by their
/// start, and `all_time`, which holds every row read, kept only with
/// `allHistory`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ranges {
    starts: [i64; 3],
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

    /// The ranges that hold a row at whole second `t`, as indices: 0 to 2
    /// the bounded ranges in order, 3 `all_time`.
    pub(crate) fn holding(&self, t: i64) -> impl Iterator<Item = usize> + '_ {
        (0..3)
            .filter(move |at| t >= self.starts[*at])
            .chain(self.all_time.then_some(3))
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

/// `stats/graphs-revlog` V5.3: 24 hours per range; `all_time` only with
/// `allHistory`. The round 11 extras of H7 are U288's.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct HourlyRecord {
    pub one_month: Hours,
    pub three_months: Hours,
    pub one_year: Hours,
    pub all_time: Option<Hours>,
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
    let mut ranges_hours = [[HourCounts::default(); 24]; 4];
    for scoped in input.scope.rows() {
        let row = scoped.row;
        // V1: only Learn, Review and Relearn rows, whatever their `ease`.
        if !matches!(row.kind, RevlogKind::Learning | RevlogKind::Review | RevlogKind::Relearning) {
            continue;
        }
        let t = row.id.div_euclid(1000);
        let hour = hour_of(t, offset);
        for range in ranges.holding(t) {
            let cell = &mut ranges_hours[range][hour];
            cell.total += 1;
            cell.correct += u64::from(row.ease > 1);
        }
    }
    let [one_month, three_months, one_year, all_time] = ranges_hours;
    Ok(HourlyRecord {
        one_month,
        three_months,
        one_year,
        all_time: ranges.all_time.then_some(all_time),
    })
}

impl GraphRecord for HourlyRecord {
    fn graph(&self) -> Graph {
        Graph::Hourly
    }

    /// A list of the ranges present (three, or four with `all_time`), each
    /// a list of 24 hours, each its total then its correct count.
    fn encode(&self, out: &mut Encoder) {
        let present: Vec<&Hours> = [&self.one_month, &self.three_months, &self.one_year]
            .into_iter()
            .chain(self.all_time.as_ref())
            .collect();
        out.list(&present, |out, hours| {
            out.list(&hours[..], |out, hour| {
                out.integer(hour.total as i64);
                out.integer(hour.correct as i64);
            });
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
