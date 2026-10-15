//! Typed reads and writes of every table of `collection.anki2`
//! (`specs/model/row-records.md`, `specs/storage/ids-usn-mtime.md`): the
//! column layout of each table, stored values by storage class, row keys and
//! images for the journal, and the typed read errors (R10).
//!
//! Reads follow row-records R1 with each table's tolerances. Writes go
//! through a [`crate::write_scope::WriteScope`], which compares, stamps and
//! journals them.

pub mod cards;
pub mod col;
pub mod config;
pub mod deck_config;
pub mod decks;
pub mod graves;
pub mod notes;
pub mod notetypes;
pub mod revlog;
pub mod tags;

use std::fmt;

use rusqlite::types::{ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, params_from_iter};

use crate::error::StorageError;

/// The twelve tables of a schema-18 collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Table {
    Col,
    Notes,
    Cards,
    Revlog,
    Graves,
    DeckConfig,
    Config,
    Tags,
    Fields,
    Templates,
    Notetypes,
    Decks,
}

/// Where a table keeps its stamps (`ids-usn-mtime` S1).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stamps {
    pub usn: Option<usize>,
    /// The modification time column, in epoch seconds.
    pub mtime: Option<usize>,
}

impl Table {
    pub const ALL: [Table; 12] = [
        Table::Col,
        Table::Notes,
        Table::Cards,
        Table::Revlog,
        Table::Graves,
        Table::DeckConfig,
        Table::Config,
        Table::Tags,
        Table::Fields,
        Table::Templates,
        Table::Notetypes,
        Table::Decks,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Table::Col => "col",
            Table::Notes => "notes",
            Table::Cards => "cards",
            Table::Revlog => "revlog",
            Table::Graves => "graves",
            Table::DeckConfig => "deck_config",
            Table::Config => "config",
            Table::Tags => "tags",
            Table::Fields => "fields",
            Table::Templates => "templates",
            Table::Notetypes => "notetypes",
            Table::Decks => "decks",
        }
    }

    /// The columns in the order of `storage/schema-18` S2. The key columns
    /// come first. Statements name them in any letter case: schemas 14 to
    /// 16 spelled config's key column `key` (`config-table` C1).
    pub fn columns(self) -> &'static [&'static str] {
        match self {
            Table::Col => &[
                "id", "crt", "mod", "scm", "ver", "dty", "usn", "ls", "conf", "models", "decks", "dconf", "tags",
            ],
            Table::Notes => &[
                "id", "guid", "mid", "mod", "usn", "tags", "flds", "sfld", "csum", "flags", "data",
            ],
            Table::Cards => &[
                "id", "nid", "did", "ord", "mod", "usn", "type", "queue", "due", "ivl", "factor", "reps", "lapses",
                "left", "odue", "odid", "flags", "data",
            ],
            Table::Revlog => &["id", "cid", "usn", "ease", "ivl", "lastIvl", "factor", "time", "type"],
            Table::Graves => &["oid", "type", "usn"],
            Table::DeckConfig => &["id", "name", "mtime_secs", "usn", "config"],
            Table::Config => &["KEY", "usn", "mtime_secs", "val"],
            Table::Tags => &["tag", "usn", "collapsed", "config"],
            Table::Fields => &["ntid", "ord", "name", "config"],
            Table::Templates => &["ntid", "ord", "name", "mtime_secs", "usn", "config"],
            Table::Notetypes => &["id", "name", "mtime_secs", "usn", "config"],
            Table::Decks => &["id", "name", "mtime_secs", "usn", "common", "kind"],
        }
    }

    /// How many leading columns make the key.
    pub fn key_len(self) -> usize {
        match self {
            Table::Graves | Table::Fields | Table::Templates => 2,
            _ => 1,
        }
    }

    /// The tables whose new rows get an allocated id (`ids-usn-mtime` I1).
    pub fn allocates_ids(self) -> bool {
        matches!(
            self,
            Table::Notes | Table::Cards | Table::Revlog | Table::Decks | Table::DeckConfig | Table::Notetypes
        )
    }

    /// S1: `templates` and `fields` change with their note type, and `col`
    /// gets `mod` when an operation commits (S5); none of them is stamped
    /// row by row.
    pub(crate) fn stamps(self) -> Stamps {
        let (usn, mtime) = match self {
            Table::Notes => (Some(4), Some(3)),
            Table::Cards => (Some(5), Some(4)),
            Table::Revlog => (Some(2), None),
            Table::Graves => (Some(2), None),
            Table::DeckConfig | Table::Notetypes | Table::Decks => (Some(3), Some(2)),
            Table::Config => (Some(1), Some(2)),
            Table::Tags => (Some(1), None),
            Table::Col | Table::Fields | Table::Templates => (None, None),
        };
        Stamps { usn, mtime }
    }

    /// The column holding a row's `usn`, for the tables whose rows carry
    /// one as their own (S1: not `col`, whose `usn` is the sync's, nor
    /// `templates`, which change with their note type).
    pub fn usn_column(self) -> Option<usize> {
        self.stamps().usn
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One stored value by its SQLite storage class. Text keeps its bytes: a
/// value another client stored need not be UTF-8.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl Cell {
    pub fn text(text: impl Into<String>) -> Cell {
        Cell::Text(text.into().into_bytes())
    }

    pub(crate) fn from_ref(value: ValueRef<'_>) -> Cell {
        match value {
            ValueRef::Null => Cell::Null,
            ValueRef::Integer(v) => Cell::Integer(v),
            ValueRef::Real(v) => Cell::Real(v),
            ValueRef::Text(v) => Cell::Text(v.to_vec()),
            ValueRef::Blob(v) => Cell::Blob(v.to_vec()),
        }
    }

    /// The heap bytes the value holds.
    pub(crate) fn heap_bytes(&self) -> usize {
        match self {
            Cell::Text(v) | Cell::Blob(v) => v.capacity(),
            _ => 0,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Cell::Integer(v) => Some(*v),
            _ => None,
        }
    }
}

