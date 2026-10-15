//! The `graves` row (`specs/model/row-records.md` R6). When graves are added
//! is the storage area's rule (U23).

use crate::usn::Usn;

/// `graves.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraveKind {
    Card = 0,
    Note = 1,
    Deck = 2,
}

impl GraveKind {
    pub fn from_stored(stored: i64) -> Option<GraveKind> {
        match stored {
            0 => Some(GraveKind::Card),
            1 => Some(GraveKind::Note),
            2 => Some(GraveKind::Deck),
            _ => None,
        }
    }

    pub fn stored(self) -> i64 {
        self as i64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Grave {
    /// The id of the deleted card, note or deck.
    pub oid: i64,
    pub kind: GraveKind,
    pub usn: Usn,
}
