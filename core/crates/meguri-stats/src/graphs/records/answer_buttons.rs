//! The `buttons` record (`stats/graphs-revlog` V1, V6): presses of buttons
//! 1 to 4 per group, over Hourly Breakdown's ranges (V5.2).

use meguri_model::revlog::RevlogKind;

use super::hourly::Ranges;
use super::reviews::MATURE_FROM;
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;
use crate::graphs::scope::History;

/// Presses of buttons 1 to 4, index 0 for button 1.
pub type Presses = [u64; 4];

/// One range's three groups (V6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonCounts {
    pub learning: Presses,
    pub young: Presses,
    pub mature: Presses,
}

/// `stats/graphs-revlog` V6.2: one entry per range of V5.2, `all_time`
/// only with `allHistory`. The round 11 `six_months` range is U288's.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct AnswerButtonsRecord {
    pub one_month: ButtonCounts,
    pub three_months: ButtonCounts,
    pub one_year: ButtonCounts,
    pub all_time: Option<ButtonCounts>,
}

/// V6.1's groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Learning,
    Young,
    Mature,
}

impl ButtonCounts {
    fn press(&mut self, group: Group, button: usize) {
        let presses = match group {
            Group::Learning => &mut self.learning,
            Group::Young => &mut self.young,
            Group::Mature => &mut self.mature,
        };
        presses[button] += 1;
    }
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<AnswerButtonsRecord, NotBuilt> {
    let ranges = Ranges::new(input);
    let mut counts = [ButtonCounts::default(); 4];
    for scoped in input.scope.rows() {
        let row = scoped.row;
        // Only buttons 1 to 4; `ease` 0 and 5 and above are left out.
        if !(1..=4).contains(&row.ease) {
            continue;
        }
        let group = match row.kind {
            RevlogKind::Learning | RevlogKind::Relearning | RevlogKind::Filtered => Group::Learning,
            RevlogKind::Review if row.last_ivl < MATURE_FROM => Group::Young,
            RevlogKind::Review => Group::Mature,
            RevlogKind::Manual | RevlogKind::Rescheduled => continue,
        };
        let button = usize::from(row.ease - 1);
        for range in ranges.holding(row.id.div_euclid(1000)) {
            counts[range].press(group, button);
        }
    }
    let [one_month, three_months, one_year, all_time] = counts;
    Ok(AnswerButtonsRecord {
        one_month,
        three_months,
        one_year,
        all_time: (input.history == History::AllHistory).then_some(all_time),
    })
}

impl GraphRecord for AnswerButtonsRecord {
    fn graph(&self) -> Graph {
        Graph::AnswerButtons
    }

    /// A list of the ranges present (three, or four with `all_time`), each
    /// the learning, young and mature presses, each a list of four counts.
    fn encode(&self, out: &mut Encoder) {
        let present: Vec<ButtonCounts> = [self.one_month, self.three_months, self.one_year]
            .into_iter()
            .chain(self.all_time)
            .collect();
        out.list(&present, |out, range| {
            for presses in [range.learning, range.young, range.mature] {
                out.list(&presses, |out, count| out.integer(*count as i64));
            }
        });
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
