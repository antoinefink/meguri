//! Write scopes: the only code path that writes rows into
//! `collection.anki2`. A scope is one `BEGIN IMMEDIATE` transaction on the
//! writer with a stamping mode ([`StampMode`]) and a journal. Every write
//! compares the row it is handed with the stored one and skips it when they
//! are equal in every column (`ids-usn-mtime` S4); a real change is stamped
//! per the mode (S2, S3) and journalled. A `Local` or `KeepMtime` scope with
//! a change sets `col.mod` to now when it commits (S5).
//!
//! U24's executor hands out every scope for the live collection
//! ([`crate::Writer::begin_write`]). The storage-only [`CollectionBuilder`]
//! creates a new collection file and hands out `Raw` and `CallerSupplied`
//! scopes without the executor, journal off; layering-check keeps it out of
//! meguri-engine and meguri-ffi (edges.toml `[storage_builder]`).

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use meguri_model::card::Card;
use meguri_model::clock::Clock;
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::grave::{Grave, GraveKind};
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_model::revlog::RevlogEntry;
use meguri_model::time::EpochMillis;
use meguri_model::usn::Usn;
use rusqlite::{Connection, OpenFlags};

use crate::connections::{Writer, file_uri, sibling};
use crate::error::{BuilderRefusal, StorageError};
use crate::ids;
use crate::journal::{Journal, JournalOptions};
use crate::open::{self, OpenOptions, OpenedCollection};
use crate::rows::config::{ConfigValue, same_value};
use crate::rows::{self, Cell, Record, RowImage, RowKey, Table, col::COL_KEY, graves, notetypes};
use crate::schema::{SCHEMA_VERSION, ddl18_statements};
use crate::stamp::StampMode;

/// What a write did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// The stored row already held exactly this (S4), or there was nothing
    /// to remove: nothing written, nothing journalled.
    Unchanged,
    Inserted,
    Updated,
    Removed,
}

impl Written {
    pub fn changed(self) -> bool {
        self != Written::Unchanged
    }
}

/// What a committed scope leaves.
#[derive(Debug)]
pub struct Committed {
    /// A failure after SQLite committed never implies rollback. Callers
    /// still publish the journal and must not repeat the operation.
    pub post_commit_error: Option<StorageError>,
    /// Every real change, `col.mod` included; `None` when the scope kept no
    /// journal.
    pub journal: Option<Journal>,
    /// Whether the scope changed anything.
    pub changed: bool,
    /// The `col.mod` the commit set (S5).
    pub col_mod: Option<EpochMillis>,
}

/// One transaction on the writer. Dropping it without [`WriteScope::commit`]
/// rolls it back.
pub struct WriteScope<'c> {
    conn: &'c Connection,
    mode: StampMode,
    clock: &'c dyn Clock,
    journal: Option<Journal>,
    changed: bool,
    open: bool,
    pub(crate) normal_stage: bool,
    /// Open savepoints, innermost last: whether the scope had a change when
    /// each was taken.
    savepoints: Vec<bool>,
}

/// A savepoint inside a write scope (`engine/op-model` O3): one step of a
/// composite op. Released or rolled back innermost first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Savepoint {
    depth: usize,
}

impl std::fmt::Debug for WriteScope<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteScope")
            .field("mode", &self.mode)
            .field("journal", &self.journal)
            .field("changed", &self.changed)
            .finish()
    }
}

impl Writer {
    /// Begins a write scope on the live collection (U24's executor): `BEGIN
    /// IMMEDIATE`, then every write through the scope. `journal` `None`
    /// keeps no journal.
    pub fn begin_write<'c>(
        &'c self,
        mode: StampMode,
        clock: &'c dyn Clock,
        journal: Option<JournalOptions>,
    ) -> Result<WriteScope<'c>, StorageError> {
        WriteScope::begin(&self.conn, mode, clock, journal)
    }
}

