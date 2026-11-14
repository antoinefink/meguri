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
use crate::undo::names;
use crate::undo::restorers::Direction;

/// `col.mod`, by column index.
const COL_MOD: usize = 2;
/// `graves.usn`, by column index.
const GRAVE_USN: usize = 2;
/// How long after its first op began a step takes in note edits (A3).
const MERGE_SECS: i64 = 60;

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
    /// `col.mod` to restore when this step is undone or redone (M4.4, A7):
    /// its value just before the step's first op ran, or, for a step an
    /// undo or a redo made, when that undo or redo ran. `None` when the op
    /// did not write `col`.
    pub col_mod: Option<i64>,
    /// The epoch second at which the step's first op began, or the redo
    /// that put it back (A3).
    pub began: i64,
    pub upload: UploadState,
    /// Present only for CloudKit flags (M9.6-M9.7). Manual-transfer pending
    /// state has no map and cannot settle or clear these flags. Owned here
    /// so eviction, truncation and reset also discard the map.
    pub cloud_flagged: Option<BTreeSet<(Table, RowKey)>>,
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
    /// A step made by a committed op that began at epoch second `began`,
    /// from its journal and change set.
    pub(crate) fn recorded(
        info: StepInfo,
        journal: Arc<Journal>,
        changes: &ChangeSet,
        state: Option<RestorerState>,
        began: i64,
    ) -> Entry {
        let col_mod = col_mod_before(&journal);
        Entry {
            info,
            touched_notes: Arc::new(touched_notes(changes)),
            journal,
            col_mod,
            began,
            upload: UploadState::Local,
            cloud_flagged: None,
            state,
        }
    }

    /// The step an undo or redo of `self` leaves (M4.5, M5): the restoring
    /// op's journal under `self`'s name, made at epoch second `began`, with
    /// `col_mod` the value `col.mod` held when the restoring op ran (A7).
    pub(crate) fn inverse(
        &self,
        id: EntryId,
        journal: Arc<Journal>,
        changes: &ChangeSet,
        col_mod: Option<i64>,
        began: i64,
    ) -> Entry {
        Entry {
            info: StepInfo {
                id,
                ..self.info.clone()
            },
            touched_notes: Arc::new(touched_notes(changes)),
            journal,
            col_mod: self.col_mod.and(col_mod),
            began,
            upload: UploadState::Local,
            cloud_flagged: None,
            state: self.state.clone(),
        }
    }

    /// A3: takes `later`, a step just made, into this one when both are
    /// "Update Note" steps of the same session that changed one and the
    /// same note row and nothing else but `col`, this one is still local
    /// (never uploaded or pending, M9), and `later` began less than 60
    /// seconds after this step's first op (a clock moved back counts as 0).
    /// This step keeps its before-images, id, `col.mod` and time and takes
    /// `later`'s after-images, so its undo puts back the note as before its
    /// first edit and M4.2's check accepts it. Returns whether it merged.
    pub(crate) fn absorb(&mut self, later: &Entry) -> bool {
        let mergeable = self.upload == UploadState::Local
            && self.info.name == names::UPDATE_NOTE
            && later.info.name == names::UPDATE_NOTE
            && self.info.session == later.info.session
            && (later.began - self.began).max(0) < MERGE_SECS
            && single_note(&self.journal).is_some()
            && single_note(&self.journal) == single_note(&later.journal);
        if !mergeable {
            return false;
        }
        match self.journal.then(&later.journal) {
            Ok(journal) => {
                self.journal = Arc::new(journal);
                true
            }
            Err(error) => {
                tracing::warn!(%error, step = %self.info.id, "note edits not merged");
                false
            }
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
                    match (self.before(entry)?, self.journal.after(entry)?) {
                        (Some(before), Some(after)) => changed_columns(&before, &after)
                            .into_iter()
                            .all(|i| i == COL_MOD || current.get(i) == after.get(i)),
                        _ => false,
                    }
                }
                table => {
                    let expected = match entry.after() {
                        After::Absent => None,
                        After::Row(_) | After::Spilled { .. } => {
                            let image = self
                                .journal
                                .after(entry)?
                                .ok_or_else(|| EngineError::internal("missing final journal row"))?;
                            Some(journal::digest(&image))
                        }
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
    /// scope. A redo writes the graves its undo removed back pending (A5).
    pub(crate) fn restore(&self, scope: &mut WriteScope<'_>, direction: Direction) -> Result<(), EngineError> {
        // These maps were checked by the native op. Identity is its own
        // inverse, so the restored journal can carry the same report to redo.
        // Capture before restoring: those raw row writes invalidate reports.
        let reports: Vec<_> = self.journal.identity_layouts().map(|(id, report)| {
            scope.start_ordinal_report(
                meguri_model::ids::NotetypeId(id),
                (report.field_map.len(), report.template_map.len()),
            )
        }).collect();
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
            if let Some(mut before) = self.before(entry)? {
                if direction == Direction::Redo && entry.table() == Table::Graves && !entry.exists_after() {
                    before[GRAVE_USN] = Cell::Integer(-1);
                }
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
        self.restore_col(scope)?;
        for report in reports {
            scope.complete_ordinal_report(report);
        }
        Ok(())
    }

    fn restore_col(&self, scope: &mut WriteScope<'_>) -> Result<(), EngineError> {
        let Some(entry) = self.journal.entries().find(|entry| entry.table() == Table::Col) else {
            return Ok(());
        };
        let Some(current) = scope.row_image(Table::Col, entry.key())? else {
            return Err(EngineError::internal("the collection has no col row"));
        };
        let mut image = current.clone();
        if let (Some(before), Some(after)) = (self.before(entry)?, self.journal.after(entry)?) {
            for i in changed_columns(&before, &after) {
                if i != COL_MOD {
                    image[i] = before[i].clone();
                }
            }
        }
        if let Some(restore) = self.col_mod {
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

/// `col.mod` before the op, from its journal, when the op wrote `col`.
fn col_mod_before(journal: &Journal) -> Option<i64> {
    journal.col_mod_before()
}

/// The key of the one note row a journal changed (there before and after),
/// when it changed nothing else but `col` (A3).
fn single_note(journal: &Journal) -> Option<&RowKey> {
    let mut rows = journal.entries().filter(|entry| entry.table() != Table::Col);
    let note = rows.next()?;
    (rows.next().is_none() && note.table() == Table::Notes && note.existed_before() && note.exists_after())
        .then(|| note.key())
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
