//! The derived store's file (`storage/derived-store` D2, D5.1): opening it,
//! replacing a file that is not a good store, its settings, the stamp and
//! component rows, and the write transactions of the derived writer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::collation;
use crate::connections::{file_uri, sibling};
use crate::derived::rebuild;
use crate::derived::registry::{ComponentSchema, ComponentTables, Registry, Synced, Target};
use crate::derived::stamp::{CollectionMark, ComponentRecord, RebuildReason, Stamp};
use crate::derived::{APPLICATION_ID, FORMAT};
use crate::error::StorageError;

const CACHE_SIZE_KIB: i64 = -8_192;
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1_000);

const READ_WRITE_CREATE: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
    .union(OpenFlags::SQLITE_OPEN_CREATE)
    .union(OpenFlags::SQLITE_OPEN_URI)
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX);

const STORE_TABLES: &str = "
    CREATE TABLE IF NOT EXISTS meguri_stamp (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        crt INTEGER NOT NULL,
        scm INTEGER NOT NULL,
        col_mod INTEGER NOT NULL,
        clean INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS meguri_components (
        name TEXT PRIMARY KEY,
        version INTEGER NOT NULL,
        ready INTEGER NOT NULL,
        build INTEGER NOT NULL
    );";

/// How [`DerivedFile::open`] found the file (D5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// There was none.
    Created,
    Opened,
    /// The file was not a good store; it was deleted and created again.
    Recreated {
        reason: String,
    },
}

/// The derived store's file and its only writable connection, owned by the
/// derived writer (D6).
#[derive(Debug)]
pub struct DerivedFile {
    conn: Connection,
    path: PathBuf,
}

impl DerivedFile {
    /// Opens the store at `path`: creates it when missing, and deletes and
    /// creates it again when it is not a good store (D5.1).
    pub fn open(path: &Path) -> Result<(DerivedFile, Found), StorageError> {
        if !path.exists() {
            remove_sidecars(path)?;
            return Ok((create(path)?, Found::Created));
        }
        match open_existing(path) {
            Ok(file) => Ok((file, Found::Opened)),
            Err(Unusable(reason)) => {
                remove(path)?;
                remove_sidecars(path)?;
                Ok((create(path)?, Found::Recreated { reason }))
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `meguri_stamp` row; `None` in a new file.
    pub fn stamp(&self) -> Result<Option<Stamp>, StorageError> {
        read_stamp(&self.conn)
    }

    /// Every `meguri_components` row, by name.
    pub fn components(&self) -> Result<Vec<ComponentRecord>, StorageError> {
        read_components(&self.conn)
    }

    /// Begins a write transaction (`BEGIN IMMEDIATE`).
    pub fn begin(&mut self) -> Result<DerivedTx<'_>, StorageError> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(DerivedTx {
            conn: &self.conn,
            open: true,
            savepoints: std::cell::Cell::new(0),
        })
    }

    /// Copies the `-wal` into the file, as far as readers allow.
    pub fn checkpoint(&self) -> Result<(), StorageError> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// Checkpoints and closes; SQLite removes the `-wal` and `-shm` when no
    /// reader has the file attached.
    pub fn close(self) -> Result<(), StorageError> {
        let _ = self.checkpoint();
        self.conn.close().map_err(|(_, e)| StorageError::from(e))
    }
}

/// Why an existing file is not a good store.
struct Unusable(String);

impl From<rusqlite::Error> for Unusable {
    fn from(error: rusqlite::Error) -> Unusable {
        Unusable(error.to_string())
    }
}

fn connect(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(file_uri(path, None), READ_WRITE_CREATE)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    Ok(conn)
}

/// The settings of D2, after `page_size` (which only a new file takes).
fn settings(conn: &Connection) -> Result<(), StorageError> {
    collation::register(conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StorageError::Io {
            detail: format!("the derived store's journal mode stayed {mode}, not wal"),
        });
    }
    conn.execute_batch(&format!(
        "PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = {CACHE_SIZE_KIB};
         PRAGMA temp_store = MEMORY;"
    ))?;
    conn.set_prepared_statement_cache_capacity(64);
    Ok(())
}

