//! The parts of the `stats` observation (`conformance/stats-ops` H10): each
//! graph record written as the JSON of `stats/graphs-common` G14, with the
//! narrower rule of `stats/graphs-revlog` V8 for the review-log parts and
//! `stats/graphs-cards` C12's for the card parts. Both rules write map keys
//! as decimal strings, leave out a map with no key and a scalar at its
//! default, and always write a message member (a period, a range, a count
//! set). V8 also writes every list whole, an empty hour as `{}`.
//!
//! Only the records' Anki members are written; the Meguri extras (due
//! tomorrow, notes, labels, cards left out) are not observed (C12).

use std::collections::BTreeMap;
use std::fmt::Display;

use meguri_stats::graphs::records::answer_buttons::{AnswerButtonsRecord, ButtonCounts};
use meguri_stats::graphs::records::card_counts::{CardCountsRecord, StateCounts};
use meguri_stats::graphs::records::hourly::{HourlyRecord, Hours};
use meguri_stats::graphs::records::retention::{RetentionCounts, RetentionRecord};
use meguri_stats::graphs::records::reviews::{ReviewKinds, ReviewsRecord};
use meguri_stats::graphs::records::{
    AddedRecord, DifficultyRecord, FutureDueRecord, IntervalsRecord, RetrievabilityRecord, StabilityRecord,
};
use meguri_stats::graphs::{Context, Graph, GraphRecord, Preferences};
use serde_json::{Map, Value};

use super::f32_value;
use crate::scenario::FormatError;

/// One part's members as they are added, defaults left out.
#[derive(Default)]
struct Members(Map<String, Value>);

impl Members {
    /// A count: left out at 0.
    fn count(mut self, name: &str, value: impl Into<u64>) -> Self {
        let value = value.into();
        if value != 0 {
            self.0.insert(name.to_owned(), Value::from(value));
        }
        self
    }

    /// A signed value: left out at 0.
    fn signed(mut self, name: &str, value: i64) -> Self {
        if value != 0 {
            self.0.insert(name.to_owned(), Value::from(value));
        }
        self
    }

    /// A boolean: left out when false.
    fn flag(mut self, name: &str, value: bool) -> Self {
        if value {
            self.0.insert(name.to_owned(), Value::Bool(true));
        }
        self
    }

    /// A 32-bit float, as its shortest decimal with a fraction: left out
    /// at 0. A NaN or infinite value has no JSON form and fails the
    /// observation as a run error; no scenario stores the cards that give
    /// one.
    fn float(mut self, name: &str, value: f32) -> Result<Self, FormatError> {
        if value != 0.0 {
            self.0.insert(name.to_owned(), f32_value(value)?);
        }
        Ok(self)
    }

    /// A map member: left out with no key.
    fn map(mut self, name: &str, value: Map<String, Value>) -> Self {
        if !value.is_empty() {
            self.0.insert(name.to_owned(), Value::Object(value));
        }
        self
    }

    /// A message or list member: always written.
    fn whole(mut self, name: &str, value: Value) -> Self {
        self.0.insert(name.to_owned(), value);
        self
    }

    fn done(self) -> Value {
        Value::Object(self.0)
    }
}

/// A sparse map keyed by decimal strings.
fn keyed<K: Display, V>(map: &BTreeMap<K, V>, mut value: impl FnMut(&V) -> Value) -> Map<String, Value> {
    map.iter().map(|(key, entry)| (key.to_string(), value(entry))).collect()
}

fn card_numbers<K: Display>(map: &BTreeMap<K, u32>) -> Map<String, Value> {
    keyed(map, |cards| Value::from(*cards))
}

fn kinds(day: &ReviewKinds) -> Value {
    Members::default()
        .signed("learn", day.learn)
        .signed("relearn", day.relearn)
        .signed("young", day.young)
        .signed("mature", day.mature)
        .signed("filtered", day.filtered)
        .done()
}

fn reviews(record: &ReviewsRecord) -> Value {
    Members::default()
        .map("count", keyed(&record.count, kinds))
        .map("time", keyed(&record.time, kinds))
        .done()
}

fn period(counts: &RetentionCounts) -> Value {
    Members::default()
        .count("young_passed", counts.young_passed)
        .count("young_failed", counts.young_failed)
        .count("mature_passed", counts.mature_passed)
        .count("mature_failed", counts.mature_failed)
        .done()
}

/// The ranges of the three parts that have an all-time member, which only
/// `allHistory` gives (G3).
fn ranges<T>(named: [(&str, &T); 3], all_time: Option<&T>, write: impl Fn(&T) -> Value) -> Value {
    let mut members = Members::default();
    for (name, range) in named {
        members = members.whole(name, write(range));
    }
    if let Some(all) = all_time {
        members = members.whole("all_time", write(all));
    }
    members.done()
}

