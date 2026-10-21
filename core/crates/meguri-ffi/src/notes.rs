//! U27 typed note inputs; storage-only fields are never reconstructed here.
use crate::{CoreError, callback, mutations::MutationSummary, runtime};
use meguri_engine::{OpSource, notes};
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_storage::{ReadRows, ReaderRole};
#[derive(Clone, Debug, uniffi::Record)]
pub struct AddNoteRequest {
    pub notetype_id: i64,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
    pub deck_id: i64,
    pub guid: Option<String>,
    pub update_add_defaults: bool,
}
impl From<AddNoteRequest> for notes::AddNote {
    fn from(r: AddNoteRequest) -> Self {
        Self {
            notetype_id: NotetypeId(r.notetype_id),
            fields: r.fields,
            tags: r.tags,
            deck_id: DeckId(r.deck_id),
            guid: r.guid,
            update_add_defaults: r.update_add_defaults,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct UpdateNoteRequest {
    pub id: i64,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NoteMutation {
    pub summary: MutationSummary,
    pub id: i64,
    pub cards: Vec<i64>,
    pub written: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NoteRecord {
    pub id: i64,
    pub notetype_id: i64,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
    pub guid: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FieldState {
    Normal,
    Empty,
    Duplicate,
    MissingCloze,
    NotetypeNotCloze,
    FieldNotCloze,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FieldCheck {
    pub state: FieldState,
    pub duplicates: Vec<i64>,
}
#[uniffi::export]
pub fn add_note(request: AddNoteRequest) -> Result<NoteMutation, CoreError> {
    callback::enter("add_note");
    let r = notes::add(&runtime::host()?.collection()?, request.into(), OpSource::UserUndoable)?;
    Ok(NoteMutation {
        summary: MutationSummary::of(&r),
        id: r.value.id.0,
        cards: r.value.cards.iter().map(|i| i.0).collect(),
        written: true,
    })
}
#[uniffi::export]
pub fn update_note(request: UpdateNoteRequest) -> Result<NoteMutation, CoreError> {
    callback::enter("update_note");
    let id = request.id;
    let r = notes::update(
        &runtime::host()?.collection()?,
        notes::UpdateNote {
            id: NoteId(id),
            fields: request.fields,
            tags: request.tags,
        },
        OpSource::UserUndoable,
    )?;
    Ok(NoteMutation {
        summary: MutationSummary::of(&r),
        id,
        cards: r.value.cards.iter().map(|i| i.0).collect(),
        written: r.value.written,
    })
}
#[uniffi::export]
pub fn read_note(id: i64) -> Result<NoteRecord, CoreError> {
    callback::enter("read_note");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let n = r
        .note(NoteId(id))?
        .ok_or(meguri_engine::EngineError::NoteNotFound { id: NoteId(id) })?;
    Ok(NoteRecord {
        id,
        notetype_id: n.notetype_id.0,
        fields: n.field_values().map(str::to_owned).collect(),
        tags: meguri_engine::tags::split_text(&n.tags).map(str::to_owned).collect(),
        guid: n.guid,
    })
}
#[uniffi::export]
pub fn check_note_fields(
    notetype_id: i64,
    fields: Vec<String>,
    own_note: Option<i64>,
) -> Result<FieldCheck, CoreError> {
    callback::enter("check_note_fields");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let snapshot = r.snapshot()?;
    let nt = snapshot
        .notetype(NotetypeId(notetype_id))?
        .ok_or_else(|| CoreError::InvalidArgument {
            message: "unknown note type".into(),
        })?;
    let check = notes::check(&*snapshot, &nt, &fields, own_note.map(NoteId))?;
    let state = match check.state {
        notes::FieldState::Normal => FieldState::Normal,
        notes::FieldState::Empty => FieldState::Empty,
        notes::FieldState::Duplicate => FieldState::Duplicate,
        notes::FieldState::MissingCloze => FieldState::MissingCloze,
        notes::FieldState::NotetypeNotCloze => FieldState::NotetypeNotCloze,
        notes::FieldState::FieldNotCloze => FieldState::FieldNotCloze,
    };
    Ok(FieldCheck {
        state,
        duplicates: check.duplicates.iter().map(|i| i.0).collect(),
    })
}

use std::sync::{Arc, Mutex};
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NoteDeletion {
    pub summary: Option<MutationSummary>,
    pub count: u64,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct NoteDeletionJobStart {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<NoteDeletionJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct NoteDeletionJob {
    result: Arc<Mutex<Option<NoteDeletion>>>,
}
#[uniffi::export]
impl NoteDeletionJob {
    pub fn result(&self) -> Result<Option<NoteDeletion>, CoreError> {
        callback::enter("NoteDeletionJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[uniffi::export]
pub fn start_delete_notes(ids: Vec<i64>) -> Result<NoteDeletionJobStart, CoreError> {
    callback::enter("start_delete_notes");
    let c = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let r = meguri_engine::notes::delete_controlled(
            &c,
            ids.into_iter().map(NoteId).collect(),
            meguri_engine::OpSource::UserUndoable,
            j.cancel.clone(),
            j.progress.clone(),
        )?;
        *out.lock().unwrap() = Some(match r {
            Some(r) => NoteDeletion {
                summary: Some(MutationSummary::of(&r)),
                count: r.value as u64,
            },
            None => NoteDeletion {
                summary: None,
                count: 0,
            },
        });
        Ok(())
    })?;
    Ok(NoteDeletionJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NoteDeletionJob { result }),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AddedNote {
    pub id: i64,
    pub cards: Vec<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BulkNoteAddition {
    pub summary: MutationSummary,
    pub notes: Vec<AddedNote>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct NoteAdditionJobStart {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<NoteAdditionJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct NoteAdditionJob {
    result: Arc<Mutex<Option<BulkNoteAddition>>>,
}
#[uniffi::export]
impl NoteAdditionJob {
    pub fn result(&self) -> Result<Option<BulkNoteAddition>, CoreError> {
        callback::enter("NoteAdditionJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[uniffi::export]
pub fn start_add_notes(requests: Vec<AddNoteRequest>) -> Result<NoteAdditionJobStart, CoreError> {
    callback::enter("start_add_notes");
    let c = runtime::host()?.collection()?;
    let inputs = requests.into_iter().map(Into::into).collect::<Vec<notes::AddNote>>();
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        // U27's bulk spec, with the job's existing cancel/progress ports.
        // All note/card behavior remains in the engine's bulk_add_in.
        let spec = meguri_engine::OpSpec::new(meguri_engine::executor::catalog::BULK_OP, OpSource::UserUndoable)
            .undo_step(meguri_engine::undo::names::ADD_NOTE, meguri_engine::StepKind::Other)
            .declare_long_write(std::time::Duration::from_secs(1))
            .cancel(j.cancel.clone())
            .progress(j.progress.clone());
        let r = c.run(spec, move |ctx| {
            let (scope, random) = ctx.scope_and_random();
            notes::bulk_add_in(scope, random, &inputs)
        })?;
        *out.lock().unwrap() = Some(BulkNoteAddition {
            summary: MutationSummary::of(&r),
            notes: r
                .value
                .into_iter()
                .map(|n| AddedNote {
                    id: n.id.0,
                    cards: n.cards.into_iter().map(|i| i.0).collect(),
                })
                .collect(),
        });
        Ok(())
    })?;
    Ok(NoteAdditionJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NoteAdditionJob { result }),
    })
}
