//! The day clock's test collections (`specs/sched/day-clock.md` D11, D14):
//! real U28 bootstrap before the engine opens it, so the engine's first
//! day query is the one at open.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use meguri_engine::{Collection, Engine, EngineConfig, OpenConfig};
use meguri_model::clock::Clock;
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::SqlValue;
use meguri_storage::{OpenOptions, open_collection};
use tempfile::TempDir;

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

/// The real bootstrap at the scenario clock; legacy tests deliberately
/// remove only the creation offset before the normal engine open.
pub fn new_collection(clock: &dyn Clock, timing: Timing) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    meguri_engine::bootstrap::create(&path, clock, &SeededRandom::new(7)).unwrap();
    if let Timing::Legacy = timing {
        raw(
            &path,
            clock,
            &[("DELETE FROM config WHERE KEY = 'creationOffset'", Vec::new())],
        );
    }
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
