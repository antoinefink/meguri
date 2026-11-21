//! Browse's note actions (`browser/workspace` V8.1, `browser/bulk-actions`
//! B1, B9, B11, B16) as bulk actions (`browser/bulk-framework` K7).
//!
//! - **Add Tags** and **Remove Tags** are `tags/tags` T10 Add ("Update Tag")
//!   and Remove ("Remove Tag") on the target notes, with the text the tag
//!   entry gives. B9: every run of U+0020, LF, TAB and VT in the text
//!   becomes one space first, so each of them separates two tags, for Remove
//!   Tags too (B18). A text holding no tag runs nothing: it is refused with
//!   `InvalidArgument` before anything is read, so both undo histories stay
//!   as they were.
//! - **Delete Notes** is `notes/note-records` D1 on the target notes in
//!   list order ("Delete Note"): every card of each note goes too. It
//!   reports the notes removed as `affected` and the cards removed as its
//!   second count (B11, K5).
//!
//! Each commit reads every target note from its live row, so nothing the
//! compute phase saw is written over (K3).

use std::path::PathBuf;
use std::sync::Arc;

use meguri_model::ids::NoteId;
use meguri_storage::{ReadRows, ReaderRole};

use super::{BulkCounts, BulkJob, BulkOutcome, NoteTarget, ResolvedNotes, check_ids, op_spec};
use crate::cancel::CancelToken;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpContext, OpResult};
use crate::executor::plan_commit::{Plan, PlanContext, plan_and_commit_with};
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;
use crate::undo::names::{DELETE_NOTE, REMOVE_TAG, UPDATE_TAG};

/// A note action of V8.1 besides Find and Replace and Change Note Type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteAction {
    /// The tag entry's text (B9).
    AddTags(String),
    /// The tag entry's text (B9).
    RemoveTags(String),
    DeleteNotes,
}

/// A note action's result: B16's counts, and for Delete Notes the cards
/// removed (B11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoteCounts {
    pub counts: BulkCounts,
    pub cards_removed: Option<u64>,
}

/// B9: every run of U+0020, LF, TAB and VT becomes one space.
pub fn tag_entry_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if matches!(c, ' ' | '\n' | '\t' | '\u{0b}') {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

impl NoteAction {
    fn op_name(&self) -> &'static str {
        match self {
            NoteAction::AddTags(_) => "add_note_tags",
            NoteAction::RemoveTags(_) => "remove_note_tags",
            NoteAction::DeleteNotes => "delete_notes",
        }
    }

    fn step(&self) -> &'static str {
        match self {
            NoteAction::AddTags(_) => UPDATE_TAG,
            NoteAction::RemoveTags(_) => REMOVE_TAG,
            NoteAction::DeleteNotes => DELETE_NOTE,
        }
    }

    /// The tag text after B9's rule, refused when it holds no tag.
    fn tag_text(text: &str) -> Result<String, EngineError> {
        let text = tag_entry_text(text);
        if crate::tags::split_text(&text).next().is_none() {
            return Err(EngineError::InvalidArgument {
                message: "the tag entry holds no tag (B9)".to_owned(),
            });
        }
        Ok(text)
    }
}

/// What the compute phase hands the commit: only the ids (K7).
#[derive(Debug)]
struct Planned {
    notes: Vec<NoteId>,
}

/// The compute phase: the commit re-reads every note from its live row, so
/// there is nothing to plan ahead; the cancel point and `Planning` progress.
fn plan(context: &PlanContext<'_>, notes: &[NoteId]) -> Result<Plan<Planned>, EngineError> {
    context.check_cancel()?;
    if let Some(progress) = context.progress() {
        let total = notes.len() as u64;
        progress.report(total, Some(total));
    }
    Ok(Plan {
        value: Planned { notes: notes.to_vec() },
        preconditions: Vec::new(),
    })
}

/// The write phase. `text` is the B9 text of a tag action.
fn commit(
    context: &mut OpContext<'_, '_>,
    action: &NoteAction,
    text: &str,
    path: &std::path::Path,
    planned: Planned,
) -> Result<NoteCounts, EngineError> {
    let considered = planned.notes.len() as u64;
    let notes = planned.notes;
    let (affected, cards_removed) = match action {
        NoteAction::AddTags(_) => (crate::tags::add_in(context.scope(), &notes, text)?, None),
        NoteAction::RemoveTags(_) => (crate::tags::remove_in(context.scope(), &notes, text)?, None),
        NoteAction::DeleteNotes => {
            // The cards that go with the notes, counted before they go.
            let cards = context.scope().cards_of_notes_with_ids(&notes)?.len() as u64;
            let removed = crate::notes::delete_in(context.scope(), path, &notes)?;
            (removed, Some(cards))
        }
    };
    Ok(NoteCounts {
        counts: BulkCounts {
            affected: affected as u64,
            considered,
        },
        cards_removed,
    })
}

/// The action on `target`, blocking: a compute phase on a maintenance
/// snapshot, then one write transaction and one undo step (B16). The
/// all-notes target is Find and Replace's alone (`browser/find-replace` F8):
/// here it is refused with `InvalidArgument`.
pub fn run(
    collection: &Collection,
    target: &NoteTarget,
    action: &NoteAction,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<NoteCounts>, EngineError> {
    cancel.check()?;
    let ResolvedNotes::Notes(notes) = target.resolve(collection)? else {
        return Err(EngineError::InvalidArgument {
            message: "a note action other than Find and Replace takes listed or selected notes".to_owned(),
        });
    };
    check_ids(&notes, "note")?;
    let text = match action {
        NoteAction::AddTags(text) | NoteAction::RemoveTags(text) => NoteAction::tag_text(text)?,
        NoteAction::DeleteNotes => String::new(),
    };
    let (name, step) = (action.op_name(), action.step());
    let action = action.clone();
    let path: PathBuf = collection.path().to_owned();
    let (cancel_plan, progress_plan) = (Arc::clone(&cancel), progress.clone());
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        op_spec(name, step, 0, &cancel, &progress),
        move |context| plan(context, &notes),
        move |_, planned| op_spec(name, step, planned.notes.len(), &cancel_plan, &progress_plan),
        move |context, planned| commit(context, &action, &text, &path, planned),
    )
}

/// [`run`] as a job (`browser/bulk-framework` K2).
pub fn start(engine: &Engine, collection: Collection, target: NoteTarget, action: NoteAction) -> Result<BulkJob, EngineError> {
    super::start(engine, move |context| {
        run(
            &collection,
            &target,
            &action,
            Arc::clone(&context.cancel),
            context.progress.clone(),
        )
        .map(|result| BulkOutcome::split(result, |value| (value.counts, value.cards_removed)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b9_runs_of_separators_become_one_space() {
        assert_eq!(tag_entry_text("a\tb"), "a b");
        assert_eq!(tag_entry_text("a \n\t\u{0b} b"), "a b");
        assert_eq!(tag_entry_text("  a  "), " a ");
        assert_eq!(tag_entry_text("a\rb"), "a\rb");
        assert_eq!(tag_entry_text("a\u{3000}b"), "a\u{3000}b");
        assert_eq!(tag_entry_text(""), "");
    }
}
