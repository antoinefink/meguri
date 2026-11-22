//! Find Duplicates (`specs/browser/workspace.md` V10): the notes of a
//! target whose values of one field have equal keys, in groups of two or
//! more (the key is meguri-search's `browser::duplicates::key`).
//!
//! It is a read that scales with the collection, so it runs as filter
//! counts and selection counts do: on a search worker's reader taken
//! through the search registry, never the query reader that row pages use,
//! so a replacement, a suspend or a close cancels it as it cancels a search
//! (`browser/result-sets` H3), and so does the caller's token. The note
//! types and the notes are read in one snapshot; a Found target's note ids
//! are copied from its handle in memory. It never writes.

use std::collections::HashSet;

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_search::SearchMode;
use meguri_search::browser::duplicates::Groups;
use meguri_search::browser::fields;
use meguri_storage::ReadRows;

use super::registry::bridge;
use super::selection::merged;
use super::{BrowseError, HandleSelection, check_revision, lock};
use crate::cancel::CancelToken;
use crate::error::EngineError;
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;

/// Notes read between two progress reports of All Notes.
const PACE: u64 = 1_000;
/// Note ids bound in one statement.
const CHUNK: usize = 10_000;

/// The notes Find Duplicates looks at (V10's Notes row).
#[derive(Clone, Copy, Debug)]
pub enum DuplicatesTarget<'a> {
    /// All Notes: every note of the collection.
    AllNotes(&'a Engine),
    /// All Found: the notes of these rows of a search handle (every note of
    /// its results when the selection covers them all).
    Found(&'a HandleSelection),
}

/// What Find Duplicates found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Duplicates {
    /// The groups: each holds two or more notes with equal keys, ascending;
    /// the groups by their first note.
    pub groups: Vec<Vec<NoteId>>,
    /// The target notes read: those whose note type has the field.
    pub considered: u64,
}

impl Duplicates {
    /// Every grouped note, ascending: the Duplicates token's note ids
    /// (`search/filter-tokens` T3).
    pub fn note_ids(&self) -> Vec<NoteId> {
        let mut ids: Vec<NoteId> = self.groups.iter().flatten().copied().collect();
        ids.sort_unstable();
        ids
    }
}

/// V10's `find_duplicates(field name, target)`: for every target note whose
/// note type has a field named exactly `field` (the first of two with that
/// name), its key; notes with an empty key are left out, and notes with
/// equal keys form a group. A field name no note type has finds nothing.
/// Reports its progress to `progress`: the notes read of those note types'
/// notes for All Notes, the listed notes read of the listed notes for All
/// Found. A Found target is refused as `resolve_selection` refuses
/// it (`UnknownHandle`, `StaleRevision`, `OutOfRange`); `Cancelled` once
/// `cancel` or a stop cancels it.
pub fn find_duplicates(
    target: DuplicatesTarget<'_>,
    field: &str,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Duplicates, BrowseError> {
    let cancelled = || BrowseError::Engine(EngineError::Cancelled);
    let engine = match target {
        DuplicatesTarget::AllNotes(engine) => engine.clone(),
        DuplicatesTarget::Found(selection) => {
            // A released handle is unknown, whatever else is unavailable (H6).
            selection.shared.handle(selection.handle)?;
            selection.shared.engine()?
        }
    };
    let registry = &engine.inner.searches;
    let token = CancelToken::new();
    bridge(cancel, &token, registry);
    let (epoch, stopping) = registry.epoch();
    if stopping {
        return Err(BrowseError::Engine(EngineError::unavailable(
            "the collection is changing generation, suspending or closing",
        )));
    }
    let _listed = registry.run(&token, epoch);
    let collection = engine.collection()?;
    let slot = registry.reader(&token).ok_or_else(cancelled)?;
    let lease = collection.search_reader(slot.index)?;
    let snapshot = lease.snapshot()?;
    // H6: the generation is compared while the reader is held.
    let notes: Option<Vec<NoteId>> = match target {
        DuplicatesTarget::AllNotes(_) => None,
        DuplicatesTarget::Found(selection) => {
            let handle = selection.shared.handle(selection.handle)?;
            let handle = lock(&handle);
            if handle.generation != collection.generation() {
                return Err(BrowseError::UnknownHandle);
            }
            check_revision(&handle, selection.revision)?;
            let ranges = merged(&selection.ranges);
            if ranges.last().is_some_and(|range| range.end > handle.count()) {
                return Err(BrowseError::OutOfRange);
            }
            let indexes = ranges.iter().flat_map(|range| range.start as usize..range.end as usize);
            let listed = match handle.mode {
                SearchMode::Cards => &handle.nids,
                SearchMode::Notes => &handle.ids,
            };
            let mut seen = HashSet::new();
            Some(
                indexes
                    .map(|index| listed[index])
                    .filter(|note| seen.insert(*note))
                    .map(NoteId)
                    .collect(),
            )
        }
    };
    let stopped = |error: meguri_storage::StorageError| {
        if token.is_cancelled() { cancelled() } else { BrowseError::from(error) }
    };
    // Each note type with the field, and the field's position in it.
    // Every note type id, `i64::MIN` included (an id lower bound would
    // leave it out).
    let mut ords: Vec<(NotetypeId, u32)> = Vec::new();
    for (id, _) in snapshot.notetype_names()? {
        if let Some(kind) = snapshot.notetype(NotetypeId(id))?
            && let Some(ord) = fields::ord_of(&kind, field)
        {
            ords.push((kind.id, ord));
        }
    }
    if token.is_cancelled() {
        return Err(cancelled());
    }
    let mut found = Duplicates::default();
    if ords.is_empty() || notes.as_ref().is_some_and(Vec::is_empty) {
        return Ok(found);
    }
    ords.sort_unstable();
    let kinds: Vec<NotetypeId> = ords.iter().map(|(id, _)| *id).collect();
    let total = match &notes {
        Some(notes) => notes.len() as u64,
        None => snapshot
            .note_counts_by_notetype()?
            .into_iter()
            .filter(|(id, _)| kinds.binary_search(id).is_ok())
            .map(|(_, count)| count)
            .sum(),
    };
    if token.is_cancelled() {
        return Err(cancelled());
    }
    progress.report(0, Some(total));
    let mut groups = Groups::new();
    let stop = || token.is_cancelled();
    let every = notes.is_none();
    let mut visit = |id: i64, mid: i64, flds: Option<&str>| {
        found.considered += 1;
        if every && found.considered.is_multiple_of(PACE) {
            progress.report(found.considered, Some(total));
        }
        if let (Some(flds), Ok(at)) = (flds, ords.binary_search_by_key(&mid, |(id, _)| id.0)) {
            groups.add(NoteId(id), fields::field_at(flds, ords[at].1));
        }
        !token.is_cancelled()
    };
    match &notes {
        None => snapshot
            .visit_note_fields(&kinds, None, &stop, &mut visit)
            .map_err(stopped)?,
        // The listed notes a chunk at a time, each chunk one report.
        Some(notes) => {
            let mut done = 0u64;
            for chunk in notes.chunks(CHUNK) {
                if token.is_cancelled() {
                    return Err(cancelled());
                }
                snapshot
                    .visit_note_fields(&kinds, Some(chunk), &stop, &mut visit)
                    .map_err(stopped)?;
                done += chunk.len() as u64;
                progress.report(done, Some(total));
            }
        }
    }
    if token.is_cancelled() {
        return Err(cancelled());
    }
    progress.report(total, Some(total));
    found.groups = groups.finish();
    Ok(found)
}
