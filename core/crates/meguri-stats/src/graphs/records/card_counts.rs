//! The `card_counts` record (`stats/graphs-cards` C4, C11): U101 gives it its members, fills
//! [`build`] from the scope and writes its encoding. Until then a request
//! naming it gets [`NotBuilt`], never invented values.

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-cards` C4, C11.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct CardCountsRecord {}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<CardCountsRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::CardCounts))
}

impl GraphRecord for CardCountsRecord {
    fn graph(&self) -> Graph {
        Graph::CardCounts
    }

    fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
