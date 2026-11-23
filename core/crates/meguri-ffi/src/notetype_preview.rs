//! The Cards tab's live preview over FFI (U283, `notetypes/workspace` W9;
//! `render/preview` V2 to V11): one card of a structure draft, unsaved
//! texts and styling included, for a stored note of the type or a new
//! empty note, and the type's note ids for the note menu. Both read one
//! snapshot on the interactive lane and write nothing (V10).

use meguri_engine::notetypes::draft_render;
use meguri_engine::notetypes::structure::StructureDraft;
use meguri_engine::rendering::Platform;
use meguri_model::ids::{NoteId, NotetypeId};

use crate::notetype_structure::NotetypeStructureDraft;
use crate::render::{RenderPlatform, RenderedCard};
use crate::{CoreError, callback, runtime};

/// One preview render and the note it rendered.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeDraftRender {
    pub card: RenderedCard,
    /// The stored note rendered; none for a new empty note (the given note
    /// is gone or belongs to another note type, or none was given).
    pub note_id: Option<i64>,
    /// A cloze draft's card numbers (V6), ascending; empty otherwise.
    pub cloze_numbers: Vec<u32>,
}

/// V2 for a draft: `target` is a card type's position in the draft
/// (standard) or a cloze number minus one (cloze). Refuses as the draft's
/// planning does (`Stale`, `InvalidArgument`, `NotetypeRefused`), a target
/// past 65534 and a standard draft with no template there (`NotFound`).
#[uniffi::export]
pub fn render_notetype_draft(
    draft: NotetypeStructureDraft,
    note_id: Option<i64>,
    target: u32,
    fill_empty: bool,
    platform: RenderPlatform,
) -> Result<NotetypeDraftRender, CoreError> {
    callback::enter("render_notetype_draft");
    let draft = StructureDraft::try_from(draft)?;
    let collection = runtime::host()?.collection()?;
    let platform = match platform {
        RenderPlatform::Mac => Platform::Mac,
        RenderPlatform::Ios => Platform::Ios,
    };
    let render = draft_render::render_draft(&collection, &draft, note_id.map(NoteId), target, fill_empty, platform)?;
    Ok(NotetypeDraftRender {
        card: render.card.into(),
        note_id: render.note.map(|n| n.0),
        cloze_numbers: render.cloze_numbers,
    })
}

/// W9's note menu: the type's note ids, ascending; `NotFound` for an
/// unknown note type.
#[uniffi::export]
pub fn notetype_note_ids(notetype_id: i64) -> Result<Vec<i64>, CoreError> {
    callback::enter("notetype_note_ids");
    let collection = runtime::host()?.collection()?;
    Ok(draft_render::note_ids(&collection, NotetypeId(notetype_id))?
        .into_iter()
        .map(|n| n.0)
        .collect())
}
