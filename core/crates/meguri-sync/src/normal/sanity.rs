use super::{Result, storage_error};
use crate::wire::sanity::SanityCounts;
use meguri_storage::{Table, normal_stage::NormalStage};
pub(super) fn counts(stage: &NormalStage) -> Result<SanityCounts> {
    if stage.pending_unsent().map_err(storage_error)? {
        return Err(super::missing_outgoing());
    }
    Ok(SanityCounts {
        due: [0; 3],
        cards: stage.count(Table::Cards).map_err(storage_error)?,
        notes: stage.count(Table::Notes).map_err(storage_error)?,
        revlog: stage.count(Table::Revlog).map_err(storage_error)?,
        graves: stage.count(Table::Graves).map_err(storage_error)?,
        notetypes: stage.count(Table::Notetypes).map_err(storage_error)?,
        decks: stage.count(Table::Decks).map_err(storage_error)?,
        deck_configs: stage.count(Table::DeckConfig).map_err(storage_error)?,
    })
}
