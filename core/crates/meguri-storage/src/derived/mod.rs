//! The derived store (`specs/storage/derived-store.md`): Meguri's own
//! indexes in `meguri-derived.db` next to the collection, never inside it
//! (R1487, R2016). This module holds the file and its connection, the stamp,
//! the component registry, the staging tables of a rebuild, and how a
//! collection reader attaches the file. The engine's `derived_host` runs it:
//! the threads, the observer, readiness and generations.

mod file;
pub mod files;
mod rebuild;
mod registry;
mod stamp;

use std::path::{Path, PathBuf};

pub use file::{DerivedFile, DerivedTx, Found, TxSavepoint};
pub use files::{MappedFile, component_dir, files_root};
pub use registry::{ComponentSchema, ComponentTables, IndexDef, Registry, Synced, TableDef, Target};
pub use stamp::{CollectionMark, ComponentRecord, RebuildReason, Stamp, Verdict, assess};

use rusqlite::OptionalExtension;

use crate::connections::{Reader, file_uri};
use crate::error::StorageError;
use crate::rows::{Cell, RowImage};
use crate::snapshot::ReadSnapshot;

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

impl ReadSnapshot<'_> {
    /// The stamp's `seq` as this snapshot reads the attached store, when
    /// component `name` is ready there (D3, D8); `None` when no store is
    /// attached or the component is not ready. The snapshot's view of the
    /// store is fixed at its first read of `derived`, like its view of the
    /// collection: the answer holds for every read of that view.
    pub fn derived_seq(&self, name: &str) -> Result<Option<i64>, StorageError> {
        if self.reader().derived.is_none() {
            return Ok(None);
        }
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT s.seq FROM {ATTACH_SCHEMA}.meguri_stamp AS s, {ATTACH_SCHEMA}.meguri_components AS c
                     WHERE s.id = 1 AND c.name = ?1 AND c.ready = 1"
                ),
                [name],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// The rows of component `component`'s table `table` as this snapshot
    /// reads the attached store, each with the cells of `columns` (a
    /// comma-separated column list of that table); empty when no store is
    /// attached. Ask [`ReadSnapshot::derived_seq`] (the engine's
    /// `is_current`) first: the snapshot's view of the store is fixed at its
    /// first read of `derived`.
    pub fn derived_rows(&self, component: &str, table: &str, columns: &str) -> Result<Vec<RowImage>, StorageError> {
        if self.reader().derived.is_none() {
            return Ok(Vec::new());
        }
        let mut statement = self
            .conn()
            .prepare(&format!("SELECT {columns} FROM {ATTACH_SCHEMA}.\"{component}__{table}\""))?;
        let width = statement.column_count();
        let rows = statement.query_map([], |row| {
            (0..width).map(|index| row.get_ref(index).map(Cell::from_ref)).collect::<rusqlite::Result<RowImage>>()
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// [`ReadSnapshot::derived_rows`] row by row, without keeping them:
    /// each row's cells of `columns` go to `visit`, which returns false to
    /// stop. Nothing is visited when no store is attached. `cancel` is
    /// polled while SQLite works, as `ReadRows::visit_id_rows` polls it.
    pub fn visit_derived(
        &self,
        component: &str,
        table: &str,
        columns: &str,
        cancel: &dyn Fn() -> bool,
        visit: &mut dyn FnMut(&[Cell]) -> bool,
    ) -> Result<(), StorageError> {
        if self.reader().derived.is_none() {
            return Ok(());
        }
        crate::search_rows::visit_cells(
            self.conn(),
            &format!("SELECT {columns} FROM {ATTACH_SCHEMA}.\"{component}__{table}\""),
            &[],
            cancel,
            visit,
        )
    }

    /// [`ReadSnapshot::visit_derived`] for the rows that `filter`, an SQL
    /// condition on that table's columns, holds for: SQLite skips the rest
    /// without building their cells.
    pub fn visit_derived_where(
        &self,
        component: &str,
        table: &str,
        columns: &str,
        filter: &str,
        cancel: &dyn Fn() -> bool,
        visit: &mut dyn FnMut(&[Cell]) -> bool,
    ) -> Result<(), StorageError> {
        if self.reader().derived.is_none() {
            return Ok(());
        }
        crate::search_rows::visit_cells(
            self.conn(),
            &format!("SELECT {columns} FROM {ATTACH_SCHEMA}.\"{component}__{table}\" WHERE {filter}"),
            &[],
            cancel,
            visit,
        )
    }
}
