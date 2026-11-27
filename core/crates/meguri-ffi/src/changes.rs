//! What Swift receives in a `Changes` event (KTD9): the engine's change set
//! (`engine/change-stream` S1) with plain ids. Lists keep the engine's order:
//! sorted by key, each key once.

use meguri_model::changes as model;

/// Where an op comes from (`engine/op-model` O1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum OpSource {
    UserUndoable,
    UserNonUndoable,
    SyncIncoming,
    LocalApi,
    Optimizer,
    Rollover,
    Maintenance,
}

/// How one row changed in an op.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChangeKind {
    Added,
    Updated,
    Removed,
}

/// A changed card, with its note.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardChange {
    pub id: i64,
    /// The note after the op; for a removed card, before it.
    pub note_id: i64,
    pub kind: ChangeKind,
    /// Only the FSRS memory state and the stamps changed: nothing a queue or
    /// a due date depends on.
    pub memory_state_only: bool,
}

/// A changed note, deck or deck config.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct IdChange {
    pub id: i64,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChange {
    pub id: i64,
    pub kind: ChangeKind,
    /// Added or removed, or a field or template added, removed or renamed.
    pub schema_changed: bool,
}

/// A changed config key or tag.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NameChange {
    pub name: String,
    pub kind: ChangeKind,
}

/// Everything one committed op changed.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ChangeSet {
    pub whole_collection: bool,
    pub source: OpSource,
    pub cards: Vec<CardChange>,
    pub notes: Vec<IdChange>,
    pub notetypes: Vec<NotetypeChange>,
    pub decks: Vec<IdChange>,
    pub deck_configs: Vec<IdChange>,
    pub config_keys: Vec<NameChange>,
    pub tags: Vec<NameChange>,
    pub revlog_added: Vec<i64>,
    pub revlog_removed: Vec<i64>,
    pub media_added: Vec<String>,
    pub media_removed: Vec<String>,
    pub skipped_incoming_cards: Vec<i64>,
    pub skipped_incoming_notes: Vec<i64>,
    pub skipped_incoming_decks: Vec<i64>,
    /// A `col` column other than `mod` changed.
    pub collection_row: bool,
}

impl From<model::OpSource> for OpSource {
    fn from(source: model::OpSource) -> OpSource {
        match source {
            model::OpSource::UserUndoable => OpSource::UserUndoable,
            model::OpSource::UserNonUndoable => OpSource::UserNonUndoable,
            model::OpSource::SyncIncoming => OpSource::SyncIncoming,
            model::OpSource::LocalApi => OpSource::LocalApi,
            model::OpSource::Optimizer => OpSource::Optimizer,
            model::OpSource::Rollover => OpSource::Rollover,
            model::OpSource::Maintenance => OpSource::Maintenance,
            model::OpSource::CloudIncoming => OpSource::SyncIncoming,
            model::OpSource::CloudRekey => OpSource::SyncIncoming,
        }
    }
}

impl From<model::ChangeKind> for ChangeKind {
    fn from(kind: model::ChangeKind) -> ChangeKind {
        match kind {
            model::ChangeKind::Added => ChangeKind::Added,
            model::ChangeKind::Updated => ChangeKind::Updated,
            model::ChangeKind::Removed => ChangeKind::Removed,
        }
    }
}

impl From<&model::ChangeSet> for ChangeSet {
    fn from(changes: &model::ChangeSet) -> ChangeSet {
        ChangeSet {
            whole_collection: changes.whole_collection,
            source: changes.source.into(),
            cards: changes
                .cards
                .iter()
                .map(|c| CardChange {
                    id: c.id.0,
                    note_id: c.note_id.0,
                    kind: c.kind.into(),
                    memory_state_only: c.memory_state_only,
                })
                .collect(),
            notes: id_changes(&changes.notes, |n| (n.id.0, n.kind)),
            notetypes: changes
                .notetypes
                .iter()
                .map(|n| NotetypeChange {
                    id: n.id.0,
                    kind: n.kind.into(),
                    schema_changed: n.schema_changed,
                })
                .collect(),
            decks: id_changes(&changes.decks, |d| (d.id.0, d.kind)),
            deck_configs: id_changes(&changes.deck_configs, |d| (d.id.0, d.kind)),
            config_keys: changes
                .config_keys
                .iter()
                .map(|c| NameChange {
                    name: c.key.clone(),
                    kind: c.kind.into(),
                })
                .collect(),
            tags: changes
                .tags
                .iter()
                .map(|t| NameChange {
                    name: t.name.clone(),
                    kind: t.kind.into(),
                })
                .collect(),
            revlog_added: changes.revlog_added.iter().map(|id| id.0).collect(),
            revlog_removed: changes.revlog_removed.iter().map(|id| id.0).collect(),
            media_added: changes.media_added.clone(),
            media_removed: changes.media_removed.clone(),
            skipped_incoming_cards: changes.skipped_incoming_cards.iter().map(|id| id.0).collect(),
            skipped_incoming_notes: changes.skipped_incoming_notes.iter().map(|id| id.0).collect(),
            skipped_incoming_decks: changes.skipped_incoming_decks.iter().map(|id| id.0).collect(),
            collection_row: changes.collection_row,
        }
    }
}

impl From<OpSource> for model::OpSource {
    fn from(source: OpSource) -> model::OpSource {
        match source {
            OpSource::UserUndoable => model::OpSource::UserUndoable,
            OpSource::UserNonUndoable => model::OpSource::UserNonUndoable,
            OpSource::SyncIncoming => model::OpSource::SyncIncoming,
            OpSource::LocalApi => model::OpSource::LocalApi,
            OpSource::Optimizer => model::OpSource::Optimizer,
            OpSource::Rollover => model::OpSource::Rollover,
            OpSource::Maintenance => model::OpSource::Maintenance,
        }
    }
}

