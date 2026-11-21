//! Selection by index (`browser/result-sets` H12): selections stay index
//! ranges of a handle until a command runs; `resolve_selection` then gives
//! the ids it works on, from one read transaction on the query reader.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use meguri_search::SearchMode;
use meguri_search::plan::ids;
use meguri_search::plan::sql::id_list;
use meguri_storage::{ReadRows, ReaderRole};

use super::{BrowseError, HandleId, SessionShared, check_revision, lock, worker};

/// Ranges ascending and merged; an empty range adds nothing.
pub(super) fn merged(ranges: &[Range<u64>]) -> Vec<Range<u64>> {
    let mut sorted: Vec<Range<u64>> = ranges.iter().filter(|range| range.start < range.end).cloned().collect();
    sorted.sort_by_key(|range| range.start);
    let mut out: Vec<Range<u64>> = Vec::new();
    for range in sorted {
        match out.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}

/// Every card of each note in `notes`, ascending card id, from one read
/// transaction on the query reader.
pub(super) fn cards_of_notes(
    collection: &crate::Collection,
    notes: &[i64],
) -> Result<HashMap<i64, Vec<i64>>, BrowseError> {
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let mut cards: HashMap<i64, Vec<i64>> = HashMap::new();
    let params = worker::cells(&[id_list(notes)]);
    snapshot.visit_id_rows(ids::CARDS_OF_NOTES, &params, &|| false, &mut |row| {
        cards.entry(row[1]).or_default().push(row[0]);
        true
    })?;
    Ok(cards)
}

pub(super) fn resolve(
    shared: &Arc<SessionShared>,
    handle: HandleId,
    revision: u64,
    ranges: &[Range<u64>],
    target: SearchMode,
) -> Result<Vec<i64>, BrowseError> {
    let collection = shared.engine()?.collection()?;
    let (mode, selected, notes) = {
        let handle = shared.handle(handle)?;
        let handle = lock(&handle);
        check_revision(&handle, revision)?;
        let ranges = merged(ranges);
        if ranges.last().is_some_and(|range| range.end > handle.count()) {
            return Err(BrowseError::OutOfRange);
        }
        let indexes = ranges.iter().flat_map(|range| range.start as usize..range.end as usize);
        let selected: Vec<i64> = indexes.clone().map(|index| handle.ids[index]).collect();
        let notes: Vec<i64> = if handle.mode == SearchMode::Cards {
            indexes.map(|index| handle.nids[index]).collect()
        } else {
            Vec::new()
        };
        (handle.mode, selected, notes)
    };
    Ok(match (mode, target) {
        (SearchMode::Cards, SearchMode::Cards) | (SearchMode::Notes, SearchMode::Notes) => selected,
        // The distinct notes, in the order of their first selected card.
        (SearchMode::Cards, SearchMode::Notes) => {
            let mut seen = HashSet::new();
            notes.into_iter().filter(|note| seen.insert(*note)).collect()
        }
        // Every card of each selected note, ascending card id; a note
        // without cards adds none.
        (SearchMode::Notes, SearchMode::Cards) => {
            let mut cards = cards_of_notes(&collection, &selected)?;
            selected
                .iter()
                .flat_map(|note| cards.remove(note).unwrap_or_default())
                .collect()
        }
    })
}
