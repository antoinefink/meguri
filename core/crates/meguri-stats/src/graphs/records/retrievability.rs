//! The `retrievability` record (`stats/graphs-cards` C6, C9, C11): each
//! card's chance of recall now, by percent, with its sums by card and by
//! note and the started cards left out for having no memory state. R is
//! computed for the request's now every time (C9.5): no cache holds it.
//!
//! Encoding (G6): the percent map (one integer member: cards), then
//! `average`, `sum_by_card` and `sum_by_note` as binary32, then the
//! left-out count.

use std::collections::BTreeMap;

use meguri_model::card::{CardQueue, CardType};

use super::card_rules::{home_due, lacks_memory, percent_bin, tally, write_counts};
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;
use crate::graphs::rows::StatsCard;

const DAY_SECS: i64 = 86_400;

/// C9.1: a home due at most this is a day number for the day-due test.
const DAY_NUMBER_AT_MOST: i64 = 365_000;

/// C9.2: the decay of a card whose data has none.
const DEFAULT_DECAY: f32 = 0.5;

/// `stats/graphs-cards` C9.7.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct RetrievabilityRecord {
    /// The `retrievability` member: percent (C9.3) → cards.
    pub by_percent: BTreeMap<u32, u32>,
    /// C9.4: `sum_by_card` × 100 / cards, 0 with no card.
    pub average: f32,
    pub sum_by_card: f32,
    pub sum_by_note: f32,
    /// C6.2.
    pub left_out: u32,
}

/// C9.1's day-due test.
fn day_due(card: &StatsCard, home: i64) -> bool {
    (card.card_type != CardType::New && home <= DAY_NUMBER_AT_MOST)
        || matches!(card.queue, CardQueue::Review | CardQueue::InterdayLearning)
        || (card.card_type == CardType::Review && (card.queue as i8) < 0)
}

