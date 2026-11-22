//! Scopes and history (`stats/graphs-common` G2, G3): which cards and
//! review-log rows a request reads, resolved once per request over a view
//! of the column copies, and the iteration the graphs compute from.

use std::collections::HashSet;

use meguri_model::deck::Deck;
use meguri_model::ids::DeckId;

use super::overlay::{CardKey, View};
use super::rows::{BadColumn, CardRead, RowRead, StatsCard, StatsRow};

/// A request's scope, passed typed to the core (G2, R1350).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Every card, and every review-log row, deleted cards' rows included.
    Collection,
    /// The deck, its subdecks and the filtered-deck cards whose home deck is
    /// among them (G2.1).
    Deck(DeckId),
    /// The cards the search matches in cards mode, as given (G2). A text of
    /// White_Space only is the Collection scope.
    Search(String),
}

impl Scope {
    /// G2: a Search text that is empty or holds only White_Space is the
    /// Collection scope.
    pub(crate) fn effective(&self) -> &Scope {
        match self {
            Scope::Search(text) if text.chars().all(char::is_whitespace) => &Scope::Collection,
            other => other,
        }
    }
}

/// The history control (G3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum History {
    /// Rows with `id ≥ (N − 366 × 86,400) × 1,000`.
    Last12Months,
    /// Rows with `id ≥ 0`.
    AllHistory,
}

impl History {
    /// The smallest review-log id the window reads; it has no upper end.
    pub fn window_start(self, next_day_at: i64) -> i64 {
        match self {
            History::Last12Months => next_day_at.saturating_sub(366 * 86_400).saturating_mul(1000).max(0),
            History::AllHistory => 0,
        }
    }
}

/// The decks of a Deck scope (G2.1): `deck` and every deck whose stored
/// name starts with its stored name and U+001F, compared under simple case
/// folding as `deck:` compares them. `None` when `deck` names no deck.
pub(crate) fn deck_set(decks: &[Deck], deck: DeckId) -> Option<HashSet<i64>> {
    let root = decks.iter().find(|d| d.id == deck)?;
    let folded = meguri_search::simple_fold(&root.name);
    let mut prefix = folded.clone();
    prefix.push('\u{1f}');
    Some(
        decks
            .iter()
            .filter(|d| {
                let name = meguri_search::simple_fold(&d.name);
                name == folded || name.starts_with(&prefix)
            })
            .map(|d| d.id.0)
            .collect(),
    )
}

/// Which cards a scope holds.
#[derive(Clone, Debug)]
pub(crate) enum Membership {
    All,
    /// G2.1: `did` in the set, or a non-zero `odid` in it.
    Decks(HashSet<i64>),
    /// A search's cards.
    Cards(HashSet<i64>),
}

impl Membership {
    fn holds(&self, card: &CardRead) -> bool {
        let (did, odid) = card.decks();
        self.holds_parts(card.id(), did, odid)
    }

    fn holds_parts(&self, id: i64, did: Option<i64>, odid: Option<i64>) -> bool {
        match self {
            Membership::All => true,
            Membership::Decks(decks) => {
                did.is_some_and(|did| decks.contains(&did))
                    || odid.is_some_and(|odid| odid != 0 && decks.contains(&odid))
            }
            Membership::Cards(ids) => ids.contains(&id),
        }
    }
}

/// A row that makes the request fail (G4, G8.2): its table, column and key
/// (`model/row-records` R10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub table: &'static str,
    pub column: &'static str,
    pub key: i64,
}

impl From<(BadColumn, i64)> for Unreadable {
    fn from((column, key): (BadColumn, i64)) -> Unreadable {
        Unreadable {
            table: column.table(),
            column: column.column(),
            key,
        }
    }
}

/// A row of the scope, with the card it belongs to now (none for a deleted
/// card's row, which only the Collection scope reads).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeRow {
    pub row: StatsRow,
    pub card: Option<i64>,
}

/// The resolved scope of one request (G8.3's memo): the scope's cards and
/// its rows in the history window, over one view of the copies.
#[derive(Debug)]
pub struct ScopeData {
    view: View,
    /// The Collection scope: every row counts, deleted cards' included.
    collection: bool,
    /// Base slots in the scope, by their card as it is now.
    slots: Vec<bool>,
    /// The patch's cards in the scope.
    patched: HashSet<i64>,
    window_start: i64,
    first_row: usize,
    from_caches: bool,
}

impl ScopeData {
    pub(crate) fn new(view: View, membership: &Membership, window_start: i64, from_caches: bool) -> ScopeData {
        let cards = &view.cards;
        let patch = &view.patch;
        let slots = (0..cards.slots())
            .map(|slot| {
                let id = cards.id(slot);
                if patch.cards.contains_key(&id) {
                    return false;
                }
                let (did, odid) = cards.decks(slot);
                membership.holds_parts(id, did, odid)
            })
            .collect();
        let patched = patch
            .cards
            .iter()
            .filter_map(|(id, card)| card.filter(|card| membership.holds(card)).map(|_| *id))
            .collect();
        let first_row = view.revlog.lower_bound(window_start);
        ScopeData {
            collection: matches!(membership, Membership::All),
            view,
            slots,
            patched,
            window_start,
            first_row,
            from_caches,
        }
    }

