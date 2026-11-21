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

/// H6: a call through a handle reads the generation the handle was made in.
fn check_generation(handle: &super::handle::Handle, collection: &crate::Collection) -> Result<(), BrowseError> {
    if handle.generation == collection.generation() {
        Ok(())
    } else {
        Err(BrowseError::UnknownHandle)
    }
}

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

/// Every card of each note in `notes`, ascending card id, in a read
/// transaction the caller holds.
fn cards_of_notes_in(rows: &impl ReadRows, notes: &[i64]) -> Result<HashMap<i64, Vec<i64>>, BrowseError> {
    let mut cards: HashMap<i64, Vec<i64>> = HashMap::new();
    let params = worker::cells(&[id_list(notes)]);
    rows.visit_id_rows(ids::CARDS_OF_NOTES, &params, &|| false, &mut |row| {
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
    // H6: the generation is compared while the query reader is held, and
    // the notes' cards are read in the same transaction.
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let (mode, selected, notes) = {
        let handle = shared.handle(handle)?;
        let handle = lock(&handle);
        check_generation(&handle, &collection)?;
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
            let mut cards = cards_of_notes_in(&*snapshot, &selected)?;
            selected
                .iter()
                .flat_map(|note| cards.remove(note).unwrap_or_default())
                .collect()
        }
    })
}

/// The notes of `cards` that exist, from one read transaction on the query
/// reader: card id to note id.
fn notes_of_cards_in(rows: &impl ReadRows, cards: &[i64]) -> Result<HashMap<i64, i64>, BrowseError> {
    let mut notes = HashMap::new();
    let params = worker::cells(&[id_list(cards)]);
    rows.visit_id_rows(ids::NOTES_OF_CARDS, &params, &|| false, &mut |row| {
        notes.insert(row[0], row[1]);
        true
    })?;
    Ok(notes)
}

/// H12: the index of `id` (a card id when `kind` is cards, a note id when
/// it is notes) in the handle's revision `revision`, or none.
pub(super) fn index_of(
    shared: &Arc<SessionShared>,
    handle: HandleId,
    revision: u64,
    kind: SearchMode,
    id: i64,
) -> Result<Option<u64>, BrowseError> {
    let collection = shared.engine()?.collection()?;
    // H6: the generation is compared while the query reader is held.
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let handle = shared.handle(handle)?;
    let mode = {
        let handle = lock(&handle);
        check_generation(&handle, &collection)?;
        check_revision(&handle, revision)?;
        handle.mode
    };
    // A card id on a notes handle: its note, read now (none when the card
    // no longer exists); then the lookup, under the handle's lock, over the
    // announced prefix only.
    let (target, by_note) = match (mode, kind) {
        (SearchMode::Notes, SearchMode::Cards) => match notes_of_cards_in(&*snapshot, &[id])?.get(&id) {
            Some(note) => (*note, false),
            None => return Ok(None),
        },
        // The lowest index among the note's cards.
        (SearchMode::Cards, SearchMode::Notes) => (id, true),
        _ => (id, false),
    };
    let handle = lock(&handle);
    check_generation(&handle, &collection)?;
    check_revision(&handle, revision)?;
    let count = handle.count() as usize;
    let list = if by_note { &handle.nids[..count] } else { &handle.ids[..count] };
    Ok(list.iter().position(|id| *id == target).map(|index| index as u64))
}

/// Where `map_selection` maps from (H12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapFrom {
    /// A handle of the session, at a revision.
    Handle { handle: HandleId, revision: u64 },
    /// A released handle's mode: the selection and the current row are ids
    /// the caller holds (rows it fetched).
    Mode(SearchMode),
}

/// What is selected in `from` (H12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Index ranges of a [`MapFrom::Handle`].
    Ranges(Vec<Range<u64>>),
    /// Ids of `from`'s mode.
    Ids(Vec<i64>),
}

/// The result of `map_selection`: ascending merged ranges of `to` and the
/// current index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapped {
    pub selected: Vec<Range<u64>>,
    pub current: Option<u64>,
}

fn ranges_of(mut indexes: Vec<u64>) -> Vec<Range<u64>> {
    indexes.sort_unstable();
    indexes.dedup();
    let mut out: Vec<Range<u64>> = Vec::new();
    for index in indexes {
        match out.last_mut() {
            Some(last) if last.end == index => last.end += 1,
            _ => out.push(index..index + 1),
        }
    }
    out
}

