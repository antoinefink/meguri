//! The write journal: for each row one operation changed, its table, key,
//! image before the operation (or absent) and image after it (or absent).
//! U24 derives the ChangeSet and feeds in-core observers from it, U25 keeps
//! its before-images as undo entries, and U107 overlays held-back rows with
//! them (KTD6).
//!
//! - Only real changes are journalled: a write that changes nothing (S4)
//!   and an insert-or-ignore that finds its row (graves G2) leave no entry.
//! - The first before-image of a key wins: a row written twice in one
//!   operation has one entry, whose before-image is the row as it was
//!   before the operation and whose after-image is the last write.
//! - Memory is bounded (R1851): a note's after-image is its digest and the
//!   columns that changed (observers that need the content read it after
//!   commit), and once the before-images held in memory pass the spill
//!   threshold, further before-images go to an unnamed temporary file in
//!   the collection's directory, removed when the journal drops.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::StorageError;
use crate::rows::{Cell, RowImage, RowKey, Table};

/// Where and when a journal spills before-images.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalOptions {
    /// Bytes of before-images held in memory; later ones spill.
    pub spill_threshold: usize,
    /// The directory of the spill file: the collection's directory. `None`
    /// keeps every before-image in memory.
    pub spill_dir: Option<PathBuf>,
}

impl JournalOptions {
    /// The threshold until the op catalog (`specs/engine/op-model.md`, U24)
    /// sets its own.
    pub const DEFAULT_SPILL_THRESHOLD: usize = 8 * 1024 * 1024;

    /// The default threshold, spilling into `dir`.
    pub fn spilling_into(dir: &Path) -> JournalOptions {
        JournalOptions {
            spill_threshold: JournalOptions::DEFAULT_SPILL_THRESHOLD,
            spill_dir: Some(dir.to_path_buf()),
        }
    }

    /// Every before-image in memory.
    pub fn in_memory() -> JournalOptions {
        JournalOptions {
            spill_threshold: usize::MAX,
            spill_dir: None,
        }
    }
}

/// Columns of one table, by index in [`Table::columns`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ColumnSet(u32);

impl ColumnSet {
    pub fn all(table: Table) -> ColumnSet {
        ColumnSet((1u32 << table.columns().len()) - 1)
    }

    /// The columns whose cells differ.
    pub fn changed(before: &[Cell], after: &[Cell]) -> ColumnSet {
        let mut set = 0u32;
        for (i, (a, b)) in before.iter().zip(after).enumerate() {
            if a != b {
                set |= 1 << i;
            }
        }
        ColumnSet(set)
    }

    pub fn contains(self, column: usize) -> bool {
        column < 32 && self.0 & (1 << column) != 0
    }

    /// The names of the columns in the set.
    pub fn names(self, table: Table) -> Vec<&'static str> {
        table
            .columns()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.contains(*i))
            .map(|(_, c)| *c)
            .collect()
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A note row after the operation: the SHA-256 of its image (in the
/// journal's own encoding) and the columns that differ from the
/// before-image (all of them for a new note).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteDigest {
    pub sha256: [u8; 32],
    pub changed: ColumnSet,
}

/// A row after the operation.
#[derive(Clone, Debug, PartialEq)]
pub enum After {
    /// Deleted by the operation (or never there, for a row the operation
    /// both created and deleted).
    Absent,
    Row(RowImage),
    /// A note row: its digest instead of its image.
    Note(NoteDigest),
}

#[derive(Clone, Debug, PartialEq)]
enum Before {
    Absent,
    Held(RowImage),
    Spilled { offset: u64, len: u64 },
}

/// One changed row.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    table: Table,
    key: RowKey,
    before: Before,
    after: After,
}

impl Entry {
    pub fn table(&self) -> Table {
        self.table
    }

    pub fn key(&self) -> &RowKey {
        &self.key
    }

    pub fn after(&self) -> &After {
        &self.after
    }

    /// Whether the row existed before the operation.
    pub fn existed_before(&self) -> bool {
        !matches!(self.before, Before::Absent)
    }