impl ToSql for Cell {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Cell::Null => ValueRef::Null,
            Cell::Integer(v) => ValueRef::Integer(*v),
            Cell::Real(v) => ValueRef::Real(*v),
            Cell::Text(v) => ValueRef::Text(v),
            Cell::Blob(v) => ValueRef::Blob(v),
        }))
    }
}

/// A row's key: the id, the pair (`graves`: oid and type; `fields` and
/// `templates`: note type id and ordinal), or the text (`tags`, `config`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowKey {
    Id(i64),
    Pair(i64, i64),
    Text(Vec<u8>),
}

impl RowKey {
    pub fn text(text: &str) -> RowKey {
        RowKey::Text(text.as_bytes().to_vec())
    }

    pub(crate) fn cells(&self) -> Vec<Cell> {
        match self {
            RowKey::Id(id) => vec![Cell::Integer(*id)],
            RowKey::Pair(a, b) => vec![Cell::Integer(*a), Cell::Integer(*b)],
            RowKey::Text(t) => vec![Cell::Text(t.clone())],
        }
    }

    /// The key of a row from its leading cells.
    pub(crate) fn of(table: Table, cells: &[Cell]) -> RowKey {
        match (table.key_len(), cells) {
            (2, [Cell::Integer(a), Cell::Integer(b), ..]) => RowKey::Pair(*a, *b),
            (1, [Cell::Integer(id), ..]) => RowKey::Id(*id),
            (1, [Cell::Text(t), ..]) => RowKey::Text(t.clone()),
            // A key of another storage class: kept by its text form, which
            // never collides with the typed keys above.
            (_, cells) => RowKey::Text(format!("{:?}", &cells[..table.key_len().min(cells.len())]).into_bytes()),
        }
    }
}

impl fmt::Display for RowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowKey::Id(id) => write!(f, "{id}"),
            RowKey::Pair(a, b) => write!(f, "{a},{b}"),
            RowKey::Text(t) => f.write_str(&String::from_utf8_lossy(t)),
        }
    }
}

