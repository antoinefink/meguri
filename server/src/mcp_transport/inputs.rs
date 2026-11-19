// SPDX-License-Identifier: MIT
//! Typed, bounded named inputs. No tenant selector is accepted.
use super::descriptors::*;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchNotes {
    pub query: Option<String>,
    pub deck_id: Option<String>,
    pub tag: Option<String>,
    pub note_id: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogKind { All, Deck, NoteType }
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCatalog {
    pub kind: Option<CatalogKind>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateNote {
    pub deck_id: String,
    pub note_type_id: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Option<Vec<String>>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditNote {
    pub note_id: String,
    pub revision: String,
    pub fields: Option<BTreeMap<String, String>>,
    pub add_tags: Option<Vec<String>>,
    pub remove_tags: Option<Vec<String>>,
}
#[derive(Clone, Debug)]
pub enum ToolCall {
    SearchNotes(SearchNotes),
    ListDecksAndNoteTypes(ListCatalog),
    CreateNote(CreateNote),
    EditNote(EditNote),
}
impl ToolCall {
    pub fn access(&self) -> Access {
        match self {
            Self::SearchNotes(_) | Self::ListDecksAndNoteTypes(_) => Access::Read,
            Self::CreateNote(_) | Self::EditNote(_) => Access::Write,
        }
    }
}

fn id(value: &str) -> bool {
    !value.starts_with('0') && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<i64>().is_ok_and(|v| v > 0)
}
fn string(value: &str, max: usize) -> bool { value.len() <= max }
fn tags(value: &Option<Vec<String>>) -> bool {
    value.as_ref().is_none_or(|tags| tags.len() <= MAX_TAGS
        && tags.iter().all(|v| !v.is_empty() && string(v, MAX_FILTER_BYTES)))
}
fn fields(value: &BTreeMap<String, String>) -> bool {
    value.len() <= MAX_FIELDS && value.iter().all(|(k,v)|
        !k.is_empty() && string(k, MAX_FILTER_BYTES) && string(v, MAX_STRING_BYTES))
}
fn page(limit: Option<u32>, cursor: &Option<String>) -> bool {
    limit.is_none_or(|v| (1..=MAX_PAGE_ITEMS as u32).contains(&v))
        && cursor.as_ref().is_none_or(|v| string(v, MAX_HANDLE_BYTES))
}

/// The byte-level preflight must run first: a Value cannot retain duplicate keys.
pub fn parse(name: &str, arguments: Map<String, Value>) -> Result<ToolCall, ()> {
    // Optional does not admit explicit null: published schemas require real values.
    if arguments.values().any(Value::is_null) { return Err(()); }
    let value = Value::Object(arguments);
    let call = match name {
        "search_notes" => {
            let v: SearchNotes = serde_json::from_value(value).map_err(|_| ())?;
            let filters = v.query.is_some() || v.deck_id.is_some() || v.tag.is_some();
            if !page(v.limit, &v.cursor)
                || v.query.as_ref().is_some_and(|v| !string(v, MAX_FILTER_BYTES))
                || v.tag.as_ref().is_some_and(|v| !string(v, MAX_FILTER_BYTES))
                || v.deck_id.as_ref().is_some_and(|v| !id(v))
                || v.note_id.as_ref().is_some_and(|v| !id(v))
                || if v.note_id.is_some() { filters || v.limit.is_some() || v.cursor.is_some() } else { !filters }
            { return Err(()); }
            ToolCall::SearchNotes(v)
        }
        "list_decks_and_note_types" => {
            let v: ListCatalog = serde_json::from_value(value).map_err(|_| ())?;
            if !page(v.limit, &v.cursor) { return Err(()); }
            ToolCall::ListDecksAndNoteTypes(v)
        }
        "create_note" => {
            let v: CreateNote = serde_json::from_value(value).map_err(|_| ())?;
            if !id(&v.deck_id) || !id(&v.note_type_id) || !fields(&v.fields) || !tags(&v.tags) { return Err(()); }
            ToolCall::CreateNote(v)
        }
        "edit_note" => {
            let v: EditNote = serde_json::from_value(value).map_err(|_| ())?;
            if !id(&v.note_id) || !string(&v.revision, MAX_HANDLE_BYTES)
                || (v.fields.is_none() && v.add_tags.is_none() && v.remove_tags.is_none())
                || v.fields.as_ref().is_some_and(|v| !fields(v))
                || !tags(&v.add_tags) || !tags(&v.remove_tags) { return Err(()); }
            // Case-folding/canonical tag overlap requires the collection's tag
            // rules; the backend rejects it before mutation, under its authority.
            ToolCall::EditNote(v)
        }
        _ => return Err(()),
    };
    Ok(call)
}
