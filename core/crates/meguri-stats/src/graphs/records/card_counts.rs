//! The `card_counts` record (`stats/graphs-cards` C4, C11): the scope's
//! cards by state, counted twice (suspended and buried apart, and within
//! their type), the notes they belong to, and the item text of each shown
//! label (`stats/browse-queries` Q4.1). Totals and percentages are the
//! presentation layer's.
//!
//! Encoding (G6): the seven counts of `excluding_inactive`, then those of
//! `including_inactive` (each in [`StateCounts`]' field order), the notes
//! count, then the label lists for separating and for not separating, each
//! a list of (label as its [`CountLabel`] index, item text).

use std::collections::HashSet;

use meguri_model::card::{CardQueue, CardType};

use super::card_rules::is_buried;
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// C4.1: a review card is mature from this interval on, young below it.
pub const MATURE_IVL: u32 = 21;

/// One count set (C4.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateCounts {
    /// `newCards`.
    pub new: u32,
    /// `learn`.
    pub learning: u32,
    /// `relearn`.
    pub relearning: u32,
    pub young: u32,
    pub mature: u32,
    pub suspended: u32,
    pub buried: u32,
}

impl StateCounts {
    /// The counts in field order (the encoding's order).
    pub fn values(&self) -> [u32; 7] {
        [
            self.new,
            self.learning,
            self.relearning,
            self.young,
            self.mature,
            self.suspended,
            self.buried,
        ]
    }

    /// The count of `label`'s row.
    pub fn count(&self, label: CountLabel) -> u32 {
        self.values()[label as usize]
    }

    fn slot(&mut self, label: CountLabel) -> &mut u32 {
        match label {
            CountLabel::New => &mut self.new,
            CountLabel::Learning => &mut self.learning,
            CountLabel::Relearning => &mut self.relearning,
            CountLabel::Young => &mut self.young,
            CountLabel::Mature => &mut self.mature,
            CountLabel::Suspended => &mut self.suspended,
            CountLabel::Buried => &mut self.buried,
        }
    }
}

/// A Card Counts label (Q4.1), in the table's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CountLabel {
    New,
    Learning,
    Relearning,
    Young,
    Mature,
    Suspended,
    Buried,
}

/// A shown label and the text that finds its cards (Q4.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelText {
    pub label: CountLabel,
    pub text: String,
}

/// `stats/graphs-cards` C4.4 (the round 11 extra `suspended_by_state` is
/// `stats/history-reads` H8, U288's).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct CardCountsRecord {
    /// Suspended and buried cards in their own rows
    /// (`cardCountsSeparateInactive` true).
    pub excluding_inactive: StateCounts,
    /// Every card in its type's row; `suspended` and `buried` stay 0.
    pub including_inactive: StateCounts,
    /// C4.2: distinct notes among the scope's cards.
    pub notes: u32,
    /// C4.3 with the setting true: seven labels.
    pub labels_separate: Vec<LabelText>,
    /// C4.3 with the setting false: the five type labels.
    pub labels_together: Vec<LabelText>,
}

/// C4.1: the row a card's type gives.
fn type_row(card_type: CardType, ivl: u32) -> CountLabel {
    match card_type {
        CardType::New => CountLabel::New,
        CardType::Learning => CountLabel::Learning,
        CardType::Relearning => CountLabel::Relearning,
        CardType::Review if ivl < MATURE_IVL => CountLabel::Young,
        CardType::Review => CountLabel::Mature,
    }
}

/// Q4.1: what the five type labels append when inactive cards have their
/// own rows.
const ACTIVE_ONLY: &str = " AND -(\"is:buried\" OR \"is:suspended\")";

/// Q4.1's item text of `label`, for one value of the setting; none for a
/// label the setting does not show.
pub fn label_text(label: CountLabel, separate: bool) -> Option<String> {
    let base = match label {
        CountLabel::New => "\"is:new\"",
        CountLabel::Learning => "(-\"is:review\" AND \"is:learn\")",
        CountLabel::Relearning => "(\"is:review\" AND \"is:learn\")",
        CountLabel::Young => "(\"is:review\" AND -\"is:learn\") AND \"prop:ivl<21\"",
        CountLabel::Mature => "(\"is:review\" AND -\"is:learn\") AND \"prop:ivl>=21\"",
        // Never with the suffix, and only when separating.
        CountLabel::Suspended => return separate.then(|| "\"is:suspended\"".to_owned()),
        CountLabel::Buried => return separate.then(|| "\"is:buried\"".to_owned()),
    };
    let mut text = base.to_owned();
    if separate {
        text.push_str(ACTIVE_ONLY);
    }
    Some(text)
}

const LABELS: [CountLabel; 7] = [
    CountLabel::New,
    CountLabel::Learning,
    CountLabel::Relearning,
    CountLabel::Young,
    CountLabel::Mature,
    CountLabel::Suspended,
    CountLabel::Buried,
];

fn shown_labels(separate: bool) -> Vec<LabelText> {
    LABELS
        .iter()
        .filter_map(|label| label_text(*label, separate).map(|text| LabelText { label: *label, text }))
        .collect()
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<CardCountsRecord, NotBuilt> {
    let mut excluding = StateCounts::default();
    let mut including = StateCounts::default();
    let mut notes = HashSet::new();
    for card in input.scope.cards() {
        let row = type_row(card.card_type, card.ivl);
        *including.slot(row) += 1;
        let inactive_row = match card.queue {
            CardQueue::Suspended => CountLabel::Suspended,
            queue if is_buried(queue) => CountLabel::Buried,
            _ => row,
        };
        *excluding.slot(inactive_row) += 1;
        notes.insert(card.nid);
    }
    Ok(CardCountsRecord {
        excluding_inactive: excluding,
        including_inactive: including,
        notes: notes.len() as u32,
        labels_separate: shown_labels(true),
        labels_together: shown_labels(false),
    })
}

fn write_labels(out: &mut Encoder, labels: &[LabelText]) {
    out.list(labels, |out, label| {
        out.integer(label.label as i64);
        out.text(&label.text);
    });
}

impl GraphRecord for CardCountsRecord {
    fn graph(&self) -> Graph {
        Graph::CardCounts
    }

    fn encode(&self, out: &mut Encoder) {
        for set in [&self.excluding_inactive, &self.including_inactive] {
            for value in set.values() {
                out.integer(i64::from(value));
            }
        }
        out.integer(i64::from(self.notes));
        write_labels(out, &self.labels_separate);
        write_labels(out, &self.labels_together);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
