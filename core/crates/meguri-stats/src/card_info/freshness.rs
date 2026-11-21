//! What a snapshot depends on and the test of a change set against it
//! (`stats/card-info` C10).

use meguri_model::{
    changes::ChangeSet,
    ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId},
};
use meguri_storage::{ReadRows, StorageError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependencies {
    pub card: CardId,
    pub note: NoteId,
    pub notetype: NotetypeId,
    /// The card's current deck (`did`).
    pub deck: DeckId,
    pub home_deck: DeckId,
    pub preset: DeckConfigId,
    /// The review-log ids the snapshot read, ascending.
    pub revlog: Vec<RevlogId>,
    /// The due instant comes from a day number (C3.3, C10.2).
    pub due_from_day_number: bool,
    /// The collection generation the snapshot was built in.
    pub generation: u64,
}

impl Dependencies {
    /// C10's test, with the card ids of the change set's added review-log
    /// rows given by `added_card` (the observer payload's rows, or a read).
    pub fn affected_with(&self, changes: &ChangeSet, mut added_card: impl FnMut(RevlogId) -> Option<CardId>) -> bool {
        changes.whole_collection
            || changes.collection_row
            || changes.cards.iter().any(|c| c.id == self.card)
            || changes.notes.iter().any(|n| n.id == self.note)
            || changes.notetypes.iter().any(|n| n.id == self.notetype)
            || changes.decks.iter().any(|d| d.id == self.deck || d.id == self.home_deck)
            || changes.deck_configs.iter().any(|p| p.id == self.preset)
            // Both keys feed `sched_day` (C10, ruled 2026-11-21).
            || changes
                .config_keys
                .iter()
                .any(|k| k.key == "rollover" || k.key == "creationOffset")
            || changes
                .revlog_removed
                .iter()
                .any(|id| self.revlog.binary_search(id).is_ok())
            || changes.revlog_added.iter().any(|id| added_card(*id) == Some(self.card))
    }

    /// C10's test outside an observer: the added rows' card ids are read by
    /// id from `rows`. An added row that cannot be read counts as affected,
    /// since its card is unknown.
    pub fn affected(&self, changes: &ChangeSet, rows: &impl ReadRows) -> Result<bool, StorageError> {
        let mut failure = None;
        let affected = self.affected_with(changes, |id| match rows.revlog_entry(id) {
            Ok(row) => row.map(|row| row.card_id),
            Err(StorageError::Row(_)) => Some(self.card),
            Err(error) => {
                failure.get_or_insert(error);
                None
            }
        });
        match failure {
            Some(error) if !affected => Err(error),
            _ => Ok(affected),
        }
    }
}
