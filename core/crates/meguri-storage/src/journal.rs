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
//!   before the operation and whose after-image is the last write. Keys
//!   match as the table's key compares them: a `tags` key with `unicase`,
//!   so a tag renamed from `Foo` to `foo` stays one entry under `Foo`.
//! - After-images are the rows as SQLite stored them (the write scope reads
//!   them back), so a column's affinity is applied.
//! - Memory is bounded (R1851): a note's after-image is its digest and the
//!   columns that changed (observers that need the content read it after
//!   commit). Both image sides and savepoint replacements share one retained
//!   byte limit; further images go to an unnamed temporary file in
//!   the collection's directory, removed when the journal drops.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
#[cfg(feature = "test-hooks")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};

use sha2::{Digest, Sha256};

use crate::collation;
use crate::error::StorageError;
use crate::rows::{Cell, RowImage, RowKey, Table};

/// Where and when a journal spills full images.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalOptions {
    /// Combined resident before/after/savepoint payload limit; later images spill.
    pub spill_threshold: usize,
    /// The directory of the spill file: the collection's directory. `None`
    /// keeps every image in memory.
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

    /// Every image in memory (unbounded, for small storage-only callers).
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

    /// The columns in either set.
    pub fn union(self, other: ColumnSet) -> ColumnSet {
        ColumnSet(self.0 | other.0)
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
    /// Encoded image in this journal's append-only backing file.
    Spilled {
        offset: u64,
        len: u64,
    },
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

/// A text key of a table whose key compares with `unicase`, ordered and
/// equal as SQLite compares it.
#[derive(Clone, Debug)]
struct UnicaseKey(Vec<u8>);

impl Ord for UnicaseKey {
    fn cmp(&self, other: &UnicaseKey) -> Ordering {
        collation::compare_stored(&self.0, &other.0)
    }
}

impl PartialOrd for UnicaseKey {
    fn partial_cmp(&self, other: &UnicaseKey) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for UnicaseKey {
    fn eq(&self, other: &UnicaseKey) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for UnicaseKey {}

/// The journal of one write scope.
pub struct Journal {
    options: JournalOptions,
    chunks: Vec<Vec<Entry>>,
    len: usize,
    /// Entries by key, for keys that compare byte for byte.
    index: HashMap<(Table, RowKey), usize>,
    /// Entries of `unicase` keys (`tags`), by that comparison.
    unicase: BTreeMap<UnicaseKey, usize>,
    /// Bytes of all resident images, including savepoint replacements.
    held: usize,
    spill: Option<Spill>,
    live_spilled: u64,
    /// Open savepoints, innermost last (`WriteScope::savepoint`).
    marks: Vec<Mark>,
    /// Compact undo fact captured before images can spill.
    col_mod_before: Option<i64>,
    #[cfg(feature = "test-hooks")]
    reads: AtomicUsize,
    #[cfg(feature = "test-hooks")]
    fail_reads: AtomicBool,
    #[cfg(feature = "test-hooks")]
    fail_after_reads: AtomicUsize,
}

/// The journal's state when a savepoint was taken, and the after-images of
/// older entries first replaced since, so that rolling back to the
/// savepoint restores both.
#[derive(Debug)]
struct Mark {
    len: usize,
    replaced: HashMap<usize, After>,
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
            unicase: BTreeMap::new(),
            held: 0,
            spill: None,
            live_spilled: 0,
            marks: Vec::new(),
            col_mod_before: None,
            #[cfg(feature = "test-hooks")]
            reads: AtomicUsize::new(0),
            #[cfg(feature = "test-hooks")]
            fail_reads: AtomicBool::new(false),
            #[cfg(feature = "test-hooks")]
            fail_after_reads: AtomicUsize::new(usize::MAX),
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

    /// The entry of a key, if the operation changed it; the key matches as
    /// the table's key compares (`tags` with `unicase`).
    pub fn entry(&self, table: Table, key: &RowKey) -> Option<&Entry> {
        self.slot(table, key).map(|i| self.at(i))
    }

    fn slot(&self, table: Table, key: &RowKey) -> Option<usize> {
        match key {
            RowKey::Text(text) if table.unicase_key() => self.unicase.get(&UnicaseKey(text.clone())).copied(),
            _ => self.index.get(&(table, key.clone())).copied(),
        }
    }

    fn add_slot(&mut self, table: Table, key: &RowKey, at: usize) {
        match key {
            RowKey::Text(text) if table.unicase_key() => {
                self.unicase.insert(UnicaseKey(text.clone()), at);
            }
            _ => {
                self.index.insert((table, key.clone()), at);
            }
        }
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
        #[cfg(feature = "test-hooks")]
        if !matches!(entry.before, Before::Absent) {
            self.test_read()?;
        }
        match &entry.before {
            Before::Absent => Ok(None),
            Before::Held(image) => Ok(Some(image.clone())),
            Before::Spilled { offset, len } => self.read_spill(*offset, *len).map(Some),
        }
    }

    /// Final stored image. Notes deliberately retain only a digest, so requesting
    /// their contents is an error, distinct from an absent row.
    pub fn after(&self, entry: &Entry) -> Result<Option<RowImage>, StorageError> {
        #[cfg(feature = "test-hooks")]
        if matches!(entry.after, After::Row(_) | After::Spilled { .. }) {
            self.test_read()?;
        }
        match &entry.after {
            After::Absent => Ok(None),
            After::Row(image) => Ok(Some(image.clone())),
            After::Spilled { offset, len } => self.read_spill(*offset, *len).map(Some),
            After::Note(_) => Err(StorageError::Mode {
                detail: "note after-images contain a digest only".into(),
            }),
        }
    }

    /// Captured from the original stored collection row, without a payload read.
    pub fn col_mod_before(&self) -> Option<i64> {
        self.col_mod_before
    }

    #[cfg(feature = "test-hooks")]
    fn test_read(&self) -> Result<(), StorageError> {
        let read = self.reads.fetch_add(1, AtomicOrdering::Relaxed);
        if self.fail_reads.load(AtomicOrdering::Relaxed) || read >= self.fail_after_reads.load(AtomicOrdering::Relaxed)
        {
            return Err(corrupt("injected image read I/O failure"));
        }
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub fn image_reads(&self) -> usize {
        self.reads.load(AtomicOrdering::Relaxed)
    }
    #[cfg(feature = "test-hooks")]
    pub fn fail_image_reads(&self, fail: bool) {
        self.fail_reads.store(fail, AtomicOrdering::Relaxed);
        if !fail {
            self.fail_after_reads.store(usize::MAX, AtomicOrdering::Relaxed);
        }
    }
    #[cfg(feature = "test-hooks")]
    pub fn fail_after_image_reads(&self, successful_reads: usize) {
        self.fail_after_reads
            .store(self.image_reads() + successful_reads, AtomicOrdering::Relaxed);
    }
    #[cfg(feature = "test-hooks")]
    pub fn truncate_backing(&self) -> Result<(), StorageError> {
        self.spill
            .as_ref()
            .ok_or_else(|| corrupt("no spill file"))?
            .file
            .set_len(0)
            .map_err(io)
    }
    #[cfg(feature = "test-hooks")]
    pub fn corrupt_backing(&self) -> Result<(), StorageError> {
        self.spill
            .as_ref()
            .ok_or_else(|| corrupt("no spill file"))?
            .file
            .write_all_at(&[255; 32], 0)
            .map_err(io)
    }

    fn read_spill(&self, offset: u64, len: u64) -> Result<RowImage, StorageError> {
        let spill = self.spill.as_ref().ok_or_else(|| corrupt("missing backing file"))?;
        let len = usize::try_from(len).map_err(|_| corrupt("invalid image length"))?;
        let mut checksum = [0; 32];
        spill.file.read_exact_at(&mut checksum, offset).map_err(io)?;
        let mut bytes = vec![0; len];
        spill.file.read_exact_at(&mut bytes, offset + 32).map_err(io)?;
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != checksum {
            return Err(corrupt("image checksum mismatch"));
        }
        decode(&bytes).ok_or_else(|| corrupt("invalid image encoding"))
    }

    /// Bytes of all resident images, including savepoint replacements.
    pub fn held_bytes(&self) -> usize {
        self.held
    }

    /// Encoded bytes of live spilled images (including checksum headers).
    pub fn spilled_bytes(&self) -> u64 {
        self.live_spilled
    }

    /// Records one real change of the row `key`.
    pub(crate) fn record(
        &mut self,
        table: Table,
        key: RowKey,
        before: Option<RowImage>,
        after: Option<RowImage>,
    ) -> Result<(), StorageError> {
        if let Some(at) = self.slot(table, &key) {
            let after = match after {
                Some(image) if table == Table::Notes => {
                    let original = self.before(self.at(at))?;
                    After::Note(note_digest(original.as_deref(), &image))
                }
                Some(image) => self.keep_after(image)?,
                None => After::Absent,
            };
            let replaced = std::mem::replace(&mut self.at_mut(at).after, after);
            self.remember_replacement(at, replaced);
            return Ok(());
        }
        let after = match after {
            Some(image) if table == Table::Notes => After::Note(note_digest(before.as_deref(), &image)),
            Some(image) => self.keep_after(image)?,
            None => After::Absent,
        };
        self.push(table, key, before, after)
    }

    /// Adds an entry for a key the journal does not hold yet.
    fn push(&mut self, table: Table, key: RowKey, before: Option<RowImage>, after: After) -> Result<(), StorageError> {
        let col_mod_before = before.as_ref().and_then(|row| row.get(2)).and_then(Cell::as_i64);
        let before = match before {
            None => Before::Absent,
            Some(image) => match self.keep(image) {
                Ok(before) => before,
                Err(error) => {
                    // Undo accounting for the retained after-image when
                    // storing its matching before-image fails.
                    self.held -= after_bytes(&after);
                    self.live_spilled -= after_spilled(&after);
                    return Err(error);
                }
            },
        };
        if table == Table::Col {
            self.col_mod_before = col_mod_before;
        }
        self.add_slot(table, &key, self.len);
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

    /// The journal of this operation followed by `later`, as if one
    /// operation had made both (U25: note edits merged into one undo step,
    /// `engine/undo` A3). Every key either changed, in this journal's order
    /// and then `later`'s, with the before-image of the first journal that
    /// changed it and the after-image of the last. A note's changed columns
    /// are those either operation changed, so they may include one the
    /// second set back. Before-images are copied and spill as this
    /// journal's options say.
    pub fn then(&self, later: &Journal) -> Result<Journal, StorageError> {
        let mut out = Journal::new(self.options.clone());
        for entry in self.entries() {
            let after = match later.entry(entry.table, &entry.key) {
                None => out.copy_after(self, entry)?,
                Some(next) => match (&entry.after, &next.after) {
                    (After::Note(first), After::Note(second)) => After::Note(NoteDigest {
                        sha256: second.sha256,
                        changed: first.changed.union(second.changed),
                    }),
                    _ => out.copy_after(later, next)?,
                },
            };
            out.push(entry.table, entry.key.clone(), self.before(entry)?, after)?;
        }
        for entry in later.entries() {
            if self.slot(entry.table, &entry.key).is_none() {
                let after = out.copy_after(later, entry)?;
                out.push(entry.table, entry.key.clone(), later.before(entry)?, after)?;
            }
        }
        Ok(out)
    }

    /// Remembers the journal's state for a savepoint.
    pub(crate) fn push_mark(&mut self) {
        self.marks.push(Mark {
            len: self.len,
            replaced: HashMap::new(),
        });
    }

    /// Releases the innermost savepoint: its entries stay. After-images it
    /// replaced of entries older than the enclosing savepoint are kept for
    /// that one, which may still roll back.
    pub(crate) fn release_mark(&mut self) {
        let Some(mark) = self.marks.pop() else { return };
        for (at, after) in mark.replaced {
            self.remember_replacement(at, after);
        }
    }

    fn remember_replacement(&mut self, at: usize, after: After) {
        // Only the first prior value per key belongs to a mark. Replacing the
        // same key repeatedly cannot grow a second per-write metadata log.
        if let Some(mark) = self.marks.last_mut()
            && at < mark.len
            && let std::collections::hash_map::Entry::Vacant(slot) = mark.replaced.entry(at)
        {
            slot.insert(after);
        } else {
            self.held -= after_bytes(&after);
            self.live_spilled -= after_spilled(&after);
        }
    }

    /// Rolls the journal back to the innermost savepoint: the entries added
    /// since are dropped (their spilled images become unused space in the
    /// spill file) and older entries get back their after-images.
    pub(crate) fn rollback_mark(&mut self) {
        let Some(mark) = self.marks.pop() else { return };
        for (at, after) in mark.replaced {
            let current = std::mem::replace(&mut self.at_mut(at).after, after);
            self.held -= after_bytes(&current);
            self.live_spilled -= after_spilled(&current);
        }
        while self.len > mark.len {
            self.len -= 1;
            let entry = self
                .chunks
                .last_mut()
                .and_then(Vec::pop)
                .expect("the journal holds `len` entries");
            if self.chunks.last().is_some_and(Vec::is_empty) {
                self.chunks.pop();
            }
            self.held -= before_bytes(&entry.before) + after_bytes(&entry.after);
            self.live_spilled -= before_spilled(&entry.before) + after_spilled(&entry.after);
            if entry.table == Table::Col {
                self.col_mod_before = None;
            }
            match &entry.key {
                RowKey::Text(text) if entry.table.unicase_key() => {
                    self.unicase.remove(&UnicaseKey(text.clone()));
                }
                key => {
                    self.index.remove(&(entry.table, key.clone()));
                }
            }
        }
        // Extents are append-only, even across rollback. No surviving reference
        // can ever observe a reused offset.
    }

    fn copy_after(&mut self, source: &Journal, entry: &Entry) -> Result<After, StorageError> {
        match entry.after() {
            After::Absent => Ok(After::Absent),
            After::Note(digest) => Ok(After::Note(*digest)),
            After::Row(_) | After::Spilled { .. } => {
                let image = source.after(entry)?.ok_or_else(|| corrupt("missing final image"))?;
                self.keep_after(image)
            }
        }
    }

    fn keep_after(&mut self, image: RowImage) -> Result<After, StorageError> {
        Ok(match self.keep(image)? {
            Before::Absent => After::Absent,
            Before::Held(image) => After::Row(image),
            Before::Spilled { offset, len } => After::Spilled { offset, len },
        })
    }

    fn keep(&mut self, image: RowImage) -> Result<Before, StorageError> {
        let size = retained_bytes(&image);
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
        let offset = spill.len;
        let len = if size <= self.options.spill_threshold {
            let bytes = encode(&image);
            let checksum: [u8; 32] = Sha256::digest(&bytes).into();
            spill.file.write_all_at(&checksum, offset).map_err(io)?;
            spill.file.write_all_at(&bytes, offset + 32).map_err(io)?;
            bytes.len() as u64
        } else {
            let checksum = digest(&image);
            spill.file.write_all_at(&checksum, offset).map_err(io)?;
            let mut len = 0_u64;
            let mut written = Ok(());
            encoded_parts(&image, |bytes| {
                if written.is_ok() {
                    written = spill.file.write_all_at(bytes, offset + 32 + len);
                    len += bytes.len() as u64;
                }
            });
            written.map_err(io)?;
            len
        };
        spill.len += 32 + len;
        self.live_spilled += 32 + len;
        Ok(Before::Spilled { offset, len })
    }
}

fn before_spilled(before: &Before) -> u64 {
    match before {
        Before::Spilled { len, .. } => 32 + len,
        _ => 0,
    }
}
fn after_spilled(after: &After) -> u64 {
    match after {
        After::Spilled { len, .. } => 32 + len,
        _ => 0,
    }
}

fn after_bytes(after: &After) -> usize {
    match after {
        After::Row(image) => retained_bytes(image),
        _ => 0,
    }
}

fn before_bytes(before: &Before) -> usize {
    match before {
        Before::Held(image) => retained_bytes(image),
        _ => 0,
    }
}

fn retained_bytes(image: &RowImage) -> usize {
    image.capacity() * std::mem::size_of::<Cell>() + image.iter().map(Cell::heap_bytes).sum::<usize>()
}

fn corrupt(detail: &str) -> StorageError {
    StorageError::Io {
        detail: format!("write journal spill file: {detail}"),
    }
}

fn io(error: std::io::Error) -> StorageError {
    StorageError::Io {
        detail: format!("write journal spill file: {error}"),
    }
}

/// The SHA-256 of a row image in the journal's own encoding: a note's
/// after-image digest, and the row digests of `engine/op-model` O7.
pub fn digest(image: &[Cell]) -> [u8; 32] {
    let mut hash = Sha256::new();
    encoded_parts(image, |bytes| hash.update(bytes));
    hash.finalize().into()
}

fn note_digest(before: Option<&[Cell]>, after: &[Cell]) -> NoteDigest {
    let sha256 = digest(after);
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
    encoded_parts(image, |bytes| out.extend_from_slice(bytes));
    out
}

fn encoded_parts(image: &[Cell], mut emit: impl FnMut(&[u8])) {
    for cell in image {
        match cell {
            Cell::Null => emit(&[0]),
            Cell::Integer(v) => {
                emit(&[1]);
                emit(&v.to_le_bytes());
            }
            Cell::Real(v) => {
                emit(&[2]);
                emit(&v.to_bits().to_le_bytes());
            }
            Cell::Text(bytes) | Cell::Blob(bytes) => {
                emit(&[if matches!(cell, Cell::Text(_)) { 3 } else { 4 }]);
                emit(&(bytes.len() as u64).to_le_bytes());
                emit(bytes);
            }
        }
    }
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
    fn streamed_spill_and_digest_keep_the_exact_image_encoding() {
        let image = vec![
            Cell::Null,
            Cell::Integer(-5),
            Cell::Real(-0.0),
            Cell::Text(vec![0xff, b'a']),
            Cell::Blob(Vec::new()),
        ];
        let mut expected = vec![0, 1];
        expected.extend_from_slice(&(-5_i64).to_le_bytes());
        expected.push(2);
        expected.extend_from_slice(&(-0.0_f64).to_bits().to_le_bytes());
        expected.push(3);
        expected.extend_from_slice(&2_u64.to_le_bytes());
        expected.extend_from_slice(&[0xff, b'a']);
        expected.push(4);
        expected.extend_from_slice(&0_u64.to_le_bytes());
        assert_eq!(encode(&image), expected);
        let checksum: [u8; 32] = Sha256::digest(&expected).into();
        assert_eq!(digest(&image), checksum);
        let directory = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(JournalOptions {
            spill_threshold: 0,
            spill_dir: Some(directory.path().to_owned()),
        });
        let Before::Spilled { offset, len } = journal.keep(image.clone()).unwrap() else {
            panic!("spilled image");
        };
        assert_eq!(len, expected.len() as u64);
        assert_eq!(journal.read_spill(offset, len).unwrap(), image);
        let file = &journal.spill.as_ref().unwrap().file;
        let mut stored = vec![0; 32 + expected.len()];
        file.read_exact_at(&mut stored, offset).unwrap();
        assert_eq!(&stored[..32], &checksum);
        assert_eq!(&stored[32..], expected);
        file.write_all_at(&[checksum[0] ^ 1], offset).unwrap();
        assert!(journal.read_spill(offset, len).is_err());
    }

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
