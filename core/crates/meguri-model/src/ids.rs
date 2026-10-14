//! Row ids (`specs/model/row-records.md`). Each is the `i64` stored in its
//! table; how new ids are chosen is the storage area's rule (U23).

use std::fmt;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub i64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<i64> for $name {
            fn from(id: i64) -> $name {
                $name(id)
            }
        }

        impl From<$name> for i64 {
            fn from(id: $name) -> i64 {
                id.0
            }
        }
    };
}

id_type!(
    /// `notes.id`.
    NoteId
);
id_type!(
    /// `cards.id`.
    CardId
);
id_type!(
    /// `decks.id`; 0 means no deck where a column allows it (`cards.odid`).
    DeckId
);
id_type!(
    /// `deck_config.id`.
    DeckConfigId
);
id_type!(
    /// `notetypes.id`.
    NotetypeId
);
id_type!(
    /// `revlog.id`: the review time in epoch milliseconds.
    RevlogId
);
