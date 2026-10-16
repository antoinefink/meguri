//! Marked notes (`specs/tags/flags-and-marked.md` M1, M2): a note is marked
//! when it stores a tag equal to `marked` under `unicase`; marking adds the
//! tag and unmarking removes it, descendants included, through the tag
//! primitives of `tags/tags` T10.

use meguri_model::ids::NoteId;
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use meguri_text::fold::folded;

use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::OpResult;
use crate::executor::plan_commit::REPLANS;
use crate::executor::preconditions::Precondition;
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
/// selected notes. The decision is read before the op and held by a
/// precondition on the current note's row, so a save in between makes it
/// decide again.
pub fn toggle_mark(
    collection: &Collection,
    current: NoteId,
    targets: &[NoteId],
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let key = RowKey::Id(current.0);
    let mut attempt = 0;
    loop {
        let (marked, digest) = {
            let reader = collection.reader(ReaderRole::Query)?;
            let marked = reader.note(current)?.is_some_and(|note| is_marked(&note.tags));
            (marked, reader.row_digest(Table::Notes, &key)?)
        };
        let precondition = Precondition::RowDigest {
            table: Table::Notes,
            key: key.clone(),
            digest,
        };
        let notes = targets.to_vec();
        let result = if marked {
            collection.run(
                op_spec("remove_note_tags", REMOVE_TAG, notes.len(), options).precondition(precondition),
                move |ctx| {
                    let progress = ctx.progress().cloned();
                    Ok(bulk::remove_with(ctx.scope(), &notes, MARKED, progress.as_ref())?)
                },
            )
        } else {
            collection.run(
                op_spec("add_note_tags", UPDATE_TAG, notes.len(), options).precondition(precondition),
                move |ctx| {
                    let progress = ctx.progress().cloned();
                    Ok(bulk::add_with(ctx.scope(), &notes, MARKED, progress.as_ref())?)
                },
            )
        };
        match result {
            Err(EngineError::Stale { .. }) if attempt < REPLANS => attempt += 1,
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
