use meguri_model::{card::Card, ids::DeckId};
pub fn apply(card: &mut Card) {
    crate::model::card_sched::return_from_filtered(card);
    // Damaged rows without a home still clear the original-due column (M8).
    card.original_deck_id = DeckId(0);
    card.original_due = 0;
}
