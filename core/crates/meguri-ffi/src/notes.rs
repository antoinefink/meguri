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
pub struct AddFieldIdentity {
    pub ord: u32,
    pub id: Option<i64>,
    pub name: String,
}

/// Dedicated native Add entry. Legacy add_note deliberately keeps A1's
/// fallback semantics and caller-controlled update_add_defaults flag.
#[derive(Clone, Debug, uniffi::Record)]
pub struct AddScreenNoteRequest {
    pub expected_generation: u64,
    pub notetype_id: i64,
    pub kind: i32,
    pub field_layout: Vec<AddFieldIdentity>,
    pub notetype_base: Vec<u8>,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
    pub deck_id: i64,
}

#[uniffi::export]
pub fn add_note_for_screen(request: AddScreenNoteRequest) -> Result<NoteMutation, CoreError> {
    callback::enter("add_note_for_screen");
    // Resolve once at actual lane execution. Never check global generation
    // and reacquire a potentially different collection for the mutation.
    let collection = runtime::host()?.collection()?;
    let expected_generation = meguri_engine::Generation(request.expected_generation);
    if collection.generation() != expected_generation {
        return Err(meguri_engine::EngineError::StaleGeneration {
            planned: expected_generation,
            current: collection.generation(),
        }.into());
    }
    let base: [u8; 32] = request.notetype_base.try_into().map_err(|_| CoreError::InvalidArgument {
        message: "expected note type aggregate digest must contain 32 bytes".into(),
    })?;
    let result = notes::add_for_screen(&collection, notes::AddScreenNote {
        expected_generation,
        notetype_id: NotetypeId(request.notetype_id),
        kind: request.kind,
        field_layout: request.field_layout.into_iter().map(|field| notes::AddFieldIdentity {
            ord: field.ord,
            id: field.id,
            name: field.name,
        }).collect(),
        notetype_base: meguri_engine::notetypes::AggregateDigest(base),
        fields: request.fields,
        tags: request.tags,
        deck_id: DeckId(request.deck_id),
    })?;
    Ok(NoteMutation::with_cards(
        MutationSummary::of(&result), result.value.id, result.value.cards, true,
    ))
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct UpdateNoteRequest {
    pub id: i64,
    pub fields: Vec<String>,
    pub tags: Vec<String>,
}
impl From<UpdateNoteRequest> for notes::UpdateNote {
    fn from(request: UpdateNoteRequest) -> Self {
        Self {
            id: NoteId(request.id),
            fields: request.fields,
            tags: request.tags,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NoteMutation {
    pub summary: MutationSummary,
    pub id: i64,
    pub cards: Vec<i64>,
    pub written: bool,
}
impl NoteMutation {
    fn with_cards(summary: MutationSummary, id: NoteId, cards: Vec<meguri_model::ids::CardId>, written: bool) -> Self {
        Self {
            summary,
            id: id.0,
            cards: cards.into_iter().map(|card| card.0).collect(),
            written,
        }
    }
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
    Ok(NoteMutation::with_cards(
        MutationSummary::of(&r),
        r.value.id,
        r.value.cards,
        true,
    ))
}
#[uniffi::export]
pub fn update_note(request: UpdateNoteRequest) -> Result<NoteMutation, CoreError> {
    callback::enter("update_note");
    let id = request.id;
    let r = notes::update(&runtime::host()?.collection()?, request.into(), OpSource::UserUndoable)?;
    Ok(NoteMutation::with_cards(
        MutationSummary::of(&r),
        NoteId(id),
        r.value.cards,
        r.value.written,
    ))
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct NoteFieldValue {
    pub ord: u32,
    pub html: String,
}
/// `editor/field-html` S2: the note type and layout as the editor loaded
/// them, the fields it changed, and the whole tag list only when it changed.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NotePatchRequest {
    /// The generation the editor loaded the note in; the patch is refused
    /// with `StaleGeneration` on any other, so a draft never lands in a
    /// replacement collection. None for hosts that do not track it.
    pub expected_generation: Option<u64>,
    pub id: i64,
    pub notetype_id: i64,
    pub field_layout: Vec<AddFieldIdentity>,
    pub fields: Vec<NoteFieldValue>,
    pub tags: Option<Vec<String>>,
}
#[uniffi::export]
/// `session_id`: the open review session when the review's Edit Note saves
/// (`review/edit-current` E2); the step then belongs to that session.
pub fn patch_note(request: NotePatchRequest, session_id: Option<u64>) -> Result<NoteMutation, CoreError> {
    callback::enter("patch_note");
    let id = NoteId(request.id);
    let input = notes::NotePatch {
        id,
        notetype_id: NotetypeId(request.notetype_id),
        field_layout: request.field_layout.into_iter().map(|field| notes::AddFieldIdentity {
            ord: field.ord,
            id: field.id,
            name: field.name,
        }).collect(),
        fields: request.fields.into_iter().map(|field| (field.ord, field.html)).collect(),
        tags: request.tags,
    };
    let host = runtime::host()?;
    // Resolve the collection once and check it against the editor's
    // generation; the op then runs on that collection only.
    let collection = host.collection()?;
    let expected = request.expected_generation.map(meguri_engine::Generation);
    if let Some(expected) = expected {
        if collection.generation() != expected {
            return Err(meguri_engine::EngineError::StaleGeneration { planned: expected, current: collection.generation() }.into());
        }
    }
    let r = match session_id {
        // The review resolves its own bound collection and checks the generation there.
        Some(session) => host.review().patch_note(input, meguri_engine::SessionId(session), expected)?,
        None => notes::patch(&collection, input, OpSource::UserUndoable)?,
    };
    Ok(NoteMutation::with_cards(MutationSummary::of(&r), id, r.value.cards, r.value.written))
}
/// The deck a card is in now (`review/edit-current` E1's subtitle).
#[uniffi::export]
pub fn card_deck(card_id: i64) -> Result<i64, CoreError> {
    callback::enter("card_deck");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let card = r.card(meguri_model::ids::CardId(card_id))?.ok_or_else(|| meguri_engine::EngineError::NotFound {
        table: meguri_storage::Table::Cards,
        key: card_id.to_string(),
    })?;
    Ok(card.deck_id.0)
}
/// One card of a note, as Browse's inspector shows it (`browser/workspace`
/// V7, V12): its ordinal, the deck it is in now and, for a card in a
/// filtered deck, its home deck (0 otherwise).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NoteCard {
    pub id: i64,
    pub ord: u32,
    pub deck_id: i64,
    pub original_deck_id: i64,
}
/// The cards of a note by ordinal, then id: in Notes mode the inspector
/// shows the first (V7), and "Card <i> of <k>" steps through them (V12).
/// A pure read of one query snapshot; it writes nothing. A note with no
/// card gives an empty list; a missing note gives `NoteNotFound`.
#[uniffi::export]
pub fn note_cards(note_id: i64) -> Result<Vec<NoteCard>, CoreError> {
    callback::enter("note_cards");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let snapshot = r.snapshot()?;
    if snapshot.note(NoteId(note_id))?.is_none() {
        return Err(meguri_engine::EngineError::NoteNotFound { id: NoteId(note_id) }.into());
    }
    let mut cards: Vec<NoteCard> = snapshot
        .cards_of_note(NoteId(note_id))?
        .into_iter()
        .map(|card| NoteCard {
            id: card.id.0,
            ord: u32::from(card.ord),
            deck_id: card.deck_id.0,
            original_deck_id: card.original_deck_id.0,
        })
        .collect();
    cards.sort_by_key(|card| (card.ord, card.id));
    Ok(cards)
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
