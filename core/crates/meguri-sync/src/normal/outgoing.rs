//! Outgoing values come only from the disk-backed V, after staged effects.
use super::{Result, storage_error};
use crate::wire::{
    changes::{OutgoingChanges, encode_apply_changes},
    chunk::{OutgoingChunk, encode_apply_chunk},
    graves::{Graves, encode_apply_graves},
};
use meguri_model::usn::Usn;
use meguri_model::{
    card::Card,
    grave::{Grave, GraveKind},
    ids::NotetypeId,
    note::Note,
    revlog::RevlogEntry,
};
use meguri_storage::{
    ReadRows, Record, RowKey, Table,
    normal_stage::{Namespace, NormalStage},
};

pub(super) struct GravesCursor {
    kind: usize,
    after: Option<RowKey>,
}
impl GravesCursor {
    pub fn new() -> Self {
        Self { kind: 0, after: None }
    }
    pub fn next(&mut self, stage: &NormalStage) -> Result<Option<(Vec<u8>, u64)>> {
        let mut result = Graves::default();
        while result.len() < 250 && self.kind < 3 {
            // The storage query filters the type, so paging never retains the
            // collection's grave list in memory.
            let page = stage
                .pending_graves(self.kind as i64, self.after.as_ref(), 250 - result.len())
                .map_err(storage_error)?;
            if page.is_empty() {
                self.kind += 1;
                self.after = None;
                continue;
            }
            for image in page {
                self.after = Some(NormalStage::row_key(Table::Graves, &image));
                let grave = Grave::from_cells(&image).map_err(|e| storage_error(e.into()))?;
                stage
                    .store(Namespace::Sent, Table::Graves, &image)
                    .map_err(storage_error)?;
                match grave.kind {
                    GraveKind::Card => result.cards.push(meguri_model::ids::CardId(grave.oid)),
                    GraveKind::Note => result.notes.push(meguri_model::ids::NoteId(grave.oid)),
                    GraveKind::Deck => result.decks.push(meguri_model::ids::DeckId(grave.oid)),
                }
            }
        }
        Ok((!result.is_empty()).then(|| (encode_apply_graves(&result), result.len() as u64)))
    }
}
pub(super) fn changes(stage: &NormalStage, newer: bool, usn: Usn) -> Result<(Vec<u8>, u64)> {
    let mut models = Vec::new();
    let mut decks = Vec::new();
    let mut presets = Vec::new();
    let mut tags = Vec::new();
    let mut config = Vec::new();
    for table in [
        Table::Notetypes,
        Table::Decks,
        Table::DeckConfig,
        Table::Tags,
        Table::Config,
    ] {
        if table == Table::Config && !newer {
            continue;
        }
        let mut after = None;
        loop {
            let page = stage
                .page(Namespace::View, table, after.as_ref(), table != Table::Config)
                .map_err(storage_error)?;
            if page.is_empty() {
                break;
            }
            for image in page {
                let key = NormalStage::row_key(table, &image);
                after = Some(key.clone());
                stage.store(Namespace::Sent, table, &image).map_err(storage_error)?;
                match table {
                    Table::Notetypes => {
                        let RowKey::Id(id) = key else { unreachable!() };
                        models.push(
                            stage
                                .notetype(NotetypeId(id))
                                .map_err(storage_error)?
                                .ok_or_else(super::missing_outgoing)?,
                        );
                    }
                    Table::Decks => {
                        decks.push(meguri_model::deck::Deck::from_cells(&image).map_err(|e| storage_error(e.into()))?)
                    }
                    Table::DeckConfig => presets.push(
                        meguri_model::deck_config::DeckConfig::from_cells(&image)
                            .map_err(|e| storage_error(e.into()))?,
                    ),
                    Table::Tags => tags.push(
                        meguri_model::tag::Tag::from_cells(&image)
                            .map_err(|e| storage_error(e.into()))?
                            .name,
                    ),
                    Table::Config => {
                        let entry = meguri_model::config::ConfigEntry::from_cells(&image)
                            .map_err(|e| storage_error(e.into()))?;
                        config.push(meguri_storage::StoredConfig {
                            is_blob: matches!(image[3], meguri_storage::Cell::Blob(_)),
                            entry,
                        });
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    let count = (models.len() + decks.len() + presets.len() + tags.len()) as u64;
    let created = if newer {
        Some(stage.col().map_err(storage_error)?.created)
    } else {
        None
    };
    let body = encode_apply_changes(
        &OutgoingChanges {
            notetypes: &models,
            decks: &decks,
            deck_configs: &presets,
            tags: &tags,
            config: newer.then_some(config.as_slice()),
            created,
        },
        usn,
    )?;
    Ok((body, count))
}
/// Each cursor buffers at most 250 rows. Turns restart with revlog for each
/// request, including the final empty request for exact multiples of 250.
struct Cursor {
    table: Table,
    after: Option<RowKey>,
    rows: std::collections::VecDeque<meguri_storage::RowImage>,
    ended: bool,
}
impl Cursor {
    fn new(table: Table) -> Self {
        Self {
            table,
            after: None,
            rows: Default::default(),
            ended: false,
        }
    }
    fn take(&mut self, stage: &NormalStage) -> Result<Option<meguri_storage::RowImage>> {
        if self.rows.is_empty() && !self.ended {
            let page = stage
                .page(Namespace::View, self.table, self.after.as_ref(), true)
                .map_err(storage_error)?;
            self.ended = page.len() < 250;
            self.after = page.last().map(|r| NormalStage::row_key(self.table, r));
            self.rows = page.into();
        }
        let image = self.rows.pop_front();
        if let Some(image) = &image {
            stage.store(Namespace::Sent, self.table, image).map_err(storage_error)?;
        }
        Ok(image)
    }
}
pub(super) struct Chunks {
    cursors: [Cursor; 3],
}
impl Chunks {
    pub fn new() -> Self {
        Self {
            cursors: [
                Cursor::new(Table::Revlog),
                Cursor::new(Table::Notes),
                Cursor::new(Table::Cards),
            ],
        }
    }
    pub fn next(&mut self, stage: &NormalStage, usn: Usn) -> Result<(Vec<u8>, u64, bool)> {
        let mut reviews = Vec::new();
        let mut notes = Vec::new();
        let mut cards = Vec::new();
        let mut count = 0;
        while count < 250 {
            let mut took = false;
            for cursor in &mut self.cursors {
                if count == 250 {
                    break;
                }
                if let Some(image) = cursor.take(stage)? {
                    took = true;
                    count += 1;
                    match cursor.table {
                        Table::Revlog => {
                            reviews.push(RevlogEntry::from_cells(&image).map_err(|e| storage_error(e.into()))?)
                        }
                        Table::Notes => notes.push(Note::from_cells(&image).map_err(|e| storage_error(e.into()))?),
                        Table::Cards => cards.push(Card::from_cells(&image).map_err(|e| storage_error(e.into()))?),
                        _ => unreachable!(),
                    }
                }
            }
            if !took {
                break;
            }
        }
        let done = count < 250;
        Ok((
            encode_apply_chunk(
                &OutgoingChunk {
                    done,
                    revlog: &reviews,
                    cards: &cards,
                    notes: &notes,
                },
                usn,
            ),
            count,
            done,
        ))
    }
}
