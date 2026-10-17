use meguri_model::{grave::GraveKind, ids::CardId, usn::Usn};
use meguri_storage::{ReadRows, StorageError, WriteScope};
use std::collections::BTreeSet;

/// D2: graves for removed cards, optionally notes left empty; never logs.
pub fn remove_in(scope: &mut WriteScope<'_>, ids: &[CardId], remove_empty_notes: bool) -> Result<usize, StorageError> {
    let mut notes = BTreeSet::new();
    let mut count = 0;
    for &id in ids {
        if let Some(card) = scope.card(id)? {
            notes.insert(card.note_id);
            scope.remove_card(id)?;
            scope.add_grave(id.0, GraveKind::Card, Usn::PENDING)?;
            count += 1;
        }
    }
    if remove_empty_notes {
        for id in notes {
            if scope.cards_of_note(id)?.is_empty() && scope.remove_note(id)?.changed() {
                scope.add_grave(id.0, GraveKind::Note, Usn::PENDING)?;
            }
        }
    }
    Ok(count)
}
