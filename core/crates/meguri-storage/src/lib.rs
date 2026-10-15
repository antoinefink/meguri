//! SQLite access to `collection.anki2`: open, collation, WAL, connections,
//! schema, rows, graves, config, the derived store and the media store. The
//! only crate that issues SQL against the collection.
//!
//! Specs: `specs/storage/open-and-connections.md`, `schema-18.md` and
//! `schema-upgrade.md`.

pub mod checkpoint;
pub mod collation;
pub mod conn_init;
pub mod connections;
mod error;
pub mod open;
mod pragmas;
#[cfg(feature = "test-hooks")]
pub mod raw;
pub mod schema;

pub use checkpoint::CheckpointOutcome;
pub use conn_init::Initializers;
pub use connections::{Reader, ReaderRole, Writer};
pub use error::{OpenError, StorageError, UpgradeStage};
pub use open::{OpenEvent, OpenOptions, OpenedCollection, open_collection};
/// Initializers receive the connection (C5); other crates register their SQL
/// functions through this re-export, never by opening a collection.
pub use rusqlite;
pub use schema::expected_objects::{ExpectedObjects, SchemaObject};
