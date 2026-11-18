//! Read-only Add Note queries (notes/add-defaults D1–D2/D5, tags/complete C1–C4).
use std::sync::Arc;

use crate::{CancelToken, CoreError, callback, notetypes::NotetypeRecord, runtime};
use meguri_engine::editor;
use meguri_model::ids::{CardId, DeckId, NotetypeId};
use meguri_storage::{ReadRows, ReaderRole};

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

/// D5 uses stored name order only when the selected identity disappeared.
/// Identity, complete aggregate and generation all come from this snapshot.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AddNotetypeSelection {
    pub generation: u64,
    pub notetype: Option<NotetypeRecord>,
}

#[uniffi::export]
pub fn add_notetype_selection(expected_generation: u64, selected_id: i64) -> Result<AddNotetypeSelection, CoreError> {
    callback::enter("add_notetype_selection");
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let expected = meguri_engine::Generation(expected_generation);
    if snapshot.generation() != expected {
        return Err(meguri_engine::EngineError::StaleGeneration {
            planned: expected, current: snapshot.generation(),
        }.into());
    }
    let selected = match snapshot.notetype(NotetypeId(selected_id))? {
        Some(notetype) => Some(notetype),
        None => match snapshot.first_notetype_id_by_name()? {
            Some(id) => snapshot.notetype(id)?,
            None => None,
        },
    };
    Ok(AddNotetypeSelection {
        generation: snapshot.generation().0,
        notetype: selected.as_ref().map(NotetypeRecord::from),
    })
}
