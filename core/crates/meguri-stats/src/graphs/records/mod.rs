//! The graphs (`stats/graphs-common` G5) and their records (G6): one
//! variant per graph, in Anki's order, each record in its own file so the
//! units that build them (U99 the review-log graphs, U101 the card graphs)
//! never edit one another's. A record holds aggregates at the finest
//! resolution and the extras its graph spec lists, nothing per card or row.

pub mod today;
pub mod future_due;
pub mod calendar;
pub mod reviews;
pub mod card_counts;
pub mod intervals;
pub mod stability;
pub mod difficulty;
pub mod retrievability;
pub mod retention;
pub mod hourly;
pub mod answer_buttons;
pub mod added;

pub use today::TodayRecord;
pub use future_due::FutureDueRecord;
pub use calendar::CalendarRecord;
pub use reviews::ReviewsRecord;
pub use card_counts::CardCountsRecord;
pub use intervals::IntervalsRecord;
pub use stability::StabilityRecord;
pub use difficulty::DifficultyRecord;
pub use retrievability::RetrievabilityRecord;
pub use retention::RetentionRecord;
pub use hourly::HourlyRecord;
pub use answer_buttons::AnswerButtonsRecord;
pub use added::AddedRecord;

use super::context::{Context, Preferences};
use super::encoding::Encoder;
use super::scope::{History, ScopeData};

/// A graph of the set (G5), in Anki's order (R1357). Card Ease is not one
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

impl Graph {
    pub const ALL: [Graph; 13] = [Graph::Today, Graph::FutureDue, Graph::Calendar, Graph::Reviews, Graph::CardCounts, Graph::Intervals, Graph::Stability, Graph::Difficulty, Graph::Retrievability, Graph::Retention, Graph::Hourly, Graph::AnswerButtons, Graph::Added];

    /// Its part name in the `stats` observation (G14, `stats/graphs-revlog`
    /// V8, `stats/graphs-cards` C12).
    pub fn part(self) -> &'static str {
        match self {
            Graph::Today => "today",
            Graph::FutureDue => "future_due",
            Graph::Calendar => "calendar",
            Graph::Reviews => "reviews",
            Graph::CardCounts => "card_counts",
            Graph::Intervals => "intervals",
            Graph::Stability => "stability",
            Graph::Difficulty => "difficulty",
            Graph::Retrievability => "retrievability",
            Graph::Retention => "true_retention",
            Graph::Hourly => "hours",
            Graph::AnswerButtons => "buttons",
            Graph::Added => "added",
        }
    }
}

/// A graph's record (G6).
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Today(TodayRecord),
    FutureDue(FutureDueRecord),
    Calendar(CalendarRecord),
    Reviews(ReviewsRecord),
    CardCounts(CardCountsRecord),
    Intervals(IntervalsRecord),
    Stability(StabilityRecord),
    Difficulty(DifficultyRecord),
    Retrievability(RetrievabilityRecord),
    Retention(RetentionRecord),
    Hourly(HourlyRecord),
    AnswerButtons(AnswerButtonsRecord),
    Added(AddedRecord),
}

impl Record {
    pub fn graph(&self) -> Graph {
        match self {
            Record::Today(_) => Graph::Today,
            Record::FutureDue(_) => Graph::FutureDue,
            Record::Calendar(_) => Graph::Calendar,
            Record::Reviews(_) => Graph::Reviews,
            Record::CardCounts(_) => Graph::CardCounts,
            Record::Intervals(_) => Graph::Intervals,
            Record::Stability(_) => Graph::Stability,
            Record::Difficulty(_) => Graph::Difficulty,
            Record::Retrievability(_) => Graph::Retrievability,
            Record::Retention(_) => Graph::Retention,
            Record::Hourly(_) => Graph::Hourly,
            Record::AnswerButtons(_) => Graph::AnswerButtons,
            Record::Added(_) => Graph::Added,
        }
    }

    /// The G6 encoding U103's bridge carries.
    pub fn encode(&self, out: &mut Encoder) {
        match self {
            Record::Today(record) => record.encode(out),
            Record::FutureDue(record) => record.encode(out),
            Record::Calendar(record) => record.encode(out),
            Record::Reviews(record) => record.encode(out),
            Record::CardCounts(record) => record.encode(out),
            Record::Intervals(record) => record.encode(out),
            Record::Stability(record) => record.encode(out),
            Record::Difficulty(record) => record.encode(out),
            Record::Retrievability(record) => record.encode(out),
            Record::Retention(record) => record.encode(out),
            Record::Hourly(record) => record.encode(out),
            Record::AnswerButtons(record) => record.encode(out),
            Record::Added(record) => record.encode(out),
        }
    }
}

/// What a graph computes from: the resolved scope and the request's
/// context, preferences and history.
#[derive(Clone, Copy, Debug)]
pub struct GraphInput<'a> {
    pub scope: &'a ScopeData,
    pub context: &'a Context,
    pub preferences: &'a Preferences,
    pub history: History,
}

/// A graph whose record is not built yet (its unit has not landed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotBuilt(pub Graph);

/// The record of `graph`.
pub(crate) fn compute(graph: Graph, input: &GraphInput<'_>) -> Result<Record, NotBuilt> {
    Ok(match graph {
        Graph::Today => Record::Today(today::compute(input)?),
        Graph::FutureDue => Record::FutureDue(future_due::compute(input)?),
        Graph::Calendar => Record::Calendar(calendar::compute(input)?),
        Graph::Reviews => Record::Reviews(reviews::compute(input)?),
        Graph::CardCounts => Record::CardCounts(card_counts::compute(input)?),
        Graph::Intervals => Record::Intervals(intervals::compute(input)?),
        Graph::Stability => Record::Stability(stability::compute(input)?),
        Graph::Difficulty => Record::Difficulty(difficulty::compute(input)?),
        Graph::Retrievability => Record::Retrievability(retrievability::compute(input)?),
        Graph::Retention => Record::Retention(retention::compute(input)?),
        Graph::Hourly => Record::Hourly(hourly::compute(input)?),
        Graph::AnswerButtons => Record::AnswerButtons(answer_buttons::compute(input)?),
        Graph::Added => Record::Added(added::compute(input)?),
    })
}
