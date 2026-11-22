//! Rules more than one card graph uses (`stats/graphs-cards` C1, C2, C6):
//! the home due, the buried and day-due tests, the percent bins and the
//! count maps' encoding.

use std::collections::BTreeMap;

use meguri_model::card::{CardQueue, CardType};

use crate::graphs::encoding::{Encoder, Member};
use crate::graphs::rows::StatsCard;

/// C2: `odue` when the card sits in a filtered deck (`odid` above 0), else
/// `due`. A negative `odid` counts as not filtered.
pub(crate) fn home_due(card: &StatsCard) -> i32 {
    if card.odid > 0 { card.odue } else { card.due }
}

/// `queue` −2 or −3.
pub(crate) fn is_buried(queue: CardQueue) -> bool {
    matches!(queue, CardQueue::BuriedByScheduler | CardQueue::BuriedByUser)
}

/// C6.2: a review or relearning card that has no memory state.
pub(crate) fn lacks_memory(card: &StatsCard) -> bool {
    card.memory.is_none() && matches!(card.card_type, CardType::Review | CardType::Relearning)
}

/// C8.1 and C9.3: a percentage `x` (already × 100, in f32) as its 1 % key.
/// Exactly 100 shares the last bin; anything else is its floor as a `u32`,
/// saturating (NaN and values below 0 give 0).
pub(crate) fn percent_bin(x: f32) -> u32 {
    if x == 100.0 { 99 } else { x.floor() as u32 }
}

/// Adds one card at `key`.
pub(crate) fn tally<K: Ord>(map: &mut BTreeMap<K, u32>, key: K) {
    *map.entry(key).or_insert(0) += 1;
}

/// A count map as G6's sparse map with one integer member.
pub(crate) fn write_counts<K: Copy + Into<i64>>(out: &mut Encoder, map: &BTreeMap<K, u32>) {
    let keys: Vec<i64> = map.keys().map(|key| (*key).into()).collect();
    let counts: Vec<i64> = map.values().map(|count| i64::from(*count)).collect();
    out.sparse_map(&keys, &[Member::Integers(&counts)]);
}
