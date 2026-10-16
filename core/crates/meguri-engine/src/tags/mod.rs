//! Tags (`specs/tags/tags.md`): names and their normalization, the registry
//! and the spelling of new tags, what a note save stores, the management
//! operations (rename, reparent, delete, clear unused, collapsed state),
//! adding tags to notes and removing them, and the tag tree.
//!
//! Each operation has two forms: an engine op on a [`Collection`], one
//! undoable step (T15), planned on a snapshot where it scans the notes
//! (`engine/op-model` O6) and written in one write phase that reads every
//! note it rewrites from the live row (O7); and an `_in` form that does the
//! same inside a write scope the caller holds (the conformance adapters
//! before U38, composite ops). An op that writes more than
//! [`LONG_WRITE_ROWS`] notes runs as the catalog's bulk op and declares its
//! write (O8), so answers wait in the review session's overlay meanwhile.

pub mod bulk;
pub mod canonical;
pub mod manage;
pub mod matching;
pub mod registry;

use std::sync::Arc;
use std::time::Duration;

use meguri_model::changes::OpSource;
use meguri_model::ids::NoteId;
use meguri_storage::{ReadRows, ReaderRole, StorageError, WriteScope, Written};

pub use bulk::{add_in, add_to_notes, remove_from_notes, remove_in};
pub use canonical::{InvalidName, check_new_name, join_note_tags, normalize, split_text};
pub use manage::{
    clear_unused, clear_unused_in, delete, delete_in, rename, rename_in, reparent, reparent_in, reparent_moves,
    set_collapsed,
};
pub use matching::Matcher;
pub use registry::{Speller, TagNode, note_tags_for_save, read_tree, set_collapsed_in, spell, tree};

/// T13 on the collection: the tag tree as the registry holds it now.
pub fn tag_tree(collection: &Collection) -> Result<Vec<TagNode>, EngineError> {
    let reader = collection.reader(ReaderRole::Query)?;
    Ok(read_tree(&*reader)?)
}

use crate::cancel::CancelToken;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::catalog;
use crate::executor::op::{OpContext, OpResult, OpSpec};
use crate::executor::plan_commit::{Plan, PlanContext, plan_and_commit_with};
use crate::progress::ProgressSink;
use crate::undo::entry::StepKind;

/// What a caller may attach to a tag, flag or mark op: a progress sink and a
/// cancel token for large selections (R1831; U86's bulk actions). Cancel
/// stops the planning; the write phase completes once started (O5).
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub progress: Option<ProgressSink>,
    pub cancel: Option<Arc<CancelToken>>,
}

/// Above this many rows to write, an op runs as the catalog's bulk op with a
/// declared write (`engine/op-model` O8, O9).
pub const LONG_WRITE_ROWS: usize = 2_000;

/// The write time declared per row: a note read, compared, updated and
/// journalled.
const WRITE_PER_ROW: Duration = Duration::from_micros(40);

/// Notes read per page while scanning the notes' tags.
const SCAN_PAGE: usize = 10_000;

/// Notes written between two progress reports.
const PROGRESS_EVERY: usize = 256;

/// The spec of a user op that records the undo step `step` and writes about
/// `rows` rows.
pub(crate) fn op_spec(name: &'static str, step: &'static str, rows: usize, options: &RunOptions) -> OpSpec {
    let spec = if rows > LONG_WRITE_ROWS {
        let expected = WRITE_PER_ROW.saturating_mul(u32::try_from(rows).unwrap_or(u32::MAX));
        OpSpec::new(catalog::BULK_OP, OpSource::UserUndoable).declare_long_write(expected)
    } else {
        OpSpec::named(name, OpSource::UserUndoable)
    };
    let spec = spec.undo_step(step, StepKind::Other);
    let spec = match &options.progress {
        Some(progress) => spec.progress(progress.clone()),
        None => spec,
    };
    match &options.cancel {
        Some(cancel) => spec.cancel(Arc::clone(cancel)),
        None => spec,
    }
}

