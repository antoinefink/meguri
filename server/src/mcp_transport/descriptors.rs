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
        "download_collection" | "search_notes" | "list_decks_and_note_types" => Some(Access::Read),
        "create_note" | "edit_note" | "prepare_media_upload" => Some(Access::Write),
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
    let mut create = object(json!({"deck_id":id(), "note_type_id":id(),
        "fields":fields(), "tags":tags()}), &["deck_id", "note_type_id", "fields"]);
    let mut edit = object(json!({"note_id":id(), "revision":text(MAX_HANDLE_BYTES),
        "fields":fields(), "add_tags":tags(), "remove_tags":tags()}), &["note_id", "revision"]);
    for schema in [&mut create, &mut edit] {
        schema["properties"]["files"] = json!({"type":"array","maxItems":4,"items":object(json!({
            "download_url":text(4096),"file_id":text(256),"file_name":text(1024),"mime_type":text(256)
        }), &["download_url","file_id"])});
        schema["properties"]["file_fields"] = json!({"type":"array","maxItems":4,"items":text(MAX_FILTER_BYTES)});
        schema["properties"]["uploads"] = json!({"type":"array","maxItems":4,"items":object(json!({
            "upload_id":text(43),"field":text(MAX_FILTER_BYTES)
        }), &["upload_id","field"])});
        schema["dependentRequired"] = json!({"files":["file_fields"],"file_fields":["files"]});
    }
    edit["anyOf"] = json!([{"required":["fields"]}, {"required":["add_tags"]},
        {"required":["remove_tags"]},{"required":["files"]},{"required":["uploads"]}]);
    let mut tools = vec![
        descriptor("download_collection", "Prepare a link to download the full committed SQLite collection as collection.anki2. Share download_url with the user. The user must open it, sign in to the same Meguri account and choose Download collection before expires_at. The single-use link lasts up to ten minutes, limited by the original access token expiry. Includes the collection database only. Media and server account metadata are excluded.", object(json!({}), &[]), Access::Read),
        descriptor("search_notes", "Use note_id alone for exact lookup. A bounded paged search requires at least one of query, deck_id or tag. Query matches literal substrings and filters combine with AND. The limit sets page size. For one note without a search term, use deck_id with limit 1. Use list_decks_and_note_types with kind set to deck if you need a deck ID. Returns complete fields, tags, opaque revisions and a continuation cursor. A page can be empty and still return next_cursor. To continue, pass it as cursor with the same filters and limit.", search, Access::Read),
        descriptor("list_decks_and_note_types", "Read existing decks and note types, including field names in stored order. Pages list decks first, then note types, in increasing ID order within each kind.", object(catalog, &[]), Access::Read),
        descriptor("create_note", "Create a note in an existing normal deck with the named note type. Named fields and optional tags are saved and cards are generated; omitted fields start empty. Changes the synced collection. Attach up to four JPEG, PNG, GIF, WebP, AVIF, HEIC/HEIF, MP3, WAV, FLAC, AAC (ADTS), audio-only M4A, AIFF/AIFC, CAF, Ogg Opus/Vorbis or PDF files (8 MiB each, 16 MiB total) using files and matching file_fields, or staged uploads. Meguri appends safe media references.", create, Access::Write),
        descriptor("edit_note", "Change a note only when its opaque revision still matches. Replace named fields and add or remove named tags, preserving omitted values and existing cards. Changes the synced collection. Optional files with matching file_fields or staged uploads append media references, including in an attachment-only edit. Accepts up to four JPEG, PNG, GIF, WebP, AVIF, HEIC/HEIF, MP3, WAV, FLAC, AAC (ADTS), audio-only M4A, AIFF/AIFC, CAF, Ogg Opus/Vorbis or PDF files, 8 MiB each and 16 MiB total.", edit, Access::Write),
        descriptor("prepare_media_upload", "Prepare a signed-in Meguri upload link when your client cannot attach a file directly. The same account must upload one JPEG, PNG, GIF, WebP, AVIF, HEIC/HEIF, MP3, WAV, FLAC, AAC (ADTS), audio-only M4A, AIFF/AIFC, CAF, Ogg Opus/Vorbis or PDF (up to 8 MiB), then return to chat. Use upload_id with a target field in create_note or edit_note within ten minutes; the same grant may reuse it. This step does not edit a note.", object(json!({}), &[]), Access::Write),
    ];
    for tool in &mut tools {
        if matches!(tool["name"].as_str(),Some("create_note"|"edit_note")) {
            tool["_meta"]["openai/fileParams"] = json!(["files"]);
        }
    }
    tools
}
