//! The change value types (KTD2, `specs/engine/change-stream.md` S1): what
//! a committed op reports. The engine derives a [`ChangeSet`] from the op's
//! write journal; scheduling, search and stats read these types without
//! depending on the engine.

use std::fmt;

use crate::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};

/// Where an op comes from (`engine/op-model` O1). It decides the op's queue,
/// its default stamping and, in U25, its undo effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OpSource {
    /// A user action the user can undo: answers, edits, deck and note
    /// actions.
    UserUndoable,
    /// A user write that cannot be undone and clears undo.
    UserNonUndoable,
    /// A normal sync's merge commit.
    SyncIncoming,
    /// AnkiConnect and Migaku writes.
    LocalApi,
    /// The FSRS optimizer's commit.
    Optimizer,
    /// Day-rollover writes.
    Rollover,
    /// Check Database, Empty Cards and other maintenance.
    Maintenance,
}

impl OpSource {
    pub const ALL: [OpSource; 7] = [
        OpSource::UserUndoable,
        OpSource::UserNonUndoable,
        OpSource::SyncIncoming,
        OpSource::LocalApi,
        OpSource::Optimizer,
        OpSource::Rollover,
        OpSource::Maintenance,
    ];

    /// Whether the op waits in the interactive queue, ahead of background
    /// ops (O2): user writes and local-server writes, which share one FIFO.
    pub fn is_interactive(self) -> bool {
        matches!(
            self,
            OpSource::UserUndoable | OpSource::UserNonUndoable | OpSource::LocalApi
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OpSource::UserUndoable => "user_undoable",
            OpSource::UserNonUndoable => "user_non_undoable",
            OpSource::SyncIncoming => "sync_incoming",
            OpSource::LocalApi => "local_api",
            OpSource::Optimizer => "optimizer",
            OpSource::Rollover => "rollover",
            OpSource::Maintenance => "maintenance",
        }
    }
}

impl fmt::Display for OpSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How one row changed in an op (S1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// The row did not exist before the op.
    Added,
    /// The row existed before and after the op.
    Updated,
    /// The row existed before the op and not after it.
    Removed,
}

/// A changed card, with its note so consumers need no card→note lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CardChange {
    pub id: CardId,
    /// The note after the op; for a removed card, before it.
    pub note_id: NoteId,
    pub kind: ChangeKind,
    /// Only the FSRS values of `data` (`s`, `d`, `dr`, `decay`, `lrt`) and
    /// the stamps changed: nothing a queue or a due date depends on.
    pub memory_state_only: bool,
}

impl CardChange {
    /// The opposite of [`CardChange::memory_state_only`].
    pub fn scheduling_relevant(&self) -> bool {
        !self.memory_state_only
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteChange {
    pub id: NoteId,
    pub kind: ChangeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NotetypeChange {
    pub id: NotetypeId,
    pub kind: ChangeKind,
    /// Added or removed, or a field or template added, removed or renamed.
    pub schema_changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeckChange {
    pub id: DeckId,
    pub kind: ChangeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeckConfigChange {
    pub id: DeckConfigId,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConfigKeyChange {
    pub key: String,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TagChange {
    /// The tag as stored after the op (before it, for a removed tag).
    pub name: String,
    pub kind: ChangeKind,
}

/// Everything one committed op changed (S1). Every list is sorted by its key
/// and holds each key once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    pub source: OpSource,
    pub cards: Vec<CardChange>,
    pub notes: Vec<NoteChange>,
    pub notetypes: Vec<NotetypeChange>,
    pub decks: Vec<DeckChange>,
    pub deck_configs: Vec<DeckConfigChange>,
    pub config_keys: Vec<ConfigKeyChange>,
    pub tags: Vec<TagChange>,
    pub revlog_added: Vec<RevlogId>,
    pub revlog_removed: Vec<RevlogId>,
    pub media_added: Vec<String>,
    pub media_removed: Vec<String>,
    /// A `col` column other than `mod` changed (`scm`, `usn`, `ls`, ...).
    pub collection_row: bool,
}

impl ChangeSet {
    /// A change set that lists nothing.
    pub fn empty(source: OpSource) -> ChangeSet {
        ChangeSet {
            source,
            cards: Vec::new(),
            notes: Vec::new(),
            notetypes: Vec::new(),
            decks: Vec::new(),
            deck_configs: Vec::new(),
            config_keys: Vec::new(),
            tags: Vec::new(),
            revlog_added: Vec::new(),
            revlog_removed: Vec::new(),
            media_added: Vec::new(),
            media_removed: Vec::new(),
            collection_row: false,
        }
    }

    /// Whether the change set lists nothing; such a set is never published.
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
            && self.notes.is_empty()
            && self.notetypes.is_empty()
            && self.decks.is_empty()
            && self.deck_configs.is_empty()
            && self.config_keys.is_empty()
            && self.tags.is_empty()
            && self.revlog_added.is_empty()
            && self.revlog_removed.is_empty()
            && self.media_added.is_empty()
            && self.media_removed.is_empty()
            && !self.collection_row
    }

    /// The change of a card, if the op changed it.
    pub fn card(&self, id: CardId) -> Option<&CardChange> {
        self.cards
            .binary_search_by_key(&id, |c| c.id)
            .ok()
            .map(|i| &self.cards[i])
    }

    /// The change of a note, if the op changed it.
    pub fn note(&self, id: NoteId) -> Option<&NoteChange> {
        self.notes
            .binary_search_by_key(&id, |n| n.id)
            .ok()
            .map(|i| &self.notes[i])
    }
}