/// A plan/commit op whose spec follows the size of its plan (`rows`).
pub(crate) fn planned<P, T, PF, RF, CF>(
    collection: &Collection,
    (name, step): (&'static str, &'static str),
    options: &RunOptions,
    plan: PF,
    rows: RF,
    commit: CF,
) -> Result<OpResult<T>, EngineError>
where
    P: Send + 'static,
    T: Send + 'static,
    PF: FnMut(&PlanContext<'_>) -> Result<Plan<P>, EngineError>,
    RF: Fn(&P) -> usize,
    CF: Fn(&mut OpContext<'_, '_>, P) -> Result<T, EngineError> + Send + Sync + 'static,
{
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        op_spec(name, step, 0, options),
        plan,
        |_, value| op_spec(name, step, rows(value), options),
        commit,
    )
}

/// The ids of the notes whose stored tags match (T5), in id order, reading
/// every note's tags a page at a time; `between` runs before each page (the
/// plan's cancel check).
pub(crate) fn matching_notes(
    reader: &(impl ReadRows + ?Sized),
    matcher: &Matcher,
    between: &dyn Fn() -> Result<(), EngineError>,
) -> Result<Vec<NoteId>, EngineError> {
    let mut found = Vec::new();
    if matcher.is_empty() {
        return Ok(found);
    }
    let mut after = None;
    loop {
        between()?;
        let page = reader.note_tags_after(after, SCAN_PAGE)?;
        let Some((last, _)) = page.last() else {
            return Ok(found);
        };
        after = Some(*last);
        found.extend(
            page.iter()
                .filter(|(_, tags)| matcher.matches_stored(tags))
                .map(|(id, _)| *id),
        );
    }
}

/// Every note's stored tags, a page at a time (T11).
pub(crate) fn each_note_tags(
    reader: &(impl ReadRows + ?Sized),
    between: &dyn Fn() -> Result<(), EngineError>,
    mut each: impl FnMut(&str),
) -> Result<(), EngineError> {
    let mut after = None;
    loop {
        between()?;
        let page = reader.note_tags_after(after, SCAN_PAGE)?;
        let Some((last, _)) = page.last() else {
            return Ok(());
        };
        after = Some(*last);
        for (_, tags) in &page {
            each(tags);
        }
    }
}

/// Rewrites the listed notes' tags in place (T6): `rewrite` gets a note's
/// stored text and returns its new text, or `None` to leave it. Notes that
/// no longer exist are skipped, and a note whose new text equals the stored
/// one is not written (`storage/ids-usn-mtime` S4). Returns how many notes
/// were written.
pub(crate) fn rewrite_notes(
    scope: &mut WriteScope<'_>,
    notes: &[NoteId],
    progress: Option<&ProgressSink>,
    mut rewrite: impl FnMut(&mut WriteScope<'_>, &str) -> Result<Option<String>, StorageError>,
) -> Result<usize, StorageError> {
    let total = u64::try_from(notes.len()).unwrap_or(u64::MAX);
    let mut written = 0;
    for (done, id) in notes.iter().enumerate() {
        if let Some(progress) = progress
            && done % PROGRESS_EVERY == 0
        {
            progress.report(u64::try_from(done).unwrap_or(u64::MAX), Some(total));
        }
        let Some(mut note) = scope.note(*id)? else {
            continue;
        };
        let Some(tags) = rewrite(scope, &note.tags)? else {
            continue;
        };
        if tags == note.tags {
            continue;
        }
        note.tags = tags;
        if scope.put(&note)? == Written::Updated {
            written += 1;
        }
    }
    if let Some(progress) = progress {
        progress.report(total, Some(total));
    }
    Ok(written)
}

/// A plan's cancel check, for [`matching_notes`] and [`each_note_tags`].
pub(crate) fn plan_check<'p>(context: &'p PlanContext<'_>) -> impl Fn() -> Result<(), EngineError> + 'p {
    move || context.check_cancel()
}

/// No cancel check: a scan inside a write scope.
pub(crate) fn no_check() -> Result<(), EngineError> {
    Ok(())
}
