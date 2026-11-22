//! The `reviews` record (`stats/graphs-revlog` V1, V2): per day index, how
//! many rows of each of five kinds were answered and how long they took.
//! Round 11's By Answer (`stats/history-reads` H6) reads the same rows by
//! the answer given as a part of its own (`crate::parts::by_answer`), so
//! this record holds Anki's members only (G6's size limit).

use std::collections::BTreeMap;

use meguri_model::revlog::RevlogKind;

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::{Encoder, Member};
use crate::graphs::records::Graph;
use crate::graphs::rows::StatsRow;

/// Seconds in one of B3's fixed day spans.
const DAY_SECS: i64 = 86_400;

/// Below this `lastIvl` a Review row is young, negative values included
/// (V1).
pub(crate) const MATURE_FROM: i32 = 21;

/// The five kinds of one day (R1385), as counts or as summed milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReviewKinds {
    pub learn: i64,
    pub relearn: i64,
    pub young: i64,
    pub mature: i64,
    pub filtered: i64,
}

impl ReviewKinds {
    /// The five values in the order of the encoding.
    pub fn values(&self) -> [i64; 5] {
        [self.learn, self.relearn, self.young, self.mature, self.filtered]
    }

    fn slot(&mut self, column: Column) -> &mut i64 {
        match column {
            Column::Learn => &mut self.learn,
            Column::Relearn => &mut self.relearn,
            Column::Young => &mut self.young,
            Column::Mature => &mut self.mature,
            Column::Filtered => &mut self.filtered,
        }
    }

    fn get(mut self, column: Column) -> i64 {
        *self.slot(column)
    }

    fn bump(&mut self, column: Column, amount: i64) {
        *self.slot(column) += amount;
    }
}

/// `stats/graphs-revlog` V2.2: two sparse maps, day index to the five
/// kinds, with exactly the same keys: the days on which a row counted.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ReviewsRecord {
    pub count: BTreeMap<i32, ReviewKinds>,
    /// Stored `time` values in milliseconds, summed in 64 bits with no wrap
    /// (V9 `stats-review-time-no-wrap`).
    pub time: BTreeMap<i32, ReviewKinds>,
}

/// What one day's rows add up to while the day is open.
#[derive(Clone, Copy, Debug, Default)]
struct DayTotals {
    count: ReviewKinds,
    time: ReviewKinds,
}

impl DayTotals {
    fn add(&mut self, column: Column, row: &StatsRow) {
        self.count.bump(column, 1);
        self.time.bump(column, i64::from(row.time));
    }

    fn store(self, day: i32, record: &mut ReviewsRecord) {
        let count = record.count.entry(day).or_default();
        let time = record.time.entry(day).or_default();
        for column in Column::ALL {
            count.bump(column, self.count.get(column));
            time.bump(column, self.time.get(column));
        }
    }
}

/// Which of the five kinds a row adds to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Learn,
    Relearn,
    Young,
    Mature,
    Filtered,
}

impl Column {
    const ALL: [Column; 5] = [Column::Learn, Column::Relearn, Column::Young, Column::Mature, Column::Filtered];
}

/// V1: Manual and Rescheduled rows count nowhere in Reviews; every `ease`
/// counts, 0 included.
fn column_of(row: &StatsRow) -> Option<Column> {
    Some(match row.kind {
        RevlogKind::Learning => Column::Learn,
        RevlogKind::Relearning => Column::Relearn,
        RevlogKind::Filtered => Column::Filtered,
        RevlogKind::Review if row.last_ivl < MATURE_FROM => Column::Young,
        RevlogKind::Review => Column::Mature,
        RevlogKind::Manual | RevlogKind::Rescheduled => return None,
    })
}

/// Whether V2 counts the row (V1): every kind but Manual and Rescheduled,
/// whatever its `ease`.
pub(crate) fn counted(row: &StatsRow) -> bool {
    column_of(row).is_some()
}

/// `stats/day-buckets` B3 with V2.1's 32-bit key: `(t − N) / 86,400`
/// rounded toward zero, `t` the row's whole second, then its low 32 bits
/// read as a signed integer.
pub(crate) fn day_index(id_ms: i64, next_day_at: i64) -> i32 {
    let seconds = id_ms.div_euclid(1000);
    let days = seconds.wrapping_sub(next_day_at) / DAY_SECS;
    days as i32
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<ReviewsRecord, NotBuilt> {
    let next_day_at = input.context.next_day_at;
    let mut record = ReviewsRecord::default();
    // Rows come in id order, so a day's rows are consecutive but for the
    // rare 32-bit cut: keep the current day open and store it when the key
    // changes.
    let mut open: Option<(i32, DayTotals)> = None;
    for scoped in input.scope.rows() {
        let row = scoped.row;
        let Some(column) = column_of(&row) else {
            continue;
        };
        let day = day_index(row.id, next_day_at);
        match &open {
            Some((current, _)) if *current == day => {}
            _ => {
                if let Some((done, totals)) = open.take() {
                    totals.store(done, &mut record);
                }
                open = Some((day, DayTotals::default()));
            }
        }
        let (_, totals) = open.as_mut().expect("opened above");
        totals.add(column, &row);
    }
    if let Some((done, totals)) = open {
        totals.store(done, &mut record);
    }
    Ok(record)
}

impl GraphRecord for ReviewsRecord {
    fn graph(&self) -> Graph {
        Graph::Reviews
    }

    /// One sparse map over the shared day keys with ten members: the five
    /// counts, then the five times, each in [`ReviewKinds::values`] order.
    fn encode(&self, out: &mut Encoder) {
        debug_assert!(self.count.keys().eq(self.time.keys()), "count and time have the same days (V2.2)");
        let keys: Vec<i64> = self.count.keys().map(|day| i64::from(*day)).collect();
        let column = |map: &BTreeMap<i32, ReviewKinds>, at: usize| -> Vec<i64> {
            map.values().map(|kinds| kinds.values()[at]).collect()
        };
        let columns: Vec<Vec<i64>> = (0..5)
            .map(|at| column(&self.count, at))
            .chain((0..5).map(|at| column(&self.time, at)))
            .collect();
        let members: Vec<Member<'_>> = columns.iter().map(|values| Member::Integers(values)).collect();
        out.sparse_map(&keys, &members);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
