//! Change sets, in-core observers and the change stream
//! (`specs/engine/change-stream.md`). This module derives an op's change
//! set and observer payload from its write journal (S1, S2).

pub mod observers;
pub mod stream;

use std::collections::BTreeMap;

use meguri_model::card_data::CardData;
use meguri_model::changes::{
    CardChange, ChangeKind, ChangeSet, ConfigKeyChange, DeckChange, DeckConfigChange, NoteChange, NotetypeChange,
    OpSource, TagChange,
};
use meguri_model::col::Col;
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_storage::derived::CollectionMark;
use meguri_storage::journal::{ColumnSet, Entry, Journal};
use meguri_storage::{Cell, Record, RowKey, StorageError, Table};

/// The `cards` columns a memory-state-only change may touch (S1): `mod`,
/// `usn` and `data`.
const CARD_MOD: usize = 4;
const CARD_USN: usize = 5;
const CARD_DATA: usize = 17;
/// `col.mod`.
const COL_MOD: usize = 2;
/// The `name` column of `fields` and `templates`.
const PART_NAME: usize = 2;

/// What the writer needs from a committed op's journal.
pub(crate) struct Derived {
    pub changes: ChangeSet,
    /// The `col` row after the op, when the op changed it.
    pub collection: Option<CollectionMark>,
    /// Rows the journal holds.
    pub rows: usize,
    pub local_modification: bool,
}