fn create(path: &Path) -> Result<DerivedFile, StorageError> {
    let conn = connect(path)?;
    conn.execute_batch("PRAGMA page_size = 4096")?;
    settings(&conn)?;
    conn.execute_batch(&format!(
        "BEGIN IMMEDIATE;
         PRAGMA application_id = {APPLICATION_ID};
         PRAGMA user_version = {FORMAT};
         {STORE_TABLES}
         COMMIT;"
    ))?;
    Ok(DerivedFile {
        conn,
        path: path.to_path_buf(),
    })
}

fn open_existing(path: &Path) -> Result<DerivedFile, Unusable> {
    let conn = connect(path)?;
    settings(&conn).map_err(|e| Unusable(e.to_string()))?;
    let application_id: i64 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let format: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let tables: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table'", [], |row| {
        row.get(0)
    })?;
    if application_id != APPLICATION_ID {
        if tables > 0 {
            return Err(Unusable(format!(
                "application id {application_id:#x} is not the store's"
            )));
        }
        // An empty database (a zero-length file): make it the store.
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;
             PRAGMA application_id = {APPLICATION_ID};
             PRAGMA user_version = {FORMAT};
             {STORE_TABLES}
             COMMIT;"
        ))?;
    } else if format != FORMAT {
        return Err(Unusable(format!("format {format}, not {FORMAT}")));
    }
    let check: Vec<String> = {
        let mut statement = conn.prepare("PRAGMA quick_check(1)")?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    if check != ["ok"] {
        return Err(Unusable(format!("quick_check: {}", check.join("; "))));
    }
    conn.execute_batch(STORE_TABLES)?;
    Ok(DerivedFile {
        conn,
        path: path.to_path_buf(),
    })
}

fn remove(path: &Path) -> Result<(), StorageError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::Io {
            detail: format!("removing {}: {error}", path.display()),
        }),
    }
}

fn remove_sidecars(path: &Path) -> Result<(), StorageError> {
    for suffix in ["-wal", "-shm", "-journal"] {
        remove(&sibling(path, suffix))?;
    }
    Ok(())
}

fn read_stamp(conn: &Connection) -> Result<Option<Stamp>, StorageError> {
    Ok(conn
        .query_row(
            "SELECT crt, scm, col_mod, clean FROM meguri_stamp WHERE id = 1",
            [],
            |row| {
                Ok(Stamp {
                    mark: CollectionMark {
                        crt: row.get(0)?,
                        scm: row.get(1)?,
                        col_mod: row.get(2)?,
                    },
                    clean: row.get::<_, i64>(3)? != 0,
                })
            },
        )
        .optional()?)
}

fn read_components(conn: &Connection) -> Result<Vec<ComponentRecord>, StorageError> {
    let mut statement = conn.prepare("SELECT name, version, ready, build FROM meguri_components ORDER BY name")?;
    let records = statement.query_map([], |row| {
        Ok(ComponentRecord {
            name: row.get(0)?,
            version: row.get(1)?,
            ready: row.get::<_, i64>(2)? != 0,
            build: row.get(3)?,
        })
    })?;
    Ok(records.collect::<rusqlite::Result<_>>()?)
}

/// A write transaction on the derived file. Dropping it without
/// [`DerivedTx::commit`] rolls it back.
#[derive(Debug)]
pub struct DerivedTx<'f> {
    conn: &'f Connection,
    open: bool,
    savepoints: std::cell::Cell<u32>,
}

