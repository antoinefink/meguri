//! The `intervals` record (`stats/graphs-cards` C5, C11): review and
//! relearning cards by interval, whatever their queue. Hiding interval 0,
//! the quantile ranges and the median are the presentation layer's.
//!
//! Encoding (G6): the interval map (one integer member: cards).

use std::collections::BTreeMap;

use meguri_model::card::CardType;

use super::card_rules::{tally, write_counts};
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-cards` C5.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct IntervalsRecord {
    /// The `intervals` member: `ivl` in days → cards, 0 kept.
    pub by_interval: BTreeMap<u32, u32>,
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<IntervalsRecord, NotBuilt> {
    let mut by_interval = BTreeMap::new();
    for card in input.scope.cards() {
        // Suspended and buried cards too (R1394).
        if matches!(card.card_type, CardType::Review | CardType::Relearning) {
            tally(&mut by_interval, card.ivl);
        }
    }
    Ok(IntervalsRecord { by_interval })
}

impl GraphRecord for IntervalsRecord {
    fn graph(&self) -> Graph {
        Graph::Intervals
    }

    fn encode(&self, out: &mut Encoder) {
        write_counts(out, &self.by_interval);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
