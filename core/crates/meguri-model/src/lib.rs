//! Typed records of a schema-18 Anki collection, their protobuf blobs with
//! unknown-field capture, the card `data` JSON, the schema-11 JSON form, and
//! the seams every core crate takes time and randomness from (R1875) and puts
//! its fault points through (R2003), and the change value types every
//! committed op reports (KTD2).
//!
//! Specs: `specs/model/row-records.md`, `protobuf-records.md`,
//! `card-data-json.md`, `schema11-json.md` and `json-numbers.md`.

pub mod card;
pub mod card_data;
pub mod changes;
pub mod clock;
pub mod col;
pub mod config;
pub mod deck;
pub mod deck_config;
mod error;
pub mod faultpoint;
pub mod grave;
pub mod ids;
mod json;
pub mod note;
pub mod notetype;
pub mod proto;
pub mod revlog;
pub mod rng;
pub mod schema11;
pub mod tag;
pub mod time;
pub mod usn;

pub use error::{BlobColumn, BlobError, ModelError, Schema11Item};

/// The text rules of `model/json-numbers` for JSON that other crates read and
/// write outside the records above: the sync bodies (`sync/wire-format` W1).
pub mod json_text {
    use serde_json::value::RawValue;

    /// JSON text read per N9 and N10: `None` when it is not valid JSON, holds
    /// an unpaired surrogate escape, or holds a number beyond the 64-bit
    /// float range anywhere. No nesting limit: callers that need one check
    /// it first.
    pub fn parse(text: &str) -> Option<&RawValue> {
        crate::json::parse_value(text)
    }

    /// N8: `text` as a JSON string, with only the escapes JSON requires.
    pub fn push_string(out: &mut String, text: &str) {
        crate::json::push_string(out, text);
    }
}
