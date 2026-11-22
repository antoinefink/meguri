//! The card_counts part's record (`stats/graphs-cards` C4, C11). Its members and its computation
//! are U101's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-cards` C4, C11).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct CardCountsRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<CardCountsRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::CardCounts))
}

impl CardCountsRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