impl From<ChangeKind> for model::ChangeKind {
    fn from(kind: ChangeKind) -> model::ChangeKind {
        match kind {
            ChangeKind::Added => model::ChangeKind::Added,
            ChangeKind::Updated => model::ChangeKind::Updated,
            ChangeKind::Removed => model::ChangeKind::Removed,
        }
    }
}

/// A change set Swift received in a `Changes` event, back in the engine's
/// terms: what Browse hands to `invalidated_rows` (`browser/result-sets`
/// H10). Every list is copied as it is.
impl From<&ChangeSet> for model::ChangeSet {
    fn from(changes: &ChangeSet) -> model::ChangeSet {
        use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
        model::ChangeSet {
            whole_collection: changes.whole_collection,
            source: changes.source.into(),
            cards: changes
                .cards
                .iter()
                .map(|c| model::CardChange {
                    id: CardId(c.id),
                    note_id: NoteId(c.note_id),
                    kind: c.kind.into(),
                    memory_state_only: c.memory_state_only,
                })
                .collect(),
            notes: changes
                .notes
                .iter()
                .map(|n| model::NoteChange {
                    id: NoteId(n.id),
                    kind: n.kind.into(),
                })
                .collect(),
            notetypes: changes
                .notetypes
                .iter()
                .map(|n| model::NotetypeChange {
                    id: NotetypeId(n.id),
                    kind: n.kind.into(),
                    schema_changed: n.schema_changed,
                })
                .collect(),
            decks: changes
                .decks
                .iter()
                .map(|d| model::DeckChange {
                    id: DeckId(d.id),
                    kind: d.kind.into(),
                })
                .collect(),
            deck_configs: changes
                .deck_configs
                .iter()
                .map(|d| model::DeckConfigChange {
                    id: DeckConfigId(d.id),
                    kind: d.kind.into(),
                })
                .collect(),
            config_keys: changes
                .config_keys
                .iter()
                .map(|c| model::ConfigKeyChange {
                    key: c.name.clone(),
                    kind: c.kind.into(),
                })
                .collect(),
            tags: changes
                .tags
                .iter()
                .map(|t| model::TagChange {
                    name: t.name.clone(),
                    kind: t.kind.into(),
                })
                .collect(),
            revlog_added: changes.revlog_added.iter().copied().map(RevlogId).collect(),
            revlog_removed: changes.revlog_removed.iter().copied().map(RevlogId).collect(),
            media_added: changes.media_added.clone(),
            media_removed: changes.media_removed.clone(),
            skipped_incoming_cards: changes.skipped_incoming_cards.iter().copied().map(CardId).collect(),
            skipped_incoming_notes: changes.skipped_incoming_notes.iter().copied().map(NoteId).collect(),
            skipped_incoming_decks: changes.skipped_incoming_decks.iter().copied().map(DeckId).collect(),
            collection_row: changes.collection_row,
        }
    }
}

fn id_changes<T>(list: &[T], entry: impl Fn(&T) -> (i64, model::ChangeKind)) -> Vec<IdChange> {
    list.iter()
        .map(|item| {
            let (id, kind) = entry(item);
            IdChange { id, kind: kind.into() }
        })
        .collect()
}

/// A change set with every list filled, for tests and `test_emit_changes`:
/// source LocalApi; card 11 of note 21 updated (memory state only) and card
/// 12 of note 21 added; note 21 added; notetype 31 updated with a schema
/// change; deck 41 removed; deck config 51 added; config key `curDeck`
/// updated; tag `leech` added; review-log entry 61 added and 62 removed;
/// media `a.jpg` added and `b.mp3` removed; the `col` row changed.
/// Synthetic diagnostics fill each list with independent numeric keys, including
/// 71 in all three entity kinds.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn sample() -> model::ChangeSet {
    use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
    model::ChangeSet {
        whole_collection: false,
        source: model::OpSource::LocalApi,
        cards: vec![
            model::CardChange {
                id: CardId(11),
                note_id: NoteId(21),
                kind: model::ChangeKind::Updated,
                memory_state_only: true,
            },
            model::CardChange {
                id: CardId(12),
                note_id: NoteId(21),
                kind: model::ChangeKind::Added,
                memory_state_only: false,
            },
        ],
        notes: vec![model::NoteChange {
            id: NoteId(21),
            kind: model::ChangeKind::Added,
        }],
        notetypes: vec![model::NotetypeChange {
            id: NotetypeId(31),
            kind: model::ChangeKind::Updated,
            schema_changed: true,
        }],
        decks: vec![model::DeckChange {
            id: DeckId(41),
            kind: model::ChangeKind::Removed,
        }],
        deck_configs: vec![model::DeckConfigChange {
            id: DeckConfigId(51),
            kind: model::ChangeKind::Added,
        }],
        config_keys: vec![model::ConfigKeyChange {
            key: "curDeck".to_owned(),
            kind: model::ChangeKind::Updated,
        }],
        tags: vec![model::TagChange {
            name: "leech".to_owned(),
            kind: model::ChangeKind::Added,
        }],
        revlog_added: vec![RevlogId(61)],
        revlog_removed: vec![RevlogId(62)],
        media_added: vec!["a.jpg".to_owned()],
        media_removed: vec!["b.mp3".to_owned()],
        skipped_incoming_cards: vec![CardId(71), CardId(72)],
        skipped_incoming_notes: vec![NoteId(71), NoteId(73)],
        skipped_incoming_decks: vec![DeckId(71), DeckId(74)],
        collection_row: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_set_comes_back_from_swift_as_the_engine_sent_it() {
        let engine = sample();
        assert_eq!(model::ChangeSet::from(&ChangeSet::from(&engine)), engine);
    }
}
