// SPDX-License-Identifier: MIT
//! Collection-only MCP backend (M4–M6, C7). No transport or credential parsing.
//!
//! The adapter supplies a server-resolved path and an Authority for the exact
//! original grant/account/publication. Admission must be nonblocking, refuse an
//! active native writer, and acquire the account publication barrier before any
//! collection state lock. Its guard must retain the barrier through commit (and
//! through the final read check), including cancellation of the blocking worker.
//! Never implement admission using native round retirement. Schedule WAL
//! checkpoint maintenance separately; MCP disables automatic/close checkpoints.
//! All methods are
//! synchronous: run them on the server's bounded blocking collection executor.
mod input;
mod read;
mod tokens;
mod write;
mod generation;
#[cfg(test)] mod tests;

pub use input::{CatalogInput, CatalogKind, CreateInput, EditInput, Fields, SearchInput};
use crate::{Error, Result};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, path::Path};

pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_RESULT_BYTES: usize = 512 * 1024;
pub const MAX_RECORD_BYTES: usize = 128 * 1024;
pub const MAX_OBJECT_BYTES: usize = 512 * 1024;
pub const MAX_FIELDS: usize = 256;
pub const MAX_TEMPLATES: usize = 500;
pub const MAX_TAGS: usize = 256;
pub const MAX_CANDIDATES: usize = 200;
pub const MAX_PAGE_WORK_BYTES: usize = 2 * 1024 * 1024;
const TOKEN_RESERVE: usize = 2048;

/// Server-owned identity, never populated from tool arguments. Publication must
/// change on every full replacement, even if the replacement bytes are equal.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context { pub account: String, pub publication: String }

pub trait Authority {
    type Guard;
    fn context(&self) -> &Context;
    /// Validate read or read+write scope, original grant, expiry, account
    /// credential generation, and pinned publication under the acquired guard.
    /// Contention returns busy(); no waits on a barrier or a native round.
    fn admit(&self, write: bool) -> Result<Self::Guard>;
    /// Revalidate the same authority and publication. Called before releasing
    /// reads and immediately before COMMIT, with the admission guard still held.
    fn recheck(&self, guard: &Self::Guard) -> Result<()>;
}

/// Core-owned seams, so collection work needs no new dependency graph. The
/// production adapter must use the U21 clock, R1875 randomness, and exactly P4's
/// seeded StdRng calculation. A failure rolls back the complete operation.
pub trait WriteRuntime {
    /// Called inside the existing write transaction immediately before each new
    /// card insert. Refusal rolls back note, tag, card, counter and stamp effects.
    fn permit_new_card(&self) -> Result<()>;
    fn now_millis(&self) -> Result<i64>;
    fn guid_random_u64(&self) -> Result<u64>;
    fn random_position(&self, counter: u32) -> Result<u32>;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteResult {
    pub note_id: String,
    pub note_type_id: String,
    pub fields: BTreeMap<String, String>,
    pub tags: Vec<String>,
    pub revision: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult { pub notes: Vec<NoteResult>, pub next_cursor: Option<String> }
#[derive(Debug, Serialize, Deserialize)]
pub struct DeckResult { pub deck_id: String, pub name: String, pub filtered: bool }
#[derive(Debug, Serialize, Deserialize)]
pub struct NoteTypeResult { pub note_type_id: String, pub name: String, pub field_names: Vec<String> }
#[derive(Debug, Serialize, Deserialize)]
pub struct CatalogResult {
    pub decks: Vec<DeckResult>, pub note_types: Vec<NoteTypeResult>, pub next_cursor: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct WriteResult { pub note_id: String, pub revision: String }

/// Keep one secret per service, stable for the desired handle lifetime. Keys
/// must be server-generated and separate from client-controlled credentials.
/// Rotating it deliberately invalidates outstanding revisions/cursors.
pub struct Backend { key: [u8; 32] }
impl Backend {
    pub fn new(key: [u8; 32]) -> Self { Self { key } }
    /// Parse raw arguments, before Value/map conversion can erase duplicates.
    pub fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
        if bytes.len() > MAX_REQUEST_BYTES { return Err(invalid()); }
        serde_json::from_slice(bytes).map_err(|_| invalid())
    }
    /// Use this one value for structuredContent and for JSON text content.
    pub fn result_json(value: &impl Serialize) -> Result<String> {
        let bytes = serde_json::to_string(value).map_err(|_| unsupported())?;
        if bytes.len() > MAX_RESULT_BYTES { return Err(too_large()); }
        Ok(bytes)
    }
    fn transaction<A: Authority, T>(&self, path: &Path, authority: &A, write: bool,
        f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let context = authority.context();
        if context.account.len() > 256 || context.publication.len() > 256 { return Err(unsupported()); }
        let guard = authority.admit(write)?;
        let mut c = super::schema::open_mcp(path, write)?;
        let tx = c.transaction_with_behavior(if write { TransactionBehavior::Immediate } else { TransactionBehavior::Deferred })
            .map_err(sql_error)?;
        let version: i64 = tx.query_row("SELECT ver FROM col WHERE id=1", [], |r| r.get(0)).map_err(sql_error)?;
        if version != 18 { return Err(unsupported()); }
        let value = f(&tx)?;
        authority.recheck(&guard)?;
        tx.commit().map_err(sql_error)?;
        // This guard also owns any account/path pin and capacity reservation.
        Ok(value)
    }
}
pub fn busy() -> Error { Error { code: "busy", field: None, retry_after: Some(1) } }
fn invalid() -> Error { Error::new("invalid_input") }
fn conflict() -> Error { Error::new("conflict") }
fn unsupported() -> Error { Error::new("unsupported_collection") }
fn too_large() -> Error { Error::new("result_too_large") }
pub(super) fn sql_error(e: rusqlite::Error) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(ref detail, _) if matches!(detail.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => busy(),
        _ => unsupported(),
    }
}
fn size(value: &impl Serialize) -> Result<usize> {
    Ok(serde_json::to_vec(value).map_err(|_| unsupported())?.len())
}