/// H12: maps a selection and a current row between two handles of the
/// session (a re-search or a toggle), over the final prefix of `to` at
/// `to_revision`.
pub(super) fn map(
    shared: &Arc<SessionShared>,
    from: MapFrom,
    selection: Selection,
    current: Option<i64>,
    to: HandleId,
    to_revision: u64,
) -> Result<Mapped, BrowseError> {
    let collection = shared.engine()?.collection()?;
    // H6: generations are compared while the query reader is held; the
    // lookups read the same transaction.
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    // The selected ids of `from`'s mode, with their notes in cards mode.
    let (from_mode, selected, from_nids): (SearchMode, Vec<i64>, Option<HashMap<i64, i64>>) = match (&from, selection) {
        (MapFrom::Handle { handle, revision }, selection) => {
            let handle = shared.handle(*handle)?;
            let handle = lock(&handle);
            check_generation(&handle, &collection)?;
            check_revision(&handle, *revision)?;
            let count = handle.count() as usize;
            let indexes: Vec<usize> = match selection {
                Selection::Ranges(ranges) => {
                    let ranges = merged(&ranges);
                    if ranges.last().is_some_and(|range| range.end > handle.count()) {
                        return Err(BrowseError::OutOfRange);
                    }
                    ranges.iter().flat_map(|range| range.start as usize..range.end as usize).collect()
                }
                Selection::Ids(ids) => {
                    let wanted: HashSet<i64> = ids.into_iter().collect();
                    (0..count).filter(|index| wanted.contains(&handle.ids[*index])).collect()
                }
            };
            let selected: Vec<i64> = indexes.iter().map(|index| handle.ids[*index]).collect();
            let nids = (handle.mode == SearchMode::Cards).then(|| {
                let mut nids: HashMap<i64, i64> =
                    indexes.iter().map(|index| (handle.ids[*index], handle.nids[*index])).collect();
                if let Some(card) = current
                    && let Some(index) = handle.ids[..count].iter().position(|id| *id == card)
                {
                    nids.insert(card, handle.nids[index]);
                }
                nids
            });
            (handle.mode, selected, nids)
        }
        (MapFrom::Mode(mode), Selection::Ids(ids)) => (*mode, ids, None),
        (MapFrom::Mode(_), Selection::Ranges(_)) => {
            return Err(BrowseError::invalid("a released handle's selection is ids, not ranges"));
        }
    };
    let target = shared.handle(to)?;
    let (to_mode, to_ids, to_nids) = {
        let target = lock(&target);
        check_generation(&target, &collection)?;
        check_revision(&target, to_revision)?;
        // H12: over the final prefix of `to_revision` only.
        let count = target.count() as usize;
        (
            target.mode,
            target.ids[..count].to_vec(),
            target.nids.get(..count).map(<[i64]>::to_vec).unwrap_or_default(),
        )
    };
    // Cards from a released handle: their notes, read now.
    let from_nids = match (from_mode, from_nids) {
        (SearchMode::Cards, None) if to_mode == SearchMode::Notes => {
            let mut cards = selected.clone();
            cards.extend(current);
            Some(notes_of_cards_in(&*snapshot, &cards)?)
        }
        (_, nids) => nids,
    };
    let mut index_of: HashMap<i64, u64> = HashMap::new();
    for (index, id) in to_ids.iter().enumerate() {
        index_of.entry(*id).or_insert(index as u64);
    }
    Ok(match (from_mode, to_mode) {
        (SearchMode::Cards, SearchMode::Cards) | (SearchMode::Notes, SearchMode::Notes) => Mapped {
            selected: ranges_of(selected.iter().filter_map(|id| index_of.get(id).copied()).collect()),
            current: current.and_then(|id| index_of.get(&id).copied()),
        },
        (SearchMode::Cards, SearchMode::Notes) => {
            let notes = from_nids.unwrap_or_default();
            Mapped {
                selected: ranges_of(
                    selected
                        .iter()
                        .filter_map(|card| notes.get(card))
                        .filter_map(|note| index_of.get(note).copied())
                        .collect(),
                ),
                current: current
                    .and_then(|card| notes.get(&card))
                    .and_then(|note| index_of.get(note).copied()),
            }
        }
        (SearchMode::Notes, SearchMode::Cards) => {
            let wanted: HashSet<i64> = selected.iter().copied().collect();
            let selected = (0..to_nids.len()).filter(|index| wanted.contains(&to_nids[*index])).map(|index| index as u64).collect();
            // The current note's first card (lowest card id, columns C1)
            // when it is in `to`; otherwise the note's lowest-id card in it.
            let current = match current {
                None => None,
                Some(note) => {
                    let first = cards_of_notes_in(&*snapshot, &[note])?
                        .get(&note)
                        .and_then(|cards| cards.iter().min().copied());
                    match first.and_then(|card| index_of.get(&card).copied()) {
                        Some(index) => Some(index),
                        None => (0..to_nids.len())
                            .filter(|index| to_nids[*index] == note)
                            .min_by_key(|index| to_ids[*index])
                            .map(|index| index as u64),
                    }
                }
            };
            Mapped {
                selected: ranges_of(selected),
                current,
            }
        }
    })
}
