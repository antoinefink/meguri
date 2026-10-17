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
use meguri_model::revlog::RevlogEntry;
use meguri_storage::journal::{After, ColumnSet, Entry, Journal};
use meguri_storage::{Cell, Record, RowImage, RowKey, Table};

use observers::{CardDelta, ConfigDelta};

/// The `cards` columns a memory-state-only change may touch (S1): `mod`,
/// `usn` and `data`.
const CARD_MOD: usize = 4;
const CARD_USN: usize = 5;
const CARD_DATA: usize = 17;
const CARD_NID: usize = 1;
/// `col.mod`.
const COL_MOD: usize = 2;
/// The `name` column of `fields` and `templates`.
const PART_NAME: usize = 2;
/// `config.val`.
const CONFIG_VAL: usize = 3;

/// What the writer needs from a committed op's journal.
pub(crate) struct Derived {
    pub changes: ChangeSet,
    pub cards: Vec<CardDelta>,
    pub revlog_added: Vec<RevlogEntry>,
    pub revlog_removed: Vec<RevlogEntry>,
    pub config: Vec<ConfigDelta>,
    /// The `col` row after the op, when the op changed it.
    pub collection: Option<Col>,
    /// Rows the journal holds.
    pub rows: usize,
}

/// Media names an op reported through its context.
#[derive(Debug, Default)]
pub(crate) struct MediaNotes {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Derives the change set (S1) and, when `payload` is set, the observers'
/// row payloads (S2). A before-image that cannot be read back from the
/// spill file counts as absent and is logged: the commit already stands.
pub(crate) fn derive(journal: Option<&Journal>, source: OpSource, media: MediaNotes, payload: bool) -> Derived {
    let mut changes = ChangeSet::empty(source);
    let mut cards = Vec::new();
    let mut revlog_added = Vec::new();
    let mut revlog_removed = Vec::new();
    let mut config = Vec::new();
    let mut collection = None;
    let mut notetypes: BTreeMap<i64, (Option<ChangeKind>, bool)> = BTreeMap::new();
    let mut rows = 0;

    for entry in journal.into_iter().flat_map(|journal| journal.entries()) {
        rows += 1;
        let Some(kind) = kind_of(entry) else { continue };
        match entry.table() {
            Table::Col => {
                if payload && let After::Row(image) = entry.after() {
                    match Col::from_cells(image) {
                        Ok(col) => collection = Some(col),
                        Err(error) => tracing::warn!(%error, "the changed col row is unreadable"),
                    }
                }
                if kind != ChangeKind::Updated {
                    changes.collection_row = true;
                } else if let (Some(before), After::Row(after)) = (before_image(journal, entry), entry.after()) {
                    let changed = ColumnSet::changed(&before, after);
                    changes.collection_row = (0..after.len()).any(|i| i != COL_MOD && changed.contains(i));
                }
            }
            Table::Notes => {
                if let RowKey::Id(id) = entry.key() {
                    changes.notes.push(NoteChange { id: NoteId(*id), kind });
                }
            }
            Table::Cards => {
                let RowKey::Id(id) = entry.key() else { continue };
                let before = before_image(journal, entry);
                let after = match entry.after() {
                    After::Row(image) => Some(image.clone()),
                    _ => None,
                };
                let note = after
                    .as_ref()
                    .or(before.as_ref())
                    .and_then(|image| image.get(CARD_NID))
                    .and_then(Cell::as_i64)
                    .unwrap_or_default();
                let memory_state_only = match (&before, &after) {
                    (Some(before), Some(after)) => memory_state_only(before, after),
                    _ => false,
                };
                changes.cards.push(CardChange {
                    id: CardId(*id),
                    note_id: NoteId(note),
                    kind,
                    memory_state_only,
                });
                if payload {
                    cards.push(CardDelta {
                        id: CardId(*id),
                        before,
                        after,
                    });
                }
            }
            Table::Revlog => {
                let RowKey::Id(id) = entry.key() else { continue };
                match kind {
                    ChangeKind::Added => {
                        changes.revlog_added.push(RevlogId(*id));
                        if payload && let After::Row(image) = entry.after() {
                            push_revlog(&mut revlog_added, image);
                        }
                    }
                    ChangeKind::Removed => {
                        changes.revlog_removed.push(RevlogId(*id));
                        if payload && let Some(image) = before_image(journal, entry) {
                            push_revlog(&mut revlog_removed, &image);
                        }
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
                let key = String::from_utf8_lossy(key).into_owned();
                changes.config_keys.push(ConfigKeyChange { key: key.clone(), kind });
                if payload {
                    let value = |image: &RowImage| match image.get(CONFIG_VAL) {
                        Some(Cell::Blob(bytes) | Cell::Text(bytes)) => Some(bytes.clone()),
                        _ => None,
                    };
                    config.push(ConfigDelta {
                        key,
                        before: before_image(journal, entry).as_ref().and_then(value),
                        after: match entry.after() {
                            After::Row(image) => value(image),
                            _ => None,
                        },
                    });
                }
            }
            Table::Tags => {
                let image = match entry.after() {
                    After::Row(image) => Some(image.clone()),
                    _ => before_image(journal, entry),
                };
                let name = match image.as_ref().and_then(|i| i.first()) {
                    Some(Cell::Text(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
                    _ => match entry.key() {
                        RowKey::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                        other => other.to_string(),
                    },
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
                    || match (before_image(journal, entry), entry.after()) {
                        (Some(before), After::Row(after)) => before.get(PART_NAME) != after.get(PART_NAME),
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
    cards.sort_by_key(|c: &CardDelta| c.id);
    config.sort_by(|a: &ConfigDelta, b: &ConfigDelta| a.key.cmp(&b.key));

    Derived {
        changes,
        cards,
        revlog_added,
        revlog_removed,
        config,
        collection,
        rows,
    }
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

fn before_image(journal: Option<&Journal>, entry: &Entry) -> Option<RowImage> {
    let journal = journal?;
    match journal.before(entry) {
        Ok(image) => image,
        Err(error) => {
            tracing::warn!(table = %entry.table(), key = %entry.key(), %error, "a journal before-image is unreadable");
            None
        }
    }
}

fn push_revlog(out: &mut Vec<RevlogEntry>, image: &[Cell]) {
    match RevlogEntry::from_cells(image) {
        Ok(entry) => out.push(entry),
        Err(error) => tracing::warn!(%error, "a changed review-log row is unreadable"),
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
