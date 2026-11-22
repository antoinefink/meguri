//! The calendar part's record (`stats/graphs-revlog` V3). Its members and its computation
//! are U99's; until they land, a request naming this graph fails with
//! [`NotBuilt`] rather than return made-up values.

use super::{GraphInput, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// The record (`stats/graphs-revlog` V3).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct CalendarRecord {}

pub(crate) fn compute(input: &GraphInput<'_>) -> Result<CalendarRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Calendar))
}

impl CalendarRecord {
    /// Its G6 encoding.
    pub fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }
}
