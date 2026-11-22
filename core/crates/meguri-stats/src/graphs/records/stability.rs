//! The `stability` record (`stats/graphs-cards` C6, C7, C11): the cards
//! with a memory state by stability in whole days, and the started cards
//! left out for having none. Ranges and the median are the presentation
//! layer's.
//!
//! Encoding (G6): the stability map (one integer member: cards), then the
//! left-out count.

use std::collections::BTreeMap;

use super::card_rules::{lacks_memory, tally, write_counts};
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-cards` C7.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct StabilityRecord {
    /// The `intervals` member: rounded stability (C7) → cards.
    pub by_days: BTreeMap<u32, u32>,
    /// C6.2.
    pub left_out: u32,
}

/// C7: halves away from zero, then saturating into `u32` (NaN gives 0).
fn stability_key(stability: f32) -> u32 {
    stability.round() as u32
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<StabilityRecord, NotBuilt> {
    let mut record = StabilityRecord::default();
    for card in input.scope.cards() {
        match card.memory {
            Some(memory) => tally(&mut record.by_days, stability_key(memory.stability)),
            None => record.left_out += u32::from(lacks_memory(&card)),
        }
    }
    Ok(record)
}

impl GraphRecord for StabilityRecord {
    fn graph(&self) -> Graph {
        Graph::Stability
    }

    fn encode(&self, out: &mut Encoder) {
        write_counts(out, &self.by_days);
        out.integer(i64::from(self.left_out));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c7_rounding_and_saturation() {
        for (s, key) in [
            (0.4, 0),
            (0.5, 1),
            (2.5, 3),
            (20.49, 20),
            (-3.0, 0),
            (f32::INFINITY, u32::MAX),
            (f32::NAN, 0),
            (5e9, u32::MAX),
        ] {
            assert_eq!(stability_key(s), key, "{s}");
        }
    }
}