/// C9.1: the seconds since the card's last review, in wrapping 64-bit
/// arithmetic, keeping the low 32 bits.
fn elapsed_secs(card: &StatsCard, now_s: i64, today: i64) -> u32 {
    if let Some(reviewed) = card.lrt {
        return now_s.wrapping_sub(reviewed) as u32;
    }
    let home = i64::from(home_due(card));
    let ivl = i64::from(card.ivl);
    if !day_due(card, home) {
        return now_s.wrapping_sub(home) as u32;
    }
    let secs = if card.queue == CardQueue::Learning {
        now_s.wrapping_sub(home.wrapping_sub(DAY_SECS.wrapping_mul(ivl)))
    } else {
        today.wrapping_sub(home).wrapping_add(ivl).wrapping_mul(DAY_SECS)
    };
    secs.max(0) as u32
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<RetrievabilityRecord, NotBuilt> {
    let now_s = input.context.now_ms.div_euclid(1000);
    let today = i64::from(input.context.today);
    let mut record = RetrievabilityRecord::default();
    let mut with_memory = 0u32;
    // (note, R or 0) per scope card, in card id order.
    let mut by_note: Vec<(i64, f32)> = Vec::new();
    for card in input.scope.cards() {
        let Some(memory) = card.memory else {
            record.left_out += u32::from(lacks_memory(&card));
            by_note.push((card.nid, 0.0));
            continue;
        };
        let days = elapsed_secs(&card, now_s, today) as f32 / DAY_SECS as f32;
        let recall = fsrs::current_retrievability(
            fsrs::MemoryState {
                stability: memory.stability,
                difficulty: memory.difficulty,
            },
            days,
            card.decay.unwrap_or(DEFAULT_DECAY),
        );
        tally(&mut record.by_percent, percent_bin(recall * 100.0));
        record.sum_by_card += recall;
        with_memory += 1;
        by_note.push((card.nid, recall));
    }
    if with_memory > 0 {
        record.average = record.sum_by_card * 100.0 / with_memory as f32;
    }
    record.sum_by_note = note_means(by_note);
    Ok(record)
}

/// C9.4's `sum_by_note`: each note's mean R over its scope cards, summed
/// in ascending note id. The stable sort keeps card id order within a note.
fn note_means(mut cards: Vec<(i64, f32)>) -> f32 {
    cards.sort_by_key(|(note, _)| *note);
    let mut total = 0.0f32;
    for group in cards.chunk_by(|a, b| a.0 == b.0) {
        let sum = group.iter().fold(0.0f32, |sum, (_, recall)| sum + recall);
        total += sum / group.len() as f32;
    }
    total
}

impl GraphRecord for RetrievabilityRecord {
    fn graph(&self) -> Graph {
        Graph::Retrievability
    }

    fn encode(&self, out: &mut Encoder) {
        write_counts(out, &self.by_percent);
        out.f32(self.average);
        out.f32(self.sum_by_card);
        out.f32(self.sum_by_note);
        out.integer(i64::from(self.left_out));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use meguri_model::card_data::MemoryState;

    use super::*;

    /// C9.6's clock.
    const NOW_S: i64 = 1_897_293_600;
    const TODAY: i64 = 30;

    fn card(card_type: CardType, queue: CardQueue, due: i32, ivl: u32) -> StatsCard {
        StatsCard {
            id: 1,
            nid: 1,
            did: 1,
            odid: 0,
            card_type,
            queue,
            due,
            odue: 0,
            ivl,
            memory: Some(MemoryState {
                stability: 5.0,
                difficulty: 5.0,
            }),
            decay: None,
            lrt: None,
        }
    }

    #[test]
    fn c9_1_elapsed_by_each_branch() {
        let with_lrt = |lrt: i64| StatsCard {
            lrt: Some(lrt),
            ..card(CardType::Review, CardQueue::Review, 31, 10)
        };
        let e = |card: &StatsCard| elapsed_secs(card, NOW_S, TODAY);
        assert_eq!(e(&with_lrt(NOW_S - 864_000)), 864_000);
        assert_eq!(e(&with_lrt(NOW_S + 10)), 4_294_967_286);
        assert_eq!(e(&with_lrt(i64::MIN)), 1_897_293_600);
        assert_eq!(e(&card(CardType::Review, CardQueue::Review, 35, 20)), 15 * 86_400);
        assert_eq!(e(&card(CardType::Review, CardQueue::Review, 35, 40)), 35 * 86_400);
        assert_eq!(e(&card(CardType::Relearning, CardQueue::Learning, (NOW_S + 600) as i32, 0)), 4_294_966_696);
        assert_eq!(e(&card(CardType::Relearning, CardQueue::Learning, (NOW_S - 3_600) as i32, 2)), 3_600);
        assert_eq!(e(&card(CardType::New, CardQueue::New, 5, 0)), (NOW_S - 5) as u32);
        assert_eq!(e(&card(CardType::Relearning, CardQueue::Learning, 25, 2)), (NOW_S + 172_775) as u32);
        assert_eq!(e(&card(CardType::Review, CardQueue::Suspended, 1_896_429_600, 0)), 0);
        let filtered = StatsCard {
            odid: 7,
            odue: 0,
            ..card(CardType::Review, CardQueue::Review, 35, 20)
        };
        assert_eq!(e(&filtered), 50 * 86_400);
        let stray = StatsCard {
            odid: -1,
            odue: 40,
            ..card(CardType::Review, CardQueue::Review, 35, 20)
        };
        assert_eq!(e(&stray), 15 * 86_400);
    }

    #[test]
    fn c9_4_note_means_in_note_order() {
        // Note A: R 0.7182781 and a card without a memory state; note B:
        // 0.7661308.
        let sum = note_means(vec![(2, 0.7661308), (1, 0.7182781), (1, 0.0)]);
        assert_eq!(sum, 1.1252699);
    }
}
