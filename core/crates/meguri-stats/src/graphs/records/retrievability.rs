//! The retrievability part's record (`stats/graphs-cards` C6, C9, C11). Its members and its computation
//! are U101's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-cards` C6, C9, C11).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct RetrievabilityRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<RetrievabilityRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Retrievability))
}

impl RetrievabilityRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
