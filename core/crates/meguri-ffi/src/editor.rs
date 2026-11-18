//! Read-only Add Note queries (notes/add-defaults D1–D2).
use crate::{CoreError, callback, runtime};
use meguri_engine::editor;
use meguri_model::ids::{CardId, DeckId, NotetypeId};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AddDefaults {
    pub deck_id: i64,
    pub notetype_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeLastDeck {
    pub deck_id: Option<i64>,
}

#[uniffi::export]
pub fn add_defaults(review_card: Option<i64>, preset_deck: Option<i64>) -> Result<AddDefaults, CoreError> {
    callback::enter("add_defaults");
    let value = editor::add_defaults(
        &runtime::host()?.collection()?,
        review_card.map(CardId),
        preset_deck.map(DeckId),
    )?;
    Ok(AddDefaults { deck_id: value.deck_id.0, notetype_id: value.notetype_id.0 })
}

#[uniffi::export]
pub fn notetype_last_deck(notetype_id: i64) -> Result<NotetypeLastDeck, CoreError> {
    callback::enter("notetype_last_deck");
    Ok(NotetypeLastDeck {
        deck_id: editor::notetype_last_deck(&runtime::host()?.collection()?, NotetypeId(notetype_id))?.map(|id| id.0),
    })
}
