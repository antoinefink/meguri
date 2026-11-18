//! Add defaults: notes/add-defaults D1–D2 and editor/config-keys K4.
use crate::{Collection, EngineError, decks};
use meguri_model::ids::{CardId, DeckId, NotetypeId};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddDefaults {
    pub deck_id: DeckId,
    pub notetype_id: NotetypeId,
}

/// Resolve both identities from one short Query snapshot. No writer or
/// scheduler path is entered, including when a candidate is missing.
pub fn add_defaults(
    collection: &Collection,
    review_card: Option<CardId>,
    preset_deck: Option<DeckId>,
) -> Result<AddDefaults, EngineError> {
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let rows = &*snapshot;
    let add_to_current = rows.config_value("addToCur")?.and_then(|v| v.as_bool()).unwrap_or(true);
    let preset = match preset_deck {
        Some(id) => normal_deck(rows, id)?,
        None => None,
    };
    if add_to_current {
        // This order is observable when both decks and note types are absent.
        let deck_id = match preset {
            Some(id) => id,
            None => current_deck_for_adding(rows, review_card)?,
        };
        let notetype_id = match remembered_notetype(rows, &format!("_deck_{}_lastNotetype", deck_id.0))? {
            Some(id) => id,
            None => current_notetype_for_adding(rows)?,
        };
        Ok(AddDefaults { deck_id, notetype_id })
    } else {
        let notetype_id = current_notetype_for_adding(rows)?;
        let deck_id = match preset {
            Some(id) => id,
            None => match last_deck(rows, notetype_id)? {
                Some(id) => id,
                None => current_deck_for_adding(rows, review_card)?,
            },
        };
        Ok(AddDefaults { deck_id, notetype_id })
    }
}

/// D2 deliberately does not require the note type to exist, or read addToCur.
pub fn notetype_last_deck(collection: &Collection, id: NotetypeId) -> Result<Option<DeckId>, EngineError> {
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    last_deck(&*snapshot, id)
}

fn integer(rows: &impl ReadRows, key: &str) -> Result<Option<i64>, EngineError> {
    Ok(rows.config_value(key)?.and_then(|value| value.as_i64()))
}

fn normal_deck(rows: &impl ReadRows, id: DeckId) -> Result<Option<DeckId>, EngineError> {
    Ok(rows.deck(id)?.filter(|deck| !decks::is_filtered(deck)).map(|deck| deck.id))
}

fn last_deck(rows: &impl ReadRows, id: NotetypeId) -> Result<Option<DeckId>, EngineError> {
    match integer(rows, &format!("_nt_{}_lastDeck", id.0))? {
        Some(id) => normal_deck(rows, DeckId(id)),
        None => Ok(None),
    }
}

fn current_deck_for_adding(rows: &impl ReadRows, review_card: Option<CardId>) -> Result<DeckId, EngineError> {
    // D11's typed-key default applies before checking kind or existence.
    let current = DeckId(integer(rows, "curDeck")?.unwrap_or(1));
    if let Some(id) = normal_deck(rows, current)? {
        return Ok(id);
    }
    if let Some(card) = review_card.map(|id| rows.card(id)).transpose()?.flatten() {
        let home = if card.original_deck_id.0 != 0 { card.original_deck_id } else { card.deck_id };
        // The chosen home must exist; a missing odid does not retry did.
        // Kind is intentionally unchecked here (D1's filtered result).
        if rows.deck(home)?.is_some() {
            return Ok(home);
        }
    }
    Ok(decks::require(rows, DeckId(1))?.id)
}

fn remembered_notetype(rows: &impl ReadRows, key: &str) -> Result<Option<NotetypeId>, EngineError> {
    match integer(rows, key)? {
        Some(id) if rows.row_image(Table::Notetypes, &RowKey::Id(id))?.is_some() => Ok(Some(NotetypeId(id))),
        _ => Ok(None),
    }
}

fn current_notetype_for_adding(rows: &impl ReadRows) -> Result<NotetypeId, EngineError> {
    if let Some(id) = remembered_notetype(rows, "curModel")? {
        return Ok(id);
    }
    rows.first_notetype_id_by_name()?.ok_or_else(|| EngineError::InvalidArgument {
        message: "collection has no note type".into(),
    })
}
