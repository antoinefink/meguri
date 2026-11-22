//! The changes after a column base (`stats/graphs-common` G9.3, G9.4): each
//! committed change set's effect on the copies, kept with its commit's seq,
//! and their fold into one patch a reader applies over the base.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::columns::{CardTable, RevlogTable};
use super::rows::{CardRead, RowRead};

/// What one committed change set does to the copies (G9.4), in the order it
/// is applied: `revlog_removed`, `cards` `Removed`, `cards` `Added` and
/// `Updated` (an added card's rows attach to it), `revlog_added`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Change {
    pub revlog_removed: Vec<i64>,
    pub cards_removed: Vec<i64>,
    /// The after-images of added and updated cards.
    pub cards_put: Vec<CardRead>,
    /// Per added card, the ids of its review-log rows as committed.
    pub attach: Vec<(i64, Vec<i64>)>,
    pub revlog_added: Vec<RowRead>,
}

impl Change {
    pub(crate) fn is_empty(&self) -> bool {
        self.revlog_removed.is_empty()
            && self.cards_removed.is_empty()
            && self.cards_put.is_empty()
            && self.attach.is_empty()
            && self.revlog_added.is_empty()
    }
}

/// One overlay entry: a change and the commit it came from.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub generation: u64,
    pub seq: u64,
    pub change: Arc<Change>,
}

/// Changes folded over a base: the cards now (an override, or `None` for a
/// removed card), the base rows no longer read as they are, the rows added
/// after the base with their card, and base rows given another card.
#[derive(Clone, Debug, Default)]
pub(crate) struct Patch {
    pub cards: BTreeMap<i64, Option<CardRead>>,
    /// Base row ids (readable or not) that are removed or replaced.
    pub hidden: HashSet<i64>,
    pub rows: BTreeMap<i64, RowRead>,
    /// Base row id → the card it now belongs to.
    pub attach: HashMap<i64, i64>,
}

impl Patch {
    /// Folds one change, after the earlier ones (G9.4's order).
    pub(crate) fn apply(&mut self, change: &Change) {
        for id in &change.revlog_removed {
            self.rows.remove(id);
            self.attach.remove(id);
            self.hidden.insert(*id);
        }
        for id in &change.cards_removed {
            self.cards.insert(*id, None);
        }
        for card in &change.cards_put {
            self.cards.insert(card.id(), Some(*card));
        }
        // The rows of an added card are those the committed review log
        // lists for it by `cid`, whatever card the copy gave them (a re-key
        // or a restored card, G9.4).
        for (card, rows) in &change.attach {
            for id in rows {
                match self.rows.get_mut(id) {
                    Some(row) => *row = row.with_cid(*card),
                    None => {
                        self.attach.insert(*id, *card);
                    }
                }
            }
        }
        for row in &change.revlog_added {
            self.hidden.insert(row.id());
            self.attach.remove(&row.id());
            self.rows.insert(row.id(), *row);
        }
    }
}

/// The copies as one snapshot sees them: a base and the patch of the
/// changes after it up to the snapshot's seq.
#[derive(Clone, Debug)]
pub(crate) struct View {
    pub cards: Arc<CardTable>,
    pub revlog: Arc<RevlogTable>,
    pub patch: Arc<Patch>,
}

/// A card's place in a view: a base slot, or a card of the patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardKey {
    Slot(usize),
    Patched(i64),
}

impl View {
    /// Where the card `id` is now; `None` for no such card (deleted).
    pub(crate) fn key_of(&self, id: i64) -> Option<CardKey> {
        if self.patch.cards.is_empty() {
            return self.cards.slot_of(id).map(CardKey::Slot);
        }
        match self.patch.cards.get(&id) {
            Some(Some(_)) => Some(CardKey::Patched(id)),
            Some(None) => None,
            None => self.cards.slot_of(id).map(CardKey::Slot),
        }
    }

    /// Every card now, readable or not, in ascending id order.
    pub(crate) fn cards(&self) -> Cards<'_> {
        let cards = &self.cards;
        let patch = &self.patch;
        // The few cards outside the readable base: the unreadable base
        // cards and the patch's cards, merged with the base by id.
        let mut extra: Vec<(CardKey, CardRead)> = (cards.readable()..cards.slots())
            .filter(|slot| !patch.cards.contains_key(&cards.id(*slot)))
            .map(|slot| (CardKey::Slot(slot), cards.read(slot)))
            .chain(
                patch
                    .cards
                    .iter()
                    .filter_map(|(id, card)| card.map(|card| (CardKey::Patched(*id), card))),
            )
            .collect();
        extra.sort_by_key(|(_, card)| card.id());
        Cards {
            view: self,
            slot: 0,
            extra,
            next_extra: 0,
        }
    }

    /// The card a base row belongs to now.
    pub(crate) fn base_row_card(&self, id: i64, slot: Option<usize>) -> Option<CardKey> {
        if self.patch.attach.is_empty() && self.patch.cards.is_empty() {
            return slot.map(CardKey::Slot);
        }
        match self.patch.attach.get(&id) {
            Some(card) => self.key_of(*card),
            None => {
                let slot = slot?;
                let card = self.cards.id(slot);
                if self.patch.cards.contains_key(&card) {
                    self.key_of(card)
                } else {
                    Some(CardKey::Slot(slot))
                }
            }
        }
    }
}

/// [`View::cards`].
pub(crate) struct Cards<'v> {
    view: &'v View,
    slot: usize,
    extra: Vec<(CardKey, CardRead)>,
    next_extra: usize,
}

impl Iterator for Cards<'_> {
    type Item = (CardKey, CardRead);

    fn next(&mut self) -> Option<(CardKey, CardRead)> {
        let cards = &self.view.cards;
        // The next readable base card that the patch does not override.
        while !self.view.patch.cards.is_empty()
            && self.slot < cards.readable()
            && self.view.patch.cards.contains_key(&cards.id(self.slot))
        {
            self.slot += 1;
        }
        let base = (self.slot < cards.readable()).then(|| cards.id(self.slot));
        let extra = self.extra.get(self.next_extra).map(|(_, card)| card.id());
        match (base, extra) {
            (Some(base), Some(extra)) if extra < base => self.take_extra(),
            (Some(_), _) => {
                let slot = self.slot;
                self.slot += 1;
                Some((CardKey::Slot(slot), CardRead::Card(cards.card(slot))))
            }
            (None, Some(_)) => self.take_extra(),
            (None, None) => None,
        }
    }
}

impl Cards<'_> {
    fn take_extra(&mut self) -> Option<(CardKey, CardRead)> {
        let item = self.extra.get(self.next_extra).copied();
        self.next_extra += 1;
        item
    }
}
