//! The Note Types screens' reads over FFI (U267): the list in
//! `notetypes/editor-actions` A2's order and, as a job, its note counts;
//! as jobs on one snapshot each (progress, cancel, no write): the save
//! summary of a draft (`notetypes/draft-save` D2), the notes with content
//! of each field (`notetypes/workspace` W3.1), the notes a draft's fronts
//! give no card (W8) and the delete question's counts (W12); and the near
//! name of an unknown field (W7.1), which reads no collection.

use std::sync::{Arc, Mutex};

use meguri_engine::notetypes::counts;
use meguri_engine::notetypes::structure::{self, StructureDraft};
use meguri_model::ids::NotetypeId;

use crate::jobs::JobHandle;
use crate::notetype_structure::{NotetypeDraftSummary, NotetypeStructureDraft};
use crate::notetypes::NotetypeName;
use crate::{CoreError, callback, runtime};

/// A2's ordered list: every note type by name under `unicase`, ties by id,
/// read at once (the counts come from [`start_notetype_note_counts`]).
#[uniffi::export]
pub fn notetype_list() -> Result<Vec<NotetypeName>, CoreError> {
    callback::enter("notetype_list");
    let c = runtime::host()?.collection()?;
    let r = c.reader(meguri_storage::ReaderRole::Query)?;
    Ok(counts::ordered_names(&*r)?
        .into_iter()
        .map(|(id, name)| NotetypeName { id: id.0, name })
        .collect())
}

/// One entry of A2's note counts. A note type added after the list was
/// read has no entry until the next refresh.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeNoteCount {
    pub id: i64,
    pub notes: u64,
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeNoteCountsJob {
    result: Arc<Mutex<Option<Vec<NotetypeNoteCount>>>>,
}

#[uniffi::export]
impl NotetypeNoteCountsJob {
    pub fn result(&self) -> Result<Option<Vec<NotetypeNoteCount>>, CoreError> {
        callback::enter("NotetypeNoteCountsJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeNoteCountsJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeNoteCountsJob>,
}

/// A2's note counts on one snapshot, on the job pool: every note type's
/// notes (0 for none), by id.
#[uniffi::export]
pub fn start_notetype_note_counts() -> Result<NotetypeNoteCountsJobStart, CoreError> {
    callback::enter("start_notetype_note_counts");
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        job.cancel.check()?;
        let read = counts::list_note_counts(&collection)?;
        *out.lock().unwrap() = Some(
            read.into_iter()
                .map(|(id, notes)| NotetypeNoteCount { id: id.0, notes })
                .collect(),
        );
        Ok(())
    })?;
    Ok(NotetypeNoteCountsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeNoteCountsJob { result }),
    })
}

/// W3.1's result.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeFieldContentCounts {
    /// The type's notes.
    pub notes: u64,
    /// Per stored field ordinal: the notes whose value there is not empty
    /// (`text/field-emptiness` E1: `<br>` alone is empty).
    pub fields: Vec<u64>,
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeFieldContentCountsJob {
    result: Arc<Mutex<Option<NotetypeFieldContentCounts>>>,
}

#[uniffi::export]
impl NotetypeFieldContentCountsJob {
    pub fn result(&self) -> Result<Option<NotetypeFieldContentCounts>, CoreError> {
        callback::enter("NotetypeFieldContentCountsJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeFieldContentCountsJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeFieldContentCountsJob>,
}

/// W3.1's `field_content_counts(notetype id)` on the job pool; `NotFound`
/// for an unknown note type.
#[uniffi::export]
pub fn start_notetype_field_content_counts(
    notetype_id: i64,
) -> Result<NotetypeFieldContentCountsJobStart, CoreError> {
    callback::enter("start_notetype_field_content_counts");
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let read = counts::field_content_counts(&collection, NotetypeId(notetype_id), &job.cancel, &job.progress)?;
        *out.lock().unwrap() = Some(NotetypeFieldContentCounts {
            notes: read.notes,
            fields: read.fields,
        });
        Ok(())
    })?;
    Ok(NotetypeFieldContentCountsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeFieldContentCountsJob { result }),
    })
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeDraftSummaryJob {
    result: Arc<Mutex<Option<NotetypeDraftSummary>>>,
}

