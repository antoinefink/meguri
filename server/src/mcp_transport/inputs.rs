// SPDX-License-Identifier: MIT
//! Typed, bounded named inputs. No tenant selector is accepted.
use super::descriptors::*;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Public validation reasons and messages contain no request-derived text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    Json, Envelope, Parameters, Version, HeaderMismatch, Method,
    Arguments, MissingFilter, ExactIdCombination, Page, Search, CatalogKind, Create, Edit, NoChange,
}
impl ValidationError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Json => "invalid_json", Self::Envelope => "invalid_envelope",
            Self::Parameters => "invalid_parameters", Self::Version => "unsupported_version",
            Self::HeaderMismatch => "protocol_header_mismatch", Self::Method => "unsupported_method",
            Self::Arguments => "invalid_arguments", Self::MissingFilter => "missing_filter",
            Self::ExactIdCombination => "exact_id_combination", Self::Page => "invalid_pagination",
            Self::Search => "invalid_search", Self::CatalogKind => "invalid_catalog_kind",
            Self::Create => "invalid_create", Self::Edit => "invalid_edit", Self::NoChange => "missing_change",
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::Json => "Send one valid JSON object with no duplicate members and within the structure limits.",
            Self::Envelope => "Send a JSON-RPC 2.0 request object with a supported method and a string or integer id.",
            Self::Parameters => "Use the published parameters for this MCP method.",
            Self::Version => "Use one of the supported MCP protocol versions.",
            Self::HeaderMismatch => "Make the MCP protocol, method and tool headers agree with the request metadata.",
            Self::Method => "Use a supported MCP method. To list tools, call tools/list.",
            Self::Arguments => "Use the tool's published argument names and types. Omit optional values instead of sending null.",
            Self::MissingFilter => "Provide query, deck_id or tag to search notes, or provide note_id alone for an exact lookup.",
            Self::ExactIdCombination => "Use note_id alone, without filters, limit or cursor.",
            Self::Page => "Use a limit from 1 to 50 and a cursor within the published size limit.",
            Self::Search => "Use positive decimal string IDs and search text within the published size limits.",
            Self::CatalogKind => "Use kind all, deck or note_type, or omit kind.",
            Self::Create => "Provide deck_id and note_type_id as positive decimal strings, with fields and tags within the published limits.",
            Self::Edit => "Provide note_id as a positive decimal string. Use the revision returned for that note. Keep fields and tags within the published limits.",
            Self::NoChange => "Provide fields, add_tags, remove_tags or attachments to edit the note.",
        }
    }
}

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
    pub files: Option<Vec<FileInput>>,
    pub file_fields: Option<Vec<String>>,
    pub uploads: Option<Vec<UploadInput>>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditNote {
    pub note_id: String,
    pub revision: String,
    pub fields: Option<BTreeMap<String, String>>,
    pub add_tags: Option<Vec<String>>,
    pub remove_tags: Option<Vec<String>>,
    pub files: Option<Vec<FileInput>>,
    pub file_fields: Option<Vec<String>>,
    pub uploads: Option<Vec<UploadInput>>,
}
/// Metadata is advisory; it is never a saved name, MIME authority or credential.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileInput {
    pub download_url: String,
    pub file_id: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadInput { pub upload_id: String, pub field: String }
