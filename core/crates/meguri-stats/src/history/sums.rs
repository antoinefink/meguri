//! The per-deck-key sums (`stats/history-reads` H14.1) and the parts a page
//! scope reads from them (H2.4): a scope's series is the sum of the series
//! of the keys it selects, never a new replay.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::replay::{BadCard, Contribution, Grid, Key};
use super::{CardStatesOverTime, StartedPart, StateCounts};
use crate::graphs::History;
use crate::graphs::scope::Unreadable;

/// The changes of one key on one day: the five state counts, then the
/// cards first reviewed that day.
pub(crate) type DayValues = [i64; 6];
/// Where the started count sits in [`DayValues`].
pub(crate) const STARTED: usize = 5;

/// The sums of a set of cards per key: what the component's tables hold,
/// or the change one commit makes to them.
#[derive(Debug, Default)]
pub(crate) struct Sums {
    pub days: HashMap<(Key, i64), DayValues>,
    /// Suspended cards per key (H2.1's `left_out`).
    pub left_out: HashMap<Key, i64>,
}

impl Sums {
    /// Adds `sign` times what `contribution` adds; an unreadable card adds
    /// nothing to the sums.
    pub(crate) fn add(&mut self, contribution: &Contribution, sign: i64) {
        let Contribution::Card(card) = contribution else { return };
        let key = card.record.key;
        if card.record.suspended {
            *self.left_out.entry(key).or_default() += sign;
        } else {
            let mut before = None;
            for (day, state) in &card.segments {
                let values = self.days.entry((key, *day)).or_default();
                values[state.index()] += sign;
                if let Some(previous) = before {
                    values[previous] -= sign;
                }
                before = Some(state.index());
            }
        }
        // Started keeps suspended cards (H2.3).
        if let Some(day) = card.record.first_review {
            self.days.entry((key, day)).or_default()[STARTED] += sign;
        }
    }

    /// Drops the entries that sum to nothing.
    pub(crate) fn prune(&mut self) {
        self.days.retain(|_, values| values.iter().any(|v| *v != 0));
        self.left_out.retain(|_, count| *count != 0);
    }
}

/// Which deck keys a page scope selects (H1): every key, or the keys whose
/// `did` is in the set or whose non-zero `odid` is (`stats/graphs-common`
/// G2.1).
#[derive(Clone, Debug)]
pub(crate) enum Selection {
    All,
    Decks(HashSet<i64>),
}

impl Selection {
    pub(crate) fn holds(&self, did: Option<i64>, odid: Option<i64>) -> bool {
        match self {
            Selection::All => true,
            Selection::Decks(decks) => {
                did.is_some_and(|did| decks.contains(&did)) || odid.is_some_and(|odid| odid != 0 && decks.contains(&odid))
            }
        }
    }

    fn holds_key(&self, key: Key) -> bool {
        self.holds(Some(key.did), Some(key.odid))
    }
}

/// The first unreadable card of a scope, else its first unreadable row, as
/// `stats/graphs-common` G4 orders them.
#[derive(Debug, Default)]
pub(crate) struct FirstBad {
    card: Option<BadCard>,
    row: Option<BadCard>,
}

impl FirstBad {
    pub(crate) fn offer(&mut self, selection: &Selection, bad: BadCard) {
        if !selection.holds(bad.did, bad.odid) {
            return;
        }
        match bad.row {
            None => {
                if self.card.is_none_or(|first| bad.card < first.card) {
                    self.card = Some(bad);
                }
            }
            Some(row) => {
                if self.row.is_none_or(|first| first.row.is_none_or(|first| row < first)) {
                    self.row = Some(bad);
                }
            }
        }
    }

    pub(crate) fn first(&self) -> Option<Unreadable> {
        let bad = self.card.or(self.row)?;
        Some(Unreadable::from((bad.column, bad.row.unwrap_or(bad.card))))
    }
}

/// H2.4's two parts, summed from the selected keys' rows as they come.
#[derive(Debug)]
pub(crate) struct Totals {
    grid: Grid,
    next_day_at: i64,
    history: History,
    selection: Selection,
    selected: HashMap<Key, bool>,
    start: [i64; 5],
    deltas: BTreeMap<i32, [i64; 5]>,
    started: BTreeMap<i32, i64>,
    first_state_day: Option<i32>,
    first_started_day: Option<i32>,
    left_out: i64,
}

/// `last12Months` reads days −366 to 0 (H1's window).
const WINDOW_FIRST_DAY: i32 = -366;

impl Totals {
    pub(crate) fn new(grid: Grid, next_day_at: i64, history: History, selection: Selection) -> Totals {
        Totals {
            grid,
            next_day_at,
            history,
            selection,
            selected: HashMap::new(),
            start: [0; 5],
            deltas: BTreeMap::new(),
            started: BTreeMap::new(),
            first_state_day: None,
            first_started_day: None,
            left_out: 0,
        }
    }

    fn selects(&mut self, key: Key) -> bool {
        let selection = &self.selection;
        *self.selected.entry(key).or_insert_with(|| selection.holds_key(key))
    }

    /// One key's changes on one absolute day.
    pub(crate) fn add_day(&mut self, key: Key, day: i64, values: &DayValues) {
        if !self.selects(key) {
            return;
        }
        let relative = self.grid.relative(day, self.next_day_at);
        let states: [i64; 5] = [values[0], values[1], values[2], values[3], values[4]];
        if states.iter().any(|v| *v != 0) {
            // A key's oldest day only adds cards: it is where its first
            // card enters.
            self.first_state_day = Some(self.first_state_day.map_or(relative, |first| first.min(relative)));
            let before_window = self.history == History::Last12Months && relative < WINDOW_FIRST_DAY;
            let target = if before_window {
                &mut self.start
            } else {
                self.deltas.entry(relative).or_default()
            };
            for (total, value) in target.iter_mut().zip(states) {
                *total += value;
            }
        }
        let started = values[super::sums::STARTED];
        if started != 0 {
            self.first_started_day = Some(self.first_started_day.map_or(relative, |first| first.min(relative)));
            if self.history == History::AllHistory || relative >= WINDOW_FIRST_DAY {
                *self.started.entry(relative).or_default() += started;
            }
        }
    }

    pub(crate) fn add_left_out(&mut self, key: Key, count: i64) {
        if self.selects(key) {
            self.left_out += count;
        }
    }

    pub(crate) fn add_sums(&mut self, sums: &Sums) {
        for ((key, day), values) in &sums.days {
            self.add_day(*key, *day, values);
        }
        for (key, count) in &sums.left_out {
            self.add_left_out(*key, *count);
        }
    }

    pub(crate) fn finish(self) -> (CardStatesOverTime, StartedPart) {
        let deltas = self
            .deltas
            .into_iter()
            .filter(|(_, values)| values.iter().any(|v| *v != 0))
            .map(|(day, values)| (day, StateCounts::from(values)))
            .collect();
        let started = self
            .started
            .into_iter()
            .filter(|(_, count)| *count != 0)
            .map(|(day, count)| (day, u64::try_from(count).unwrap_or(0)))
            .collect();
        (
            CardStatesOverTime {
                start: StateCounts::from(self.start),
                deltas,
                left_out: u64::try_from(self.left_out).unwrap_or(0),
                first_day: self.first_state_day,
            },
            StartedPart {
                days: started,
                first_day: self.first_started_day,
            },
        )
    }
}
