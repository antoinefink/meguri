//! U27 note adapters through the hosted engine (engine/host H4, H5).
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{
    Fields, FieldsCheckCase, FieldsCheckRequest, NoteSpec, ObservationArgs, ObservationRequest, Setup, Step,
};
use meguri_engine::{notes, notetypes};
use meguri_model::{
    ids::{DeckId, NoteId},
    notetype::Notetype,
};
use meguri_storage::{ReadRows, ReaderRole, Table};
use serde_json::{Value, json};

pub(super) fn register(registry: &mut Registry) {
    registry.setup("notes", setup_notes);
    registry.op("add_note", add_note);
    registry.op("edit_note", edit_note);
    registry.op("delete_notes", delete_notes);
    registry.observation("fields_check", fields_check);
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
fn unknown(e: impl Into<String>) -> ExecError {
    ExecError::new("unknown_ref", e)
}
fn invalid(e: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", e)
}
fn collection<'s>(session: &'s Session<'_>) -> Result<&'s super::storage::SessionCollection, ExecError> {
    session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection open"))
}
fn find(reader: &impl ReadRows, name: &str) -> Result<Notetype, ExecError> {
    notetypes::read::by_name(reader, name)
        .map_err(core)?
        .ok_or_else(|| unknown(format!("no note type {name:?}")))
}
fn values(nt: &Notetype, mut values: Vec<String>, fields: &Fields) -> Result<Vec<String>, ExecError> {
    match fields {
        Fields::List(list) => {
            if list.len() > values.len() {
                return Err(invalid("too many fields"));
            }
            for (slot, value) in values.iter_mut().zip(list) {
                slot.clone_from(value);
            }
        }
        Fields::Map(map) => {
            for (name, value) in map {
                let index = nt
                    .fields
                    .iter()
                    .position(|f| f.name == *name)
                    .ok_or_else(|| invalid(format!("unknown field {name:?}")))?;
                values[index].clone_from(value);
            }
        }
    }
    Ok(values)
}

fn add(session: &mut Session<'_>, spec: &NoteSpec) -> Result<notes::Added, ExecError> {
    let collection = session.engine_collection()?;
    let writer = collection.reader(ReaderRole::Query).map_err(core)?;
    let nt = find(&*writer, &spec.notetype)?;
    let deck = match &spec.deck {
        None => DeckId(1),
        Some(name) => writer
            .deck_rows()
            .map_err(core)?
            .iter()
            .find(|d| d.human_name() == *name)
            .map(|d| d.id)
            .or_else(|| session.decks.get(name).copied().map(DeckId))
            .ok_or_else(|| unknown(format!("no deck {name:?}")))?,
    };
    let input = notes::AddNote {
        notetype_id: nt.id,
        fields: values(&nt, vec![String::new(); nt.fields.len()], &spec.fields)?,
        tags: spec.tags.clone(),
        deck_id: deck,
        guid: None,
        update_add_defaults: true,
    };
    drop(writer);
    let added = notes::add(&collection, input, meguri_engine::OpSource::UserUndoable)
        .map_err(core)?
        .value;
    session.notes.push(added.id.0);
    session.cards.extend(added.cards.iter().map(|id| id.0));
    Ok(added)
}

fn cache_decks(session: &mut Session<'_>) -> Result<(), ExecError> {
    let mut found = Vec::new();
    let mut after = None;
    let writer = collection(session)?.reader()?;
    loop {
        let ids = writer.ids_after(Table::Decks, after, 1000).map_err(core)?;
        let Some(last) = ids.last().copied() else { break };
        after = Some(last);
        for id in ids {
            if let Some(deck) = writer.deck(DeckId(id)).map_err(core)? {
                found.push((deck.name.replace('\u{1f}', "::"), id));
            }
        }
    }
    drop(writer);
    session.decks.extend(found);
    Ok(())
}
fn setup_notes(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    cache_decks(session)?;
    for spec in setup.notes.iter().flatten() {
        add(session, spec)?;
    }
    Ok(())
}
fn add_note(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::AddNote(spec) = step else {
        return Err(invalid("expected add_note"));
    };
    // setup.notes may be absent; capture deck ids before any step deletes them.
    let n = session.notes.len() + 1;
    let c = session.cards.len() + 1;
    let added = add(session, spec)?;
    Ok(OpResult::Output(
        json!({ "note": format!("#n{n}"), "cards": (c..c + added.cards.len()).map(|i| format!("#c{i}")).collect::<Vec<_>>() }),
    ))
}
fn stored(session: &Session<'_>, index: usize) -> Result<meguri_model::note::Note, ExecError> {
    let id = *session
        .notes
        .get(index)
        .ok_or_else(|| unknown("note was not created"))?;
    collection(session)?
        .reader()?
        .note(NoteId(id))
        .map_err(core)?
        .ok_or_else(|| unknown("note no longer exists"))
}
fn edit_note(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::EditNote(args) = step else {
        return Err(invalid("expected edit_note"));
    };
    let collection = session.engine_collection()?;
    let note = stored(session, args.note.0.index())?;
    let writer = collection.reader(ReaderRole::Query).map_err(core)?;
    let nt = writer
        .notetype(note.notetype_id)
        .map_err(core)?
        .ok_or_else(|| unknown("note type disappeared"))?;
    let mut fields: Vec<_> = note.field_values().map(str::to_owned).collect();
    if let Some(replacement) = &args.fields {
        fields = values(&nt, fields, replacement)?;
    }
    let input = notes::UpdateNote {
        id: note.id,
        fields,
        tags: args
            .tags
            .clone()
            .unwrap_or_else(|| note.tags.split_whitespace().map(str::to_owned).collect()),
    };
    drop(writer);
    notes::update(&collection, input, meguri_engine::OpSource::UserUndoable).map_err(core)?;
    Ok(OpResult::Done)
}
fn delete_notes(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::DeleteNotes(args) = step else {
        return Err(invalid("expected delete_notes"));
    };
    let ids = args
        .notes
        .iter()
        .map(|r| {
            session
                .notes
                .get(r.0.index())
                .copied()
                .map(NoteId)
                .ok_or_else(|| unknown("note was not created"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let collection = session.engine_collection()?;
    notes::delete(&collection, ids, meguri_engine::OpSource::UserUndoable).map_err(core)?;
    Ok(OpResult::Done)
}
fn fields_check(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("fields_check expects cases"));
    };
    let cases = FieldsCheckRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))?;
    let reader = collection(session)?.reader()?;
    cases
        .cases
        .iter()
        .map(|case| {
            let (nt, fields, own) = match case {
                FieldsCheckCase::NewNote { notetype, fields } => {
                    let nt = find(&*reader, notetype)?;
                    let fields = values(&nt, vec![String::new(); nt.fields.len()], fields)?;
                    (nt, fields, None)
                }
                FieldsCheckCase::StoredNote { note, fields } => {
                    let id = *session
                        .notes
                        .get(note.0.index())
                        .ok_or_else(|| unknown("note was not created"))?;
                    let note = reader
                        .note(NoteId(id))
                        .map_err(core)?
                        .ok_or_else(|| unknown("note no longer exists"))?;
                    let nt = reader
                        .notetype(note.notetype_id)
                        .map_err(core)?
                        .ok_or_else(|| unknown("missing note type"))?;
                    let mut draft = note.field_values().map(str::to_owned).collect();
                    if let Some(fields) = fields {
                        draft = values(&nt, draft, fields)?;
                    }
                    (nt, draft, Some(note.id))
                }
            };
            let checked = notes::check(&*reader, &nt, &fields, own).map_err(core)?;
            let mut value = json!({ "state": checked.state.name() });
            if checked.state == notes::FieldState::Duplicate {
                value["duplicates"] = Value::Array(
                    checked
                        .duplicates
                        .iter()
                        .map(|id| {
                            session
                                .notes
                                .iter()
                                .position(|n| *n == id.0)
                                .map(|i| json!(format!("#n{}", i + 1)))
                                .ok_or_else(|| unknown("duplicate has no logical id"))
                        })
                        .collect::<Result<_, _>>()?,
                );
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// After deck setup, before steps, even when no setup.notes entry exists.
pub(super) fn capture_decks(session: &mut Session<'_>) -> Result<(), ExecError> {
    if session.storage.collection().is_some() {
        cache_decks(session)?;
    }
    Ok(())
}
