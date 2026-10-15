//! Steps (`specs/engine/undo.md` M1): what one undoable op leaves, and how an
//! undo or redo checks and restores its rows (M4.2 to M4.4).

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use meguri_model::changes::ChangeSet;
use meguri_model::grave::GraveKind;
use meguri_storage::journal::{self, After, Entry as JournalEntry, Journal};
use meguri_storage::rows::graves;
use meguri_storage::{Cell, ReadRows, RowImage, RowKey, Table, WriteScope};

use crate::changes::stream::Seq;
use crate::error::EngineError;

/// `col.mod`, by column index.
const COL_MOD: usize = 2;

/// A step's id: the seq of the op that made it (M1), so "made after a
/// sync's snapshot" is a comparison of seqs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub u64);

impl EntryId {
    pub fn seq(self) -> Seq {
        Seq(self.0)
    }
}

impl From<Seq> for EntryId {
    fn from(seq: Seq) -> EntryId {
        EntryId(seq.0)
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A review session (U43): steps carry the session they were made in, for
/// session-scoped undo and the held-back run (M3, M8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

/// What kind of step: only the held-back run reads it (M8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StepKind {
    Answer,
    #[default]
    Other,
}

/// A step as callers see it (M3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepInfo {
    pub id: EntryId,
    /// Anki's op name ("Answer Card", R370), or the op's name.
    pub name: &'static str,
    pub kind: StepKind,
    pub session: Option<SessionId>,
    /// The write phase the step's op declared (`engine/op-model` O8); its
    /// undo and redo declare the same one (M4).
    pub long_write: Option<Duration>,
}

/// Whether a sync uploaded the step (M9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UploadState {
    #[default]
    Local,
    /// In the snapshot of a running sync.
    Pending,
    Uploaded,
}

/// The opaque state an op attaches for restorers (M10).
pub type RestorerState = Arc<dyn Any + Send + Sync>;

/// One step of the history or of redo.
#[derive(Clone)]
pub(crate) struct Entry {
    pub info: StepInfo,
    pub journal: Arc<Journal>,
    /// The notes the step changed, and the notes of the cards it changed.
    pub touched_notes: Arc<BTreeSet<i64>>,
    /// `col.mod` to restore when this step is undone, and the value the
    /// step set (M4.4, M4.5).
    pub col_mod: Option<(i64, i64)>,
    pub upload: UploadState,
    pub state: Option<RestorerState>,
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("info", &self.info)
            .field("rows", &self.journal.len())
            .field("upload", &self.upload)
            .finish_non_exhaustive()
    }
}

impl Entry {
    /// A step made by a committed op from its journal and change set.
    pub(crate) fn recorded(
        info: StepInfo,
        journal: Arc<Journal>,
        changes: &ChangeSet,
        state: Option<RestorerState>,
    ) -> Entry {
        let col_mod = col_mod_change(&journal);
        Entry {
            info,
            touched_notes: Arc::new(touched_notes(changes)),
            journal,
            col_mod,
            upload: UploadState::Local,
            state,
        }
    }

    /// The step an undo or redo of `self` leaves: the restoring op's journal
    /// under `self`'s name, with `col.mod` swapped (M4.5, M5).
    pub(crate) fn inverse(&self, id: EntryId, journal: Arc<Journal>, changes: &ChangeSet) -> Entry {
        Entry {
            info: StepInfo {
                id,
                ..self.info.clone()
            },
            touched_notes: Arc::new(touched_notes(changes)),
            journal,
            col_mod: self.col_mod.map(|(restore, set)| (set, restore)),
            upload: UploadState::Local,
            state: self.state.clone(),
        }
    }

    /// Whether a change to `target` reaches this step's touched set (M1,
    /// M7, M9.4). A grave reaches the step that changed its card, note or
    /// deck. With `expand_notes`, a note reaches every step that touched it
    /// or one of its cards (AE80).
    pub(crate) fn intersects(&self, target: &(Table, RowKey), expand_notes: bool) -> bool {
        let (table, key) = target;
        if *table == Table::Col {
            return false;
        }
        if self.journal.entry(*table, key).is_some() {
            return true;
        }
        if let Some(object) = grave_target(*table, key)
            && self.journal.entry(object.0, &object.1).is_some()
        {
            return true;
        }
        expand_notes && *table == Table::Notes && matches!(key, RowKey::Id(id) if self.touched_notes.contains(id))
    }