impl<'c> WriteScope<'c> {
    pub(crate) fn begin(
        conn: &'c Connection,
        mode: StampMode,
        clock: &'c dyn Clock,
        journal: Option<JournalOptions>,
    ) -> Result<WriteScope<'c>, StorageError> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(WriteScope {
            conn,
            mode,
            clock,
            journal: journal.map(Journal::new),
            changed: false,
            open: true,
            normal_stage: false,
            savepoints: Vec::new(),
        })
    }

    /// Starts a step: an SQLite savepoint and a journal mark. Writes after it
    /// can be rolled back to it without ending the transaction.
    pub fn savepoint(&mut self) -> Result<Savepoint, StorageError> {
        let depth = self.savepoints.len();
        self.conn.execute_batch(&format!("SAVEPOINT meguri_step_{depth}"))?;
        self.savepoints.push(self.changed);
        if let Some(journal) = &mut self.journal {
            journal.push_mark();
        }
        Ok(Savepoint { depth })
    }

    fn innermost(&self, savepoint: Savepoint) -> Result<(), StorageError> {
        if savepoint.depth + 1 == self.savepoints.len() {
            Ok(())
        } else {
            Err(StorageError::Mode {
                detail: format!(
                    "savepoint {} is not the innermost of {} open savepoints",
                    savepoint.depth,
                    self.savepoints.len()
                ),
            })
        }
    }

    /// Ends a step and keeps its writes in the transaction.
    pub fn release_savepoint(&mut self, savepoint: Savepoint) -> Result<(), StorageError> {
        self.innermost(savepoint)?;
        self.conn
            .execute_batch(&format!("RELEASE meguri_step_{}", savepoint.depth))?;
        self.savepoints.pop();
        if let Some(journal) = &mut self.journal {
            journal.release_mark();
        }
        Ok(())
    }

    /// Undoes a step: its writes and its journal entries are gone, the
    /// transaction stays open with the writes made before the step.
    pub fn rollback_to_savepoint(&mut self, savepoint: Savepoint) -> Result<(), StorageError> {
        self.innermost(savepoint)?;
        let name = format!("meguri_step_{}", savepoint.depth);
        self.conn
            .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"))?;
        if let Some(changed) = self.savepoints.pop() {
            self.changed = changed;
        }
        if let Some(journal) = &mut self.journal {
            journal.rollback_mark();
        }
        Ok(())
    }

    pub fn mode(&self) -> StampMode {
        self.mode
    }

    /// The journal so far.
    pub fn journal(&self) -> Option<&Journal> {
        self.journal.as_ref()
    }

    /// Whether the scope has made a real change.
    pub fn has_changes(&self) -> bool {
        self.changed
    }

    /// The clock's current instant (`ids-usn-mtime`: "now").
    pub fn now(&self) -> EpochMillis {
        EpochMillis::from_clock(self.clock)
    }

    pub(crate) fn conn(&self) -> &'c Connection {
        self.conn
    }

    /// Records one real change.
    pub(crate) fn journal_change(
        &mut self,
        table: Table,
        key: RowKey,
        before: Option<RowImage>,
        after: Option<RowImage>,
    ) -> Result<(), StorageError> {
        self.changed = true;
        match &mut self.journal {
            Some(journal) => journal.record(table, key, before, after),
            None => Ok(()),
        }
    }

    /// Records a change that keeps no journal entry: a bulk path with no
    /// journal does not read the rows it changes.
    pub(crate) fn note_change(&mut self) {
        self.changed = true;
    }

    /// Writes a whole row: skipped when the stored row with its key equals
    /// it in every column; otherwise stamped per the mode and inserted or
    /// updated. `force` stamps and writes a row equal to the stored one (a
    /// note type whose fields or templates changed, S1). The after-image
    /// journalled is the row as SQLite stored it, not `cells`: a column's
    /// affinity may change a value's storage class.
    pub(crate) fn put_cells(
        &mut self,
        table: Table,
        mut cells: RowImage,
        force: bool,
    ) -> Result<Written, StorageError> {
        let stored = rows::select_compare(self.conn, table, &cells)?;
        if let Some((_, true)) = &stored
            && !force
        {
            return Ok(Written::Unchanged);
        }
        let now = self.now();
        self.mode.stamp(table, &mut cells, now);
        match stored {
            Some((before, _)) => {
                let key = RowKey::of(table, &before);
                let after = rows::update_stored(self.conn, table, &key, &cells)?;
                self.journal_change(table, key, Some(before), Some(after))?;
                Ok(Written::Updated)
            }
            None => {
                let after = rows::insert_stored(self.conn, table, &cells)?;
                self.journal_change(table, RowKey::of(table, &after), None, Some(after))?;
                Ok(Written::Inserted)
            }
        }
    }

    /// Deletes the row with `key`; nothing when there is none (S4).
    pub(crate) fn remove_key(&mut self, table: Table, key: &RowKey) -> Result<Written, StorageError> {
        let Some(before) = rows::select_image(self.conn, table, key)? else {
            return Ok(Written::Unchanged);
        };
        let key = RowKey::of(table, &before);
        rows::delete(self.conn, table, &key)?;
        self.journal_change(table, key, Some(before), None)?;
        Ok(Written::Removed)
    }

    fn require_as_given(&self, what: &str) -> Result<(), StorageError> {
        match self.mode {
            StampMode::CallerSupplied | StampMode::Raw => Ok(()),
            mode => Err(StorageError::Mode {
                detail: format!("{what} writes rows as given; a {mode:?} scope stamps them"),
            }),
        }
    }

    /// Writes a journalled image back exactly as it was, `usn` and
    /// modification time included, or removes the row when `image` is
    /// `None`: undo and redo restores (S3). Only in a `CallerSupplied` or
    /// `Raw` scope. `image` must fit `key` as the table's key compares them:
    /// a `tags` entry's after-image may differ from its key in letter case.
    pub fn restore(&mut self, table: Table, key: &RowKey, image: Option<&RowImage>) -> Result<Written, StorageError> {
        self.require_as_given("restore")?;
        match image {
            Some(image) => {
                if image.len() != table.columns().len() || !rows::same_key(table, &RowKey::of(table, image), key) {
                    return Err(StorageError::Mode {
                        detail: format!("an image of {table} that does not fit key {key}"),
                    });
                }
                self.put_cells(table, image.clone(), false)
            }
            None => self.remove_key(table, key),
        }
    }

    /// Writes a typed record: inserted, updated, or skipped when unchanged.
    pub fn put<R: Record>(&mut self, record: &R) -> Result<Written, StorageError> {
        self.put_cells(R::TABLE, record.to_cells(), false)
    }

    /// Stamp even an unchanged row (decks/decks D8, D9).
    pub fn put_deck_forced(&mut self, deck: &Deck) -> Result<Written, StorageError> {
        self.put_cells(Table::Decks, deck.to_cells(), true)
    }

    pub fn put_card_forced(&mut self, card: &Card) -> Result<Written, StorageError> {
        self.put_cells(Table::Cards, card.to_cells(), true)
    }

    /// Replace data while preserving every other stored scheduling column,
    /// including imported values that a typed whole-card write would normalize.
    /// The ordinary scope stamps and journals only a changed row.
    pub fn put_card_data(
        &mut self,
        id: meguri_model::ids::CardId,
        data: &meguri_model::card_data::CardData,
    ) -> Result<Written, StorageError> {
        let Some(mut image) = rows::select_image(self.conn, Table::Cards, &RowKey::Id(id.0))? else {
            return Ok(Written::Unchanged);
        };
        image[17] = Cell::text(data.to_text());
        self.put_cells(Table::Cards, image, false)
    }

    /// M4's memory-only update. Read once, compare as stored, and name only
    /// data/stamps in UPDATE so unrelated scheduling indexes stay untouched.
    /// Journal the exact before-image and the actual stored after-image.
    pub fn update_card_memory(
        &mut self,
        id: CardId,
        memory: Option<meguri_model::card_data::MemoryState>,
        retention: f32,
        decay: f32,
    ) -> Result<Written, StorageError> {
        let key = RowKey::Id(id.0);
        let Some(before) = rows::select_image(self.conn, Table::Cards, &key)? else {
            return Ok(Written::Unchanged);
        };
        let mut data = Card::from_cells(&before)?.data;
        let mut desired = meguri_model::card_data::CardData::default();
        desired.memory_state = memory;
        desired.desired_retention = Some(retention);
        desired.decay = Some(decay);
        if data.same_memory_values(&desired) {
            return Ok(Written::Unchanged);
        }
        data.memory_state = memory;
        data.desired_retention = Some(retention);
        data.decay = Some(decay);
        let mut after = before.clone();
        after[17] = Cell::text(data.to_text());
        self.mode.stamp(Table::Cards, &mut after, self.now());
        self.conn
            .prepare_cached("UPDATE cards SET mod=?1,usn=?2,data=?3 WHERE id=?4")?
            .execute(rusqlite::params![after[4], after[5], after[17], id.0])?;
        let after = rows::stored(self.conn, Table::Cards, &[Cell::Integer(id.0)])?;
        self.journal_change(Table::Cards, key, Some(before), Some(after))?;
        Ok(Written::Updated)
    }

    fn add<R: Record>(&mut self, record: &R) -> Result<i64, StorageError> {
        let id = ids::allocate(self.conn, R::TABLE, self.now().0)?;
        let mut cells = record.to_cells();
        cells[0] = Cell::Integer(id);
        self.put_cells(R::TABLE, cells, false)?;
        Ok(id)
    }

    /// Adds a note under a new id (I2, I3); the note's own id is ignored.
    pub fn add_note(&mut self, note: &Note) -> Result<NoteId, StorageError> {
        self.add(note).map(NoteId)
    }

    /// Adds a card under a new id (I2, I3).
    pub fn add_card(&mut self, card: &Card) -> Result<CardId, StorageError> {
        self.add(card).map(CardId)
    }

    /// Adds a review-log row under a new id: the review instant, now (I2,
    /// I3).
    pub fn add_revlog(&mut self, entry: &RevlogEntry) -> Result<RevlogId, StorageError> {
        self.add(entry).map(RevlogId)
    }

    /// Allocate from the recorded answer instant (pending session replay may
    /// commit later). This remains an insertion, never an existing-row update.
    pub fn add_answer_revlog(
        &mut self,
        entry: &RevlogEntry,
        answered_at: EpochMillis,
    ) -> Result<RevlogId, StorageError> {
        let id = ids::allocate(self.conn, Table::Revlog, answered_at.0)?;
        let mut entry = entry.clone();
        entry.id = RevlogId(id);
        self.put(&entry)?;
        Ok(entry.id)
    }

    /// Replay an already prepared identity. Never overwrite a taken id.
    pub fn insert_prepared_answer(&mut self, entry: &RevlogEntry) -> Result<RevlogId, StorageError> {
        if rows::select_image(self.conn, Table::Revlog, &RowKey::Id(entry.id.0))?.is_some() {
            return Err(StorageError::Mode {
                detail: "prepared answer id is already occupied".into(),
            });
        }
        self.put(entry)?;
        Ok(entry.id)
    }

    /// Only answer-owned fields are replaced; identity, original-deck fields
    /// and flags retain their stored representation.
    pub fn put_answer_card(&mut self, card: &Card) -> Result<Written, StorageError> {
        let Some(mut image) = rows::select_image(self.conn, Table::Cards, &RowKey::Id(card.id.0))? else {
            return Err(StorageError::Mode {
                detail: "answer card disappeared".into(),
            });
        };
        let values = card.to_cells();
        for i in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 17] {
            image[i] = values[i].clone();
        }
        self.put_cells(Table::Cards, image, false)
    }

    /// Adds a deck under a new id (I2, I3).
    pub fn add_deck(&mut self, deck: &Deck) -> Result<DeckId, StorageError> {
        self.add(deck).map(DeckId)
    }

    /// Adds a deck preset under a new id (I2, I3).
    pub fn add_deck_config(&mut self, config: &DeckConfig) -> Result<DeckConfigId, StorageError> {
        self.add(config).map(DeckConfigId)
    }

    /// Inserts a review-log row that arrives with its id (I4): when a row
    /// with that id exists, nothing is inserted and that row stays.
    pub fn insert_revlog(&mut self, entry: &RevlogEntry) -> Result<Written, StorageError> {
        if rows::select_image(self.conn, Table::Revlog, &RowKey::Id(entry.id.0))?.is_some() {
            return Ok(Written::Unchanged);
        }
        self.put(entry)
    }

    pub fn remove_note(&mut self, id: NoteId) -> Result<Written, StorageError> {
        self.remove_key(Table::Notes, &RowKey::Id(id.0))
    }

    pub fn remove_card(&mut self, id: CardId) -> Result<Written, StorageError> {
        self.remove_key(Table::Cards, &RowKey::Id(id.0))
    }

    pub fn remove_revlog(&mut self, id: RevlogId) -> Result<Written, StorageError> {
        self.remove_key(Table::Revlog, &RowKey::Id(id.0))
    }

    pub fn remove_deck(&mut self, id: DeckId) -> Result<Written, StorageError> {
        self.remove_key(Table::Decks, &RowKey::Id(id.0))
    }

    pub fn remove_deck_config(&mut self, id: DeckConfigId) -> Result<Written, StorageError> {
        self.remove_key(Table::DeckConfig, &RowKey::Id(id.0))
    }

    pub fn remove_tag(&mut self, name: &str) -> Result<Written, StorageError> {
        self.remove_key(Table::Tags, &RowKey::text(name))
    }

    /// Adds the grave of `oid` (graves G2): when that grave exists, it stays
    /// exactly as it is and nothing changes. `usn` is the grave's in a
    /// `CallerSupplied` or `Raw` scope; the other modes stamp -1.
    pub fn add_grave(&mut self, oid: i64, kind: GraveKind, usn: Usn) -> Result<Written, StorageError> {
        if rows::select_image(self.conn, Table::Graves, &graves::key(oid, kind))?.is_some() {
            return Ok(Written::Unchanged);
        }
        self.put(&Grave { oid, kind, usn })
    }

    pub fn remove_grave(&mut self, oid: i64, kind: GraveKind) -> Result<Written, StorageError> {
        self.remove_key(Table::Graves, &graves::key(oid, kind))
    }

    /// The storage side of deleting a note (graves G3): each of its cards
    /// removed with a card grave, in card id order, then the note with a
    /// note grave. A note that no longer exists is no change.
    pub fn remove_note_and_cards(&mut self, id: NoteId) -> Result<Written, StorageError> {
        if rows::select_image(self.conn, Table::Notes, &RowKey::Id(id.0))?.is_none() {
            return Ok(Written::Unchanged);
        }
        let cards = rows::select_where(self.conn, Table::Cards, "nid = ?1", &[Cell::Integer(id.0)])?;
        for card in cards {
            let Some(card_id) = card[0].as_i64() else { continue };
            self.remove_key(Table::Cards, &RowKey::Id(card_id))?;
            self.add_grave(card_id, GraveKind::Card, Usn::PENDING)?;
        }
        self.remove_key(Table::Notes, &RowKey::Id(id.0))?;
        self.add_grave(id.0, GraveKind::Note, Usn::PENDING)?;
        Ok(Written::Removed)
    }

    /// Writes a note type: its `notetypes` row, its `fields` and
    /// `templates` rows (removing only keys absent from the aggregate).
    /// When any of them changes, the `notetypes` row is stamped even if its
    /// own columns did not change (S1: template rows change with their note type).
    pub fn put_notetype(&mut self, notetype: &Notetype) -> Result<Written, StorageError> {
        let id = notetype.id;
        let fields: Vec<RowImage> = notetype.fields.iter().map(|f| notetypes::field_cells(id, f)).collect();
        let templates: Vec<RowImage> = notetype
            .templates
            .iter()
            .map(|t| notetypes::template_cells(id, t))
            .collect();
        let mut parts = Written::Unchanged;
        for (table, wanted) in [(Table::Fields, fields), (Table::Templates, templates)] {
            // Rows that change are removed first and written after, so names
            // that move between ordinals never meet in the unique (name,
            // ntid) index halfway.
            let mut changing = Vec::new();
            for cells in wanted.iter() {
                if !matches!(rows::select_compare(self.conn, table, cells)?, Some((_, true))) {
                    changing.push(cells.clone());
                }
            }
            // Stored ordinals may have gaps: list length cannot identify
            // stale children. Both sets are bounded to this note type.
            let wanted_keys: HashSet<RowKey> = wanted.iter().map(|cells| RowKey::of(table, cells)).collect();
            let existing = rows::select_where(self.conn, table, "ntid = ?1", &[Cell::Integer(id.0)])?;
            let stale = existing
                .iter()
                .filter(|cells| !wanted_keys.contains(&RowKey::of(table, cells)));
            for cells in stale.chain(&changing) {
                self.remove_key(table, &RowKey::of(table, cells))?;
                parts = Written::Updated;
            }
            for cells in changing {
                self.put_cells(table, cells, false)?;
            }
        }
        let head = self.put_cells(Table::Notetypes, notetypes::head_cells(notetype), parts.changed())?;
        Ok(if head.changed() { head } else { parts })
    }

    /// Adds a note type under a new id (I2, I3), its fields and templates
    /// with it.
    pub fn add_notetype(&mut self, notetype: &Notetype) -> Result<NotetypeId, StorageError> {
        let id = NotetypeId(ids::allocate(self.conn, Table::Notetypes, self.now().0)?);
        let mut notetype = notetype.clone();
        notetype.id = id;
        self.put_notetype(&notetype)?;
        Ok(id)
    }

    /// Removes a note type's `notetypes`, `fields` and `templates` rows. Its
    /// notes, cards and config keys are the note type deletion's (U29).
    pub fn remove_notetype(&mut self, id: NotetypeId) -> Result<Written, StorageError> {
        let mut written = Written::Unchanged;
        for table in [Table::Fields, Table::Templates] {
            for row in rows::select_where(self.conn, table, "ntid = ?1", &[Cell::Integer(id.0)])? {
                self.remove_key(table, &RowKey::of(table, &row))?;
                written = Written::Removed;
            }
        }
        if self.remove_key(Table::Notetypes, &RowKey::Id(id.0))?.changed() {
            written = Written::Removed;
        }
        Ok(written)
    }

    /// Sets a config key to a value (config-table C4): nothing is written
    /// when the stored value is the same value, whatever its bytes;
    /// otherwise the value is stored in compact form (C5) as a BLOB,
    /// stamped per the mode.
    pub fn set_config(&mut self, key: &str, value: &ConfigValue) -> Result<Written, StorageError> {
        self.set_config_ordered(key, value, &[])
    }

    /// C4 comparison and stamping, with selected root object members first.
    /// Nested values and remaining members use C5 (backups B6.2).
    pub fn set_config_ordered(
        &mut self,
        key: &str,
        value: &ConfigValue,
        first: &[&str],
    ) -> Result<Written, StorageError> {
        let stored = rows::select_image(self.conn, Table::Config, &RowKey::text(key))?;
        let (usn, mtime) = match &stored {
            Some(row) if same_value(&row[3], value) => return Ok(Written::Unchanged),
            Some(row) => (row[1].clone(), row[2].clone()),
            None => (Cell::Integer(0), Cell::Integer(0)),
        };
        let cells = vec![Cell::text(key), usn, mtime, Cell::Blob(value.compact_ordered(first).into_bytes())];
        self.put_cells(Table::Config, cells, false)
    }

    /// Changes members of an object-valued key (C6): every other member
    /// keeps its value and kind; a stored value that is not an object
    /// counts as an empty one. The result is written per C4.
    pub fn set_config_members(&mut self, key: &str, members: &[(&str, ConfigValue)]) -> Result<Written, StorageError> {
        let stored = rows::select_image(self.conn, Table::Config, &RowKey::text(key))?;
        let mut value = stored
            .as_ref()
            .and_then(|row| rows::config::stored_value(&row[3]))
            .unwrap_or(ConfigValue::Object(Vec::new()));
        for (name, member) in members {
            value = value.with_member(name, member.clone());
        }
        self.set_config(key, &value)
    }

    /// Writes a config row with its bytes as given (a sync, an undo).
    pub fn put_config_entry(&mut self, entry: &ConfigEntry) -> Result<Written, StorageError> {
        self.put(entry)
    }

    /// Removes a config key (C10): no grave; a missing key is no change.
    pub fn remove_config(&mut self, key: &str) -> Result<Written, StorageError> {
        self.remove_key(Table::Config, &RowKey::text(key))
    }

    /// Removes every key that begins with `prefix`, byte for byte (C10:
    /// `_nt_<id>_`, `_deck_<id>_`). Returns how many were removed.
    pub fn remove_config_family(&mut self, prefix: &str) -> Result<usize, StorageError> {
        let found = rows::select_where(
            self.conn,
            Table::Config,
            "substr(CAST(\"KEY\" AS BLOB), 1, ?2) = CAST(?1 AS BLOB)",
            &[Cell::text(prefix), Cell::Integer(prefix.len() as i64)],
        )?;
        for row in &found {
            self.remove_key(Table::Config, &RowKey::of(Table::Config, row))?;
        }
        Ok(found.len())
    }

    /// Writes the `col` row as given (no column of it is stamped row by
    /// row; S5 moves `mod` at commit).
    pub fn put_col(&mut self, col: &Col) -> Result<Written, StorageError> {
        self.put(col)
    }

    /// Sets `col.mod` (undo and redo, S5; a sync's end, R1531). Only in a
    /// `CallerSupplied` or `Raw` scope: the other modes set it at commit.
    pub fn set_col_mod(&mut self, at: EpochMillis) -> Result<Written, StorageError> {
        self.require_as_given("set_col_mod")?;
        self.set_col_mod_unchecked(at)
    }

    fn set_col_mod_unchecked(&mut self, at: EpochMillis) -> Result<Written, StorageError> {
        let Some(mut cells) = rows::select_image(self.conn, Table::Col, &COL_KEY)? else {
            return Err(StorageError::Mode {
                detail: "the collection has no col row".to_owned(),
            });
        };
        cells[2] = Cell::Integer(at.0);
        self.put_cells(Table::Col, cells, false)
    }

    /// Commits: in a `Local` or `KeepMtime` scope with a change, `col.mod`
    /// is set to now first (S5).
    pub fn commit(self) -> Result<Committed, StorageError> {
        self.commit_with(|_, _| Ok::<(), StorageError>(()))
            .map(|(committed, ())| committed)
    }

    /// Secure caller-owned compact commit facts after final col.mod stamping,
    /// including the requested local stamp even when its bytes are unchanged,
    /// while any image/decode failure can still roll back. The callback must
    /// not publish effects. Once COMMIT succeeds, only completed-commit errors
    /// are returned in `Committed::post_commit_error`.
    pub fn commit_with<T, E: From<StorageError>>(
        mut self,
        prepare: impl FnOnce(Option<&Journal>, Option<EpochMillis>) -> Result<T, E>,
    ) -> Result<(Committed, T), E> {
        let col_mod = if self.mode.moves_col_mod() && self.changed {
            let now = self.now();
            self.set_col_mod_unchecked(now).map_err(E::from)?;
            Some(now)
        } else {
            None
        };
        let facts = prepare(self.journal.as_ref(), col_mod)?;
        meguri_model::faultpoint!("storage.commit.before")
            .map_err(StorageError::from)
            .map_err(E::from)?;
        self.conn
            .execute_batch("COMMIT")
            .map_err(StorageError::from)
            .map_err(E::from)?;
        self.open = false;
        let detach_error = if self.normal_stage {
            self.conn
                .execute_batch("DETACH DATABASE meguri_sync")
                .err()
                .map(StorageError::from)
        } else {
            None
        };
        self.normal_stage = false;
        let post_commit_error = meguri_model::faultpoint!("storage.commit.after")
            .err()
            .map(StorageError::from)
            .or(detach_error);
        Ok((
            Committed {
                post_commit_error,
                journal: self.journal.take(),
                changed: self.changed,
                col_mod,
            },
            facts,
        ))
    }

    /// Rolls back every write of the scope.
    pub fn rollback(mut self) -> Result<(), StorageError> {
        self.open = false;
        self.conn.execute_batch("ROLLBACK")?;
        if self.normal_stage {
            self.conn.execute_batch("DETACH DATABASE meguri_sync")?;
            self.normal_stage = false;
        }
        Ok(())
    }
}

