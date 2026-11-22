//! Find and Replace over FFI (`browser/find-replace` F5 to F13,
//! `browser/workspace` V9 and V17): the sheet's live preview as a job, and
//! the run as a bulk job (`browser/bulk-framework` K2, K7).
//!
//! The target is Selected or All Found, each a Browse selection that the
//! core resolves to its notes (H12; All Found is a selection of every row
//! of the results), or All Notes (`None`). Neither call sends ids across.
//!
//! - A preview (`FindReplacePreviews::start`) reads one snapshot of the
//!   maintenance reader, as the run's compute phase does, at default QoS
//!   (the user waits on the sheet). It writes nothing. A newer preview of
//!   the same `FindReplacePreviews` cancels the older one, in call order, as
//!   filter counts do; dropping the object cancels its newest preview.
//! - The run (`start_find_replace`) carries the count the preview showed as
//!   `confirmed` (F11): it refuses with `CountChanged` when it would write
//!   more notes. It runs admitted by the review service, as U87's actions do
//!   (`bulk_actions::admitted`): held answers are written first, and the
//!   session takes no answer until the run ends.
//!
//! Refusals cross as `CoreError::FindReplaceRefused` (F11's own: the
//! separator, `InvalidRegex` with its message only, F10 as amended, the
//! field count, the missing note type, `CountChanged`) or as the framework's
//! errors (`InvalidArgument` for an empty or repeated list, an empty field
//! name or a selection that cannot be resolved; `Stale`, `StaleGeneration`,
//! `Unavailable`, `Cancelled`).

use std::sync::{Arc, Mutex};

use meguri_engine::bulk::find_replace::{self, DiffSpan, Edit, FindIn, FindReplace, Preview, PreviewEntry, Refusal};
use meguri_engine::bulk::{BulkOutcome, NoteTarget};
use meguri_engine::{EngineError, JobKind};
use meguri_model::ids::NoteId;

use crate::browse_query::Newest;
use crate::browse_search::BrowseSelection;
use crate::bulk_actions::{BulkActionJobStart, admitted};
use crate::jobs::JobHandle;
use crate::{CoreError, callback, runtime};

/// Where the find text is looked for (F1's In).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FindReplaceIn {
    AllFields,
    Tags,
    /// The field of this name in every target note whose note type has it,
    /// compared under `unicase` (F6 step 2).
    Field { name: String },
}

/// The sheet's inputs (F1, V9), without the target and the confirmed count.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FindReplaceRequest {
    pub find: String,
    pub replace: String,
    /// Regular Expression (F4 step 2).
    pub regex: bool,
    /// Match Case off (F4 step 3, V9).
    pub ignore_case: bool,
    /// Whole Words (V9).
    pub whole_words: bool,
    pub within: FindReplaceIn,
}

impl FindReplaceRequest {
    fn engine(self, confirmed: u64) -> FindReplace {
        FindReplace {
            find: self.find,
            replace: self.replace,
            regex: self.regex,
            ignore_case: self.ignore_case,
            whole_words: self.whole_words,
            within: match self.within {
                FindReplaceIn::AllFields => FindIn::AllFields,
                FindReplaceIn::Tags => FindIn::Tags,
                FindReplaceIn::Field { name } => FindIn::Field(name),
            },
            confirmed,
        }
    }
}

/// Why Find and Replace refused (F11); nothing was written and both undo
/// histories are as they were (F9).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FindReplaceRefusal {
    /// F7 step 1: a Tags replacement holding U+0020 or U+3000.
    SeparatorInReplacement,
    /// F4 step 5: the pattern does not compile. The message only, no range
    /// (F10 as amended 2026-11-21).
    InvalidRegex { message: String },
    /// F6 step 7: a matched note whose field count differs from its note
    /// type's.
    FieldCountMismatch { note_id: i64 },
    /// F6 step 7: a target note whose note type does not exist.
    NotetypeMissing { note_id: i64 },
    /// F11: the run would write more notes than were confirmed; `notes` is
    /// the count it read, for the sheet to show again.
    CountChanged { notes: u64 },
}