    fn selected(&self, key: Option<CardKey>) -> bool {
        match key {
            Some(CardKey::Slot(slot)) => self.slots[slot],
            Some(CardKey::Patched(id)) => self.patched.contains(&id),
            None => false,
        }
    }

    fn card_id(&self, key: Option<CardKey>) -> Option<i64> {
        match key? {
            CardKey::Slot(slot) => Some(self.view.cards.id(slot)),
            CardKey::Patched(id) => Some(id),
        }
    }

    fn row_counts(&self, key: Option<CardKey>) -> bool {
        self.collection || self.selected(key)
    }

    /// The first unreadable card of the scope, else the first unreadable row
    /// of its window (G4): either fails the whole request.
    pub(crate) fn first_unreadable(&self) -> Option<Unreadable> {
        let cards = &self.view.cards;
        let mut bad_cards: Vec<(i64, BadColumn)> = (cards.readable()..cards.slots())
            .filter(|slot| self.slots[*slot])
            .filter_map(|slot| match cards.read(slot) {
                CardRead::Bad(bad) => Some((bad.id, bad.column)),
                CardRead::Card(_) => None,
            })
            .chain(self.view.patch.cards.iter().filter_map(|(id, card)| match card {
                Some(CardRead::Bad(bad)) if self.patched.contains(id) => Some((bad.id, bad.column)),
                _ => None,
            }))
            .collect();
        bad_cards.sort_unstable();
        if let Some((id, column)) = bad_cards.first() {
            return Some((*column, *id).into());
        }
        let patch = &self.view.patch;
        let mut bad_rows: Vec<(i64, BadColumn)> = self
            .view
            .revlog
            .bad()
            .iter()
            .filter(|bad| bad.id >= self.window_start && !patch.hidden.contains(&bad.id))
            .filter(|bad| {
                let card = patch.attach.get(&bad.id).copied().or(bad.cid);
                self.row_counts(card.and_then(|card| self.view.key_of(card)))
            })
            .map(|bad| (bad.id, bad.column))
            .chain(patch.rows.range(self.window_start..).filter_map(|(id, row)| match row {
                RowRead::Bad(bad) if self.row_counts(bad.cid.and_then(|card| self.view.key_of(card))) => {
                    Some((*id, bad.column))
                }
                _ => None,
            }))
            .collect();
        bad_rows.sort_unstable();
        bad_rows.first().map(|(id, column)| (*column, *id).into())
    }

    /// The scope's readable cards, in ascending id order (a request with an
    /// unreadable one never computes, G4).
    pub fn cards(&self) -> impl Iterator<Item = StatsCard> + '_ {
        self.view.cards().filter_map(move |(key, card)| match card {
            CardRead::Card(card) if self.selected(Some(key)) => Some(card),
            _ => None,
        })
    }

    /// The scope's readable review-log rows in the history window, in
    /// ascending id order, each with its card now.
    pub fn rows(&self) -> Rows<'_> {
        Rows {
            data: self,
            next: self.first_row,
            patched: self.view.patch.rows.range(self.window_start..).peekable(),
        }
    }

    /// The window's first id (G3).
    pub fn window_start(&self) -> i64 {
        self.window_start
    }

    /// Whether the copies came from the caches (else from the snapshot).
    pub fn from_caches(&self) -> bool {
        self.from_caches
    }
}

/// [`ScopeData::rows`].
pub struct Rows<'d> {
    data: &'d ScopeData,
    next: usize,
    patched: std::iter::Peekable<std::collections::btree_map::Range<'d, i64, RowRead>>,
}

impl Iterator for Rows<'_> {
    type Item = ScopeRow;

    fn next(&mut self) -> Option<ScopeRow> {
        let data = self.data;
        let revlog = &data.view.revlog;
        let patch = &data.view.patch;
        loop {
            // The next base row that is neither removed nor replaced.
            while !patch.hidden.is_empty() && self.next < revlog.len() && patch.hidden.contains(&revlog.id(self.next)) {
                self.next += 1;
            }
            let base = (self.next < revlog.len()).then(|| revlog.id(self.next));
            let patched = self.patched.peek().map(|(id, _)| **id);
            let (row, card) = match (base, patched) {
                (None, None) => return None,
                (Some(base), Some(patched)) if patched < base => self.take_patched()?,
                (None, Some(_)) => self.take_patched()?,
                (Some(id), _) => {
                    let index = self.next;
                    self.next += 1;
                    // The card first: a row outside the scope is never
                    // decoded.
                    let card = data.view.base_row_card(id, revlog.slot(index));
                    if !data.row_counts(card) {
                        continue;
                    }
                    (Some(revlog.get(index).row), card)
                }
            };
            if let Some(row) = row
                && data.row_counts(card)
            {
                return Some(ScopeRow {
                    row,
                    card: data.card_id(card),
                });
            }
        }
    }
}

impl Rows<'_> {
    /// The next patch row: its readable row (none when unreadable) and card.
    fn take_patched(&mut self) -> Option<(Option<StatsRow>, Option<CardKey>)> {
        let (_, row) = self.patched.next()?;
        let card = row.cid().and_then(|card| self.data.view.key_of(card));
        Some((
            match row {
                RowRead::Row { row, .. } => Some(*row),
                RowRead::Bad(_) => None,
            },
            card,
        ))
    }
}