    /// Whether the row exists after the operation.
    pub fn exists_after(&self) -> bool {
        !matches!(self.after, After::Absent)
    }
}

/// Entries per chunk: entries are kept in chunks of this many, so a large
/// journal never holds a doubled copy of itself while it grows.
const CHUNK: usize = 4096;

/// The journal of one write scope.
pub struct Journal {
    options: JournalOptions,
    chunks: Vec<Vec<Entry>>,
    len: usize,
    index: HashMap<(Table, RowKey), usize>,
    /// Bytes of before-images held in memory.
    held: usize,
    spill: Option<Spill>,
}

struct Spill {
    file: File,
    len: u64,
}

impl fmt::Debug for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Journal")
            .field("entries", &self.len)
            .field("held", &self.held)
            .field("spilled", &self.spill.as_ref().map(|s| s.len))
            .finish()
    }
}

impl Journal {
    pub fn new(options: JournalOptions) -> Journal {
        Journal {
            options,
            chunks: Vec::new(),
            len: 0,
            index: HashMap::new(),
            held: 0,
            spill: None,
        }
    }

    /// The entries in the order their keys were first changed.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.chunks.iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The entry of a key, if the operation changed it.
    pub fn entry(&self, table: Table, key: &RowKey) -> Option<&Entry> {
        self.index.get(&(table, key.clone())).map(|&i| self.at(i))
    }

    fn at(&self, index: usize) -> &Entry {
        &self.chunks[index / CHUNK][index % CHUNK]
    }

    fn at_mut(&mut self, index: usize) -> &mut Entry {
        &mut self.chunks[index / CHUNK][index % CHUNK]
    }

    /// The row as it was before the operation; `None` when the operation
    /// created it. Reads a spilled image back from the spill file.
    pub fn before(&self, entry: &Entry) -> Result<Option<RowImage>, StorageError> {
        match &entry.before {
            Before::Absent => Ok(None),
            Before::Held(image) => Ok(Some(image.clone())),
            Before::Spilled { offset, len } => {
                let spill = self.spill.as_ref().ok_or_else(|| StorageError::Io {
                    detail: "the journal's spill file is gone".to_owned(),
                })?;
                let mut bytes = vec![0; usize::try_from(*len).unwrap_or(usize::MAX)];
                spill.file.read_exact_at(&mut bytes, *offset).map_err(io)?;
                decode(&bytes).map(Some).ok_or_else(|| StorageError::Io {
                    detail: "a spilled before-image does not decode".to_owned(),
                })
            }
        }
    }

    /// Bytes of before-images held in memory.
    pub fn held_bytes(&self) -> usize {
        self.held
    }

    /// Bytes written to the spill file.
    pub fn spilled_bytes(&self) -> u64 {
        self.spill.as_ref().map_or(0, |s| s.len)
    }

    /// Records one real change of the row `key`.
    pub(crate) fn record(
        &mut self,
        table: Table,
        key: RowKey,
        before: Option<RowImage>,
        after: Option<RowImage>,
    ) -> Result<(), StorageError> {
        if let Some(&at) = self.index.get(&(table, key.clone())) {
            let after = match after {
                Some(image) if table == Table::Notes => {
                    let original = self.before(self.at(at))?;
                    After::Note(note_digest(original.as_deref(), &image))
                }
                Some(image) => After::Row(image),
                None => After::Absent,
            };
            self.at_mut(at).after = after;
            return Ok(());
        }
        let after = match after {
            Some(image) if table == Table::Notes => After::Note(note_digest(before.as_deref(), &image)),
            Some(image) => After::Row(image),
            None => After::Absent,
        };
        let before = match before {
            None => Before::Absent,
            Some(image) => self.keep(image)?,
        };
        self.index.insert((table, key.clone()), self.len);
        if self.chunks.last().is_none_or(|chunk| chunk.len() == CHUNK) {
            self.chunks.push(Vec::with_capacity(CHUNK));
        }
        if let Some(chunk) = self.chunks.last_mut() {
            chunk.push(Entry {
                table,
                key,
                before,
                after,
            });
        }
        self.len += 1;
        Ok(())
    }

