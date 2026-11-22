//! Components (`storage/derived-store` D4): their schemas, the stored names
//! of their tables and indexes, and the tables a component writes through.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::derived::stamp::{ComponentRecord, RebuildReason};
use crate::error::StorageError;

/// Longest component, table or index name.
const MAX_NAME: usize = 40;
/// The prefix of staging tables (D2, D7).
pub(crate) const STAGING_PREFIX: &str = "meguri_build__";

/// One table of a component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableDef {
    pub name: String,
    pub kind: TableKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableKind {
    /// A column list and table options: `(a INTEGER PRIMARY KEY, b) WITHOUT ROWID`.
    Columns(String),
    /// A virtual table module with its arguments: `fts5(body)`.
    Virtual(String),
}

impl TableDef {
    pub fn columns(name: impl Into<String>, definition: impl Into<String>) -> TableDef {
        TableDef {
            name: name.into(),
            kind: TableKind::Columns(definition.into()),
        }
    }

    pub fn virtual_table(name: impl Into<String>, module: impl Into<String>) -> TableDef {
        TableDef {
            name: name.into(),
            kind: TableKind::Virtual(module.into()),
        }
    }

    fn create(&self, stored: &str) -> String {
        match &self.kind {
            TableKind::Columns(definition) => format!("CREATE TABLE \"{stored}\" {definition}"),
            TableKind::Virtual(module) => format!("CREATE VIRTUAL TABLE \"{stored}\" USING {module}"),
        }
    }
}

/// One index of a component's table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexDef {
    pub name: String,
    pub table: String,
    /// The indexed columns: `(nid, ord)`.
    pub columns: String,
    pub unique: bool,
}

impl IndexDef {
    pub fn new(name: impl Into<String>, table: impl Into<String>, columns: impl Into<String>) -> IndexDef {
        IndexDef {
            name: name.into(),
            table: table.into(),
            columns: columns.into(),
            unique: false,
        }
    }

    pub fn unique(name: impl Into<String>, table: impl Into<String>, columns: impl Into<String>) -> IndexDef {
        IndexDef {
            unique: true,
            ..IndexDef::new(name, table, columns)
        }
    }
}

/// What a component stores (D4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentSchema {
    pub name: String,
    pub version: u32,
    pub tables: Vec<TableDef>,
    pub indexes: Vec<IndexDef>,
    /// The record table naming the files the component owns beside the
    /// store (D1), by its `name` column; `None` for a component without
    /// files.
    pub files: Option<String>,
}

impl ComponentSchema {
    pub fn new(name: impl Into<String>, version: u32) -> ComponentSchema {
        ComponentSchema {
            name: name.into(),
            version,
            tables: Vec::new(),
            indexes: Vec::new(),
            files: None,
        }
    }

    pub fn table(mut self, table: TableDef) -> ComponentSchema {
        self.tables.push(table);
        self
    }

    pub fn index(mut self, index: IndexDef) -> ComponentSchema {
        self.indexes.push(index);
        self
    }

    /// The component owns files beside the store, named by the `name`
    /// column of its table `table` (D1): a file is live only while a row of
    /// that table names it.
    pub fn files(mut self, table: impl Into<String>) -> ComponentSchema {
        self.files = Some(table.into());
        self
    }

    /// The stored name of `table` in `target`.
    pub(crate) fn stored_table(&self, table: &str, target: Target) -> String {
        match target {
            Target::Live => format!("{}__{table}", self.name),
            Target::Staging => format!("{STAGING_PREFIX}{}__{table}", self.name),
        }
    }

    /// The stored name of `index` for the tables of `build`.
    pub(crate) fn stored_index(&self, index: &str, build: i64) -> String {
        format!("{}__{index}__{build}", self.name)
    }

    /// The statements creating the tables of `target`.
    pub(crate) fn create_tables(&self, target: Target) -> Vec<String> {
        self.tables
            .iter()
            .map(|table| table.create(&self.stored_table(&table.name, target)))
            .collect()
    }

    /// The statements creating the indexes on the tables of `target`, named
    /// for `build`.
    pub(crate) fn create_indexes(&self, target: Target, build: i64) -> Vec<String> {
        self.indexes
            .iter()
            .map(|index| {
                format!(
                    "CREATE {}INDEX \"{}\" ON \"{}\" {}",
                    if index.unique { "UNIQUE " } else { "" },
                    self.stored_index(&index.name, build),
                    self.stored_table(&index.table, target),
                    index.columns
                )
            })
            .collect()
    }