/// A whole stored row: one cell per column of [`Table::columns`].
pub type RowImage = Vec<Cell>;

/// A row that cannot be read under row-records R1 to R9 (R10): the table,
/// the column, the row's key and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowError {
    pub table: &'static str,
    pub column: &'static str,
    pub key: String,
    pub reason: String,
}

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{} of row {}: {}", self.table, self.column, self.key, self.reason)
    }
}

impl std::error::Error for RowError {}

/// Reads the cells of one stored row, with the table and key for errors.
pub(crate) struct RowCells<'a> {
    pub table: Table,
    pub cells: &'a [Cell],
}

impl RowCells<'_> {
    fn error(&self, index: usize, reason: impl Into<String>) -> RowError {
        RowError {
            table: self.table.name(),
            column: self.table.columns()[index],
            key: RowKey::of(self.table, self.cells).to_string(),
            reason: reason.into(),
        }
    }

    fn cell(&self, index: usize) -> &Cell {
        &self.cells[index]
    }

    /// R1: an INTEGER that fits `T`.
    pub fn int<T: TryFrom<i64>>(&self, index: usize) -> Result<T, RowError> {
        match self.cell(index) {
            Cell::Integer(v) => T::try_from(*v).map_err(|_| self.error(index, format!("{v} is out of range"))),
            other => Err(self.error(index, format!("not an integer: {}", class(other)))),
        }
    }

    /// An INTEGER as is, or a default for other storage classes and values
    /// out of `T`'s range (the R4 and R5 tolerances).
    pub fn int_or<T: TryFrom<i64>>(&self, index: usize, default: T) -> T {
        match self.cell(index) {
            Cell::Integer(v) => T::try_from(*v).unwrap_or(default),
            _ => default,
        }
    }

    /// The raw INTEGER, for the low-8-bit readings of R4 and R5.
    pub fn raw_int(&self, index: usize) -> Result<i64, RowError> {
        self.int::<i64>(index)
    }

    /// R1: TEXT, as a string; bytes that are not UTF-8 make the row
    /// unreadable.
    pub fn text(&self, index: usize) -> Result<String, RowError> {
        match self.cell(index) {
            Cell::Text(bytes) => String::from_utf8(bytes.clone()).map_err(|_| self.error(index, "text is not UTF-8")),
            other => Err(self.error(index, format!("not text: {}", class(other)))),
        }
    }

    /// TEXT, or a default for any other storage class.
    pub fn text_or_empty(&self, index: usize) -> String {
        match self.cell(index) {
            Cell::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => String::new(),
        }
    }

    /// A BLOB's bytes.
    pub fn blob(&self, index: usize) -> Result<&[u8], RowError> {
        match self.cell(index) {
            Cell::Blob(bytes) => Ok(bytes),
            other => Err(self.error(index, format!("not a blob: {}", class(other)))),
        }
    }

    pub fn wrap<T, E: fmt::Display>(&self, index: usize, result: Result<T, E>) -> Result<T, RowError> {
        result.map_err(|e| self.error(index, e.to_string()))
    }

    pub fn fail<T>(&self, index: usize, reason: impl Into<String>) -> Result<T, RowError> {
        Err(self.error(index, reason))
    }
}

fn class(cell: &Cell) -> &'static str {
    match cell {
        Cell::Null => "NULL",
        Cell::Integer(_) => "INTEGER",
        Cell::Real(_) => "REAL",
        Cell::Text(_) => "TEXT",
        Cell::Blob(_) => "BLOB",
    }
}

