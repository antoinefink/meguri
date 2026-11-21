//! Result handles (`browser/result-sets` H6): the ordered ids of a request
//! and what the core needs to page, reverse, map and invalidate them, with
//! the index arithmetic of H8's reverse and H10's change sets.

use std::collections::HashSet;

use meguri_model::changes::{ChangeKind, ChangeSet};
use meguri_search::{ContextReads, SearchContext, SearchMode, Sort};

use crate::changes::stream::Seq;
use crate::lifecycle::Generation;

/// A handle (H6). Cards mode keeps each card's note id beside it.
#[derive(Debug)]
pub(super) struct Handle {
    pub(super) mode: SearchMode,
    pub(super) sort: Sort,
    pub(super) text: String,
    /// The context of H9 the predicate read, kept by resorts.
    pub(super) context: SearchContext,
    /// The context values the plan read (`search/query-model` Q8).
    pub(super) reads: ContextReads,
    /// The now and today the sort keys read: the context's, or after a
    /// resort the resort's own (H8). Only the conformance hook reads it in
    /// this unit: B2's runs from the handle's own keys.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    pub(super) sort_clock: SearchContext,
    /// The final prefix while the request runs, the whole list after.
    pub(super) ids: Vec<i64>,
    /// Cards mode: `nids[i]` is the note id of card `ids[i]`; empty in
    /// notes mode.
    pub(super) nids: Vec<i64>,
    /// Cards mode: matched cards without a note row that S9 leaves out of
    /// the list under the handle's sort, with their `nid` (H5).
    pub(super) set_aside: Vec<(i64, i64)>,
    pub(super) match_seq: Seq,
    pub(super) applied_seq: Seq,
    pub(super) generation: Generation,
    pub(super) revision: u64,
    /// H11: notes mode with card or card-and-note scope, the cards of the
    /// matching card-and-note pairs (8 bytes each, H6), when the request
    /// kept them.
    pub(super) match_data: Option<Vec<i64>>,
    pub(super) complete: bool,
    /// H10: ids of the handle's mode listed `Removed` while the request
    /// runs; the worker leaves them out of the rows it confirms later.
    pub(super) removed_while_running: HashSet<i64>,
    /// While in progress, the final prefix's length the caller was told
    /// (`FirstPage`, then `Progress`): rows appended beyond it are not
    /// addressable until announced (H6).
    pub(super) prefix: usize,
}

/// What [`Handle::invalidate`] says to refetch (H10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refetch {
    /// Indexes of the new revision, ascending.
    Rows(Vec<u64>),
    /// Every row: a note type or a deck changed, or the whole collection.
    All,
}

/// The result of `invalidated_rows` (H10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalidation {
    /// Indexes in the revision the call named, ascending.
    pub removed: Vec<u64>,
    pub refetch: Refetch,
    pub revision: u64,
    pub complete: bool,
    /// The final prefix's length while in progress, the whole count after.
    pub count: u64,
}

impl Handle {
    /// The final prefix's length while in progress, the whole count after.
    pub(super) fn count(&self) -> u64 {
        if self.complete {
            self.ids.len() as u64
        } else {
            self.prefix.min(self.ids.len()) as u64
        }
    }

    /// H8: the list in the other direction; index i becomes count − 1 − i.
    pub(super) fn reverse(&mut self) -> u64 {
        self.ids.reverse();
        self.nids.reverse();
        self.sort = self.sort.reversed();
        self.revision += 1;
        self.revision
    }

    /// H10 for the change sets after the applied seq, in seq order.
    pub(super) fn invalidate(&mut self, change_sets: &[(Seq, &ChangeSet)]) -> Invalidation {
        let mut removed_ids: HashSet<i64> = HashSet::new();
        let mut removed_cards: HashSet<i64> = HashSet::new();
        let mut updated_cards: HashSet<i64> = HashSet::new();
        let mut updated_notes: HashSet<i64> = HashSet::new();
        let mut notes_of_cards: HashSet<i64> = HashSet::new();
        let mut all = false;
        for (seq, changes) in change_sets {
            if *seq <= self.applied_seq {
                continue;
            }
            self.applied_seq = *seq;
            all |= changes.whole_collection || !changes.notetypes.is_empty() || !changes.decks.is_empty();
            for card in &changes.cards {
                notes_of_cards.insert(card.note_id.0);
                match card.kind {
                    ChangeKind::Removed => {
                        removed_cards.insert(card.id.0);
                        if self.mode == SearchMode::Cards {
                            removed_ids.insert(card.id.0);
                        }
                    }
                    ChangeKind::Updated => {
                        updated_cards.insert(card.id.0);
                    }
                    ChangeKind::Added => {}
                }
            }
            for note in &changes.notes {
                match note.kind {
                    ChangeKind::Removed if self.mode == SearchMode::Notes => {
                        removed_ids.insert(note.id.0);
                    }
                    ChangeKind::Updated => {
                        updated_notes.insert(note.id.0);
                    }
                    _ => {}
                }
            }
        }
        if !removed_cards.is_empty() {
            self.set_aside.retain(|(card, _)| !removed_cards.contains(card));
        }
        let mut removed = Vec::new();
        if !removed_ids.is_empty() {
            if !self.complete {
                self.removed_while_running.extend(removed_ids.iter().copied());
            }
            // Indexes in the revision the call named: only the announced
            // prefix is addressable while the request runs.
            let addressable = self.count() as usize;
            removed = (0..addressable)
                .filter(|&index| removed_ids.contains(&self.ids[index]))
                .map(|index| index as u64)
                .collect();
            let keep = |index: usize| !removed_ids.contains(&self.ids[index]);
            let kept: Vec<usize> = (0..self.ids.len()).filter(|&index| keep(index)).collect();
            if kept.len() != self.ids.len() {
                self.prefix -= removed.len().min(self.prefix);
                self.ids = kept.iter().map(|&index| self.ids[index]).collect();
                if !self.nids.is_empty() {
                    self.nids = kept.iter().map(|&index| self.nids[index]).collect();
                }
            }
            if !removed.is_empty() {
                self.revision += 1;
            }
        }
        let refetch = if all {
            Refetch::All
        } else {
            Refetch::Rows(
                (0..self.count() as usize)
                    .filter(|&index| match self.mode {
                        SearchMode::Cards => {
                            updated_cards.contains(&self.ids[index]) || updated_notes.contains(&self.nids[index])
                        }
                        SearchMode::Notes => {
                            updated_notes.contains(&self.ids[index]) || notes_of_cards.contains(&self.ids[index])
                        }
                    })
                    .map(|index| index as u64)
                    .collect(),
            )
        };
        Invalidation {
            removed,
            refetch,
            revision: self.revision,
            complete: self.complete,
            count: self.count(),
        }
    }
}
