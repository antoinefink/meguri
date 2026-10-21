use meguri_model::card::{Card, CardQueue};
pub fn apply(card: &mut Card) -> bool {
    if card.queue == CardQueue::Suspended {
        return false;
    }
    card.queue = CardQueue::Suspended;
    true
}
pub use crate::model::card_sched::restore_negative_queue as restore;
