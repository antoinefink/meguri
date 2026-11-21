//! The counts of a Browse selection (`specs/browser/workspace.md` V7, and
//! V8.1's "shown when"): for its target cards (`browser/bulk-actions` B1:
//! every card of a selected note in notes mode) the suspended and buried
//! ones and their home decks; for its target notes their note types. One
//! read transaction on the query reader after the selection resolves; a
//! read never writes.

use std::collections::{BTreeSet, HashSet};

use meguri_model::card::CardQueue;
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_search::SearchMode;
use meguri_search::plan::ids;
use meguri_search::plan::sql::id_list;
use meguri_storage::{ReadRows, ReaderRole};

use super::search::{BrowseError, HandleSelection, cells};
use crate::cancel::CancelToken;
use crate::collection::Collection;
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
    /// The note types of the target notes, ascending.
    pub notetypes: Vec<NotetypeId>,
}

/// The counts of `selection` on `collection` (its handle's collection and
/// generation, `UnknownHandle` otherwise). `cancel` stops it between
/// statements and inside them.
pub fn selection_counts(
    collection: &Collection,
    selection: &HandleSelection,
    cancel: &CancelToken,
) -> Result<SelectionCounts, BrowseError> {
    let cards = selection.resolve_for(collection, SearchMode::Cards)?;
    let notes = selection.resolve_for(collection, SearchMode::Notes)?;
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let stop = || cancel.is_cancelled();
    let mut counts = SelectionCounts::default();
    let mut decks = HashSet::new();
    let mut seen_cards = HashSet::new();
    for chunk in cards.chunks(CHUNK) {
        cancel.check()?;
        snapshot.visit_count_rows(
            ids::HOME_DECKS_AND_QUEUES,
            &cells(&[id_list(chunk)]),
            &stop,
            &mut |row| {
                // A card listed twice (never, from a handle) counts once.
                if seen_cards.insert(row[0]) {
                    counts.cards += 1;
                    decks.insert(row[1]);
                    match CardQueue::from_stored(row[2]) {
                        Some(CardQueue::Suspended) => counts.suspended += 1,
                        Some(CardQueue::BuriedByScheduler | CardQueue::BuriedByUser) => counts.buried += 1,
                        _ => {}
                    }
                }
                true
            },
        )
        .map_err(|error| stopped(cancel, error))?;
    }
    let mut notetypes = BTreeSet::new();
    let mut seen_notes = HashSet::new();
    for chunk in notes.chunks(CHUNK) {
        cancel.check()?;
        snapshot.visit_id_rows(ids::NOTETYPES_OF_NOTES, &cells(&[id_list(chunk)]), &stop, &mut |row| {
            if seen_notes.insert(row[0]) {
                counts.notes += 1;
                notetypes.insert(row[1]);
            }
            true
        })
        .map_err(|error| stopped(cancel, error))?;
    }
    cancel.check()?;
    counts.decks = decks.len() as u64;
    counts.deck = match decks.len() {
        1 => decks.iter().next().copied().map(DeckId),
        _ => None,
    };
    counts.notetypes = notetypes.into_iter().map(NotetypeId).collect();
    Ok(counts)
}

/// A statement the token interrupted ends `Cancelled`, not as the database
/// refusal the interrupt gives.
fn stopped(cancel: &CancelToken, error: meguri_storage::StorageError) -> BrowseError {
    if cancel.is_cancelled() {
        BrowseError::Engine(EngineError::Cancelled)
    } else {
        error.into()
    }
}