    fn check(&self) -> Result<(), StorageError> {
        check_name("component", &self.name)?;
        if self.name.starts_with("meguri") {
            return Err(refused(format!("component name {:?} starts with meguri", self.name)));
        }
        if self.version == 0 {
            return Err(refused(format!("component {}: version 0", self.name)));
        }
        if self.tables.is_empty() {
            return Err(refused(format!("component {} has no table", self.name)));
        }
        let mut tables = HashSet::new();
        for table in &self.tables {
            check_name("table", &table.name)?;
            if !tables.insert(table.name.as_str()) {
                return Err(refused(format!("component {}: table {} twice", self.name, table.name)));
            }
        }
        let mut indexes = HashSet::new();
        for index in &self.indexes {
            check_name("index", &index.name)?;
            if !indexes.insert(index.name.as_str()) {
                return Err(refused(format!("component {}: index {} twice", self.name, index.name)));
            }
            if !tables.contains(index.table.as_str()) {
                return Err(refused(format!(
                    "component {}: index {} names no table of it ({})",
                    self.name, index.name, index.table
                )));
            }
        }
        if let Some(files) = &self.files
            && !tables.contains(files.as_str())
        {
            return Err(refused(format!(
                "component {}: its file records name no table of it ({files})",
                self.name
            )));
        }
        Ok(())
    }
}

fn refused(detail: String) -> StorageError {
    StorageError::Derived { detail }
}

/// `[a-z][a-z0-9_]*`, at most 40, without `__` (D4).
fn check_name(what: &str, name: &str) -> Result<(), StorageError> {
    let mut chars = name.chars();
    let valid = matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
        && name.len() <= MAX_NAME
        && !name.contains("__");
    if valid {
        Ok(())
    } else {
        Err(refused(format!(
            "{what} name {name:?} is not [a-z][a-z0-9_]* of at most {MAX_NAME} without __"
        )))
    }
}

/// The registered components, in registration order (D4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    schemas: Vec<ComponentSchema>,
}

impl Registry {
    /// Checks every name and that no component is registered twice.
    pub fn new(schemas: Vec<ComponentSchema>) -> Result<Registry, StorageError> {
        let mut names = HashSet::new();
        for schema in &schemas {
            schema.check()?;
            if !names.insert(schema.name.as_str()) {
                return Err(refused(format!("component {} registered twice", schema.name)));
            }
        }
        Ok(Registry { schemas })
    }

    pub fn schemas(&self) -> &[ComponentSchema] {
        &self.schemas
    }

    pub fn get(&self, name: &str) -> Option<&ComponentSchema> {
        self.schemas.iter().find(|schema| schema.name == name)
    }
}

/// A component's row after the components were synced at bind (D5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Synced {
    pub record: ComponentRecord,
    /// Why its tables were emptied; `None` when they were kept.
    pub reset: Option<RebuildReason>,
}

/// Which tables a write goes to: the live ones, or a rebuild's staging
/// tables (D7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Live,
    Staging,
}

/// A component's tables inside a derived transaction: the connection, and
/// the stored name of each of its tables for the target being written.
#[derive(Debug)]
pub struct ComponentTables<'a> {
    conn: &'a Connection,
    names: Vec<(&'a str, String)>,
}

impl<'a> ComponentTables<'a> {
    pub(crate) fn new(conn: &'a Connection, schema: &'a ComponentSchema, target: Target) -> ComponentTables<'a> {
        ComponentTables {
            conn,
            names: schema
                .tables
                .iter()
                .map(|table| (table.name.as_str(), schema.stored_table(&table.name, target)))
                .collect(),
        }
    }

    /// The derived file's connection, inside the transaction.
    pub fn conn(&self) -> &'a Connection {
        self.conn
    }

    /// The stored name of the component's table `name`.
    pub fn table(&self, name: &str) -> Result<&str, StorageError> {
        self.names
            .iter()
            .find(|(logical, _)| *logical == name)
            .map(|(_, stored)| stored.as_str())
            .ok_or_else(|| refused(format!("the component has no table {name}")))
    }
}
