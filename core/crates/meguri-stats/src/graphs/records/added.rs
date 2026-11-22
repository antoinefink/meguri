//! The `added` record (`stats/graphs-cards` C10, C11): the scope's cards by
//! the day they were created, counted in fixed 86,400-second spans back from
//! `N`. Ranges, the cumulative series and the bar queries are the
//! presentation layer's.
//!
//! Encoding (G6): the day map (one integer member: cards).

use std::collections::BTreeMap;

use super::card_rules::{tally, write_counts};
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-cards` C10.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct AddedRecord {
    /// The `added` member: creation day a → cards.
    pub by_day: BTreeMap<i32, u32>,
}

/// C10: the day of a card created at second `created`: the ceiling of the
/// spans from `N`, in binary64, saturating into `i32`. Day 0 holds
/// (`N` − 86,400, `N`].
fn creation_day(created: i64, next_day_at: i64) -> i32 {
    ((created as f64 - next_day_at as f64) / 86_400.0).ceil() as i32
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<AddedRecord, NotBuilt> {
    let next_day_at = input.context.next_day_at;
    let mut by_day = BTreeMap::new();
    for card in input.scope.cards() {
        // The id's seconds, truncated toward zero.
        tally(&mut by_day, creation_day(card.id / 1000, next_day_at));
    }
    Ok(AddedRecord { by_day })
}

impl GraphRecord for AddedRecord {
    fn graph(&self) -> Graph {
        Graph::Added
    }

    fn encode(&self, out: &mut Encoder) {
        write_counts(out, &self.by_day);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c10_day_edges() {
        let n = 1_791_856_800;
        for (id, day) in [
            (1_791_856_800_000i64, 0),
            (1_791_770_401_000, 0),
            (1_791_770_400_999, -1),
            (1_791_770_400_000, -1),
            (1_791_856_801_000, 1),
            (1_757_296_800_000, -400),
            (1_791_856_799_999, 0),
            (-1, -20_739),
            (-1_500, -20_739),
        ] {
            assert_eq!(creation_day(id / 1000, n), day, "{id}");
        }
        assert_eq!(creation_day(i64::MAX / 1000, i64::MIN), i32::MAX);
    }
}
