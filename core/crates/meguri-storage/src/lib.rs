//! SQLite access to `collection.anki2`: open, collation, WAL, connections,
//! schema, rows, graves, config, the derived store and the media store. The
//! only crate that issues SQL against the collection.
//!
//! Specs: `specs/storage/open-and-connections.md`, `schema-18.md`,
//! `schema-upgrade.md`, `ids-usn-mtime.md`, `config-table.md`, `graves.md`
//! and `derived-store.md`.

pub mod atomic_file;
pub mod bootstrap;
pub mod bulk;
pub mod checkpoint;
pub mod collation;
pub mod collection_package;
pub mod conn_init;
pub mod connections;
#[cfg(feature = "test-hooks")]
pub mod counting_vfs;
mod day_writes;
pub mod derived;
mod durability;
mod error;
pub mod ids;
pub mod integrity;
pub mod journal;
pub mod leftovers;
mod live_snapshot;
pub mod lockfile;
pub mod media;
pub mod one_way;
pub mod open;
pub mod package_inspection;
pub mod paging;
pub mod paths;
mod pragmas;
pub mod queries;
#[cfg(feature = "test-hooks")]
pub mod raw;
pub mod rows;
pub mod schema;
pub mod sidecar;
pub mod snapshot;
pub mod space;
pub mod stamp;
pub mod verified_replace;
pub mod write_scope;

pub use checkpoint::CheckpointOutcome;
pub use conn_init::Initializers;
pub use connections::{Reader, ReaderFactory, ReaderRole, Writer};
pub use day_writes::CardQueueRow;
pub use error::{BuilderRefusal, OpenError, StorageError, UpgradeStage};
pub use journal::{Journal, JournalOptions};
pub use open::{OpenEvent, OpenOptions, OpenedCollection, open_collection};
pub use paging::{ReadRows, StoredConfig};
pub use rows::config::ConfigValue;
pub use rows::{Cell, Record, RowError, RowImage, RowKey, Table};
/// Initializers receive the connection (C5); other crates register their SQL
/// functions through this re-export, never by opening a collection.
pub use rusqlite;
pub use schema::expected_objects::{ExpectedObjects, SchemaObject};
pub use snapshot::ReadSnapshot;
pub use stamp::StampMode;
pub use write_scope::{CollectionBuilder, Committed, Savepoint, WriteScope, Written};
