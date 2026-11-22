//! The counts of a Browse selection (`specs/browser/workspace.md` V7, and
//! V8.1's "shown when"): for its target cards (`browser/bulk-actions` B1:
//! every card of a selected note in notes mode) the suspended and buried
//! ones and their home decks; for its target notes their note types.
//!
//! It scales with the selection, so it reads on a search worker's reader
//! taken through the search registry, never the query reader that row pages
//! use (`engine/jobs` J2.1.1): a replacement, a suspend or a close cancels
//! it as it cancels a search, and so does the caller's token. The handle's
//! ids are copied in memory; everything else is read in one snapshot. A
//! read never writes.

use std::collections::{BTreeSet, HashSet};

use meguri_model::card::CardQueue;
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_search::SearchMode;
use meguri_search::plan::ids;
use meguri_search::plan::sql::id_list;
use meguri_storage::ReadRows;

use super::registry::bridge;
use super::selection::merged;
use super::{BrowseError, HandleSelection, check_revision, lock, worker};
use crate::cancel::CancelToken;
use crate::error::EngineError;

/// Ids bound in one statement.
const CHUNK: usize = 10_000;

/// What the inspector's summary line and the commands' "shown when" read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionCounts {
    /// The target cards that exist.
    pub cards: u64,
    /// The target notes that exist.
    pub notes: u64,
    /// Target cards in the suspended queue.
    pub suspended: u64,
    /// Target cards buried by the scheduler or by the user.
    pub buried: u64,
    /// The distinct home decks of the target cards: a filtered card's
    /// original deck, any other card's deck.
    pub decks: u64,
    /// That deck when there is exactly one.
    pub deck: Option<DeckId>,
    /// The note types of the target notes that exist, ascending.
    pub notetypes: Vec<NotetypeId>,
}

/// The counts of `selection`, in one snapshot of a search worker's reader.
/// Refused as `resolve_selection` refuses (`UnknownHandle`,
/// `StaleRevision`, `OutOfRange`); `Cancelled` once `cancel` or a stop
/// cancels it.
pub fn selection_counts(selection: &HandleSelection, cancel: &CancelToken) -> Result<SelectionCounts, BrowseError> {
    let cancelled = || BrowseError::Engine(EngineError::Cancelled);
    let shared = &selection.shared;
    // A released handle is unknown, whatever else is unavailable (H6).
    shared.handle(selection.handle)?;
    let engine = shared.engine()?;
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
    let (mode, selected, notes_of_cards) = {
        let handle = shared.handle(selection.handle)?;
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
        let selected: Vec<i64> = indexes.clone().map(|index| handle.ids[index]).collect();
        let notes: Vec<i64> = match handle.mode {
            SearchMode::Cards => indexes.map(|index| handle.nids[index]).collect(),
            SearchMode::Notes => Vec::new(),
        };
        (handle.mode, selected, notes)
    };
    // B1: the target cards are the selected cards, or every card of the
    // selected notes; the target notes the selected notes, or the notes of
    // the selected cards.
    let (cards_sql, card_keys, notes) = match mode {
        SearchMode::Cards => (ids::HOME_DECKS_AND_QUEUES, selected, notes_of_cards),
        SearchMode::Notes => (ids::HOME_DECKS_AND_QUEUES_OF_NOTES, selected.clone(), selected),
    };
    let stop = || token.is_cancelled();
    let stopped = |error: meguri_storage::StorageError| {
        if token.is_cancelled() { cancelled() } else { BrowseError::from(error) }
    };
    let mut counts = SelectionCounts::default();
    let mut decks = HashSet::new();
    let mut seen = HashSet::new();
    for chunk in card_keys.chunks(CHUNK) {
        if token.is_cancelled() {
            return Err(cancelled());
        }
        snapshot
            .visit_count_rows(cards_sql, &worker::cells(&[id_list(chunk)]), &stop, &mut |row| {
                // A card counts once, whatever lists it twice.
                if seen.insert(row[0]) {
                    counts.cards += 1;
                    decks.insert(row[1]);
                    match CardQueue::from_stored(row[2]) {
                        Some(CardQueue::Suspended) => counts.suspended += 1,
                        Some(CardQueue::BuriedByScheduler | CardQueue::BuriedByUser) => counts.buried += 1,
                        _ => {}
                    }
                }
                true
            })
            .map_err(stopped)?;
    }
    let mut notetypes = BTreeSet::new();
    let mut seen = HashSet::new();
    for chunk in notes.chunks(CHUNK) {
        if token.is_cancelled() {
            return Err(cancelled());
        }
        snapshot
            .visit_id_rows(ids::NOTETYPES_OF_NOTES, &worker::cells(&[id_list(chunk)]), &stop, &mut |row| {
                if seen.insert(row[0]) {
                    counts.notes += 1;
                    notetypes.insert(row[1]);
                }
                true
            })
            .map_err(stopped)?;
    }
    if token.is_cancelled() {
        return Err(cancelled());
    }
    counts.decks = decks.len() as u64;
    counts.deck = match decks.len() {
        1 => decks.iter().next().copied().map(DeckId),
        _ => None,
    };
    counts.notetypes = notetypes.into_iter().map(NotetypeId).collect();
    Ok(counts)
}