#[uniffi::export]
impl NotetypeDraftSummaryJob {
    pub fn result(&self) -> Result<Option<NotetypeDraftSummary>, CoreError> {
        callback::enter("NotetypeDraftSummaryJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeDraftSummaryJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeDraftSummaryJob>,
}

/// D2's `draft_summary(draft)` on the job pool: the save's own counting
/// pass over the draft, so the counts are those the save takes back as
/// its confirmed losses (each `FieldRemoved` row's `notes_with_content` and
/// each `TemplateRemoved` row's `cards_removed`, by `ord`). The draft's
/// `confirmed` is not read. Refuses as the save's planning does before it
/// compares losses (`Stale`, `InvalidArgument`, `NotetypeRefused`,
/// `NotetypeStructureRefused`).
#[uniffi::export]
pub fn start_notetype_draft_summary(draft: NotetypeStructureDraft) -> Result<NotetypeDraftSummaryJobStart, CoreError> {
    callback::enter("start_notetype_draft_summary");
    let draft = StructureDraft::try_from(draft)?;
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let read = structure::draft_summary(&collection, &draft, &job.cancel, &job.progress)?;
        *out.lock().unwrap() = Some(read.into());
        Ok(())
    })?;
    Ok(NotetypeDraftSummaryJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeDraftSummaryJob { result }),
    })
}

/// W8 for one card type of the draft, in the draft's order.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeBlankFront {
    /// Every note whose front would render nothing, ascending; Show Notes
    /// opens Browse on them.
    pub note_ids: Vec<i64>,
    /// The position in the draft's fields of the one field that decides it
    /// (W8's reason line), or none.
    pub field: Option<u32>,
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeNotesWithoutCardJob {
    result: Arc<Mutex<Option<Vec<NotetypeBlankFront>>>>,
}

#[uniffi::export]
impl NotetypeNotesWithoutCardJob {
    pub fn result(&self) -> Result<Option<Vec<NotetypeBlankFront>>, CoreError> {
        callback::enter("NotetypeNotesWithoutCardJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeNotesWithoutCardJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeNotesWithoutCardJob>,
}

/// W8's `notes_without_card` for every card type of the draft, on the job
/// pool; no entries for a cloze note type.
#[uniffi::export]
pub fn start_notetype_notes_without_card(
    draft: NotetypeStructureDraft,
) -> Result<NotetypeNotesWithoutCardJobStart, CoreError> {
    callback::enter("start_notetype_notes_without_card");
    let draft = StructureDraft::try_from(draft)?;
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let read = counts::notes_without_card(&collection, &draft, &job.cancel, &job.progress)?;
        *out.lock().unwrap() = Some(
            read.into_iter()
                .map(|front| NotetypeBlankFront {
                    note_ids: front.notes.into_iter().map(|id| id.0).collect(),
                    field: front.field,
                })
                .collect(),
        );
        Ok(())
    })?;
    Ok(NotetypeNotesWithoutCardJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeNotesWithoutCardJob { result }),
    })
}

/// W12's counts, which the removal takes back as its confirmed counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeRemovalCounts {
    pub notes: u64,
    pub cards: u64,
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeRemovalCountsJob {
    result: Arc<Mutex<Option<NotetypeRemovalCounts>>>,
}

#[uniffi::export]
impl NotetypeRemovalCountsJob {
    pub fn result(&self) -> Result<Option<NotetypeRemovalCounts>, CoreError> {
        callback::enter("NotetypeRemovalCountsJob::result");
        Ok(*self.result.lock().unwrap())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeRemovalCountsJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeRemovalCountsJob>,
}

/// W12's question counts on the job pool, by the removal's own census
/// (`notetypes/structure-changes` S12); `NotFound` for an unknown id.
#[uniffi::export]
pub fn start_notetype_removal_counts(notetype_id: i64) -> Result<NotetypeRemovalCountsJobStart, CoreError> {
    callback::enter("start_notetype_removal_counts");
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let read = counts::removal_counts(&collection, NotetypeId(notetype_id), &job.cancel, &job.progress)?;
        *out.lock().unwrap() = Some(NotetypeRemovalCounts {
            notes: read.notes,
            cards: read.cards,
        });
        Ok(())
    })?;
    Ok(NotetypeRemovalCountsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeRemovalCountsJob { result }),
    })
}

/// W7.1: the position in `fields` (the draft's field names in order) of
/// the name to suggest for the unknown field name `unknown`, or none. Pure.
#[uniffi::export]
pub fn notetype_near_field_name(unknown: String, fields: Vec<String>) -> Result<Option<u32>, CoreError> {
    callback::enter("notetype_near_field_name");
    let names: Vec<&str> = fields.iter().map(String::as_str).collect();
    Ok(counts::near_field_name(&unknown, &names).and_then(|at| u32::try_from(at).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_names_cross_the_bridge_as_positions() {
        let fields = vec!["Front".to_owned(), "Back".to_owned()];
        assert_eq!(notetype_near_field_name("back".into(), fields.clone()).unwrap(), Some(1));
        assert_eq!(notetype_near_field_name("Frnt".into(), fields.clone()).unwrap(), Some(0));
        assert_eq!(notetype_near_field_name("Elsewhere".into(), fields).unwrap(), None);
    }
}
