//! The difficulty part's record (`stats/graphs-cards` C6, C8, C11). Its members and its computation
//! are U101's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-cards` C6, C8, C11).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct DifficultyRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<DifficultyRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Difficulty))
}

impl DifficultyRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
