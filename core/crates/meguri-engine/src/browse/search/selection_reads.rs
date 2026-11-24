//! Reads of a Browse selection besides its counts (`specs/browser/workspace.md`
//! V7, V8.1, V8.2), each in the core so a large selection never crosses the
//! FFI as ids: the cards a Delete removes, the tags of the target notes
//! (Remove Tags' suggestions) and the rows Select Sibling Cards selects.
//! Like the counts they read one snapshot of a search worker's reader
//! (`selection_counts`), and never write.

use std::collections::HashSet;
use std::ops::Range;

use meguri_search::SearchMode;
use meguri_search::plan::ids;
use meguri_search::plan::sql::id_list;
use meguri_storage::ReadRows;

use super::selection::merged;
use super::selection_counts::read_selection;
use super::{BrowseError, HandleSelection, check_revision, lock, worker};
use crate::cancel::CancelToken;
use crate::error::EngineError;
use crate::tags::canonical::split_text;

/// Ids bound in one statement.
const CHUNK: usize = 10_000;

fn distinct(ids: &[i64]) -> Vec<i64> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// V8.2: every card of the target notes that exist (`browser/bulk-actions`
/// B1, B11), the cards the search did not find included: the cards a
/// Delete removes. Refused and cancelled as `selection_counts` is.
pub fn selection_note_cards(selection: &HandleSelection, cancel: &CancelToken) -> Result<u64, BrowseError> {
    read_selection(selection, cancel, |snapshot, picked, token| {
        let notes = distinct(picked.notes());
        let stop = || token.is_cancelled();
        let mut cards = 0u64;
        for chunk in notes.chunks(CHUNK) {
            if token.is_cancelled() {
                return Err(BrowseError::Engine(EngineError::Cancelled));
            }
            snapshot
                .visit_id_rows(ids::CARDS_OF_EXISTING_NOTES, &worker::cells(&[id_list(chunk)]), &stop, &mut |_| {
                    cards += 1;
                    true
                })
                .map_err(|error| stopped(token, error))?;
        }
        Ok(cards)
    })
}

/// V8.1: the tags the target notes store (Remove Tags' field suggests only
/// these), each once whatever its letter case, sorted by `unicase`
/// (`text/unicode-primitives` U2). Two spellings are one when their
/// lowercase forms are equal, close to the simple folding Remove matches
/// with (`tags/tags` T5): `Straße` and `STRASSE` stay two, as removing one
/// leaves the other. The spelling of the note with the lowest id is kept. A
/// note whose `tags` is not text holds none. Refused and cancelled as
/// `selection_counts` is.
pub fn selection_tags(selection: &HandleSelection, cancel: &CancelToken) -> Result<Vec<String>, BrowseError> {
    read_selection(selection, cancel, |snapshot, picked, token| {
        let notes = distinct(picked.notes());
        let stop = || token.is_cancelled();
        let mut seen = HashSet::new();
        let mut tags: Vec<String> = Vec::new();
        for chunk in notes.chunks(CHUNK) {
            if token.is_cancelled() {
                return Err(BrowseError::Engine(EngineError::Cancelled));
            }
            snapshot
                .visit_tags_of_notes(chunk, &stop, &mut |_, text| {
                    for tag in text.into_iter().flat_map(split_text) {
                        if seen.insert(tag.to_lowercase()) {
                            tags.push(tag.to_owned());
                        }
                    }
                    true
                })
                .map_err(|error| stopped(token, error))?;
        }
        tags.sort_by(|a, b| meguri_text::fold::compare_folded(a, b).then_with(|| a.cmp(b)));
        Ok(tags)
    })
}

fn stopped(token: &CancelToken, error: meguri_storage::StorageError) -> BrowseError {
    if token.is_cancelled() {
        BrowseError::Engine(EngineError::Cancelled)
    } else {
        BrowseError::from(error)
    }
}

impl HandleSelection {
    /// V7's Select Sibling Cards: the rows of the handle, at the selection's
    /// revision, whose note is the note of a selected row, as ascending merged
    /// ranges. It adds no row the handle does not list; in notes mode the
    /// selection's own rows. Read from the handle in memory, under its lock:
    /// no reader and no write. Refused as `resolve_selection` refuses
    /// (`UnknownHandle`, `StaleRevision`, `OutOfRange`).
    pub fn sibling_rows(&self) -> Result<Vec<Range<u64>>, BrowseError> {
        let handle = self.shared.handle(self.handle)?;
        let collection = self.shared.engine()?.collection()?;
        let handle = lock(&handle);
        if handle.generation != collection.generation() {
            return Err(BrowseError::UnknownHandle);
        }
        check_revision(&handle, self.revision)?;
        let ranges = merged(&self.ranges);
        if ranges.last().is_some_and(|range| range.end > handle.count()) {
            return Err(BrowseError::OutOfRange);
        }
        if handle.mode == SearchMode::Notes {
            return Ok(ranges);
        }
        let notes: HashSet<i64> = ranges
            .iter()
            .flat_map(|range| range.start as usize..range.end as usize)
            .map(|index| handle.nids[index])
            .collect();
        let mut out: Vec<Range<u64>> = Vec::new();
        for (index, note) in handle.nids.iter().take(handle.count() as usize).enumerate() {
            if !notes.contains(note) {
                continue;
            }
            let index = index as u64;
            match out.last_mut() {
                Some(last) if last.end == index => last.end = index + 1,
                _ => out.push(index..index + 1),
            }
        }
        Ok(out)
    }
}
