//! One exact deck-and-descendant operand for generated searches (J3.5).
use super::{names, require};
use crate::EngineError;
use meguri_model::ids::DeckId;
use meguri_search::{builder::build, filter_tokens::{DeckValue, Value}};
use meguri_storage::ReadRows;

pub fn deck(rows: &impl ReadRows, id: DeckId) -> Result<String, EngineError> {
    let source = require(rows, id)?;
    let tree = rows.deck_rows()?.into_iter()
        .filter(|d| d.id == id || names::descendant(&d.name, &source.name))
        .map(|d| d.id).collect();
    let value = Value::Deck(DeckValue { id, native: source.name, tree });
    let item = value.item().ok_or_else(|| super::filtered::invalid("This deck has no exact search representation"))?;
    build(&item).map_err(|_| super::filtered::invalid("This deck has no exact search representation"))
}
