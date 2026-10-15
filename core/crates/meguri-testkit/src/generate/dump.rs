//! The logical dump hash of a collection: SHA-256 over every row of the
//! twelve tables, in key order, each value with its storage class. Two
//! collections with the same rows have the same hash whatever their page
//! layout. Read through the test-only raw reads (`meguri_storage::raw`).

use std::path::Path;

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::SqlValue;
use meguri_storage::{OpenOptions, open_collection};
use sha2::{Digest, Sha256};

use super::GenError;

/// The tables and their key order; the last three are read a page at a
/// time by id.
const TABLES: [(&str, &str); 12] = [
    ("col", "id"),
    ("config", "\"KEY\""),
    ("deck_config", "id"),
    ("decks", "id"),
    ("notetypes", "id"),
    ("fields", "ntid, ord"),
    ("templates", "ntid, ord"),
    ("tags", "tag"),
    ("graves", "oid, type"),
    ("notes", "id"),
    ("cards", "id"),
    ("revlog", "id"),
];

const PAGED: [&str; 3] = ["notes", "cards", "revlog"];
const PAGE: i64 = 20_000;

fn feed(hasher: &mut Sha256, row: &[SqlValue]) {
    for value in row {
        match value {
            SqlValue::Null => hasher.update([0]),
            SqlValue::Integer(v) => {
                hasher.update([1]);
                hasher.update(v.to_le_bytes());
            }
            SqlValue::Real(v) => {
                hasher.update([2]);
                hasher.update(v.to_bits().to_le_bytes());
            }
            SqlValue::Text(v) => {
                hasher.update([3]);
                hasher.update((v.len() as u64).to_le_bytes());
                hasher.update(v.as_bytes());
            }
            SqlValue::Blob(v) => {
                hasher.update([4]);
                hasher.update((v.len() as u64).to_le_bytes());
                hasher.update(v);
            }
        }
    }
    hasher.update([0xff]);
}

/// The logical dump hash of the collection at `path`, lower-case hex.
pub fn logical_sha256(path: &Path) -> Result<String, GenError> {
    let clock = FixedClock::new(0, UtcOffset::UTC);
    let random = SeededRandom::new(0);
    let collection = open_collection(path, &OpenOptions::new(&clock, &random)).map_err(GenError::open)?;
    let writer = collection.writer();
    let mut hasher = Sha256::new();
    hash_tables(writer, &mut hasher)?;
    collection.close().map_err(GenError::storage)?;
    Ok(super::hex(&hasher.finalize()))
}

fn hash_tables(writer: &meguri_storage::Writer, hasher: &mut Sha256) -> Result<(), GenError> {
    for (table, order) in TABLES {
        hasher.update(table.as_bytes());
        hasher.update([0]);
        if PAGED.contains(&table) {
            let mut after = i64::MIN;
            loop {
                let rows = writer
                    .execute_raw(
                        &format!("SELECT * FROM {table} WHERE id > ?1 ORDER BY id LIMIT ?2"),
                        &[SqlValue::Integer(after), SqlValue::Integer(PAGE)],
                    )
                    .map_err(GenError::storage)?;
                for row in &rows.rows {
                    feed(hasher, row);
                }
                match rows.rows.last().and_then(|r| r.first()) {
                    Some(SqlValue::Integer(id)) if rows.rows.len() as i64 == PAGE => after = *id,
                    _ => break,
                }
            }
        } else {
            let rows = writer
                .execute_raw(&format!("SELECT * FROM {table} ORDER BY {order}"), &[])
                .map_err(GenError::storage)?;
            for row in &rows.rows {
                feed(hasher, row);
            }
        }
    }
    Ok(())
}
