#![allow(dead_code)]
use meguri_model::{
    card::{Card, CardQueue, CardType},
    ids::{CardId, DeckId, NoteId},
};

pub fn card() -> Card {
    Card {
        id: CardId(1_894_701_600_000),
        note_id: NoteId(1),
        deck_id: DeckId(1),
        ord: 0,
        mtime: Default::default(),
        usn: Default::default(),
        card_type: CardType::New,
        queue: CardQueue::New,
        due: 1,
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: Default::default(),
    }
}
