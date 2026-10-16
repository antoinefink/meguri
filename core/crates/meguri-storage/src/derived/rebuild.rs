//! A component's tables on disk (`storage/derived-store` D5.1, D7): emptying
//! them, dropping what no component owns, and a rebuild's staging tables and
//! their swap into the live names.

use rusqlite::{Connection, OptionalExtension};

use crate::derived::registry::{ComponentSchema, STAGING_PREFIX, Target};
use crate::error::StorageError;

/// Drops every table whose name starts with `prefix`, virtual tables first
/// (dropping one drops its shadow tables). Their indexes go with them.
fn drop_prefixed(conn: &Connection, prefix: &str) -> Result<(), StorageError> {
    let tables: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND substr(name, 1, length(?1)) = ?1
             ORDER BY coalesce(substr(upper(sql), 1, 14) = 'CREATE VIRTUAL', 0) DESC, name",
        )?;
        statement
            .query_map([prefix], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for table in tables {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\""))?;
    }
    Ok(())
}

/// Drops the live tables of the component `name`.
pub(crate) fn drop_component(conn: &Connection, name: &str) -> Result<(), StorageError> {
    drop_prefixed(conn, &format!("{name}__"))
}

/// Drops every staging table of every component.
pub(crate) fn drop_staging_leftovers(conn: &Connection) -> Result<(), StorageError> {
    drop_prefixed(conn, STAGING_PREFIX)
}

/// Whether every live table of `schema` exists.
pub(crate) fn tables_present(conn: &Connection, schema: &ComponentSchema) -> Result<bool, StorageError> {
    for table in &schema.tables {
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [schema.stored_table(&table.name, Target::Live)],
                |row| row.get(0),
            )
            .optional()?;
        if found.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The build number after the component's current one.
fn next_build(conn: &Connection, name: &str) -> Result<i64, StorageError> {
    let current: Option<i64> = conn
        .query_row("SELECT build FROM meguri_components WHERE name = ?1", [name], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(current.unwrap_or(0) + 1)
}

fn execute_all(conn: &Connection, statements: &[String]) -> Result<(), StorageError> {
    for statement in statements {
        conn.execute_batch(statement)?;
    }
    Ok(())
}

pub(crate) fn reset(conn: &Connection, schema: &ComponentSchema) -> Result<(), StorageError> {
    let build = next_build(conn, &schema.name)?;
    discard_staging(conn, schema)?;
    drop_component(conn, &schema.name)?;
    execute_all(conn, &schema.create_tables(Target::Live))?;
    execute_all(conn, &schema.create_indexes(Target::Live, build))?;
    conn.execute(
        "INSERT OR REPLACE INTO meguri_components (name, version, ready, build) VALUES (?1, ?2, 0, ?3)",
        (&schema.name, schema.version, build),
    )?;
    Ok(())
}

pub(crate) fn begin_staging(conn: &Connection, schema: &ComponentSchema) -> Result<(), StorageError> {
    discard_staging(conn, schema)?;
    execute_all(conn, &schema.create_tables(Target::Staging))
}

pub(crate) fn discard_staging(conn: &Connection, schema: &ComponentSchema) -> Result<(), StorageError> {
    drop_prefixed(conn, &format!("{STAGING_PREFIX}{}__", schema.name))
}

pub(crate) fn finish_staging(conn: &Connection, schema: &ComponentSchema) -> Result<(), StorageError> {
    let build = next_build(conn, &schema.name)?;
    // Building the indexes after the rows is faster than keeping them up to
    // date during the rebuild.
    execute_all(conn, &schema.create_indexes(Target::Staging, build))?;
    drop_component(conn, &schema.name)?;
    for table in &schema.tables {
        conn.execute_batch(&format!(
            "ALTER TABLE \"{}\" RENAME TO \"{}\"",
            schema.stored_table(&table.name, Target::Staging),
            schema.stored_table(&table.name, Target::Live)
        ))?;
    }
    conn.execute(
        "INSERT OR REPLACE INTO meguri_components (name, version, ready, build) VALUES (?1, ?2, 1, ?3)",
        (&schema.name, schema.version, build),
    )?;
    Ok(())
}
