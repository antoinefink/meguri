//! The per-deck-key sums (`stats/history-reads` H14.1) and the parts a page
//! scope reads from them (H2.4): a scope's series is the sum of the series
//! of the keys it selects, never a new replay.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::knowledge::{Frame, Knowledge, Memory, in_cards};
use super::replay::{BadCard, Contribution, Grid, Key};
use super::{CardStatesOverTime, KnowledgeOverTime, KnowledgeSample, StartedPart, StateCounts};
use crate::graphs::History;
use crate::graphs::scope::Unreadable;

/// The changes of one key on one day: the five state counts, then the
/// cards first reviewed that day.
pub(crate) type DayValues = [i64; 6];
/// Where the started count sits in [`DayValues`].
pub(crate) const STARTED: usize = 5;
/// The knowledge of one key at one sample day (H14.1): its cards' summed
/// recall probabilities in 2^-40 cards, then its studied cards.
pub(crate) type SampleValues = [i64; 2];

/// The sums of a set of cards per key: what the component's tables hold,
/// or the change one commit makes to them.
#[derive(Debug, Default)]
pub(crate) struct Sums {
    pub days: HashMap<(Key, i64), DayValues>,
    /// Suspended cards per key (H2.1's `left_out`).
    pub left_out: HashMap<Key, i64>,
    /// H3's kept samples per key and sample day.
    pub knowledge: HashMap<(Key, i64), SampleValues>,
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

    /// Adds `sign` times what a card of `key` adds to the kept samples.
    pub(crate) fn add_knowledge(&mut self, key: Key, knowledge: &Knowledge, sign: i64) {
        for (day, recall) in &knowledge.points {
            let values = self.knowledge.entry((key, *day)).or_default();
            values[0] += sign * recall;
            values[1] += sign;
        }
    }

    /// Adds every sum of `other`.
    pub(crate) fn merge(&mut self, other: Sums) {
        for (at, values) in other.days {
            let sum = self.days.entry(at).or_default();
            for (total, value) in sum.iter_mut().zip(values) {
                *total += value;
            }
        }
        for (key, count) in other.left_out {
            *self.left_out.entry(key).or_default() += count;
        }
        for (at, values) in other.knowledge {
            let sum = self.knowledge.entry(at).or_default();
            sum[0] += values[0];
            sum[1] += values[1];
        }
    }

    /// Drops the entries that sum to nothing.
    pub(crate) fn prune(&mut self) {
        self.days.retain(|_, values| values.iter().any(|v| *v != 0));
        self.left_out.retain(|_, count| *count != 0);
        self.knowledge.retain(|_, values| values.iter().any(|v| *v != 0));
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

/// One card's record as today's point reads it (H3.1): its key, whether it
/// is suspended, its home preset's decay and cutoff, and its memory over
/// every row with and without that cutoff.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NowInputs {
    pub key: Key,
    pub suspended: bool,
    pub decay: f64,
    pub cutoff: Option<i64>,
    pub cut: Memory,
    pub free: Memory,
}

/// H3.3's part, summed from the selected keys' kept samples and records.
#[derive(Debug)]
pub(crate) struct KnowledgeTotals {
    selection: Selection,
    selected: HashMap<Key, bool>,
    days: HashMap<i64, [i128; 2]>,
    now: [i128; 2],
    failed: u64,
    left_out: i64,
}

impl KnowledgeTotals {
    pub(crate) fn new(selection: Selection) -> KnowledgeTotals {
        KnowledgeTotals {
            selection,
            selected: HashMap::new(),
            days: HashMap::new(),
            now: [0; 2],
            failed: 0,
            left_out: 0,
        }
    }

    fn selects(&mut self, key: Key) -> bool {
        let selection = &self.selection;
        *self.selected.entry(key).or_insert_with(|| selection.holds_key(key))
    }

    /// One key's kept sample on one absolute day.
    pub(crate) fn add_day(&mut self, key: Key, day: i64, values: &SampleValues) {
        if self.selects(key) {
            let sum = self.days.entry(day).or_default();
            sum[0] += i128::from(values[0]);
            sum[1] += i128::from(values[1]);
        }
    }

    pub(crate) fn add_left_out(&mut self, key: Key, count: i64) {
        if self.selects(key) {
            self.left_out += count;
        }
    }

    pub(crate) fn add_sums(&mut self, sums: &Sums) {
        for ((key, day), values) in &sums.knowledge {
            self.add_day(*key, *day, values);
        }
        for (key, count) in &sums.left_out {
            self.add_left_out(*key, *count);
        }
    }

    /// One card's part of today's point at `now_ms` (H3.1: the same replay
    /// at now, from the card's record).
    pub(crate) fn add_now(&mut self, card: &NowInputs, now_ms: i64) {
        if card.suspended || !self.selects(card.key) {
            return;
        }
        match Knowledge::now(card.cut, card.free, card.cutoff, now_ms) {
            Memory::Failed => self.failed += 1,
            memory => {
                if let Some(recall) = memory.recall_at(now_ms.div_euclid(1_000), card.decay) {
                    self.now[0] += i128::from(recall);
                    self.now[1] += 1;
                }
            }
        }
    }

    /// The part for a request on `frame` (its `N` is `next_day_at`, its now
    /// `now_ms`), with month samples back to the month of the absolute day
    /// `oldest` when `months` (H3.2).
    pub(crate) fn finish(self, grid: Grid, frame: Frame, now_ms: i64, months: bool, oldest: Option<i64>) -> KnowledgeOverTime {
        let today = frame.latest + 1;
        // Never fewer months than the kept samples hold: the earliest kept
        // day of the selection bounds them too.
        let first = frame.first_daily();
        let earliest = self
            .days
            .iter()
            .filter(|(day, values)| values[1] != 0 && (frame.is_month_end(**day) || **day >= first) && **day <= frame.latest)
            .map(|(day, _)| *day)
            .min();
        let from = match (oldest, earliest) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let days = frame.sample_days(if months { from } else { None });
        let mut samples: Vec<KnowledgeSample> = days
            .into_iter()
            .map(|day| {
                let values = self.days.get(&day).copied().unwrap_or_default();
                KnowledgeSample {
                    day: i32::try_from(day - today).unwrap_or(i32::MIN),
                    instant_ms: grid.end(day).saturating_mul(1_000),
                    knowledge: in_cards(values[0]),
                    studied: u64::try_from(values[1]).unwrap_or(0),
                }
            })
            .collect();
        samples.push(KnowledgeSample {
            day: 0,
            instant_ms: now_ms,
            knowledge: in_cards(self.now[0]),
            studied: u64::try_from(self.now[1]).unwrap_or(0),
        });
        // H3.1's no estimate: no card of the scope gives an item anywhere.
        if samples.iter().all(|sample| sample.studied == 0) {
            samples.clear();
        }
        KnowledgeOverTime {
            samples,
            failed: self.failed,
            left_out: u64::try_from(self.left_out).unwrap_or(0),
        }
    }
}