/// Media names an op reported through its context.
#[derive(Debug, Default)]
pub(crate) struct MediaNotes {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Validates and secures S1 compact facts after final stamping and before
/// COMMIT. No observer payload is retained here and no fallible image access
/// is required to construct/publish the committed ChangeSet afterwards.
pub(crate) fn prepare(
    journal: Option<&Journal>,
    source: OpSource,
    media: MediaNotes,
    local_modification: bool,
) -> Result<Derived, StorageError> {
    let mut changes = ChangeSet::empty(source);
    let mut collection = None;
    let mut notetypes: BTreeMap<i64, (Option<ChangeKind>, bool)> = BTreeMap::new();
    let mut rows = 0;

    for (journal, entry) in journal
        .into_iter()
        .flat_map(|journal| journal.entries().map(move |entry| (journal, entry)))
    {
        rows += 1;
        let Some(kind) = kind_of(entry) else { continue };
        match entry.table() {
            Table::Col => {
                let before = journal.before(entry)?;
                let after = journal.after(entry)?;
                if let Some(image) = &after {
                    collection = Some(CollectionMark::of(&Col::from_cells(image)?));
                }
                if let Some(image) = &before {
                    Col::from_cells(image)?;
                }
                changes.collection_row = match (&before, &after) {
                    (Some(before), Some(after)) => {
                        let changed = ColumnSet::changed(before, after);
                        (0..after.len()).any(|i| i != COL_MOD && changed.contains(i))
                    }
                    _ => true,
                };
            }
            Table::Notes => {
                if let RowKey::Id(id) = entry.key() {
                    changes.notes.push(NoteChange { id: NoteId(*id), kind });
                }
            }
            Table::Cards => {
                let RowKey::Id(id) = entry.key() else { continue };
                let before = journal.before(entry)?;
                let after = journal.after(entry)?;
                // Decode card identities before the transaction becomes durable.
                let before_card = before
                    .as_deref()
                    .map(meguri_model::card::Card::from_cells)
                    .transpose()?;
                let after_card = after.as_deref().map(meguri_model::card::Card::from_cells).transpose()?;
                let note = after_card
                    .as_ref()
                    .or(before_card.as_ref())
                    .ok_or_else(|| StorageError::Io {
                        detail: "changed card has no image".into(),
                    })?
                    .note_id;
                let memory_state_only = match (&before, &after) {
                    (Some(before), Some(after)) => memory_state_only(before, after),
                    _ => false,
                };
                changes.cards.push(CardChange {
                    id: CardId(*id),
                    note_id: note,
                    kind,
                    memory_state_only,
                });
            }
            Table::Revlog => {
                let RowKey::Id(id) = entry.key() else { continue };
                match kind {
                    ChangeKind::Added => {
                        changes.revlog_added.push(RevlogId(*id));
                    }
                    ChangeKind::Removed => {
                        changes.revlog_removed.push(RevlogId(*id));
                    }
                    ChangeKind::Updated => {}
                }
            }
            Table::Decks => {
                if let RowKey::Id(id) = entry.key() {
                    changes.decks.push(DeckChange { id: DeckId(*id), kind });
                }
            }
            Table::DeckConfig => {
                if let RowKey::Id(id) = entry.key() {
                    changes.deck_configs.push(DeckConfigChange {
                        id: DeckConfigId(*id),
                        kind,
                    });
                }
            }
            Table::Config => {
                let RowKey::Text(key) = entry.key() else { continue };
                let key = String::from_utf8(key.clone()).map_err(|_| meguri_storage::RowError {
                    table: "config",
                    column: "key",
                    key: String::new(),
                    reason: "invalid UTF-8 key".into(),
                })?;
                changes.config_keys.push(ConfigKeyChange { key: key.clone(), kind });
            }
            Table::Tags => {
                let image = if entry.exists_after() {
                    journal.after(entry)?
                } else {
                    journal.before(entry)?
                };
                let name = match image.as_ref().and_then(|i| i.first()) {
                    Some(Cell::Text(bytes)) => {
                        String::from_utf8(bytes.clone()).map_err(|_| meguri_storage::RowError {
                            table: "tags",
                            column: "tag",
                            key: String::new(),
                            reason: "invalid UTF-8 tag".into(),
                        })?
                    }
                    _ => {
                        return Err(StorageError::Io {
                            detail: "changed tag has no stored name".into(),
                        });
                    }
                };
                changes.tags.push(TagChange { name, kind });
            }
            Table::Notetypes => {
                if let RowKey::Id(id) = entry.key() {
                    let slot = notetypes.entry(*id).or_default();
                    slot.0 = Some(kind);
                    slot.1 |= kind != ChangeKind::Updated;
                }
            }
            Table::Fields | Table::Templates => {
                let RowKey::Pair(notetype, _) = entry.key() else {
                    continue;
                };
                let renamed = kind != ChangeKind::Updated
                    || match (journal.before(entry)?, journal.after(entry)?) {
                        (Some(before), Some(after)) => before.get(PART_NAME) != after.get(PART_NAME),
                        _ => true,
                    };
                notetypes.entry(*notetype).or_default().1 |= renamed;
            }
            Table::Graves => {}
        }
    }
    changes.notetypes = notetypes
        .into_iter()
        .map(|(id, (kind, schema_changed))| NotetypeChange {
            id: NotetypeId(id),
            kind: kind.unwrap_or(ChangeKind::Updated),
            schema_changed,
        })
        .collect();

    changes.cards.sort_by_key(|c| c.id);
    changes.notes.sort_by_key(|n| n.id);
    changes.decks.sort_by_key(|d| d.id);
    changes.deck_configs.sort_by_key(|d| d.id);
    changes.config_keys.sort_by(|a, b| a.key.cmp(&b.key));
    changes.tags.sort_by(|a, b| a.name.cmp(&b.name));
    changes.revlog_added.sort();
    changes.revlog_removed.sort();
    changes.media_added = sorted_unique(media.added);
    changes.media_removed = sorted_unique(media.removed);

    Ok(Derived {
        changes,
        collection,
        rows,
        local_modification,
    })
}

/// The change kind of a journal entry; `None` for a row the op both created
/// and deleted.
fn kind_of(entry: &Entry) -> Option<ChangeKind> {
    match (entry.existed_before(), entry.exists_after()) {
        (false, true) => Some(ChangeKind::Added),
        (true, true) => Some(ChangeKind::Updated),
        (true, false) => Some(ChangeKind::Removed),
        (false, false) => None,
    }
}

fn sorted_unique(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

/// S1: every changed column is `mod`, `usn` or `data`, and in `data` only the
/// FSRS values `s`, `d`, `dr`, `decay` and `lrt` differ.
fn memory_state_only(before: &[Cell], after: &[Cell]) -> bool {
    let changed = ColumnSet::changed(before, after);
    if (0..after.len()).any(|i| changed.contains(i) && ![CARD_MOD, CARD_USN, CARD_DATA].contains(&i)) {
        return false;
    }
    if !changed.contains(CARD_DATA) {
        return true;
    }
    without_memory_state(before.get(CARD_DATA)) == without_memory_state(after.get(CARD_DATA))
}

/// The `data` text rebuilt without its FSRS values.
fn without_memory_state(cell: Option<&Cell>) -> String {
    let text = match cell {
        Some(Cell::Text(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    };
    let mut data = CardData::parse(&text);
    data.memory_state = None;
    data.desired_retention = None;
    data.decay = None;
    data.last_review_secs = None;
    data.to_text()
}
