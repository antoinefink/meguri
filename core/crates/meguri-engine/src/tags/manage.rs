//! The tag management operations (`specs/tags/tags.md` T7 rename, T8
//! reparent, T9 delete, T11 Clear Unused Tags, T12 collapsed state), each
//! one undoable step (T15).
//!
//! Rename and reparent remove the old rows before they spell the new names,
//! write into the notes the spelling they register (T7, T8 deviations
//! `tags-rename-new-casing`, `tags-rename-existing-spelling`,
//! `tags-reparent-existing-spelling`), and give each row they register the
//! columns of the removed row it replaces (T7 step 5, deviations
//! `tags-rename-carries-columns*`).

use std::collections::{HashMap, HashSet};

use meguri_model::ids::NoteId;
use meguri_model::tag::Tag;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, StorageError, Table, WriteScope};
use meguri_text::fold::folded;

use super::canonical::{
    ancestors, check_new_name, first_component, join_note_tags, last_component, normalize, split_text,
};
use super::matching::Matcher;
use super::registry::{Speller, new_row, set_collapsed_in};
use super::{RunOptions, each_note_tags, matching_notes, no_check, op_spec, plan_check, planned, rewrite_notes};
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::OpResult;
use crate::executor::plan_commit::Plan;
use crate::executor::preconditions::Precondition;
use crate::progress::ProgressSink;
use crate::undo::names::{CLEAR_UNUSED_TAGS, EXPAND_COLLAPSE, REMOVE_TAG, RENAME_TAG};

fn invalid(error: impl std::fmt::Display) -> EngineError {
    EngineError::InvalidArgument {
        message: error.to_string(),
    }
}

/// The plan of a note-rewriting op: the notes that match on the snapshot,
/// with the set precondition that no note changed since (O7: the op's
/// target set is a predicate over the notes).
fn notes_plan(context: &crate::PlanContext<'_>, matcher: &Matcher) -> Result<Plan<Vec<NoteId>>, EngineError> {
    let notes = matching_notes(&**context.snapshot(), matcher, &plan_check(context))?;
    Ok(Plan {
        value: notes,
        preconditions: vec![Precondition::TableUntouched {
            table: Table::Notes,
            since: context.seq(),
        }],
    })
}

/// Removes every row that matches (T5). Returns the rows that went, by
/// their names' folding (U2): the sources of T7 step 5.
fn remove_matching_rows(scope: &mut WriteScope<'_>, matcher: &Matcher) -> Result<HashMap<String, Tag>, StorageError> {
    let mut removed = HashMap::new();
    for row in scope.tag_rows()? {
        if matcher.matches(&row.name) && scope.remove_tag(&row.name)?.changed() {
            removed.insert(folded(&row.name), row);
        }
    }
    Ok(removed)
}

/// T7 step 5: the row registered for a renamed or moved tag, with the
/// `collapsed` and `config` of its source (the removed row equal to the
/// matching tag) and `usn` -1; a new row's columns when no removed row
/// lists the tag.
fn carried_row(name: &str, source: Option<&Tag>) -> Tag {
    match source {
        Some(source) => Tag {
            name: name.to_owned(),
            usn: Usn::PENDING,
            collapsed: source.collapsed,
            config: source.config.clone(),
        },
        None => new_row(name),
    }
}

