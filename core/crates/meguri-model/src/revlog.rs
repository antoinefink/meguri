//! The `revlog` row (`specs/model/row-records.md` R5, R1462).

use crate::ids::{CardId, RevlogId};
use crate::usn::Usn;

/// `revlog.type`. Meguri writes every kind but `Filtered`, and keeps the
/// filtered rows it receives (R1462).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RevlogKind {
    Learning = 0,
    Review = 1,
    Relearning = 2,
    /// Early review in a filtered deck.
    Filtered = 3,
    Manual = 4,
    Rescheduled = 5,
}

impl RevlogKind {
    /// R5: the low 8 bits of the stored value; an unknown kind reads as
    /// `Learning` (0).
    pub fn from_stored(stored: i64) -> RevlogKind {
        match stored as u8 {
            1 => RevlogKind::Review,
            2 => RevlogKind::Relearning,
            3 => RevlogKind::Filtered,
            4 => RevlogKind::Manual,
            5 => RevlogKind::Rescheduled,
            _ => RevlogKind::Learning,
        }
    }

    pub fn stored(self) -> i64 {
        self as i64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevlogEntry {
    /// The review time in epoch milliseconds.
    pub id: RevlogId,
    /// `cid`.
    pub card_id: CardId,
    pub usn: Usn,
    /// `ease`: 1 Again, 2 Hard, 3 Good, 4 Easy; 0 for manual changes.
    pub button: u8,
    /// `ivl`: positive in days, negative in seconds.
    pub interval: i32,
    /// `lastIvl`, same units.
    pub last_interval: i32,
    /// `factor`: ease in permille, or the FSRS difficulty mapped to 100-1100.
    pub ease_factor: u32,
    /// `time`: answer time in milliseconds.
    pub taken_millis: u32,
    pub kind: RevlogKind,
}

impl RevlogEntry {
    /// R5: a stored `time` outside the `u32` range reads as 0.
    pub fn taken_millis_from_stored(stored: i64) -> u32 {
        u32::try_from(stored).unwrap_or(0)
    }
}