    /// M4.2: whether every row the step changed still holds its after-image
    /// (the `col` row: the columns it changed other than `mod`).
    pub(crate) fn still_applied(&self, scope: &WriteScope<'_>) -> Result<bool, EngineError> {
        for entry in self.journal.entries() {
            let holds = match entry.table() {
                Table::Col => {
                    let Some(current) = scope.row_image(Table::Col, entry.key())? else {
                        return Ok(false);
                    };
                    match (self.before(entry)?, entry.after()) {
                        (Some(before), After::Row(after)) => changed_columns(&before, after)
                            .into_iter()
                            .all(|i| i == COL_MOD || current.get(i) == after.get(i)),
                        _ => false,
                    }
                }
                table => {
                    let expected = match entry.after() {
                        After::Absent => None,
                        After::Row(image) => Some(journal::digest(image)),
                        After::Note(note) => Some(note.sha256),
                    };
                    scope.row_digest(table, entry.key())? == expected
                }
            };
            if !holds {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// M4.3, M4.4: writes every before-image back, in a `CallerSupplied`
    /// scope.
    pub(crate) fn restore(&self, scope: &mut WriteScope<'_>) -> Result<(), EngineError> {
        let rows: Vec<&JournalEntry> = self
            .journal
            .entries()
            .filter(|entry| entry.table() != Table::Col)
            .collect();
        // Rows the step made go; rows of tables with unique names go first,
        // so a name that moved between rows never meets itself.
        for entry in &rows {
            if entry.exists_after() && (!entry.existed_before() || has_unique_name(entry.table())) {
                scope.restore(entry.table(), entry.key(), None)?;
            }
        }
        for entry in &rows {
            if let Some(before) = self.before(entry)? {
                scope.restore(entry.table(), entry.key(), Some(&before))?;
            }
        }
        // An object the step removed without adding its grave (it was there
        // already, graves G2) loses that grave too, as in Anki (graves G6).
        for entry in &rows {
            if !entry.existed_before() || entry.exists_after() {
                continue;
            }
            let (Some(kind), RowKey::Id(oid)) = (grave_kind(entry.table()), entry.key()) else {
                continue;
            };
            if self.journal.entry(Table::Graves, &graves::key(*oid, kind)).is_none() {
                scope.remove_grave(*oid, kind)?;
            }
        }
        self.restore_col(scope)
    }

    fn restore_col(&self, scope: &mut WriteScope<'_>) -> Result<(), EngineError> {
        let Some(entry) = self.journal.entries().find(|entry| entry.table() == Table::Col) else {
            return Ok(());
        };
        let Some(current) = scope.row_image(Table::Col, entry.key())? else {
            return Err(EngineError::internal("the collection has no col row"));
        };
        let mut image = current.clone();
        if let (Some(before), After::Row(after)) = (self.before(entry)?, entry.after()) {
            for i in changed_columns(&before, after) {
                if i != COL_MOD {
                    image[i] = before[i].clone();
                }
            }
        }
        if let Some((restore, _)) = self.col_mod {
            image[COL_MOD] = Cell::Integer(restore);
        }
        if image != current {
            scope.restore(Table::Col, entry.key(), Some(&image))?;
        }
        Ok(())
    }

    pub(crate) fn before(&self, entry: &JournalEntry) -> Result<Option<RowImage>, EngineError> {
        Ok(self.journal.before(entry)?)
    }
}

/// The notes a change set reaches: changed notes and the notes of changed
/// cards.
fn touched_notes(changes: &ChangeSet) -> BTreeSet<i64> {
    changes
        .notes
        .iter()
        .map(|note| note.id.0)
        .chain(changes.cards.iter().map(|card| card.note_id.0))
        .collect()
}

/// `col.mod` before and after the op, from its journal.
fn col_mod_change(journal: &Journal) -> Option<(i64, i64)> {
    let entry = journal.entries().find(|entry| entry.table() == Table::Col)?;
    let before = journal.before(entry).ok().flatten()?;
    let After::Row(after) = entry.after() else {
        return None;
    };
    Some((before.get(COL_MOD)?.as_i64()?, after.get(COL_MOD)?.as_i64()?))
}

fn changed_columns(before: &[Cell], after: &[Cell]) -> Vec<usize> {
    (0..before.len().max(after.len()))
        .filter(|i| before.get(*i) != after.get(*i))
        .collect()
}

/// Tables with a unique index on a name (`storage/schema-18` S3).
fn has_unique_name(table: Table) -> bool {
    matches!(
        table,
        Table::Decks | Table::Notetypes | Table::Fields | Table::Templates
    )
}

/// The grave kind of an object table.
fn grave_kind(table: Table) -> Option<GraveKind> {
    match table {
        Table::Cards => Some(GraveKind::Card),
        Table::Notes => Some(GraveKind::Note),
        Table::Decks => Some(GraveKind::Deck),
        _ => None,
    }
}

/// The card, note or deck a grave key names.
pub(crate) fn grave_target(table: Table, key: &RowKey) -> Option<(Table, RowKey)> {
    let (Table::Graves, RowKey::Pair(oid, kind)) = (table, key) else {
        return None;
    };
    let object = match GraveKind::from_stored(*kind)? {
        GraveKind::Card => Table::Cards,
        GraveKind::Note => Table::Notes,
        GraveKind::Deck => Table::Decks,
    };
    Some((object, RowKey::Id(*oid)))
}

/// The keys an op's journal changed, as sync and local-server targets: the
/// `col` row left out, graves given as the object they delete (M7, M9.4).
pub(crate) fn journal_targets(journal: &Journal) -> Vec<(Table, RowKey)> {
    journal
        .entries()
        .filter(|entry| entry.table() != Table::Col)
        .map(|entry| grave_target(entry.table(), entry.key()).unwrap_or_else(|| (entry.table(), entry.key().clone())))
        .collect()
}