/// Whether one of the listed notes, read now, matches.
fn any_matches(scope: &WriteScope<'_>, notes: &[NoteId], matcher: &Matcher) -> Result<bool, StorageError> {
    for id in notes {
        if let Some(note) = scope.note(*id)?
            && matcher.matches_stored(&note.tags)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// T7 steps 4 to 7 and T8 step 4: each matching tag replaced by
/// `targets[name]` followed by the rest of the tag after its matched part,
/// normalized and spelled against the registry as it is now, in note id
/// order and stored order; other tags stay where they are (T6). A spelling
/// no row is equal to is registered with the columns of the tag's row in
/// `removed`, the first matching tag in that order supplying them; one
/// equal to a row is a merge and leaves that row as it is.
fn move_tags(
    scope: &mut WriteScope<'_>,
    matcher: &Matcher,
    targets: &[String],
    removed: &HashMap<String, Tag>,
    notes: &[NoteId],
    progress: Option<&ProgressSink>,
) -> Result<usize, StorageError> {
    let mut speller = Speller::new();
    rewrite_notes(scope, notes, progress, |scope, stored| {
        if !matcher.matches_stored(stored) {
            return Ok(None);
        }
        let mut tags = Vec::new();
        for tag in split_text(stored) {
            match matcher.first_match(tag) {
                Some((name, matched)) => {
                    let moved = format!("{}{}", targets[name], &tag[matched..]);
                    let source = removed.get(&folded(tag));
                    tags.push(speller.spell_with(scope, &normalize(&moved), |spelling| carried_row(spelling, source))?);
                }
                None => tags.push(tag.to_owned()),
            }
        }
        Ok(Some(join_note_tags(&tags)))
    })
}

/// T7 steps 2 to 7 on the notes that matched `from` when planned: nothing
/// when none of them matches now; otherwise the rows matching `from` go, and
/// the notes' matching tags move under `to`, their rows with them. Returns
/// the notes written.
fn apply_rename(
    scope: &mut WriteScope<'_>,
    from: &str,
    to: &str,
    notes: &[NoteId],
    progress: Option<&ProgressSink>,
) -> Result<usize, StorageError> {
    let matcher = Matcher::new([from]);
    if !any_matches(scope, notes, &matcher)? {
        return Ok(0);
    }
    let removed = remove_matching_rows(scope, &matcher)?;
    move_tags(scope, &matcher, &[to.to_owned()], &removed, notes, progress)
}

/// T7: renames `from` and its descendants to `to` on every note, as one
/// "Rename Tag" step. Refuses `to` per T7 step 1 (`InvalidArgument`
/// carrying `invalid_name`; [`check_new_name`] tells callers beforehand).
/// The value is the number of notes written.
pub fn rename(
    collection: &Collection,
    from: &str,
    to: &str,
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    check_new_name(to).map_err(invalid)?;
    let matcher = Matcher::new([from]);
    let (from, to) = (from.to_owned(), to.to_owned());
    planned(
        collection,
        ("rename_tag", RENAME_TAG),
        options,
        |context| notes_plan(context, &matcher),
        Vec::len,
        move |ctx, notes| {
            let progress = ctx.progress().cloned();
            Ok(apply_rename(ctx.scope(), &from, &to, &notes, progress.as_ref())?)
        },
    )
}

/// T7 inside `scope`.
pub fn rename_in(scope: &mut WriteScope<'_>, from: &str, to: &str) -> Result<usize, EngineError> {
    check_new_name(to).map_err(invalid)?;
    let notes = matching_notes(&*scope, &Matcher::new([from]), &no_check)?;
    Ok(apply_rename(scope, from, to, &notes, None)?)
}

/// T8 step 1: each source that moves, with its new name, in list order.
/// The comparisons are by exact characters.
pub fn reparent_moves<S: AsRef<str>>(sources: &[S], target: Option<&str>) -> Vec<(String, String)> {
    let mut moves = Vec::new();
    for source in sources {
        let source = source.as_ref();
        let last = last_component(source);
        let new_name = match target {
            Some(target) => {
                if target.starts_with(source) && first_component(target) == first_component(source) {
                    continue;
                }
                format!("{target}::{last}")
            }
            None => last.to_owned(),
        };
        if new_name != source {
            moves.push((source.to_owned(), new_name));
        }
    }
    moves
}

/// T8 steps 2 to 5 for the sources that move.
fn apply_reparent(
    scope: &mut WriteScope<'_>,
    moves: &[(String, String)],
    notes: &[NoteId],
    progress: Option<&ProgressSink>,
) -> Result<usize, StorageError> {
    let matcher = Matcher::new(moves.iter().map(|(source, _)| source));
    if !any_matches(scope, notes, &matcher)? {
        return Ok(0);
    }
    let removed = remove_matching_rows(scope, &matcher)?;
    let targets: Vec<String> = moves.iter().map(|(_, new_name)| new_name.clone()).collect();
    move_tags(scope, &matcher, &targets, &removed, notes, progress)
}

/// T8: moves the sources (as the tree spells them) under `target`, or to
/// the top level when `target` is `None`, as one "Rename Tag" step. Sources
/// that T8 step 1 skips are left alone, also when other sources move (Anki
/// fails there). The value is the number of notes written.
pub fn reparent<S: AsRef<str>>(
    collection: &Collection,
    sources: &[S],
    target: Option<&str>,
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let moves = reparent_moves(sources, target);
    let matcher = Matcher::new(moves.iter().map(|(source, _)| source));
    planned(
        collection,
        ("reparent_tags", RENAME_TAG),
        options,
        |context| notes_plan(context, &matcher),
        Vec::len,
        move |ctx, notes| {
            let progress = ctx.progress().cloned();
            Ok(apply_reparent(ctx.scope(), &moves, &notes, progress.as_ref())?)
        },
    )
}

/// T8 inside `scope`.
pub fn reparent_in<S: AsRef<str>>(
    scope: &mut WriteScope<'_>,
    sources: &[S],
    target: Option<&str>,
) -> Result<usize, EngineError> {
    let moves = reparent_moves(sources, target);
    let matcher = Matcher::new(moves.iter().map(|(source, _)| source));
    let notes = matching_notes(&*scope, &matcher, &no_check)?;
    Ok(apply_reparent(scope, &moves, &notes, None)?)
}

/// T9 steps 1 and 2: the notes lose their matching tags, and every matching
/// row goes, also when no note matched.
fn apply_delete(
    scope: &mut WriteScope<'_>,
    matcher: &Matcher,
    notes: &[NoteId],
    progress: Option<&ProgressSink>,
) -> Result<usize, StorageError> {
    let written = rewrite_notes(scope, notes, progress, |_, stored| {
        if !matcher.matches_stored(stored) {
            return Ok(None);
        }
        let kept: Vec<&str> = split_text(stored).filter(|tag| !matcher.matches(tag)).collect();
        Ok(Some(join_note_tags(&kept)))
    })?;
    remove_matching_rows(scope, matcher)?;
    Ok(written)
}

/// T9: removes the tags of `text` (split at separators) and their
/// descendants from every note and from the registry, as one "Remove Tag"
/// step. The value is the number of notes written.
pub fn delete(collection: &Collection, text: &str, options: &RunOptions) -> Result<OpResult<usize>, EngineError> {
    let names: Vec<String> = split_text(text).map(str::to_owned).collect();
    let matcher = Matcher::new(&names);
    let commit_matcher = matcher.clone();
    planned(
        collection,
        ("delete_tags", REMOVE_TAG),
        options,
        |context| notes_plan(context, &matcher),
        Vec::len,
        move |ctx, notes| {
            let progress = ctx.progress().cloned();
            Ok(apply_delete(ctx.scope(), &commit_matcher, &notes, progress.as_ref())?)
        },
    )
}

/// T9 inside `scope`.
pub fn delete_in(scope: &mut WriteScope<'_>, text: &str) -> Result<usize, EngineError> {
    let matcher = Matcher::new(split_text(text));
    let notes = matching_notes(&*scope, &matcher, &no_check)?;
    Ok(apply_delete(scope, &matcher, &notes, None)?)
}

/// T11 steps 1 and 2: the rows no note uses, by name. A note uses each of
/// its tags and every ancestor of each.
fn unused_rows(
    reader: &(impl ReadRows + ?Sized),
    between: &dyn Fn() -> Result<(), EngineError>,
) -> Result<Vec<String>, EngineError> {
    let mut used = HashSet::new();
    each_note_tags(reader, between, |stored| {
        for tag in split_text(stored) {
            if used.insert(folded(tag)) {
                for ancestor in ancestors(tag) {
                    used.insert(folded(ancestor));
                }
            }
        }
    })?;
    Ok(reader
        .tag_rows()?
        .into_iter()
        .filter(|row| !used.contains(&folded(&row.name)))
        .map(|row| row.name)
        .collect())
}

fn remove_rows(scope: &mut WriteScope<'_>, names: &[String]) -> Result<usize, StorageError> {
    let mut removed = 0;
    for name in names {
        if scope.remove_tag(name)?.changed() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// T11: removes the rows no note uses, keeping the ancestors of used tags,
/// as one "Clear Unused Tags" step. Notes are untouched. The value is the
/// number of rows removed.
pub fn clear_unused(collection: &Collection, options: &RunOptions) -> Result<OpResult<usize>, EngineError> {
    planned(
        collection,
        ("clear_unused_tags", CLEAR_UNUSED_TAGS),
        options,
        |context| {
            let unused = unused_rows(&**context.snapshot(), &plan_check(context))?;
            Ok(Plan {
                value: unused,
                preconditions: vec![
                    Precondition::TableUntouched {
                        table: Table::Notes,
                        since: context.seq(),
                    },
                    Precondition::TableUntouched {
                        table: Table::Tags,
                        since: context.seq(),
                    },
                ],
            })
        },
        Vec::len,
        |ctx, unused| Ok(remove_rows(ctx.scope(), &unused)?),
    )
}

/// T11 inside `scope`.
pub fn clear_unused_in(scope: &mut WriteScope<'_>) -> Result<usize, EngineError> {
    let unused = unused_rows(&*scope, &no_check)?;
    Ok(remove_rows(scope, &unused)?)
}

/// T12: sets the collapsed state of the named tags (normalized and spelled,
/// registered when missing), as one "Expand/Collapse" step. The value is
/// the number of rows whose state changed.
pub fn set_collapsed<S: AsRef<str>>(
    collection: &Collection,
    names: &[S],
    collapsed: bool,
) -> Result<OpResult<usize>, EngineError> {
    let names: Vec<String> = names.iter().map(|n| n.as_ref().to_owned()).collect();
    collection.run(
        op_spec(
            "set_tag_collapsed",
            EXPAND_COLLAPSE,
            names.len(),
            &RunOptions::default(),
        ),
        move |ctx| Ok(set_collapsed_in(ctx.scope(), &names, collapsed)?),
    )
}