impl Drop for WriteScope<'_> {
    fn drop(&mut self) {
        if self.open {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
        if self.normal_stage {
            let _ = self.conn.execute_batch("DETACH DATABASE meguri_sync");
        }
    }
}

/// The storage-only builder: creates a new collection file with the
/// schema-18 tables and indexes, the given `col` row and config rows, opens
/// it through the open gate, and hands out `Raw` and `CallerSupplied` scopes
/// with no journal (U11's generator, the staging copies of U33, U122 and
/// U123, the one-way upload transform). It refuses a path where a file is
/// (or a `-wal`, `-shm` or `-journal` file next to it) and a path open as a
/// collection in this process.
#[derive(Debug)]
pub struct CollectionBuilder {
    collection: OpenedCollection,
}

/// The files next to a collection that SQLite makes.
const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

/// A path a builder claimed by creating its file exclusively. Dropped
/// before [`Claim::keep`], it removes the files of this attempt: the file it
/// created, while that file is still the one it created, and the sidecars,
/// none of which was there when it claimed the path. It never removes a
/// file another caller made.
pub(crate) struct Claim {
    path: PathBuf,
    /// Device and inode of the file the claim created.
    file: (u64, u64),
    settled: bool,
}

impl Claim {
    /// Creates the empty file at `path` with `O_CREAT | O_EXCL`, so of
    /// callers racing for one path exactly one gets it. Refused when the
    /// file exists or a sidecar is next to it (SQLite would read a stale
    /// `-wal` into the new file).
    pub(crate) fn take(path: &Path) -> Result<Claim, StorageError> {
        let exists = StorageError::BuilderRefused {
            reason: BuilderRefusal::Exists,
        };
        let io = |e: std::io::Error| StorageError::Io {
            detail: format!("creating the new collection: {e}"),
        };
        let file = match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(exists),
            Err(e) => return Err(io(e)),
        };
        let mut claim = Claim {
            path: path.to_path_buf(),
            file: (0, 0),
            settled: false,
        };
        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                // Unknown identity: the file is left rather than risk
                // removing another caller's.
                claim.settled = true;
                return Err(io(e));
            }
        };
        claim.file = (metadata.dev(), metadata.ino());
        if SIDECARS
            .iter()
            .any(|suffix| sibling(path, suffix).symlink_metadata().is_ok())
        {
            // Only the file this claim created goes; the sidecars stay.
            if claim.still_ours() {
                let _ = std::fs::remove_file(path);
            }
            claim.settled = true;
            return Err(exists);
        }
        Ok(claim)
    }

    /// Whether the file at the path is still the one this claim created.
    fn still_ours(&self) -> bool {
        self.path
            .symlink_metadata()
            .is_ok_and(|m| (m.dev(), m.ino()) == self.file)
    }

    fn keep(mut self) {
        self.settled = true;
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.settled || !self.still_ours() {
            return;
        }
        for suffix in SIDECARS {
            let _ = std::fs::remove_file(sibling(&self.path, suffix));
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

impl CollectionBuilder {
    /// Creates the file at `path`. `col` must be at schema 18, and `config`
    /// must hold `schedVer` 2: the open gate refuses a collection without it
    /// (`storage/open-and-connections` C2.2). When creating or opening the
    /// new file fails, the files this call made are removed.
    pub fn create(
        path: &Path,
        col: &Col,
        config: &[ConfigEntry],
        options: &OpenOptions<'_>,
    ) -> Result<CollectionBuilder, StorageError> {
        if open::is_open(path) {
            return Err(StorageError::BuilderRefused {
                reason: BuilderRefusal::Open,
            });
        }
        if col.version != SCHEMA_VERSION {
            return Err(StorageError::Mode {
                detail: format!("the builder writes schema 18, not {}", col.version),
            });
        }
        let claim = Claim::take(path)?;
        create_file(path, col, config)?;
        let collection = open::open_collection(path, options).map_err(|e| StorageError::Io {
            detail: format!("opening the new collection: {e}"),
        })?;
        claim.keep();
        Ok(CollectionBuilder { collection })
    }

    pub fn path(&self) -> &Path {
        self.collection.path()
    }

    /// A scope with no journal. `mode` is `Raw` or `CallerSupplied`.
    pub fn begin<'c>(&'c self, mode: StampMode, clock: &'c dyn Clock) -> Result<WriteScope<'c>, StorageError> {
        if !matches!(mode, StampMode::Raw | StampMode::CallerSupplied) {
            return Err(StorageError::Mode {
                detail: format!("the builder hands out Raw and CallerSupplied scopes, not {mode:?}"),
            });
        }
        WriteScope::begin(&self.collection.writer().conn, mode, clock, None)
    }

    /// Closes the file (`storage/open-and-connections` C10) and returns its
    /// path.
    pub fn finish(self) -> Result<PathBuf, StorageError> {
        let path = self.collection.path().to_path_buf();
        self.collection.close()?;
        Ok(path)
    }
}

/// The new file a [`Claim`] created, still empty: page size 4096, then the
/// schema-18 statements, the `col` row and the config rows in one
/// transaction. Opened without create: if the claimed file is gone, this
/// fails rather than make another.
fn create_file(path: &Path, col: &Col, config: &[ConfigEntry]) -> Result<(), StorageError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(file_uri(path, None), flags)?;
    crate::conn_init::probe(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    conn.execute_batch("PRAGMA page_size = 4096")?;
    let mut sql = String::from("BEGIN;\n");
    for statement in ddl18_statements() {
        sql.push_str(statement);
        sql.push_str(";\n");
    }
    conn.execute_batch(&sql)?;
    rows::insert(&conn, Table::Col, &col.to_cells())?;
    for entry in config {
        rows::insert(&conn, Table::Config, &entry.to_cells())?;
    }
    conn.execute_batch("COMMIT")?;
    conn.close().map_err(|(_, e)| StorageError::from(e))
}
