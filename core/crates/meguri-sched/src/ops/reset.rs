use meguri_model::{
    card::{Card, CardQueue, CardType},
    ids::DeckId,
};
pub fn apply(card: &mut Card, next: &mut u32, restore: bool, counts: bool) {
    let saved = crate::model::position::forget_memory_and_position(card);
    let position = if let Some(p) = saved.filter(|_| restore) {
        p
    } else {
        let p = *next;
        *next = next.wrapping_add(1);
        p
    };
    if card.original_deck_id.0 != 0 {
        card.deck_id = card.original_deck_id;
        card.original_deck_id = DeckId(0);
        card.original_due = 0;
    }
    card.card_type = CardType::New;
    card.queue = CardQueue::New;
    card.due = position as i32;
    card.interval = 0;
    card.ease_factor = 0;
    if counts {
        card.reps = 0;
        card.lapses = 0;
    }
}
