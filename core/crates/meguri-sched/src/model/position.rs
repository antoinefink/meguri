//! K6 position interpretation. The sole counter writer remains U27's
//! engine::cardgen::position::take_counter; later engine ops reuse it.
use meguri_model::card::{Card, CardType};

pub fn save_answer_position(card: &mut Card, next_type: CardType, preview: bool, filtered: bool) {
    if card.card_type != CardType::New || next_type == CardType::New || preview {
        return;
    }
    let due = if filtered && card.original_due != 0 {
        card.original_due
    } else {
        card.due
    };
    card.data.new_position = Some(due.max(0) as u32);
}

pub fn current_new_position(card: &Card) -> u32 {
    (if card.original_deck_id.0 != 0 {
        card.original_due
    } else {
        card.due
    }) as u32
}

pub fn save_set_due_position(card: &mut Card) {
    if card.card_type == CardType::New {
        card.data.new_position = Some(current_new_position(card));
    }
}

/// Returns the position Forget may restore, and clears its owned keys only.
pub fn forget_memory_and_position(card: &mut Card) -> Option<u32> {
    let position = if card.card_type == CardType::New {
        Some(current_new_position(card))
    } else {
        card.data.new_position
    };
    card.data.new_position = None;
    card.data.memory_state = None;
    position
}
