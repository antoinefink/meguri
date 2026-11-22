//! By Deck (`stats/history-reads` H10, U291): a table of the page scope's
//! decks, its cards partitioned by home deck so the rows add up, unlike the
//! page's other parts, which follow G2.1's deck rule.

use std::collections::HashMap;

use meguri_engine::decks::tree::Node;
use meguri_model::ids::DeckId;

use super::decks::{DeckFacts, home_deck};
use super::{PERIOD_DAYS, period_start};
use crate::graphs::records::retention::outcome;
use crate::graphs::records::reviews::counted;
use crate::graphs::{History, Scope, ScopeData};

/// One row's values in one period window (H10.2). Member order: `reviews`,
/// `time_ms`, `passed`, `failed`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeckValues {
    /// Its cards' rows that `stats/graphs-revlog` V2 counts.
    pub reviews: u64,
    /// Their summed `time`, in 64 bits.
    pub time_ms: i64,
    /// Its cards' rows of H4.1 (V4.1) answered with anything but Again.
    pub passed: u64,
    /// Those answered Again.
    pub failed: u64,
}

impl DeckValues {
    fn is_empty(&self) -> bool {
        *self == DeckValues::default()
    }
}

/// A row's values per period window (H1). Member order: `one_month`,
/// `three_months`, `six_months`, `one_year`, `all_time`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeckWindows {
    pub one_month: DeckValues,
    pub three_months: DeckValues,
    pub six_months: DeckValues,
    pub one_year: DeckValues,
    /// Every row the request reads; only with `allHistory`.
    pub all_time: Option<DeckValues>,
}

impl DeckWindows {
    /// The windows present, in member order.
    pub fn values(&self) -> impl Iterator<Item = &DeckValues> {
        [&self.one_month, &self.three_months, &self.six_months, &self.one_year]
            .into_iter()
            .chain(self.all_time.as_ref())
    }
}

/// One row of the table (H10.1). Member order: `deck`, `own_cards`,
/// `cards`, `windows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeckRow {
    /// The row's deck: a top-level normal deck (All Decks), or a direct
    /// normal subdeck of the scope's deck, or that deck itself for its own
    /// cards.
    pub deck: DeckId,
    /// The scope deck's row of its own cards: only the cards whose home
    /// deck is that deck, outside its subdecks. It does not filter the page
    /// (`stats/statistics-page` S7.15).
    pub own_cards: bool,
    /// Its existing cards, suspended and buried included; the window plays
    /// no part.
    pub cards: u64,
    pub windows: DeckWindows,
}

/// Part `by_deck`. Member order: `rows`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ByDeckPart {
    /// The rows in the deck tree's order (`sched/counts` C1), the own-cards
    /// row first; a deck with no cards and no reviews in any window is not
    /// a row.
    pub rows: Vec<DeckRow>,
}

/// The decks of a node's subtree, the node included.
fn subtree(node: &Node, out: &mut Vec<i64>) {
    out.push(node.id.0);
    for child in &node.children {
        subtree(child, out);
    }
}

fn find(nodes: &[Node], deck: DeckId) -> Option<&Node> {
    nodes
        .iter()
        .find_map(|node| if node.id == deck { Some(node) } else { find(&node.children, deck) })
}

/// The rows' decks and which row each home deck counts in.
fn layout(scope: &Scope, decks: &DeckFacts) -> (Vec<(DeckId, bool)>, HashMap<i64, usize>) {
    let mut rows: Vec<(DeckId, bool)> = Vec::new();
    let mut of_home: HashMap<i64, usize> = HashMap::new();
    let mut add_subtrees = |nodes: &[Node], rows: &mut Vec<(DeckId, bool)>| {
        for node in nodes.iter().filter(|node| !node.filtered) {
            let mut members = Vec::new();
            subtree(node, &mut members);
            for deck in members {
                of_home.insert(deck, rows.len());
            }
            rows.push((node.id, false));
        }
    };
    match scope {
        Scope::Deck(deck) => {
            if let Some(node) = find(&decks.tree, *deck) {
                // First the deck's own cards, outside its subdecks.
                let mut own = vec![(node.id, true)];
                add_subtrees(&node.children, &mut own);
                of_home.insert(node.id.0, 0);
                rows = own;
            }
        }
        // A Search scope never reaches here (H1: `unsupported_scope`).
        Scope::Collection | Scope::Search(_) => add_subtrees(&decks.tree, &mut rows),
    }
    (rows, of_home)
}

/// H10 over the scope's cards and window rows.
pub(crate) fn build(scope: &ScopeData, page_scope: &Scope, history: History, next_day_at: i64, decks: &DeckFacts) -> ByDeckPart {
    let (layout, of_home) = layout(page_scope, decks);
    let mut rows: Vec<DeckRow> = layout
        .iter()
        .map(|(deck, own_cards)| DeckRow {
            deck: *deck,
            own_cards: *own_cards,
            cards: 0,
            windows: DeckWindows {
                all_time: (history == History::AllHistory).then(DeckValues::default),
                ..DeckWindows::default()
            },
        })
        .collect();
    // Each existing card of the scope in its home deck's row, if any. Under
    // G2.1 the scope holds every card whose home deck is in its tree.
    let mut row_of_card: HashMap<i64, usize> = HashMap::new();
    for card in scope.cards() {
        if let Some(at) = of_home.get(&home_deck(card.did, card.odid)) {
            rows[*at].cards += 1;
            row_of_card.insert(card.id, *at);
        }
    }
    let bounds = PERIOD_DAYS.map(|days| period_start(next_day_at, days));
    for scoped in scope.rows() {
        // Deleted cards' rows are in no row.
        let Some(at) = scoped.card.and_then(|card| row_of_card.get(&card)) else {
            continue;
        };
        let row = scoped.row;
        let reviewed = counted(&row);
        let answered = outcome(&row);
        if !reviewed && answered.is_none() {
            continue;
        }
        let add = |values: &mut DeckValues| {
            if reviewed {
                values.reviews += 1;
                values.time_ms += i64::from(row.time);
            }
            match answered {
                Some(result) if result.passed => values.passed += 1,
                Some(_) => values.failed += 1,
                None => {}
            }
        };
        let windows = &mut rows[*at].windows;
        let timed = [
            &mut windows.one_month,
            &mut windows.three_months,
            &mut windows.six_months,
            &mut windows.one_year,
        ];
        for (values, start) in timed.into_iter().zip(bounds) {
            if row.id >= start {
                add(values);
            }
        }
        if let Some(all) = windows.all_time.as_mut() {
            add(all);
        }
    }
    rows.retain(|row| row.cards > 0 || row.windows.values().any(|values| !values.is_empty()));
    ByDeckPart { rows }
}
