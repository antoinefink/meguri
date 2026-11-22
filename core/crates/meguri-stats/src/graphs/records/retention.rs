//! The true_retention part's record (`stats/graphs-revlog` V4). Its members and its computation
//! are U99's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-revlog` V4).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct RetentionRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<RetentionRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Retention))
}

impl RetentionRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
