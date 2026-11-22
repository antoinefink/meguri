//! The `today` record (`stats/studied-today` T3, T4): the Today section's unit gives it its members, fills
//! [`build`] from the scope and writes its encoding. Until then a request
//! naming it gets [`NotBuilt`], never invented values.

use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// `stats/studied-today` T3, T4.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct TodayRecord {}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<TodayRecord, NotBuilt> {
    let _ = input;
    Err(NotBuilt(Graph::Today))
}

impl GraphRecord for TodayRecord {
    fn graph(&self) -> Graph {
        Graph::Today
    }

    fn encode(&self, out: &mut Encoder) {
        let _ = out;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
