//! The `future_due` record (`stats/graphs-cards` C2, C3, C11): when the
//! scope's started cards fall due, by day from today, with the daily load
//! and the backlog flag. The ranges and the bar queries are the
//! presentation layer's.
//!
//! Encoding (G6): the day map (one integer member: cards), then
//! `have_backlog` as 0 or 1, `daily_load`, due tomorrow.

use std::collections::BTreeMap;

use meguri_model::card::{CardQueue, CardType};

use super::card_rules::{home_due, is_buried, tally, write_counts};
use super::{GraphInput, GraphRecord, NotBuilt};
use crate::graphs::encoding::Encoder;
use crate::graphs::records::Graph;

/// C3.2: a home due above this is an epoch second, anything else a day
/// number.
const TIMESTAMP_ABOVE: i64 = 1_000_000_000;

const DAY_SECS: i64 = 86_400;

/// `stats/graphs-cards` C3.6 (the round 11 extra `future_due_young` is
/// `stats/history-reads` H8, U288's).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct FutureDueRecord {
    /// The `future_due` member: due day k → counted cards (C3.2, C3.3),
    /// overdue days (k < 0) included.
    pub due_days: BTreeMap<i32, u32>,
    /// C3.5: some counted card is overdue.
    pub have_backlog: bool,
    /// C3.4.
    pub daily_load: u32,
    /// Meguri's extra: the cards at day 1, with the backlog shown or hidden.
    pub due_tomorrow: u32,
}

/// C3.2: the day a home due `home` falls on.
fn due_day(home: i32, next_day_at: i64, today: i32) -> i32 {
    if i64::from(home) > TIMESTAMP_ABOVE {
        // Truncated toward zero, as `/` on integers is.
        (i64::from(home).wrapping_sub(next_day_at) / DAY_SECS) as i32
    } else {
        home.wrapping_sub(today)
    }
}

pub(crate) fn build(input: &GraphInput<'_>) -> Result<FutureDueRecord, NotBuilt> {
    let next_day_at = input.context.next_day_at;
    // C3.2's day numbers subtract in signed 32 bits.
    let today = input.context.today as i32;
    let mut due_days = BTreeMap::new();
    let mut load = 0.0f32;
    for card in input.scope.cards() {
        // C3.1: started cards that are not suspended.
        if card.card_type == CardType::New || card.queue == CardQueue::Suspended {
            continue;
        }
        // C3.4: before C3.3, so every buried card adds to the load.
        load += 1.0 / card.ivl.max(1) as f32;
        let day = due_day(home_due(&card), next_day_at, today);
        // C3.3: a buried card counts only from tomorrow on.
        if is_buried(card.queue) && day <= 0 {
            continue;
        }
        tally(&mut due_days, day);
    }
    Ok(FutureDueRecord {
        have_backlog: due_days.keys().next().is_some_and(|day| *day < 0),
        // Truncated toward zero, saturating.
        daily_load: load as u32,
        due_tomorrow: due_days.get(&1).copied().unwrap_or(0),
        due_days,
    })
}

impl GraphRecord for FutureDueRecord {
    fn graph(&self) -> Graph {
        Graph::FutureDue
    }

    fn encode(&self, out: &mut Encoder) {
        write_counts(out, &self.due_days);
        out.integer(i64::from(self.have_backlog));
        out.integer(i64::from(self.daily_load));
        out.integer(i64::from(self.due_tomorrow));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c3_2_the_kind_follows_the_value() {
        // `today` 30, N 1,897,354,800 (C3.2's table).
        let n = 1_897_354_800;
        let day = |home: i64| due_day(home as i32, n, 30);
        assert_eq!(day(n + 1_800), 0);
        assert_eq!(day(n + 90_000), 1);
        assert_eq!(day(n - 1), 0);
        assert_eq!(day(n - 86_400), -1);
        assert_eq!(day(33), 3);
        assert_eq!(day(25), -5);
        assert_eq!(day(1_000_000_000), 999_999_970);
        assert_eq!(day(n + 259_200), 3);
        assert_eq!(day(-2_147_483_648), 2_147_483_618);
    }
}