pub fn validate_attachments(files: &Option<Vec<FileInput>>, fields: &Option<Vec<String>>, uploads: &Option<Vec<UploadInput>>) -> Result<(), ValidationError> {
    let invalid = ValidationError::Arguments;
    if files.is_none() && fields.is_none() && uploads.is_none() { return Ok(()); }
    let count = files.as_ref().map_or(0,Vec::len) + uploads.as_ref().map_or(0,Vec::len);
    if !(1..=4).contains(&count) || files.is_some() != fields.is_some() { return Err(invalid); }
    if let Some(files) = files {
        if fields.as_ref().is_none_or(|fields|fields.len() != files.len() || fields.iter().any(|f|f.is_empty() || f.len() > MAX_FILTER_BYTES))
            || files.iter().any(|f|f.download_url.is_empty() || f.download_url.len() > 4096 || f.file_id.is_empty() || f.file_id.len() > 256
                || f.file_name.as_ref().is_some_and(|v|v.len() > 1024) || f.mime_type.as_ref().is_some_and(|v|v.len() > 256)) { return Err(invalid); }
    }
    if uploads.as_ref().is_some_and(|v|v.iter().any(|v|v.upload_id.len() != 43 || v.field.is_empty() || v.field.len() > MAX_FILTER_BYTES)) { return Err(invalid); }
    Ok(())
}
#[derive(Clone, Debug)]
pub enum ToolCall {
    SearchNotes(SearchNotes),
    ListDecksAndNoteTypes(ListCatalog),
    CreateNote(CreateNote),
    EditNote(EditNote),
    PrepareMediaUpload,
}
impl ToolCall {
    pub fn access(&self) -> Access {
        match self {
            Self::SearchNotes(_) | Self::ListDecksAndNoteTypes(_) => Access::Read,
            Self::CreateNote(_) | Self::EditNote(_) | Self::PrepareMediaUpload => Access::Write,
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
pub fn parse(name: &str, arguments: Map<String, Value>) -> Result<ToolCall, ValidationError> {
    use ValidationError as V;
    // Optional does not admit explicit null: published schemas require real values.
    if arguments.values().any(Value::is_null) { return Err(V::Arguments); }
    let value = Value::Object(arguments);
    let call = match name {
        "search_notes" => {
            let v: SearchNotes = serde_json::from_value(value).map_err(|_| V::Arguments)?;
            let filters = v.query.is_some() || v.deck_id.is_some() || v.tag.is_some();
            if v.note_id.is_some() && (filters || v.limit.is_some() || v.cursor.is_some()) {
                return Err(V::ExactIdCombination);
            }
            if v.note_id.is_none() && !filters { return Err(V::MissingFilter); }
            if !page(v.limit, &v.cursor) { return Err(V::Page); }
            if v.query.as_ref().is_some_and(|v| !string(v, MAX_FILTER_BYTES))
                || v.tag.as_ref().is_some_and(|v| !string(v, MAX_FILTER_BYTES))
                || v.deck_id.as_ref().is_some_and(|v| !id(v))
                || v.note_id.as_ref().is_some_and(|v| !id(v))
            { return Err(V::Search); }
            ToolCall::SearchNotes(v)
        }
        "list_decks_and_note_types" => {
            if value.get("kind").is_some_and(|v| !matches!(v.as_str(), Some("all" | "deck" | "note_type"))) {
                return Err(V::CatalogKind);
            }
            let v: ListCatalog = serde_json::from_value(value).map_err(|_| V::Arguments)?;
            if !page(v.limit, &v.cursor) { return Err(V::Page); }
            ToolCall::ListDecksAndNoteTypes(v)
        }
        "create_note" => {
            let v: CreateNote = serde_json::from_value(value).map_err(|_| V::Arguments)?;
            if !id(&v.deck_id) || !id(&v.note_type_id) || !fields(&v.fields) || !tags(&v.tags) { return Err(V::Create); }
            validate_attachments(&v.files,&v.file_fields,&v.uploads)?;
            ToolCall::CreateNote(v)
        }
        "edit_note" => {
            let v: EditNote = serde_json::from_value(value).map_err(|_| V::Arguments)?;
            if v.fields.is_none() && v.add_tags.is_none() && v.remove_tags.is_none() && v.files.is_none() && v.uploads.is_none() { return Err(V::NoChange); }
            if !id(&v.note_id) || !string(&v.revision, MAX_HANDLE_BYTES)
                || v.fields.as_ref().is_some_and(|v| !fields(v))
                || !tags(&v.add_tags) || !tags(&v.remove_tags) { return Err(V::Edit); }
            // Case-folding/canonical tag overlap requires the collection's tag
            // rules; the backend rejects it before mutation, under its authority.
            validate_attachments(&v.files,&v.file_fields,&v.uploads)?;
            ToolCall::EditNote(v)
        }
        "prepare_media_upload" if value.as_object().is_some_and(|v|v.is_empty()) => ToolCall::PrepareMediaUpload,
        _ => return Err(V::Arguments),
    };
    Ok(call)
}

#[cfg(test)]
mod attachment_tests {
    use super::*;
    use serde_json::json;
    fn parsed(name: &str, v: Value) -> Result<ToolCall,ValidationError> { parse(name,v.as_object().unwrap().clone()) }
    #[test]
    fn file_mapping_is_bounded_and_attachment_only_edits_are_changes() {
        let files = json!([{"download_url":"https://files.oaiusercontent.com/generated","file_id":"generated-secret","file_name":"advisory.png","mime_type":"image/png"}]);
        let input = json!({"note_id":"1","revision":"generated","files":files,"file_fields":["Front"],"uploads":[{"upload_id":"u".repeat(43),"field":"Back"}]});
        let ToolCall::EditNote(edit) = parsed("edit_note",input.clone()).unwrap() else { panic!() };
        assert_eq!(edit.files.unwrap()[0].file_id,"generated-secret");
        assert_eq!(edit.file_fields.unwrap(),vec!["Front"]);
        assert_eq!(edit.uploads.unwrap()[0].field,"Back");
        for (key,value) in [("file_fields",json!([])),("files",json!([])),("uploads",json!([{"upload_id":"bad","field":"Back"}])),("unknown",json!(1))] {
            let mut bad = input.clone(); bad[key] = value; assert!(parsed("edit_note",bad).is_err());
        }
        let mut bad = input.clone(); bad.as_object_mut().unwrap().remove("file_fields"); assert!(parsed("edit_note",bad).is_err());
        let mut bad = input.clone(); bad["files"] = json!(vec![json!({"download_url":"x","file_id":"f"});5]); bad["file_fields"] = json!(vec!["Front";5]);
        assert!(parsed("edit_note",bad).is_err());
        assert!(parsed("prepare_media_upload",json!({})).is_ok());
        assert!(parsed("prepare_media_upload",json!({"account_id":"other"})).is_err());
    }
}
