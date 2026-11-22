//! The future_due part's record (`stats/graphs-cards` C3, C11). Its members and its computation
//! are U101's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-cards` C3, C11).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct FutureDueRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<FutureDueRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::FutureDue))
}

impl FutureDueRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
