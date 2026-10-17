use crate::undo::names::UPDATE_NOTE;
use crate::{Collection, EngineError, OpResult};
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, NoteId};
use meguri_storage::{ReadRows, WriteScope};

#[derive(Clone, Debug)]
pub struct UpdateNote {
    pub id: NoteId,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Updated {
    pub written: bool,
    pub cards: Vec<CardId>,
}

pub fn update(collection: &Collection, input: UpdateNote, source: OpSource) -> Result<OpResult<Updated>, EngineError> {
    collection.run(super::op_spec(UPDATE_NOTE, source, 1), move |context| {
        update_in(context.scope(), &input)
    })
}

pub fn update_in(scope: &mut WriteScope<'_>, input: &UpdateNote) -> Result<Updated, EngineError> {
    let mut note = scope
        .note(input.id)?
        .ok_or(EngineError::NoteNotFound { id: input.id })?;
    let nt = scope
        .notetype(note.notetype_id)?
        .ok_or_else(|| super::invalid("unknown note type"))?;
    if input.fields.is_empty() || input.fields.len() != nt.fields.len() {
        return Err(super::invalid("wrong field count"));
    }
    let fields = super::fields::normalize(scope, &input.fields)?;
    // Canonicalizing can register tags. Roll those writes back on a no-op.
    let savepoint = scope.savepoint()?;
    let tags = crate::tags::note_tags_for_save(scope, &input.tags)?;
    let joined = fields.join("\u{1f}");
    if joined == note.fields && tags == note.tags {
        scope.rollback_to_savepoint(savepoint)?;
        return Ok(Updated {
            written: false,
            cards: Vec::new(),
        });
    }
    scope.release_savepoint(savepoint)?;
    note.fields = joined;
    note.tags = tags;
    note.sort_field = meguri_text::checksum::sort_text_of(&fields, nt.config.sort_field_idx as usize).into_owned();
    note.checksum = meguri_text::checksum::note_checksum(&fields[0]);
    scope.put(&note)?;
    let cards = crate::cardgen::generate_in(scope, &nt, &note, None)?;
    Ok(Updated { written: true, cards })
}
