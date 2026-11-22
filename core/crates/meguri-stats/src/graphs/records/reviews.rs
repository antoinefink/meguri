//! The `reviews` record (`stats/graphs-revlog` V1, V2): per day index, how
//! many rows of each of five kinds were answered and how long they took,
//! and, for round 11's By Answer (`stats/history-reads` H6), the same rows
//! by the answer given.

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

/// One day's rows by answer, Again, Hard, Good and Easy (index 0 is
/// Again), as counts or as summed milliseconds.
pub type Answers = [i64; 4];

/// `stats/graphs-revlog` V2.2: two sparse maps, day index to the five
/// kinds, with exactly the same keys: the days on which a row counted.
/// Then the Meguri extras of `stats/history-reads` H6, which are not
/// observed and change none of the values above.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ReviewsRecord {
    pub count: BTreeMap<i32, ReviewKinds>,
    /// Stored `time` values in milliseconds, summed in 64 bits with no wrap
    /// (V9 `stats-review-time-no-wrap`).
    pub time: BTreeMap<i32, ReviewKinds>,
    /// H6: the rows counted above that have `ease` 1 to 4, by answer, on
    /// V2's day index. The keys are the days on which such a row counted.
    pub count_by_answer: BTreeMap<i32, Answers>,
    /// H6: their summed `time`, in 64 bits as `time`; the same keys.
    pub time_by_answer: BTreeMap<i32, Answers>,
    /// H6: the rows counted above with another `ease` (0, or 5 to 255) in
    /// the window: in By Type, not in By Answer.
    pub unrated: u64,
}

/// What one day's rows add up to while the day is open.
#[derive(Clone, Copy, Debug, Default)]
struct DayTotals {
    count: ReviewKinds,
    time: ReviewKinds,
    answers: Answers,
    answer_time: Answers,
    /// Some row of the day had `ease` 1 to 4.
    rated: bool,
}

impl DayTotals {
    fn add(&mut self, column: Column, row: &StatsRow) -> bool {
        let time = i64::from(row.time);
        self.count.bump(column, 1);
        self.time.bump(column, time);
        match row.ease {
            1..=4 => {
                let answer = usize::from(row.ease - 1);
                self.answers[answer] += 1;
                self.answer_time[answer] += time;
                self.rated = true;
                true
            }
            _ => false,
        }
    }

    fn store(self, day: i32, record: &mut ReviewsRecord) {
        let count = record.count.entry(day).or_default();
        let time = record.time.entry(day).or_default();
        for column in Column::ALL {
            count.bump(column, self.count.get(column));
            time.bump(column, self.time.get(column));
        }
        if self.rated {
            let answers = record.count_by_answer.entry(day).or_default();
            let answer_time = record.time_by_answer.entry(day).or_default();
            for at in 0..4 {
                answers[at] += self.answers[at];
                answer_time[at] += self.answer_time[at];
            }
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
        if !totals.add(column, &row) {
            record.unrated += 1;
        }
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
    /// Then H6's extras: a sparse map over the By Answer day keys with
    /// eight members, the four counts (Again, Hard, Good, Easy), then the
    /// four times in the same order; then `unrated`.
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
        debug_assert!(
            self.count_by_answer.keys().eq(self.time_by_answer.keys()),
            "By Answer's counts and times have the same days (H6)"
        );
        let keys: Vec<i64> = self.count_by_answer.keys().map(|day| i64::from(*day)).collect();
        let answer = |map: &BTreeMap<i32, Answers>, at: usize| -> Vec<i64> { map.values().map(|day| day[at]).collect() };
        let columns: Vec<Vec<i64>> = (0..4)
            .map(|at| answer(&self.count_by_answer, at))
            .chain((0..4).map(|at| answer(&self.time_by_answer, at)))
            .collect();
        let members: Vec<Member<'_>> = columns.iter().map(|values| Member::Integers(values)).collect();
        out.sparse_map(&keys, &members);
        out.integer(self.unrated as i64);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
