//! The `stability` record (`stats/graphs-cards` C6, C7, C11): U101 gives it its members, fills
//! [`build`] from the scope and writes its encoding. Until then a request
//! naming it gets [`NotBuilt`], never invented values.

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-cards` C6, C7, C11.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct StabilityRecord {}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<StabilityRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Stability))
}

impl GraphRecord for StabilityRecord {
    fn graph(&self) -> Graph {
        Graph::Stability
    }

    fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