    fn keep(&mut self, image: RowImage) -> Result<Before, StorageError> {
        let size = image_bytes(&image);
        let Some(dir) = &self.options.spill_dir else {
            self.held += size;
            return Ok(Before::Held(image));
        };
        if self.held.saturating_add(size) <= self.options.spill_threshold {
            self.held += size;
            return Ok(Before::Held(image));
        }
        if self.spill.is_none() {
            let file = tempfile::tempfile_in(dir).map_err(io)?;
            self.spill = Some(Spill { file, len: 0 });
        }
        let spill = self.spill.as_mut().ok_or_else(|| StorageError::Io {
            detail: "no spill file".to_owned(),
        })?;
        let bytes = encode(&image);
        let offset = spill.len;
        spill.file.write_all_at(&bytes, offset).map_err(io)?;
        let len = bytes.len() as u64;
        spill.len += len;
        Ok(Before::Spilled { offset, len })
    }
}

fn io(error: std::io::Error) -> StorageError {
    StorageError::Io {
        detail: format!("write journal spill file: {error}"),
    }
}

fn note_digest(before: Option<&[Cell]>, after: &[Cell]) -> NoteDigest {
    let sha256: [u8; 32] = Sha256::digest(encode(after)).into();
    let changed = match before {
        Some(before) => ColumnSet::changed(before, after),
        None => ColumnSet::all(Table::Notes),
    };
    NoteDigest { sha256, changed }
}

/// The memory an image takes.
fn image_bytes(image: &[Cell]) -> usize {
    std::mem::size_of_val(image) + image.iter().map(Cell::heap_bytes).sum::<usize>()
}

/// The journal's own encoding of an image: per cell a tag byte, then
/// 8 bytes little-endian for numbers, or a length and the bytes.
pub(crate) fn encode(image: &[Cell]) -> Vec<u8> {
    let mut out = Vec::with_capacity(image_bytes(image) + image.len() * 9);
    for cell in image {
        match cell {
            Cell::Null => out.push(0),
            Cell::Integer(v) => {
                out.push(1);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Cell::Real(v) => {
                out.push(2);
                out.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            Cell::Text(bytes) | Cell::Blob(bytes) => {
                out.push(if matches!(cell, Cell::Text(_)) { 3 } else { 4 });
                out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                out.extend_from_slice(bytes);
            }
        }
    }
    out
}

pub(crate) fn decode(mut bytes: &[u8]) -> Option<RowImage> {
    let mut image = Vec::new();
    while let Some((&tag, rest)) = bytes.split_first() {
        if tag == 0 {
            image.push(Cell::Null);
            bytes = rest;
            continue;
        }
        let (word, rest) = rest.split_first_chunk::<8>()?;
        let word = u64::from_le_bytes(*word);
        bytes = match tag {
            1 => {
                image.push(Cell::Integer(word as i64));
                rest
            }
            2 => {
                image.push(Cell::Real(f64::from_bits(word)));
                rest
            }
            3 | 4 => {
                let len = usize::try_from(word).ok()?;
                let payload = rest.get(..len)?.to_vec();
                image.push(if tag == 3 {
                    Cell::Text(payload)
                } else {
                    Cell::Blob(payload)
                });
                &rest[len..]
            }
            _ => return None,
        };
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_round_trip_through_the_encoding() {
        let image = vec![
            Cell::Null,
            Cell::Integer(-5),
            Cell::Real(-0.0),
            Cell::Text(vec![0xff, b'a']),
            Cell::Blob(Vec::new()),
            Cell::Null,
        ];
        assert_eq!(decode(&encode(&image)), Some(image));
        assert_eq!(decode(&[]), Some(Vec::new()));
        assert_eq!(decode(&[3, 9, 0, 0, 0, 0, 0, 0, 0, b'x']), None);
        assert_eq!(decode(&[7]), None);
    }
}
