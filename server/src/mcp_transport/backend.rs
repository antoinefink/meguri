// SPDX-License-Identifier: MIT
//! Application seam. Authority is server-owned and deliberately generic here.
use super::{descriptors::*, inputs::ToolCall};
use axum::{http::HeaderMap, response::Response};
pub use crate::error::McpDiagnosticDetail;
use serde::Serialize;
use std::{collections::BTreeMap, future::Future, pin::Pin};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Protocol metadata only. The ID is for a denied tool-result envelope, never
/// an account identity. No tenant, device or collection selector is supplied.
pub struct AuthorizationRequest<'a> {
    pub headers: &'a HeaderMap,
    pub required: Access,
    pub rpc_id: Option<&'a serde_json::Value>,
    pub tool_name: Option<&'a str>,
}

/// Implement with the MCP bearer verifier, never browser/native credentials.
/// Resolve/validate this request's grant, canonical resource, expiration and
/// credential generation, and enforce the COMPLETE required scope set here.
/// Return OAuth denials at the HTTP boundary (401/403 plus Bearer challenge).
/// A server-owned client binding may instead return the specified tool-level
/// challenge for write step-up. Never use body fields to select an account.
/// Implementations must not log headers, tokens, body data or challenge state.
pub trait Authorizer<A>: Send + Sync + 'static {
    fn authorize<'a>(&'a self, request: AuthorizationRequest<'a>)
        -> BoxFuture<'a, Result<A, Response>>;
}

/// All operations use the supplied authority, not any collection/tenant selector.
/// The backend owns bounded collection admission, blocking-worker isolation,
/// publication pinning, cursor/revision authentication, and grant rechecks at
/// admission, before releasing reads, and under the write barrier through commit.
/// Enforce the item/byte limits while reading/building, BEFORE growing results.
/// A dropped HTTP caller must not abandon a live durable worker or release its
/// root/barrier/admission ownership; that worker retains its own ownership.
pub trait Backend<A>: Send + Sync + 'static {
    fn dispatch(&self, authority: A, call: ToolCall)
        -> BoxFuture<'_, Result<ToolOutput, DispatchFailure>>;
}

/// Late OAuth failures must bypass the HTTP-success tool envelope. The bridge
/// supplies the same server-built challenge used by its HTTP authorizer.
pub enum DispatchFailure {
    Tool(ToolFailure),
    Diagnosed { failure: ToolFailure, detail: McpDiagnosticDetail },
    Challenge(Response),
}
impl From<ToolFailure> for DispatchFailure {
    fn from(failure: ToolFailure) -> Self { Self::Tool(failure) }
}

#[derive(Debug, Serialize)]
pub struct Note {
    pub note_id: String,
    pub note_type_id: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
    pub revision: String,
}
#[derive(Debug, Serialize)]
pub struct Deck {
    pub deck_id: String,
    pub name: String,
    pub filtered: bool,
}
#[derive(Debug, Serialize)]
pub struct NoteType {
    pub note_type_id: String,
    pub name: String,
    pub field_names: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ToolOutput {
    Notes { notes: Vec<Note>, next_cursor: Option<String> },
    Catalog { decks: Vec<Deck>, note_types: Vec<NoteType>, next_cursor: Option<String> },
    Changed { note_id: String, revision: String },
    Download { download_url: String, expires_at: String },
    Upload { upload_id: String, upload_url: String, expires_at: String },
}
impl ToolOutput {
    pub(super) fn bounded(&self) -> bool {
        let text = |s: &str| s.len() <= MAX_STRING_BYTES;
        let handle = |v: &Option<String>| v.as_ref().is_none_or(|s| s.len() <= MAX_HANDLE_BYTES);
        match self {
            Self::Notes { notes, next_cursor } => notes.len() <= MAX_PAGE_ITEMS && handle(next_cursor)
                && notes.iter().all(|n| n.fields.len() <= MAX_FIELDS && n.tags.len() <= MAX_TAGS
                    && n.revision.len() <= MAX_HANDLE_BYTES
                    && n.fields.iter().all(|(k,v)| text(k) && text(v))
                    && n.tags.iter().all(|v| text(v))),
            Self::Catalog { decks, note_types, next_cursor } =>
                decks.len().saturating_add(note_types.len()) <= MAX_PAGE_ITEMS && handle(next_cursor)
                && decks.iter().all(|d| text(&d.name))
                && note_types.iter().all(|n| text(&n.name) && n.field_names.len() <= MAX_FIELDS
                    && n.field_names.iter().all(|s| text(s))),
            Self::Changed { revision, .. } => revision.len() <= MAX_HANDLE_BYTES,
            Self::Download { download_url, expires_at } => download_url.len() <= MAX_HANDLE_BYTES && expires_at.len() <= 64,
            Self::Upload { upload_id,upload_url,expires_at } => upload_id.len() == 43 && upload_url.len() <= MAX_HANDLE_BYTES && expires_at.len() <= 64,
        }
    }
}

/// Fixed public outcomes prevent SQL, collection text or bearer material from
/// entering protocol errors. Write busy certifies that no write committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolFailure {
    NotFound,
    Conflict,
    InvalidInput,
    ResultTooLarge,
    UnsupportedCollection,
    Busy,
    AttachmentInvalid,
    AttachmentUnavailable,
    AttachmentConflict,
    Internal,
}
impl ToolFailure {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::NotFound => "not_found", Self::Conflict => "conflict",
            Self::InvalidInput => "invalid_input", Self::ResultTooLarge => "result_too_large",
            Self::UnsupportedCollection => "unsupported_collection", Self::Busy => "busy",
            Self::AttachmentInvalid => "attachment_invalid", Self::AttachmentUnavailable => "attachment_unavailable",
            Self::AttachmentConflict => "attachment_conflict", Self::Internal => "internal_error",
        }
    }
    pub(super) fn value_for_access(self, access: Access) -> serde_json::Value {
        if self == Self::Busy && access == Access::Read {
            serde_json::json!({"error":{"code":"busy",
                "message":"The collection is busy. Try again after one second.",
                "retryable":true,"retry_after_ms":1000}})
        } else { self.value() }
    }
    pub(super) fn value(self) -> serde_json::Value {
        let code = self.code();
        let message = match self {
            Self::NotFound => "The note was not found. Search for the note again before editing.",
            Self::Conflict => "The note, layout or collection changed. Restart the search or list, or fetch the current note before editing.",
            Self::InvalidInput => "Check the tool arguments against the published schema and fetch current IDs or revisions when needed.",
            Self::ResultTooLarge => "The data exceeds the tool's size limit. Send less content, narrow the search or request a smaller page.",
            Self::UnsupportedCollection => "This collection is not supported for this operation. Use a supported collection.",
            Self::Busy => "The collection is busy. The write did not commit. Retry after one second.",
            Self::AttachmentInvalid => "Use a supported image, audio file or PDF within 8 MiB per file and 16 MiB per call. The note was not changed.",
            Self::AttachmentUnavailable => "The attachment is unavailable. Attach the file again or use prepare_media_upload and return to chat before it expires. The note was not changed.",
            Self::AttachmentConflict => "The media state changed or its saved name is occupied. The note was not changed. Check the collection before trying again.",
            Self::Internal => "The operation could not be completed. Check the current collection state before deciding whether to try again.",
        };
        if matches!(self, Self::Busy) {
            serde_json::json!({"error":{"code":code, "message":message, "retryable":true, "retry_after_ms":1000,
                "write_committed":false}})
        } else { serde_json::json!({"error":{"code":code, "message":message}}) }
    }
}
