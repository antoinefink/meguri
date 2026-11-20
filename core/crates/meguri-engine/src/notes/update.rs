use super::AddFieldIdentity;
use crate::undo::names::UPDATE_NOTE;
use crate::{Collection, EngineError, OpResult, SessionId};
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, Table, WriteScope};

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

/// `editor/field-html` S2: the field patch, a second input form of the note
/// update. The note type and its field layout are those the editor loaded;
/// `fields` lists (ordinal, value) pairs; `tags` is the whole list, or none
/// to keep the stored tags.
#[derive(Clone, Debug)]
pub struct NotePatch {
    pub id: NoteId,
    pub notetype_id: NotetypeId,
    pub field_layout: Vec<AddFieldIdentity>,
    pub fields: Vec<(u32, String)>,
    pub tags: Option<Vec<String>>,
}

pub fn update(collection: &Collection, input: UpdateNote, source: OpSource) -> Result<OpResult<Updated>, EngineError> {
    collection.run(super::op_spec(UPDATE_NOTE, source, 1), move |context| {
        update_in(context.scope(), &input)
    })
}

pub fn update_in(scope: &mut WriteScope<'_>, input: &UpdateNote) -> Result<Updated, EngineError> {
    let note = scope
        .note(input.id)?
        .ok_or(EngineError::NoteNotFound { id: input.id })?;
    let nt = scope
        .notetype(note.notetype_id)?
        .ok_or_else(|| super::invalid("unknown note type"))?;
    if input.fields.is_empty() || input.fields.len() != nt.fields.len() {
        return Err(super::invalid("wrong field count"));
    }
    save_in(scope, note, &nt, &input.fields, Some(&input.tags))
}

/// The same op and step name as `update`, so consecutive patches of one
/// note merge as `engine/undo` A3 says.
pub fn patch(collection: &Collection, input: NotePatch, source: OpSource) -> Result<OpResult<Updated>, EngineError> {
    patch_for(collection, input, source, None)
}

/// `review/edit-current` E2: the review's Edit Note names its session, so
/// the step belongs to that session's undo scope (`engine/undo` M3). The
/// review admits the call (`ReviewService::patch_note`); other hosts pass none.
pub fn patch_for(
    collection: &Collection,
    input: NotePatch,
    source: OpSource,
    session: Option<SessionId>,
) -> Result<OpResult<Updated>, EngineError> {
    let mut spec = super::op_spec(UPDATE_NOTE, source, 1);
    if let Some(id) = session {
        spec = spec.session(id);
    }
    collection.run(spec, move |context| patch_in(context.scope(), &input))
}

/// Reads the live rows in the writer's transaction and refuses, writing
/// nothing, before it touches a field: `NoteNotFound` for a deleted note,
/// `Stale` when the note's type or that type's layout is no longer the one
/// the editor loaded, `InvalidArgument` for an ordinal outside the layout or
/// a stored row whose field count differs from its note type's.
pub fn patch_in(scope: &mut WriteScope<'_>, input: &NotePatch) -> Result<Updated, EngineError> {
    let note = scope
        .note(input.id)?
        .ok_or(EngineError::NoteNotFound { id: input.id })?;
    let stale = |table, key: i64| EngineError::Stale {
        table,
        key: key.to_string(),
    };
    if note.notetype_id != input.notetype_id {
        return Err(stale(Table::Notes, input.id.0));
    }
    let nt = scope
        .notetype(note.notetype_id)?
        .ok_or_else(|| stale(Table::Notetypes, input.notetype_id.0))?;
    let layout = nt.fields.iter().map(|field| AddFieldIdentity {
        ord: field.ord,
        id: field.config.id,
        name: field.name.clone(),
    });
    if !layout.eq(input.field_layout.iter().cloned()) {
        return Err(stale(Table::Notetypes, input.notetype_id.0));
    }
    let mut values: Vec<String> = note.field_values().map(str::to_owned).collect();
    if values.len() != nt.fields.len() {
        return Err(super::invalid("the stored note has the wrong field count"));
    }
    let mut listed = vec![false; values.len()];
    for (ord, value) in &input.fields {
        let index = usize::try_from(*ord).map_err(|_| super::invalid("field ordinal out of range"))?;
        match listed.get_mut(index) {
            Some(seen) if !*seen => *seen = true,
            Some(_) => return Err(super::invalid("a field is listed twice")),
            None => return Err(super::invalid("field ordinal out of range")),
        }
        values[index].clone_from(value);
    }
    save_in(scope, note, &nt, &values, input.tags.as_deref())
}

/// N2 for every field, nothing written when the result equals the stored
/// row (U2), then stamps and card generation (U3). Without `tags` the stored
/// tag text is kept as it is.
fn save_in(
    scope: &mut WriteScope<'_>,
    mut note: Note,
    nt: &Notetype,
    values: &[String],
    tags: Option<&[String]>,
) -> Result<Updated, EngineError> {
    let fields = super::fields::normalize(scope, values)?;
    // Canonicalizing can register tags. Roll those writes back on a no-op.
    let savepoint = scope.savepoint()?;
    let tags = match tags {
        Some(tags) => crate::tags::note_tags_for_save(scope, tags)?,
        None => note.tags.clone(),
    };
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
    let cards = crate::cardgen::generate_in(scope, nt, &note, None)?;
    Ok(Updated { written: true, cards })
}