/// `"a", "b", ...`: column names quoted, since some are keywords (`left`,
/// `type`).
fn column_list(table: Table) -> String {
    table
        .columns()
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `"k0" = ?1 AND "k1" = ?2`: the key columns with their own collation, as
/// the primary key compares them (`tags` by `unicase`).
fn key_condition(table: Table, first_param: usize) -> String {
    table.columns()[..table.key_len()]
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", first_param + i))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// The stored row with `key`, every column by storage class.
pub(crate) fn select_image(conn: &Connection, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
    let sql = format!(
        "SELECT {} FROM {} WHERE {}",
        column_list(table),
        table.name(),
        key_condition(table, 1)
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let width = table.columns().len();
    Ok(statement
        .query_row(params_from_iter(key.cells()), |row| {
            (0..width).map(|i| row.get_ref(i).map(Cell::from_ref)).collect()
        })
        .optional()?)
}

/// The stored row with the key of `new`, and whether it equals `new` in
/// every column (S4). Values compare as SQLite stores them: `new`'s values
/// get each column's affinity, text compares byte for byte, and a value of
/// another storage class never equals.
pub(crate) fn select_compare(
    conn: &Connection,
    table: Table,
    new: &[Cell],
) -> Result<Option<(RowImage, bool)>, StorageError> {
    let same = table
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| format!("(\"{c}\" IS ?{} COLLATE BINARY)", i + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "SELECT {}, {same} FROM {} WHERE {}",
        column_list(table),
        table.name(),
        key_condition(table, 1)
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let width = table.columns().len();
    Ok(statement
        .query_row(params_from_iter(new.iter()), |row| {
            let image = (0..width)
                .map(|i| row.get_ref(i).map(Cell::from_ref))
                .collect::<rusqlite::Result<RowImage>>()?;
            let same: Option<i64> = row.get(width)?;
            Ok((image, same == Some(1)))
        })
        .optional()?)
}

pub(crate) fn insert(conn: &Connection, table: Table, cells: &[Cell]) -> Result<(), StorageError> {
    let params = (1..=cells.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({params})",
        table.name(),
        column_list(table)
    );
    conn.prepare_cached(&sql)?.execute(params_from_iter(cells.iter()))?;
    Ok(())
}

/// Rewrites every column of the row whose key is `key` (the stored key:
/// for `tags`, a key that differs only in letter case is the same row).
pub(crate) fn update(conn: &Connection, table: Table, key: &RowKey, cells: &[Cell]) -> Result<(), StorageError> {
    let set = table
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE {} SET {set} WHERE {}",
        table.name(),
        key_condition(table, cells.len() + 1)
    );
    let params = cells.iter().cloned().chain(key.cells());
    conn.prepare_cached(&sql)?.execute(params_from_iter(params))?;
    Ok(())
}

pub(crate) fn delete(conn: &Connection, table: Table, key: &RowKey) -> Result<(), StorageError> {
    let sql = format!("DELETE FROM {} WHERE {}", table.name(), key_condition(table, 1));
    conn.prepare_cached(&sql)?.execute(params_from_iter(key.cells()))?;
    Ok(())
}

/// Every stored row of `table` that `filter` (an SQL condition with
/// positional parameters) selects, in key order.
pub(crate) fn select_where(
    conn: &Connection,
    table: Table,
    filter: &str,
    params: &[Cell],
) -> Result<Vec<RowImage>, StorageError> {
    let order = table.columns()[..table.key_len()]
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM {} WHERE {filter} ORDER BY {order}",
        column_list(table),
        table.name()
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let width = table.columns().len();
    let rows = statement.query_map(params_from_iter(params.iter()), |row| {
        (0..width).map(|i| row.get_ref(i).map(Cell::from_ref)).collect()
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<RowImage>>>()?)
}

/// A typed record of one table: its cells to write and its reading of
/// stored cells.
pub trait Record: Sized {
    const TABLE: Table;

    /// The cells to write, in column order.
    fn to_cells(&self) -> RowImage;

    /// Reads a stored row under row-records R1 to R9.
    fn from_cells(cells: &[Cell]) -> Result<Self, RowError>;
}

/// Reads one typed record by key.
pub(crate) fn get<R: Record>(conn: &Connection, key: &RowKey) -> Result<Option<R>, StorageError> {
    match select_image(conn, R::TABLE, key)? {
        Some(cells) => Ok(Some(R::from_cells(&cells)?)),
        None => Ok(None),
    }
}
