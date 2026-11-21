//! The removal preview of `notetypes/structure-changes` S11: for the
//! template editor's question, the cards of each template proposed for
//! removal and the notes that would be left with no card. Read-only.

use std::collections::{BTreeMap, BTreeSet};

use meguri_model::ids::NotetypeId;
use meguri_storage::{ReadRows, ReaderRole};

use super::invalid;
use crate::{Collection, EngineError};

/// S11's output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemovalPreview {
    /// For each listed base ordinal: the cards of the type with that `ord`.
    pub cards: BTreeMap<u32, u64>,
    /// Notes of the type that have a card and whose cards all have a listed
    /// ordinal. Notes that already have no card are not counted.
    pub notes_without_cards: u64,
}

/// S11 on `reader`'s rows.
pub fn removal_preview(
    reader: &impl ReadRows,
    notetype: NotetypeId,
    ords: &BTreeSet<u32>,
) -> Result<RemovalPreview, EngineError> {
    if reader.notetype(notetype)?.is_none() {
        return Err(invalid(format!("no note type {notetype}")));
    }
    let mut preview = RemovalPreview {
        cards: ords.iter().map(|ord| (*ord, 0)).collect(),
        notes_without_cards: 0,
    };
    let mut after = None;
    loop {
        let page = reader.notetype_notes_after(notetype, after, 500)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in page {
            let cards = reader.cards_of_note(note.id)?;
            let mut listed = 0;
            for card in &cards {
                if let Some(count) = preview.cards.get_mut(&u32::from(card.ord)) {
                    *count += 1;
                    listed += 1;
                }
            }
            if !cards.is_empty() && listed == cards.len() {
                preview.notes_without_cards += 1;
            }
        }
    }
    Ok(preview)
}

/// S11 on one snapshot of the collection.
pub fn preview_removal(
    collection: &Collection,
    notetype: NotetypeId,
    ords: &BTreeSet<u32>,
) -> Result<RemovalPreview, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    removal_preview(&*snapshot, notetype, ords)
}
