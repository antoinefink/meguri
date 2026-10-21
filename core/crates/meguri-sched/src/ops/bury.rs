use meguri_model::card::{Card, CardQueue};
pub fn apply(card: &mut Card) -> bool {
    if matches!(card.queue, CardQueue::Suspended | CardQueue::BuriedByUser) {
        return false;
    }
    card.queue = CardQueue::BuriedByUser;
    true
}
