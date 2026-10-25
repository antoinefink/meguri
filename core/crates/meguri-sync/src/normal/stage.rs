//! Strictly decoded incoming data is retained on disk, including rejected
//! targets. M is updated independently of the live collection (H6-H8).
use super::{Result, storage_error};
use crate::{
    ErrorKind, JobError, Method,
    wire::{changes::IncomingChanges, chunk::IncomingChunk, graves::Graves},
};
use meguri_engine::tags;
use meguri_model::{
    clock::Clock,
    config::ConfigEntry,
    grave::{Grave, GraveKind},
    note::Note,
    time::EpochSecs,
    usn::Usn,
};
use meguri_storage::{
    ReadRows, Record, RowKey, Table, WriteScope,
    normal_stage::{Namespace, NormalStage},
};

#[derive(Clone, Copy, Default)]
pub(super) struct Meta {
    pub config: bool,
    pub created: Option<EpochSecs>,
}
fn refuse(kind: ErrorKind, method: Method) -> JobError {
    JobError::new(kind, Some(method), "incoming collection contract")
}
pub(super) fn record_graves(stage: &NormalStage, graves: &Graves, usn: Usn, clock: &dyn Clock) -> Result<()> {
    stage.write(clock, |_| -> Result<()> {
        for id in &graves.notes {
            stage
                .store_record(
                    Namespace::Incoming,
                    &Grave {
                        oid: id.0,
                        kind: GraveKind::Note,
                        usn,
                    },
                )
                .map_err(storage_error)?;
        }
        for id in &graves.cards {
            stage
                .store_record(
                    Namespace::Incoming,
                    &Grave {
                        oid: id.0,
                        kind: GraveKind::Card,
                        usn,
                    },
                )
                .map_err(storage_error)?;
        }
        for id in &graves.decks {
            stage
                .store_record(
                    Namespace::Incoming,
                    &Grave {
                        oid: id.0,
                        kind: GraveKind::Deck,
                        usn,
                    },
                )
                .map_err(storage_error)?;
        }
        Ok(())
    })
}
pub(super) fn graves(stage: &NormalStage, graves: &Graves, usn: Usn, clock: &dyn Clock) -> Result<()> {
    stage.write(clock, |scope| -> Result<()> {
        let mut apply = |id, kind, table| -> Result<()> {
            let grave = Grave { oid: id, kind, usn };
            stage.store_record(Namespace::Incoming, &grave).map_err(storage_error)?;
            scope.restore(table, &RowKey::Id(id), None).map_err(storage_error)?;
            scope.add_grave(id, kind, usn).map_err(storage_error)?;
            Ok(())
        };
        for id in &graves.notes {
            apply(id.0, GraveKind::Note, Table::Notes)?;
        }
        for id in &graves.cards {
            apply(id.0, GraveKind::Card, Table::Cards)?;
        }
        for id in &graves.decks {
            apply(id.0, GraveKind::Deck, Table::Decks)?;
        }
        Ok(())
    })
}
pub(super) fn objects(stage: &NormalStage, changes: IncomingChanges, usn: Usn, clock: &dyn Clock) -> Result<Meta> {
    let meta = Meta {
        config: changes.config.is_some(),
        created: changes.created,
    };
    stage.write(clock, |scope| -> Result<()> {
        for mut nt in changes.notetypes {
            stage.store_notetype(Namespace::Incoming, &nt).map_err(storage_error)?;
            if let Some(local) = scope.notetype(nt.id).map_err(storage_error)? {
                if local.mtime > nt.mtime {
                    continue;
                }
                if local.fields.len() != nt.fields.len() || local.templates.len() != nt.templates.len() {
                    return Err(refuse(
                        ErrorKind::NotetypeSchemaChanged { notetype_id: nt.id },
                        Method::ApplyChanges,
                    ));
                }
            }
            unique(
                scope,
                Table::Notetypes,
                nt.id.0,
                &mut nt.name,
                &mut nt.mtime,
                &mut nt.usn,
                usn,
            )
            .map_err(storage_error)?;
            scope.put_notetype(&nt).map_err(storage_error)?;
        }
        for mut deck in changes.decks {
            stage.store_record(Namespace::Incoming, &deck).map_err(storage_error)?;
            if scope
                .deck(deck.id)
                .map_err(storage_error)?
                .is_some_and(|local| local.mtime > deck.mtime)
            {
                continue;
            }
            unique(
                scope,
                Table::Decks,
                deck.id.0,
                &mut deck.name,
                &mut deck.mtime,
                &mut deck.usn,
                usn,
            )
            .map_err(storage_error)?;
            scope.put(&deck).map_err(storage_error)?;
        }
        for preset in changes.deck_configs {
            stage
                .store_record(Namespace::Incoming, &preset)
                .map_err(storage_error)?;
            if scope
                .deck_config(preset.id)
                .map_err(storage_error)?
                .is_some_and(|local| local.mtime > preset.mtime)
            {
                continue;
            }
            scope.put(&preset).map_err(storage_error)?;
        }
        for tag in changes.tags {
            // Store the received target even if registration is a no-op.
            let row = meguri_model::tag::Tag {
                name: tags::normalize(&tag),
                usn,
                collapsed: true,
                config: None,
            };
            stage.store_record(Namespace::Incoming, &row).map_err(storage_error)?;
            register_tag(scope, &tag, usn).map_err(storage_error)?;
        }
        if let Some(created) = changes.created {
            let mut col = scope.col().map_err(storage_error)?;
            col.created = created;
            scope.put_col(&col).map_err(storage_error)?;
        }
        if let Some(config) = changes.config {
            // Config is whole-table replacement; timestamps change even for an
            // equal value. No ConfigValue same-value shortcut applies here.
            for key in scope.config_keys().map_err(storage_error)? {
                scope.remove_config(&key).map_err(storage_error)?;
            }
            for (key, text) in config {
                let value = meguri_storage::ConfigValue::parse(text.as_bytes())
                    .ok_or_else(|| JobError::bad_response(Method::ApplyChanges, "conf value"))?;
                let row = ConfigEntry {
                    key,
                    value: value.compact().into_bytes(),
                    usn,
                    mtime: EpochSecs(scope.now().0.div_euclid(1000)),
                };
                stage.store_record(Namespace::Incoming, &row).map_err(storage_error)?;
                scope
                    .restore(Table::Config, &RowKey::text(&row.key), Some(&row.to_cells()))
                    .map_err(storage_error)?;
            }
        }
        Ok(())
    })?;
    Ok(meta)
}
pub(super) fn unique(
    scope: &WriteScope<'_>,
    table: Table,
    id: i64,
    name: &mut String,
    mtime: &mut EpochSecs,
    row_usn: &mut Usn,
    usn: Usn,
) -> std::result::Result<(), meguri_storage::StorageError> {
    let mut renamed = false;
    while scope.sync_name_taken(table, id, name)? {
        name.push('+');
        renamed = true;
    }
    if renamed {
        *mtime = EpochSecs(scope.now().0.div_euclid(1000));
        *row_usn = usn;
    }
    Ok(())
}
pub(super) fn register_tag(
    scope: &mut WriteScope<'_>,
    name: &str,
    usn: Usn,
) -> std::result::Result<(), meguri_storage::StorageError> {
    let normalized = tags::normalize(name);
    if normalized.is_empty() {
        return Ok(());
    }
    tags::Speller::new().spell_with(scope, &normalized, |name| meguri_model::tag::Tag {
        name: name.into(),
        usn,
        collapsed: true,
        config: None,
    })?;
    Ok(())
}
pub(super) fn accepts(local_usn: Usn, local_mtime: EpochSecs, incoming_mtime: EpochSecs) -> bool {
    local_usn != Usn::PENDING || local_mtime < incoming_mtime
}
pub(super) fn validate_note(note: &mut Note, nt: Option<meguri_model::notetype::Notetype>) -> Result<()> {
    let nt = nt.ok_or_else(|| {
        refuse(
            ErrorKind::MissingNotetype {
                notetype_id: note.notetype_id,
                note_id: note.id,
            },
            Method::Chunk,
        )
    })?;
    let fields = note
        .field_values()
        .map(|f| meguri_engine::notes::sync_field(f))
        .collect::<Vec<_>>();
    if fields.is_empty() || fields.len() != nt.fields.len() {
        return Err(refuse(
            ErrorKind::NoteFieldsMismatch {
                notetype_id: note.notetype_id,
                note_id: note.id,
            },
            Method::Chunk,
        ));
    }
    meguri_engine::notes::prepare_sync_note(note, &nt, &fields);
    Ok(())
}
pub(super) fn chunk(stage: &NormalStage, chunk: IncomingChunk, clock: &dyn Clock) -> Result<u64> {
    let count = (chunk.cards.len() + chunk.notes.len() + chunk.revlog.len()) as u64;
    if count > 250 {
        return Err(JobError::bad_response(Method::Chunk, "chunk exceeds 250 objects"));
    }
    stage.write(clock, |scope| -> Result<()> {
        let mut reviews = Vec::new();
        let mut cards = Vec::new();
        let mut notes = Vec::new();
        for row in chunk.revlog {
            stage.store_record(Namespace::Incoming, &row).map_err(storage_error)?;
            if scope.revlog_entry(row.id).map_err(storage_error)?.is_none() {
                reviews.push(row.to_cells());
            }
        }
        scope.merge_sync_rows(Table::Revlog, &reviews).map_err(storage_error)?;
        for row in chunk.cards {
            stage.store_record(Namespace::Incoming, &row).map_err(storage_error)?;
            if scope
                .card(row.id)
                .map_err(storage_error)?
                .is_none_or(|local| accepts(local.usn, local.mtime, row.mtime))
            {
                cards.push(row.to_cells());
            }
        }
        scope.merge_sync_rows(Table::Cards, &cards).map_err(storage_error)?;
        for row in chunk.notes {
            let mut note = Note {
                id: row.id,
                guid: row.guid,
                notetype_id: row.notetype_id,
                mtime: row.mtime,
                usn: row.usn,
                tags: if row.tags.is_empty() {
                    String::new()
                } else {
                    format!(" {} ", row.tags.join(" "))
                },
                fields: row.fields,
                ..Default::default()
            };
            let accepted = scope
                .note(note.id)
                .map_err(storage_error)?
                .is_none_or(|local| accepts(local.usn, local.mtime, note.mtime));
            if accepted {
                let nt = scope.notetype(note.notetype_id).map_err(storage_error)?;
                validate_note(&mut note, nt)?;
            }
            stage.store_record(Namespace::Incoming, &note).map_err(storage_error)?;
            if accepted {
                notes.push(note.to_cells());
            }
        }
        scope.merge_sync_rows(Table::Notes, &notes).map_err(storage_error)?;
        Ok(())
    })?;
    Ok(count)
}
