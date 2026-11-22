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
use crate::graphs::ScopeData;

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

/// H6 over the scope's window rows.
pub(crate) fn build(scope: &ScopeData, next_day_at: i64) -> ByAnswerPart {
    let mut part = ByAnswerPart::default();
    for scoped in scope.rows() {
        let row = scoped.row;
        if !counted(&row) {
            continue;
        }
        match row.ease {
            1..=4 => {
                let day = day_index(row.id, next_day_at);
                let answer = usize::from(row.ease - 1);
                part.count_by_answer.entry(day).or_default()[answer] += 1;
                part.time_by_answer.entry(day).or_default()[answer] += i64::from(row.time);
            }
            _ => part.unrated += 1,
        }
    }
    part
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
