//! Adding tags to notes and removing them (`specs/tags/tags.md` T10): the
//! primitives of Browse's Add Tags and Remove Tags, the sidebar's Add to and
//! Remove from Selected Notes, marking (`tags/flags-and-marked` M2) and
//! AnkiConnect (U141). Each is one undoable step: "Update Tag" or "Remove
//! Tag" (T15). The write phase reads every listed note from the live row;
//! notes that no longer exist are skipped.

use std::collections::HashSet;

use meguri_model::ids::NoteId;
use meguri_storage::{StorageError, WriteScope};
use meguri_text::fold::{compare_folded, folded};

use super::canonical::{join_note_tags, normalize, split_text};
use super::matching::Matcher;
use super::registry::Speller;
use super::{RunOptions, op_spec, rewrite_notes};
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::OpResult;
use crate::progress::ProgressSink;
use crate::undo::names::{REMOVE_TAG, UPDATE_TAG};

/// T10 Add step 1: the tags of `text`, normalized and spelled in order
/// (registering new ones), each once.
fn spelled_tags(scope: &mut WriteScope<'_>, text: &str) -> Result<Vec<(String, String)>, StorageError> {
    let mut speller = Speller::new();
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for piece in split_text(text) {
        let spelling = speller.spell(scope, &normalize(piece))?;
        let fold = folded(&spelling);
        if seen.insert(fold.clone()) {
            tags.push((spelling, fold));
        }
    }
    Ok(tags)
}

/// T10 Add within `scope`, with a progress sink for the write phase.
pub(crate) fn add_with(
    scope: &mut WriteScope<'_>,
    notes: &[NoteId],
    text: &str,
    progress: Option<&ProgressSink>,
) -> Result<usize, StorageError> {
    let adding = spelled_tags(scope, text)?;
    if adding.is_empty() {
        return Ok(0);
    }
    rewrite_notes(scope, notes, progress, |_, stored| {
        let mut tags: Vec<&str> = split_text(stored).collect();
        let stored_folds: HashSet<String> = tags.iter().map(|tag| folded(tag)).collect();
        let before = tags.len();
        tags.extend(
            adding
                .iter()
                .filter(|(_, fold)| !stored_folds.contains(fold))
                .map(|(spelling, _)| spelling.as_str()),
        );
        if tags.len() == before {
            return Ok(None);
        }
        let mut seen = HashSet::new();
        tags.retain(|tag| seen.insert(folded(tag)));
        tags.sort_by(|a, b| compare_folded(a, b));
        Ok(Some(join_note_tags(&tags)))
    })
}

/// T10 Add inside `scope`: the number of notes written. Registration happens
/// even when no note changes.
pub fn add_in(scope: &mut WriteScope<'_>, notes: &[NoteId], text: &str) -> Result<usize, EngineError> {
    Ok(add_with(scope, notes, text, None)?)
}

/// T10 Add: adds the tags of `text` (split at separators) to the notes, as
/// one "Update Tag" step. The value is the number of notes written.
pub fn add_to_notes(
    collection: &Collection,
    notes: &[NoteId],
    text: &str,
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let (notes, text) = (notes.to_vec(), text.to_owned());
    collection.run(op_spec("add_note_tags", UPDATE_TAG, notes.len(), options), move |ctx| {
        let progress = ctx.progress().cloned();
        Ok(add_with(ctx.scope(), &notes, &text, progress.as_ref())?)
    })
}

/// T10 Remove within `scope`, with a progress sink for the write phase.
pub(crate) fn remove_with(
    scope: &mut WriteScope<'_>,
    notes: &[NoteId],
    text: &str,
    progress: Option<&ProgressSink>,
) -> Result<usize, StorageError> {
    let matcher = Matcher::new(split_text(text));
    if matcher.is_empty() {
        return Ok(0);
    }
    rewrite_notes(scope, notes, progress, |_, stored| {
        if !matcher.matches_stored(stored) {
            return Ok(None);
        }
        let kept: Vec<&str> = split_text(stored).filter(|tag| !matcher.matches(tag)).collect();
        Ok(Some(join_note_tags(&kept)))
    })
}

/// T10 Remove inside `scope`: the number of notes written. Rows stay.
pub fn remove_in(scope: &mut WriteScope<'_>, notes: &[NoteId], text: &str) -> Result<usize, EngineError> {
    Ok(remove_with(scope, notes, text, None)?)
}

/// T10 Remove: removes the tags that match a name of `text` (T5,
/// descendants included) from the notes, as one "Remove Tag" step. The
/// registry is untouched. The value is the number of notes written.
pub fn remove_from_notes(
    collection: &Collection,
    notes: &[NoteId],
    text: &str,
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let (notes, text) = (notes.to_vec(), text.to_owned());
    collection.run(
        op_spec("remove_note_tags", REMOVE_TAG, notes.len(), options),
        move |ctx| {
            let progress = ctx.progress().cloned();
            Ok(remove_with(ctx.scope(), &notes, &text, progress.as_ref())?)
        },
    )
}
