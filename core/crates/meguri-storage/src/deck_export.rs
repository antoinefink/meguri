//! Read-only deck export selection (package/apkg-export A2–A8). Only ids and
//! structure records are retained; note/card/history payloads are streamed again
//! from the same pinned view when building the new file.
use std::collections::{BTreeMap, BTreeSet};

use meguri_model::{
    deck::Deck,
    deck_config::DeckConfig,
    ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId},
    notetype::Notetype,
    proto::deck::DeckKind,
};
use rusqlite::OptionalExtension;

use crate::{ReadRows, ReadSnapshot, StorageError};

#[derive(Debug)]
pub enum DeckExportReadError {
    DeckNotFound(i64),
    InvalidIds,
    MissingReference { table: &'static str, id: i64 },
    Cancelled,
    Storage(StorageError),
}
impl From<StorageError> for DeckExportReadError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}
impl From<rusqlite::Error> for DeckExportReadError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Storage(value.into())
    }
}

pub struct DeckExportSelection {
    pub name: String,
    pub notes: Vec<i64>,
    pub cards: Vec<i64>,
    pub revlog: Vec<i64>,
    pub decks: BTreeMap<i64, Deck>,
    pub notetypes: BTreeMap<i64, Notetype>,
    pub presets: BTreeMap<i64, DeckConfig>,
    pub return_home: bool,
    pub created: meguri_model::time::EpochSecs,
    pub creation_offset: Option<i32>,
    pub next_position: u32,
}

fn check(cancel: &(dyn Fn() -> bool + Sync)) -> Result<(), DeckExportReadError> {
    if cancel() {
        Err(DeckExportReadError::Cancelled)
    } else {
        Ok(())
    }
}
fn missing(table: &'static str, id: i64) -> DeckExportReadError {
    DeckExportReadError::MissingReference { table, id }
}

