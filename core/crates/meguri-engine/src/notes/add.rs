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
