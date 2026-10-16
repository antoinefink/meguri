//! The strings the output guard looks for (R1940): deck names, tags, media
//! names and the words of note fields, read from a copy of a collection.
//!
//! This module only collects them. Matching is the scanner's
//! (`tools/scripts/private-scan/private_scan.py`), which also runs as the
//! pre-commit hook: it splits every candidate into words with its own
//! tokenizer, keeps every deck, tag and media name and the field words of
//! four or more characters, and drops what is already committed in the
//! repository. So the field words here are the field's text
//! (markup and entities removed, `meguri_text::html::strip_markup`) split at
//! white space and ASCII punctuation only, where every tokenizer splits.
//!
//! The candidates file is JSON lines, `{"kind": "deck"|"tag"|"media"|"word",
//! "text": ...}`, one per distinct string and kind. It holds private text and
//! is written only under `build/private/`.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::SqlValue;
use meguri_storage::{OpenOptions, Writer, open_collection};
use meguri_text::{html, media_refs};

use super::vault::{COLLECTION, COLLECTION_WAL, MEDIA_DIR};

/// The separator of deck name components in the `decks` table (native form).
const NATIVE_SEPARATOR: char = '\u{1f}';
/// The separator of hierarchy components in human deck names and tags.
const HIERARCHY_SEPARATOR: &str = "::";
/// The separator of fields in `notes.flds`.
const FIELD_SEPARATOR: char = '\u{1f}';
/// Notes read per page.
const PAGE: i64 = 5_000;
/// Field chunks shorter than this (in characters) cannot hold a word the
/// scanner keeps, even after case folding expands a character.
const MIN_CHUNK_CHARS: usize = 2;

/// Distinct private strings by kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Candidates {
    /// Deck names in human form (`A::B`) and each of their components.
    pub decks: BTreeSet<String>,
    /// Tags as written and each of their `::` components.
    pub tags: BTreeSet<String>,
    /// Media file names: the media folder's files and the local names fields
    /// refer to.
    pub media: BTreeSet<String>,
    /// Field text chunks, split at white space and ASCII punctuation.
    pub words: BTreeSet<String>,
}

/// Why the candidates could not be read. Messages carry no collection text.
#[derive(Debug)]
pub enum RedactError {
    Open(String),
    Read(String),
    Io(io::Error),
}

impl fmt::Display for RedactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedactError::Open(detail) => write!(f, "opening the copy failed: {detail}"),
            RedactError::Read(detail) => write!(f, "reading the copy failed: {detail}"),
            RedactError::Io(e) => write!(f, "reading the copy failed: {e}"),
        }
    }
}

impl std::error::Error for RedactError {}

impl Candidates {
    /// Collects the candidates of the collection copy in `copy` (a folder
    /// holding [`COLLECTION`], optionally its write-ahead log and
    /// [`MEDIA_DIR`]). The database is opened from a scratch copy in a
    /// temporary folder, since the open gate may upgrade or checkpoint it:
    /// `copy` stays byte for byte what the vault made.
    pub fn extract(copy: &Path) -> Result<Candidates, RedactError> {
        let mut out = Candidates::default();
        let scratch = tempfile::tempdir().map_err(RedactError::Io)?;
        fs::copy(copy.join(COLLECTION), scratch.path().join(COLLECTION)).map_err(RedactError::Io)?;
        match fs::copy(copy.join(COLLECTION_WAL), scratch.path().join(COLLECTION_WAL)) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(RedactError::Io(e)),
        }
        let clock = FixedClock::new(0, UtcOffset::UTC);
        let random = SeededRandom::new(0);
        let collection = open_collection(&scratch.path().join(COLLECTION), &OpenOptions::new(&clock, &random))
            .map_err(|e| RedactError::Open(e.to_string()))?;
        let read = out.read_rows(collection.writer());
        collection.close().map_err(|e| RedactError::Read(e.to_string()))?;
        read?;
        let media = copy.join(MEDIA_DIR);
        if media.is_dir() {
            for entry in fs::read_dir(&media).map_err(RedactError::Io)? {
                let entry = entry.map_err(RedactError::Io)?;
                if entry.file_type().map_err(RedactError::Io)?.is_file() {
                    out.media.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        Ok(out)
    }

    fn read_rows(&mut self, writer: &Writer) -> Result<(), RedactError> {
        let query = |sql: &str, params: &[SqlValue]| {
            writer
                .execute_raw(sql, params)
                .map_err(|e| RedactError::Read(e.to_string()))
        };
        for row in query("SELECT name FROM decks", &[])?.rows {
            if let Some(SqlValue::Text(native)) = row.first() {
                let human = native
                    .split(NATIVE_SEPARATOR)
                    .collect::<Vec<_>>()
                    .join(HIERARCHY_SEPARATOR);
                self.add_hierarchy(Kind::Deck, &human);
            }
        }
        for row in query("SELECT tag FROM tags", &[])?.rows {
            if let Some(SqlValue::Text(tag)) = row.first() {
                self.add_hierarchy(Kind::Tag, tag);
            }
        }
        let mut after = i64::MIN;
        loop {
            let page = query(
                "SELECT id, flds, tags FROM notes WHERE id > ?1 ORDER BY id LIMIT ?2",
                &[SqlValue::Integer(after), SqlValue::Integer(PAGE)],
            )?;
            for row in &page.rows {
                if let Some(SqlValue::Text(fields)) = row.get(1) {
                    for field in fields.split(FIELD_SEPARATOR) {
                        self.add_field(field);
                    }
                }
                if let Some(SqlValue::Text(tags)) = row.get(2) {
                    for tag in tags.split_whitespace() {
                        self.add_hierarchy(Kind::Tag, tag);
                    }
                }
            }
            match page.rows.last().and_then(|row| row.first()) {
                Some(SqlValue::Integer(id)) if page.rows.len() as i64 == PAGE => after = *id,
                _ => break,
            }
        }
        Ok(())
    }

    fn add_hierarchy(&mut self, kind: Kind, name: &str) {
        let set = match kind {
            Kind::Deck => &mut self.decks,
            Kind::Tag => &mut self.tags,
        };
        if name.is_empty() {
            return;
        }
        set.insert(name.to_owned());
        for part in name.split(HIERARCHY_SEPARATOR) {
            if !part.is_empty() {
                set.insert(part.to_owned());
            }
        }
    }

    fn add_field(&mut self, field: &str) {
        for name in media_refs::local_references(field) {
            if !name.is_empty() {
                self.media.insert(name);
            }
        }
        let text = html::strip_markup(field);
        for chunk in text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation()) {
            if chunk.chars().count() >= MIN_CHUNK_CHARS {
                self.words.insert(chunk.to_owned());
            }
        }
    }

    /// Writes the candidates as JSON lines: decks, tags, media, then words,
    /// each set in order.
    pub fn write_jsonl(&self, out: &mut impl Write) -> io::Result<()> {
        for (kind, set) in [
            ("deck", &self.decks),
            ("tag", &self.tags),
            ("media", &self.media),
            ("word", &self.words),
        ] {
            for text in set {
                serde_json::to_writer(&mut *out, &serde_json::json!({"kind": kind, "text": text}))?;
                out.write_all(b"\n")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Deck,
    Tag,
}
