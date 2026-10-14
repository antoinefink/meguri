//! The `cards` row (`specs/model/row-records.md` R4, R1459).

use crate::card_data::CardData;
use crate::ids::{CardId, DeckId, NoteId};
use crate::time::EpochSecs;
use crate::usn::Usn;

/// `cards.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardType {
    New = 0,
    Learning = 1,
    Review = 2,
    Relearning = 3,
}

impl CardType {
    /// R4: the low 8 bits of the stored value; `None` when they are not a
    /// known type (the row is unreadable).
    pub fn from_stored(stored: i64) -> Option<CardType> {
        match stored as u8 {
            0 => Some(CardType::New),
            1 => Some(CardType::Learning),
            2 => Some(CardType::Review),
            3 => Some(CardType::Relearning),
            _ => None,
        }
    }

    pub fn stored(self) -> i64 {
        self as i64
    }
}

/// `cards.queue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardQueue {
    New = 0,
    Learning = 1,
    Review = 2,
    InterdayLearning = 3,
    Preview = 4,
    Suspended = -1,
    BuriedByScheduler = -2,
    BuriedByUser = -3,
}

impl CardQueue {
    /// R4: the low 8 bits of the stored value as a two's-complement byte;
    /// `None` when that is not a known queue (the row is unreadable).
    pub fn from_stored(stored: i64) -> Option<CardQueue> {
        match stored as u8 as i8 {
            0 => Some(CardQueue::New),
            1 => Some(CardQueue::Learning),
            2 => Some(CardQueue::Review),
            3 => Some(CardQueue::InterdayLearning),
            4 => Some(CardQueue::Preview),
            -1 => Some(CardQueue::Suspended),
            -2 => Some(CardQueue::BuriedByScheduler),
            -3 => Some(CardQueue::BuriedByUser),
            _ => None,
        }
    }

    pub fn stored(self) -> i64 {
        self as i64
    }
}

#[derive(Clone, Debug)]
pub struct Card {
    pub id: CardId,
    /// `nid`.
    pub note_id: NoteId,
    /// `did`; for a card in a filtered deck, that filtered deck.
    pub deck_id: DeckId,
    /// Template ordinal (cloze number - 1 for cloze note types).
    pub ord: u16,
    /// `mod`, epoch seconds.
    pub mtime: EpochSecs,
    pub usn: Usn,
    pub card_type: CardType,
    pub queue: CardQueue,
    /// A new-queue position, epoch seconds (learning, preview) or a day number
    /// (review, interday learning), by queue (R1459).
    pub due: i32,
    /// `ivl`, days.
    pub interval: u32,
    /// `factor`: ease in permille; 0 for new cards. Kept for SM-2 (R1484).
    pub ease_factor: u16,
    pub reps: u32,
    pub lapses: u32,
    /// `left`: learning steps left.
    pub remaining_steps: u32,
    /// `odue`: for a card in a filtered deck, its `due` before it moved there.
    pub original_due: i32,
    /// `odid`: for a card in a filtered deck, its home deck; else 0.
    pub original_deck_id: DeckId,
    /// The low 3 bits are the user flag; other bits are kept.
    pub flags: u8,
    pub data: CardData,
}

impl Card {
    /// The user flag: 0 none, 1 to 7.
    pub fn user_flag(&self) -> u8 {
        self.flags & 0b111
    }

    /// R4: `due` and `odue` outside the `i32` range read as 0.
    pub fn i32_or_zero(stored: i64) -> i32 {
        i32::try_from(stored).unwrap_or(0)
    }
}
