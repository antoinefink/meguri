//! The expected-object check of the open gate and the schema guard
//! (`storage/schema-18` S6, R2016). Objects are compared by type and name
//! (and table, for indexes), never by their SQL text: older clients stored
//! other spacing and letter case.

use rusqlite::Connection;

use crate::connections::{Reader, Writer};
use crate::error::StorageError;

/// The twelve tables of S2.
pub const REQUIRED_TABLES: [&str; 12] = [
    "col",
    "notes",
    "cards",
    "revlog",
    "graves",
    "deck_config",
    "config",
    "tags",
    "fields",
    "templates",
    "notetypes",
    "decks",
];

/// The sixteen indexes of S3, each with its table.
pub const REQUIRED_INDEXES: [(&str, &str); 16] = [
    ("ix_notes_usn", "notes"),
    ("ix_cards_usn", "cards"),
    ("ix_revlog_usn", "revlog"),
    ("ix_cards_nid", "cards"),
    ("ix_cards_sched", "cards"),
    ("ix_revlog_cid", "revlog"),
    ("ix_notes_csum", "notes"),
    ("idx_fields_name_ntid", "fields"),
    ("idx_templates_name_ntid", "templates"),
    ("idx_templates_usn", "templates"),
    ("idx_notetypes_name", "notetypes"),
    ("idx_notetypes_usn", "notetypes"),
    ("idx_decks_name", "decks"),
    ("idx_notes_mid", "notes"),
    ("idx_cards_odid", "cards"),
    ("idx_graves_pending", "graves"),
];

/// One row of the main database's `sqlite_master`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaObject {
    /// `type`: `table`, `index`, `view` or `trigger`.
    pub kind: String,
    pub name: String,
    pub tbl_name: String,
    /// `sql` as stored; `None` for objects SQLite stores without text
    /// (automatic indexes).
    pub sql: Option<String>,
}

impl SchemaObject {
    /// SQLite's own objects (S6): its statistics tables, `sqlite_sequence`
    /// and automatic indexes.
    pub fn is_sqlite_own(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        matches!(
            name.as_str(),
            "sqlite_stat1" | "sqlite_stat2" | "sqlite_stat3" | "sqlite_stat4" | "sqlite_sequence"
        ) || (self.kind == "index" && name.starts_with("sqlite_autoindex_"))
    }

    pub(crate) fn is_required(&self) -> bool {
        match self.kind.as_str() {
            "table" => REQUIRED_TABLES.iter().any(|t| self.name.eq_ignore_ascii_case(t)),
            "index" => REQUIRED_INDEXES
                .iter()
                .any(|(i, t)| self.name.eq_ignore_ascii_case(i) && self.tbl_name.eq_ignore_ascii_case(t)),
            _ => false,
        }
    }
}

/// What S6.1 reports about a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExpectedObjects {
    /// Required tables that are absent: the file is not a collection.
    pub missing_tables: Vec<String>,
    /// Required indexes that are absent: the open still succeeds; Check
    /// Database reports them (U128). Meguri does not recreate them.
    pub missing_indexes: Vec<String>,
    /// Objects neither required nor SQLite's own (an add-on's table, a view,
    /// a trigger): kept as they are, never dropped or rewritten.
    pub foreign: Vec<SchemaObject>,
}

/// Every row of `sqlite_master` (no table scan of collection data).
pub(crate) fn read(conn: &Connection) -> rusqlite::Result<Vec<SchemaObject>> {
    let mut statement = conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master")?;
    let rows = statement.query_map([], |row| {
        Ok(SchemaObject {
            kind: row.get(0)?,
            name: row.get(1)?,
            tbl_name: row.get(2)?,
            sql: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// S6.1 over a list of `sqlite_master` rows.
pub fn check(objects: &[SchemaObject]) -> ExpectedObjects {
    let has = |kind: &str, name: &str, table: Option<&str>| {
        objects.iter().any(|o| {
            o.kind == kind
                && o.name.eq_ignore_ascii_case(name)
                && table.is_none_or(|t| o.tbl_name.eq_ignore_ascii_case(t))
        })
    };
    ExpectedObjects {
        missing_tables: REQUIRED_TABLES
            .iter()
            .filter(|t| !has("table", t, None))
            .map(|t| (*t).to_owned())
            .collect(),
        missing_indexes: REQUIRED_INDEXES
            .iter()
            .filter(|(i, t)| !has("index", i, Some(t)))
            .map(|(i, _)| (*i).to_owned())
            .collect(),
        foreign: objects
            .iter()
            .filter(|o| !o.is_required() && !o.is_sqlite_own())
            .cloned()
            .collect(),
    }
}

impl Writer {
    /// The rows of the collection file's `sqlite_master` (temporary objects
    /// are not in it).
    pub fn schema_objects(&self) -> Result<Vec<SchemaObject>, StorageError> {
        Ok(read(&self.conn)?)
    }
}

impl Reader {
    /// The collection schema on a query connection, including foreign objects.
    pub fn schema_objects(&self) -> Result<Vec<SchemaObject>, StorageError> {
        Ok(read(&self.conn)?)
    }
}