impl ReadSnapshot<'_> {
    pub fn select_deck_export(
        &self,
        deck_id: i64,
        scheduling: bool,
        presets: bool,
        now: i64,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<DeckExportSelection, DeckExportReadError> {
        check(cancel)?;
        let source = self.conn().path().ok_or_else(|| StorageError::Io {
            detail: "export source has no path".into(),
        })?;
        crate::connections::validate_identity(
            self.conn(),
            std::path::Path::new(source),
            self.reader().native_identity(),
        )?;
        let name: String = self
            .conn()
            .query_row("SELECT name FROM decks WHERE id=?1", [deck_id], |r| r.get(0))
            .optional()?
            .ok_or(DeckExportReadError::DeckNotFound(deck_id))?;
        // The name index compares complete folded strings, so a parent whose
        // spelling expands under case folding still bounds the same subtree.
        // U+001F is the hierarchy separator; U+0020 is its exclusive successor.
        // Start at decks/cards rather than probing every note in the collection.
        let selected = "SELECT id FROM notes WHERE id IN (SELECT c.nid FROM cards c WHERE c.did IN (SELECT id FROM decks WHERE name=?1 COLLATE unicase OR (name >= (?1 || char(31)) COLLATE unicase AND name < (?1 || char(32)) COLLATE unicase)))";
        let ids = |sql: &str| -> Result<Vec<i64>, DeckExportReadError> {
            let mut statement = self.conn().prepare(sql)?;
            let mut rows = statement.query([&name])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                check(cancel)?;
                out.push(row.get(0)?);
            }
            Ok(out)
        };
        let notes = ids(&format!("{selected} ORDER BY id"))?;
        let cards = ids(&format!("SELECT id FROM cards WHERE nid IN ({selected}) ORDER BY id"))?;
        let revlog = if scheduling {
            ids(&format!(
                "SELECT id FROM revlog WHERE cid IN (SELECT id FROM cards WHERE nid IN ({selected})) ORDER BY id"
            ))?
        } else {
            Vec::new()
        };
        let upper = now.saturating_add(86_400_000);
        if notes.iter().chain(&cards).chain(&revlog).any(|id| *id > upper) {
            return Err(DeckExportReadError::InvalidIds);
        }
        // Read dependency columns first; decoding row payloads follows A8's
        // table order, so an unrelated malformed record cannot block export.
        let mut deck_ids = BTreeSet::new();
        let mut homes = BTreeSet::new();
        let mut origins = BTreeSet::new();
        let mut current_decks = BTreeSet::new();
        for id in &cards {
            check(cancel)?;
            let (did, odid): (i64, i64) =
                self.conn()
                    .query_row("SELECT did,odid FROM cards WHERE id=?1", [id], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?;
            deck_ids.insert(did);
            current_decks.insert(did);
            if odid == 0 {
                homes.insert(did);
            }
            if scheduling && odid != 0 {
                origins.insert(odid);
                deck_ids.insert(odid);
            }
        }
        let mut pending: Vec<_> = deck_ids.iter().copied().collect();
        while let Some(id) = pending.pop() {
            check(cancel)?;
            let deck_name: Option<String> = self
                .conn()
                .query_row("SELECT name FROM decks WHERE id=?1", [id], |r| r.get(0))
                .optional()?;
            let Some(mut ancestor) = deck_name else {
                if current_decks.contains(&id) {
                    return Err(missing("decks", id));
                }
                deck_ids.remove(&id);
                continue;
            };
            while let Some((parent, _)) = ancestor.rsplit_once('\u{1f}') {
                check(cancel)?;
                let parent = parent.to_owned();
                let found: Option<i64> = self
                    .conn()
                    .query_row("SELECT id FROM decks WHERE name=?1 COLLATE unicase", [&parent], |r| {
                        r.get(0)
                    })
                    .optional()?;
                let Some(found) = found else {
                    break;
                };
                if !deck_ids.insert(found) {
                    break;
                }
                pending.push(found);
                ancestor = parent;
            }
        }
        if !scheduling {
            deck_ids.remove(&1);
        }
        let mut decks = BTreeMap::new();
        for id in deck_ids {
            check(cancel)?;
            decks.insert(id, self.deck(DeckId(id))?.ok_or_else(|| missing("decks", id))?);
        }
        let mut mids = BTreeSet::new();
        for id in &notes {
            check(cancel)?;
            mids.insert(
                self.conn()
                    .query_row("SELECT mid FROM notes WHERE id=?1", [id], |r| r.get::<_, i64>(0))?,
            );
        }
        let mut notetypes = BTreeMap::new();
        for id in mids {
            check(cancel)?;
            notetypes.insert(
                id,
                self.notetype(NotetypeId(id))?.ok_or_else(|| missing("notetypes", id))?,
            );
        }
        let mut configs = BTreeMap::new();
        if presets {
            let ids: BTreeSet<_> = decks
                .values()
                .filter_map(|d| match &d.kind.kind {
                    DeckKind::Normal(n) => Some(n.config_id),
                    _ => None,
                })
                .collect();
            for id in ids {
                check(cancel)?;
                configs.insert(
                    id,
                    self.deck_config(DeckConfigId(id))?
                        .ok_or_else(|| missing("deck_config", id))?,
                );
            }
        }
        for id in &notes {
            check(cancel)?;
            self.note(NoteId(*id))?.ok_or_else(|| missing("notes", *id))?;
        }
        for id in &cards {
            check(cancel)?;
            self.card(CardId(*id))?.ok_or_else(|| missing("cards", *id))?;
        }
        for id in &revlog {
            check(cancel)?;
            self.revlog_entry(RevlogId(*id))?
                .ok_or_else(|| missing("revlog", *id))?;
        }
        let return_home = scheduling
            && !(homes.iter().all(|id| {
                decks
                    .get(id)
                    .is_some_and(|d| matches!(d.kind.kind, DeckKind::Normal(_)))
            }) && origins.iter().all(|id| *id == 1 || decks.contains_key(id)));
        let created = meguri_model::time::EpochSecs(self.conn().query_row(
            "SELECT crt FROM col WHERE id=1",
            [],
            |r| r.get(0),
        )?);
        let creation_offset = self
            .config_value("creationOffset")?
            .and_then(|v| v.as_i64())
            .and_then(|v| i32::try_from(v).ok());
        let next_position = self
            .config_value("nextPos")?
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        Ok(DeckExportSelection {
            name,
            notes,
            cards,
            revlog,
            decks,
            notetypes,
            presets: configs,
            return_home,
            created,
            creation_offset,
            next_position,
        })
    }
}
