//! One pass over the scope's review-log rows for the page's per-request
//! review-log parts (`stats/history-reads` H4, H6, H10, H11), so a
//! refresh reads its 1.2 million rows once for all of them, not once per
//! part. The windowed parts take the rows from the window's first id on;
//! Activity takes every row with `id` ≥ 0 (H11), from the same view.

use super::activity::{ActivityCounter, ActivityDaysPart};
use super::by_answer::{ByAnswerCounter, ByAnswerPart};
use super::by_deck::{ByDeckCounter, ByDeckPart};
use super::decks::{DeckEntry, Retentions};
use super::retention_by_day::{RetentionByDayPart, RetentionCounter};
use crate::graphs::{History, Scope, ScopeData};

/// What the pass computes; `None` for a part not asked for or that cannot
/// be (its decks or its rows unread).
#[derive(Default)]
pub(crate) struct LogParts {
    pub by_answer: Option<ByAnswerPart>,
    pub retention_by_day: Option<RetentionByDayPart>,
    pub by_deck: Option<ByDeckPart>,
    pub activity_days: Option<ActivityDaysPart>,
}

/// The pass's inputs.
pub(crate) struct LogInputs<'a> {
    /// The window's scope.
    pub window: &'a ScopeData,
    /// The scope's rows of all history, when Activity is asked for and they
    /// read (else Activity is not computed).
    pub all: Option<&'a ScopeData>,
    pub page_scope: &'a Scope,
    pub history: History,
    pub next_day_at: i64,
    pub by_answer: bool,
    pub retentions: Option<&'a Retentions>,
    pub decks: Option<&'a [DeckEntry]>,
}

pub(crate) fn compute(inputs: &LogInputs<'_>) -> LogParts {
    let n = inputs.next_day_at;
    let window = inputs.window;
    let mut answers = inputs.by_answer.then(|| ByAnswerCounter::new(n));
    let mut retention = inputs.retentions.map(|decks| RetentionCounter::new(window, n, decks));
    let mut by_deck = inputs
        .decks
        .map(|decks| ByDeckCounter::new(window, inputs.page_scope, inputs.history, n, decks));
    let mut activity = inputs.all.map(|_| ActivityCounter::new(n));
    let windowed = answers.is_some() || retention.is_some() || by_deck.is_some();
    if windowed || activity.is_some() {
        // The rows of all history hold the window's as their last ones.
        let source = inputs.all.unwrap_or(window);
        let start = window.window_start();
        for (scoped, card_decks) in source.rows_with_decks() {
            let row = &scoped.row;
            if let Some(counter) = activity.as_mut() {
                counter.add(row);
            }
            if !windowed || row.id < start {
                continue;
            }
            if let Some(counter) = answers.as_mut() {
                counter.add(row);
            }
            if let Some(counter) = retention.as_mut() {
                counter.add(row, card_decks);
            }
            if let Some(counter) = by_deck.as_mut() {
                counter.add(row, card_decks);
            }
        }
    }
    LogParts {
        by_answer: answers.map(ByAnswerCounter::finish),
        retention_by_day: retention.map(RetentionCounter::finish),
        by_deck: by_deck.map(ByDeckCounter::finish),
        activity_days: activity.map(ActivityCounter::finish),
    }
}
