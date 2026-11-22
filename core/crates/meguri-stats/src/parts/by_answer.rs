//! By Answer (`stats/history-reads` H6; Reviews Over Time's By Answer
//! switch, `stats/statistics-page` S7.4): the rows the `reviews` record
//! counts (`stats/graphs-revlog` V2), by the answer given, on V2's day
//! index. A part of its own, beside the `reviews` record rather than in it,
//! so that record keeps Anki's members within `stats/graphs-common` G6's
//! size limit; computed in its own pass over the same rows, not observed
//! (V8).

use std::collections::BTreeMap;

use crate::graphs::encoding::{Encoder, Member};
use crate::graphs::records::reviews::{counted, day_index};
use crate::graphs::rows::StatsRow;

/// One day's rows by answer, Again, Hard, Good and Easy (index 0 is
/// Again), as counts or as summed milliseconds.
pub type Answers = [i64; 4];

/// Part `by_answer`. Member order: `count_by_answer`, `time_by_answer`,
/// `unrated` (see [`ByAnswerPart::encode`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ByAnswerPart {
    /// The rows V2 counts that have `ease` 1 to 4, per V2 day index (B3,
    /// unfolded, with V2.1's 32-bit key), by answer. The keys are the days
    /// on which such a row counted.
    pub count_by_answer: BTreeMap<i32, Answers>,
    /// Their summed `time` in milliseconds, in 64 bits with no wrap, as V2's
    /// time; the same keys.
    pub time_by_answer: BTreeMap<i32, Answers>,
    /// The rows V2 counts with another `ease` (0, or 5 to 255) in the
    /// window: in By Type, not in By Answer.
    pub unrated: u64,
}

/// H6's counts, fed the window's rows in id order.
pub(crate) struct ByAnswerCounter {
    next_day_at: i64,
    part: ByAnswerPart,
    /// The day being counted: rows come in id order, so a day's rows are
    /// consecutive but for V2.1's rare 32-bit cut.
    open: Option<(i32, Answers, Answers)>,
}

impl ByAnswerCounter {
    pub(crate) fn new(next_day_at: i64) -> ByAnswerCounter {
        ByAnswerCounter {
            next_day_at,
            part: ByAnswerPart::default(),
            open: None,
        }
    }

    pub(crate) fn add(&mut self, row: &StatsRow) {
        if !counted(row) {
            return;
        }
        let 1..=4 = row.ease else {
            self.part.unrated += 1;
            return;
        };
        let day = day_index(row.id, self.next_day_at);
        if self.open.is_some_and(|(open, _, _)| open != day) {
            self.store();
        }
        let (_, counts, times) = self.open.get_or_insert((day, [0; 4], [0; 4]));
        let answer = usize::from(row.ease - 1);
        counts[answer] += 1;
        times[answer] += i64::from(row.time);
    }

    fn store(&mut self) {
        if let Some((day, counts, times)) = self.open.take() {
            let stored = self.part.count_by_answer.entry(day).or_default();
            let stored_time = self.part.time_by_answer.entry(day).or_default();
            for at in 0..4 {
                stored[at] += counts[at];
                stored_time[at] += times[at];
            }
        }
    }

    pub(crate) fn finish(mut self) -> ByAnswerPart {
        self.store();
        self.part
    }
}

impl ByAnswerPart {
    /// G6's bytes: a sparse map over the By Answer day keys with eight
    /// integer members, the four counts (Again, Hard, Good, Easy), then the
    /// four times in the same order; then `unrated` as an integer.
    pub fn encode(&self, out: &mut Encoder) {
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
        out.integer(i64::try_from(self.unrated).unwrap_or(i64::MAX));
    }
}