impl From<Refusal> for FindReplaceRefusal {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::SeparatorInReplacement => FindReplaceRefusal::SeparatorInReplacement,
            Refusal::InvalidRegex { message } => FindReplaceRefusal::InvalidRegex { message },
            Refusal::FieldCountMismatch { note } => FindReplaceRefusal::FieldCountMismatch { note_id: note.0 },
            Refusal::NotetypeMissing { note } => FindReplaceRefusal::NotetypeMissing { note_id: note.0 },
            Refusal::CountChanged { notes } => FindReplaceRefusal::CountChanged { notes },
        }
    }
}

impl From<FindReplaceRefusal> for Refusal {
    fn from(refusal: FindReplaceRefusal) -> Self {
        match refusal {
            FindReplaceRefusal::SeparatorInReplacement => Refusal::SeparatorInReplacement,
            FindReplaceRefusal::InvalidRegex { message } => Refusal::InvalidRegex { message },
            FindReplaceRefusal::FieldCountMismatch { note_id } => Refusal::FieldCountMismatch { note: NoteId(note_id) },
            FindReplaceRefusal::NotetypeMissing { note_id } => Refusal::NotetypeMissing { note: NoteId(note_id) },
            FindReplaceRefusal::CountChanged { notes } => Refusal::CountChanged { notes },
        }
    }
}

/// What a span of a preview entry's text is (V9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FindReplaceEdit {
    Kept,
    /// Only in the old text: crossed out in red.
    Removed,
    /// Only in the new text: in the accent colour.
    Inserted,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FindReplaceSpan {
    pub edit: FindReplaceEdit,
    pub text: String,
}

/// One changed (note, field) pair; Tags count as one field (V9).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FindReplacePreviewEntry {
    pub note_id: i64,
    /// The note's Sort Field before the change (C7.1); empty when its note
    /// type is missing.
    pub sort_field: String,
    /// The changed field's name (shown only with All Fields); `None` for
    /// Tags.
    pub field: Option<String>,
    /// The old and the new text as one line, marked with their character
    /// diff: kept text, and in each run of changes the removed text before
    /// the inserted text.
    pub spans: Vec<FindReplaceSpan>,
}

/// What a Find and Replace would do (V9, F10), read without writing.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FindReplacePreview {
    /// The notes the run would write: the sheet's c, and the run's
    /// `confirmed` count.
    pub changed: u64,
    /// The run's `considered` (F8): the target's notes, deleted ones
    /// included; the notes read for All Notes. The sheet's k.
    pub considered: u64,
    /// The entries of the first 100 changed notes, in result order
    /// (ascending note id for All Notes), each note's in field order.
    pub entries: Vec<FindReplacePreviewEntry>,
}

impl From<DiffSpan> for FindReplaceSpan {
    fn from(span: DiffSpan) -> Self {
        FindReplaceSpan {
            edit: match span.edit {
                Edit::Kept => FindReplaceEdit::Kept,
                Edit::Removed => FindReplaceEdit::Removed,
                Edit::Inserted => FindReplaceEdit::Inserted,
            },
            text: span.text,
        }
    }
}

