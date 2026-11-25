use crate::undo::names::ADD_NOTE;
use crate::{Collection, EngineError, OpResult};
use meguri_model::changes::OpSource;
use meguri_model::{
    ids::{CardId, DeckId, NoteId, NotetypeId},
    note::Note,
    rng::RandomSource,
};
use meguri_storage::{ConfigValue, ReadRows, WriteScope};

#[derive(Clone, Debug)]
pub struct AddNote {
    pub notetype_id: NotetypeId,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
    pub deck_id: DeckId,
    pub guid: Option<String>,
    pub update_add_defaults: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: NoteId,
    pub cards: Vec<CardId>,
}

/// Identity of a field as loaded, in stored ordinal order. An absent stored
/// id is significant; it is never replaced with the ordinal or a generated id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddFieldIdentity {
    pub ord: u32,
    pub id: Option<i64>,
    pub name: String,
}

/// Add-screen authority captured with the flushed draft and field check.
/// `notetype_base` is the existing AggregateDigest exposed by read_notetype,
/// not an id comparison. It also invalidates consent after template edits.
#[derive(Clone, Debug)]
pub struct AddScreenNote {
    pub expected_generation: crate::Generation,
    pub notetype_id: NotetypeId,
    pub kind: i32,
    pub field_layout: Vec<AddFieldIdentity>,
    pub notetype_base: crate::notetypes::AggregateDigest,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
    pub deck_id: DeckId,
}

/// A5/D5: never reinterpret a draft using a successor collection, changed
/// layout, or fallback deck. Refusals roll back the op, preserving undo/redo.
/// Field-check refusals and missing-cloze consent remain the screen's job.
pub fn add_for_screen(collection: &Collection, input: AddScreenNote) -> Result<OpResult<Added>, EngineError> {
    // Compare the captured expectation with THIS handle, then submit on this
    // same handle. The executor checks its generation again after queue wait.
    if input.expected_generation != collection.generation() {
        return Err(EngineError::StaleGeneration {
            planned: input.expected_generation,
            current: collection.generation(),
        });
    }
    collection.run(super::op_spec(ADD_NOTE, OpSource::UserUndoable, 1), move |context| {
        let (scope, random) = context.scope_and_random();
        add_screen_in(scope, random, input, true)
    })
}

/// Shared live destination/layout checks for native and local API adds.
pub(super) fn add_screen_in(
    scope: &mut WriteScope<'_>, random: &dyn RandomSource,
    input: AddScreenNote, update_add_defaults: bool,
) -> Result<Added, EngineError> {
    // These are the first reads in the real writer transaction, before
    // normalization, tag registration, row generation or defaults writes.
    let nt = scope.notetype(input.notetype_id)?.ok_or_else(|| EngineError::NotFound {
        table: meguri_storage::Table::Notetypes,
        key: input.notetype_id.0.to_string(),
    })?;
    let layout: Vec<_> = nt.fields.iter().map(|field| AddFieldIdentity {
        ord: field.ord,
        id: field.config.id,
        name: field.name.clone(),
    }).collect();
    if nt.config.kind != input.kind || layout != input.field_layout
        || crate::notetypes::AggregateDigest::of(&nt) != input.notetype_base
    {
        return Err(EngineError::Stale {
            table: meguri_storage::Table::Notetypes,
            key: input.notetype_id.0.to_string(),
        });
    }
    let deck = scope.deck(input.deck_id)?.ok_or_else(|| EngineError::NotFound {
        table: meguri_storage::Table::Decks,
        key: input.deck_id.0.to_string(),
    })?;
    if !matches!(deck.kind.kind, meguri_model::proto::deck::DeckKind::Normal(_)) {
        return Err(EngineError::DeckRefused(crate::decks::Refusal::FilteredDeck));
    }
    add_in(scope, random, &AddNote {
        notetype_id: input.notetype_id,
        fields: input.fields,
        tags: input.tags,
        deck_id: input.deck_id,
        guid: None,
        update_add_defaults,
    })
}

pub fn add(collection: &Collection, input: AddNote, source: OpSource) -> Result<OpResult<Added>, EngineError> {
    collection.run(super::op_spec(ADD_NOTE, source, 1), move |context| {
        let (scope, random) = context.scope_and_random();
        add_in(scope, random, &input)
    })
}

pub fn bulk_add(
    collection: &Collection,
    inputs: Vec<AddNote>,
    source: OpSource,
) -> Result<OpResult<Vec<Added>>, EngineError> {
    // A note may create hundreds of cards, so note count alone cannot
    // prove a bulk write will stay under the executor's short-write bound.
    let spec = crate::OpSpec::new(crate::executor::catalog::BULK_OP, source)
        .undo_step(ADD_NOTE, crate::undo::entry::StepKind::Other)
        .declare_long_write(std::time::Duration::from_secs(1));
    collection.run(spec, move |context| {
        let (scope, random) = context.scope_and_random();
        bulk_add_in(scope, random, &inputs)
    })
}

pub fn bulk_add_in(
    scope: &mut WriteScope<'_>,
    random: &dyn RandomSource,
    inputs: &[AddNote],
) -> Result<Vec<Added>, EngineError> {
    inputs.iter().map(|input| add_in(scope, random, input)).collect()
}

pub fn add_in(scope: &mut WriteScope<'_>, random: &dyn RandomSource, input: &AddNote) -> Result<Added, EngineError> {
    let nt = scope
        .notetype(input.notetype_id)?
        .ok_or_else(|| super::invalid("unknown note type"))?;
    if input.fields.is_empty() || input.fields.len() != nt.fields.len() {
        return Err(super::invalid("wrong field count"));
    }
    let fields = super::fields::normalize(scope, &input.fields)?;
    let mut note = Note {
        notetype_id: nt.id,
        mtime: scope.now().whole_secs(),
        usn: meguri_model::usn::Usn::PENDING,
        guid: input.guid.clone().unwrap_or_else(|| super::guid::new(random)),
        fields: fields.join("\u{1f}"),
        tags: crate::tags::note_tags_for_save(scope, &input.tags)?,
        sort_field: meguri_text::checksum::sort_text_of(&fields, nt.config.sort_field_idx as usize).into_owned(),
        checksum: meguri_text::checksum::note_checksum(&fields[0]),
        ..Note::default()
    };
    note.id = scope.add_note(&note)?;
    let cards = crate::cardgen::generate_in(scope, &nt, &note, Some(input.deck_id))?;
    if input.update_add_defaults {
        scope.set_config(
            &format!("_nt_{}_lastDeck", nt.id.0),
            &ConfigValue::from(input.deck_id.0),
        )?;
        scope.set_config(
            &format!("_deck_{}_lastNotetype", input.deck_id.0),
            &ConfigValue::from(nt.id.0),
        )?;
        scope.set_config("curModel", &ConfigValue::from(nt.id.0))?;
    }
    Ok(Added { id: note.id, cards })
}