impl<'f> DerivedTx<'f> {
    /// Writes the stamp row.
    pub fn set_stamp(&self, stamp: &Stamp) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meguri_stamp (id, crt, scm, col_mod, clean) VALUES (1, ?1, ?2, ?3, ?4)",
            (
                stamp.mark.crt,
                stamp.mark.scm,
                stamp.mark.col_mod,
                i64::from(stamp.clean),
            ),
        )?;
        Ok(())
    }

    /// Sets a component's `ready`.
    pub fn set_ready(&self, name: &str, ready: bool) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE meguri_components SET ready = ?2 WHERE name = ?1",
            (name, i64::from(ready)),
        )?;
        Ok(())
    }

    /// The stamp row as this transaction sees it.
    pub fn stamp(&self) -> Result<Option<Stamp>, StorageError> {
        read_stamp(self.conn)
    }

    /// Gives every registered component its row and live tables, empties
    /// those whose row is new, whose version changed or whose table is
    /// missing, and drops what no registered component owns: other
    /// components' tables and rows, and staging tables (D5.1).
    pub fn sync_components(&self, registry: &Registry) -> Result<Vec<Synced>, StorageError> {
        rebuild::drop_staging_leftovers(self.conn)?;
        let records = read_components(self.conn)?;
        for record in &records {
            if registry.get(&record.name).is_none() {
                rebuild::drop_component(self.conn, &record.name)?;
                self.conn
                    .execute("DELETE FROM meguri_components WHERE name = ?1", [&record.name])?;
            }
        }
        let mut synced = Vec::new();
        for schema in registry.schemas() {
            let previous = records.iter().find(|r| r.name == schema.name);
            let reset = match previous {
                None => Some(RebuildReason::NewComponent),
                Some(record) if record.version != schema.version => {
                    Some(RebuildReason::VersionChanged { from: record.version })
                }
                Some(_) if !rebuild::tables_present(self.conn, schema)? => Some(RebuildReason::MissingTable),
                Some(_) => None,
            };
            let record = match (reset, previous) {
                (None, Some(record)) => record.clone(),
                _ => {
                    if previous.is_none() {
                        // Tables named like this one's, left without a row.
                        rebuild::drop_component(self.conn, &schema.name)?;
                    }
                    self.reset_component(schema)?;
                    read_components(self.conn)?
                        .into_iter()
                        .find(|r| r.name == schema.name)
                        .expect("reset writes the row")
                }
            };
            synced.push(Synced { record, reset });
        }
        Ok(synced)
    }

    /// Empties a component: its live and staging tables dropped, its live
    /// tables created empty with their indexes, `ready` 0, the next build
    /// number (D5.1, D10).
    pub fn reset_component(&self, schema: &ComponentSchema) -> Result<(), StorageError> {
        rebuild::reset(self.conn, schema)
    }

    /// Creates the empty staging tables of a rebuild, dropping any left.
    pub fn begin_staging(&self, schema: &ComponentSchema) -> Result<(), StorageError> {
        rebuild::begin_staging(self.conn, schema)
    }

    /// Drops the staging tables; the live tables stay.
    pub fn discard_staging(&self, schema: &ComponentSchema) -> Result<(), StorageError> {
        rebuild::discard_staging(self.conn, schema)
    }

    /// Ends a rebuild (D7): indexes on the staging tables, the live tables
    /// dropped, the staging tables renamed to the live names, `ready` 1.
    pub fn finish_staging(&self, schema: &ComponentSchema) -> Result<(), StorageError> {
        rebuild::finish_staging(self.conn, schema)
    }

    /// A component's tables of `target`, for its own SQL.
    pub fn tables<'t>(&'t self, schema: &'t ComponentSchema, target: Target) -> ComponentTables<'t> {
        ComponentTables::new(self.conn, schema, target)
    }

    /// Begins a savepoint inside the transaction.
    pub fn savepoint(&self) -> Result<TxSavepoint<'_>, StorageError> {
        let depth = self.savepoints.get() + 1;
        self.conn.execute_batch(&format!("SAVEPOINT derived_{depth}"))?;
        self.savepoints.set(depth);
        Ok(TxSavepoint {
            tx: self,
            depth,
            done: false,
        })
    }

    pub fn commit(mut self) -> Result<(), StorageError> {
        self.conn.execute_batch("COMMIT")?;
        self.open = false;
        Ok(())
    }

    pub fn rollback(mut self) -> Result<(), StorageError> {
        self.open = false;
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }
}

impl Drop for DerivedTx<'_> {
    fn drop(&mut self) {
        if self.open {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

/// A savepoint of a [`DerivedTx`]. Dropping it without release rolls it
/// back.
#[derive(Debug)]
pub struct TxSavepoint<'t> {
    tx: &'t DerivedTx<'t>,
    depth: u32,
    done: bool,
}

impl TxSavepoint<'_> {
    pub fn release(mut self) -> Result<(), StorageError> {
        self.done = true;
        self.tx.savepoints.set(self.depth - 1);
        self.tx.conn.execute_batch(&format!("RELEASE derived_{}", self.depth))?;
        Ok(())
    }

    pub fn rollback(mut self) -> Result<(), StorageError> {
        self.done = true;
        self.finish_rollback()
    }

    fn finish_rollback(&self) -> Result<(), StorageError> {
        self.tx.savepoints.set(self.depth - 1);
        self.tx
            .conn
            .execute_batch(&format!("ROLLBACK TO derived_{0}; RELEASE derived_{0}", self.depth))?;
        Ok(())
    }
}

impl Drop for TxSavepoint<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.finish_rollback();
        }
    }
}
