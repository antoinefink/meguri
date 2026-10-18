use meguri_model::{
    card::{Card, CardQueue, CardType},
    ids::DeckId,
};

/// K3: exactly one billion remains a day number.
pub fn queue_for_type(card_type: CardType, due: i32) -> CardQueue {
    match card_type {
        CardType::New => CardQueue::New,
        CardType::Review => CardQueue::Review,
        CardType::Learning | CardType::Relearning if due > 1_000_000_000 => CardQueue::Learning,
        CardType::Learning | CardType::Relearning => CardQueue::InterdayLearning,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    Position(u32),
    Timestamp(i64),
    Day(i32),
}

pub struct CardSched<'a> {
    card: &'a Card,
}
impl<'a> CardSched<'a> {
    pub fn new(card: &'a Card) -> Self {
        Self { card }
    }
    pub fn card(&self) -> &Card {
        self.card
    }
    pub fn home_deck(&self) -> DeckId {
        if self.card.original_deck_id.0 != 0 {
            self.card.original_deck_id
        } else {
            self.card.deck_id
        }
    }
    pub fn remaining_steps(&self) -> u32 {
        self.card.remaining_steps % 1000
    }
    pub fn due(&self) -> Due {
        let queue = if self.card.queue.stored() < 0 {
            queue_for_type(self.card.card_type, self.card.due)
        } else {
            self.card.queue
        };
        match queue {
            CardQueue::New => Due::Position(self.card.due as u32),
            CardQueue::Learning | CardQueue::Preview => Due::Timestamp(i64::from(self.card.due)),
            _ => Due::Day(self.card.due),
        }
    }
}

/// Only an inactive card is restored. Its caller owns stamps and persistence.
pub fn restore_negative_queue(card: &mut Card) -> bool {
    if card.queue.stored() >= 0 {
        return false;
    }
    card.queue = queue_for_type(card.card_type, card.due);
    true
}

/// K3: return to the home deck before interpreting the restored due value.
pub fn return_from_filtered(card: &mut Card) -> bool {
    if card.original_deck_id.0 == 0 {
        return false;
    }
    card.deck_id = card.original_deck_id;
    if card.original_due != 0 {
        card.due = card.original_due;
    }
    if card.queue.stored() >= 0 {
        card.queue = queue_for_type(card.card_type, card.due);
    }
    card.original_deck_id = DeckId(0);
    card.original_due = 0;
    true
}
