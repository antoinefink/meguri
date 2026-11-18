//! Read-only Add Note queries (notes/add-defaults D1–D2, tags/complete C1–C4).
use std::sync::Arc;

use crate::{CancelToken, CoreError, callback, runtime};
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

#[uniffi::export]
pub fn tag_complete(input: String, limit: u64, cancel: Arc<CancelToken>) -> Result<Vec<String>, CoreError> {
    callback::enter("tag_complete");
    cancel.check()?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    Ok(editor::tag_complete(&runtime::host()?.collection()?, &input, limit, cancel.engine_token())?)
}

/// Pure field conversion, shared by every native editor context. No host
/// or collection is consulted, and nothing is persisted.
#[uniffi::export]
pub fn editor_field_html(html: Vec<String>, for_display: bool) -> Result<Vec<String>, CoreError> {
    callback::enter("editor_field_html");
    Ok(html.iter().map(|value| if for_display {
        meguri_text::iri::encode_for_display(value).into_owned()
    } else {
        meguri_text::iri::editor_target(value)
    }).collect())
}
