//! The day clock's test collections (`specs/sched/day-clock.md` D11, D14):
//! the new collection a day scenario starts from, made from a copy of the
//! schema-18 fixture through the storage open gate before the engine opens
//! it, so the engine's first day query is the one at open.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use meguri_engine::{Collection, Engine, EngineConfig, OpenConfig};
use meguri_model::clock::Clock;
use meguri_model::rng::SeededRandom;
use meguri_sched::timing::new_collection_timing;
use meguri_storage::raw::SqlValue;
use meguri_storage::{OpenOptions, open_collection};
use tempfile::TempDir;

use super::copy_fixture;

/// What the new collection holds of the day clock's state.
#[derive(Clone, Copy, Debug)]
pub enum Timing {
    /// `crt` and `creationOffset` from D11 at the clock's instant.
    New,
    /// D11's `crt` with no `creationOffset` (legacy timing, D4).
    Legacy,
}

/// Runs raw statements on a collection file through the storage open gate.
pub fn raw(path: &Path, clock: &dyn Clock, statements: &[(&str, Vec<SqlValue>)]) {
    let random = SeededRandom::new(1);
    let opened = open_collection(path, &OpenOptions::new(clock, &random)).unwrap();
    for (sql, params) in statements {
        opened
            .writer()
            .execute_raw(sql, params)
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    opened.close().unwrap();
}

/// A copy of the fixture made into a new collection created at the clock's
/// instant, as the day goldens show one: no notes, cards, review log, graves
/// or tags; `crt` and `creationOffset` (`usn` 0, `mtime_secs` 0) from D11;
/// no `rollover`, `localOffset` or `lastUnburied` key; `col.mod` 0.
pub fn new_collection(clock: &dyn Clock, timing: Timing) -> (TempDir, PathBuf) {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let now = clock.now_ms().div_euclid(1000);
    let created = new_collection_timing(now, clock);
    let mut statements: Vec<(&str, Vec<SqlValue>)> = [
        "DELETE FROM notes",
        "DELETE FROM cards",
        "DELETE FROM revlog",
        "DELETE FROM graves",
        "DELETE FROM tags",
    ]
    .into_iter()
    .map(|sql| (sql, Vec::new()))
    .collect();
    statements.push((
        "UPDATE col SET crt = ?1, mod = 0, scm = ?2",
        vec![SqlValue::Integer(created.crt), SqlValue::Integer(now * 1000)],
    ));
    statements.push((
        "DELETE FROM config WHERE KEY IN ('rollover', 'localOffset', 'lastUnburied', 'creationOffset')",
        Vec::new(),
    ));
    if let Timing::New = timing {
        statements.push((
            "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('creationOffset', 0, 0, CAST(?1 AS BLOB))",
            vec![SqlValue::Text(created.creation_offset.to_string())],
        ));
    }
    raw(&path, clock, &statements);
    (dir, path)
}

/// An engine with the collection at `path` open on `clock`.
pub fn open(path: &Path, clock: Arc<dyn Clock>, ports: meguri_engine::Ports) -> (Engine, Collection) {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let collection = engine
        .open_raw(
            path,
            OpenConfig::new(clock, Arc::new(SeededRandom::new(7))).ports(ports),
        )
        .unwrap();
    (engine, collection)
}