impl From<PreviewEntry> for FindReplacePreviewEntry {
    fn from(entry: PreviewEntry) -> Self {
        FindReplacePreviewEntry {
            note_id: entry.note.0,
            sort_field: entry.sort_field,
            field: entry.field,
            spans: entry.diff.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<Preview> for FindReplacePreview {
    fn from(preview: Preview) -> Self {
        FindReplacePreview {
            changed: preview.changed,
            considered: preview.considered,
            entries: preview.entries.into_iter().map(Into::into).collect(),
        }
    }
}

/// Selected or All Found: the selection's notes, resolved in the core; All
/// Notes: `None`.
fn note_target(selection: Option<Arc<BrowseSelection>>) -> NoteTarget {
    match selection {
        Some(selection) => NoteTarget::Selection(selection.engine_selection().clone()),
        None => NoteTarget::AllNotes,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

type PreviewResult = Arc<Mutex<Option<Result<FindReplacePreview, CoreError>>>>;

/// Fills a preview's result once. Dropped unfilled, it records why, as
/// `bulk::start`'s completion does: a job cancelled before it started (the
/// registry drops its work unrun, also while admission is paused) is
/// `Cancelled`; work that started and ended without a result panicked.
struct PreviewCompletion {
    out: PreviewResult,
    started: bool,
}

impl PreviewCompletion {
    fn set(&self, result: Result<FindReplacePreview, CoreError>) {
        let mut out = lock(&self.out);
        if out.is_none() {
            *out = Some(result);
        }
    }
}

impl Drop for PreviewCompletion {
    fn drop(&mut self) {
        self.set(Err(if self.started {
            CoreError::Internal {
                message: "the preview job ended without a result".into(),
            }
        } else {
            CoreError::Cancelled
        }));
    }
}

#[derive(Debug, uniffi::Object)]
pub struct FindReplacePreviewJob {
    result: PreviewResult,
}

#[uniffi::export]
impl FindReplacePreviewJob {
    /// `None` until the job ends; then the preview, or the refusal the job
    /// failed with (its `JobFinished` carries the same error); `Cancelled`
    /// for a job cancelled before it started. Set before `JobFinished`.
    pub fn result(&self) -> Result<Option<FindReplacePreview>, CoreError> {
        callback::enter("FindReplacePreviewJob::result");
        lock(&self.result).clone().transpose()
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FindReplacePreviewJobStart {
    pub handle: JobHandle,
    pub result: Arc<FindReplacePreviewJob>,
}

/// One Find and Replace sheet's previews: each `start` supersedes and
/// cancels the previous one, in call order.
#[derive(Debug, uniffi::Object)]
pub struct FindReplacePreviews {
    newest: Newest,
}

impl Drop for FindReplacePreviews {
    fn drop(&mut self) {
        self.newest.cancel_running();
    }
}

#[uniffi::export]
impl FindReplacePreviews {
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, CoreError> {
        callback::enter("FindReplacePreviews::new");
        Ok(Arc::new(FindReplacePreviews {
            newest: Newest::default(),
        }))
    }

    /// V9's preview of `request` on `target` (a selection's notes, or All
    /// Notes for `None`), as a job at default QoS with progress (notes
    /// read) and Cancel. It cancels this sheet's older preview; a call older
    /// than one that already started is refused `Cancelled`. A refusal
    /// (F11's, in its order) fails the job, and `result` returns it; a
    /// collection replaced before the job runs fails it with
    /// `StaleGeneration`.
    pub fn start(
        &self,
        target: Option<Arc<BrowseSelection>>,
        request: FindReplaceRequest,
    ) -> Result<FindReplacePreviewJobStart, CoreError> {
        callback::enter("FindReplacePreviews::start");
        let ticket = self.newest.ticket();
        let host = runtime::host()?;
        let engine = runtime::engine()?;
        // The selection names rows of the collection open now.
        let generation = host.collection()?.generation();
        let target = note_target(target);
        // The preview reads no confirmation.
        let request = request.engine(0);
        let result: PreviewResult = Arc::default();
        let mut completion = PreviewCompletion {
            out: Arc::clone(&result),
            started: false,
        };
        let handle = self.newest.start(ticket, || {
            Ok(engine.start_job(JobKind::BulkOp, move |job| {
                completion.started = true;
                match preview_on(&host, generation, &target, &request, job) {
                    Ok(preview) => {
                        completion.set(Ok(preview.into()));
                        Ok(())
                    }
                    Err(error) => {
                        completion.set(Err(error.clone().into()));
                        Err(error)
                    }
                }
            })?)
        })?;
        Ok(FindReplacePreviewJobStart {
            handle: crate::jobs::wrap(handle),
            result: Arc::new(FindReplacePreviewJob { result }),
        })
    }
}

/// The preview on the collection `host` holds, if it is still of
/// `generation`.
fn preview_on(
    host: &meguri_host::CoreHost,
    generation: meguri_engine::Generation,
    target: &NoteTarget,
    request: &FindReplace,
    job: &meguri_engine::JobContext,
) -> Result<Preview, EngineError> {
    let collection = host.collection()?;
    if collection.generation() != generation {
        return Err(EngineError::StaleGeneration {
            planned: generation,
            current: collection.generation(),
        });
    }
    find_replace::preview(&collection, target, request, &job.cancel, &job.progress)
}

/// F11: runs `request` on `target` (a selection's notes, or All Notes for
/// `None`) as a bulk job admitted by the review service (K2). `confirmed`
/// is the preview's `changed`, the count the user confirmed: a run that
/// would write more notes refuses with `CountChanged`, writing nothing. The
/// result is `BulkActionJob`'s: `affected` (the notes written), `considered`
/// and the commit, with one undo step named "Find and Replace", or "Update
/// Tag" for Tags (F9).
#[uniffi::export]
pub fn start_find_replace(
    target: Option<Arc<BrowseSelection>>,
    request: FindReplaceRequest,
    confirmed: u64,
) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_find_replace");
    let target = note_target(target);
    let request = request.engine(confirmed);
    admitted(move |collection, cancel, progress| {
        find_replace::run(collection, &target, &request, cancel, progress).map(BulkOutcome::of)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_cross_both_ways_unchanged() {
        for refusal in [
            Refusal::SeparatorInReplacement,
            Refusal::InvalidRegex {
                message: "unclosed group".into(),
            },
            Refusal::FieldCountMismatch { note: NoteId(-3) },
            Refusal::NotetypeMissing { note: NoteId(9) },
            Refusal::CountChanged { notes: 12 },
        ] {
            let error = CoreError::from(EngineError::FindReplace(refusal.clone()));
            assert_eq!(
                error,
                CoreError::FindReplaceRefused {
                    refusal: refusal.clone().into()
                }
            );
            assert_eq!(EngineError::from(error), EngineError::FindReplace(refusal));
        }
    }

    /// A preview cancelled before its job started (its work dropped unrun)
    /// reads `Cancelled`, not `None` forever; work that started and left no
    /// result reads an internal error; a set result is kept.
    #[test]
    fn a_preview_dropped_without_a_result_records_why() {
        let read = |started: bool, set: Option<Result<FindReplacePreview, CoreError>>| {
            let out: PreviewResult = Arc::default();
            let completion = PreviewCompletion {
                out: Arc::clone(&out),
                started,
            };
            if let Some(result) = set {
                completion.set(result);
            }
            drop(completion);
            lock(&out).clone()
        };
        assert_eq!(read(false, None), Some(Err(CoreError::Cancelled)));
        assert!(matches!(read(true, None), Some(Err(CoreError::Internal { .. }))));
        let shown = FindReplacePreview {
            changed: 1,
            considered: 2,
            entries: Vec::new(),
        };
        assert_eq!(read(true, Some(Ok(shown.clone()))), Some(Ok(shown)));
    }

    #[test]
    fn a_request_keeps_its_inputs_and_takes_the_confirmed_count() {
        let request = FindReplaceRequest {
            find: "a$".into(),
            replace: "$1 b".into(),
            regex: true,
            ignore_case: false,
            whole_words: true,
            within: FindReplaceIn::Field { name: "Back".into() },
        };
        assert_eq!(
            request.engine(7),
            FindReplace {
                find: "a$".into(),
                replace: "$1 b".into(),
                regex: true,
                ignore_case: false,
                whole_words: true,
                within: FindIn::Field("Back".into()),
                confirmed: 7,
            }
        );
    }
}
