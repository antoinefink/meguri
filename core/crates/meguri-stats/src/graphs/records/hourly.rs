//! The `hours` record (`stats/graphs-revlog` V5): U99 gives it its members, fills
//! [`build`] from the scope and writes its encoding. Until then a request
//! naming it gets [`NotBuilt`], never invented values.

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-revlog` V5.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct HourlyRecord {}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<HourlyRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Hourly))
}

impl GraphRecord for HourlyRecord {
    fn graph(&self) -> Graph {
        Graph::Hourly
    }

    fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
