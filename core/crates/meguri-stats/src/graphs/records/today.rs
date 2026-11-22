//! The today part's record (`stats/studied-today` T3, T4). Its members and its computation
//! are the Today section's unit's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/studied-today` T3, T4).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct TodayRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<TodayRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Today))
}

impl TodayRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
