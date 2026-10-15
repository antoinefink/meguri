//! Shared helpers: fixture copies in temporary directories, file digests and
//! directory listings, and raw SQL shortcuts. Fixtures are read only to copy
//! them; SQLite only ever opens the copies.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{OpenError, OpenOptions, OpenedCollection, Reader, Writer, open_collection};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// 2030-01-15T10:00:00Z, the storage scenarios' clock.
pub const NOW_MS: i64 = 1_894_701_600_000;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures/storage")
        .join(name)
}

/// A temporary directory holding a copy of the fixture as
/// `collection.anki2`.
pub fn copy_fixture(name: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(fixture(name), &path).unwrap();
    (dir, path)
}

pub fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn listing(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

pub fn clock() -> FixedClock {
    FixedClock::new(NOW_MS, UtcOffset::UTC)
}

pub fn open(path: &Path) -> Result<OpenedCollection, OpenError> {
    let clock = clock();
    let random = SeededRandom::new(7);
    open_collection(path, &OpenOptions::new(&clock, &random))
}

pub fn exec(writer: &Writer, sql: &str) -> RawRows {
    writer.execute_raw(sql, &[]).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

pub fn query(reader: &Reader, sql: &str) -> RawRows {
    reader.execute_raw(sql, &[]).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// The only value of a one-row, one-column result.
pub fn single(rows: &RawRows) -> SqlValue {
    assert_eq!(rows.rows.len(), 1, "{rows:?}");
    assert_eq!(rows.rows[0].len(), 1, "{rows:?}");
    rows.rows[0][0].clone()
}

pub fn int(rows: &RawRows) -> i64 {
    match single(rows) {
        SqlValue::Integer(v) => v,
        other => panic!("not an integer: {other:?}"),
    }
}

pub fn text(rows: &RawRows) -> String {
    match single(rows) {
        SqlValue::Text(v) => v,
        other => panic!("not text: {other:?}"),
    }
}

/// `(type, name)` of every object in `sqlite_master`, names lower-cased:
/// SQLite names are case-insensitive, and releases at schema 17 stored the
/// table `FIELDS`.
pub fn object_set(writer: &Writer) -> BTreeSet<(String, String)> {
    writer
        .schema_objects()
        .unwrap()
        .into_iter()
        .map(|o| (o.kind, o.name.to_ascii_lowercase()))
        .collect()
}

/// The required tables and indexes of `storage/schema-18` S2 and S3 plus the
/// statistics tables of S5: the objects of a file Anki or Meguri created
/// (S6.2). Written out from the spec, not taken from the crate.
pub fn anki_made_objects() -> BTreeSet<(String, String)> {
    let tables = [
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
        "sqlite_stat1",
        "sqlite_stat4",
    ];
    let indexes = [
        "ix_notes_usn",
        "ix_cards_usn",
        "ix_revlog_usn",
        "ix_cards_nid",
        "ix_cards_sched",
        "ix_revlog_cid",
        "ix_notes_csum",
        "idx_fields_name_ntid",
        "idx_templates_name_ntid",
        "idx_templates_usn",
        "idx_notetypes_name",
        "idx_notetypes_usn",
        "idx_decks_name",
        "idx_notes_mid",
        "idx_cards_odid",
        "idx_graves_pending",
    ];
    tables
        .iter()
        .map(|t| ("table".to_owned(), (*t).to_owned()))
        .chain(indexes.iter().map(|i| ("index".to_owned(), (*i).to_owned())))
        .collect()
}

/// A plain SQLite connection to a test's own file (never a fixture under
/// `conformance/`), for building files the gate must handle.
pub fn plain(path: &Path) -> meguri_storage::rusqlite::Connection {
    meguri_storage::rusqlite::Connection::open(path).unwrap()
}
