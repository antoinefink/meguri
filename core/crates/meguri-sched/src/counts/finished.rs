//! Native today facts and all-queue-1 compatibility facts have different
//! scopes. Tomorrow is a projection from the same snapshot, never a write.
use crate::{
    counts::deck_tree::DeckTree,
    queue::{Counts, Environment, Queue},
};
use meguri_model::ids::DeckId;
use meguri_storage::queries::sched_queue::DeckAggregate;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Difference {
    pub new: i64,
    pub learning: i64,
    pub review: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckStudyState {
    pub deck: DeckId,
    pub queue_counts: Counts,
    pub list_counts: Counts,
    pub difference: Difference,
    pub finished: bool,
    pub learning_remaining_today: u32,
    pub learning_later_today: u32,
    pub next_learning_due: Option<i64>,
    pub next_learning_ready_at: Option<i64>,
    pub review_limit_reached: bool,
    pub new_limit_reached: bool,
    pub learning_remaining: u32,
    pub secs_until_next_learn: i64,
    pub review_remaining: bool,
    pub new_remaining: bool,
    pub sibling_buried: u32,
    pub user_buried: u32,
    pub tomorrow: Counts,
    pub empty: bool,
}
pub fn build(
    env: &Environment,
    q: &Queue,
    list: &DeckTree,
    tomorrow: &DeckTree,
    groups: &[DeckAggregate],
    empty: bool,
) -> DeckStudyState {
    let full = env.subtree(q.deck, false);
    let normal = env.subtree(q.deck, true);
    let selected = groups.iter().filter(|g| full.contains(&g.deck)).collect::<Vec<_>>();
    let native = selected.iter().filter(|g| normal.contains(&g.deck)).collect::<Vec<_>>();
    let next_due = native.iter().filter_map(|g| g.next_learning_due).min();
    let list_counts = list.counts(q.deck);
    DeckStudyState {
        deck: q.deck,
        queue_counts: q.counts,
        list_counts,
        difference: Difference {
            new: i64::from(list_counts.new) - i64::from(q.counts.new),
            learning: i64::from(list_counts.learning) - i64::from(q.counts.learning),
            review: i64::from(list_counts.review) - i64::from(q.counts.review),
        },
        finished: q.main.is_empty() && !q.learning.iter().any(|c| c.due <= env.now + i64::from(env.ahead)),
        learning_remaining_today: native.iter().map(|g| g.learning_remaining_today).sum(),
        learning_later_today: native.iter().map(|g| g.learning_later_today).sum(),
        next_learning_due: next_due,
        next_learning_ready_at: next_due.map(|t| t - i64::from(env.ahead)),
        review_limit_reached: q.review_limit_reached,
        new_limit_reached: q.new_limit_reached,
        learning_remaining: selected.iter().map(|g| g.learning_remaining).sum(),
        secs_until_next_learn: selected
            .iter()
            .filter_map(|g| g.compatibility_next_due)
            .min()
            .map_or(86400, |due| (due - i64::from(env.ahead) - env.now).max(60)),
        review_remaining: selected.iter().any(|g| g.today.review > 0 || g.today.interday > 0),
        new_remaining: selected.iter().any(|g| g.today.new > 0),
        sibling_buried: selected.iter().map(|g| g.sibling_buried).sum(),
        user_buried: selected.iter().map(|g| g.user_buried).sum(),
        tomorrow: tomorrow.counts(q.deck),
        empty,
    }
}
