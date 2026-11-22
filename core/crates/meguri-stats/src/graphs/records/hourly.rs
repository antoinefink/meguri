//! The hours part's record (`stats/graphs-revlog` V5). Its members and its computation
//! are U99's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-revlog` V5).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct HourlyRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<HourlyRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Hourly))
}

impl HourlyRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
