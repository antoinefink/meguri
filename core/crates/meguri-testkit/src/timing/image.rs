//! The image (T2.1) and the collection summary (T2.2).

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use meguri_model::clock::Clock;
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::{PlainDatabase, RawValue, SqlValue};
use meguri_storage::{OpenOptions, open_collection};
use serde_json::{Value, json};

/// The collection file's name in `<dir>`, in the image's directory and in
/// every run directory.
pub const COLLECTION: &str = "collection.anki2";
const WAL: &str = "collection.anki2-wal";

/// Why the image could not be made.
#[derive(Debug)]
pub enum ImageError {
    /// Reading `<dir>` or writing the scratch directory failed.
    Io(io::Error),
    /// SQLite could not fold the log into the copy: not a collection it can
    /// read (T8 `open_failed`).
    Unreadable(String),
}

/// Copies `from` to `to` byte for byte: a real copy, never a file-system
/// clone, and never synced (T3.2: a fresh copy, nothing more). A sync
/// would change what a run's first commit has to flush.
pub fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let mut source = File::open(from)?;
    let mut target = File::create(to)?;
    io::copy(&mut source, &mut target)?;
    Ok(())
}

/// T2.1: copies `<source>/collection.anki2` (and its `-wal` when present)
/// into `into` and folds the log into the copy with plain SQLite: a
/// `wal_checkpoint(TRUNCATE)`, the close, then an empty `-wal` left behind
/// deleted. Nothing else changes the copy. Returns the image's path.
pub fn make_image(source: &Path, into: &Path) -> Result<PathBuf, ImageError> {
    fs::create_dir_all(into).map_err(ImageError::Io)?;
    let image = into.join(COLLECTION);
    copy_file(&source.join(COLLECTION), &image).map_err(ImageError::Io)?;
    let log = source.join(WAL);
    if log.is_file() {
        copy_file(&log, &into.join(WAL)).map_err(ImageError::Io)?;
    }
    let unreadable = |e: meguri_storage::StorageError| ImageError::Unreadable(e.to_string());
    let db = PlainDatabase::open(&image).map_err(unreadable)?;
    let rows = db.rows("PRAGMA wal_checkpoint(TRUNCATE)", &[]).map_err(unreadable)?;
    if rows.first().and_then(|row| row.first()) != Some(&RawValue::Integer(0)) {
        return Err(ImageError::Unreadable("the log could not be folded in".to_owned()));
    }
    db.close().map_err(unreadable)?;
    let left = into.join(WAL);
    if fs::metadata(&left).is_ok_and(|m| m.len() == 0) {
        fs::remove_file(&left).map_err(ImageError::Io)?;
    }
    Ok(image)
}

/// T2.2: row counts of a copy of the image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub notes: u64,
    pub cards: u64,
    pub revlog: u64,
    pub notetypes: u64,
    pub decks: u64,
    /// The rows of `deck_config`.
    pub presets: u64,
    pub tags: u64,
    pub graves: u64,
}

impl Summary {
    pub fn to_json(&self) -> Value {
        json!({
            "notes": self.notes,
            "cards": self.cards,
            "revlog": self.revlog,
            "notetypes": self.notetypes,
            "decks": self.decks,
            "presets": self.presets,
            "tags": self.tags,
            "graves": self.graves,
        })
    }
}

/// T2.2: opens a copy of `image` in `dir` through the open gate (the open a
/// run's operations use until `CoreHost::open` exists, U38), counts its
/// rows, closes it and deletes the copy. The error is the open's or the
/// count's, for T8 `open_failed`.
pub fn summarize(image: &Path, dir: &Path, clock: &dyn Clock) -> Result<Summary, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let copy = dir.join(COLLECTION);
    let counted = copy_file(image, &copy)
        .map_err(|e| e.to_string())
        .and_then(|()| count(&copy, clock));
    let removed = fs::remove_dir_all(dir).map_err(|e| e.to_string());
    let summary = counted?;
    removed?;
    Ok(summary)
}

fn count(copy: &Path, clock: &dyn Clock) -> Result<Summary, String> {
    let random = SeededRandom::new(1);
    let collection = open_collection(copy, &OpenOptions::new(clock, &random)).map_err(|e| e.to_string())?;
    let rows = |table: &str| -> Result<u64, String> {
        let out = collection
            .writer()
            .execute_raw(&format!("SELECT count(*) FROM {table}"), &[])
            .map_err(|e| e.to_string())?;
        match out.rows.first().and_then(|row| row.first()) {
            Some(SqlValue::Integer(n)) => u64::try_from(*n).map_err(|e| e.to_string()),
            _ => Err(format!("no count of {table}")),
        }
    };
    let summary = Summary {
        notes: rows("notes")?,
        cards: rows("cards")?,
        revlog: rows("revlog")?,
        notetypes: rows("notetypes")?,
        decks: rows("decks")?,
        presets: rows("deck_config")?,
        tags: rows("tags")?,
        graves: rows("graves")?,
    };
    collection.close().map_err(|e| e.to_string())?;
    Ok(summary)
}
