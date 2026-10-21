use crate::model::deck_sched::DeckSched;
use meguri_model::{card::CardQueue, deck::Deck, time::EpochSecs, usn::Usn};

pub fn answered(deck: &Deck, today: u32, queue: CardQueue, elapsed_ms: u32, now: i64) -> Deck {
    let mut deck = deck.clone();
    DeckSched::record_answer(&mut deck.common, today, queue, elapsed_ms, u32::MAX);
    deck.mtime = EpochSecs(now);
    deck.usn = Usn::PENDING;
    deck
}
