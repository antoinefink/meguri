// SPDX-License-Identifier: MIT
//! Independently authored from specs/servers/meguri-collection-mcp.md M4–M6.
use serde_json::{Value, json};

pub const READ_SCOPE: &str = "collection:read";
pub const WRITE_SCOPE: &str = "collection:write";
pub const MAX_PAGE_ITEMS: usize = 50;
pub const MAX_FIELDS: usize = 128;
pub const MAX_TAGS: usize = 128;
pub const MAX_STRING_BYTES: usize = 64 * 1024;
pub const MAX_FILTER_BYTES: usize = 4096;
pub const MAX_HANDLE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}
impl Access {
    pub fn scopes(self) -> &'static [&'static str] {
        match self {
            Self::Read => &[READ_SCOPE],
            Self::Write => &[READ_SCOPE, WRITE_SCOPE],
        }
    }
}

pub fn access(name: &str) -> Option<Access> {
    match name {
        "search_notes" | "list_decks_and_note_types" => Some(Access::Read),
        "create_note" | "edit_note" => Some(Access::Write),
        _ => None,
    }
}

fn id() -> Value {
    // The numeric upper bound is checked again by typed input validation.
    json!({"type":"string", "pattern":"^[1-9][0-9]{0,18}$", "maxLength":19,
        "description":"Positive decimal ID, at most 9223372036854775807."})
}
fn text(max: usize) -> Value {
    json!({"type":"string", "maxLength":max})
}
fn fields() -> Value {
    json!({"type":"object", "maxProperties":MAX_FIELDS,
        "propertyNames":{"type":"string", "minLength":1, "maxLength":MAX_FILTER_BYTES},
        "additionalProperties":text(MAX_STRING_BYTES)})
}
fn tags() -> Value {
    json!({"type":"array", "maxItems":MAX_TAGS,
        "items":{"type":"string", "minLength":1, "maxLength":MAX_FILTER_BYTES}})
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties,
        "required":required, "additionalProperties":false})
}
fn paging(properties: &mut Value) {
    properties["limit"] = json!({"type":"integer", "minimum":1,
        "maximum":MAX_PAGE_ITEMS, "default":20});
    properties["cursor"] = text(MAX_HANDLE_BYTES);
}
fn descriptor(name: &str, description: &str, schema: Value, access: Access) -> Value {
    let security = json!([{"type":"oauth2", "scopes":access.scopes()}]);
    json!({
        "name":name, "description":description, "inputSchema":schema,
        "securitySchemes":security,
        "_meta":{"securitySchemes":security},
        "annotations":{
            "readOnlyHint":access == Access::Read,
            "destructiveHint":name == "edit_note",
            "idempotentHint":access == Access::Read,
            "openWorldHint":false
        }
    })
}

/// The complete wire descriptors, including older clients' metadata mirror.
/// Length schemas count Unicode characters; admission additionally caps UTF-8 bytes.
pub fn descriptors() -> Vec<Value> {
    let mut search = json!({"query":text(MAX_FILTER_BYTES), "deck_id":id(),
        "tag":text(MAX_FILTER_BYTES), "note_id":id()});
    paging(&mut search);
    let mut search = object(search, &[]);
    search["oneOf"] = json!([
        {"required":["note_id"], "not":{"anyOf":[
            {"required":["query"]}, {"required":["deck_id"]}, {"required":["tag"]},
            {"required":["limit"]}, {"required":["cursor"]}]}},
        {"not":{"required":["note_id"]}, "anyOf":[
            {"required":["query"]}, {"required":["deck_id"]}, {"required":["tag"]}]}
    ]);
    let mut catalog = json!({"kind":{"type":"string",
        "enum":["all","deck","note_type"], "default":"all"}});
    paging(&mut catalog);
    let create = object(json!({"deck_id":id(), "note_type_id":id(),
        "fields":fields(), "tags":tags()}), &["deck_id", "note_type_id", "fields"]);
    let mut edit = object(json!({"note_id":id(), "revision":text(MAX_HANDLE_BYTES),
        "fields":fields(), "add_tags":tags(), "remove_tags":tags()}), &["note_id", "revision"]);
    edit["anyOf"] = json!([{"required":["fields"]}, {"required":["add_tags"]},
        {"required":["remove_tags"]}]);
    vec![
        descriptor("search_notes", "Use note_id alone for exact lookup. A bounded paged search requires at least one of query, deck_id or tag. Query matches literal substrings and filters combine with AND. The limit sets page size. For one note without a search term, use deck_id with limit 1. Use list_decks_and_note_types with kind set to deck if you need a deck ID. Returns complete fields, tags, opaque revisions and a continuation cursor. A page can be empty and still return next_cursor. To continue, pass it as cursor with the same filters and limit.", search, Access::Read),
        descriptor("list_decks_and_note_types", "Read existing decks and note types, including field names in stored order. Pages list decks first, then note types, in increasing ID order within each kind.", object(catalog, &[]), Access::Read),
        descriptor("create_note", "Create a note in an existing normal deck with the named note type. Named fields and optional tags are saved and cards are generated; omitted fields start empty. Changes the synced collection.", create, Access::Write),
        descriptor("edit_note", "Change a note only when its opaque revision still matches. Replace named fields and add or remove named tags, preserving omitted values and existing cards. Changes the synced collection.", edit, Access::Write),
    ]
}
