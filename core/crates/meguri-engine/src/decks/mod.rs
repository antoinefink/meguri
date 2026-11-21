//! Deck records and operations (specs/decks/decks.md). All mutations read
//! live rows in the writer's scope; blobs keep their original bytes.
pub mod current;
pub mod delete;
pub mod filtered;
pub mod names;
pub mod ops;
pub mod tree;

pub use current::{current, select, select_in};
#[cfg(feature = "test-hooks")]
pub use delete::without_table_preconditions_for_testing;
pub use delete::{ConfirmedLosses, DeletionSummary, delete, delete_controlled, delete_in, deletion_summary};
pub use ops::{
    CollapseScope, add, add_in, collapse, collapse_controlled, collapse_in, move_decks, move_decks_controlled, move_in,
    rename, rename_controlled, rename_in, save_in,
};

use crate::EngineError;
use meguri_model::{
    deck::Deck,
    ids::DeckId,
    proto::{
        ProtoBlob,
        deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck},
    },
    time::EpochSecs,
    usn::Usn,
};
use meguri_storage::ReadRows;

pub fn is_filtered(deck: &Deck) -> bool {
    matches!(deck.kind.kind, DeckKind::Filtered(_))
}

/// D4 and new-collection B6. The scope supplies local stamps on insertion.
pub fn new_deck(name: String) -> Deck {
    let mut common = DeckCommon::default();
    common.study_collapsed = true;
    common.browser_collapsed = true;
    let mut normal = NormalDeck::default();
    normal.config_id = 1;
    Deck {
        id: DeckId(0),
        name,
        mtime: EpochSecs(0),
        usn: Usn(0),
        common: ProtoBlob::new(common),
        kind: ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))),
    }
}

pub(crate) fn require(rows: &impl ReadRows, id: DeckId) -> Result<Deck, EngineError> {
    rows.deck(id)?.ok_or_else(|| EngineError::NotFound {
        table: meguri_storage::Table::Decks,
        key: id.0.to_string(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    FilteredParent,
    TooDeep,
    UnderDescendant,
    FilteredDeck,
    DefaultPreset,
    /// D8 step 6: a live count of the deletion grew past the confirmed one;
    /// the fresh counts.
    LossesChanged { decks: u64, cards: u64 },
}
impl Refusal {
    pub const fn kind(self) -> &'static str {
        match self {
            Self::FilteredParent => "filtered_parent",
            Self::TooDeep => "too_deep",
            Self::UnderDescendant => "under_descendant",
            Self::FilteredDeck => "filtered_deck",
            Self::DefaultPreset => "default_preset",
            Self::LossesChanged { .. } => "losses_changed",
        }
    }
}
pub(crate) fn refused(reason: Refusal) -> EngineError {
    EngineError::DeckRefused(reason)
}
