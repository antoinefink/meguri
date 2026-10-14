//! Typed records of a schema-18 Anki collection, their protobuf blobs with
//! unknown-field capture, the card `data` JSON, the schema-11 JSON form, and
//! the seams every core crate takes time and randomness from (R1875) and puts
//! its fault points through (R2003).
//!
//! Specs: `specs/model/row-records.md`, `protobuf-records.md`,
//! `card-data-json.md`, `schema11-json.md` and `json-numbers.md`.

pub mod card;
pub mod card_data;
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
