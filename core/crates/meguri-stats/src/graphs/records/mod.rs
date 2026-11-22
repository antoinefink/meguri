//! The graphs a request can name (`stats/graphs-common` G5) and the records
//! they give (G6). Each record lives in its own file, so the units that
//! build them (U99 for the review-log ones, U101 for the card ones) never
//! touch the same file; this one only lists them. A record holds
//! aggregates at the finest resolution plus its graph spec's extras, never
//! a value per card or per row.

use std::sync::Arc;

pub mod added;
pub mod answer_buttons;
pub mod calendar;
pub mod card_counts;
mod card_rules;
pub mod difficulty;
pub mod future_due;
pub mod hourly;
pub mod intervals;
pub mod retention;
pub mod retrievability;
pub mod reviews;
pub mod stability;
pub mod today;

pub use added::AddedRecord;
pub use answer_buttons::AnswerButtonsRecord;
pub use calendar::CalendarRecord;
pub use card_counts::CardCountsRecord;
pub use difficulty::DifficultyRecord;
pub use future_due::FutureDueRecord;
pub use hourly::HourlyRecord;
pub use intervals::IntervalsRecord;
pub use retention::RetentionRecord;
pub use retrievability::RetrievabilityRecord;
pub use reviews::ReviewsRecord;
pub use stability::StabilityRecord;
pub use today::TodayRecord;

use super::context::{Context, Preferences};
use super::encoding::Encoder;
use super::scope::{History, ScopeData};

/// One graph, listed in the order of G5 (R1357); Card Ease is absent
/// (R1360).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Graph {
    Today,
    FutureDue,
    Calendar,
    Reviews,
    CardCounts,
    Intervals,
    Stability,
    Difficulty,
    Retrievability,
    Retention,
    Hourly,
    AnswerButtons,
    Added,
}

/// What every record offers the bridge and the observation.
pub trait GraphRecord: std::fmt::Debug + Send + Sync {
    fn graph(&self) -> Graph;
    /// The G6 bytes U103 carries.
    fn encode(&self, out: &mut Encoder);
    /// For a caller that reads one graph's members.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A computed record, shared by the result and its clones.
pub type Record = Arc<dyn GraphRecord>;

type Builder = fn(&GraphInput<'_>) -> Result<Record, NotBuilt>;

fn boxed<T: GraphRecord + 'static>(record: T) -> Record {
    Arc::new(record)
}

/// Each graph with its `stats` observation part name (G14,
/// `stats/graphs-revlog` V8, `stats/graphs-cards` C12) and its builder.
const GRAPHS: [(Graph, &str, Builder); 13] = [
    (Graph::Today, "today", |input| today::build(input).map(boxed)),
    (Graph::FutureDue, "future_due", |input| future_due::build(input).map(boxed)),
    (Graph::Calendar, "calendar", |input| calendar::build(input).map(boxed)),
    (Graph::Reviews, "reviews", |input| reviews::build(input).map(boxed)),
    (Graph::CardCounts, "card_counts", |input| card_counts::build(input).map(boxed)),
    (Graph::Intervals, "intervals", |input| intervals::build(input).map(boxed)),
    (Graph::Stability, "stability", |input| stability::build(input).map(boxed)),
    (Graph::Difficulty, "difficulty", |input| difficulty::build(input).map(boxed)),
    (Graph::Retrievability, "retrievability", |input| retrievability::build(input).map(boxed)),
    (Graph::Retention, "true_retention", |input| retention::build(input).map(boxed)),
    (Graph::Hourly, "hours", |input| hourly::build(input).map(boxed)),
    (Graph::AnswerButtons, "buttons", |input| answer_buttons::build(input).map(boxed)),
    (Graph::Added, "added", |input| added::build(input).map(boxed)),
];

impl Graph {
    /// Every graph, in G5's order.
    pub fn all() -> impl Iterator<Item = Graph> {
        GRAPHS.iter().map(|(graph, _, _)| *graph)
    }

    /// Its part name in the `stats` observation.
    pub fn part(self) -> &'static str {
        GRAPHS
            .iter()
            .find(|(graph, _, _)| *graph == self)
            .map_or("", |(_, part, _)| part)
    }
}

/// What a record is computed from: the resolved scope, and the request's
/// context, preferences and history.
#[derive(Clone, Copy, Debug)]
pub struct GraphInput<'a> {
    pub scope: &'a ScopeData,
    pub context: &'a Context,
    pub preferences: &'a Preferences,
    pub history: History,
}

/// A graph whose record's unit has not landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotBuilt(pub Graph);

/// The record of `graph` over `input`.
pub(crate) fn compute(graph: Graph, input: &GraphInput<'_>) -> Result<Record, NotBuilt> {
    let (_, _, build) = GRAPHS
        .iter()
        .find(|(listed, _, _)| *listed == graph)
        .ok_or(NotBuilt(graph))?;
    build(input)
}
