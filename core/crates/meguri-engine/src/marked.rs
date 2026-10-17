//! Marked notes (`specs/tags/flags-and-marked.md` M1, M2): a note is marked
//! when it stores a tag equal to `marked` under `unicase`; marking adds the
//! tag and unmarking removes it, descendants included, through the tag
//! primitives of `tags/tags` T10.

use meguri_model::ids::NoteId;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_text::fold::folded;

use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::OpResult;
use crate::executor::plan_commit::REPLANS;
use crate::tags::canonical::split_text;
use crate::tags::{RunOptions, bulk, op_spec};
use crate::undo::names::{REMOVE_TAG, UPDATE_TAG};

/// The marked tag.
pub const MARKED: &str = "marked";

/// M1: whether a note with this stored `tags` text is marked. A descendant
/// such as `marked::x` alone does not mark it.
pub fn is_marked(stored: &str) -> bool {
    let marked = folded(MARKED);
    split_text(stored).any(|tag| folded(tag) == marked)
}

/// M2 Mark: adds `marked` (as the registry spells it) to the notes, one
/// "Update Tag" step. The value is the number of notes written.
pub fn mark(collection: &Collection, notes: &[NoteId], options: &RunOptions) -> Result<OpResult<usize>, EngineError> {
    bulk::add_to_notes(collection, notes, MARKED, options)
}

/// M2 Unmark: removes `marked` and its descendants from the notes, one
/// "Remove Tag" step. The value is the number of notes written.
pub fn unmark(collection: &Collection, notes: &[NoteId], options: &RunOptions) -> Result<OpResult<usize>, EngineError> {
    bulk::remove_from_notes(collection, notes, MARKED, options)
}

/// M2 Toggle: the current note decides for every target. When it is
/// marked, the targets are unmarked; otherwise they are marked. Review
/// passes the current note alone; Browse the focused row's note and the
/// selected notes.
///
/// The write phase decides: it reads the current note from the live row,
/// in the transaction that writes, as `flags::toggle_flag` does. The op
/// must name its undo step ("Update Tag" or "Remove Tag") before it runs, so
/// it is submitted as the step a read made beforehand expects; when the
/// live row disagrees, the write phase writes nothing and the op runs again
/// as the other step.
pub fn toggle_mark(
    collection: &Collection,
    current: NoteId,
    targets: &[NoteId],
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let mut unmark = {
        let reader = collection.reader(ReaderRole::Query)?;
        reader.note(current)?.is_some_and(|note| is_marked(&note.tags))
    };
    let mut attempt = 0;
    loop {
        let notes = targets.to_vec();
        let (name, step) = if unmark {
            ("remove_note_tags", REMOVE_TAG)
        } else {
            ("add_note_tags", UPDATE_TAG)
        };
        let result = collection.run(op_spec(name, step, notes.len(), options), move |ctx| {
            let marked = ctx.scope().note(current)?.is_some_and(|note| is_marked(&note.tags));
            if marked != unmark {
                return Err(EngineError::Stale {
                    table: Table::Notes,
                    key: current.0.to_string(),
                });
            }
            let progress = ctx.progress().cloned();
            let written = if unmark {
                bulk::remove_with(ctx.scope(), &notes, MARKED, progress.as_ref())?
            } else {
                bulk::add_with(ctx.scope(), &notes, MARKED, progress.as_ref())?
            };
            Ok(written)
        });
        match result {
            // The live row had the other state: run as the step it decides.
            Err(EngineError::Stale { .. }) if attempt < REPLANS => {
                attempt += 1;
                unmark = !unmark;
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_marked;

    #[test]
    fn marked_is_the_tag_itself_in_any_case() {
        assert!(is_marked(" Marked q "));
        assert!(is_marked(" MARKED "));
        assert!(!is_marked(" marked::x "));
        assert!(!is_marked(" markedly "));
        assert!(!is_marked(""));
    }
}