fn retention(record: &RetentionRecord) -> Value {
    let mut members = Members::default();
    for (name, counts) in [
        ("today", &record.today),
        ("yesterday", &record.yesterday),
        ("week", &record.week),
        ("month", &record.month),
        ("year", &record.year),
    ] {
        members = members.whole(name, period(counts));
    }
    if let Some(all) = &record.all_time {
        members = members.whole("all_time", period(all));
    }
    members.done()
}

fn hours(range: &Hours) -> Value {
    range
        .iter()
        .map(|hour| Members::default().count("total", hour.total).count("correct", hour.correct).done())
        .collect()
}

fn hourly(record: &HourlyRecord) -> Value {
    let named = [("one_month", &record.one_month), ("three_months", &record.three_months), ("one_year", &record.one_year)];
    ranges(named, record.all_time.as_ref(), hours)
}

fn presses(group: &ButtonCounts) -> Value {
    let list = |values: &[u64; 4]| Value::from(values.to_vec());
    Members::default()
        .whole("learning", list(&group.learning))
        .whole("young", list(&group.young))
        .whole("mature", list(&group.mature))
        .done()
}

fn buttons(record: &AnswerButtonsRecord) -> Value {
    let named = [("one_month", &record.one_month), ("three_months", &record.three_months), ("one_year", &record.one_year)];
    ranges(named, record.all_time.as_ref(), presses)
}

fn future_due(record: &FutureDueRecord) -> Value {
    Members::default()
        .map("future_due", card_numbers(&record.due_days))
        .flag("have_backlog", record.have_backlog)
        .count("daily_load", record.daily_load)
        .done()
}

fn state_counts(counts: &StateCounts) -> Value {
    Members::default()
        .count("newCards", counts.new)
        .count("learn", counts.learning)
        .count("relearn", counts.relearning)
        .count("young", counts.young)
        .count("mature", counts.mature)
        .count("suspended", counts.suspended)
        .count("buried", counts.buried)
        .done()
}

fn card_counts(record: &CardCountsRecord) -> Value {
    Members::default()
        .whole("including_inactive", state_counts(&record.including_inactive))
        .whole("excluding_inactive", state_counts(&record.excluding_inactive))
        .done()
}

fn intervals(record: &IntervalsRecord) -> Value {
    Members::default().map("intervals", card_numbers(&record.by_interval)).done()
}

fn stability(record: &StabilityRecord) -> Value {
    Members::default().map("intervals", card_numbers(&record.by_days)).done()
}

fn difficulty(record: &DifficultyRecord) -> Result<Value, FormatError> {
    // Anki's `average` member carries the median (C8.2).
    Ok(Members::default()
        .map("eases", card_numbers(&record.by_percent))
        .float("average", record.median)?
        .done())
}

fn retrievability(record: &RetrievabilityRecord) -> Result<Value, FormatError> {
    Ok(Members::default()
        .map("retrievability", card_numbers(&record.by_percent))
        .float("average", record.average)?
        .float("sum_by_card", record.sum_by_card)?
        .float("sum_by_note", record.sum_by_note)?
        .done())
}

fn added(record: &AddedRecord) -> Value {
    Members::default().map("added", card_numbers(&record.by_day)).done()
}

/// The `context` part (G14): the rollover hour and `fsrs` as the request
/// read them.
pub fn context_part(context: &Context, preferences: &Preferences) -> Value {
    Members::default()
        .count("rollover_hour", context.rollover_hour)
        .flag("fsrs", preferences.fsrs)
        .done()
}

fn typed<T: 'static>(record: &dyn GraphRecord) -> Result<&T, FormatError> {
    record
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| FormatError::new(format!("the {} record has another type", record.graph().part())))
}

/// One graph's part, named by [`Graph::part`]. The Today and Calendar
/// records have no part: the observation never names them (H10).
pub fn graph_part(record: &dyn GraphRecord) -> Result<Value, FormatError> {
    Ok(match record.graph() {
        Graph::Reviews => reviews(typed(record)?),
        Graph::Retention => retention(typed(record)?),
        Graph::Hourly => hourly(typed(record)?),
        Graph::AnswerButtons => buttons(typed(record)?),
        Graph::FutureDue => future_due(typed(record)?),
        Graph::CardCounts => card_counts(typed(record)?),
        Graph::Intervals => intervals(typed(record)?),
        Graph::Stability => stability(typed(record)?),
        Graph::Difficulty => difficulty(typed(record)?)?,
        Graph::Retrievability => retrievability(typed(record)?)?,
        Graph::Added => added(typed(record)?),
        Graph::Today | Graph::Calendar => {
            return Err(FormatError::new(format!("the {} record is not observed", record.graph().part())));
        }
    })
}
