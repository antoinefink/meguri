//! The `reviews` record (`stats/graphs-revlog` V2): U99 gives it its members, fills
//! [`build`] from the scope and writes its encoding. Until then a request
//! naming it gets [`NotBuilt`], never invented values.

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/graphs-revlog` V2.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ReviewsRecord {}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<ReviewsRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Reviews))
}

impl GraphRecord for ReviewsRecord {
    fn graph(&self) -> Graph {
        Graph::Reviews
    }

    fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
