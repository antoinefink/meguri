//! The derived store (`specs/storage/derived-store.md`): Meguri's own
//! indexes in `meguri-derived.db` next to the collection, never inside it
//! (R1487, R2016). This module holds the file and its connection, the stamp,
//! the component registry, the staging tables of a rebuild, and how a
//! collection reader attaches the file. The engine's `derived_host` runs it:
//! the threads, the observer, readiness and generations.

mod file;
mod rebuild;
mod registry;
mod stamp;

use std::path::{Path, PathBuf};

pub use file::{DerivedFile, DerivedTx, Found, TxSavepoint};
pub use registry::{ComponentSchema, ComponentTables, IndexDef, Registry, Synced, TableDef, Target};
pub use stamp::{CollectionMark, ComponentRecord, RebuildReason, Stamp, Verdict, assess};

use crate::connections::{Reader, file_uri};
use crate::error::StorageError;

/// The file's name, in the collection's directory (D1).
pub const FILE_NAME: &str = "meguri-derived.db";
/// The schema name under which readers attach the file (D9).
pub const ATTACH_SCHEMA: &str = "derived";
/// `PRAGMA user_version` of the store's format (D2).
pub const FORMAT: i64 = 1;
/// `PRAGMA application_id`: "MGRD" (D2).
pub const APPLICATION_ID: i64 = 0x4D47_5244;

/// Where the derived store of the collection at `collection` lives.
pub fn derived_path(collection: &Path) -> PathBuf {
    collection.with_file_name(FILE_NAME)
}

impl Reader {
    /// Attaches the derived store at `path` read-only as schema `derived`
    /// (D9). Nothing happens when the store of `epoch` is attached already;
    /// another one is detached first. Fails inside a read transaction.
    pub fn attach_derived(&mut self, path: &Path, epoch: u64) -> Result<(), StorageError> {
        if self.derived == Some(epoch) {
            return Ok(());
        }
        self.detach_derived()?;
        self.conn.execute(
            &format!("ATTACH DATABASE ?1 AS {ATTACH_SCHEMA}"),
            [file_uri(path, Some("mode=ro"))],
        )?;
        self.derived = Some(epoch);
        Ok(())
    }

    /// Detaches the derived store, when one is attached.
    pub fn detach_derived(&mut self) -> Result<(), StorageError> {
        if self.derived.is_some() {
            self.conn.execute_batch(&format!("DETACH DATABASE {ATTACH_SCHEMA}"))?;
            self.derived = None;
        }
        Ok(())
    }

    /// The epoch of the attached derived store.
    pub fn derived_epoch(&self) -> Option<u64> {
        self.derived
    }
}
