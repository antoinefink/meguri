//! Returning cards when a filtered deck is deleted (D9).
use crate::EngineError;
use meguri_model::{
    card::{CardQueue, CardType},
    ids::DeckId,
};
use meguri_storage::{ReadRows, WriteScope};

pub fn return_home_in(scope: &mut WriteScope<'_>, id: DeckId) -> Result<(), EngineError> {
    for mut card in scope.cards_of_deck(id)? {
        if card.original_deck_id.0 != 0 {
            card.deck_id = card.original_deck_id;
            card.original_deck_id = DeckId(0);
            if card.original_due != 0 {
                card.due = card.original_due;
            }
            card.original_due = 0;
            if card.queue.stored() >= 0 {
                card.queue = match card.card_type {
                    CardType::New => CardQueue::New,
                    CardType::Review => CardQueue::Review,
                    CardType::Learning | CardType::Relearning if card.due > 1_000_000_000 => CardQueue::Learning,
                    _ => CardQueue::InterdayLearning,
                };
            }
        }
        scope.put_card_forced(&card)?;
    }
    Ok(())
}
